//! 主窗口的情绪参考工作台；网络筛选与试听在后台运行，提交使用角色库短事务。
use super::{Sessions, binding_path};
use crate::commands::AppState;
use galfriends_core::{
    companion::{
        Profile,
        emotions::{Configuration, Emotion, PageData, Target, storage},
        storage as model_storage,
    },
    connections::{Service, credentials, emotion_recommendations},
    store,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, WebviewWindow, ipc::Channel};

#[derive(Default)]
pub(crate) struct Tasks(Mutex<Option<(u32, Arc<AtomicBool>)>>);

struct TaskGuard(tauri::AppHandle);
impl Drop for TaskGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.state::<Tasks>().0.lock() {
            *active = None;
        }
    }
}

fn main_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("companion.failed".into());
    }
    Ok(())
}

fn target(app: &tauri::AppHandle, id: &str) -> Result<Profile, String> {
    let sessions = app.state::<Sessions>();
    let entries = sessions.entries.lock().map_err(|_| "companion.failed")?;
    let session = entries
        .values()
        .find(|s| s.profile.id == id)
        .ok_or("emotion.no_target")?;
    Ok(session.profile.clone())
}

pub(super) fn config_path(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(binding_path(app, id)?.with_file_name("emotion-references.json"))
}

fn load(app: &tauri::AppHandle, profile: &Profile) -> Result<Configuration, String> {
    storage::load(
        &config_path(app, &profile.id)?,
        &profile.id,
        &profile.model_dir,
    )
}

// 锁顺序为模型绑定、角色库、会话。网络与读取音频不进入提交事务。
fn commit(
    app: &tauri::AppHandle,
    profile: &Profile,
    update: impl FnOnce(&mut Configuration) -> Result<(), String>,
) -> Result<Configuration, String> {
    let sessions = app.state::<Sessions>();
    let _binding = sessions.bindings.lock().map_err(|_| "companion.failed")?;
    let state = app.state::<AppState>();
    let library = state.library.lock().map_err(|_| "companion.failed")?;
    let library = library.as_ref().map_err(Clone::clone)?;
    if !library.characters.iter().any(|c| c.id == profile.id) {
        return Err("emotion.no_target".into());
    }
    store::ensure_writable(&state.path)?;
    target(app, &profile.id)?;
    let mut config = load(app, profile)?;
    update(&mut config)?;
    storage::save(&config_path(app, &profile.id)?, &config)?;
    Ok(config)
}

#[tauri::command]
pub(crate) async fn emotion_page(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: Option<String>,
) -> Result<PageData, String> {
    main_window(&window)?;
    crate::diagnostics::spawn_background("emotion_page", move || {
        let (targets, selected) = {
            let sessions = app.state::<Sessions>();
            let entries = sessions.entries.lock().map_err(|_| "companion.failed")?;
            let mut entries: Vec<_> = entries.values().collect();
            entries.sort_by_key(|s| std::cmp::Reverse(s.focused));
            let selected = id
                .filter(|id| entries.iter().any(|s| &s.profile.id == id))
                .or_else(|| entries.first().map(|s| s.profile.id.clone()));
            let targets = entries
                .iter()
                .map(|s| Target {
                    id: s.profile.id.clone(),
                    name: s.profile.name.clone(),
                })
                .collect();
            (targets, selected)
        };
        let configuration = selected
            .map(|id| load(&app, &target(&app, &id)?))
            .transpose()?;
        Ok(PageData {
            targets,
            configuration,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn emotion_recommend(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    request_id: u32,
    on_progress: Channel<(usize, usize)>,
) -> Result<Configuration, String> {
    main_window(&window)?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let tasks = app.state::<Tasks>();
        let mut active = tasks.0.lock().map_err(|_| "companion.failed")?;
        if active.is_some() {
            return Err("companion.busy".into());
        }
        *active = Some((request_id, cancel.clone()));
    }
    // 排队前登记取消标记，避免后台线程池繁忙时漏掉用户取消。
    let task = TaskGuard(app.clone());
    crate::diagnostics::spawn_background("emotion_recommend", move || {
        let _task = task;
        let profile = target(&app, &id)?;
        let config = load(&app, &profile)?;
        let candidates = storage::candidates(Path::new(&config.source_dir), &id)?;
        let (service, proxy, credentials_path) = {
            let state = app.state::<AppState>();
            let library = state.library.lock().map_err(|_| "companion.failed")?;
            let library = library.as_ref().map_err(Clone::clone)?;
            (
                Service::parse(&library.settings.ai_provider).ok_or("ai.invalid_provider")?,
                library.settings.proxy.clone(),
                state.credentials_path.clone(),
            )
        };
        let language = galfriends_i18n::Language::from_code(&profile.language).unwrap_or_default();
        let credentials =
            credentials::load(&credentials_path, service).map_err(|e| e.render(language))?;
        let recommendations = emotion_recommendations::recommend(
            service,
            &credentials,
            &proxy,
            &profile.language,
            &candidates,
            &cancel,
            |batch, total| {
                let _ = on_progress.send((batch, total));
            },
        )?;
        commit(&app, &profile, |config| {
            if cancel.load(Ordering::Relaxed) {
                return Err("voice.cancelled".into());
            }
            config.recommendations = recommendations;
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn emotion_cancel(
    app: tauri::AppHandle,
    window: WebviewWindow,
    request_id: u32,
) -> Result<(), String> {
    main_window(&window)?;
    if let Some((id, cancel)) = app
        .state::<Tasks>()
        .0
        .lock()
        .map_err(|_| "companion.failed")?
        .as_ref()
        && *id == request_id
    {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn emotion_choose(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    emotion: Emotion,
    candidate_id: String,
) -> Result<Configuration, String> {
    main_window(&window)?;
    crate::diagnostics::spawn_background("emotion_choose", move || {
        let profile = target(&app, &id)?;
        let config = load(&app, &profile)?;
        let candidate = config
            .recommendations
            .get(&emotion)
            .and_then(|list| list.iter().find(|c| c.id == candidate_id))
            .ok_or("emotion.invalid_data")?;
        let selected = storage::reference(Path::new(&config.source_dir), candidate)?;
        commit(&app, &profile, |current| {
            if !current
                .recommendations
                .get(&emotion)
                .is_some_and(|list| list.contains(candidate))
            {
                return Err("emotion.changed".into());
            }
            current.selected.insert(emotion, selected);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn emotion_preview(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    candidate_id: String,
) -> Result<String, String> {
    main_window(&window)?;
    crate::diagnostics::spawn_background("emotion_preview", move || {
        let profile = target(&app, &id)?;
        let config = load(&app, &profile)?;
        let candidate = config
            .recommendations
            .values()
            .flatten()
            .chain(config.selected.values().map(|r| &r.candidate))
            .find(|c| c.id == candidate_id)
            .ok_or("emotion.invalid_data")?;
        storage::reference(Path::new(&config.source_dir), candidate)?;
        model_storage::audio_data(&storage::audio_path(
            Path::new(&config.source_dir),
            &candidate_id,
        )?)
    })
    .await
    .map_err(|e| e.to_string())?
}
