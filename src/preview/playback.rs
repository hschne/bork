//! A compressed cycle clock; shader animation keeps its ordinary wall clock.
use crate::effect::{Glitchiness, Settings};
use std::time::Instant;

pub(super) const DEMO_SECONDS: f32 = 15.0;
const GRACE_START: f32 = 1.0;
const OVERTIME_START: f32 = 12.0;

#[derive(Default)]
pub(super) struct Playback {
    pub position: f32,
    pub playing: bool,
    last: Option<Instant>,
}

impl Playback {
    pub fn tick(&mut self, now: Instant) {
        if self.playing {
            if let Some(last) = self.last {
                self.position =
                    (self.position + now.duration_since(last).as_secs_f32()).min(DEMO_SECONDS);
            }
            self.last = Some(now);
            if self.position == DEMO_SECONDS {
                self.pause();
            }
        }
    }

    pub fn toggle(&mut self, now: Instant) {
        if self.playing {
            self.pause();
        } else {
            if self.position >= DEMO_SECONDS {
                self.position = 0.0;
            }
            self.playing = true;
            self.last = Some(now);
        }
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.last = None;
    }

    pub fn seek(&mut self, position: f32) {
        self.position = position.clamp(0.0, DEMO_SECONDS);
        self.pause();
    }

    pub fn level(&self, settings: &Settings) -> Glitchiness {
        if self.position < GRACE_START {
            Glitchiness::ZERO
        } else if settings.timing.grace_seconds == 0 {
            Glitchiness::MAXIMUM
        } else {
            settings.curve().level(
                ((self.position - GRACE_START) / (OVERTIME_START - GRACE_START)).clamp(0.0, 1.0),
            )
        }
    }

    pub fn test_card(&self, settings: &Settings) -> bool {
        self.position >= OVERTIME_START
            || (settings.timing.grace_seconds == 0 && self.position >= GRACE_START)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn progression_uses_configured_curve_and_keeps_deadline_separate_from_strength() {
        let mut settings = Settings::default();
        let mut playback = Playback::default();
        assert_eq!(playback.level(&settings), Glitchiness::ZERO);
        playback.seek(6.5);
        assert_eq!(playback.level(&settings).value(), 25.0);
        settings.set_curve(3.0).unwrap();
        assert_eq!(playback.level(&settings).value(), 12.5);
        settings.set_strength(0.0).unwrap();
        playback.seek(12.0);
        assert!(playback.test_card(&settings));
        assert_eq!(playback.level(&settings), Glitchiness::MAXIMUM);
        settings.timing.grace_seconds = 0;
        playback.seek(1.0);
        assert!(playback.test_card(&settings));
    }

    #[test]
    fn playback_stops_restarts_and_scrubbing_pauses_without_advancing_animation() {
        let now = Instant::now();
        let mut playback = Playback::default();
        playback.toggle(now);
        playback.tick(now + Duration::from_secs(7));
        assert_eq!(playback.position, 7.0);
        playback.seek(3.0);
        playback.tick(now + Duration::from_secs(30));
        assert_eq!(playback.position, 3.0);
        playback.toggle(now + Duration::from_secs(30));
        playback.tick(now + Duration::from_secs(60));
        assert_eq!(playback.position, DEMO_SECONDS);
        assert!(!playback.playing);
        playback.toggle(now + Duration::from_secs(60));
        assert_eq!(playback.position, 0.0);
        assert!(playback.playing);
    }
}
