//! Thin executable contract; configuration and formatting matrices live with their owners.
use std::process::Command;

#[test]
fn config_dispatch_preserves_streams_and_exit_status_without_graphics() {
    let home = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_bork"))
            .arg("config")
            .args(args)
            .env("XDG_CONFIG_HOME", home.path())
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("_BORK_RUNTIME_READY")
            .env("BORK_LIBRARY_PATH", "broken")
            .env("BORK_NIX_DISCOVERY", "broken")
            .output()
            .unwrap()
    };
    let set = run(&["set", "timing.break_seconds", "240"]);
    assert!(set.status.success());
    assert!(set.stdout.is_empty() && set.stderr.is_empty());
    let get = run(&["get", "timing.break_seconds"]);
    assert!(get.status.success());
    assert_eq!(get.stdout, b"240\n");
    assert!(get.stderr.is_empty());
    let invalid = run(&["set", "timing.break_seconds", "not JSON"]);
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    assert!(!invalid.stderr.is_empty());
}

#[test]
fn status_and_emergency_reset_work_with_broken_graphics_configuration() {
    let runtime = tempfile::tempdir().unwrap();
    for action in ["status", "reset"] {
        let output = Command::new(env!("CARGO_BIN_EXE_bork"))
            .arg(action)
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env("HYPRLAND_INSTANCE_SIGNATURE", "fixture-instance")
            .env_remove("_BORK_RUNTIME_READY")
            .env("BORK_LIBRARY_PATH", "broken")
            .env("BORK_NIX_DISCOVERY", "broken")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<bork::control::Status>(&output.stdout).unwrap(),
            bork::control::Status::stopped()
        );
        assert!(output.stderr.is_empty());
    }
}
