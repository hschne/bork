//! Bounded request transport, owned and polled by the background timer.
use crate::control::{Command, Reply, Status};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use rustix::event::{PollFd, PollFlags, poll};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) struct Server {
    listener: UnixListener,
    path: PathBuf,
    _lock: File,
}

pub(crate) struct Pending {
    pub command: Command,
    stream: UnixStream,
}

impl Pending {
    pub fn respond(mut self, result: &Result<Status>) {
        let reply = result.as_ref().map_err(|error| format!("{error:#}"));
        // Losing a receipt never rolls back an already-applied command.
        if let Ok(bytes) = serde_json::to_vec(&reply) {
            let _ = self.stream.write_all(&bytes);
        }
    }
}

impl Server {
    pub fn bind(runtime: &Path) -> Result<Self> {
        crate::recovery::private_directory(runtime)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(runtime.join("timer.lock"))?;
        lock.try_lock_exclusive()
            .context("Bork background timer is already running")?;
        let path = runtime.join("timer.sock");
        match fs::remove_file(&path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path,
            _lock: lock,
        })
    }

    /// Sleep only until a command arrives; status requests must not postpone pause.
    pub fn wait(&self, timeout: Duration) -> Result<()> {
        let mut fds = [PollFd::new(&self.listener, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout.try_into()?)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn poll(&self) -> Result<Option<Pending>> {
        let (mut stream, _) = match self.listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_millis(100)))?;
        let request = (|| -> Result<Command> {
            let mut bytes = Vec::new();
            (&mut stream).take(1025).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 1024, "Timer request too large");
            Ok(serde_json::from_slice(&bytes)?)
        })();
        match request {
            Ok(command) => Ok(Some(Pending { command, stream })),
            Err(error) => {
                let reply: Reply = Err(format!("{error:#}"));
                let _ = stream.write_all(&serde_json::to_vec(&reply)?);
                Ok(None)
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests;
