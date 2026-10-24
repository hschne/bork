//! Break-cycle policy, independent of clocks, input protocols, and rendering.
//! Time values are boot-time seconds, so a suspend is ordinary input-free time.

use crate::effect::{Glitchiness, GraceCurve};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

pub const WORK_INTERVAL: u64 = 20 * 60;
pub const GRACE_INTERVAL: u64 = 3 * 60;
pub const BREAK_INTERVAL: u64 = 5 * 60;

/// Durations in whole seconds. Work and break must be positive; grace may be zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    pub work_seconds: NonZeroU32,
    pub grace_seconds: u32,
    pub break_seconds: NonZeroU32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            work_seconds: NonZeroU32::new(WORK_INTERVAL as u32).unwrap(),
            grace_seconds: GRACE_INTERVAL as u32,
            break_seconds: NonZeroU32::new(BREAK_INTERVAL as u32).unwrap(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DesiredPhase {
    Work,
    Interference(Glitchiness),
    TestCard { remaining: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Running,
    Paused,
    Resting,
}

pub struct Snapshot {
    pub mode: Mode,
    pub phase: DesiredPhase,
    pub worked: u64,
    pub phase_remaining: u64,
    pub break_remaining: u64,
    pub resting_seconds: u64,
}

impl Snapshot {
    pub fn display_phase(&self) -> DesiredPhase {
        if self.mode == Mode::Running {
            self.phase
        } else {
            DesiredPhase::Work
        }
    }
}

/// Owns work debt and the evidence needed to complete a continuous break.
pub struct BreakCycle {
    state: CycleState,
    timing: Timing,
    locked: bool,
    idle_since: Option<u64>,
    idle_floor: u64,
    paused_at: Option<u64>,
}

enum CycleState {
    Working { started_at: u64 },
    Resting { since: u64 },
}

impl BreakCycle {
    pub fn new(now: u64) -> Self {
        Self {
            state: CycleState::Working { started_at: now },
            timing: Timing::default(),
            locked: false,
            idle_since: None,
            idle_floor: 0,
            paused_at: None,
        }
    }

    /// Retuning preserves work debt and already observed inactivity.
    pub fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
    }

    /// Observe one adapter sample. Unlock is a return even without an input event.
    pub fn observe_input(
        &mut self,
        now: u64,
        locked: bool,
        resumed: bool,
        idle_since: Option<u64>,
    ) {
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
    pub fn pause(&mut self, now: u64) {
        if self.paused_at.is_none() {
            self.complete_break(now);
            self.paused_at = Some(now);
            self.idle_since = None;
        }
    }

    pub fn resume(&mut self, now: u64) {
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
    pub fn reset(&mut self, now: u64) {
        let timing = self.timing;
        let locked = self.locked;
        *self = Self::new(now);
        self.timing = timing;
        self.locked = locked;
        self.idle_floor = now;
    }

    /// Only proven suspend is input-free. Conservatively round short samples;
    /// a full break can complete even if input resumes before the next tick.
    pub fn observe_suspend(&mut self, now: u64, suspended: u64) {
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
    pub fn observe_idle_since(&mut self, since: u64) {
        if self.paused_at.is_none() {
            let since = since.max(self.idle_floor);
            self.idle_since = Some(self.idle_since.map_or(since, |known| known.min(since)));
        }
    }

    /// Records input. A completed break restarts work only when input resumes.
    pub fn observe_activity(&mut self, now: u64) {
        self.complete_break(now);
        self.idle_since = None;
        if !self.locked && matches!(self.state, CycleState::Resting { .. }) {
            // Remember resumed input during pause, but accrue no work until resume.
            self.state = CycleState::Working {
                started_at: self.paused_at.unwrap_or(now),
            };
        }
    }

    /// Advances the cycle and returns the only display phase callers need to know.
    pub fn advance(&mut self, now: u64, curve: GraceCurve) -> DesiredPhase {
        self.snapshot(now, curve).display_phase()
    }

    pub fn snapshot(&mut self, now: u64, curve: GraceCurve) -> Snapshot {
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

#[cfg(test)]
mod tests;
