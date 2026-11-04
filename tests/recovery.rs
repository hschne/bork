//! Process + Unix-socket simulation. These tests never connect to the user's compositor.
use bork::{
    effect::{Glitchiness, Interference},
    recovery::{self, DisplayState, Message},
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::DirBuilderExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const ORIGINAL_SHADER: &str = "/someone else's/original.frag";

struct Fake {
    dir: TempDir,
    socket: PathBuf,
    values: Arc<Mutex<HashMap<String, String>>>,
    failure: Arc<Mutex<Option<(String, bool)>>>,
    done: Arc<AtomicBool>,
    locked: Arc<AtomicBool>,
    disconnected: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Fake {
    fn new() -> Self {
        Self::with_lua_parser(false)
    }

    fn with_lua_parser(lua: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("hypr.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let values: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::from([
            ("decoration:screen_shader".into(), ORIGINAL_SHADER.into()),
            ("debug:damage_tracking".into(), "2".into()),
            ("debug:vfr".into(), "1".into()),
        ])));
        let failure: Arc<Mutex<Option<(String, bool)>>> = Arc::default();
        let done = Arc::new(AtomicBool::new(false));
        let v = values.clone();
        let f = failure.clone();
        let d = done.clone();
        let locked = Arc::new(AtomicBool::new(false));
        let disconnected = Arc::new(AtomicBool::new(false));
        let l = locked.clone();
        let disconnected_peer = disconnected.clone();
        let handle = thread::spawn(move || {
            while !d.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                if disconnected_peer.load(Ordering::Relaxed) {
                    continue;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = String::new();
                stream.read_to_string(&mut request).unwrap();
                let response = if request == "j/locked" {
                    serde_json::json!({"locked":l.load(Ordering::Relaxed)}).to_string()
                } else if request == "j/version" {
                    r#"{"version":"0.56.2-fixture"}"#.into()
                } else if let Some(key) = request.strip_prefix("j/getoption ") {
                    let value = v.lock().unwrap()[key].clone();
                    if key == "decoration:screen_shader" {
                        serde_json::json!({"str": value}).to_string()
                    } else if key == "debug:vfr" {
                        serde_json::json!({"bool": value == "1"}).to_string()
                    } else {
                        serde_json::json!({"int": value.parse::<i64>().unwrap()}).to_string()
                    }
                } else if lua && request.starts_with("/keyword ") {
                    // Hyprland 0.57 no longer registers the keyword IPC command.
                    "unknown request".into()
                } else if lua && request.starts_with("/eval hl.config(") {
                    let setting = request
                        .strip_prefix("/eval hl.config({[")
                        .unwrap()
                        .strip_suffix("})")
                        .unwrap();
                    let (key, literal) = setting.split_once("]=").unwrap();
                    let key = serde_json::from_str::<String>(key)
                        .unwrap()
                        .replace('.', ":");
                    let literal: serde_json::Value = serde_json::from_str(literal).unwrap();
                    let value = match literal {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Bool(b) => if b { "1" } else { "0" }.into(),
                        serde_json::Value::Number(n) => n.to_string(),
                        _ => panic!("Unexpected Lua literal"),
                    };
                    v.lock().unwrap().insert(key, value);
                    "ok".into()
                } else if let Some(setting) = request.strip_prefix("/keyword ") {
                    let (key, value) = setting.split_once(' ').unwrap();
                    let fault = {
                        let mut failure = f.lock().unwrap();
                        if failure.as_ref().is_some_and(|(k, _)| k == key) {
                            failure.take()
                        } else {
                            None
                        }
                    };
                    if fault.as_ref().is_none_or(|(_, after)| *after) {
                        v.lock().unwrap().insert(key.into(), value.into());
                    }
                    if fault.is_some() {
                        "injected ambiguous failure".into()
                    } else {
                        "ok".into()
                    }
                } else {
                    panic!("Unexpected request: {request}");
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            dir,
            socket,
            values,
            failure,
            done,
            locked,
            disconnected,
            thread: Some(handle),
        }
    }

    fn runtime(&self) -> PathBuf {
        self.dir.path().join("bork")
    }

    fn original(&self) -> bool {
        let values = self.values.lock().unwrap();
        values["decoration:screen_shader"] == ORIGINAL_SHADER
            && values["debug:damage_tracking"] == "2"
            && values["debug:vfr"] == "1"
    }

    fn wait_original(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.original() || self.runtime().join("journal.json").exists() {
            assert!(
                Instant::now() < deadline,
                "Recovery did not finish: {:?}",
                self.values.lock().unwrap()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

struct Process {
    runtime: PathBuf,
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Process {
    fn start(fake: &Fake) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bork"))
            .arg("guardian")
            .arg(fake.runtime())
            .arg(&fake.socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut p = Self {
            runtime: fake.runtime(),
            child,
            input,
            output,
        };
        assert_eq!(p.reply(), "ok");
        p
    }
    fn reply(&mut self) -> String {
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        line.trim().into()
    }
    fn send(&mut self, message: Message) -> String {
        let input = self.input.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &message).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
        self.reply()
    }
    fn card(&mut self, remaining: u64) -> String {
        self.send(Message::Display(DisplayState::TestCard {
            remaining,
            total: 300,
            appearance: bork::overtime::Overtime::default(),
        }))
    }

    fn install(&mut self, trial: bool) -> String {
        let shader = Interference::default()
            .hyprland_shader_at(Glitchiness::try_from(100.0).unwrap(), 0.0, 0)
            .unwrap();
        if trial {
            recovery::request_trial(&self.runtime, &recovery::TrialRequest::Start { shader })
                .unwrap();
            "ok".into()
        } else {
            self.send(Message::Display(DisplayState::Interference { shader }))
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.wait();
    }
}

#[test]
fn removed_keyword_command_uses_lua_without_killing_guardian() {
    let fake = Fake::with_lua_parser(true);
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    assert!(!fake.original());
    // Subsequent grace-period publications must update the shader, not exit the guardian.
    assert_eq!(process.install(false), "ok");
    assert_eq!(process.send(Message::Heartbeat), "ok");
    assert_eq!(process.send(Message::Reset), "ok");
    fake.wait_original();
    assert_eq!(process.send(Message::Heartbeat), "ok");
}

#[test]
fn parent_eof_recovers_exact_nondefault_settings() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    assert!(!fake.original());
    drop(process); // Parent pipe EOF, not a normal-exit request.
    fake.wait_original();
}

#[test]
fn failed_setup_each_step_before_and_after_mutation_rolls_back() {
    for key in [
        "debug:damage_tracking",
        "debug:vfr",
        "decoration:screen_shader",
    ] {
        for after in [false, true] {
            let fake = Fake::new();
            let mut process = Process::start(&fake);
            *fake.failure.lock().unwrap() = Some((key.into(), after));
            assert!(process.install(false).starts_with("error:"));
            drop(process);
            fake.wait_original();
        }
    }
}

#[test]
fn guardian_sigkill_leaves_journal_for_independent_recovery() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    process.child.kill().unwrap();
    process.child.wait().unwrap();
    recovery::stop(&fake.runtime()).unwrap();
    fake.wait_original();
}

#[test]
fn unrelated_shader_changes_are_not_overwritten() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    fake.values
        .lock()
        .unwrap()
        .insert("decoration:screen_shader".into(), "/user/new.frag".into());
    drop(process);
    let values = fake.values.lock().unwrap();
    assert_eq!(values["decoration:screen_shader"], "/user/new.frag");
    assert_eq!(values["debug:damage_tracking"], "2");
    assert_eq!(values["debug:vfr"], "1");
    assert!(fake.runtime().join("journal.json").exists());
}

#[test]
fn stalled_parent_lease_expires_without_eof() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    fake.wait_original();
    assert!(process.child.wait().unwrap().success());
}

#[test]
fn emergency_socket_stops_even_with_parent_alive() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    recovery::stop(&fake.runtime()).unwrap();
    fake.wait_original();
    assert!(process.child.wait().unwrap().success());
}

#[test]
fn five_second_trial_expires_despite_regular_heartbeats() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    let start = Instant::now();
    assert_eq!(process.install(true), "ok");
    while !fake.original() {
        assert!(start.elapsed() < Duration::from_secs(7));
        assert_eq!(process.send(Message::Heartbeat), "ok");
        thread::sleep(Duration::from_millis(50));
    }
    assert!(start.elapsed() >= Duration::from_secs(5));
    fake.wait_original();
}

#[test]
fn second_owner_is_rejected_without_touching_existing_state() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let result = Command::new(env!("CARGO_BIN_EXE_bork"))
        .arg("guardian")
        .arg(fake.runtime())
        .arg(&fake.socket)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!fake.original());
    drop(process);
    fake.wait_original();
}

