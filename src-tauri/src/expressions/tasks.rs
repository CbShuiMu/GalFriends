//! 应用级表情扫描调度；弹窗关闭不停止任务，退出取消进程后保留磁盘断点。
use crate::commands::AppState;
use galfriends_core::{
    artwork::expressions::discovery, characters::ExpressionScan, game_library, store,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub(crate) struct Tasks {
    active: Mutex<HashMap<String, Job>>,
    statuses: Mutex<HashMap<String, ExpressionScan>>,
    serial: Mutex<()>,
    next: AtomicU32,
    stopping: AtomicBool,
}
struct Job {
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Tasks {
    pub(crate) fn ensure_can_remove(&self, ids: &[String]) -> Result<(), String> {
        let active = self.active.lock().map_err(|e| e.to_string())?;
        if ids.iter().any(|id| active.contains_key(id)) {
            return Err("expression.scan_remove".into());
        }
        Ok(())
    }

    fn publish(&self, app: &tauri::AppHandle, status: ExpressionScan) {
        if let Ok(mut statuses) = self.statuses.lock() {
            if statuses
                .get(&status.character_id)
                .is_some_and(|previous| previous.task > status.task)
            {
                return;
            }
            statuses.insert(status.character_id.clone(), status.clone());
        }
        if let Err(error) = app.emit_to("main", "expression-scan-progress", &status) {
            log::warn!("expression_scan_event_failed error={error}");
        }
    }
}

/// 与删除共用库事务获取任务所有权；后续耗时读取、匹配、解包均在锁外。
pub(crate) fn start(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let guard = state.library.lock().map_err(|e| e.to_string())?;
    let library = guard.as_ref().map_err(Clone::clone)?;
    let character = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("expression.changed")?
        .clone();
    let catalog_path = library.settings.galgear_library_path.clone();
    let tasks = app.state::<Tasks>();
    let mut active = tasks.active.lock().map_err(|e| e.to_string())?;
    if tasks.stopping.load(Ordering::Relaxed) || active.contains_key(id) {
        return Ok(());
    }
    store::ensure_writable(&state.path)?;
    discovery::enable(
        state.path.parent().ok_or("Missing character directory")?,
        id,
    )?;
    let path = state.path.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    let task = tasks.next.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let status = ExpressionScan {
        character_id: id.into(),
        task,
        stage: "queued".into(),
        ..Default::default()
    };
    tasks.publish(app, status.clone());
    active.insert(
        id.into(),
        Job {
            cancel,
            thread: None,
        },
    );
    let app = app.clone();
    let id = id.to_owned();
    let key = id.clone();
    let handle = std::thread::Builder::new()
        .name("expression-scan".into())
        .spawn(move || {
            let tasks = app.state::<Tasks>();
            let result = (|| {
                // 单应用只同时运行一个扫描，避免多个角色争抢 CPU/磁盘。
                let _serial = tasks.serial.lock().map_err(|e| e.to_string())?;
                if cancelled.load(Ordering::Relaxed) {
                    return Ok("paused".to_owned());
                }
                let catalog = if catalog_path.trim().is_empty() {
                    game_library::Catalog::default()
                } else {
                    game_library::load(Path::new(&catalog_path))?
                };
                let folders = discovery::folders(&character, &catalog)?;
                let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
                discovery::run(
                    &project,
                    &path,
                    &character,
                    &folders,
                    &cancelled,
                    |mut progress| {
                        progress.task = task;
                        tasks.publish(&app, progress);
                    },
                )
            })();
            let mut status = status;
            status.stage = "finished".into();
            match result {
                Ok(result) => status.result = result,
                Err(error) => {
                    log::warn!("expression_scan_failed character={id} error={error}");
                    status.result = "failed".into();
                    status.error = error;
                }
            }
            if let Ok(mut active) = tasks.active.lock() {
                active.remove(&id);
            }
            tasks.publish(&app, status);
        })
        .map_err(|e| e.to_string());
    match handle {
        Ok(handle) => {
            if let Some(job) = active.get_mut(&key) {
                job.thread = Some(handle);
            }
        }
        Err(error) => {
            active.remove(&key);
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn resume(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(library) = crate::commands::read(&state) {
        for character in library.characters {
            let enabled = state
                .path
                .parent()
                .ok_or("Missing character directory".to_owned())
                .and_then(|root| discovery::pending(root, &character.id));
            let enabled = match enabled {
                Ok(enabled) => enabled,
                Err(error) => {
                    report_error(app, &character.id, error);
                    false
                }
            };
            if enabled && let Err(error) = start(app, &character.id) {
                report_error(app, &character.id, error);
            }
        }
    }
}

pub(crate) fn report_error(app: &tauri::AppHandle, id: &str, error: String) {
    log::warn!("expression_scan_failed character={id} error={error}");
    let tasks = app.state::<Tasks>();
    let task = tasks.next.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    tasks.publish(
        app,
        ExpressionScan {
            character_id: id.into(),
            task,
            stage: "finished".into(),
            result: "failed".into(),
            error,
            ..Default::default()
        },
    );
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    let tasks = app.state::<Tasks>();
    tasks.stopping.store(true, Ordering::Relaxed);
    let jobs: Vec<_> = tasks
        .active
        .lock()
        .map(|mut active| active.drain().map(|(_, job)| job).collect())
        .unwrap_or_default();
    for job in &jobs {
        job.cancel.store(true, Ordering::Relaxed);
    }
    for job in jobs {
        if let Some(thread) = job.thread {
            let _ = thread.join();
        }
    }
}

#[tauri::command]
pub(crate) fn expression_scan_status(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Vec<ExpressionScan>, String> {
    if window.label() != "main" {
        return Err("Main window required".into());
    }
    Ok(app
        .state::<Tasks>()
        .statuses
        .lock()
        .map_err(|e| e.to_string())?
        .values()
        .cloned()
        .collect())
}

#[tauri::command]
pub(crate) fn expression_scan_pause(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    id: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Main window required".into());
    }
    if let Some(job) = app
        .state::<Tasks>()
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .get(&id)
    {
        job.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}
