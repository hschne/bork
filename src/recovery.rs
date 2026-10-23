//! The guardian is a second invocation of this same executable, and the only display writer.
//! Parent EOF or a missed heartbeat restores state independently of destructors/signals.
use crate::hyprland::Hyprland;
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub const SHADER: &str = "decoration:screen_shader";
const DAMAGE: &str = "debug:damage_tracking";
const VFR: &str = "debug:vfr";

#[derive(Serialize, Deserialize, Debug)]
struct Change {
    key: String,
    before: String,
    installed: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Journal {
    socket: PathBuf,
    changes: Vec<Change>,
}

pub fn private_directory(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir()
            && meta.mode() & 0o077 == 0
            && meta.uid() == fs::metadata("/proc/self")?.uid(),
        "Runtime directory must be an owned, private directory: {}",
        path.display()
    );
    Ok(())
}

pub fn lock(path: &Path) -> Result<File> {
    private_directory(path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.join("owner.lock"))?;
    file.try_lock_exclusive()
        .context("Signal Loss display owner is already running")?;
    Ok(file)
}

fn write_journal(path: &Path, journal: &Journal) -> Result<()> {
    let temp = path.join("journal.new");
    let mut file = File::create(&temp)?;
    serde_json::to_writer(&mut file, journal)?;
    file.sync_all()?;
    fs::rename(temp, path.join("journal.json"))?;
    File::open(path)?.sync_all()?;
    Ok(())
}

/// Compare-and-restore: never replace a value subsequently changed by the user.
/// Unrestored/conflicting entries remain on disk, so failure is visible and retryable.
pub fn restore(path: &Path) -> Result<()> {
    let journal_path = path.join("journal.json");
    if !journal_path.exists() {
        return Ok(());
    }
    let mut journal: Journal = serde_json::from_reader(File::open(&journal_path)?)?;
    let hypr = Hyprland {
        socket: journal.socket.clone(),
    };
    let mut failures = Vec::new();
    for index in (0..journal.changes.len()).rev() {
        let change = &journal.changes[index];
        let result = (|| -> Result<()> {
            let current = hypr.get(&change.key)?;
            if current == change.before {
                return Ok(());
            }
            ensure!(
                current == change.installed,
                "{} changed externally; leaving it untouched",
                change.key
            );
            hypr.set(&change.key, &change.before)?;
            ensure!(
                hypr.get(&change.key)? == change.before,
                "{} restoration did not stick",
                change.key
            );
            Ok(())
        })();
        match result {
            Ok(()) => {
                journal.changes.remove(index);
                write_journal(path, &journal)?;
            }
            Err(error) => failures.push(format!("{error:#}")),
        }
    }
    if journal.changes.is_empty() {
        fs::remove_file(journal_path)?;
        let _ = fs::remove_file(path.join("effect.frag"));
        Ok(())
    } else {
        bail!(
            "Recovery incomplete: {}. Journal retained at {}",
            failures.join("; "),
            journal_path.display()
        )
    }
}

fn install(path: &Path, hypr: &Hyprland, shader: &str) -> Result<()> {
    // Fully restore any previous installation before taking a fresh exact snapshot.
    restore(path)?;
    fs::write(path.join("effect.frag"), shader)?;
    let shader_path = path.join("effect.frag");
    let shader_path = shader_path
        .to_str()
        .context("Hyprland shader paths must be UTF-8")?;
    let mut journal = Journal {
        socket: hypr.socket.clone(),
        changes: Vec::new(),
    };
    // Disable damage tracking before compiling a shader which uses the time uniform.
    // Restoring in reverse removes the shader before re-enabling damage tracking.
    for (key, value) in [(DAMAGE, "0"), (VFR, "0"), (SHADER, shader_path)] {
        let before = hypr.get(key)?;
        journal.changes.push(Change {
            key: key.into(),
            before,
            installed: value.into(),
        });
        // Write-ahead handles an ambiguous IPC result or process death during mutation.
        write_journal(path, &journal)?;
        hypr.set(key, value)?;
        ensure!(
            hypr.get(key)? == value,
            "{key} did not retain the installed value"
        );
    }
    Ok(())
}

