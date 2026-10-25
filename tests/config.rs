//! Config commands use an isolated home and never initialize graphics or the timer.
use serde_json::{Value, json};
use std::{fs, process::Command};

struct Config {
    home: tempfile::TempDir,
}

impl Config {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_bork"))
            .arg("config")
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .env_remove("_BORK_RUNTIME_READY")
            .env("BORK_LIBRARY_PATH", "broken")
            .env("BORK_NIX_DISCOVERY", "broken")
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) -> String {
        let output = self.command(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        String::from_utf8(output.stdout).unwrap()
    }

    fn path(&self) -> std::path::PathBuf {
        self.home.path().join("config/bork/settings.json")
    }
}

#[test]
fn defaults_are_readable_without_creating_a_file_or_connecting_to_a_session() {
    let config = Config::new();
    assert_eq!(
        config.success(&["path"]),
        format!("{}\n", config.path().display())
    );
    assert_eq!(config.success(&["get", "timing.break_seconds"]), "300\n");
    let list = config.success(&["list"]);
    assert!(list.contains("timing.work_seconds=1200\n"));
    assert!(list.contains("tears.enabled=true\n"));
    assert!(list.contains("tears.maximum=0.7\n"));
    let values: Value = serde_json::from_str(&config.success(&["list", "--json"])).unwrap();
    assert_eq!(values["timing.break_seconds"], 300);
    assert_eq!(values["curve"][0]["intensity"], 35.0);
    assert_eq!(values["curve"][3]["intensity"], 100.0);
    assert_eq!(values.as_object().unwrap().len(), 20);
    for (key, expected) in values.as_object().unwrap() {
        let actual: Value = serde_json::from_str(&config.success(&["get", key])).unwrap();
        assert_eq!(&actual, expected);
    }
    assert!(!config.path().exists());
}

#[test]
fn every_key_can_be_set_and_unset_and_lists_agree() {
    let config = Config::new();
    let defaults: Value = serde_json::from_str(&config.success(&["list", "--json"])).unwrap();
    for (key, value) in defaults.as_object().unwrap() {
        assert!(config.success(&["set", key, &value.to_string()]).is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&config.success(&["get", key])).unwrap(),
            *value
        );
        assert!(config.success(&["unset", key]).is_empty());
    }
    let actual: Value = serde_json::from_str(&config.success(&["list", "--json"])).unwrap();
    assert_eq!(actual, defaults);
}

#[test]
fn updates_preserve_other_values_and_unset_restores_only_one_default() {
    let config = Config::new();
    config.success(&["set", "timing.break_seconds", "240"]);
    config.success(&["set", "timing.grace_seconds", "0"]);
    config.success(&["set", "tears.enabled", "false"]);
    config.success(&["set", "colour.maximum", "0.25"]);
    let curve = json!([
        {"progress":0.0,"intensity":20.0},
        {"progress":0.3,"intensity":40.0},
        {"progress":0.7,"intensity":70.0},
        {"progress":1.0,"intensity":100.0}
    ]);
    config.success(&["set", "curve", &curve.to_string()]);
    config.success(&["set", "preview_image", "\"/tmp/desktop.png\""]);
    config.success(&["unset", "timing.break_seconds"]);
    let values: Value = serde_json::from_str(&config.success(&["list", "--json"])).unwrap();
    assert_eq!(values["timing.break_seconds"], 300);
    assert_eq!(values["timing.grace_seconds"], 0);
    assert_eq!(values["tears.enabled"], false);
    assert_eq!(values["colour.maximum"], 0.25);
    assert_eq!(values["curve"], curve);
    assert_eq!(values["preview_image"], "/tmp/desktop.png");
}

#[test]
fn invalid_updates_and_unknown_keys_leave_the_file_untouched() {
    let config = Config::new();
    config.success(&["set", "timing.work_seconds", "900"]);
    let original = fs::read(config.path()).unwrap();
    for args in [
        vec!["get", "typo"],
        vec!["get", "timing"],
        vec!["unset", "timing"],
        vec!["unset", "timing.typo"],
        vec!["set", "timing.typo", "10"],
        vec!["set", "timing", "{}"],
        vec!["set", "timing.break_seconds", "0"],
        vec!["set", "timing.work_seconds", "4294967296"],
        vec!["set", "timing.work_seconds", "1.5"],
        vec!["set", "timing.work_seconds", "true"],
        vec!["set", "timing.work_seconds", "null"],
        vec!["set", "timing.work_seconds", "[1]"],
        vec!["set", "timing.work_seconds", "\"1200\""],
        vec!["get", "strength"],
        vec!["set", "strength", "1"],
        vec!["set", "preview_image", "42"],
        vec!["set", "curve", "0"],
        vec!["set", "curve", "NaN"],
        vec!["set", "tears.entry", "100"],
        vec!["set", "tears.maximum", "1.1"],
        vec!["set", "tears.enabled", "1"],
    ] {
        let output = config.command(&args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(fs::read(config.path()).unwrap(), original);
    }
}

#[test]
fn invalid_existing_configuration_is_not_overwritten() {
    let config = Config::new();
    fs::create_dir_all(config.path().parent().unwrap()).unwrap();
    fs::write(config.path(), "invalid").unwrap();
    for args in [
        vec!["list"],
        vec!["get", "curve"],
        vec!["set", "curve", "2"],
        vec!["unset", "curve"],
    ] {
        assert!(!config.command(&args).status.success());
        assert_eq!(fs::read_to_string(config.path()).unwrap(), "invalid");
    }
    assert!(!config.success(&["path"]).is_empty());
}

#[test]
fn home_is_the_fallback_and_missing_arguments_are_rejected() {
    let config = Config::new();
    let output = Command::new(env!("CARGO_BIN_EXE_bork"))
        .args(["config", "path"])
        .env("HOME", config.home.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "{}/.config/bork/settings.json\n",
            config.home.path().display()
        )
    );
    for args in [vec![], vec!["get"], vec!["set", "curve"], vec!["unset"]] {
        assert!(!config.command(&args).status.success());
    }
    assert_eq!(
        json!(300),
        bork::config::get(&config.path(), "timing.break_seconds").unwrap()
    );
}
