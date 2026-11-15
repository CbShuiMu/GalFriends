pub(crate) mod emotions;
mod speech;
// 桌面角色窗口与任务编排。会话按窗口隔离，取消使迟到结果失效。
use crate::commands::AppState;
use galfriends_core::{
    companion::{Dialogue, Profile, Turn, storage},
    connections::{Service, chat, credentials},
    store,
    voice::{Engine, TrainedModel, catalog},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[derive(Default)]
pub(crate) struct Sessions {
    entries: Mutex<BTreeMap<String, Session>>,
    bindings: Mutex<()>,
}
struct Session {
    profile: Profile,
    focused: std::time::Instant,
    history: Vec<Turn>,
    active: Option<Arc<AtomicBool>>,
}
impl Session {
    fn cancel(&mut self) {
        if let Some(cancel) = self.active.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn binding_path(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(storage::binding_path(
        app.state::<AppState>()
            .path
            .parent()
            .ok_or("companion.failed")?,
        id,
    ))
}

fn profile(app: &tauri::AppHandle, id: &str) -> Result<Profile, String> {
    let state = app.state::<AppState>();
    let library = state.library.lock().map_err(|_| "companion.failed")?;
    let library = library.as_ref().map_err(Clone::clone)?;
    let c = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("voice.character_required")?;
    let artwork = c.artwork().ok_or("no_art")?;
    Ok(Profile {
        id: c.id.clone(),
        name: c.name.clone(),
        introduction: c.summary.clone(),
        artwork: artwork.enhanced.as_deref().unwrap_or(&artwork.url).into(),
        language: library.settings.language.clone(),
        theme: library.settings.theme.clone(),
        color_theme: library.settings.color_theme.clone(),
        font_size: library.settings.font_size,
        model_dir: String::new(),
        voice: Default::default(),
    })
}

#[tauri::command]
pub(crate) async fn companion_open(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("companion.failed".into());
    }
    let label = storage::label(&id);
    if let Some(existing) = app.get_webview_window(&label) {
        return existing.set_focus().map_err(|e| e.to_string());
    }
    let mut profile = profile(&app, &id)?;
    profile.voice = storage::bindings(&binding_path(&app, &id)?)?
        .remove(&id)
        .unwrap_or_default();
    profile.model_dir = profile.voice.directory().to_owned();
    let title = profile.name.clone();
    let index = {
        let sessions = app.state::<Sessions>();
        let mut entries = sessions.entries.lock().map_err(|_| "companion.failed")?;
        let index = entries.len();
        entries.insert(
            label.clone(),
            Session {
                profile,
                focused: std::time::Instant::now(),
                history: Vec::new(),
                active: None,
            },
        );
        index
    };
    let created = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("companion.html".into()))
        .title(title)
        .inner_size(380., 680.)
        .min_inner_size(320., 520.)
        .position(80. + (index % 3) as f64 * 390., 70.)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .resizable(true)
        .skip_taskbar(false)
        .build();
    match created {
        Ok(window) => {
            let handle = app.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Focused(true))
                    && let Ok(mut entries) = handle.state::<Sessions>().entries.lock()
                    && let Some(session) = entries.get_mut(&label)
                {
                    session.focused = std::time::Instant::now();
                }
                if matches!(event, tauri::WindowEvent::Destroyed)
                    && let Ok(mut entries) = handle.state::<Sessions>().entries.lock()
                {
                    entries.remove(&label);
                }
            });
            Ok(())
        }
        Err(error) => {
            app.state::<Sessions>()
                .entries
                .lock()
                .map_err(|_| "companion.failed")?
                .remove(&label);
            Err(error.to_string())
        }
    }
}

