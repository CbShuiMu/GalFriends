//! 桌面更新命令在锁外下载与安装。桌面进程只使用自己核验过的资产。
use crate::commands::AppState;
use galfriends_core::{
    characters::Settings,
    updates::{self, CheckedRelease, Progress, Release, SkillInstall, SkillStatus},
};
use galfriends_i18n::Message;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, State, WebviewWindow, ipc::Channel};

#[cfg(all(windows, target_arch = "x86_64"))]
mod handoff;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) use handoff::run_from_args;

#[derive(Default)]
pub(crate) struct Updates {
    checked: Mutex<Option<(String, CheckedRelease)>>,
    active: Arc<Mutex<Option<ActiveJob>>>,
}

struct ActiveJob {
    id: u32,
    cancel: Arc<AtomicBool>,
}

struct Lease {
    active: Arc<Mutex<Option<ActiveJob>>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
    }
}

fn busy() -> Message {
    Message::new("update.busy")
}

impl Updates {
    fn begin(&self, id: u32) -> Result<Lease, Message> {
        let mut active = self.active.lock().map_err(|_| busy())?;
        if active.is_some() {
            return Err(busy());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveJob {
            id,
            cancel: cancel.clone(),
        });
        Ok(Lease {
            active: self.active.clone(),
            cancel,
        })
    }
}

fn main_window(window: &WebviewWindow) -> Result<(), Message> {
    if window.label() != "main" {
        return Err(Message::detail("error.operation", "Main window required"));
    }
    Ok(())
}

pub(crate) fn skill_home() -> Result<PathBuf, Message> {
    let exe =
        std::env::current_exe().map_err(|e| Message::detail("update.skill_install_failed", e))?;
    let parent = exe
        .parent()
        .ok_or_else(|| Message::new("update.unavailable"))?;
    Ok(updates::skill::home(parent))
}

fn settings(state: &AppState) -> Result<Settings, Message> {
    let error = |detail| Message::detail("error.operation", detail);
    let guard = state.library.lock().map_err(|e| error(e.to_string()))?;
    Ok(guard
        .as_ref()
        .map_err(|e| error(e.clone()))?
        .settings
        .clone())
}

#[tauri::command]
pub(crate) async fn check_app_update(
    window: WebviewWindow,
    state: State<'_, AppState>,
    updater: State<'_, Updates>,
) -> Result<Option<Release>, Message> {
    main_window(&window)?;
    let settings = settings(&state)?;
    let page = settings.release_page;
    let source = page.clone();
    let result = crate::diagnostics::spawn_background("check_app_update", move || {
        updates::check_release(&source, &settings.proxy)
    })
    .await
    .map_err(|e| Message::detail("update.check_failed", e))??;
    let mut checked = updater.checked.lock().map_err(|_| busy())?;
    if self::settings(&state)?.release_page != page {
        return Ok(None);
    }
    let summary = result.as_ref().map(|r| r.summary.clone());
    *checked = result.map(|release| (page, release));
    Ok(summary)
}

#[tauri::command]
pub(crate) async fn check_skill_update(state: State<'_, AppState>) -> Result<SkillStatus, Message> {
    let settings = settings(&state)?;
    crate::diagnostics::spawn_background("check_skill_update", move || {
        let directory = match crate::voice::skill_directory(&settings.galgear_library_path) {
            Ok(path) => Some(path),
            Err(error) if error == "voice.skill_missing" => None,
            Err(error) => return Err(Message::detail("update.skill_failed", error)),
        };
        let mut status = updates::skill_status(directory.as_deref());
        if let Err(error) = updates::check_skill(&mut status, &settings.proxy) {
            // 上游离线时仍可展示本地标记，远端状态留空。
            log::warn!("skill_version_check_failed error={error}");
        }
        Ok(status)
    })
    .await
    .map_err(|e| Message::detail("update.skill_failed", e))?
}

#[tauri::command]
pub(crate) async fn install_app_update(
    window: WebviewWindow,
    state: State<'_, AppState>,
    updater: State<'_, Updates>,
    tag: String,
    request_id: u32,
    on_progress: Channel<Progress>,
) -> Result<(), Message> {
    main_window(&window)?;
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (state, updater, tag, request_id, on_progress);
        Err(Message::new("update.unavailable"))
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let lease = updater.begin(request_id)?;
        let settings = settings(&state)?;
        let asset = updater
            .checked
            .lock()
            .map_err(|_| busy())?
            .as_ref()
            .filter(|(page, release)| {
                page == &settings.release_page
                    && release.summary.available
                    && release.summary.installable
                    && release.summary.tag == tag
            })
            .and_then(|(_, release)| release.asset.clone())
            .ok_or_else(|| Message::new("update.unavailable"))?;
        let handle = window.app_handle().clone();
        crate::diagnostics::spawn_background("install_app_update", move || {
            let language =
                galfriends_i18n::Language::from_code(&settings.language).unwrap_or_default();
            let stage = handoff::prepare(
                asset,
                &settings.proxy,
                language,
                &lease.cancel,
                |progress| {
                    let _ = on_progress.send(progress);
                },
            )?;
            // 避免退出时中断正在写入训练或合成产物的语音任务。
            let _voice = handle
                .state::<crate::voice::Tasks>()
                .acquire("update", request_id, Arc::new(AtomicBool::new(false)), None)
                .map_err(|_| Message::new("update.voice_busy"))?;
            if lease.cancel.load(Ordering::Relaxed) {
                return Err(Message::new("update.cancelled"));
            }
            let _ = on_progress.send(Progress::Restarting);
            handoff::launch(stage)?;
            handle.exit(0);
            Ok(())
        })
        .await
        .map_err(|e| Message::detail("update.install_failed", e))?
    }
}

#[tauri::command]
pub(crate) async fn install_skill_update(
    window: WebviewWindow,
    state: State<'_, AppState>,
    updater: State<'_, Updates>,
    request_id: u32,
    on_progress: Channel<Progress>,
) -> Result<SkillInstall, Message> {
    main_window(&window)?;
    let lease = updater.begin(request_id)?;
    let proxy = settings(&state)?.proxy;
    let home = skill_home()?;
    crate::diagnostics::spawn_background("install_skill_update", move || {
        updates::skill::install(&home, &proxy, &lease.cancel, |progress| {
            let _ = on_progress.send(progress);
        })
    })
    .await
    .map_err(|e| Message::detail("update.skill_install_failed", e))?
}

#[tauri::command]
pub(crate) fn cancel_update(
    window: WebviewWindow,
    updater: State<'_, Updates>,
    request_id: u32,
) -> Result<(), Message> {
    main_window(&window)?;
    if let Some(job) = updater
        .active
        .lock()
        .map_err(|_| busy())?
        .as_ref()
        .filter(|job| job.id == request_id)
    {
        job.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}
