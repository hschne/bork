//! Editor-side trial ownership: borrow the running guardian or own a temporary one.
use super::{Guardian, private_directory};
use crate::hyprland::Hyprland;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Serialize, Deserialize)]
pub enum TrialRequest {
    Start {
        shader: String,
    },
    StartCard {
        remaining: u64,
        total: u64,
        appearance: crate::overtime::Overtime,
    },
    Cancel {
        id: u64,
    },
}

pub(super) type TrialReply = std::result::Result<u64, String>;

pub fn request_trial(path: &Path, request: &TrialRequest) -> Result<u64> {
    use std::net::Shutdown;
    let mut stream = std::os::unix::net::UnixStream::connect(path.join("trial.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(&serde_json::to_vec(request)?)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.take(65537).read_to_end(&mut response)?;
    ensure!(response.len() <= 65536, "Trial response too large");
    let response: TrialReply = serde_json::from_slice(&response)?;
    response.map_err(anyhow::Error::msg)
}

/// A trial can borrow the background guardian without touching its controller.
/// If no background owner exists, the editor owns a temporary guardian instead.
pub struct DesktopTrial {
    path: PathBuf,
    id: Option<u64>,
    owner: Option<Guardian>,
    deadline: Duration,
}

impl DesktopTrial {
    pub fn start(hypr: &Hyprland, shader: String) -> Result<Self> {
        Self::start_request(hypr, TrialRequest::Start { shader })
    }

    pub fn start_card(
        hypr: &Hyprland,
        remaining: u64,
        total: u64,
        appearance: crate::overtime::Overtime,
    ) -> Result<Self> {
        Self::start_request(
            hypr,
            TrialRequest::StartCard {
                remaining,
                total,
                appearance,
            },
        )
    }

    fn start_request(hypr: &Hyprland, request: TrialRequest) -> Result<Self> {
        let path = hypr.runtime_directory()?;
        private_directory(&path)?;
        // Retry only a definite failure to connect. An uncertain mutation is never resent.
        let (owner, id) = match request_trial(&path, &request) {
            Ok(id) => (None, id),
            Err(error)
                if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    )
                }) =>
            {
                let owner = Guardian::spawn(hypr)?;
                let id = request_trial(&path, &request)?;
                (Some(owner), id)
            }
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            id: Some(id),
            owner,
            deadline: crate::clock::boot_time() + Duration::from_secs(5),
        })
    }

    /// UI polling is only for status and owned-parent liveness; the guardian
    /// enforces the actual deadline independently of repainting.
    pub fn poll(&mut self) -> Result<bool> {
        self.heartbeat()?;
        Ok(crate::clock::boot_time() >= self.deadline)
    }

    pub fn heartbeat(&mut self) -> Result<()> {
        if let Some(owner) = self.owner.as_mut() {
            owner.heartbeat()?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        let cancelled = if let Some(id) = self.id.take() {
            if self.path.join("trial.sock").exists() {
                request_trial(&self.path, &TrialRequest::Cancel { id }).map(|_| ())
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        let stopped = if let Some(owner) = self.owner.take() {
            owner.finish()
        } else {
            Ok(())
        };
        cancelled.and(stopped)
    }
}

impl Drop for DesktopTrial {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let _ = request_trial(&self.path, &TrialRequest::Cancel { id });
        }
        // An owned guardian then sees EOF and restores independently. A borrowed
        // guardian continues the real cycle, including debt accrued during tuning.
    }
}
