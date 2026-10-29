//! A compressed cycle clock; shader animation keeps its ordinary wall clock.
use crate::{
    config::Settings,
    effect::{Glitchiness, SIGNAL_LOSS_SECONDS, SignalLoss},
};
use std::time::Instant;

pub(super) const DEMO_SECONDS: f32 = 15.0;
const GRACE_START: f32 = 1.0;
const OVERTIME_START: f32 = 12.0;

pub(super) struct Playback {
    pub position: f32,
    pub playing: bool,
    last: Option<Instant>,
    loss: SignalLoss,
}

impl Playback {
    pub fn new(seed: u32) -> Self {
        Self {
            position: 0.0,
            playing: false,
            last: None,
            loss: SignalLoss::new(seed),
        }
    }

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
                self.loss = self.loss.next();
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
        if [GRACE_START, OVERTIME_START]
            .iter()
            .any(|start| position < *start && self.position >= *start)
        {
            self.loss = self.loss.next();
        }
        self.position = position.clamp(0.0, DEMO_SECONDS);
        self.pause();
    }

    pub fn level(&self, settings: &Settings) -> Glitchiness {
        if self.overtime(settings) {
            Glitchiness::MAXIMUM
        } else if let Some(progress) = self.grace_progress(settings) {
            settings.curve.level(progress)
        } else {
            Glitchiness::ZERO
        }
    }

    pub fn grace_progress(&self, settings: &Settings) -> Option<f32> {
        (self.position >= GRACE_START && !self.overtime(settings)).then(|| {
            ((self.position - GRACE_START) / (OVERTIME_START - GRACE_START)).clamp(0.0, 1.0)
        })
    }

    fn overtime_start(settings: &Settings) -> f32 {
        if settings.timing.grace_seconds == 0 {
            GRACE_START
        } else {
            OVERTIME_START
        }
    }

    pub fn overtime(&self, settings: &Settings) -> bool {
        self.position >= Self::overtime_start(settings)
    }

    pub fn transition(&self, settings: &Settings) -> Option<(SignalLoss, f32)> {
        let elapsed = self.position - Self::overtime_start(settings);
        (0.0..SIGNAL_LOSS_SECONDS)
            .contains(&elapsed)
            .then_some((self.loss, elapsed))
    }

    pub fn test_card(&self, settings: &Settings) -> bool {
        self.position >= Self::overtime_start(settings) + SIGNAL_LOSS_SECONDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn progression_uses_configured_curve_and_keeps_deadline_separate_from_intensity() {
        let mut settings = Settings::default();
        let mut playback = Playback::new(41);
        assert_eq!(playback.level(&settings), Glitchiness::ZERO);
        playback.seek(6.5);
        assert_eq!(playback.level(&settings), settings.curve.level(0.5));
        let mut points = settings.curve.points();
        points[0].intensity = 5.0;
        points[1].intensity = 10.0;
        settings.curve = points.try_into().unwrap();
        assert_eq!(playback.level(&settings), settings.curve.level(0.5));
        assert_eq!(playback.grace_progress(&settings), Some(0.5));
        playback.seek(12.0);
        assert!(playback.transition(&settings).is_some());
        assert!(!playback.test_card(&settings));
        assert_eq!(playback.grace_progress(&settings), None);
        playback.seek(12.0 + SIGNAL_LOSS_SECONDS);
        assert!(playback.test_card(&settings));
        assert_eq!(playback.level(&settings), Glitchiness::MAXIMUM);
        settings.timing.grace_seconds = 0;
        playback.seek(1.0);
        assert!(playback.transition(&settings).is_some());
        playback.seek(1.0 + SIGNAL_LOSS_SECONDS);
        assert!(playback.test_card(&settings));
    }

    #[test]
    fn transition_scrubs_repeatably_but_replays_with_a_new_seed() {
        let settings = Settings::default();
        let mut playback = Playback::new(41);
        playback.seek(12.2);
        let (first, elapsed) = playback.transition(&settings).unwrap();
        assert!((elapsed - 0.2).abs() < 0.00001);
        playback.seek(12.1);
        assert_eq!(
            first.shader_body(),
            playback.transition(&settings).unwrap().0.shader_body()
        );
        playback.seek(11.0);
        playback.seek(12.2);
        assert_ne!(
            first.shader_body(),
            playback.transition(&settings).unwrap().0.shader_body()
        );
        playback.seek(DEMO_SECONDS);
        playback.toggle(Instant::now());
        playback.seek(12.2);
        assert_ne!(
            first.shader_body(),
            playback.transition(&settings).unwrap().0.shader_body()
        );
    }

    #[test]
    fn playback_stops_restarts_and_scrubbing_pauses_without_advancing_animation() {
        let now = Instant::now();
        let mut playback = Playback::new(41);
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
