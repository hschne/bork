use crate::{
    effect::{GraceCurve, Interference},
    overtime::Overtime,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const WORK_INTERVAL: u64 = 20 * 60;
const GRACE_INTERVAL: u64 = 3 * 60;
const BREAK_INTERVAL: u64 = 5 * 60;

/// Durations in whole seconds. Work and break must be positive; grace may be zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    pub work_seconds: NonZeroU32,
    pub grace_seconds: u32,
    pub break_seconds: NonZeroU32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            work_seconds: NonZeroU32::new(WORK_INTERVAL as u32).unwrap(),
            grace_seconds: GRACE_INTERVAL as u32,
            break_seconds: NonZeroU32::new(BREAK_INTERVAL as u32).unwrap(),
        }
    }
}

/// Application settings. The flat JSON format is independent of the Rust module layout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub timing: Timing,
    pub curve: GraceCurve,
    pub preview_image: Option<PathBuf>,
    pub overtime: Overtime,
    #[serde(flatten)]
    pub interference: Interference,
}

pub fn path() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(path) => PathBuf::from(path),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME and XDG_CONFIG_HOME are unset")?)
                .join(".config")
        }
    };
    ensure!(root.is_absolute(), "Configuration root must be absolute");
    Ok(root.join("bork/settings.json"))
}

pub fn load(path: &Path) -> Result<Settings> {
    load_version(path).map(|(settings, _)| settings)
}

fn load_version(path: &Path) -> Result<(Settings, Option<SystemTime>)> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Settings::default(), None));
        }
        Err(error) => return Err(error.into()),
    };
    let modified = file.metadata()?.modified().ok();
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "Settings file exceeds 64 KiB");
    let settings: Settings = serde_json::from_slice(&bytes).context("Invalid Bork settings")?;
    Ok((settings, modified))
}

pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    let parent = path.parent().context("Missing config directory")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut file, settings)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// Effective leaf settings under dotted keys, including defaults.
pub fn list(path: &Path) -> Result<BTreeMap<String, serde_json::Value>> {
    fn flatten(
        prefix: &str,
        value: serde_json::Value,
        result: &mut BTreeMap<String, serde_json::Value>,
    ) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    let key = if prefix.is_empty() {
                        key
                    } else {
                        format!("{prefix}.{key}")
                    };
                    flatten(&key, value, result);
                }
            }
            value => {
                result.insert(prefix.to_owned(), value);
            }
        }
    }
    let mut result = BTreeMap::new();
    // Preserve serde's short f32 representation instead of widening it to f64.
    let values = serde_json::from_slice(&serde_json::to_vec(&load(path)?)?)?;
    flatten("", values, &mut result);
    Ok(result)
}

/// Read a known setting; object-valued groups are not keys.
pub fn get(path: &Path, key: &str) -> Result<serde_json::Value> {
    list(path)?
        .remove(key)
        .with_context(|| format!("Unknown config key: {key}"))
}

/// Set a leaf value, or restore its default with `None`, before atomic saving.
pub fn update(path: &Path, key: &str, value: Option<serde_json::Value>) -> Result<()> {
    let mut settings = serde_json::to_value(load(path)?)?;
    let mut default = serde_json::to_value(Settings::default())?;
    let mut target = &mut settings;
    let mut fallback = &mut default;
    for part in key.split('.') {
        target = target
            .get_mut(part)
            .with_context(|| format!("Unknown config key: {key}"))?;
        fallback = fallback
            .get_mut(part)
            .with_context(|| format!("Unknown config key: {key}"))?;
    }
    ensure!(!target.is_object(), "Unknown config key: {key}");
    *target = value.unwrap_or_else(|| fallback.take());
    let settings: Settings = serde_json::from_value(settings)
        .with_context(|| format!("Invalid value for config key: {key}"))?;
    save(path, &settings)
}

/// Polling settings reload owns its file version; work debt never lives here.
pub(crate) struct LiveSettings {
    path: PathBuf,
    settings: Settings,
    modified: Option<SystemTime>,
    checked: Instant,
}

impl LiveSettings {
    pub(crate) fn open(path: PathBuf) -> Result<Self> {
        let (settings, modified) = load_version(&path)?;
        Ok(Self {
            path,
            settings,
            modified,
            checked: Instant::now(),
        })
    }
    pub(crate) fn current(&self) -> &Settings {
        &self.settings
    }
    pub(crate) fn reload(&mut self, now: Instant) -> Result<bool> {
        if now.duration_since(self.checked) < Duration::from_secs(1) {
            return Ok(false);
        }
        self.checked = now;
        let modified = fs::metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .ok();
        if modified == self.modified {
            return Ok(false);
        }
        let (settings, modified) = load_version(&self.path)?;
        let changed = settings != self.settings;
        self.settings = settings;
        self.modified = modified;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listing_defaults_does_not_create_a_file_and_get_agrees() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let values = list(&path).unwrap();
        assert_eq!(values["timing.work_seconds"], 1200);
        assert_eq!(values["timing.break_seconds"], 300);
        assert_eq!(values["tears.maximum"].to_string(), "0.7");
        assert_eq!(values["curve"][0]["intensity"], 35.0);
        assert!(!path.exists());
        for (key, value) in values {
            assert_eq!(get(&path, &key).unwrap(), value);
        }
        assert!(get(&path, "timing").is_err());
        assert!(get(&path, "typo").is_err());
    }

