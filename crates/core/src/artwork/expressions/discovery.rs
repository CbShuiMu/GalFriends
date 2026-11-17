//! 关联目录、持久化启用标记和扫描进程边界；提取算法由离线 Python 包复用。
use crate::{
    characters::{Character, ExpressionScan},
    game_library,
};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// 扫描断点只放在明确角色的专属目录，旧角色在首次点击 face 前不会自动扫描。
pub fn checkpoint(root: &Path, id: &str) -> Result<PathBuf, String> {
    let path = super::catalog_path(root, id)?.with_file_name("discovery.json");
    crate::archive::check_entry(&path)?;
    Ok(path)
}

/// 在库短事务内调用，确保退出发生在启动进程之前时，下次仍会恢复。
pub fn enable(root: &Path, id: &str) -> Result<(), String> {
    let path = checkpoint(root, id)?;
    if !path.try_exists().map_err(|e| e.to_string())? {
        crate::companion::storage::write_json(
            &path,
            &json!({
                "format_version":1, "character_id":id, "enabled":true, "games":{}
            }),
        )?;
    }
    // 已完成目录再次检查新关联时，也必须在进程启动之前留下待处理请求。
    // 独立小文件避免在库事务内重写可能较大的成员清单。
    crate::companion::storage::write_json(
        &path.with_file_name("scan-request.json"),
        &json!({"format_version":1,"character_id":id,"pending":true}),
    )
}

/// 启动时只恢复未完成任务，已完成的目录留待下一次打开 face 检查新增关联。
pub fn pending(root: &Path, id: &str) -> Result<bool, String> {
    let path = checkpoint(root, id)?;
    let request = path.with_file_name("scan-request.json");
    crate::archive::check_entry(&request)?;
    if request.try_exists().map_err(|e| e.to_string())? {
        let value: serde_json::Value = serde_json::from_reader(
            fs::File::open(request)
                .map_err(|e| e.to_string())?
                .take(16384),
        )
        .map_err(|e| e.to_string())?;
        if value["format_version"] != 1 || value["character_id"] != id {
            return Err("Invalid expression request identity".into());
        }
        if value["pending"] == true {
            return Ok(true);
        }
    }
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let state: serde_json::Value =
        serde_json::from_reader(file.take(64 * 1024 * 1024)).map_err(|e| e.to_string())?;
    if state["format_version"] != 1 || state["character_id"] != id {
        return Err("Invalid expression discovery identity".into());
    }
    Ok(state["stage"] != "finished" || state["result"] == "failed")
}

/// 使用明确作品关联，按规范化路径合并重复安装记录，不依赖作品或角色名称。
pub fn folders(
    character: &Character,
    catalog: &game_library::Catalog,
) -> Result<Vec<PathBuf>, String> {
    let mut folders = Vec::new();
    let mut identities = std::collections::HashSet::new();
    for path in character
        .work_folders
        .iter()
        .filter(|(id, _)| game_library::work_folder(character, id).is_some())
        .map(|(_, path)| path.as_str())
        .chain(
            game_library::linked_games(character, catalog)
                .into_iter()
                .filter(|game| game.available)
                .map(|game| game.folder.as_str()),
        )
    {
        game_library::validate_work_folder(Path::new(path))?;
        let path = Path::new(path).canonicalize().map_err(|e| e.to_string())?;
        let identity = path.to_string_lossy().to_lowercase();
        if identities.insert(identity) {
            folders.push(path);
        }
    }
    Ok(folders)
}

