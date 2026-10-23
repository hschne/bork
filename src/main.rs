use anyhow::Result;
use clap::{Parser, Subcommand};
use signal_loss::{control, hyprland::Hyprland, recovery};
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
    /// Print the background timer status as one JSON object.
    Status,
    /// Freeze work debt and remove interference/test cards.
    Pause,
    /// Continue the paused timer with its preserved work debt.
    Resume,
    /// Start a fresh running interval and clear effects; also performs emergency recovery.
    Reset,
    #[command(hide = true)]
    Guardian { runtime: PathBuf, socket: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(
        &cli.command,
        None | Some(Action::Doctor | Action::Preview | Action::Guardian { .. })
    ) {
        signal_loss::runtime::bootstrap()?;
    }
    match cli.command {
        Some(Action::Doctor) => {
            signal_loss::runtime::describe();
            let hypr = Hyprland::from_env()?;
            let version = hypr.check()?;
            println!("Hyprland IPC and shader/redraw options available.\n{version}");
            signal_loss::wayland::check()?;
            println!(
                "Wayland input-idle v2, layer-shell, shm, and input seat available. No display settings changed."
            );
            Ok(())
        }
        Some(Action::Preview) => signal_loss::preview::open(),
        Some(Action::Status) => timer(control::Command::Status),
        Some(Action::Pause) => timer(control::Command::Pause),
        Some(Action::Resume) => timer(control::Command::Resume),
        Some(Action::Reset) => timer(control::Command::Reset),
        Some(Action::Guardian { runtime, socket }) => recovery::run_guardian(&runtime, socket),
        None => signal_loss::background::run(),
    }
}

fn timer(command: control::Command) -> Result<()> {
    let status = control::execute(&Hyprland::from_env()?.runtime_directory()?, command)?;
    serde_json::to_writer(std::io::stdout().lock(), &status)?;
    println!();
    Ok(())
}
