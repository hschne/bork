//! Linux clock sampling. Offset bounds distinguish suspend from process stalls.
use std::time::Duration;

pub(crate) fn boot_time() -> Duration {
    use rustix::time::{ClockId, clock_gettime};
    let time = clock_gettime(ClockId::Boottime);
    Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

#[derive(Clone, Copy)]
pub(crate) struct ClockSample {
    boot_nanos: i128,
    offset_lower: i128,
    offset_upper: i128,
}

impl ClockSample {
    pub(crate) fn now() -> Self {
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

    pub(crate) fn seconds(self) -> u64 {
        (self.boot_nanos / 1_000_000_000) as u64
    }

    pub(crate) fn elapsed_since(self, epoch: Self) -> f32 {
        (self.boot_nanos - epoch.boot_nanos) as f32 / 1_000_000_000.0
    }

    pub(crate) fn suspended_since(&self, previous: &Self) -> u64 {
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
