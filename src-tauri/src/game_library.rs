//! 用户指定的 GalGear 库只读接入；鲲 Gal 作品快照只写入 GalFriends 角色归档。
use crate::commands::{AppState, update};
use galfriends_core::{
    characters::Library,
    characters::Source,
    game_library::{self, Catalog},
    sources,
    voice::Input,
};
use tauri::Manager;

fn configured_path(app: &tauri::AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let guard = state.library.lock().map_err(|e| e.to_string())?;
    Ok(guard
        .as_ref()
        .map_err(Clone::clone)?
        .settings
        .galgear_library_path
        .clone())
}

#[tauri::command]
pub(crate) async fn game_library_load(app: tauri::AppHandle) -> Result<Catalog, String> {
    let path = configured_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        if path.is_empty() {
            Ok(Catalog::default())
        } else {
            game_library::load(std::path::Path::new(&path))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn refresh_character_works(
    app: tauri::AppHandle,
    id: String,
) -> Result<Library, String> {
    let (snapshots, proxy, credentials) = {
        let state = app.state::<AppState>();
        let guard = state.library.lock().map_err(|e| e.to_string())?;
        let library = guard.as_ref().map_err(Clone::clone)?;
        let character = library
            .characters
            .iter()
            .find(|c| c.id == id)
            .ok_or("voice.character_required")?;
        (
            character.snapshots.clone(),
            library.settings.proxy.clone(),
            state.credentials_path.clone(),
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        let result = sources::tag_lookup::fetch_for(&snapshots, Source::Kungal, |source| {
            crate::connections::catalog_client(&credentials, &proxy, source)
        })?
        .ok_or("game_library.no_works")?;
        update(&app.state::<AppState>(), |library| {
            let current = library
                .characters
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or("voice.character_required")?;
            if current.snapshots != snapshots {
                return Err("game_library.changed".into());
            }
            current.merge(result);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn open_work(id: String) -> Result<(), String> {
    if !game_library::numeric_id(&id) {
        return Err("Invalid work ID".into());
    }
    open::that_detached(format!("https://www.kungal.com/galgame/{id}")).map_err(|e| e.to_string())
}

pub(crate) fn resolve_training_game(
    app: &tauri::AppHandle,
    input: &Input,
) -> Result<String, String> {
    let path = configured_path(app)?;
    let catalog = game_library::load(std::path::Path::new(&path))?;
    if input.catalog_path != catalog.path {
        return Err("game_library.changed".into());
    }
    let state = app.state::<AppState>();
    let guard = state.library.lock().map_err(|e| e.to_string())?;
    let library = guard.as_ref().map_err(Clone::clone)?;
    if library.settings.galgear_library_path != path {
        return Err("game_library.changed".into());
    }
    let character = library
        .characters
        .iter()
        .find(|character| character.id == input.character_id)
        .ok_or("voice.character_required")?;
    let game = game_library::linked_games(character, &catalog)
        .into_iter()
        .find(|game| game.id == input.game_id)
        .ok_or("game_library.changed")?;
    if !game.available {
        return Err("game_library.unavailable".into());
    }
    // UI 中展示并选中的路径必须仍然有效，库移动后要求重新选择。
    if game.folder != input.game_dir {
        return Err("game_library.changed".into());
    }
    Ok(game.folder.clone())
}
