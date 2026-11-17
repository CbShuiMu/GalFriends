//! 每次进程启动独立记录诊断信息；禁止记录命令正文、成功响应和凭据。
//! 日志直接写入文件而不使用用户态缓冲，错误额外刷盘；Windows 异常使用独立句柄。

mod privacy;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod webview;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use webview::observe as observe_webview;

use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record};
use serde::Deserialize;
use serde_json::json;
use std::{
    backtrace::Backtrace,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub(crate) const FRONTEND_SCRIPT: &str = include_str!("diagnostics/frontend.js");
static LOGGER: OnceLock<SessionLog> = OnceLock::new();

struct SessionLog {
    file: Mutex<File>,
    path: PathBuf,
    started: Instant,
    privacy: privacy::Redactor,
    failed: AtomicBool,
    webview_process_failed: AtomicBool,
}

impl SessionLog {
    fn create(directory: &Path, timestamp: &str) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        // create_new 同时处理多实例和系统时间回拨，绝不截断已有日志。
        for suffix in 0..1000 {
            let suffix = if suffix == 0 {
                String::new()
            } else {
                format!("_{suffix}")
            };
            let path = directory.join(format!("galfriends_{timestamp}{suffix}.log"));
            match OpenOptions::new().append(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        file: Mutex::new(file),
                        path,
                        started: Instant::now(),
                        privacy: privacy::Redactor::new()?,
                        failed: AtomicBool::new(false),
                        webview_process_failed: AtomicBool::new(false),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Log filename collisions",
        ))
    }

    fn write(&self, level: Level, target: &str, message: &str) -> io::Result<()> {
        let thread = std::thread::current();
        let entry = json!({
            "time": Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
            "elapsed_ms": self.started.elapsed().as_millis(),
            "level": level.as_str(), "pid": std::process::id(),
            "thread": format!("{:?}", thread.id()), "thread_name": thread.name(),
            "target": target, "message": self.privacy.clean(message),
        });
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        file.write_all(&line)?;
        if level == Level::Error {
            file.sync_data()?;
        }
        Ok(())
    }
}

impl Log for SessionLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // HTTP/SDK 的第三方 debug 日志可能含请求头，只接受本项目主动写入的记录。
        metadata.level() <= Level::Info && metadata.target().starts_with("galfriends")
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata())
            && let Err(error) =
                self.write(record.level(), record.target(), &record.args().to_string())
            && !self.failed.swap(true, Ordering::Relaxed)
        {
            eprintln!("GalFriends log write failed: {error}");
            // 避免在故障/持锁线程显示模态框；只通知一次，业务继续运行。
            let _ = std::thread::Builder::new()
                .name("log-warning".into())
                .spawn(move || {
                    show_failure("diagnostics.write_failed", &error.to_string());
                });
        }
    }

    fn flush(&self) {
        if let Ok(file) = self.file.lock()
            && let Err(error) = file.sync_data()
        {
            eprintln!("GalFriends log flush failed: {error}");
        }
    }
}

fn initialize_at(directory: &Path) -> io::Result<()> {
    let logger = SessionLog::create(
        directory,
        &Local::now().format("%Y-%m-%d_%H%M%S_%3f").to_string(),
    )?;
    LOGGER
        .set(logger)
        .map_err(|_| io::Error::other("Logger already initialized"))?;
    let logger = LOGGER
        .get()
        .ok_or_else(|| io::Error::other("Logger unavailable"))?;
    log::set_logger(logger).map_err(|error| io::Error::other(error.to_string()))?;
    log::set_max_level(LevelFilter::Info);
    std::panic::set_hook(Box::new(|panic| {
        let payload = panic
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        log::error!(target: "galfriends::panic", "panic at {}: {payload}\n{}",
            panic.location().map(|value| value.to_string()).unwrap_or_default(), Backtrace::force_capture());
    }));
    log::info!(target: "galfriends::session", "session_start version={} os={} arch={} debug={} exe={} cwd={}",
        env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, cfg!(debug_assertions),
        std::env::current_exe()?.display(), std::env::current_dir()?.display());
    #[cfg(windows)]
    windows::install(&logger.path)?;
    Ok(())
}

/// 在桌面运行时启动前创建安装目录 logs。失败时明确通知，不静默改写到其他目录。
pub(crate) fn initialize() {
    let result = std::env::current_exe().and_then(|exe| {
        let directory = exe
            .parent()
            .ok_or_else(|| io::Error::other("Executable directory"))?;
        initialize_at(&directory.join("logs"))
    });
    if let Err(error) = result {
        eprintln!("GalFriends log initialization failed: {error}");
        show_failure("diagnostics.init_failed", &error.to_string());
    }
}

fn show_failure(key: &str, detail: &str) {
    native_notice(&galfriends_i18n::Message::detail(key, detail).render(Default::default()));
}

/// 窗口尚未创建或已失效时使用原生提示，不依赖 JavaScript。
fn native_notice(message: &str) {
    #[cfg(windows)]
    windows::notice(message);
    #[cfg(not(windows))]
    eprintln!("GalFriends: {message}");
}

/// 启动失败先记录并刷盘，再显示可见错误；不伪造正常退出记录。
pub(crate) fn startup_failed(error: &tauri::Error) {
    log::error!("startup_failed error={error}");
    log::logger().flush();
    show_failure("diagnostics.startup_failed", &error.to_string());
}

