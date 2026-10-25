//! Runtime configuration is exercised through the executable, without contacting a desktop.
use std::process::Command;

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bork"));
    command
        .env_remove("_BORK_RUNTIME_READY")
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .env("XDG_RUNTIME_DIR", std::env::temp_dir())
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("BORK_HYPRLAND_BINARY")
        .env_remove("BORK_LIBRARY_PATH")
        .env("BORK_NIX_DISCOVERY", "0");
    command
}

#[test]
fn invalid_library_override_is_actionable() {
    let output = command()
        .arg("doctor")
        .env("BORK_LIBRARY_PATH", "relative/library/path")
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
        .env("BORK_LIBRARY_PATH", override_dir.path())
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
