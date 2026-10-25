//! One owner for background state, trial precedence, locks, and display resources.
use super::{DisplayState, journal, trial::TrialRequest};
use crate::{clock::boot_time, hyprland::Hyprland, wayland::TestCards};
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) enum TrialDecision {
    Accepted(u64),
    Busy,
    Locked,
}

struct Trial {
    id: u64,
    deadline: Duration,
    state: DisplayState,
}

pub(super) struct DisplayOwner {
    path: PathBuf,
    hypr: Hyprland,
    background: DisplayState,
    trial: Option<Trial>,
    next_trial_id: u64,
    cards: Option<TestCards>,
    locked: bool,
    lock_checked: Instant,
}

impl DisplayOwner {
    pub(super) fn new(path: &Path, hypr: Hyprland) -> Self {
        Self {
            path: path.to_owned(),
            hypr,
            background: DisplayState::Work,
            trial: None,
            next_trial_id: 0,
            cards: None,
            locked: false,
            lock_checked: Instant::now(),
        }
    }

    pub(super) fn set_background(&mut self, state: DisplayState) -> Result<()> {
        self.background = state;
        if self.trial.is_none() {
            self.reconcile()?;
        }
        Ok(())
    }

    pub(super) fn reset(&mut self) -> Result<()> {
        self.background = DisplayState::Work;
        self.trial = None;
        self.clear()
    }

    /// Protocol refusals are nonfatal; failed display mutations require recovery.
    pub(super) fn trial(&mut self, request: TrialRequest) -> Result<TrialDecision> {
        let state = match request {
            TrialRequest::Start { shader } => DisplayState::Interference { shader },
            TrialRequest::StartCard { remaining } => DisplayState::TestCard { remaining },
            TrialRequest::Cancel { id } => {
                if self.trial.as_ref().is_some_and(|trial| trial.id == id) {
                    self.trial = None;
                    self.reconcile()?;
                }
                return Ok(TrialDecision::Accepted(id));
            }
        };
        if self.trial.is_some() {
            return Ok(TrialDecision::Busy);
        }
        if self.hypr.locked()? {
            return Ok(TrialDecision::Locked);
        }
        self.next_trial_id = self
            .next_trial_id
            .checked_add(1)
            .context("Trial IDs exhausted")?;
        let id = self.next_trial_id;
        self.trial = Some(Trial {
            id,
            state,
            deadline: boot_time() + Duration::from_secs(5),
        });
        self.reconcile()?;
        Ok(TrialDecision::Accepted(id))
    }

    pub(super) fn tick(&mut self) -> Result<()> {
        if let Some(cards) = self.cards.as_mut() {
            cards.dispatch()?;
        }
        if self.lock_checked.elapsed() >= Duration::from_millis(250) {
            self.lock_checked = Instant::now();
            let locked = self.hypr.locked()?;
            if locked != self.locked {
                self.reconcile()?;
            }
        }
        if self
            .trial
            .as_ref()
            .is_some_and(|trial| boot_time() >= trial.deadline)
        {
            self.trial = None;
            self.reconcile()?;
        }
        Ok(())
    }

    fn reconcile(&mut self) -> Result<()> {
        self.locked = self.hypr.locked()?;
        if self.locked {
            self.trial = None;
            return self.clear();
        }
        let desired = match &self.trial {
            Some(trial) => trial.state.clone(),
            None => self.background.clone(),
        };
        match desired {
            DisplayState::Work => self.clear(),
            DisplayState::Interference { shader } => {
                self.cards = None;
                if self.path.join("journal.json").exists() {
                    journal::update(&self.path, &self.hypr, &shader)
                } else {
                    journal::install(&self.path, &self.hypr, &shader)
                }
            }
            DisplayState::TestCard { remaining } => {
                journal::restore(&self.path)?;
                if let Some(cards) = self.cards.as_mut() {
                    cards.countdown(remaining)
                } else {
                    self.cards = Some(TestCards::show(remaining)?);
                    Ok(())
                }
            }
        }
    }

    fn clear(&mut self) -> Result<()> {
        self.cards = None;
        journal::restore(&self.path)
    }

    pub(super) fn finish(mut self) -> Result<()> {
        self.clear()
    }
}
