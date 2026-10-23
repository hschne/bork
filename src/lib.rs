#![deny(unsafe_code)]

pub mod artwork;
pub mod background;
pub mod config;
pub mod preview;
// Glow calls are isolated here; each unsafe block documents current-context ownership.
pub mod controller;
pub mod effect;
pub mod hyprland;
pub mod recovery;
#[allow(unsafe_code)]
pub mod renderer;
pub mod runtime;
pub mod wayland;
