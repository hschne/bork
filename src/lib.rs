//! Bork keeps break debt, interference tuning, and display ownership independent.
//! The background and editor adapters publish intent; only the guardian mutates the desktop.
#![deny(unsafe_code)]

pub mod artwork;
pub mod background;
mod clock;
pub mod config;
pub mod control;
mod cycle;
pub mod effect;
pub mod hyprland;
pub mod preview;
pub mod recovery;
pub mod runtime;
pub mod wayland;
