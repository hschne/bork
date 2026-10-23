use crate::effect::Settings;
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
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
    Ok(root.join("signal-loss/settings.json"))
}

pub fn load(path: &Path) -> Result<Settings> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "Settings file exceeds 64 KiB");
    let settings: Settings =
        serde_json::from_slice(&bytes).context("Invalid Signal Loss settings")?;
    settings.validate()?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    settings.validate()?;
    let parent = path.parent().context("Missing config directory")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.new");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, settings)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_reopen_and_invalid_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/settings.json");
        let mut s = load(&path).unwrap();
        s.tears.enabled = false;
        s.colour.entry = 32.5;
        save(&path, &s).unwrap();
        let reopened = load(&path).unwrap();
        assert!(!reopened.tears.enabled);
        assert_eq!(reopened.colour.entry, 32.5);
        fs::write(&path, "{\"curve\":0}").unwrap();
        assert!(load(&path).is_err());
    }
}
