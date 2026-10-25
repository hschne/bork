//! Windowless adapter: observe input, advance work debt, and publish display intent.
use crate::{
    clock::ClockSample,
    config::{self, Settings, Timing},
    control::{Command, Phase, State, Status},
    effect::{Glitchiness, GraceCurve},
    hyprland::Hyprland,
    recovery::{DisplayState, Guardian},
};
use anyhow::Result;

mod commands;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub fn run() -> Result<()> {
    let hypr = Hyprland::from_env()?;
    hypr.check()?;
    // Validate protocols before acquiring display ownership.
    let mut input = crate::wayland::check()?;
    let seed = crate::effect::fresh_seed()?;
    let mut settings = config::LiveSettings::open(config::path()?)?;
    let controls = commands::Server::bind(&hypr.runtime_directory()?)?;
    let mut guardian = Guardian::spawn(&hypr)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopping);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let epoch = ClockSample::now();
    let mut previous_clock = epoch;
    let mut timer = Timer::new(epoch.seconds());
    let result = (|| -> Result<()> {
        while !stopping.load(Ordering::Relaxed) {
            input.dispatch()?;
            if settings.reload(Instant::now())? {
                timer.published = None;
            }
            let locked = hypr.locked()?;
            let clock = ClockSample::now();
            let now = clock.seconds();
            let activity = input.activity();
            let observation = Observation {
                now,
                suspended: clock.suspended_since(&previous_clock),
                locked,
                resumed: activity.resumed,
                idle_since: activity.idle_since,
            };
            previous_clock = clock;

            let pending = controls.poll()?;
            let action = (|| -> Result<Status> {
                let tuning = settings.current();
                let frame = timer.update(
                    observation,
                    tuning,
                    pending.as_ref().map(|request| request.command),
                );
                match frame.display {
                    Some(DisplayUpdate::Reset) => guardian.reset()?,
                    Some(DisplayUpdate::Publish(next)) => {
                        let state = match next {
                            Publication::Work => DisplayState::Work,
                            Publication::Interference(level) => DisplayState::Interference {
                                shader: tuning.interference.hyprland_shader_at(
                                    level,
                                    clock.elapsed_since(epoch),
                                    seed,
                                )?,
                            },
                            Publication::TestCard(remaining) => DisplayState::TestCard {
                                remaining,
                                total: u64::from(tuning.timing.break_seconds.get()),
                                appearance: tuning.overtime.clone(),
                            },
                        };
                        guardian.set_display(state)?;
                    }
                    None => (),
                }
                guardian.heartbeat()?;
                Ok(frame.status)
            })();
            if let Some(request) = pending {
                request.respond(&action);
            }
            action?;
            thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    })();
    let cleanup = guardian.finish();
    if result.as_ref().is_err_and(crate::recovery::is_stopped) {
        cleanup
    } else {
        match (result, cleanup) {
            (Ok(()), result) | (result, Ok(())) => result,
            (Err(error), Err(cleanup)) => {
                Err(error.context(format!("Guardian cleanup also failed: {cleanup:#}")))
            }
        }
    }
}

/// Publication cache, not installed display state. The guardian alone owns installation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Publication {
    Work,
    Interference(Glitchiness),
    TestCard(u64),
}

impl Publication {
    fn for_phase(phase: DesiredPhase, settings: &Settings) -> Self {
        match phase {
            DesiredPhase::Work => Self::Work,
            DesiredPhase::Interference(level) => {
                // Half-percent steps bound recompilation without linearizing grace.
                let level = level.quantized();
                if settings.interference.has_visible_effect_at(level) {
                    Self::Interference(level)
                } else {
                    Self::Work
                }
            }
            DesiredPhase::TestCard { remaining } => Self::TestCard(remaining),
        }
    }
}

// Break-cycle policy, independent of clocks, input protocols, and rendering.
// Time values are boot-time seconds, so a suspend is ordinary input-free time.

