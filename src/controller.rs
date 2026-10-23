//! Timing is expressed as monotonic seconds including suspend (CLOCK_BOOTTIME at the adapter).
//! Inactivity never pauses work debt. Only a completed continuous break clears it.

pub const WORK: u64 = 20 * 60;
pub const GRACE: u64 = 3 * 60;
pub const BREAK: u64 = 5 * 60;

#[derive(Debug, PartialEq)]
pub enum Phase {
    Work,
    Grace(f32),
    TestCard { remaining: u64 },
}

pub struct Controller {
    cycle_start: u64,
    idle_since: Option<u64>,
    resting: bool,
}

impl Controller {
    pub fn new(now: u64) -> Self {
        Self {
            cycle_start: now,
            idle_since: None,
            resting: false,
        }
    }

    /// Adapters report the start of continuous keyboard AND pointer inactivity.
    /// Repeated idle reports must preserve the earliest known timestamp.
    pub fn idle(&mut self, since: u64) {
        self.idle_since = Some(self.idle_since.map_or(since, |old| old.min(since)));
    }

    pub fn active(&mut self, now: u64) {
        // Complete a break even when no tick occurred during suspend.
        self.phase(now, 2.0);
        self.idle_since = None;
        if self.resting {
            self.cycle_start = now;
            self.resting = false;
        }
    }

    pub fn phase(&mut self, now: u64, exponent: f32) -> Phase {
        let idle = self.idle_since.map_or(0, |since| now.saturating_sub(since));
        if idle >= BREAK {
            self.resting = true;
        }
        if self.resting {
            return Phase::Work;
        }
        let elapsed = now.saturating_sub(self.cycle_start);
        if elapsed < WORK {
            Phase::Work
        } else if elapsed < WORK + GRACE {
            let exponent = if exponent.is_finite() {
                exponent.clamp(1.01, 5.0)
            } else {
                2.0
            };
            Phase::Grace(100.0 * ((elapsed - WORK) as f32 / GRACE as f32).powf(exponent))
        } else {
            Phase::TestCard {
                remaining: BREAK.saturating_sub(idle),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_and_acceleration() {
        let mut c = Controller::new(10);
        assert_eq!(c.phase(1209, 2.0), Phase::Work);
        assert_eq!(c.phase(1210, 2.0), Phase::Grace(0.0));
        assert_eq!(c.phase(1300, 2.0), Phase::Grace(25.0));
        assert_eq!(c.phase(1390, 2.0), Phase::TestCard { remaining: 300 });
    }

    #[test]
    fn inactivity_crosses_deadline_and_resume_restarts_countdown() {
        let mut c = Controller::new(0);
        c.idle(1300);
        assert_eq!(c.phase(1380, 2.0), Phase::TestCard { remaining: 220 });
        c.active(1400);
        assert_eq!(c.phase(1400, 2.0), Phase::TestCard { remaining: 300 });
        c.idle(1401);
        assert_eq!(c.phase(1700, 2.0), Phase::TestCard { remaining: 1 });
        assert_eq!(c.phase(1701, 2.0), Phase::Work);
        assert_eq!(c.phase(9000, 2.0), Phase::Work);
        c.active(9000);
        assert_eq!(c.phase(10199, 2.0), Phase::Work);
        assert_eq!(c.phase(10200, 2.0), Phase::Grace(0.0));
    }

    #[test]
    fn short_pauses_do_not_erase_debt() {
        let mut c = Controller::new(0);
        c.idle(1000);
        c.active(1299);
        assert!(matches!(c.phase(1300, 2.0), Phase::Grace(_)));
    }

    #[test]
    fn idle_startup_and_suspend_complete_break_without_tick() {
        let mut c = Controller::new(1000);
        c.idle(800);
        c.active(1100);
        assert_eq!(c.phase(2299, 2.0), Phase::Work);
        assert_eq!(c.phase(2300, 2.0), Phase::Grace(0.0));
        c.idle(2400);
        c.active(5000);
        assert_eq!(c.phase(5000, 2.0), Phase::Work);
    }
}
