#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod artwork;
mod artwork_selection;
mod character_tags;
mod commands;
mod companion;
mod connections;
mod desktop;
mod diagnostics;
mod expressions;
mod game_library;
mod library_order;
mod preferences;
mod updates;
mod voice;
fn main() {
    #[cfg(all(windows, target_arch = "x86_64"))]
    if updates::run_from_args() {
        return;
    }
    diagnostics::initialize();
    if let Err(error) = desktop::run() {
        eprintln!("GalFriends: {error}");
        diagnostics::startup_failed(&error);
    }
}