#[derive(Clone, Copy, Debug, PartialEq)]
enum DesiredPhase {
    Work,
    Interference(Glitchiness),
    TestCard { remaining: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Running,
    Paused,
    Resting,
}

struct Snapshot {
    mode: Mode,
    phase: DesiredPhase,
    worked: u64,
    phase_remaining: u64,
    break_remaining: u64,
    resting_seconds: u64,
}

impl Snapshot {
    fn display_phase(&self) -> DesiredPhase {
        if self.mode == Mode::Running {
            self.phase
        } else {
            DesiredPhase::Work
        }
    }
}

/// Owns work debt and the evidence needed to complete a continuous break.
struct Timer {
    state: CycleState,
    timing: Timing,
    locked: bool,
    idle_since: Option<u64>,
    idle_floor: u64,
    paused_at: Option<u64>,
    published: Option<Publication>,
}

/// Input evidence for one boot-time sample, independent of compositor connections.
#[derive(Default)]
struct Observation {
    now: u64,
    suspended: u64,
    locked: bool,
    resumed: bool,
    idle_since: Option<u64>,
}

struct Frame {
    status: Status,
    display: Option<DisplayUpdate>,
}

#[derive(Debug, PartialEq)]
enum DisplayUpdate {
    Reset,
    Publish(Publication),
}

enum CycleState {
    Working { started_at: u64 },
    Resting { since: u64 },
}

impl Timer {
    /// One tick produces both status and display intent from the same accounting
    /// snapshot. Reset/pause must also cancel a guardian-owned desktop trial.
    fn update(
        &mut self,
        observation: Observation,
        settings: &Settings,
        command: Option<Command>,
    ) -> Frame {
        let Observation {
            now,
            suspended,
            locked,
            resumed,
            idle_since,
        } = observation;
        self.timing = settings.timing;
        self.observe_suspend(now, suspended);
        self.observe_input(now, locked, resumed, idle_since);
        match command {
            Some(Command::Pause) => self.pause(now),
            Some(Command::Resume) => {
                self.resume(now);
                self.published = None;
            }
            Some(Command::Reset) => self.reset(now),
            Some(Command::Status) | None => (),
        }
        let snapshot = self.snapshot(now, settings.curve);
        let next = Publication::for_phase(snapshot.display_phase(), settings);
        let display = if matches!(command, Some(Command::Pause | Command::Reset)) {
            Some(DisplayUpdate::Reset)
        } else if self.published != Some(next) {
            Some(DisplayUpdate::Publish(next))
        } else {
            None
        };
        self.published = Some(next);
        Frame {
            status: Status::from_snapshot(snapshot),
            display,
        }
    }

    fn new(now: u64) -> Self {
        Self {
            state: CycleState::Working { started_at: now },
            timing: Timing::default(),
            locked: false,
            idle_since: None,
            idle_floor: 0,
            paused_at: None,
            published: None,
        }
    }

    /// Observe one adapter sample. Unlock is a return even without an input event.
    fn observe_input(&mut self, now: u64, locked: bool, resumed: bool, idle_since: Option<u64>) {
        let unlocking = self.locked && !locked;
        // On unlock, process queued password input while still logically locked.
        self.locked |= locked;
        if resumed {
            self.observe_activity(now);
        }
        if let Some(since) = idle_since {
            self.observe_idle_since(since);
        }
        self.locked = locked;
        if unlocking {
            self.observe_activity(now);
            // Do not accept pre-unlock idle history on the next adapter sample.
            self.idle_floor = now;
        }
    }

    /// Pause is idempotent. Manual pauses discard idle observation, not work debt.
    fn pause(&mut self, now: u64) {
        if self.paused_at.is_none() {
            self.complete_break(now);
            self.paused_at = Some(now);
            self.idle_since = None;
        }
    }

    fn resume(&mut self, now: u64) {
        if let Some(paused_at) = self.paused_at.take() {
            match &mut self.state {
                CycleState::Working { started_at } => {
                    let worked = paused_at.saturating_sub(*started_at);
                    *started_at = now.saturating_sub(worked);
                }
                CycleState::Resting { since } => {
                    *since = since.saturating_add(now.saturating_sub(paused_at));
                }
            }
            self.idle_floor = now;
        }
    }

    /// Reset always starts a new, running work interval, even from pause or overtime.
    fn reset(&mut self, now: u64) {
        let timing = self.timing;
        let locked = self.locked;
        *self = Self::new(now);
        self.timing = timing;
        self.locked = locked;
        self.idle_floor = now;
    }