/// 每个窗口只注册一次；进程退出提示在后台线程显示，不中断写入事务。
pub(crate) fn watch_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use tauri::Manager;
        let app = window.app_handle().clone();
        observe_webview(window, move || {
            let app = app.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("webview-warning".into())
                .spawn(move || {
                    let language = app
                        .try_state::<crate::commands::AppState>()
                        .and_then(|state| {
                            state.library.lock().ok().and_then(|library| {
                                library.as_ref().ok().and_then(|library| {
                                    galfriends_i18n::Language::from_code(&library.settings.language)
                                })
                            })
                        })
                        .unwrap_or_default();
                    native_notice(
                        &galfriends_i18n::Message::new("diagnostics.webview_exited")
                            .render(language),
                    );
                })
            {
                log::error!("webview_warning_failed error={error}");
            }
        });
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// 在后台线程记录操作结果，即使页面关闭也保留失败线索；不接触成功结果正文。
pub(crate) fn spawn_background<T, E>(
    operation: &'static str,
    work: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> tauri::async_runtime::JoinHandle<Result<T, E>>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let started = Instant::now();
        log::info!(target: "galfriends::background", "background_start operation={operation}");
        let result = work();
        let elapsed = started.elapsed().as_millis();
        match &result {
            Ok(_) => {
                log::info!(target: "galfriends::background", "background_ok operation={operation} elapsed_ms={elapsed}")
            }
            Err(error) => {
                log::error!(target: "galfriends::background", "background_error operation={operation} elapsed_ms={elapsed} error={error}")
            }
        }
        result
    })
}

/// 记录已观察到的 WebView2 进程故障；ERROR 写入完成前执行 sync_data。
#[cfg(windows)]
fn record_webview_failure(message: std::fmt::Arguments<'_>) {
    if let Some(logger) = LOGGER.get() {
        logger.webview_process_failed.store(true, Ordering::Relaxed);
    }
    log::error!(target: "galfriends::diagnostics::webview", "{message}");
}

/// 仅完整退出路径调用；保留本次会话已观察到的界面进程故障标记。
pub(crate) fn finish() {
    let webview_process_failed = LOGGER
        .get()
        .is_some_and(|logger| logger.webview_process_failed.load(Ordering::Relaxed));
    log::info!(target: "galfriends::session", "session_end webview_process_failed={webview_process_failed}");
    log::logger().flush();
}

/// 在后端收到命令时记录起点，即使 WebView 后续崩溃也保留操作线索。
fn command_started(invoke: &tauri::ipc::Invoke) {
    let command = invoke.message.command();
    if command == "record_diagnostic" {
        return;
    }
    let request = invoke
        .message
        .headers()
        .get("x-galfriends-request")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("untracked");
    log::info!(target: "galfriends::command", "command_start window={} request={} command={}",
        invoke.message.webview_ref().label(), request, command);
}

/// 包装统一命令分发入口，不改变命令参数、结果或原有权限检查。
pub(crate) fn trace_commands(
    handler: fn(tauri::ipc::Invoke) -> bool,
) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        command_started(&invoke);
        handler(invoke)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FrontendKind {
    CommandOk,
    CommandError,
    JavascriptError,
    WasmPanic,
    Ready,
    View,
    Visibility,
}

#[derive(Deserialize)]
pub(crate) struct FrontendEvent {
    kind: FrontendKind,
    #[serde(default)]
    request: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    elapsed_ms: u64,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    visible: bool,
    view: Option<FrontendView>,
}

#[derive(Deserialize)]
struct FrontendView {
    sequence: u32,
    page: FrontendPage,
    overlay: FrontendOverlay,
    phase: ViewPhase,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FrontendPage {
    Boot,
    Home,
    Library,
    Favorites,
    Detail,
    Voice,
    Emotions,
    Companion,
    Settings,
    About,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FrontendOverlay {
    None,
    Settings,
    Dialog,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewPhase {
    Applied,
    Frames,
    FramesDelayed,
}

/// 记录前端错误、命令结果与界面进度，日志写入时统一脱敏和限长。
#[tauri::command]
pub(crate) fn record_diagnostic(webview: tauri::Webview, event: FrontendEvent) {
    match event.kind {
        FrontendKind::CommandOk => {
            log::info!(target: "galfriends::command", "command_ok request={} command={} elapsed_ms={}", event.request, event.command, event.elapsed_ms)
        }
        FrontendKind::CommandError => {
            log::error!(target: "galfriends::command", "command_error request={} command={} elapsed_ms={} error={}", event.request, event.command, event.elapsed_ms, event.detail)
        }
        FrontendKind::JavascriptError => {
            log::error!(target: "galfriends::frontend", "javascript_error window={} {}", webview.label(), event.detail)
        }
        FrontendKind::WasmPanic => {
            log::error!(target: "galfriends::frontend", "wasm_panic window={} {}", webview.label(), event.detail)
        }
        FrontendKind::Ready => {
            log::info!(target: "galfriends::frontend", "frontend_ready window={} {}", webview.label(), event.detail)
        }
        FrontendKind::View => {
            if let Some(view) = event.view {
                let (name, level) = match view.phase {
                    ViewPhase::Applied => ("view_applied", Level::Info),
                    ViewPhase::Frames => ("view_frames", Level::Info),
                    ViewPhase::FramesDelayed => ("view_frames_delayed", Level::Warn),
                };
                log::log!(target: "galfriends::frontend", level,
                    "{name} window={} view={} page={:?} overlay={:?} visible={} elapsed_ms={}",
                    webview.label(), view.sequence, view.page, view.overlay, event.visible, event.elapsed_ms);
            }
        }
        FrontendKind::Visibility => {
            log::info!(target: "galfriends::frontend", "frontend_visibility window={} visible={}",
                webview.label(), event.visible);
        }
    }
}
