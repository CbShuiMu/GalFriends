//! 当前应用的设置事务、只读路径与通用图片工具探测。
use crate::commands::AppState;
use galfriends_core::preferences::{Appearance, DisplayPreferences, Environment};
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

pub(crate) fn display_preferences(app: &tauri::AppHandle) -> Result<DisplayPreferences, String> {
    let state = app.state::<AppState>();
    let guard = state.library.lock().map_err(|e| e.to_string())?;
    Ok(DisplayPreferences::from(
        &guard.as_ref().map_err(Clone::clone)?.settings,
    ))
}

pub(crate) fn broadcast_display_preferences(app: &tauri::AppHandle) {
    let send = || -> Result<(), String> {
        let state = app.state::<AppState>();
        let guard = state.library.lock().map_err(|e| e.to_string())?;
        let preferences = DisplayPreferences::from(&guard.as_ref().map_err(Clone::clone)?.settings);
        // 读取最新已提交值并在同一短锁内发送，避免并发保存的通知倒序覆盖。
        app.emit("display-preferences-changed", &preferences)
            .map_err(|e| e.to_string())
    };
    if let Err(error) = send() {
        // 保存已经成功；窗口退出造成通知失败不能把已提交的设置伪装成保存失败。
        log::warn!("display_preferences_notification_failed error={error}");
    }
}

#[tauri::command]
pub(crate) async fn set_appearance(
    app: tauri::AppHandle,
    key: String,
    value: String,
) -> Result<Appearance, String> {
    let handle = app.clone();
    crate::diagnostics::spawn_background("set_appearance", move || {
        let state = app.state::<AppState>();
        let mut guard = state.library.lock().map_err(|e| e.to_string())?;
        let library = guard.as_mut().map_err(|e| e.clone())?;
        galfriends_core::store::set_appearance(&state.path, library, &key, &value)
    })
    .await
    .map_err(|e| e.to_string())?
    .inspect(|_| broadcast_display_preferences(&handle))
}

pub(crate) fn waifu2x_directory(resources: &Path) -> Option<PathBuf> {
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?;
    [resources.join("tools/waifu2x"), dev.join("tools/waifu2x")]
        .into_iter()
        .find(|p| p.join("waifu2x-ncnn-vulkan.exe").is_file())
}

#[tauri::command]
pub(crate) async fn settings_environment(app: tauri::AppHandle) -> Result<Environment, String> {
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let library = app.state::<AppState>().path.clone();
    crate::diagnostics::spawn_background("settings_environment", move || {
        Ok(Environment {
            library_path: library.display().to_string(),
            cache_path: cache.display().to_string(),
            waifu2x_path: waifu2x_directory(&resources)
                .map(|p| p.join("waifu2x-ncnn-vulkan.exe").display().to_string()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
