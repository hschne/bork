//! Only run by nested-session.sh in a namespace with no access to the active desktop.
mod support;

use bork::{
    effect::{Glitchiness, Interference},
    hyprland::Hyprland,
    recovery::{DisplayState, Message},
    wayland::InputMonitor,
};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn isolated() -> PathBuf {
    assert_eq!(std::env::var("BORK_ISOLATED").as_deref(), Ok("1"));
    let path = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    assert!(
        path.starts_with("/tmp")
            && path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("sl.")
    );
    path
}

fn pump(wayland: &mut InputMonitor, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        wayland.dispatch().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
}

fn capture(name: &str, output: &str) -> PathBuf {
    let path = isolated().join(name);
    let mut command = Command::new(std::env::var_os("BORK_GRIM").unwrap());
    if !output.is_empty() {
        command.arg("-o").arg(output);
    }
    let status = command.arg(&path).status().unwrap();
    assert!(status.success());
    path
}

fn image(path: &std::path::Path) -> (usize, usize, Vec<u8>) {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()))
        .read_info()
        .unwrap();
    let mut bytes = vec![0; decoder.output_buffer_size().unwrap()];
    let info = decoder.next_frame(&mut bytes).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgb);
    bytes.truncate(info.buffer_size());
    (info.width as usize, info.height as usize, bytes)
}