#[test]
fn raw_emergency_connection_needs_no_protocol_or_parent_cooperation() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    drop(UnixStream::connect(fake.runtime().join("control.sock")).unwrap());
    fake.wait_original();
}

#[test]
fn guardian_suppresses_effects_on_lock_without_parent_cooperation() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    fake.locked.store(true, Ordering::Relaxed);
    fake.wait_original();
    assert_eq!(process.install(false), "ok");
    assert!(fake.original());
}

#[test]
fn disconnected_compositor_retains_journal_and_recovery_can_retry() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    fake.disconnected.store(true, Ordering::Relaxed);
    process.input.take();
    assert!(!process.child.wait().unwrap().success());
    assert!(fake.runtime().join("journal.json").exists());
    fake.disconnected.store(false, Ordering::Relaxed);
    recovery::stop(&fake.runtime()).unwrap();
    fake.wait_original();
}

#[test]
fn editor_trial_borrows_owner_and_restores_latest_background_state() {
    use recovery::{TrialRequest, request_trial};
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let trial_shader = Interference::default()
        .hyprland_shader_at(Glitchiness::try_from(50.0).unwrap(), 0.0, 0)
        .unwrap();
    let next_shader = Interference::default()
        .hyprland_shader_at(Glitchiness::try_from(10.0).unwrap(), 0.0, 0)
        .unwrap();
    let id = request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: trial_shader.clone(),
        },
    )
    .unwrap();
    assert!(
        request_trial(
            &fake.runtime(),
            &TrialRequest::Start {
                shader: trial_shader.clone()
            }
        )
        .is_err()
    );
    assert_eq!(
        process.send(Message::Display(DisplayState::Interference {
            shader: next_shader.clone()
        })),
        "ok"
    );
    assert_eq!(
        std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap(),
        trial_shader
    );
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id: id + 1 }).unwrap();
    assert_eq!(
        std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap(),
        trial_shader
    );
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id }).unwrap();
    assert_eq!(
        std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap(),
        next_shader
    );
    assert!(!fake.original());
    let id = request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: trial_shader,
        },
    )
    .unwrap();
    assert_eq!(process.send(Message::Display(DisplayState::Work)), "ok");
    assert!(
        !fake.original(),
        "Background Work must not prematurely end the editor's trial"
    );
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id }).unwrap();
    fake.wait_original();
    assert_eq!(
        process.send(Message::Heartbeat),
        "ok",
        "Editor close must not stop background owner"
    );
}