#[tauri::command]
pub(crate) fn companion_profile(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<Profile, String> {
    let mut profile = app
        .state::<Sessions>()
        .entries
        .lock()
        .map_err(|_| "companion.failed")?
        .get(window.label())
        .map(|s| s.profile.clone())
        .ok_or("companion.failed")?;
    // 窗口创建和前端订阅之间可能发生设置变更，初始化时重新读取已提交值。
    profile.apply_display_preferences(&crate::preferences::display_preferences(&app)?);
    Ok(profile)
}

#[tauri::command]
pub(crate) async fn companion_models(
    app: tauri::AppHandle,
    window: WebviewWindow,
    engine: Engine,
) -> Result<Vec<TrainedModel>, String> {
    let profile = companion_profile(app.clone(), window)?;
    let file = crate::voice::catalog_file(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        catalog::models(&file, &profile.id, engine, &profile.voice)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn companion_bind(
    app: tauri::AppHandle,
    window: WebviewWindow,
    directory: String,
    engine: galfriends_core::voice::Engine,
) -> Result<galfriends_core::companion::VoiceModels, String> {
    let profile = companion_profile(app.clone(), window.clone())?;
    let label = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let sessions = app.state::<Sessions>();
        let _transaction = sessions.bindings.lock().map_err(|_| "companion.failed")?;
        let state = app.state::<AppState>();
        let library = state.library.lock().map_err(|_| "companion.failed")?;
        if !library
            .as_ref()
            .map_err(Clone::clone)?
            .characters
            .iter()
            .any(|character| character.id == profile.id)
        {
            return Err("voice.character_required".into());
        }
        store::ensure_writable(&state.path)?;
        let selection = storage::bind(
            &binding_path(&app, &profile.id)?,
            &profile.id,
            Path::new(&directory),
            engine,
        )?;
        if let Some(session) = sessions
            .entries
            .lock()
            .map_err(|_| "companion.failed")?
            .get_mut(&label)
        {
            session.profile.model_dir = directory.clone();
            session.profile.voice = selection.clone();
        }
        Ok(selection)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn begin(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(Profile, Vec<Turn>, Arc<AtomicBool>), String> {
    let sessions = app.state::<Sessions>();
    let mut entries = sessions.entries.lock().map_err(|_| "companion.failed")?;
    let session = entries.get_mut(label).ok_or("companion.failed")?;
    if session.active.is_some() {
        return Err("companion.busy".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    session.active = Some(cancel.clone());
    Ok((session.profile.clone(), session.history.clone(), cancel))
}

fn finish(
    app: &tauri::AppHandle,
    label: &str,
    cancel: &Arc<AtomicBool>,
    turn: Option<Turn>,
) -> Result<(), String> {
    let sessions = app.state::<Sessions>();
    let mut entries = sessions.entries.lock().map_err(|_| "companion.failed")?;
    let session = entries.get_mut(label).ok_or("voice.cancelled")?;
    if cancel.load(Ordering::Relaxed)
        || !session
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, cancel))
    {
        return Err("voice.cancelled".into());
    }
    session.active = None;
    if let Some(turn) = turn {
        session.history.push(turn);
        if session.history.len() > 6 {
            session.history.remove(0);
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn companion_chat(
    app: tauri::AppHandle,
    window: WebviewWindow,
    text: String,
) -> Result<Dialogue, String> {
    let label = window.label().to_owned();
    let (profile, history, cancel) = begin(&app, &label)?;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (service, proxy, path) = {
            let state = handle.state::<AppState>();
            let library = state.library.lock().map_err(|_| "companion.failed")?;
            let library = library.as_ref().map_err(Clone::clone)?;
            (
                Service::parse(&library.settings.ai_provider).ok_or("ai.invalid_provider")?,
                library.settings.proxy.clone(),
                state.credentials_path.clone(),
            )
        };
        let credentials = credentials::load(&path, service).map_err(|e| {
            e.render(galfriends_i18n::Language::from_code(&profile.language).unwrap_or_default())
        })?;
        chat::reply(service, &credentials, &proxy, &profile, &history, &text)
            .map(|reply| Turn { user: text, reply })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    finish(&app, &label, &cancel, result.as_ref().ok().cloned())?;
    result.map(|turn| turn.reply)
}

#[tauri::command]
pub(crate) async fn companion_speak(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<String, String> {
    let label = window.label().to_owned();
    let (profile, history, cancel) = begin(&app, &label)?;
    let handle = app.clone();
    let worker_cancel = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // 语音文本只来自本窗口最后一条已验证的日语回复，前端不能传入中文替代。
        let reply = &history.last().ok_or("companion.no_reply")?.reply;
        speech::synthesize(&handle, &profile, reply, worker_cancel)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    finish(&app, &label, &cancel, None)?;
    result
}

#[tauri::command]
pub(crate) fn companion_cancel(app: tauri::AppHandle, window: WebviewWindow) -> Result<(), String> {
    if let Some(session) = app
        .state::<Sessions>()
        .entries
        .lock()
        .map_err(|_| "companion.failed")?
        .get_mut(window.label())
    {
        session.cancel();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Session;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[test]
    fn closing_a_session_cancels_only_its_task() {
        let first = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));
        let session = Session {
            profile: Default::default(),
            focused: std::time::Instant::now(),
            history: vec![],
            active: Some(first.clone()),
        };
        drop(session);
        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
    }
}
