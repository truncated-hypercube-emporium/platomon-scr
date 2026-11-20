//! `platomon-config.exe`: opens the screensaver's configuration dialog
//! ([`platomon::scrconfig::ScreenSaverConfigureDialog`]) on its own, the same
//! dialog Screen Saver Settings › Settings… shows for `platomon.scr /c`.
//! Settings are saved to `%APPDATA%\platomon\screensaver.ini`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    platomon::scrconfig::configure(0);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("platomon-config configures the Windows screensaver; it only runs on Windows.");
    std::process::exit(2);
}
