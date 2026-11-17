//! 立绘选择与其他角色库写入共用事务锁，仅在落盘后发布选择。
use crate::commands::AppState;
use galfriends_core::{characters::ArtworkSelection, store};
use tauri::Manager;

#[tauri::command]
pub(crate) async fn select_artwork(
    app: tauri::AppHandle,
    selection: ArtworkSelection,
) -> Result<ArtworkSelection, String> {
    crate::diagnostics::spawn_background("select_artwork", move || {
        let state = app.state::<AppState>();
        let mut guard = state.library.lock().map_err(|e| e.to_string())?;
        let library = guard.as_mut().map_err(|e| e.clone())?;
        store::select_artwork(&state.path, library, &selection.id, selection.selected_art)?;
        drop(guard);
        crate::companion::refresh_artwork(&app, &selection.id);
        Ok(selection)
    })
    .await
    .map_err(|e| e.to_string())?
}
