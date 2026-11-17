//! 排序命令只编排后台短事务，业务规则与原子索引写入由 core 维护。
use crate::commands::AppState;
use galfriends_core::{
    library_order::{Order, Sort},
    store,
};
use tauri::Manager;

#[tauri::command]
pub(crate) async fn set_library_sort(
    app: tauri::AppHandle,
    sort: Sort,
    descending: bool,
) -> Result<Order, String> {
    crate::diagnostics::spawn_background("set_library_sort", move || {
        let state = app.state::<AppState>();
        let mut guard = state.library.lock().map_err(|e| e.to_string())?;
        let library = guard.as_mut().map_err(|e| e.clone())?;
        store::set_library_sort(&state.path, library, sort, descending)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn reorder_characters(
    app: tauri::AppHandle,
    ids: Vec<String>,
    before: Option<String>,
) -> Result<Order, String> {
    crate::diagnostics::spawn_background("reorder_characters", move || {
        let state = app.state::<AppState>();
        let mut guard = state.library.lock().map_err(|e| e.to_string())?;
        let library = guard.as_mut().map_err(|e| e.clone())?;
        store::reorder_characters(&state.path, library, &ids, before.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}