    /// Only proven suspend is input-free. Conservatively round short samples;
    /// a full break can complete even if input resumes before the next tick.
    fn observe_suspend(&mut self, now: u64, suspended: u64) {
        let break_interval = u64::from(self.timing.break_seconds.get());
        if suspended >= break_interval || suspended > 1 {
            let since = if suspended >= break_interval {
                now.saturating_sub(suspended)
            } else {
                now.saturating_sub(suspended).saturating_add(1)
            };
            self.observe_idle_since(since);
        }
    }

    /// Repeated idle reports preserve the earliest observed start of this pause.
    fn observe_idle_since(&mut self, since: u64) {
        if self.paused_at.is_none() {
            let since = since.max(self.idle_floor);
            self.idle_since = Some(self.idle_since.map_or(since, |known| known.min(since)));
        }
    }

    /// Records input. A completed break restarts work only when input resumes.
    fn observe_activity(&mut self, now: u64) {
        self.complete_break(now);
        self.idle_since = None;
        if !self.locked && matches!(self.state, CycleState::Resting { .. }) {
            // Remember resumed input during pause, but accrue no work until resume.
            self.state = CycleState::Working {
                started_at: self.paused_at.unwrap_or(now),
            };
        }
    }

    fn snapshot(&mut self, now: u64, curve: GraceCurve) -> Snapshot {
        self.complete_break(now);
        let effective_now = self.paused_at.unwrap_or(now);
        let (worked, resting, resting_seconds) = match self.state {
            CycleState::Working { started_at } => {
                (effective_now.saturating_sub(started_at), false, 0)
            }
            CycleState::Resting { since } => (0, true, effective_now.saturating_sub(since)),
        };
        let work_interval = u64::from(self.timing.work_seconds.get());
        let grace_interval = u64::from(self.timing.grace_seconds);
        let break_interval = u64::from(self.timing.break_seconds.get());
        let break_remaining = if resting {
            0
        } else {
            break_interval.saturating_sub(self.idle_duration(now))
        };
        let (phase, phase_remaining) = if worked < work_interval {
            (DesiredPhase::Work, work_interval + grace_interval - worked)
        } else if worked < work_interval + grace_interval {
            let progress = (worked - work_interval) as f32 / grace_interval as f32;
            (
                DesiredPhase::Interference(curve.level(progress)),
                work_interval + grace_interval - worked,
            )
        } else {
            (
                DesiredPhase::TestCard {
                    remaining: break_remaining,
                },
                break_remaining,
            )
        };
        let mode = if self.paused_at.is_some() {
            Mode::Paused
        } else if resting {
            Mode::Resting
        } else {
            Mode::Running
        };
        Snapshot {
            mode,
            phase,
            worked,
            phase_remaining,
            break_remaining,
            resting_seconds,
        }
    }

    fn complete_break(&mut self, now: u64) {
        let break_interval = u64::from(self.timing.break_seconds.get());
        if self.paused_at.is_none()
            && matches!(self.state, CycleState::Working { .. })
            && self.idle_duration(now) >= break_interval
        {
            self.state = CycleState::Resting {
                since: self.idle_since.unwrap().saturating_add(break_interval),
            };
        }
    }

    fn idle_duration(&self, now: u64) -> u64 {
        self.idle_since.map_or(0, |since| now.saturating_sub(since))
    }
}

impl Status {
    fn from_snapshot(snapshot: Snapshot) -> Self {
        let (phase, level) = match snapshot.phase {
            DesiredPhase::Work => (Phase::Work, 0.0),
            DesiredPhase::Interference(level) => (Phase::Grace, level.value()),
            DesiredPhase::TestCard { .. } => (Phase::Overtime, 0.0),
        };
        let state = match snapshot.mode {
            Mode::Running => State::Running,
            Mode::Paused => State::Paused,
            Mode::Resting => State::Resting,
        };
        Self {
            state,
            phase: Some(phase),
            work_seconds: Some(snapshot.worked),
            remaining_seconds: Some(snapshot.phase_remaining),
            break_remaining_seconds: Some(snapshot.break_remaining),
            resting_seconds: Some(snapshot.resting_seconds),
            glitchiness: if state == State::Paused { 0.0 } else { level },
        }
    }
}

#[cfg(test)]
mod tests;
