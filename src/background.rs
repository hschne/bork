use crate::{
    config,
    controller::{Controller, Phase},
    hyprland::Hyprland,
    recovery::{Guardian, Message},
    wayland::Wayland,
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
    // Complete protocol validation before acquiring display ownership or changing settings.
    let mut input = Wayland::connect(false, 300)?;
    let seed = crate::effect::fresh_seed()?;
    let settings_path = config::path()?;
    let mut settings = config::load(&settings_path)?;
    let mut settings_modified = std::fs::metadata(&settings_path)
        .and_then(|meta| meta.modified())
        .ok();
    let mut settings_checked = Instant::now();
    let runtime = hypr.runtime_directory()?;
    let mut guardian = Guardian::spawn(&runtime, &hypr)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let signal = stopping.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let mut previous_clock = ClockSample::now();
    let animation_epoch = previous_clock.boot_nanos;
    let mut controller = Controller::new((previous_clock.boot_nanos / 1_000_000_000) as u64);
    let mut installed = false;
    let mut card = false;
    let mut last_level = -1.0;
    let result = (|| -> Result<()> {
        while !stopping.load(Ordering::Relaxed) {
            input.dispatch()?;
            if settings_checked.elapsed() >= Duration::from_secs(1) {
                settings_checked = Instant::now();
                let modified = std::fs::metadata(&settings_path)
                    .and_then(|meta| meta.modified())
                    .ok();
                if modified != settings_modified {
                    settings = config::load(&settings_path)?;
                    settings_modified = modified;
                    last_level = -1.0; // Re-render changed tuning without touching work debt.
                }
            }
            let clock = ClockSample::now();
            let now = (clock.boot_nanos / 1_000_000_000) as u64;
            let suspended = clock.suspended_since(&previous_clock);
            if suspended > 1 {
                // Only proven suspend is input-free. Round shorter intervals up at
                // the start; a proven full break can complete even on immediate input.
                let since = if suspended >= crate::controller::BREAK {
                    now.saturating_sub(crate::controller::BREAK)
                } else {
                    now.saturating_sub(suspended).saturating_add(1)
                };
                controller.idle(since);
            }
            previous_clock = clock;
            let (resumed, since) = input.activity();
            if resumed {
                controller.active(now);
            }
            if let Some(since) = since {
                controller.idle(since);
            }
            let phase = controller.phase(now, settings.curve);
            // Locking does not reset debt. Input-free lock time counts as the same break.
            // Do not obscure or distort the real security lock screen.
            let locked = hypr.locked()?;
            match phase {
                Phase::Work if installed || card => {
                    guardian.send(&Message::Clear)?;
                    installed = false;
                    card = false;
                }
                _ if locked => {
                    if installed || card {
                        guardian.send(&Message::Clear)?;
                    }
                    installed = false;
                    card = false;
                }
                Phase::Grace(level)
                    if [
                        &settings.tears,
                        &settings.colour,
                        &settings.bands,
                        &settings.noise,
                        &settings.ghost,
                    ]
                    .iter()
                    .all(|effect| effect.strength(level) == 0.0) =>
                {
                    if installed || card {
                        guardian.send(&Message::Clear)?;
                    }
                    installed = false;
                    card = false;
                }
                Phase::Grace(level) => {
                    // Half-percent steps bound shader recompilation without making the grace linear.
                    let level = (level * 2.0).floor() / 2.0;
                    if !installed || (level - last_level).abs() >= 0.5 {
                        let animation_time =
                            (clock.boot_nanos - animation_epoch) as f32 / 1_000_000_000.0;
                        let shader = settings.hyprland_shader_at(level, animation_time, seed)?;
                        if installed {
                            guardian.send(&Message::Update { shader })?;
                        } else {
                            guardian.send(&Message::Install {
                                shader,
                                trial: false,
                            })?;
                        }
                        last_level = level;
                        installed = true;
                        card = false;
                    }
                }
                Phase::TestCard { remaining } => {
                    guardian.send(&Message::TestCard { remaining })?;
                    installed = false;
                    card = true;
                }
                Phase::Work => (),
            }
            guardian.send(&Message::Heartbeat)?;
            thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    })();
    let cleanup = guardian.finish();
    if result.as_ref().is_err_and(|error| {
        error.is::<crate::recovery::Stopped>()
            || error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
    }) {
        cleanup
    } else {
        result.and(cleanup)
    }
}

#[derive(Clone, Copy)]
struct ClockSample {
    boot_nanos: i128,
    offset_lower: i128,
    offset_upper: i128,
}

impl ClockSample {
    fn now() -> Self {
        use rustix::time::{ClockId, clock_gettime};
        let nanos = |clock| {
            let time = clock_gettime(clock);
            i128::from(time.tv_sec) * 1_000_000_000 + i128::from(time.tv_nsec)
        };
        let before = nanos(ClockId::Monotonic);
        let boot_nanos = nanos(ClockId::Boottime);
        let after = nanos(ClockId::Monotonic);
        Self {
            boot_nanos,
            offset_lower: boot_nanos - after,
            offset_upper: boot_nanos - before,
        }
    }

    fn suspended_since(&self, previous: &Self) -> u64 {
        // Bounds, not a point estimate: even arbitrary preemption between clock
        // reads cannot misclassify ordinary work as a completed suspended break.
        ((self.offset_lower - previous.offset_upper).max(0) / 1_000_000_000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suspend_bounds_exclude_clock_sampling_stalls_and_round_down() {
        let previous = ClockSample {
            boot_nanos: 0,
            offset_lower: 1000,
            offset_upper: 2000,
        };
        let stalled = ClockSample {
            boot_nanos: 600_000_000_000,
            offset_lower: 1000,
            offset_upper: 2000,
        };
        assert_eq!(stalled.suspended_since(&previous), 0);
        let preempted_sampling = ClockSample {
            offset_lower: -600_000_000_000,
            ..stalled
        };
        assert_eq!(preempted_sampling.suspended_since(&previous), 0);
        assert_eq!(
            stalled.suspended_since(&ClockSample {
                offset_upper: 600_000_000_000,
                ..previous
            }),
            0
        );
        let suspended = ClockSample {
            offset_lower: 299_999_999_999 + previous.offset_upper,
            ..stalled
        };
        assert_eq!(suspended.suspended_since(&previous), 299);
        let suspended = ClockSample {
            offset_lower: 300_000_000_000 + previous.offset_upper,
            ..stalled
        };
        assert_eq!(suspended.suspended_since(&previous), 300);
    }
}
