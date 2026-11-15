use crate::commands::{self, AppState};
use galfriends_core::{sources, store};
use tauri::Manager;
pub(crate) fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(crate::connections::Tasks::default());
            app.manage(crate::voice::Tasks::default());
            app.manage(crate::companion::Sessions::default());
            app.manage(crate::companion::emotions::Tasks::default());
            let credentials_path = app.path().app_data_dir()?.join("library.json");
            let executable = std::env::current_exe()?;
            let path = executable
                .parent()
                .ok_or("Missing executable directory")?
                .join("characters/library.json");
            // 保留加载错误给界面显示，不以空库覆盖损坏数据。
            let library = store::load_for_migration(&path, &credentials_path).and_then(|mut l| {
                if !path.exists() && !credentials_path.exists() {
                    let raw: serde_json::Value = serde_json::from_str(include_str!(
                        "../../fixtures/setsuna-kungal-search.json"
                    ))
                    .map_err(|e| e.to_string())?;
                    if let Some(c) = l.characters.first_mut() {
                        c.merge(sources::from_kungal(raw["items"][0].clone())?);
                    }
                }
                Ok(l)
            });
            app.manage(AppState {
                path,
                credentials_path,
                library: std::sync::Mutex::new(library),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::companion::emotions::emotion_page,
            crate::companion::emotions::emotion_recommend,
            crate::companion::emotions::emotion_cancel,
            crate::companion::emotions::emotion_choose,
            crate::companion::emotions::emotion_preview,
            crate::companion::companion_open,
            crate::companion::companion_profile,
            crate::companion::companion_bind,
            crate::companion::companion_models,
            crate::companion::companion_chat,
            crate::companion::companion_speak,
            crate::companion::companion_cancel,
            crate::voice::voice_run,
            crate::voice::voice_download,
            crate::voice::voice_cancel,
            commands::load_library,
            crate::game_library::game_library_load,
            crate::game_library::refresh_character_works,
            crate::game_library::open_work,
            crate::character_tags::update_character_tags,
            crate::connections::connection_statuses,
            crate::connections::connection_action,
            crate::connections::cancel_connection,
            crate::connections::open_connection_website,
            crate::preferences::settings_environment,
            crate::preferences::set_appearance,
            commands::search_characters,
            commands::import_character,
            commands::mutate_library,
            crate::artwork_selection::select_artwork,
            commands::batch_characters,
            commands::save_collection,
            commands::delete_collections,
            commands::open_source,
            commands::enhance_artwork,
            crate::artwork::reprocess_artworks
        ])
        .run(tauri::generate_context!())
}
