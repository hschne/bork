//! Runtime configuration is exercised through the executable, without contacting a desktop.
use std::process::Command;

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_signal-loss"));
    command
        .env_remove("_SIGNAL_LOSS_RUNTIME_READY")
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .env("XDG_RUNTIME_DIR", std::env::temp_dir())
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("SIGNAL_LOSS_HYPRLAND_BINARY")
        .env_remove("SIGNAL_LOSS_LIBRARY_PATH")
        .env("SIGNAL_LOSS_NIX_DISCOVERY", "0");
    command
}

#[test]
fn help_documents_runtime_overrides_without_loading_graphics() {
    let output = command().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("SIGNAL_LOSS_LIBRARY_PATH"));
    assert!(help.contains("SIGNAL_LOSS_NIX_DISCOVERY"));
    assert!(help.contains("SIGNAL_LOSS_HYPRLAND_BINARY"));
}

#[test]
fn invalid_library_override_is_actionable() {
    let output = command()
        .arg("doctor")
        .env("SIGNAL_LOSS_LIBRARY_PATH", "relative/library/path")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("existing absolute directories"));
}

#[test]
fn override_precedes_inherited_paths_and_reexec_preserves_command() {
    let override_dir = tempfile::tempdir().unwrap();
    let inherited_dir = tempfile::tempdir().unwrap();
    let output = command()
        .arg("doctor")
        .env("SIGNAL_LOSS_LIBRARY_PATH", override_dir.path())
        .env("LD_LIBRARY_PATH", inherited_dir.path())
        .output()
        .unwrap();
    // No Hyprland signature: the doctor must reach its diagnostic path, not connect.
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.find(override_dir.path().to_str().unwrap()).unwrap()
            < stdout.find(inherited_dir.path().to_str().unwrap()).unwrap()
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Not in a Hyprland session"));
}

#[test]
fn emergency_reset_ignores_broken_graphics_configuration() {
    let runtime = tempfile::tempdir().unwrap();
    let output = command()
        .arg("reset")
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("HYPRLAND_INSTANCE_SIGNATURE", "fixture-instance")
        .env("SIGNAL_LOSS_NIX_DISCOVERY", "invalid")
        .env("SIGNAL_LOSS_LIBRARY_PATH", "invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