#[test]
fn borrowed_trial_expiry_resumes_newer_background_shader() {
    use recovery::{TrialRequest, request_trial};
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let trial_shader = Interference::default()
        .hyprland_shader_at(Glitchiness::try_from(50.0).unwrap(), 0.0, 0)
        .unwrap();
    let next_shader = Interference::default()
        .hyprland_shader_at(Glitchiness::try_from(20.0).unwrap(), 0.0, 0)
        .unwrap();
    request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: trial_shader.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        process.send(Message::Display(DisplayState::Interference {
            shader: next_shader.clone()
        })),
        "ok"
    );
    let deadline = Instant::now() + Duration::from_millis(5400);
    while Instant::now() < deadline {
        assert_eq!(process.send(Message::Heartbeat), "ok");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap(),
        next_shader
    );
    drop(process);
    fake.wait_original();
}

#[test]
fn unlock_does_not_reinstate_effects_published_before_or_during_lock() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    fake.locked.store(true, Ordering::Relaxed);
    fake.wait_original();
    let shader = "latest background while locked".to_owned();
    assert_eq!(
        process.send(Message::Display(DisplayState::Interference {
            shader: shader.clone()
        })),
        "ok"
    );
    assert!(fake.original());
    fake.locked.store(false, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline {
        assert_eq!(process.send(Message::Heartbeat), "ok");
        assert!(fake.original(), "Unlock reinstated stale interference");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        process.send(Message::Display(DisplayState::Interference { shader })),
        "ok"
    );
    assert!(!fake.original(), "Fresh post-unlock intent was not applied");
}

