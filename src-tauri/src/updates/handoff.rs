//! Windows 便携 EXE 的交接。辅助进程使用旧 EXE 副本，与主窗口完全独立。
use galfriends_core::updates::{
    Progress,
    executable::{self, Asset},
};
use galfriends_i18n::{Language, Message};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Component, Path, PathBuf},
    process::{self, Command},
    sync::atomic::AtomicBool,
    thread,
    time::Duration,
};
use tempfile::TempDir;
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

const PREFIX: &str = ".galfriends-update-";
const HELPER: &str = "updater.exe";
const JOB: &str = "handoff.json";
const PAYLOAD: &str = "payload.exe";
const BACKUP: &str = "previous.exe";
const CLEAN: &str = "cleanup-ready";
const HELPER_ARG: &str = "--galfriends-run-updater";
const CLEANUP_ARG: &str = "--galfriends-update-cleanup";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Deserialize)]
struct Handoff {
    parent: u32,
    executable_name: OsString,
    asset: Asset,
    language: Language,
}

fn error(detail: impl ToString) -> Message {
    Message::detail("update.install_failed", detail)
}

fn current_exe() -> Result<PathBuf, Message> {
    let path = std::env::current_exe().map_err(error)?;
    executable::check_entry(&path)?;
    path.canonicalize().map_err(error)
}

