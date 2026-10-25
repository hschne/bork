use super::*;
use crate::{
    control::{Phase, State},
    timer::Timer,
};
use std::{net::Shutdown, thread, time::Instant};

#[test]
fn requests_apply_to_the_cycle_and_reply_with_json_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let server = Server::bind(&runtime).unwrap();
    let mut cycle = Timer::new(0);
    for (command, now, expected, worked) in [
        (Command::Pause, 1290, State::Paused, 1290),
        (Command::Status, 5000, State::Paused, 1290),
        (Command::Resume, 5000, State::Running, 1290),
        (Command::Reset, 5100, State::Running, 0),
    ] {
        let mut stream = UnixStream::connect(runtime.join("timer.sock")).unwrap();
        stream
            .write_all(&serde_json::to_vec(&command).unwrap())
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let pending = server.poll().unwrap().unwrap();
        assert_eq!(pending.command, command);
        let frame = cycle.update(
            crate::timer::Observation {
                now,
                ..Default::default()
            },
            &crate::config::Settings::default(),
            Some(pending.command),
        );
        pending.respond(&Ok(frame.status));
        let reply: Reply = serde_json::from_reader(stream).unwrap();
        let status = reply.unwrap();
        assert_eq!(status.state, expected);
        assert_eq!(status.work_seconds, Some(worked));
        assert_eq!(
            status.phase,
            Some(if worked == 0 {
                Phase::Work
            } else {
                Phase::Grace
            })
        );
    }
}

#[test]
fn malformed_and_stalled_requests_are_bounded_and_do_not_kill_the_server() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let server = Server::bind(&runtime).unwrap();
    let mut stream = UnixStream::connect(runtime.join("timer.sock")).unwrap();
    stream.write_all(b"garbage").unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    assert!(server.poll().unwrap().is_none());
    let reply: Reply = serde_json::from_reader(stream).unwrap();
    assert!(reply.is_err());
    let _stalled = UnixStream::connect(runtime.join("timer.sock")).unwrap();
    let before = Instant::now();
    assert!(server.poll().unwrap().is_none());
    assert!(before.elapsed() < Duration::from_secs(1));
    let request =
        thread::spawn(move || crate::control::execute(&runtime, Command::Status).unwrap());
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(pending) = server.poll().unwrap() {
            pending.respond(&Ok(Status::stopped()));
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(request.join().unwrap().state, State::Stopped);
}

#[test]
fn a_second_owner_cannot_replace_the_control_socket() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let owner = Server::bind(&runtime).unwrap();
    assert!(Server::bind(&runtime).is_err());
    assert!(UnixStream::connect(runtime.join("timer.sock")).is_ok());
    drop(owner);
    assert!(!runtime.join("timer.sock").exists());
    assert!(Server::bind(&runtime).is_ok());
}