    #[test]
    fn updates_preserve_other_settings_and_unset_restores_one_default() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        update(&path, "timing.break_seconds", Some(240.into())).unwrap();
        update(&path, "timing.grace_seconds", Some(0.into())).unwrap();
        update(&path, "tears.enabled", Some(false.into())).unwrap();
        let curve = serde_json::json!([
            {"progress":0.0,"intensity":20.0},
            {"progress":0.3,"intensity":40.0},
            {"progress":0.7,"intensity":70.0},
            {"progress":1.0,"intensity":100.0}
        ]);
        update(&path, "curve", Some(curve.clone())).unwrap();
        update(&path, "preview_image", Some("/tmp/desktop.png".into())).unwrap();
        update(&path, "timing.break_seconds", None).unwrap();
        let values = list(&path).unwrap();
        assert_eq!(values["timing.break_seconds"], 300);
        assert_eq!(values["timing.grace_seconds"], 0);
        assert_eq!(values["tears.enabled"], false);
        assert_eq!(values["curve"], curve);
        assert_eq!(values["preview_image"], "/tmp/desktop.png");
    }

    #[test]
    fn rejected_updates_leave_the_file_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        update(&path, "timing.work_seconds", Some(900.into())).unwrap();
        let original = fs::read(&path).unwrap();
        for (key, value) in [
            ("typo", None),
            ("timing", None),
            ("timing.typo", Some(10.into())),
            ("timing.work_seconds", Some(0.into())),
            ("timing.work_seconds", Some(serde_json::Value::Null)),
            ("timing.work_seconds", Some("1200".into())),
            ("preview_image", Some(42.into())),
            ("curve", Some(0.into())),
            ("tears.entry", Some(100.into())),
            ("tears.enabled", Some(1.into())),
        ] {
            assert!(update(&path, key, value).is_err(), "{key}");
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        fs::write(&path, "invalid").unwrap();
        assert!(list(&path).is_err());
        assert!(update(&path, "timing.work_seconds", None).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid");
    }

    #[test]
    fn flat_settings_keep_partial_defaults_and_reject_unknown_fields() {
        let settings: Settings =
            serde_json::from_str(r#"{"tears":{"enabled":false,"entry":5,"maximum":0.7}}"#).unwrap();
        assert_eq!(
            settings.interference.tears,
            crate::effect::Effect::new(false, 5.0, 0.7).unwrap()
        );
        assert_eq!(settings.interference.colour, Interference::default().colour);
        assert_eq!(settings.timing, Timing::default());
        for invalid in [
            r#"{"typo":true}"#,
            r#"{"interference":{}}"#,
            r#"{"tears":{"typo":1}}"#,
            r#"{"curve":0}"#,
        ] {
            assert!(
                serde_json::from_str::<Settings>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn reload_publishes_valid_changes_and_keeps_last_good_settings_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut live = LiveSettings::open(path.clone()).unwrap();
        let first_poll = Instant::now() + Duration::from_secs(2);
        let mut settings = Settings::default();
        let mut points = settings.curve.points();
        points[1].intensity = 60.0;
        settings.curve = points.try_into().unwrap();
        settings.timing.break_seconds = 210.try_into().unwrap();
        save(&path, &settings).unwrap();
        assert!(live.reload(first_poll).unwrap());
        assert_eq!(live.current(), &settings);
        assert!(!live.reload(first_poll).unwrap());
        // A malformed replacement must never get assigned to the live model.
        fs::write(&path, "invalid").unwrap();
        File::open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(10))
            .unwrap();
        assert!(live.reload(first_poll + Duration::from_secs(2)).is_err());
        assert_eq!(live.current(), &settings);
        fs::remove_file(&path).unwrap();
        assert!(live.reload(first_poll + Duration::from_secs(4)).unwrap());
        assert_eq!(live.current(), &Settings::default());
    }

    #[test]
    fn oversized_settings_are_rejected_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(load(&path).unwrap_err().to_string().contains("64 KiB"));
    }

    #[test]
    fn save_reopen_and_invalid_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/settings.json");
        let mut s = load(&path).unwrap();
        use crate::effect::Effect;
        s.timing.work_seconds = 900.try_into().unwrap();
        s.timing.grace_seconds = 60;
        s.timing.break_seconds = 210.try_into().unwrap();
        s.interference.tears = Effect::new(false, 5.0, 0.7).unwrap();
        s.interference.colour = Effect::new(true, 32.5, 0.65).unwrap();
        save(&path, &s).unwrap();
        let reopened = load(&path).unwrap();
        assert_eq!(reopened.timing, s.timing);
        assert_eq!(reopened.interference.tears, s.interference.tears);
        assert_eq!(reopened.interference.colour, s.interference.colour);
        fs::write(&path, "{\"curve\":0}").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn timing_defaults_and_validation_apply_to_json() {
        let mut settings = serde_json::to_value(Settings::default()).unwrap();
        settings.as_object_mut().unwrap().remove("timing");
        assert_eq!(
            serde_json::from_value::<Settings>(settings.clone()).unwrap(),
            Settings::default()
        );
        settings["timing"] = serde_json::json!({"break_seconds":210});
        let parsed: Settings = serde_json::from_value(settings.clone()).unwrap();
        assert_eq!(parsed.timing.work_seconds.get(), 1200);
        assert_eq!(parsed.timing.grace_seconds, 180);
        assert_eq!(parsed.timing.break_seconds.get(), 210);
        for invalid in [
            serde_json::json!({"work_seconds":0}),
            serde_json::json!({"break_seconds":0}),
            serde_json::json!({"grace_seconds":-1}),
            serde_json::json!({"work_seconds":1.5}),
            serde_json::json!({"work_seconds":4294967296_u64}),
            serde_json::json!({"typo":10}),
        ] {
            settings["timing"] = invalid;
            assert!(serde_json::from_value::<Settings>(settings.clone()).is_err());
        }
    }
}