/// 下载阶段使用同卷暂存区；主进程尚未退出，失败可以直接在弹窗中重试。
pub(super) fn prepare(
    asset: Asset,
    proxy: &str,
    language: Language,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<TempDir, Message> {
    let executable = current_exe()?;
    let parent = executable
        .parent()
        .ok_or_else(|| error("Missing installation directory"))?;
    executable::check_entry(parent)?;
    let name = executable
        .file_name()
        .ok_or_else(|| error("Missing EXE name"))?;
    let stage = tempfile::Builder::new()
        .prefix(PREFIX)
        .tempdir_in(parent)
        .map_err(error)?;
    executable::download(&asset, proxy, &stage.path().join(PAYLOAD), cancel, progress)?;
    fs::copy(&executable, stage.path().join(HELPER)).map_err(error)?;
    let manifest = Handoff {
        parent: process::id(),
        executable_name: name.into(),
        asset,
        language,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(error)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(stage.path().join(JOB))
        .map_err(error)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(error)?;
    Ok(stage)
}

/// 交接程序成功启动后保留暂存目录，主程序随后通过 Tauri 正常退出。
pub(super) fn launch(stage: TempDir) -> Result<(), Message> {
    Command::new(stage.path().join(HELPER))
        .arg(HELPER_ARG)
        .current_dir(stage.path())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(error)?;
    let _ = stage.keep();
    Ok(())
}

pub(crate) fn run_from_args() -> bool {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref() {
        Some(arg) if arg == HELPER_ARG => {
            run();
            true
        }
        Some(arg) if arg == CLEANUP_ARG => {
            if let (Some(leaf), Some(pid)) = (args.next(), args.next())
                && let Some(pid) = pid.to_str().and_then(|pid| pid.parse::<u32>().ok())
            {
                thread::spawn(move || cleanup(&leaf, pid));
            }
            false
        }
        _ => false,
    }
}

fn run() {
    let mut language = Language::default();
    let outcome = (|| {
        let helper = current_exe()?;
        let folder = helper
            .parent()
            .ok_or_else(|| error("Missing update directory"))?;
        let root = folder
            .parent()
            .ok_or_else(|| error("Missing installation directory"))?;
        let leaf = folder
            .file_name()
            .filter(|leaf| stage_name(leaf))
            .ok_or_else(|| error("Invalid update directory"))?;
        if helper.file_name() != Some(OsStr::new(HELPER)) {
            return Err(error("Invalid update helper name"));
        }
        for path in [root, folder, &folder.join(JOB)] {
            executable::check_entry(path)?;
        }
        let mut bytes = Vec::new();
        File::open(folder.join(JOB))
            .map_err(error)?
            .take(16 * 1024)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        let manifest: Handoff = serde_json::from_slice(&bytes).map_err(error)?;
        language = manifest.language;
        let name = Path::new(&manifest.executable_name);
        if !is_leaf(name)
            || !name
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            return Err(error("Invalid EXE name"));
        }
        let exe = root.join(name);
        wait_for_exit(manifest.parent)?;
        let lock_path = root.join(".galfriends-app-update.lock");
        executable::check_entry(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(error)?;
        lock.try_lock().map_err(|_| Message::new("update.busy"))?;
        if !executable::same_binary(&helper, &exe)? {
            return Err(error("The original EXE changed during the update"));
        }
        let payload = folder.join(PAYLOAD);
        let backup = folder.join(BACKUP);
        if let Err(failure) = executable::verify(&payload, &manifest.asset)
            .and_then(|()| executable::replace(&exe, &payload, &backup))
        {
            restart(&exe, leaf).map_err(|restart| error(format!("{failure} / {restart}")))?;
            mark_cleanup(folder);
            return Err(failure);
        }
        if let Err(failure) = restart(&exe, leaf).and_then(|mut child| {
            thread::sleep(Duration::from_secs(2));
            if let Some(status) = child.try_wait().map_err(error)? {
                return Err(error(format!(
                    "The updated EXE exited during startup ({status})"
                )));
            }
            Ok(())
        }) {
            executable::restore(&exe, &backup)
                .map_err(|restore| error(format!("{failure} / {restore}")))?;
            restart(&exe, leaf).map_err(|restart| error(format!("{failure} / {restart}")))?;
            mark_cleanup(folder);
            return Err(failure);
        }
        mark_cleanup(folder);
        Ok(())
    })();
    if let Err(failure) = outcome {
        rfd::MessageDialog::new()
            .set_title("GalFriends")
            .set_description(failure.render(language))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}

fn mark_cleanup(folder: &Path) {
    // 没有清理标记时保留暂存区，特别是失败后仍可能需要手动恢复的旧 EXE。
    let _ = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(folder.join(CLEAN));
}

fn restart(exe: &Path, leaf: &OsStr) -> Result<process::Child, Message> {
    let root = exe
        .parent()
        .ok_or_else(|| error("Missing installation directory"))?;
    Command::new(exe)
        .arg(CLEANUP_ARG)
        .arg(leaf)
        .arg(process::id().to_string())
        .current_dir(root)
        .spawn()
        .map_err(error)
}

fn is_leaf(path: &Path) -> bool {
    let mut parts = path.components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

fn stage_name(leaf: &OsStr) -> bool {
    is_leaf(Path::new(leaf))
        && leaf
            .to_str()
            .and_then(|name| name.strip_prefix(PREFIX))
            .is_some_and(|suffix| {
                suffix.len() == 6 && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
            })
}

fn wait_for_exit(pid: u32) -> Result<(), Message> {
    if pid == 0 || pid == process::id() {
        return Err(error("Invalid update process ID"));
    }
    // SAFETY: OpenProcess 返回当前进程拥有的等待句柄。仅对非空句柄等待，并在返回前关闭。
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            let code = GetLastError();
            return if code == ERROR_INVALID_PARAMETER {
                Ok(()) // 已经退出。
            } else {
                Err(error(std::io::Error::from_raw_os_error(code as i32)))
            };
        }
        let outcome = WaitForSingleObject(handle, 60_000);
        CloseHandle(handle);
        if outcome == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(error("The previous process did not exit"))
        }
    }
}

fn cleanup(leaf: &OsStr, helper_pid: u32) {
    if !stage_name(leaf) || wait_for_exit(helper_pid).is_err() {
        return;
    }
    let Ok(exe) = current_exe() else {
        return;
    };
    let Some(root) = exe.parent() else {
        return;
    };
    let folder = root.join(leaf);
    if executable::check_entry(root).is_err() || executable::check_entry(&folder).is_err() {
        return;
    }
    let clean = folder.join(CLEAN);
    if executable::check_entry(&clean).is_err() || !clean.is_file() {
        return;
    }
    // 只删交接目录内固定文件。不递归，也不跟随备份路径中的重解析点。
    for file in [HELPER, JOB, PAYLOAD, BACKUP, CLEAN].map(|name| folder.join(name)) {
        if executable::check_entry(&file).is_ok() {
            let _ = fs::remove_file(file);
        }
    }
    let _ = fs::remove_dir(folder);
}
