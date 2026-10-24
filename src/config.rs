use crate::effect::Settings;
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

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
    fn reload_publishes_valid_changes_and_keeps_last_good_settings_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut live = LiveSettings::open(path.clone()).unwrap();
        let first_poll = Instant::now() + Duration::from_secs(2);
        let mut settings = Settings::default();
        settings.set_curve(3.0).unwrap();
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
        use crate::effect::{Effect, EffectKind};
        s.timing.work_seconds = 900.try_into().unwrap();
        s.timing.grace_seconds = 60;
        s.timing.break_seconds = 210.try_into().unwrap();
        s.set_effect(EffectKind::Tears, Effect::new(false, 5.0, 0.7).unwrap());
        s.set_effect(EffectKind::Colour, Effect::new(true, 32.5, 0.65).unwrap());
        save(&path, &s).unwrap();
        let reopened = load(&path).unwrap();
        assert_eq!(reopened.timing, s.timing);
        assert!(!reopened.effect(EffectKind::Tears).enabled());
        assert_eq!(reopened.effect(EffectKind::Colour).entry(), 32.5);
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
