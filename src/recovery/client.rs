//! Parent-side guardian process ownership. EOF initiates independent recovery.
use super::{DisplayState, Message, lock, restore};
use crate::hyprland::Hyprland;
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

#[derive(Debug)]
pub struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bork display owner stopped")
    }
}
impl std::error::Error for Stopped {}

pub struct Guardian {
    path: PathBuf,
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Guardian {
    pub fn spawn(hypr: &Hyprland) -> Result<Self> {
        let path = hypr.runtime_directory()?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("guardian")
            .arg(&path)
            .arg(hypr.socket_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().context("Guardian stdout unavailable")?);
        let mut guardian = Self {
            path,
            child,
            input,
            output,
        };
        guardian.reply()?;
        Ok(guardian)
    }

    fn reply(&mut self) -> Result<()> {
        let mut reply = String::new();
        self.output.by_ref().take(65536).read_line(&mut reply)?;
        if reply.is_empty() || reply.trim() == "stopped" {
            return Err(Stopped.into());
        }
        ensure!(reply.trim() == "ok", "Guardian failed: {}", reply.trim());
        Ok(())
    }

    pub fn set_display(&mut self, state: DisplayState) -> Result<()> {
        self.send(&Message::Display(state))
    }

    pub fn reset(&mut self) -> Result<()> {
        self.send(&Message::Reset)
    }

    pub fn heartbeat(&mut self) -> Result<()> {
        self.send(&Message::Heartbeat)
    }

    fn send(&mut self, message: &Message) -> Result<()> {
        let input = self.input.as_mut().context("Guardian already stopped")?;
        let bytes = serde_json::to_vec(message)?;
        input.write_all(&bytes)?;
        input.write_all(b"\n")?;
        input.flush()?;
        self.reply()
    }

    pub fn finish(mut self) -> Result<()> {
        let result = self.send(&Message::Stop);
        self.input.take();
        let status = self.child.wait()?;
        if !status.success() {
            let _lock = lock(&self.path)?;
            restore(&self.path)?;
            let _ = fs::remove_file(self.path.join("control.sock"));
            let _ = fs::remove_file(self.path.join("trial.sock"));
            bail!("Display guardian exited unexpectedly; prior settings restored");
        }
        match result {
            Err(error) if super::is_stopped(&error) => Ok(()),
            other => other,
        }
    }
}

impl Drop for Guardian {
    fn drop(&mut self) {
        // EOF wakes independent recovery. Reap it; never kill the recovery process.
        let owned_pipe = self.input.take().is_some();
        if let Ok(status) = self.child.wait()
            && owned_pipe
            && !status.success()
            && let Ok(_lock) = lock(&self.path)
        {
            let _ = restore(&self.path);
        }
    }
}
