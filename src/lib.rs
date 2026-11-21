//! platomon — Platonic-solid ASCII system monitor.
//!
//! The binary lives in `main.rs`; everything else is a library so the
//! renderer can be benchmarked and unit-tested.

pub mod ascii;
pub mod audio;
pub mod aspect;
pub mod clock;
pub mod color;
pub mod collector;
pub mod config;
pub mod metrics;
pub mod overlay;
#[cfg(all(windows, feature = "screensaver"))]
pub mod scrconfig;
pub mod screensaver;
pub mod scrlog;
#[cfg(windows)]
pub mod winconsole;
pub mod render;
pub mod scene;
pub mod tiles;
pub mod ui;
