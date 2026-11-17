//! 语音命令只负责角色身份、任务互斥和传输；长任务不持有角色库锁。
use crate::commands::AppState;
use galfriends_core::{
    archive, store,
    voice::{BaseModel, Input, Progress, Reply, Update, base_models, catalog, process},
};
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
    character: Option<String>,
}
impl Tasks {
    pub(crate) fn acquire(
        &self,
        owner: &str,
        request_id: u32,
        cancel: Arc<AtomicBool>,
        character: Option<String>,
    ) -> Result<Lease, String> {
        let mut active = self.0.lock().map_err(|_| "voice.failed")?;
        if active.is_some() {
            return Err("voice.busy".into());
        }
        *active = Some(ActiveTask {
            owner: owner.into(),
            request_id,
            cancel: cancel.clone(),
            character,
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

    pub(crate) fn ensure_can_remove(&self, ids: &[String]) -> Result<(), String> {
        let active = self.0.lock().map_err(|_| "voice.failed")?;
        if active
            .as_ref()
            .and_then(|task| task.character.as_ref())
            .is_some_and(|id| ids.contains(id))
        {
            return Err("voice.character_in_use".into());
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
        .acquire("training", request_id, cancel.clone(), None)?;
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("voice.failed")?
        .to_owned();
    crate::diagnostics::spawn_background("voice_download", move || {
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
    on_progress: Channel<Update>,
) -> Result<Reply, String> {
    if window.label() != "main"
        || !matches!(
            action.as_str(),
            "status" | "extract" | "skill_extract" | "inspect" | "train" | "register"
        )
    {
        return Err("voice.invalid_action".into());
    }
    let extracting = matches!(action.as_str(), "extract" | "skill_extract");
    let writes_character_data = extracting || action == "train";
    let cancel = Arc::new(AtomicBool::new(false));
    let lease = app.state::<Tasks>().acquire(
        "training",
        request_id,
        cancel.clone(),
        writes_character_data.then(|| input.character_id.clone()),
    )?;
    if action == "register" {
        let folder = rfd::AsyncFileDialog::new()
            .set_parent(&window)
            .pick_folder()
            .await
            .ok_or("voice.cancelled")?;
        let file = catalog_file(&app)?;
        return crate::diagnostics::spawn_background("voice_register", move || {
            let _lease = lease;
            if cancel.load(Ordering::Relaxed) {
                return Err("voice.cancelled".into());
            }
            catalog::register(&file, folder.path())?;
            Ok(Reply::default())
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    // 解包工具与引擎无关，继续复用同一个归档适配环境。
    let engine = if matches!(action.as_str(), "extract" | "skill_extract" | "inspect") {
        Default::default()
    } else {
        input.engine
    };
    let config = runtime(engine);
    let worker =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/core/src/voice/python/worker.py");
    crate::diagnostics::spawn_background("voice_run", move || {
        let _lease = lease;
        if extracting {
            if input.game_id.is_empty() {
                return Err("game_library.changed".into());
            }
            input.game_dir = crate::game_library::resolve_training_game(&app, &input)?;
        }
        let output = writes_character_data
            .then(|| character_output(&app, &input.character_id))
            .transpose()?;
        let mut payload = serde_json::to_value(&input).map_err(|e| e.to_string())?;
        if let Some(output) = output {
            payload["output_dir"] = serde_json::json!(output);
        }
        let _skill_lock = if action == "skill_extract" {
            let home = crate::updates::skill_home().map_err(|e| e.key)?;
            let lock = galfriends_core::updates::skill::read_lock(&home).map_err(|e| e.key)?;
            let path = if lock.is_some() {
                skill_directory(&input.catalog_path)?
            } else {
                legacy_skill_directory(&input.catalog_path)?
            };
            payload["skill_dir"] = serde_json::json!(path);
            agent::configure(&app, &input.character_id, &mut payload)?;
            lock
        } else {
            None
        };
        process::run_with_updates(&config, &worker, &action, &payload, &cancel, |update| {
            let _ = on_progress.send(update);
        })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r)
}

fn character_output(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    let state = app.state::<AppState>();
    let guard = state.library.lock().map_err(|_| "voice.failed")?;
    let library = guard.as_ref().map_err(Clone::clone)?;
    store::ensure_writable(&state.path)?;
    let character = library
        .characters
        .iter()
        .find(|character| character.id == id)
        .ok_or("voice.character_required")?;
    let root = state.path.parent().ok_or("voice.output_missing")?;
    archive::ensure_character_directory(root, character)?;
    catalog::output_directory(root, id)
}

pub(crate) fn skill_directory(catalog: &str) -> Result<PathBuf, String> {
    let home = crate::updates::skill_home().map_err(|e| e.key)?;
    if let Some(path) =
        galfriends_core::updates::skill::managed_directory(&home).map_err(|e| e.key)?
    {
        return Ok(path);
    }
    legacy_skill_directory(catalog)
}

fn legacy_skill_directory(catalog: &str) -> Result<PathBuf, String> {
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
            character: Some("active".into()),
        }))));
        assert!(tasks.ensure_can_remove(&["active".into()]).is_err());
        assert!(tasks.ensure_can_remove(&["another".into()]).is_ok());
        tasks.cancel(7).unwrap();
        assert!(!cancel.load(Ordering::Relaxed));
        tasks.cancel(8).unwrap();
        assert!(cancel.load(Ordering::Relaxed));
    }
}
