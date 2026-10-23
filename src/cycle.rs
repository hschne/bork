//! Break-cycle policy, independent of clocks, input protocols, and rendering.
//! Time values are boot-time seconds, so a suspend is ordinary input-free time.

use crate::effect::{Glitchiness, GraceCurve};

pub const WORK_INTERVAL: u64 = 20 * 60;
pub const GRACE_INTERVAL: u64 = 3 * 60;
pub const BREAK_INTERVAL: u64 = 5 * 60;

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
    idle_since: Option<u64>,
    idle_floor: u64,
    paused_at: Option<u64>,
}

enum CycleState {
    Working { started_at: u64 },
    Resting,
}

impl BreakCycle {
    pub fn new(now: u64) -> Self {
        Self {
            state: CycleState::Working { started_at: now },
            idle_since: None,
            idle_floor: 0,
            paused_at: None,
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
            if let CycleState::Working { started_at } = &mut self.state {
                let worked = paused_at.saturating_sub(*started_at);
                *started_at = now.saturating_sub(worked);
            }
            self.idle_floor = now;
        }
    }

    /// Reset always starts a new, running work interval, even from pause or overtime.
    pub fn reset(&mut self, now: u64) {
        *self = Self::new(now);
        self.idle_floor = now;
    }

    /// Only proven suspend is input-free. Conservatively round short samples;
    /// a full break can complete even if input resumes before the next tick.
    pub fn observe_suspend(&mut self, now: u64, suspended: u64) {
        if suspended > 1 {
            let since = if suspended >= BREAK_INTERVAL {
                now.saturating_sub(BREAK_INTERVAL)
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
        if matches!(self.state, CycleState::Resting) {
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
        let (worked, resting) = match self.state {
            CycleState::Working { started_at } => (effective_now.saturating_sub(started_at), false),
            CycleState::Resting => (0, true),
        };
        let break_remaining = if resting {
            0
        } else {
            BREAK_INTERVAL.saturating_sub(self.idle_duration(now))
        };
        let (phase, phase_remaining) = if worked < WORK_INTERVAL {
            (DesiredPhase::Work, WORK_INTERVAL - worked)
        } else if worked < WORK_INTERVAL + GRACE_INTERVAL {
            let progress = (worked - WORK_INTERVAL) as f32 / GRACE_INTERVAL as f32;
            (
                DesiredPhase::Interference(curve.level(progress)),
                WORK_INTERVAL + GRACE_INTERVAL - worked,
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
        }
    }

    fn complete_break(&mut self, now: u64) {
        if self.paused_at.is_none() && self.idle_duration(now) >= BREAK_INTERVAL {
            self.state = CycleState::Resting;
        }
    }

    fn idle_duration(&self, now: u64) -> u64 {
        self.idle_since.map_or(0, |since| now.saturating_sub(since))
    }
}

#[cfg(test)]
mod tests;
