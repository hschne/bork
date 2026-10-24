use anyhow::Result;
use bork::{config, control, hyprland::Hyprland, recovery};
use clap::{Parser, Subcommand};
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "An escalating visual break reminder for Hyprland",
    after_help = "Graphics configuration:\n  BORK_LIBRARY_PATH      Extra colon-separated library directories (highest priority)\n  BORK_NIX_DISCOVERY     auto (default), 0 to disable, or 1 to force\n  BORK_HYPRLAND_BINARY   Compositor executable to inspect for Nix library paths"
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
    /// Read or change user settings without connecting to the compositor.
    Config {
        #[command(subcommand)]
        command: ConfigAction,
    },
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

#[derive(Subcommand)]
enum ConfigAction {
    /// List effective settings, including defaults, under dotted keys.
    List {
        /// Print one JSON object instead of key=value lines.
        #[arg(long)]
        json: bool,
    },
    /// Print the effective value of one scalar setting.
    Get { key: String },
    /// Validate and atomically save a scalar setting (JSON number or boolean).
    Set { key: String, value: String },
    /// Restore one setting to its default, leaving other values unchanged.
    Unset { key: String },
    /// Print the user settings file path without reading or creating it.
    Path,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(
        &cli.command,
        None | Some(Action::Doctor | Action::Preview | Action::Guardian { .. })
    ) {
        bork::runtime::bootstrap()?;
    }
    match cli.command {
        Some(Action::Doctor) => {
            bork::runtime::describe();
            let hypr = Hyprland::from_env()?;
            let version = hypr.check()?;
            println!("Hyprland IPC and shader/redraw options available.\n{version}");
            bork::wayland::check()?;
            println!(
                "Wayland input-idle v2, layer-shell, shm, and input seat available. No display settings changed."
            );
            Ok(())
        }
        Some(Action::Preview) => bork::preview::open(),
        Some(Action::Config { command }) => configure(command),
        Some(Action::Status) => timer(control::Command::Status),
        Some(Action::Pause) => timer(control::Command::Pause),
        Some(Action::Resume) => timer(control::Command::Resume),
        Some(Action::Reset) => timer(control::Command::Reset),
        Some(Action::Guardian { runtime, socket }) => recovery::run_guardian(&runtime, socket),
        None => bork::background::run(),
    }
}

fn configure(command: ConfigAction) -> Result<()> {
    let path = config::path()?;
    let mut stdout = std::io::stdout().lock();
    match command {
        ConfigAction::Path => writeln!(stdout, "{}", path.display())?,
        ConfigAction::List { json } => {
            let values = config::list(&path)?;
            if json {
                serde_json::to_writer(&mut stdout, &values)?;
                writeln!(stdout)?;
            } else {
                for (key, value) in values {
                    writeln!(stdout, "{key}={value}")?;
                }
            }
        }
        ConfigAction::Get { key } => writeln!(stdout, "{}", config::get(&path, &key)?)?,
        ConfigAction::Set { key, value } => {
            config::update(&path, &key, Some(serde_json::from_str(&value)?))?;
        }
        ConfigAction::Unset { key } => config::update(&path, &key, None)?,
    }
    Ok(())
}

fn timer(command: control::Command) -> Result<()> {
    let status = control::execute(&Hyprland::from_env()?.runtime_directory()?, command)?;
    serde_json::to_writer(std::io::stdout().lock(), &status)?;
    println!();
    Ok(())
}
