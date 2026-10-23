//! Background timer control. JSON is the CLI contract; display recovery stays independent.
mod server;
pub(crate) use server::Server;

use crate::{
    cycle::{DesiredPhase, Mode, Snapshot},
    recovery,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Status,
    Pause,
    Resume,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Paused,
    Resting,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Work,
    Grace,
    Overtime,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub state: State,
    pub phase: Option<Phase>,
    pub work_seconds: Option<u64>,
    pub remaining_seconds: Option<u64>,
    pub break_remaining_seconds: Option<u64>,
    pub glitchiness: f32,
}

impl Status {
    pub fn stopped() -> Self {
        Self {
            state: State::Stopped,
            phase: None,
            work_seconds: None,
            remaining_seconds: None,
            break_remaining_seconds: None,
            glitchiness: 0.0,
        }
    }

    pub(crate) fn from_snapshot(snapshot: Snapshot) -> Self {
        let (phase, level) = match snapshot.phase {
            DesiredPhase::Work => (Phase::Work, 0.0),
            DesiredPhase::Interference(level) => (Phase::Grace, level.value()),
            DesiredPhase::TestCard { .. } => (Phase::Overtime, 0.0),
        };
        let state = match snapshot.mode {
            Mode::Running => State::Running,
            Mode::Paused => State::Paused,
            Mode::Resting => State::Resting,
        };
        Self {
            state,
            phase: Some(phase),
            work_seconds: Some(snapshot.worked),
            remaining_seconds: Some(snapshot.phase_remaining),
            break_remaining_seconds: Some(snapshot.break_remaining),
            glitchiness: if state == State::Paused { 0.0 } else { level },
        }
    }
}

type Reply = std::result::Result<Status, String>;

/// Commands acknowledge only after the background has applied the requested display state.
/// Reset falls back to independent recovery if the timer is absent or unresponsive.
pub fn execute(runtime: &Path, command: Command) -> Result<Status> {
    match request(runtime, command) {
        Ok(status) => Ok(status),
        Err(error) if command == Command::Reset => {
            recovery::stop(runtime).with_context(|| {
                format!("Timer reset failed ({error:#}); emergency recovery failed")
            })?;
            Ok(Status::stopped())
        }
        Err(error) if missing_connection(&error) => {
            if command == Command::Status {
                Ok(Status::stopped())
            } else {
                Err(error.context("Signal Loss background timer is not running"))
            }
        }
        Err(error) => Err(error),
    }
}

fn missing_connection(error: &anyhow::Error) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|error| {
        matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
        )
    })
}

fn request(runtime: &Path, command: Command) -> Result<Status> {
    let mut stream = UnixStream::connect(runtime.join("timer.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_millis(250)))?;
    stream.write_all(&serde_json::to_vec(&command)?)?;
    stream.shutdown(Shutdown::Write)?;
    let mut bytes = Vec::new();
    stream.take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "Timer response too large");
    let reply: Reply = serde_json::from_slice(&bytes).context("Invalid timer response")?;
    reply.map_err(anyhow::Error::msg)
}