fn update(path: &Path, hypr: &Hyprland, shader: &str) -> Result<()> {
    let journal: Journal = serde_json::from_reader(File::open(path.join("journal.json"))?)?;
    ensure!(
        journal.changes.len() == 3,
        "Incomplete installation journal"
    );
    for change in &journal.changes {
        ensure!(
            hypr.get(&change.key)? == change.installed,
            "{} changed externally; stopping interference",
            change.key
        );
    }
    let temporary = path.join("effect.new");
    fs::write(&temporary, shader)?;
    fs::rename(temporary, path.join("effect.frag"))?;
    hypr.set(
        SHADER,
        path.join("effect.frag")
            .to_str()
            .context("Invalid shader path")?,
    )
}

#[derive(Serialize, Deserialize)]
pub enum Message {
    Heartbeat,
    Install { shader: String, trial: bool },
    Update { shader: String },
    TestCard { remaining: u64 },
    Clear,
    Stop,
}

#[derive(Debug)]
pub struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Signal Loss display owner stopped")
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
    pub fn spawn(path: &Path, hypr: &Hyprland) -> Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("guardian")
            .arg(path)
            .arg(&hypr.socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().context("Guardian stdout unavailable")?);
        let mut guardian = Self {
            path: path.to_owned(),
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

    pub fn send(&mut self, message: &Message) -> Result<()> {
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
            Err(error)
                if error.is::<Stopped>()
                    || error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe) =>
            {
                Ok(())
            }
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

#[derive(Serialize, Deserialize)]
pub enum TrialRequest {
    Start { shader: String },
    Cancel { id: u64 },
}

#[derive(Serialize, Deserialize)]
struct TrialReply {
    id: Option<u64>,
    error: Option<String>,
}

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
    if let Some(error) = response.error {
        bail!(error);
    }
    response.id.context("Missing trial receipt")
}

/// A trial can borrow the background guardian without touching its controller.
/// If no background owner exists, the editor owns a temporary guardian instead.
pub struct DesktopTrial {
    path: PathBuf,
    id: Option<u64>,
    owner: Option<Guardian>,
}

