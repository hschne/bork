use anyhow::Result;
use clap::{Parser, Subcommand};
use signal_loss::{hyprland::Hyprland, recovery};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "An escalating visual break reminder for Hyprland",
    after_help = "Graphics configuration:\n  SIGNAL_LOSS_LIBRARY_PATH      Extra colon-separated library directories (highest priority)\n  SIGNAL_LOSS_NIX_DISCOVERY     auto (default), 0 to disable, or 1 to force\n  SIGNAL_LOSS_HYPRLAND_BINARY   Compositor executable to inspect for Nix library paths"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Check compositor IPC and required rendering settings without changing them.
    Doctor,
    /// Open the shader tuning editor; ordinary desktop settings remain unchanged.
    Preview,
    /// Stop the display owner and restore its exact previous settings.
    Stop,
    #[command(hide = true)]
    Guardian { runtime: PathBuf, socket: PathBuf },
}

fn runtime() -> Result<PathBuf> {
    let hypr = Hyprland::from_env()?;
    hypr.runtime_directory()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !matches!(&cli.command, Some(Action::Stop)) {
        signal_loss::runtime::bootstrap()?;
    }
    match cli.command {
        Some(Action::Doctor) => {
            signal_loss::runtime::describe();
            let hypr = Hyprland::from_env()?;
            let version = hypr.check()?;
            println!("Hyprland IPC and shader/redraw options available.\n{version}");
            let _wayland = signal_loss::wayland::Wayland::connect(false, 300)?;
            println!(
                "Wayland input-idle v2, layer-shell, shm, and input seat available. No display settings changed."
            );
            Ok(())
        }
        Some(Action::Preview) => signal_loss::preview::open(),
        Some(Action::Stop) => recovery::stop(&runtime()?),
        Some(Action::Guardian { runtime, socket }) => recovery::run_guardian(&runtime, socket),
        None => signal_loss::background::run(),
    }
}
