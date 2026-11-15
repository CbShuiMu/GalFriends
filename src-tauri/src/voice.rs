//! 语音命令只负责角色身份、任务互斥和传输；长任务不持有角色库锁。
use crate::commands::AppState;
use galfriends_core::voice::{BaseModel, Input, Progress, Reply, base_models, catalog, process};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, ipc::Channel};
mod agent;

#[derive(Default)]
pub(crate) struct Tasks(Arc<Mutex<Option<ActiveTask>>>);
struct ActiveTask {
    owner: String,
    request_id: u32,
    cancel: Arc<AtomicBool>,
}
impl Tasks {
    pub(crate) fn acquire(
        &self,
        owner: &str,
        request_id: u32,
        cancel: Arc<AtomicBool>,
    ) -> Result<Lease, String> {
        let mut active = self.0.lock().map_err(|_| "voice.failed")?;
        if active.is_some() {
            return Err("voice.busy".into());
        }
        *active = Some(ActiveTask {
            owner: owner.into(),
            request_id,
            cancel: cancel.clone(),
        });
        Ok(Lease {
            tasks: self.0.clone(),
            cancel,
        })
    }
    fn cancel(&self, request_id: u32) -> Result<(), String> {
        if let Some(task) = self.0.lock().map_err(|_| "voice.failed")?.as_ref()
            && task.owner == "training"
            && task.request_id == request_id
        {
            task.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}
impl Drop for Tasks {
    fn drop(&mut self) {
        if let Ok(active) = self.0.lock()
            && let Some(task) = active.as_ref()
        {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// 所有 GPU 工作共用同一租约；错误和提前返回也会释放互斥。
pub(crate) struct Lease {
    tasks: Arc<Mutex<Option<ActiveTask>>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.tasks.lock()
            && active
                .as_ref()
                .is_some_and(|task| Arc::ptr_eq(&task.cancel, &self.cancel))
        {
            *active = None;
        }
    }
}

pub(crate) fn runtime(engine: galfriends_core::voice::Engine) -> PathBuf {
    let (variable, file) = match engine {
        galfriends_core::voice::Engine::IndexTts25 => ("GALFRIENDS_VOICE_RUNTIME", "runtime.json"),
        galfriends_core::voice::Engine::GptSovitsV5Turbo => {
            ("GALFRIENDS_GPT_SOVITS_RUNTIME", "gpt-sovits-runtime.json")
        }
        galfriends_core::voice::Engine::GptSovitsV2Pro => (
            "GALFRIENDS_GPT_SOVITS_V2PRO_RUNTIME",
            "gpt-sovits-v2pro-runtime.json",
        ),
    };
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../tools/voice")
                .join(file)
        })
}

pub(crate) fn catalog_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.state::<AppState>()
        .path
        .parent()
        .map(|root| root.join("voice-output-directories.json"))
        .ok_or("voice.failed".into())
}

#[tauri::command]
pub(crate) async fn voice_download(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    request_id: u32,
    model: BaseModel,
    on_progress: Channel<Progress>,
) -> Result<Reply, String> {
    if window.label() != "main" {
        return Err("voice.invalid_action".into());
    }
    let proxy = {
        let state = app.state::<AppState>();
        let library = state.library.lock().map_err(|_| "voice.failed")?;
        library
            .as_ref()
            .map_err(Clone::clone)?
            .settings
            .proxy
            .clone()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let lease = app
        .state::<Tasks>()
        .acquire("training", request_id, cancel.clone())?;
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("voice.failed")?
        .to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        base_models::install(
            &workspace.join("tools/voice"),
            model,
            &proxy,
            &cancel,
            |p| {
                let _ = on_progress.send(p);
            },
        )?;
        // 权重下载可以在全新环境中使用。未配置运行环境时保持训练禁用。
        let ready = model.engine().is_some_and(|engine| {
            process::run::<_, Reply>(
                &runtime(engine),
                &workspace.join("crates/core/src/voice/python/worker.py"),
                "status",
                &Input {
                    engine,
                    ..Default::default()
                },
                &cancel,
                |_| {},
            )
            .is_ok_and(|reply| reply.ready)
        });
        if cancel.load(Ordering::Relaxed) {
            return Err("voice.cancelled".into());
        }
        Ok(Reply {
            ready,
            ..Default::default()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn voice_run(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    request_id: u32,
    action: String,
    mut input: Input,
    on_progress: Channel<Progress>,
) -> Result<Reply, String> {
    if window.label() != "main"
        || !matches!(
            action.as_str(),
            "status" | "extract" | "skill_extract" | "inspect" | "train" | "register"
        )
    {
        return Err("voice.invalid_action".into());
    }
    if action == "train" {
        let state = app.state::<AppState>();
        let library = state.library.lock().map_err(|_| "voice.failed")?;
        let library = library.as_ref().map_err(Clone::clone)?;
        if !library
            .characters
            .iter()
            .any(|c| c.id == input.character_id)
        {
            return Err("voice.character_required".into());
        }
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let lease = app
        .state::<Tasks>()
        .acquire("training", request_id, cancel.clone())?;
    // 解包工具与引擎无关，继续复用同一个归档适配环境。
    let engine = if matches!(action.as_str(), "extract" | "skill_extract" | "inspect") {
        Default::default()
    } else {
        input.engine
    };
    let config = runtime(engine);
    let worker =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/core/src/voice/python/worker.py");
    let catalog_file = catalog_file(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        if matches!(action.as_str(), "extract" | "skill_extract") && !input.game_id.is_empty() {
            input.game_dir = crate::game_library::resolve_training_game(&app, &input)?;
        }
        let output = std::path::Path::new(&input.output_dir);
        if action == "register" {
            catalog::register(&catalog_file, output)?;
            return Ok(Reply::default());
        }
        let mut payload = serde_json::to_value(&input).map_err(|e| e.to_string())?;
        if action == "skill_extract" {
            payload["skill_dir"] = serde_json::json!(skill_directory(&input.catalog_path)?);
            agent::configure(&app, &input.character_id, &mut payload)?;
        }
        let reply = process::run(&config, &worker, &action, &payload, &cancel, |p| {
            let _ = on_progress.send(p);
        })?;
        if action == "train" {
            catalog::register(&catalog_file, output)?;
        }
        Ok(reply)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r)
}

fn skill_directory(catalog: &str) -> Result<PathBuf, String> {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("GALFRIENDS_GALGEAR_SKILL") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(parent) = std::path::Path::new(catalog).parent() {
        candidates.push(parent.join("Galgear-GalgameReverse-Skill/GalgameResourceExtract"));
        if let Some(parent) = parent.parent() {
            candidates.push(parent.join("Galgear-GalgameReverse-Skill/GalgameResourceExtract"));
        }
    }
    candidates.push(dev.join("Galgear-GalgameReverse-Skill/GalgameResourceExtract"));
    candidates.push(dev.join("../GalGear/Galgear-GalgameReverse-Skill/GalgameResourceExtract"));
    candidates
        .into_iter()
        .find(|path| path.join("SKILL.md").is_file())
        .ok_or_else(|| "voice.skill_missing".to_owned())?
        .canonicalize()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn voice_cancel(app: tauri::AppHandle, request_id: u32) -> Result<(), String> {
    app.state::<Tasks>().cancel(request_id)
}

#[cfg(test)]
mod tests {
    use super::{ActiveTask, Tasks};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn late_cancel_cannot_stop_a_new_task() {
        let cancel = Arc::new(AtomicBool::new(false));
        let tasks = Tasks(Arc::new(Mutex::new(Some(ActiveTask {
            owner: "training".into(),
            request_id: 8,
            cancel: cancel.clone(),
        }))));
        tasks.cancel(7).unwrap();
        assert!(!cancel.load(Ordering::Relaxed));
        tasks.cancel(8).unwrap();
        assert!(cancel.load(Ordering::Relaxed));
    }
}
