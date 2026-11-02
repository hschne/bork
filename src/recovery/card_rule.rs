//! A private, journaled layer-rule lease; never edits the user's animation settings.
use crate::{effect, hyprland::Hyprland};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lease {
    socket: PathBuf,
    namespace: String,
}

pub(super) fn install(path: &Path, hypr: &Hyprland) -> Result<String> {
    let journal = path.join("card-rule.json");
    let lease: Lease = if journal.exists() {
        let lease: Lease = serde_json::from_reader(File::open(&journal)?)?;
        ensure!(
            lease.socket == hypr.socket_path(),
            "Card rule belongs to another compositor"
        );
        lease
    } else {
        // This name is Bork-owned, not a user's named rule or a global option.
        let lease = Lease {
            socket: hypr.socket_path().to_owned(),
            namespace: format!(
                "bork-{:08x}{:08x}",
                effect::fresh_seed()?,
                effect::fresh_seed()?
            ),
        };
        let mut file = File::create(path.join("card-rule.new"))?;
        serde_json::to_writer(&mut file, &lease)?;
        file.sync_all()?;
        fs::rename(path.join("card-rule.new"), &journal)?;
        File::open(path)?.sync_all()?;
        lease
    };
    // Write ahead of even the first IPC mutation, including ambiguous failures.
    hypr.card_rule(&lease.namespace, true)?;
    Ok(lease.namespace)
}

pub(super) fn restore(path: &Path) -> Result<()> {
    let journal = path.join("card-rule.json");
    if !journal.exists() {
        return Ok(());
    }
    let lease: Lease = serde_json::from_reader(File::open(&journal)?)?;
    // Hyprland exposes no rule removal. Disable only this private rule; it then
    // has no effect, and a compositor config reload discards it altogether.
    Hyprland::at_socket(lease.socket).card_rule(&lease.namespace, false)?;
    fs::remove_file(journal)?;
    Ok(())
}
