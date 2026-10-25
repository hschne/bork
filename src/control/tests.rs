use super::*;
use std::{os::unix::net::UnixListener, sync::mpsc, thread, time::Instant};

#[test]
fn every_command_roundtrips_the_socket_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    crate::recovery::private_directory(&runtime).unwrap();
    for command in [
        Command::Status,
        Command::Pause,
        Command::Resume,
        Command::Reset,
    ] {
        let listener = UnixListener::bind(runtime.join("timer.sock")).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let received: Command = serde_json::from_reader(&mut stream).unwrap();
            assert_eq!(received, command);
            let status = Status {
                state: if command == Command::Pause {
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
            serde_json::to_writer(&mut stream, &Ok::<_, String>(&status)).unwrap();
            status
        });
        let status = execute(&runtime, command).unwrap();
        assert_eq!(status, peer.join().unwrap());
        std::fs::remove_file(runtime.join("timer.sock")).unwrap();
    }
}

#[test]
fn absent_timer_status_and_reset_succeed_but_pause_and_resume_fail() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    crate::recovery::private_directory(&runtime).unwrap();
    for command in [Command::Status, Command::Reset] {
        assert_eq!(execute(&runtime, command).unwrap(), Status::stopped());
    }
    for command in [Command::Pause, Command::Resume] {
        assert!(
            execute(&runtime, command)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
    }
}

#[test]
fn reset_falls_back_to_recovery_when_the_timer_does_not_reply() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    crate::recovery::private_directory(&runtime).unwrap();
    let listener = UnixListener::bind(runtime.join("timer.sock")).unwrap();
    let (release, wait) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert_eq!(
            serde_json::from_reader::<_, Command>(&mut stream).unwrap(),
            Command::Reset
        );
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let started = Instant::now();
    let result = execute(&runtime, Command::Reset);
    release.send(()).unwrap();
    peer.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(result.unwrap(), Status::stopped());
    assert!(
        runtime.join("owner.lock").exists(),
        "Recovery was not reached"
    );
}

#[test]
fn malformed_and_failed_replies_are_errors_not_stopped_status() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    crate::recovery::private_directory(&runtime).unwrap();
    for reply in [
        b"not JSON".to_vec(),
        serde_json::to_vec(&Err::<Status, _>("Timer unavailable")).unwrap(),
    ] {
        let listener = UnixListener::bind(runtime.join("timer.sock")).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let _: Command = serde_json::from_reader(&mut stream).unwrap();
            stream.write_all(&reply).unwrap();
        });
        assert!(execute(&runtime, Command::Status).is_err());
        peer.join().unwrap();
        std::fs::remove_file(runtime.join("timer.sock")).unwrap();
    }
}
