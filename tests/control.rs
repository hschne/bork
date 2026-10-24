//! Real CLI + local timer peer. No Wayland connection or display mutations.
use bork::{
    control::{Command as TimerCommand, Phase, State, Status},
    hyprland::Hyprland,
};
use std::{
    fs,
    io::Read,
    net::Shutdown,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Session {
    directory: tempfile::TempDir,
    runtime: PathBuf,
}

impl Session {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let hypr = Hyprland::at_socket(directory.path().join("hypr/cli-fixture/.socket.sock"));
        let runtime = hypr.runtime_directory().unwrap();
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        Self { directory, runtime }
    }
    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bork"));
        command
            .arg(action)
            .env("XDG_RUNTIME_DIR", self.directory.path())
            .env("HYPRLAND_INSTANCE_SIGNATURE", "cli-fixture")
            .env_remove("_BORK_RUNTIME_READY")
            .env("BORK_LIBRARY_PATH", "broken")
            .env("BORK_NIX_DISCOVERY", "broken");
        command
    }
    fn listener(&self) -> UnixListener {
        let listener = UnixListener::bind(self.runtime.join("timer.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    }
}

fn accept(listener: UnixListener) -> std::os::unix::net::UnixStream {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "CLI did not contact the timer");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn every_timer_command_roundtrips_json_without_graphics_initialization() {
    for (action, expected) in [
        ("status", TimerCommand::Status),
        ("pause", TimerCommand::Pause),
        ("resume", TimerCommand::Resume),
        ("reset", TimerCommand::Reset),
    ] {
        let session = Session::new();
        let listener = session.listener();
        let status = Status {
            state: if expected == TimerCommand::Pause {
                State::Paused
            } else {
                State::Running
            },
            phase: Some(Phase::Work),
            work_seconds: Some(42),
            remaining_seconds: Some(1338),
            break_remaining_seconds: Some(300),
            resting_seconds: Some(0),
            glitchiness: 0.0,
        };
        let peer = thread::spawn(move || {
            let mut stream = accept(listener);
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            assert_eq!(
                serde_json::from_slice::<TimerCommand>(&bytes).unwrap(),
                expected
            );
            serde_json::to_writer(&mut stream, &Ok::<_, String>(&status)).unwrap();
            status
        });
        let output = session.command(action).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.lines().count(), 1);
        let actual: Status = serde_json::from_str(&text).unwrap();
        assert_eq!(actual, peer.join().unwrap());
    }
}

#[test]
fn absent_timer_status_and_reset_are_successful_but_pause_and_resume_are_errors() {
    let session = Session::new();
    for action in ["status", "reset"] {
        let output = session.command(action).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let status: Status = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status, Status::stopped());
    }
    for action in ["pause", "resume"] {
        let output = session.command(action).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("not running"));
    }
}

#[test]
fn reset_falls_back_to_recovery_when_the_timer_does_not_reply() {
    let session = Session::new();
    let listener = session.listener();
    let (release, wait) = mpsc::channel();
    let peer = thread::spawn(move || {
        let mut stream = accept(listener);
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(
            serde_json::from_slice::<TimerCommand>(&bytes).unwrap(),
            TimerCommand::Reset
        );
        // Keep the socket open without replying, until the CLI finishes its fallback.
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        stream.shutdown(Shutdown::Both).unwrap();
    });
    let started = Instant::now();
    let output = session.command("reset").output().unwrap();
    release.send(()).unwrap();
    peer.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Status>(&output.stdout)
            .unwrap()
            .state,
        State::Stopped
    );
    assert!(
        session.runtime.join("owner.lock").exists(),
        "Fallback never reached recovery"
    );
}

#[test]
fn the_old_stop_command_is_replaced_rather_than_aliased() {
    let session = Session::new();
    let output = session.command("stop").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
}
