//! 打开弹窗只读原始表情；确认后才启动单项合成，提交时再次核对源图。
pub(crate) mod tasks;
use crate::commands::{AppState, read, update};
use galfriends_core::{
    artwork::expressions,
    characters::{Artwork, ExpressionPreview, Library},
};
use std::{path::Path, sync::Mutex};
use tauri::Manager;

#[derive(Default)]
pub(crate) struct Sessions(Mutex<Option<Session>>);
#[derive(Clone)]
struct Session {
    id: String,
    index: usize,
    artwork: Artwork,
    token: String,
    choices: Vec<expressions::Choice>,
}

#[tauri::command]
pub(crate) async fn expression_preview(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    id: String,
    discover: Option<bool>,
) -> Result<ExpressionPreview, String> {
    if window.label() != "main" {
        return Err("Main window required".into());
    }
    let library = read(&app.state::<AppState>())?;
    let character = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("expression.changed")?;
    let index = character.selected_art;
    let artwork = character.artwork().ok_or("expression.changed")?.clone();
    let state = app.state::<AppState>();
    let catalog = expressions::catalog_path(
        state.path.parent().ok_or("Missing character directory")?,
        &id,
    )?;
    crate::diagnostics::spawn_background("expression_preview", move || {
        if discover.unwrap_or(true)
            && let Err(error) = tasks::start(&app, &id)
        {
            tasks::report_error(&app, &id, error);
        }
        let choices = expressions::choices(&catalog, &id)?;
        let token = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
            .to_string();
        let preview = ExpressionPreview {
            token: token.clone(),
            variants: choices
                .iter()
                .map(|choice| choice.preview.clone())
                .collect(),
        };
        *app.state::<Sessions>()
            .0
            .lock()
            .map_err(|e| e.to_string())? = Some(Session {
            id,
            index,
            artwork,
            token,
            choices,
        });
        Ok(preview)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn expression_apply(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    token: String,
    index: usize,
) -> Result<Library, String> {
    if window.label() != "main" {
        return Err("Main window required".into());
    }
    let session = app
        .state::<Sessions>()
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("expression.changed")?;
    if session.token != token || index >= session.choices.len() {
        return Err("expression.changed".into());
    }
    crate::diagnostics::spawn_background("expression_apply", move || {
        // 确认后才读取目标像素并调用 Python。先检查过期选择，避免无意义的合成。
        let current = read(&app.state::<AppState>())?;
        expressions::validate_input(&current, &session.id, session.index, &session.artwork)?;
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let variant = expressions::compose(
            &project.join(".venv/Scripts/python.exe"),
            &project.join("character_expressions.py"),
            &project.join("dist"),
            &session.artwork,
            &session.choices[index],
        )?;
        let library = update(&app.state::<AppState>(), |library| {
            expressions::apply(
                library,
                &session.id,
                session.index,
                &session.artwork,
                &variant,
            )
        })?;
        crate::companion::refresh_artwork(&app, &session.id);
        Ok(library)
    })
    .await
    .map_err(|e| e.to_string())?
}
