//! 用户指定的 GalGear 库只读接入；鲲 Gal 作品快照只写入 GalFriends 角色归档。
use crate::commands::{AppState, update};
use galfriends_core::{
    characters::{Library, Source},
    game_library::{self, Catalog},
    sources,
    voice::Input,
};
use std::{path::Path, sync::Mutex};
use tauri::Manager;

/// 打开目录与界面共用最近一次显式读取的库。
#[derive(Default)]
pub(crate) struct CatalogState(Mutex<Option<(String, Catalog)>>);

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
    crate::diagnostics::spawn_background("game_library_load", move || {
        let result = if path.is_empty() {
            Ok(Catalog::default())
        } else {
            game_library::load(Path::new(&path))
        };
        let state = app.state::<AppState>();
        let library = state.library.lock().map_err(|e| e.to_string())?;
        if library
            .as_ref()
            .map_err(Clone::clone)?
            .settings
            .galgear_library_path
            == path
        {
            let cache = app.state::<CatalogState>();
            *cache.0.lock().map_err(|e| e.to_string())? =
                result.as_ref().ok().map(|catalog| (path, catalog.clone()));
        }
        result
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
    crate::diagnostics::spawn_background("refresh_character_works", move || {
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

#[tauri::command]
pub(crate) async fn choose_work_folder(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    id: String,
    work_id: String,
) -> Result<Option<Library>, String> {
    if !game_library::numeric_id(&work_id) {
        return Err("game_library.changed".into());
    }
    let title = {
        let state = app.state::<AppState>();
        let library = state.library.lock().map_err(|e| e.to_string())?;
        let library = library.as_ref().map_err(Clone::clone)?;
        let language =
            galfriends_i18n::Language::from_code(&library.settings.language).unwrap_or_default();
        galfriends_i18n::tr(language, "game_library.pick_folder")
    };
    let Some(folder) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_title(title)
        .pick_folder()
        .await
    else {
        return Ok(None);
    };
    crate::diagnostics::spawn_background("choose_work_folder", move || {
        let folder = game_library::validate_work_folder(folder.path())?.to_owned();
        update(&app.state::<AppState>(), |library| {
            let character = library
                .characters
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or("voice.character_required")?;
            game_library::set_work_folder(character, work_id, folder)
        })
        .map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn associated_folder(app: &tauri::AppHandle, id: &str, work_id: &str) -> Result<String, String> {
    let state = app.state::<AppState>();
    let library = state.library.lock().map_err(|e| e.to_string())?;
    let library = library.as_ref().map_err(Clone::clone)?;
    let character = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("voice.character_required")?;
    let work = game_library::works(character)
        .into_iter()
        .find(|work| work.id == work_id)
        .ok_or("game_library.changed")?;
    if let Some(folder) = character.work_folders.get(work_id) {
        return Ok(folder.clone());
    }
    let cache = app.state::<CatalogState>();
    let cached = cache.0.lock().map_err(|e| e.to_string())?;
    let game = cached
        .as_ref()
        .filter(|(path, _)| *path == library.settings.galgear_library_path)
        .and_then(|(_, catalog)| game_library::work_installation(&work, catalog))
        .filter(|game| game.available)
        .ok_or("game_library.unavailable")?;
    Ok(game.folder.clone())
}

#[tauri::command]
pub(crate) async fn open_work_folder(
    app: tauri::AppHandle,
    id: String,
    work_id: String,
) -> Result<(), String> {
    crate::diagnostics::spawn_background("open_work_folder", move || {
        let folder = associated_folder(&app, &id, &work_id)?;
        let folder = game_library::validate_work_folder(Path::new(&folder))?;
        open::that_detached(folder).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn resolve_training_game(
    app: &tauri::AppHandle,
    input: &Input,
) -> Result<String, String> {
    if let Some(work_id) = input.game_id.strip_prefix(game_library::LOCAL_GAME_PREFIX) {
        let folder = {
            let state = app.state::<AppState>();
            let library = state.library.lock().map_err(|e| e.to_string())?;
            let library = library.as_ref().map_err(Clone::clone)?;
            library
                .characters
                .iter()
                .find(|c| c.id == input.character_id)
                .and_then(|c| game_library::work_folder(c, work_id))
                .filter(|folder| *folder == input.game_dir)
                .ok_or("game_library.changed")?
                .to_owned()
        };
        return game_library::validate_work_folder(Path::new(&folder)).map(str::to_owned);
    }
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