#[test]
fn unlock_keeps_clean_background_until_fresh_timer_publication() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    fake.locked.store(true, Ordering::Relaxed);
    fake.wait_original();
    assert_eq!(process.send(Message::Display(DisplayState::Work)), "ok");
    fake.locked.store(false, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline {
        assert_eq!(process.send(Message::Heartbeat), "ok");
        assert!(fake.original(), "Unlock reinstated a stale effect");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(process.install(false), "ok");
    assert!(!fake.original(), "Fresh timer intent was not applied");
}

#[test]
fn locking_cancels_trial_and_old_receipt_cannot_cancel_its_replacement() {
    use recovery::{TrialRequest, request_trial};
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let old = request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: "old trial".into(),
        },
    )
    .unwrap();
    fake.locked.store(true, Ordering::Relaxed);
    fake.wait_original();
    assert!(
        request_trial(
            &fake.runtime(),
            &TrialRequest::Start {
                shader: "refused".into()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("locked")
    );
    // Publish normal work while locked so unlock must not restore an ended trial.
    assert_eq!(process.send(Message::Display(DisplayState::Work)), "ok");
    fake.locked.store(false, Ordering::Relaxed);
    let replacement = request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: "replacement".into(),
        },
    )
    .unwrap();
    assert_ne!(old, replacement);
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id: old }).unwrap();
    assert_eq!(
        std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap(),
        "replacement"
    );
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id: replacement }).unwrap();
    fake.wait_original();
}

#[test]
fn malformed_editor_request_does_not_mutate_or_stop_background() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let before = std::fs::read(fake.runtime().join("effect.frag")).unwrap();
    let mut stream = UnixStream::connect(fake.runtime().join("trial.sock")).unwrap();
    stream.write_all(b"not JSON").unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        serde_json::from_str::<Result<u64, String>>(&response)
            .unwrap()
            .is_err()
    );
    assert_eq!(process.send(Message::Heartbeat), "ok");
    assert_eq!(
        std::fs::read(fake.runtime().join("effect.frag")).unwrap(),
        before
    );
}

#[test]
fn failed_editor_mutation_exits_into_exact_recovery() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    *fake.failure.lock().unwrap() = Some(("debug:vfr".into(), true));
    assert!(
        recovery::request_trial(
            &fake.runtime(),
            &recovery::TrialRequest::Start {
                shader: "trial".into()
            }
        )
        .is_err()
    );
    assert!(!process.child.wait().unwrap().success());
    fake.wait_original();
}

#[test]
fn editor_traffic_cannot_renew_a_stalled_parents_lease() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let started = Instant::now();
    while process.child.try_wait().unwrap().is_none() {
        // Invalid requests neither mutate nor count as a parent heartbeat.
        let _ =
            recovery::request_trial(&fake.runtime(), &recovery::TrialRequest::Cancel { id: 123 });
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "Editor kept dead parent lease alive"
        );
        thread::sleep(Duration::from_millis(20));
    }
    fake.wait_original();
}

#[test]
fn reset_cancels_the_trial_clears_background_intent_and_keeps_guardian_alive() {
    use recovery::{TrialRequest, request_trial};
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let id = request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: "trial".into(),
        },
    )
    .unwrap();
    assert_eq!(process.send(Message::Reset), "ok");
    fake.wait_original();
    assert_eq!(process.send(Message::Heartbeat), "ok");
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id }).unwrap();
    assert!(
        fake.original(),
        "An old receipt must not reinstate pre-reset background state"
    );
    let replacement = request_trial(
        &fake.runtime(),
        &TrialRequest::Start {
            shader: "new trial".into(),
        },
    )
    .unwrap();
    request_trial(&fake.runtime(), &TrialRequest::Cancel { id: replacement }).unwrap();
    fake.wait_original();
}

