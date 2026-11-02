//! Independent display guardian and recovery entry points.
mod card_rule;
mod client;
mod display;
mod journal;
mod server;
mod trial;

pub(crate) use client::{Guardian, Stopped};
pub use server::run_guardian;
pub use trial::{DesktopTrial, TrialRequest, request_trial};

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn restore(path: &Path) -> Result<()> {
    let shader = journal::restore(path);
    let rule = card_rule::restore(path);
    shader.and(rule)
}

pub(crate) fn private_directory(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && metadata.mode() & 0o077 == 0
            && metadata.uid() == fs::metadata("/proc/self")?.uid(),
        "Runtime directory must be an owned, private directory: {}",
        path.display()
    );
    Ok(())
}

fn lock(path: &Path) -> Result<File> {
    private_directory(path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.join("owner.lock"))?;
    file.try_lock_exclusive()
        .context("Bork display owner is already running")?;
    Ok(file)
}

#[derive(Serialize, Deserialize)]
pub enum Message {
    Heartbeat,
    Display(DisplayState),
    Reset,
    Stop,
}

/// Desired background display, independent of whether a trial or lock suppresses it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DisplayState {
    Work,
    Interference {
        shader: String,
    },
    TestCard {
        remaining: u64,
        total: u64,
        appearance: crate::overtime::Overtime,
    },
}

pub fn stop(path: &Path) -> Result<()> {
    match std::os::unix::net::UnixStream::connect(path.join("control.sock")) {
        Ok(stream) => {
            drop(stream);
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if let Ok(_lock) = lock(path) {
                    return restore(path);
                }
                ensure!(
                    Instant::now() < deadline,
                    "Guardian did not stop; recovery journal remains at {}",
                    path.display()
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
        Err(_) => {
            let _lock = lock(path)?;
            restore(path)
        }
    }
}

pub(crate) fn is_stopped(error: &anyhow::Error) -> bool {
    error.is::<Stopped>()
        || error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
}
