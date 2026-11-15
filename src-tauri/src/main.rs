#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod artwork;
mod artwork_selection;
mod character_tags;
mod commands;
mod companion;
mod connections;
mod desktop;
mod game_library;
mod preferences;
mod voice;
fn main() {
    if let Err(error) = desktop::run() {
        eprintln!("GalFriends: {error}");
    }
}
