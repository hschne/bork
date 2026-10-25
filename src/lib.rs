//! Bork keeps break debt, interference tuning, and display ownership independent.
//! The background and editor adapters publish intent; only the guardian mutates the desktop.
#![deny(unsafe_code)]

pub mod artwork;
mod clock;
pub mod config;
pub mod control;
pub mod effect;
pub mod hyprland;
pub mod overtime;
pub mod preview;
pub mod recovery;
pub mod runtime;
pub mod timer;
pub mod wayland;
