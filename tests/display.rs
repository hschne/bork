//! Only run by nested-session.sh in a namespace with no access to the active desktop.
mod support;

use bork::{
    effect::{Glitchiness, Settings},
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
        if !matches!(message, Message::Heartbeat) {
            println!("Sending guardian request");
        }
        let input = self.input.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &message).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
        self.reply();
        if !matches!(message, Message::Heartbeat) {
            println!("Guardian request acknowledged");
        }
    }
    fn trial(&mut self, shader: String) {
        bork::recovery::request_trial(
            &Hyprland::from_env().unwrap().runtime_directory().unwrap(),
            &bork::recovery::TrialRequest::Start { shader },
        )
        .unwrap();
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
fn compositor_shader_overlay_and_crash_recovery() {
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
    let original = Settings::default()
        .hyprland_shader(Glitchiness::try_from(0.0).unwrap())
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
    let mut metrics = Vec::new();
    let start = graphics_counters();
    thread::sleep(Duration::from_millis(500));
    metrics.push(serde_json::json!({"stage":"work-static", "before":start,"after":graphics_counters(),"sample_ms":500}));
    println!("Starting guardian");
    let mut owner = Guardian::start();
    println!("Guardian ready");
    verify_randomness(&mut owner);
    for level in [10.0, 50.0, 100.0] {
        owner.send(Message::Display(DisplayState::Interference {
            shader: Settings::default()
                .hyprland_shader(Glitchiness::try_from(level).unwrap())
                .unwrap(),
        }));
        owner.heartbeat_for(Duration::from_millis(250));
        let start = graphics_counters();
        owner.heartbeat_for(Duration::from_millis(500));
        metrics.push(serde_json::json!({"stage":format!("glitch-{level}"),"before":start,"after":graphics_counters(),"sample_ms":500}));
        println!(
            "Shader level {level}, config errors: {}",
            hypr.request("/configerrors").unwrap()
        );
        capture(&format!("desktop-level-{level:.0}.png"), "");
    }
    owner.send(Message::Display(DisplayState::TestCard {
        remaining: 272,
        total: 300,
        appearance: bork::overtime::Overtime::default(),
    }));
    owner.heartbeat_for(Duration::from_millis(250));
    let start = graphics_counters();
    owner.heartbeat_for(Duration::from_millis(500));
    metrics.push(serde_json::json!({"stage":"test-card-static","before":start,"after":graphics_counters(),"sample_ms":500}));
    fs::write(
        runtime.join("render-metrics.json"),
        serde_json::to_vec_pretty(&metrics).unwrap(),
    )
    .unwrap();
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

    let mut owner = Guardian::start();
    owner.send(Message::Display(DisplayState::TestCard {
        remaining: 272,
        total: 300,
        appearance: bork::overtime::Overtime::default(),
    }));
    configure_output(&hypr, "proof-two,1280x720@30,1280x0,1.6,transform,1");
    owner.heartbeat_for(Duration::from_millis(500));
    assert_card(&capture("overlay-rotated-scaled.png", "proof-two"));
    hypr.request("/output create headless proof-three").unwrap();
    configure_output(&hypr, "proof-three,1280x720@30,2000x0,1");
    owner.heartbeat_for(Duration::from_millis(500));
    assert_card(&capture("overlay-hotplug.png", "proof-three"));
    hypr.request("/output remove proof-three").unwrap();
    owner.heartbeat_for(Duration::from_millis(700));
    assert_card(&capture("overlay-after-unplug.png", "proof-one"));
    hypr.request("/reload").unwrap();
    owner.heartbeat_for(Duration::from_millis(500));
    assert_card(&capture("overlay-after-reload.png", "proof-one"));
    drop(owner);
    println!("Fractional scaling, rotation, monitor hotplug/unplug and config reload passed.");

    let before_trial: Vec<_> = before
        .iter()
        .map(|(key, _)| (key.clone(), hypr.get(key).unwrap()))
        .collect();
    let mut owner = Guardian::start();
    owner.trial(
        Settings::default()
            .hyprland_shader(Glitchiness::try_from(100.0).unwrap())
            .unwrap(),
    );
    owner.heartbeat_for(Duration::from_millis(5300));
    for (key, value) in &before_trial {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    drop(owner);
    println!("Five-second real-compositor trial expired and restored exact settings.");

    let mut owner = Guardian::start();
    owner.send(Message::Display(DisplayState::Interference {
        shader: Settings::default()
            .hyprland_shader(Glitchiness::try_from(100.0).unwrap())
            .unwrap(),
    }));
    let session = target.lock();
    assert!(hypr.locked().unwrap());
    owner.heartbeat_for(Duration::from_millis(400));
    for (key, value) in &before_trial {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    assert!(
        bork::recovery::DesktopTrial::start(
            &hypr,
            Settings::default()
                .hyprland_shader(Glitchiness::try_from(50.0).unwrap())
                .unwrap()
        )
        .is_err()
    );
    target.unlock(session);
    assert!(!hypr.locked().unwrap());
    owner.send(Message::Display(DisplayState::TestCard {
        remaining: 272,
        total: 300,
        appearance: bork::overtime::Overtime::default(),
    }));
    owner.heartbeat_for(Duration::from_millis(200));
    let session = target.lock();
    owner.heartbeat_for(Duration::from_millis(400));
    assert!(
        !hypr.json("layers").unwrap().to_string().contains("bork"),
        "Test card remained on security lock screen"
    );
    target.unlock(session);
    drop(owner);
    println!("Real session lock suppresses shader/overlays independently; locked trials refused.");

    let app =
        std::env::var_os("BORK_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_bork").into());
    let mut owner = Guardian::start();
    owner.send(Message::Display(DisplayState::TestCard {
        remaining: 272,
        total: 300,
        appearance: bork::overtime::Overtime::default(),
    }));
    owner.heartbeat_for(Duration::from_millis(200));
    assert!(Command::new(&app).arg("reset").status().unwrap().success());
    drop(owner);
    println!("Public emergency reset released real overlay.");

    let clients_before = hypr.json("clients").unwrap().as_array().unwrap().len();
    let mut background = Command::new(&app)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !hypr
        .runtime_directory()
        .unwrap()
        .join("control.sock")
        .exists()
    {
        assert!(Instant::now() < deadline, "Background did not become ready");
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        hypr.json("clients").unwrap().as_array().unwrap().len(),
        clients_before,
        "Background opened a control window"
    );
    let prior_trial: Vec<_> = before
        .iter()
        .map(|(key, _)| (key.clone(), hypr.get(key).unwrap()))
        .collect();
    assert!(
        hypr.runtime_directory()
            .unwrap()
            .join("trial.sock")
            .exists()
    );
    let mut trial = bork::recovery::DesktopTrial::start(
        &hypr,
        Settings::default()
            .hyprland_shader(Glitchiness::try_from(100.0).unwrap())
            .unwrap(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_millis(5300);
    while Instant::now() < deadline {
        trial.heartbeat().unwrap();
        thread::sleep(Duration::from_millis(50));
    }
    trial.finish().unwrap();
    assert!(
        background.try_wait().unwrap().is_none(),
        "Trial expiry stopped real background controller"
    );
    for (key, value) in &prior_trial {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    let trial = bork::recovery::DesktopTrial::start(
        &hypr,
        Settings::default()
            .hyprland_shader(Glitchiness::try_from(50.0).unwrap())
            .unwrap(),
    )
    .unwrap();
    trial.finish().unwrap();
    assert!(
        background.try_wait().unwrap().is_none(),
        "Editor close stopped real background controller"
    );
    for (key, value) in &prior_trial {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    println!("Editor trials borrow background owner, expire/cancel, and leave controller running.");
    for (action, expected) in [
        ("pause", "paused"),
        ("resume", "running"),
        ("reset", "running"),
    ] {
        let result = Command::new(&app).arg(action).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(status["state"], expected);
        if action == "reset" {
            assert_eq!(status["phase"], "work");
            assert_eq!(status["work_seconds"], 0);
        }
        assert!(
            background.try_wait().unwrap().is_none(),
            "Timer command terminated background"
        );
    }
    // Reset is no longer shutdown. Terminate only this fixture-owned background.
    assert!(
        Command::new("/bin/sh")
            .args([
                "-c",
                "kill -TERM \"$1\"",
                "owned-background",
                &background.id().to_string()
            ])
            .status()
            .unwrap()
            .success()
    );
    let result = background.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "Background shutdown failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!("Windowless background status/control, reset, and SIGTERM shutdown passed.");

    let background = Command::new(&app)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !hypr
        .runtime_directory()
        .unwrap()
        .join("trial.sock")
        .exists()
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let trial = bork::recovery::DesktopTrial::start(
        &hypr,
        Settings::default()
            .hyprland_shader(Glitchiness::try_from(100.0).unwrap())
            .unwrap(),
    )
    .unwrap();
    let parent = background.id();
    let children = fs::read_to_string(format!("/proc/{parent}/task/{parent}/children")).unwrap();
    let guardian = children
        .split_whitespace()
        .find(|pid| {
            fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|args| args.split(|byte| *byte == 0).nth(1) == Some(b"guardian"))
        })
        .expect("Fixture-owned background guardian not found");
    assert!(
        Command::new("/bin/sh")
            .args(["-c", "kill -KILL \"$1\"", "owned-guardian", guardian])
            .status()
            .unwrap()
            .success()
    );
    let result = background.wait_with_output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("prior settings restored"));
    for (key, value) in &prior_trial {
        assert_eq!(hypr.get(key).unwrap(), *value);
    }
    trial.finish().unwrap();
    println!("Parent independently restored exact settings after guardian SIGKILL.");
}

fn verify_randomness(owner: &mut Guardian) {
    // A diagnostic view of the actual production shader: x is a 160-second
    // timeline, RGB are independent effect envelopes. Keep the normal adapter
    // and its monitor/session seed wiring; only substitute time and final colour.
    let probe = |seed| {
        Settings::default()
            .hyprland_shader_at(Glitchiness::MAXIMUM, 0.0, seed)
            .unwrap()
            .replace(
                "seconds = max(seconds, 0.0);",
                "seconds = floor(uv.x * 1280.0) * 0.125;",
            )
            .replace(
                "return vec4(clamp(c.rgb, 0.0, 1.0), c.a);",
                "return vec4(tearsBurst, bandsBurst, noiseBurst, 1.0);",
            )
    };
    owner.send(Message::Display(DisplayState::Interference {
        shader: probe(41),
    }));
    owner.heartbeat_for(Duration::from_millis(100));
    let (width, height, first) = image(&capture("randomness-monitor-one.png", "proof-one"));
    let (other_width, other_height, second) =
        image(&capture("randomness-monitor-two.png", "proof-two"));
    assert_eq!((width, height), (other_width, other_height));
    let row = |bytes: &[u8], channel: usize| -> Vec<bool> {
        (0..width)
            .map(|x| bytes[((height / 2) * width + x) * 3 + channel] > 127)
            .collect()
    };
    let tears = row(&first, 0);
    let differences = |a: &[bool], b: &[bool]| a.iter().zip(b).filter(|(a, b)| a != b).count();
    assert!(
        differences(&tears, &row(&second, 0)) > width / 8,
        "Equal-size monitors share a burst sequence"
    );
    for channel in [1, 2] {
        assert!(
            differences(&tears, &row(&first, channel)) > width / 8,
            "Effects share a burst sequence"
        );
    }
    let starts: Vec<_> = (1..width).filter(|&x| tears[x] && !tears[x - 1]).collect();
    let gaps: std::collections::BTreeSet<_> =
        starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let mut lengths = std::collections::BTreeSet::new();
    for &start in &starts {
        if let Some(length) = tears[start..].iter().position(|active| !active) {
            lengths.insert(length);
        }
    }
    assert!(
        starts.len() > 20 && gaps.len() > 5 && lengths.len() > 5,
        "Burst timing/duration is too regular: starts={}, gaps={gaps:?}, lengths={lengths:?}",
        starts.len()
    );
    owner.send(Message::Display(DisplayState::Interference {
        shader: probe(42),
    }));
    owner.heartbeat_for(Duration::from_millis(100));
    let (_, _, changed) = image(&capture("randomness-new-session.png", "proof-one"));
    assert!(
        differences(&tears, &row(&changed, 0)) > width / 8,
        "Session seed does not change the sequence"
    );
    owner.send(Message::Display(DisplayState::Interference {
        shader: probe(41),
    }));
    owner.heartbeat_for(Duration::from_millis(100));
    let (_, _, repeated) = image(&capture("randomness-recompiled.png", "proof-one"));
    assert_eq!(
        first, repeated,
        "Recompilation restarts/reseeds the same timeline"
    );
    println!(
        "GPU randomness: {} bursts, {} distinct onset intervals, {} distinct lengths; independent effects, equal-size monitors and sessions; stable on recompilation.",
        starts.len(),
        gaps.len(),
        lengths.len()
    );
}

fn assert_card(path: &std::path::Path) {
    let (w, h, bytes) = image(path);
    let expected = bork::overtime::Renderer::load(&bork::overtime::Overtime::default())
        .unwrap()
        .render(w as u32, h as u32, 272, 300)
        .unwrap();
    for (x, y) in [
        (w / 10, h / 5),
        (w * 3 / 10, h / 5),
        (w / 2, h / 5),
        (w * 8 / 10, h / 5),
        (w / 2, h * 3 / 10),
    ] {
        let i = (y * w + x) * 3;
        let e = (y * w + x) * 4;
        assert_eq!(
            &bytes[i..i + 3],
            &expected.rgba()[e..e + 3],
            "Test card missing at {x},{y} on {}",
            path.display()
        );
    }
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

fn graphics_counters() -> serde_json::Value {
    // Per-compositor counters are isolated; sysfs power is explicitly device-wide
    // and may include unrelated host GPU work. These are not battery estimates.
    let Ok(pid) = std::env::var("BORK_COMPOSITOR_PID") else {
        return serde_json::json!({"unavailable":"Renderer is outside this client's PID namespace"});
    };
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    let ticks = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
    let mut drm = std::collections::BTreeSet::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fdinfo"))
        .unwrap()
        .flatten()
    {
        if let Ok(info) = fs::read_to_string(entry.path()) {
            for line in info.lines().filter(|line| line.starts_with("drm-")) {
                drm.insert(line.to_string());
            }
        }
    }
    let mut powers = Vec::new();
    if let Ok(hwmon) = fs::read_dir("/sys/class/drm/card1/device/hwmon") {
        for entry in hwmon.flatten() {
            if let Ok(power) = fs::read_to_string(entry.path().join("power1_average")) {
                powers.push(power.trim().to_string());
            }
        }
    }
    serde_json::json!({"process_cpu_ticks":ticks,"drm_fdinfo":drm,"shared_device_power_microwatts":powers})
}
