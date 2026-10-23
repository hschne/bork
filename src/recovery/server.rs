//! Bounded guardian transport. DisplayOwner owns lifecycle decisions and resources.
use super::{
    Message,
    display::{DisplayOwner, TrialDecision},
    lock, restore,
    trial::{TrialReply, TrialRequest},
};
use crate::hyprland::Hyprland;
use anyhow::{Result, ensure};
use std::{
    fs,
    io::{BufRead, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub fn run_guardian(path: &Path, socket: PathBuf) -> Result<()> {
    let _lock = lock(path)?;
    restore(path)?;
    let control = path.join("control.sock");
    let trial_control = path.join("trial.sock");
    for socket in [&control, &trial_control] {
        if socket.exists() {
            fs::remove_file(socket)?;
        }
    }
    let listener = UnixListener::bind(&control)?;
    listener.set_nonblocking(true)?;
    let trials = UnixListener::bind(&trial_control)?;
    trials.set_nonblocking(true)?;
    let hypr = Hyprland::at_socket(socket);
    hypr.check()?;
    let mut display = DisplayOwner::new(path, hypr);
    // At most one pending command: a stalled display must not create an unbounded queue.
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            let mut line = String::new();
            match reader.by_ref().take(1024 * 1024 + 1).read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(size) if size > 1024 * 1024 => break,
                Ok(_) => match serde_json::from_str::<Message>(&line) {
                    Ok(message) => {
                        if tx.send(message).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
            }
        }
    });
    println!("ok");
    std::io::stdout().flush()?;
    let mut last_heartbeat = Instant::now();
    let result = (|| -> Result<()> {
        loop {
            if listener.accept().is_ok() || last_heartbeat.elapsed() > Duration::from_secs(2) {
                break;
            }
            display.tick()?;
            if let Ok((stream, _)) = trials.accept() {
                handle_trial(stream, &mut display)?;
                // Only parent commands renew the parent lease. Editor traffic must
                // not keep an abandoned background owner alive.
            }
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(message) => {
                    let stop = matches!(message, Message::Stop);
                    let action = match message {
                        Message::Display(state) => display.set_background(state),
                        Message::Reset => display.reset(),
                        Message::Heartbeat | Message::Stop => Ok(()),
                    };
                    last_heartbeat = Instant::now();
                    match &action {
                        Ok(()) => println!("ok"),
                        Err(error) => println!("error: {error:#}"),
                    }
                    std::io::stdout().flush()?;
                    action?;
                    if stop {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    })();
    let cleanup = display.finish();
    let _ = fs::remove_file(control);
    let _ = fs::remove_file(trial_control);
    if result.is_ok() && cleanup.is_ok() {
        let _ = writeln!(std::io::stdout(), "stopped");
    }
    match (result, cleanup) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("Display recovery also failed: {cleanup:#}")))
        }
    }
}

fn handle_trial(mut stream: UnixStream, display: &mut DisplayOwner) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_millis(100)))?;
    let request = (|| -> Result<TrialRequest> {
        let mut bytes = Vec::new();
        (&mut stream).take(65537).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "Trial request too large");
        Ok(serde_json::from_slice(&bytes)?)
    })();
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            let reply: TrialReply = Err(format!("{error:#}"));
            let _ = stream.write_all(&serde_json::to_vec(&reply)?);
            return Ok(());
        }
    };
    let action = display.trial(request);
    let reply: TrialReply = match &action {
        Ok(TrialDecision::Accepted(id)) => Ok(*id),
        Ok(TrialDecision::Busy) => Err("A desktop trial is already active".into()),
        Ok(TrialDecision::Locked) => Err("Session is locked; desktop trial refused".into()),
        Err(error) => Err(format!("{error:#}")),
    };
    // Failure to deliver a receipt cannot undo a mutation. The independent trial
    // deadline still applies; a fatal display failure exits into recovery.
    let _ = stream.write_all(&serde_json::to_vec(&reply)?);
    action.map(|_| ())
}