struct Guardian {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Guardian {
    fn start() -> Self {
        let hypr = Hyprland::from_env().unwrap();
        let runtime = hypr.runtime_directory().unwrap();
        let mut child = Command::new(
            std::env::var_os("BORK_TEST_BINARY")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_bork").into()),
        )
        .arg("guardian")
        .arg(runtime)
        .arg(hypr.socket_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut p = Self {
            child,
            input,
            output,
        };
        p.reply();
        p
    }
    fn reply(&mut self) {
        let mut response = String::new();
        self.output.read_line(&mut response).unwrap();
        assert_eq!(response.trim(), "ok", "Guardian response: {response}");
    }
    fn send(&mut self, message: Message) {
        let input = self.input.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &message).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
        self.reply();
    }
    fn heartbeat_for(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.send(Message::Heartbeat);
            thread::sleep(Duration::from_millis(50));
        }
    }
}
impl Drop for Guardian {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "requires tests/nested-session.sh; never run on the active desktop"]
fn compositor_renders_cards_intercepts_input_and_restores_on_exit() {
    let runtime = isolated();
    let hypr = Hyprland::from_env().unwrap();
    println!("Nested compositor: {}", hypr.check().unwrap());
    hypr.request("/output create headless proof-one").unwrap();
    configure_output(&hypr, "proof-one,1280x720@30,0x0,1");
    hypr.request("/output create headless proof-two").unwrap();
    configure_output(&hypr, "proof-two,1280x720@30,1280x0,1");
    // Wait for monitor creation before testing surface enumeration.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let monitors = hypr.json("monitors").unwrap();
        if monitors.as_array().unwrap().len() >= 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Second monitor missing: {monitors}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    println!("Outputs: {}", hypr.json("monitors").unwrap());
    thread::sleep(Duration::from_millis(200));
    let mut target = support::Desktop::new();
    target.click();
    target.key();
    thread::sleep(Duration::from_millis(50));
    target.dispatch();
    assert!(
        target.state.keys > 0 && target.state.clicks > 0,
        "Disposable target must receive input before overlay"
    );
    let original = Interference::default()
        .hyprland_shader_at(Glitchiness::try_from(0.0).unwrap(), 0.0, 0)
        .unwrap();
    fs::write(runtime.join("prior.frag"), original).unwrap();
    hypr.set(
        "decoration:screen_shader",
        runtime.join("prior.frag").to_str().unwrap(),
    )
    .unwrap();
    hypr.set("debug:damage_tracking", "1").unwrap();
    hypr.set("debug:vfr", "1").unwrap();
    let before: Vec<_> = [
        "decoration:screen_shader",
        "debug:damage_tracking",
        "debug:vfr",
    ]
    .iter()
    .map(|key| ((*key).to_string(), hypr.get(key).unwrap()))
    .collect();
    let mut owner = Guardian::start();
    owner.send(Message::Display(DisplayState::Interference {
        shader: Interference::default()
            .hyprland_shader_at(Glitchiness::MAXIMUM, 0.0, 41)
            .unwrap(),
    }));
    owner.heartbeat_for(Duration::from_millis(250));
    assert!(hypr.request("/configerrors").unwrap().trim().is_empty());
    assert_ne!(hypr.get("decoration:screen_shader").unwrap(), before[0].1);
    owner.send(Message::Display(DisplayState::TestCard {
        remaining: 272,
        total: 300,
        appearance: bork::overtime::Overtime::default(),
    }));
    let loss = fs::read_to_string(hypr.runtime_directory().unwrap().join("effect.frag")).unwrap();
    assert!(
        loss.contains("HOLD_A"),
        "Overtime skipped the final disruption"
    );
    owner.heartbeat_for(Duration::from_millis(350));
    capture("signal-loss-two-outputs.png", "");
    assert!(hypr.request("/configerrors").unwrap().trim().is_empty());
    // No more display publications: the guardian must finish the handoff itself.
    owner.heartbeat_for(Duration::from_millis(1200));
    let (w, h, bytes) = image(&capture("overlay-two-outputs.png", ""));
    assert_eq!((w, h), (2560, 720));
    let expected = bork::overtime::Renderer::load(&bork::overtime::Overtime::default())
        .unwrap()
        .render(1280, 720, 272, 300)
        .unwrap();
    // Check characteristic colours on BOTH outputs, including the opaque central panel.
    for output in [0, 1280] {
        for (x, y) in [
            (40, 40),
            (240, 40),
            (450, 40),
            (640, 40),
            (900, 40),
            (1150, 40),
            (300, 200),
            (1100, 600),
        ] {
            let i = (y * w + output + x) * 3;
            let e = (y * 1280 + x) * 4;
            assert_eq!(
                &bytes[i..i + 3],
                &expected.rgba()[e..e + 3],
                "Output {output}, point {x},{y}"
            );
        }
    }
    for (key, value) in &before {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    owner.send(Message::Display(DisplayState::TestCard {
        remaining: 271,
        total: 300,
        appearance: bork::overtime::Overtime::default(),
    }));
    assert!(
        !hypr
            .runtime_directory()
            .unwrap()
            .join("journal.json")
            .exists(),
        "Countdown update restarted signal loss"
    );
    let keys = target.state.keys;
    let clicks = target.state.clicks;
    target.click();
    target.key();
    owner.heartbeat_for(Duration::from_millis(100));
    target.dispatch();
    assert_eq!(target.state.keys, keys, "Keys leaked through test card");
    assert_eq!(
        target.state.clicks, clicks,
        "Clicks leaked through test card"
    );
    println!("Exclusive keyboard focus and pointer interception verified over fullscreen target.");
    // The independent guardian suppresses its surfaces/settings during a lock.
    let session = target.lock();
    owner.heartbeat_for(Duration::from_millis(400));
    assert!(!hypr.json("layers").unwrap().to_string().contains("bork"));
    for (key, value) in &before {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    target.unlock(session);
    owner.heartbeat_for(Duration::from_millis(250));
    // No normal shutdown message: EOF must independently release all focused surfaces.
    drop(owner);
    let recovered = capture("after-parent-eof.png", "");
    let (_, _, bytes_after) = image(&recovered);
    assert_ne!(bytes_after, bytes, "Test cards survived parent EOF");
    for (key, value) in &before {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    println!("Real compositor setting restoration and overlay removal after EOF passed.");

    println!("Connecting idle monitor");
    let mut monitor = InputMonitor::connect().unwrap();
    println!("Idle monitor connected");
    pump(&mut monitor, Duration::from_millis(1300));
    assert!(
        monitor.activity().idle_since.is_some(),
        "Input idle was not reported"
    );
    println!("Real input-idle received despite visible surface's media idle inhibitor.");
    target.key();
    pump(&mut monitor, Duration::from_millis(100));
    assert!(
        monitor.activity().resumed,
        "Keyboard did not interrupt inactivity"
    );
    pump(&mut monitor, Duration::from_millis(1200));
    assert!(monitor.activity().idle_since.is_some());
    target.motion();
    pump(&mut monitor, Duration::from_millis(100));
    assert!(
        monitor.activity().resumed,
        "Pointer motion alone did not interrupt inactivity"
    );
    println!("Keyboard and pointer-only interruption verified.");
}

#[test]
#[ignore = "requires tests/nested-session.sh; never run on the active desktop"]
fn shader_removal_is_visible_after_unlock_without_input() {
    isolated();
    let hypr = Hyprland::from_env().unwrap();
    hypr.request("/output create headless unlock-proof")
        .unwrap();
    configure_output(&hypr, "unlock-proof,1280x720@30,0x0,1");
    thread::sleep(Duration::from_millis(300));
    hypr.request("/dispatch focusmonitor unlock-proof").unwrap();
    let mut target = support::Desktop::new();
    thread::sleep(Duration::from_millis(300));
    let (width, height, before) = image(&capture("unlock-before.png", "unlock-proof"));
    assert_eq!((width, height), (1280, 720));
    let mut owner = Guardian::start();
    owner.send(Message::Display(DisplayState::Interference {
        shader: Interference::default()
            .hyprland_shader_at(Glitchiness::MAXIMUM, 0.0, 41)
            .unwrap(),
    }));
    owner.heartbeat_for(Duration::from_millis(300));
    let (_, _, affected) = image(&capture("unlock-interference.png", "unlock-proof"));
    let changed = before
        .chunks_exact(3)
        .zip(affected.chunks_exact(3))
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 10_000,
        "Capture must observe the shader effect: {changed} changed pixels"
    );
    let session = target.lock();
    owner.heartbeat_for(Duration::from_millis(400));
    owner.send(Message::Display(DisplayState::Work));
    target.unlock(session);
    owner.heartbeat_for(Duration::from_millis(400));
    let (_, _, after) = image(&capture("unlock-after.png", "unlock-proof"));
    let different = before
        .chunks_exact(3)
        .zip(after.chunks_exact(3))
        .enumerate()
        // Ignore the compositor's animated cursor at the centre and warning banners.
        .filter(|(i, (a, b))| {
            let x = i % 1280;
            let y = i / 1280;
            y > 80 && !(620..680).contains(&x) && a != b
        })
        .count();
    assert!(
        different < 100,
        "{different} pixels remained changed after unlock without input"
    );
}

fn configure_output(hypr: &Hyprland, rule: &str) {
    if std::env::var("BORK_LUA_CONFIG").as_deref() == Ok("1") {
        let fields: Vec<_> = rule.split(',').collect();
        let transform = fields.get(5).copied().unwrap_or("0");
        let code = format!(
            "/eval hl.monitor({{output={},mode={},position={},scale={},transform={}}})",
            serde_json::to_string(fields[0]).unwrap(),
            serde_json::to_string(fields[1]).unwrap(),
            serde_json::to_string(fields[2]).unwrap(),
            serde_json::to_string(fields[3]).unwrap(),
            transform
        );
        assert_eq!(hypr.request(&code).unwrap().trim(), "ok");
    } else {
        hypr.set("monitor", rule).unwrap();
    }
}