struct Worker(std::process::Child);
impl Drop for Worker {
    fn drop(&mut self) {
        // worker 的 Windows Job 同时终止 GARbro 后代，断点保留最后完整提交。
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 可取消地运行已启用扫描。调用方必须阻止扫描期间删除该角色，不能持库锁等待。
pub fn run(
    project: &Path,
    library: &Path,
    character: &Character,
    games: &[PathBuf],
    cancelled: &AtomicBool,
    progress: impl Fn(ExpressionScan),
) -> Result<String, String> {
    let root = library.parent().ok_or("Missing character directory")?;
    let status_path = checkpoint(root, &character.id)?.with_file_name("scan-progress.json");
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string();
    let indices: Vec<_> = character
        .artworks
        .iter()
        .enumerate()
        .filter(|(_, art)| {
            !art.id.starts_with("expression-")
                && (art.url.starts_with("data:image/png;") || art.url.starts_with("assets/"))
        })
        .map(|(index, _)| index)
        .take(32)
        .collect();
    if indices.is_empty() {
        return Err("expression.scan_no_reference".into());
    }
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = temp.path().join("request.json");
    fs::write(
        &input,
        serde_json::to_vec(&json!({
            "character_id":character.id, "library":library, "games":games, "run":run,
            "assets":project.join("dist"), "artwork_indices":indices,
            "garbro":project.join("tools/voice/GARbro-cli/GARbro.Console.exe"),
            "helper":project.join("tools/voice/agent_archive_tools.py")
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let log_path = temp.path().join("worker.log");
    let mut command = Command::new(project.join(".venv/Scripts/python.exe"));
    command
        .args(["-B", "-u"])
        .arg(project.join("crates/core/src/artwork/python/scan_worker.py"))
        .arg(input)
        .arg(std::process::id().to_string())
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&log_path).map_err(|e| e.to_string())?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00004000);
    }
    let mut worker = Worker(command.spawn().map_err(|e| e.to_string())?);
    let mut last = None;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Ok("paused".into());
        }
        if let Ok(file) = fs::File::open(&status_path) {
            let value: serde_json::Value =
                serde_json::from_reader(file.take(16384)).map_err(|e| e.to_string())?;
            if value["run"] == run {
                let status = ExpressionScan {
                    character_id: character.id.clone(),
                    stage: value["stage"].as_str().unwrap_or_default().into(),
                    current: value["current"].as_u64().unwrap_or_default() as usize,
                    total: value["total"].as_u64().unwrap_or_default() as usize,
                    result: value["result"].as_str().unwrap_or_default().into(),
                    ..Default::default()
                };
                if last.as_ref() != Some(&status) {
                    // finished 只有在子进程确认成功退出后才发布。
                    if status.stage != "finished" {
                        progress(status.clone());
                    }
                    last = Some(status);
                }
            }
        }
        if let Some(status) = worker.0.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                let mut detail = String::new();
                fs::File::open(&log_path)
                    .and_then(|file| file.take(4096).read_to_string(&mut detail))
                    .map_err(|e| e.to_string())?;
                return Err(format!("Expression scan failed: {}", detail.trim()));
            }
            let file = fs::File::open(&status_path).map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_reader(file.take(16384)).map_err(|e| e.to_string())?;
            if value["run"] == run && value["stage"] == "finished" {
                if matches!(
                    value["result"].as_str(),
                    Some("complete" | "empty" | "unsupported")
                ) {
                    crate::companion::storage::write_json(
                        &status_path.with_file_name("scan-request.json"),
                        &json!({"format_version":1,"character_id":character.id,"pending":false}),
                    )?;
                }
                return value["result"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Missing expression scan result".into());
            }
            return Err("Missing expression scan result".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::{checkpoint, enable, folders, pending};
    use crate::{
        characters::{Character, Snapshot, Source},
        game_library::{Catalog, Game, Identity},
    };
    #[test]
    fn first_open_is_durable_and_only_incomplete_work_resumes() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!pending(temp.path(), "c").unwrap());
        enable(temp.path(), "c").unwrap();
        assert!(pending(temp.path(), "c").unwrap());
        let path = checkpoint(temp.path(), "c").unwrap();
        let value = serde_json::json!({"format_version":1,"character_id":"c","stage":"finished","result":"complete"});
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = std::fs::read(&path).unwrap();
        enable(temp.path(), "c").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            pending(temp.path(), "c").unwrap(),
            "new-directory checks must survive exit before worker startup"
        );
        std::fs::remove_file(path.with_file_name("scan-request.json")).unwrap();
        assert!(!pending(temp.path(), "c").unwrap());
        std::fs::write(path, b"broken").unwrap();
        assert!(pending(temp.path(), "c").is_err());
    }

    #[test]
    fn manual_and_catalog_paths_are_unified_without_name_matching() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().to_string_lossy().into_owned();
        let character = Character {
            id: "c".into(),
            work_folders: [("1".into(), folder.clone())].into(),
            snapshots: vec![Snapshot {
                source: Source::Kungal,
                id: "c".into(),
                url: String::new(),
                data: serde_json::json!({"appearances":[{"spoiler":"none","work":{"id":"1"}}]}),
            }],
            ..Default::default()
        };
        let game = Game {
            id: "g".into(),
            title: "same".into(),
            folder: folder.clone(),
            available: true,
            identities: vec![Identity {
                source: Source::Kungal,
                id: "1".into(),
            }],
            cover_hash: String::new(),
        };
        let catalog = Catalog {
            games: vec![game],
            ..Default::default()
        };
        assert_eq!(folders(&character, &catalog).unwrap().len(), 1);
        let mut unrelated = catalog;
        unrelated.games[0].identities.clear();
        let mut character = character;
        character.work_folders.clear();
        assert!(folders(&character, &unrelated).unwrap().is_empty());
    }
}
