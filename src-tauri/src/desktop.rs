use crate::commands::{self, AppState};
use galfriends_core::{sources, store};
use tauri::Manager;
pub(crate) fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("diagnostics")
                .js_init_script(crate::diagnostics::FRONTEND_SCRIPT)
                .build(),
        )
        .on_page_load(|window, payload| {
            log::info!(
                "page_load window={} event={:?}",
                window.label(),
                payload.event()
            );
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                log::info!("window_close_requested window={}", window.label());
            }
        })
        .setup(|app| {
            log::info!(
                "desktop_setup available_webview_version={:?}",
                tauri::webview_version()
            );
            if let Some(window) = app.get_webview_window("main") {
                crate::diagnostics::watch_window(&window);
            }
            app.manage(crate::connections::Tasks::default());
            app.manage(crate::expressions::Sessions::default());
            app.manage(crate::expressions::tasks::Tasks::default());
            app.manage(crate::voice::Tasks::default());
            app.manage(crate::updates::Updates::default());
            app.manage(crate::game_library::CatalogState::default());
            app.manage(crate::companion::Sessions::default());
            app.manage(crate::companion::emotions::Tasks::default());
            let credentials_path = app.path().app_data_dir()?.join("library.json");
            let executable = std::env::current_exe()?;
            let path = executable
                .parent()
                .ok_or("Missing executable directory")?
                .join("characters/library.json");
            log::info!("loading_state library={}", path.display());
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
            if let Err(error) = &library {
                log::error!("library_load_failed error={error}");
            }
            app.manage(AppState {
                path,
                credentials_path,
                library: std::sync::Mutex::new(library),
            });
            log::info!("desktop_ready");
            crate::expressions::tasks::resume(app.handle());
            Ok(())
        })
        .invoke_handler(crate::diagnostics::trace_commands(
            tauri::generate_handler![
                crate::library_order::set_library_sort,
                crate::library_order::reorder_characters,
                crate::diagnostics::record_diagnostic,
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
                crate::game_library::choose_work_folder,
                crate::game_library::open_work_folder,
                crate::character_tags::update_character_tags,
                crate::connections::connection_statuses,
                crate::connections::connection_action,
                crate::connections::cancel_connection,
                crate::connections::open_connection_website,
                crate::preferences::settings_environment,
                crate::preferences::set_appearance,
                crate::updates::check_app_update,
                crate::updates::check_skill_update,
                crate::updates::install_app_update,
                crate::updates::install_skill_update,
                crate::updates::cancel_update,
                commands::search_characters,
                commands::import_character,
                commands::mutate_library,
                crate::artwork_selection::select_artwork,
                crate::expressions::expression_preview,
                crate::expressions::expression_apply,
                crate::expressions::tasks::expression_scan_status,
                crate::expressions::tasks::expression_scan_pause,
                commands::batch_characters,
                commands::save_collection,
                commands::delete_collections,
                commands::open_source,
                commands::enhance_artwork,
                crate::artwork::reprocess_artworks
            ],
        ))
        .build(tauri::generate_context!())?
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code, .. } => {
                log::info!("exit_requested code={code:?}");
            }
            tauri::RunEvent::Exit => {
                crate::expressions::tasks::shutdown(app);
                log::info!("shutdown_start");
                crate::diagnostics::finish();
            }
            _ => {}
        });
    Ok(())
}