impl DesktopTrial {
    pub fn start(hypr: &Hyprland, shader: String) -> Result<Self> {
        let path = hypr.runtime_directory()?;
        private_directory(&path)?;
        let request = TrialRequest::Start { shader };
        if path.join("trial.sock").exists() {
            match request_trial(&path, &request) {
                Ok(id) => {
                    return Ok(Self {
                        path,
                        id: Some(id),
                        owner: None,
                    });
                }
                Err(error)
                    if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                        matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                        )
                    }) => {}
                Err(error) => return Err(error), // An uncertain request is never resent.
            }
        }
        let owner = Some(Guardian::spawn(&path, hypr)?);
        let id = request_trial(&path, &request)?;
        Ok(Self {
            path,
            id: Some(id),
            owner,
        })
    }

    pub fn heartbeat(&mut self) -> Result<()> {
        if let Some(owner) = self.owner.as_mut() {
            owner.send(&Message::Heartbeat)?;
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

#[derive(Clone)]
enum DesiredDisplay {
    Work,
    Shader(String),
    Card(u64),
}

fn apply_display(
    path: &Path,
    hypr: &Hyprland,
    desired: &DesiredDisplay,
    overlay: &mut Option<crate::wayland::Wayland>,
) -> Result<()> {
    if hypr.locked()? {
        overlay.take();
        return restore(path);
    }
    match desired {
        DesiredDisplay::Work => {
            overlay.take();
            restore(path)
        }
        DesiredDisplay::Shader(shader) => {
            overlay.take();
            if path.join("journal.json").exists() {
                update(path, hypr, shader)
            } else {
                install(path, hypr, shader)
            }
        }
        DesiredDisplay::Card(remaining) => {
            restore(path)?;
            if let Some(overlay) = overlay.as_mut() {
                overlay.countdown(*remaining)
            } else {
                *overlay = Some(crate::wayland::Wayland::connect(true, *remaining)?);
                Ok(())
            }
        }
    }
}

/// Internal process entry point. Must never be invoked directly as a user action.
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
    let trial_listener = UnixListener::bind(&trial_control)?;
    trial_listener.set_nonblocking(true)?;
    let hypr = Hyprland { socket };
    hypr.check()?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            let mut line = String::new();
            match reader.by_ref().take(1024 * 1024).read_line(&mut line) {
                Ok(0) | Err(_) => break,
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
    let mut trial: Option<(u64, Duration)> = None;
    let mut next_trial_id = 0u64;
    let mut desired = DesiredDisplay::Work;
    let mut overlay: Option<crate::wayland::Wayland> = None;
    let mut last_lock_check = Instant::now();
    let result = (|| -> Result<()> {
        loop {
            if let Some(overlay) = overlay.as_mut() {
                overlay.dispatch()?;
            }
            // Keep emergency stop independent from the editor request protocol.
            if listener.accept().is_ok() {
                break;
            }
            if last_heartbeat.elapsed() > Duration::from_secs(2) {
                break;
            }
            if last_lock_check.elapsed() >= Duration::from_millis(250)
                && (overlay.is_some() || path.join("journal.json").exists())
            {
                last_lock_check = Instant::now();
                if hypr.locked()? {
                    overlay.take();
                    trial = None;
                    restore(path)?;
                }
            }
            if trial.is_some_and(|(_, deadline)| crate::wayland::boot_time() >= deadline) {
                trial = None;
                apply_display(path, &hypr, &desired, &mut overlay)?;
            }
            if let Ok((mut stream, _)) = trial_listener.accept() {
                stream.set_read_timeout(Some(Duration::from_millis(100)))?;
                stream.set_write_timeout(Some(Duration::from_millis(100)))?;
                let mut bytes = Vec::new();
                let mut changed = false;
                let action = (|| -> Result<u64> {
                    (&mut stream).take(65537).read_to_end(&mut bytes)?;
                    ensure!(bytes.len() <= 65536, "Trial request too large");
                    match serde_json::from_slice::<TrialRequest>(&bytes)? {
                        TrialRequest::Start { shader } => {
                            ensure!(trial.is_none(), "A desktop trial is already active");
                            ensure!(!hypr.locked()?, "Session is locked; desktop trial refused");
                            changed = true;
                            apply_display(
                                path,
                                &hypr,
                                &DesiredDisplay::Shader(shader),
                                &mut overlay,
                            )?;
                            next_trial_id += 1;
                            trial = Some((
                                next_trial_id,
                                crate::wayland::boot_time() + Duration::from_secs(5),
                            ));
                            Ok(next_trial_id)
                        }
                        TrialRequest::Cancel { id } => {
                            if trial.is_some_and(|(active, _)| active == id) {
                                changed = true;
                                trial = None;
                                apply_display(path, &hypr, &desired, &mut overlay)?;
                            }
                            Ok(id)
                        }
                    }
                })();
                let reply = match &action {
                    Ok(id) => TrialReply {
                        id: Some(*id),
                        error: None,
                    },
                    Err(error) => TrialReply {
                        id: None,
                        error: Some(format!("{error:#}")),
                    },
                };
                let _ = stream.write_all(&serde_json::to_vec(&reply)?);
                // Malformed/busy requests cannot kill the background cycle, but a
                // failed mutation must stop and recover the display immediately.
                if changed {
                    action?;
                }
                last_heartbeat = Instant::now();
            }
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(message) => {
                    let stop = matches!(message, Message::Stop);
                    let action = match message {
                        Message::Install {
                            shader,
                            trial: true,
                        } => {
                            ensure!(trial.is_none(), "A desktop trial is already active");
                            let result = apply_display(
                                path,
                                &hypr,
                                &DesiredDisplay::Shader(shader),
                                &mut overlay,
                            );
                            if result.is_ok() {
                                next_trial_id += 1;
                                trial = Some((
                                    next_trial_id,
                                    crate::wayland::boot_time() + Duration::from_secs(5),
                                ));
                            }
                            result
                        }
                        Message::Install {
                            shader,
                            trial: false,
                        }
                        | Message::Update { shader } => {
                            desired = DesiredDisplay::Shader(shader);
                            if trial.is_none() {
                                apply_display(path, &hypr, &desired, &mut overlay)
                            } else {
                                Ok(())
                            }
                        }
                        Message::TestCard { remaining } => {
                            desired = DesiredDisplay::Card(remaining);
                            if trial.is_none() {
                                apply_display(path, &hypr, &desired, &mut overlay)
                            } else {
                                Ok(())
                            }
                        }
                        Message::Clear => {
                            desired = DesiredDisplay::Work;
                            if trial.is_none() {
                                apply_display(path, &hypr, &desired, &mut overlay)
                            } else {
                                Ok(())
                            }
                        }
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
    drop(overlay);
    let cleanup = restore(path);
    let _ = fs::remove_file(control);
    let _ = fs::remove_file(trial_control);
    if result.is_ok() && cleanup.is_ok() {
        let _ = writeln!(std::io::stdout(), "stopped");
    }
    result.and(cleanup)
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