#[test]
fn signal_loss_does_not_restart_on_countdown_and_reset_cancels_it() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    assert_eq!(process.card(300), "ok");
    let first = std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap();
    assert!(first.contains("HOLD_A"));
    assert_eq!(process.card(299), "ok");
    assert_eq!(
        std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap(),
        first
    );
    assert_eq!(process.send(Message::Reset), "ok");
    fake.wait_original();
    assert_eq!(process.card(300), "ok");
    let second = std::fs::read_to_string(fake.runtime().join("effect.frag")).unwrap();
    assert_ne!(
        first, second,
        "Each overtime entry needs a new random identity"
    );
    assert_eq!(process.send(Message::Display(DisplayState::Work)), "ok");
    fake.wait_original();
    // Waiting beyond the old animation deadline must not resurrect the card.
    let deadline = Instant::now() + Duration::from_millis(1300);
    while Instant::now() < deadline {
        assert_eq!(process.send(Message::Heartbeat), "ok");
        assert!(fake.original());
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn signal_loss_lock_eof_and_emergency_stop_restore_exact_settings() {
    for stop in ["lock", "eof", "emergency"] {
        let fake = Fake::new();
        let mut process = Process::start(&fake);
        assert_eq!(process.card(300), "ok");
        assert!(!fake.original());
        match stop {
            "lock" => {
                fake.locked.store(true, Ordering::Relaxed);
                fake.wait_original();
                assert_eq!(process.send(Message::Display(DisplayState::Work)), "ok");
                fake.locked.store(false, Ordering::Relaxed);
            }
            "eof" => process.input.take().map(drop).unwrap(),
            _ => recovery::stop(&fake.runtime()).unwrap(),
        }
        fake.wait_original();
    }
}

#[test]
fn reset_without_a_timer_uses_independent_guardian_recovery() {
    let fake = Fake::new();
    let mut process = Process::start(&fake);
    assert_eq!(process.install(false), "ok");
    let status = bork::control::execute(&fake.runtime(), bork::control::Command::Reset).unwrap();
    assert_eq!(status.state, bork::control::State::Stopped);
    fake.wait_original();
    assert!(process.child.wait().unwrap().success());
}

#[test]
fn independent_recovery_disables_only_the_private_card_rule_lease() {
    let fake = Fake::new();
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(fake.runtime())
        .unwrap();
    let key = "layerrule[bork-0123456789abcdef]:match:namespace";
    fake.values.lock().unwrap().insert(key.into(), "1".into());
    fake.values
        .lock()
        .unwrap()
        .insert("layerrule[user-rule]:enable".into(), "1".into());
    std::fs::write(
        fake.runtime().join("card-rule.json"),
        serde_json::to_vec(&serde_json::json!({
            "socket":fake.socket,
            "namespace":"bork-0123456789abcdef"
        }))
        .unwrap(),
    )
    .unwrap();
    recovery::stop(&fake.runtime()).unwrap();
    assert_eq!(fake.values.lock().unwrap()[key], "a^");
    assert_eq!(
        fake.values.lock().unwrap()["layerrule[user-rule]:enable"],
        "1"
    );
    assert!(fake.original());
    assert!(!fake.runtime().join("card-rule.json").exists());
}

#[test]
fn uncertain_card_rule_recovery_retains_its_lease_for_retry() {
    for after in [false, true] {
        let fake = Fake::new();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(fake.runtime())
            .unwrap();
        let key = "layerrule[bork-0123456789abcdef]:match:namespace";
        fake.values.lock().unwrap().insert(key.into(), "1".into());
        std::fs::write(
            fake.runtime().join("card-rule.json"),
            serde_json::to_vec(&serde_json::json!({
                "socket":fake.socket,"namespace":"bork-0123456789abcdef"
            }))
            .unwrap(),
        )
        .unwrap();
        *fake.failure.lock().unwrap() = Some((key.into(), after));
        assert!(recovery::stop(&fake.runtime()).is_err());
        assert!(fake.runtime().join("card-rule.json").exists());
        recovery::stop(&fake.runtime()).unwrap();
        assert_eq!(fake.values.lock().unwrap()[key], "a^");
        assert!(!fake.runtime().join("card-rule.json").exists());
    }
}

#[test]
fn card_rule_recovery_cannot_name_a_user_rule() {
    let fake = Fake::new();
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(fake.runtime())
        .unwrap();
    std::fs::write(
        fake.runtime().join("card-rule.json"),
        serde_json::to_vec(&serde_json::json!({
            "socket":fake.socket,"namespace":"user-rule"
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(recovery::stop(&fake.runtime()).is_err());
    assert!(fake.runtime().join("card-rule.json").exists());
    assert!(fake.original());
}
