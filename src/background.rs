//! Windowless adapter: observe input, advance work debt, and publish display intent.
use crate::{
    clock::ClockSample,
    config,
    control::{self, Command, Status},
    cycle::{BreakCycle, DesiredPhase},
    effect::{Glitchiness, Settings},
    hyprland::Hyprland,
    recovery::{DisplayState, Guardian},
};
use anyhow::Result;
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
    let controls = control::Server::bind(&hypr.runtime_directory()?)?;
    let mut guardian = Guardian::spawn(&hypr)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopping);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let epoch = ClockSample::now();
    let mut previous_clock = epoch;
    let mut cycle = BreakCycle::new(epoch.seconds());
    let mut published = None;
    let result = (|| -> Result<()> {
        while !stopping.load(Ordering::Relaxed) {
            input.dispatch()?;
            if settings.reload(Instant::now())? {
                published = None;
            }
            cycle.set_timing(settings.current().timing);
            let locked = hypr.locked()?;
            let clock = ClockSample::now();
            let now = clock.seconds();
            cycle.observe_suspend(now, clock.suspended_since(&previous_clock));
            previous_clock = clock;
            let activity = input.activity();
            cycle.observe_input(now, locked, activity.resumed, activity.idle_since);

            let pending = controls.poll()?;
            let action = (|| -> Result<Status> {
                if let Some(request) = &pending {
                    match request.command {
                        Command::Pause => {
                            cycle.pause(now);
                            guardian.reset()?;
                            published = Some(Publication::Work);
                        }
                        Command::Resume => {
                            cycle.resume(now);
                            published = None;
                        }
                        Command::Reset => {
                            cycle.reset(now);
                            guardian.reset()?;
                            published = Some(Publication::Work);
                        }
                        Command::Status => (),
                    }
                }
                let tuning = settings.current();
                let phase = cycle.advance(now, tuning.curve());
                let next = Publication::for_phase(phase, tuning);
                if published != Some(next) {
                    let state = match next {
                        Publication::Work => DisplayState::Work,
                        Publication::Interference(level) => DisplayState::Interference {
                            shader: tuning.hyprland_shader_at(
                                level,
                                clock.elapsed_since(epoch),
                                seed,
                            )?,
                        },
                        Publication::TestCard(remaining) => DisplayState::TestCard { remaining },
                    };
                    guardian.set_display(state)?;
                    published = Some(next);
                }
                guardian.heartbeat()?;
                Ok(Status::from_snapshot(cycle.snapshot(now, tuning.curve())))
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
#[derive(Clone, Copy, PartialEq)]
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
                if settings.has_visible_effect_at(level) {
                    Self::Interference(level)
                } else {
                    Self::Work
                }
            }
            DesiredPhase::TestCard { remaining } => Self::TestCard(remaining),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publications_quantize_interference_but_never_replace_deadline_with_maximum() {
        let settings = Settings::default();
        assert!(
            Publication::for_phase(
                DesiredPhase::Interference(Glitchiness::try_from(5.49).unwrap()),
                &settings
            ) == Publication::Work
        );
        assert!(
            Publication::for_phase(
                DesiredPhase::Interference(Glitchiness::try_from(50.49).unwrap()),
                &settings
            ) == Publication::Interference(Glitchiness::try_from(50.0).unwrap())
        );
        assert!(
            Publication::for_phase(
                DesiredPhase::Interference(Glitchiness::try_from(100.0).unwrap()),
                &settings
            ) == Publication::Interference(Glitchiness::try_from(100.0).unwrap())
        );
        assert!(
            Publication::for_phase(DesiredPhase::TestCard { remaining: 300 }, &settings)
                == Publication::TestCard(300)
        );
    }
}
