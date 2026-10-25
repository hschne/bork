//! Write-ahead recovery journal. Restore only settings still owned by this process.
use crate::hyprland::Hyprland;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

/// The journal cannot name settings outside the guardian's ownership contract.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
enum Setting {
    #[serde(rename = "debug:damage_tracking")]
    DamageTracking,
    #[serde(rename = "debug:vfr")]
    VariableFrameRate,
    #[serde(rename = "decoration:screen_shader")]
    ScreenShader,
}

impl Setting {
    // Reverse restoration removes the shader before restoring normal redraw policy.
    const INSTALL_ORDER: [Self; 3] = [
        Self::DamageTracking,
        Self::VariableFrameRate,
        Self::ScreenShader,
    ];
    fn key(self) -> &'static str {
        match self {
            Self::DamageTracking => "debug:damage_tracking",
            Self::VariableFrameRate => "debug:vfr",
            Self::ScreenShader => "decoration:screen_shader",
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Change {
    key: Setting,
    before: String,
    installed: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Journal {
    socket: PathBuf,
    changes: Vec<Change>,
}

fn write_journal(path: &Path, journal: &Journal) -> Result<()> {
    let temporary = path.join("journal.new");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer(&mut file, journal)?;
    file.sync_all()?;
    fs::rename(temporary, path.join("journal.json"))?;
    File::open(path)?.sync_all()?;
    Ok(())
}

/// Compare-and-restore values still owned by Bork.
pub fn restore(path: &Path) -> Result<()> {
    let journal_path = path.join("journal.json");
    if !journal_path.exists() {
        return Ok(());
    }
    let mut journal: Journal = serde_json::from_reader(File::open(&journal_path)?)?;
    let hypr = Hyprland::at_socket(journal.socket.clone());
    let mut failures = Vec::new();
    for index in (0..journal.changes.len()).rev() {
        let change = &journal.changes[index];
        let result = (|| -> Result<()> {
            let current = hypr.get(change.key.key())?;
            if current == change.before {
                return Ok(());
            }
            ensure!(
                current == change.installed,
                "{} changed externally; leaving it untouched",
                change.key.key()
            );
            hypr.set(change.key.key(), &change.before)?;
            ensure!(
                hypr.get(change.key.key())? == change.before,
                "{} restoration did not stick",
                change.key.key()
            );
            Ok(())
        })();
        match result {
            Ok(()) => {
                journal.changes.remove(index);
                write_journal(path, &journal)?;
            }
            Err(error) => failures.push(format!("{error:#}")),
        }
    }
    if journal.changes.is_empty() {
        fs::remove_file(journal_path)?;
        let _ = fs::remove_file(path.join("effect.frag"));
        Ok(())
    } else {
        bail!(
            "Recovery incomplete: {}. Journal retained at {}",
            failures.join("; "),
            journal_path.display()
        )
    }
}

pub(super) fn install(path: &Path, hypr: &Hyprland, shader: &str) -> Result<()> {
    restore(path)?;
    fs::write(path.join("effect.frag"), shader)?;
    let shader_path = path.join("effect.frag");
    let shader_path = shader_path
        .to_str()
        .context("Hyprland shader paths must be UTF-8")?;
    let mut journal = Journal {
        socket: hypr.socket_path().to_owned(),
        changes: Vec::new(),
    };
    for (key, value) in Setting::INSTALL_ORDER
        .into_iter()
        .zip(["0", "0", shader_path])
    {
        let before = hypr.get(key.key())?;
        journal.changes.push(Change {
            key,
            before,
            installed: value.into(),
        });
        // Write ahead: an ambiguous IPC reply or process death still has an exact undo record.
        write_journal(path, &journal)?;
        hypr.set(key.key(), value)?;
        ensure!(
            hypr.get(key.key())? == value,
            "{} did not retain the installed value",
            key.key()
        );
    }
    Ok(())
}

pub(super) fn update(path: &Path, hypr: &Hyprland, shader: &str) -> Result<()> {
    let journal: Journal = serde_json::from_reader(File::open(path.join("journal.json"))?)?;
    ensure!(
        journal.socket == hypr.socket_path()
            && journal
                .changes
                .iter()
                .map(|change| change.key)
                .eq(Setting::INSTALL_ORDER),
        "Incomplete installation journal"
    );
    for change in &journal.changes {
        ensure!(
            hypr.get(change.key.key())? == change.installed,
            "{} changed externally; stopping interference",
            change.key.key()
        );
    }
    let temporary = path.join("effect.new");
    fs::write(&temporary, shader)?;
    fs::rename(temporary, path.join("effect.frag"))?;
    hypr.set(
        Setting::ScreenShader.key(),
        path.join("effect.frag")
            .to_str()
            .context("Invalid shader path")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_journal_settings_are_retained_without_connecting_to_a_compositor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "socket": path.join("absent.sock"),
            "changes": [{"key":"unmanaged:option", "before":"a", "installed":"b"}],
        }))
        .unwrap();
        fs::write(path.join("journal.json"), &bytes).unwrap();
        assert!(
            restore(path)
                .unwrap_err()
                .to_string()
                .contains("unmanaged:option")
        );
        assert_eq!(fs::read(path.join("journal.json")).unwrap(), bytes);
    }
}
