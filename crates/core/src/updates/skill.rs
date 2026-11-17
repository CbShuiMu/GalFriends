//! 提取固定 commit 的 Skill ZIP，并在 GalFriends 私有目录内切换版本。
use super::{
    Progress, SkillInstall, SkillStatus, check_cancel,
    executable::{download_client, fetch},
    github::{latest_skill_commit, skill_status},
};
use galfriends_i18n::Message;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const REPOSITORY: &str = "Galgear-GalgameReverse-Skill";
const EXTRACTOR: &str = "GalgameResourceExtract";
const PREVIOUS: &str = ".previous";
const MARKER: &str = ".galgear-commit";
const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 8 * 1024 * 1024 * 1024;
const REQUIRED: &[&str] = &[
    "GalgameResourceExtract/SKILL.md",
    "GalgameResourceExtract/scripts/memory.py",
    "GalgameResourceExtract/catalog/formats.json",
    "GalgameResourceExtract/tools/GARbro-cli/GARbro.Console.exe",
];

fn error(detail: impl ToString) -> Message {
    Message::detail("update.skill_install_failed", detail)
}

/// 私有包只保存到现有 GalFriends 安装根下，外部目录仅用于旧配置回退。
pub fn home(install: &Path) -> PathBuf {
    install.join("GalFriends-Skills")
}

fn directory(path: &Path) -> Result<bool, Message> {
    crate::archive::check_entry(path).map_err(error)?;
    match fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => Err(error("Expected a Skill directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(error(e)),
    }
}

fn package_complete(root: &Path) -> bool {
    REQUIRED.iter().all(|name| {
        let file = root.join(name);
        // 校验中间目录，防止从私有包沿重解析点跳转到其他应用。
        file.ancestors()
            .take_while(|path| *path != root)
            .all(|path| crate::archive::check_entry(path).is_ok())
            && file
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.len() != 0)
    })
}

/// 返回实际可用的私有解包路径。安装中断后，仍可读上次完整的备份。
pub fn managed_directory(home: &Path) -> Result<Option<PathBuf>, Message> {
    if !directory(home)? {
        return Ok(None);
    }
    for path in [home.join(REPOSITORY), home.join(PREVIOUS)] {
        if directory(&path)? && package_complete(&path) {
            return Ok(Some(path.join(EXTRACTOR)));
        }
    }
    Ok(None)
}

fn lock_file(home: &Path) -> Result<File, Message> {
    let path = home.join(".galfriends-skill.lock");
    crate::archive::check_entry(&path).map_err(error)?;
    OpenOptions::new()
        .write(true)
        .read(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(error)
}

/// 私有 Skill 正由任务使用时，阻止其他桌面实例替换其文件。
pub fn read_lock(home: &Path) -> Result<Option<File>, Message> {
    if !directory(home)? {
        return Ok(None);
    }
    let lock = lock_file(home)?;
    lock.try_lock_shared()
        .map_err(|_| Message::new("update.skill_busy"))?;
    Ok(Some(lock))
}

/// 获取上游 commit、下载并校验完整 ZIP、提交新目录。失败保留上一版。
pub fn install(
    home: &Path,
    proxy: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<SkillInstall, Message> {
    if !directory(home)? {
        match fs::create_dir(home) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && directory(home)? => {}
            Err(e) => return Err(error(e)),
        }
    }
    let lock = lock_file(home)?;
    lock.try_lock()
        .map_err(|_| Message::new("update.skill_busy"))?;
    progress(Progress::Checking);
    check_cancel(cancel)?;
    let commit = latest_skill_commit(proxy)?;
    let installed = home.join(REPOSITORY);
    let previous = home.join(PREVIOUS);
    if !directory(&installed)? && directory(&previous)? {
        fs::rename(&previous, &installed).map_err(error)?;
    }
    if directory(&installed)?
        && package_complete(&installed)
        && skill_status(Some(&installed.join(EXTRACTOR)))
            .commit
            .as_deref()
            == Some(&commit)
    {
        return Ok(result(commit, false));
    }
    let staging = tempfile::Builder::new()
        .prefix(".skill-stage-")
        .tempdir_in(home)
        .map_err(error)?;
    let mut archive = tempfile::tempfile_in(staging.path())
        .map_err(|e| error(format!("Create ZIP staging file: {e}")))?;
    progress(Progress::Downloading { percent: None });
    let client = download_client(proxy, "update.skill_install_failed")?;
    let url = format!("https://codeload.github.com/CbShuiMu/{REPOSITORY}/zip/{commit}");
    let mut response = fetch(&client, &url, "update.skill_install_failed")?;
    let total = response.content_length().filter(|bytes| *bytes != 0);
    if total.is_some_and(|bytes| bytes > MAX_ARCHIVE) {
        return Err(error("Skill ZIP exceeds the download limit"));
    }
    let mut received = 0;
    let mut last = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = response
            .read(&mut buffer)
            .map_err(|e| error(format!("Read Skill download: {e}")))?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > MAX_ARCHIVE {
            return Err(error("Skill ZIP exceeds the download limit"));
        }
        archive
            .write_all(&buffer[..count])
            .map_err(|e| error(format!("Write Skill ZIP: {e}")))?;
        if let Some(total) = total {
            let percent = (received * 100 / total).min(100) as u8;
            if last != percent {
                last = percent;
                progress(Progress::Downloading {
                    percent: Some(percent),
                });
            }
        }
    }
    archive.rewind().map_err(error)?;
    progress(Progress::Extracting);
    let unpacked = staging.path().join("package");
    extract(archive, &unpacked, &commit, cancel)?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(unpacked.join(MARKER))
        .and_then(|mut file| {
            file.write_all(commit.as_bytes())?;
            file.sync_all()
        })
        .map_err(error)?;
    check_cancel(cancel)?;
    progress(Progress::Installing);
    commit_package(&unpacked, &installed, &previous)?;
    Ok(result(commit, true))
}

fn result(commit: String, changed: bool) -> SkillInstall {
    SkillInstall {
        status: SkillStatus {
            installed: true,
            commit: Some(commit),
            update_available: false,
        },
        changed,
    }
}

fn extract(
    archive: impl Read + Seek,
    output: &Path,
    sha: &str,
    cancel: &AtomicBool,
) -> Result<(), Message> {
    let mut zip = zip::ZipArchive::new(archive).map_err(error)?;
    if zip.len() > 50_000 {
        return Err(error("Too many files in the Skill ZIP"));
    }
    let prefix = format!("{REPOSITORY}-{sha}/");
    let mut remaining = MAX_EXTRACTED;
    for index in 0..zip.len() {
        check_cancel(cancel)?;
        let entry = zip.by_index(index).map_err(error)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
        {
            return Err(error(
                "Links and special files are not allowed in the Skill ZIP",
            ));
        }
        let name = entry
            .name()
            .strip_prefix(&prefix)
            .ok_or_else(|| error("Unexpected Skill ZIP root"))?;
        if name.is_empty() {
            continue;
        }
        let name = name.strip_suffix('/').unwrap_or(name).to_owned();
        if name.split('/').count() > 64 || !name.split('/').all(valid_part) {
            return Err(error("Unsafe Skill ZIP path"));
        }
        let target = output.join(&name);
        let entry_error = |e| error(format!("{name}: {e}"));
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(entry_error)?;
            continue;
        }
        if entry.size() > remaining {
            return Err(error("Skill ZIP exceeds the extraction limit"));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(entry_error)?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(entry_error)?;
        let count =
            std::io::copy(&mut entry.take(remaining + 1), &mut file).map_err(entry_error)?;
        remaining = remaining
            .checked_sub(count)
            .ok_or_else(|| error("Skill ZIP exceeds the extraction limit"))?;
    }
    if !package_complete(output) {
        return Err(error(
            "The Skill ZIP is missing required extraction resources",
        ));
    }
    Ok(())
}

fn valid_part(name: &str) -> bool {
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.chars().any(char::is_control)
    {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|tail| {
            matches!(
                tail,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn commit_package(unpacked: &Path, installed: &Path, previous: &Path) -> Result<(), Message> {
    let had_previous = directory(installed)?;
    if had_previous {
        if directory(previous)? {
            // 仅递归清理私有的固定备份槽，且整棵树拒绝链接与重解析点。
            check_tree(previous)?;
            fs::remove_dir_all(previous).map_err(error)?;
        }
        fs::rename(installed, previous)
            .map_err(|e| error(format!("Back up the installed Skill: {e}")))?;
    }
    if let Err(failure) = fs::rename(unpacked, installed) {
        if had_previous {
            fs::rename(previous, installed)
                .map_err(|restore| error(format!("{failure}; restore: {restore}")))?;
        }
        return Err(error(format!("Activate the staged Skill: {failure}")));
    }
    Ok(())
}

fn check_tree(path: &Path) -> Result<(), Message> {
    crate::archive::check_entry(path).map_err(error)?;
    if path.is_dir() {
        for child in fs::read_dir(path).map_err(error)? {
            check_tree(&child.map_err(error)?.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AtomicBool, REPOSITORY, REQUIRED, Write, commit_package, extract, fs};
    use std::io::Cursor;

    #[test]
    fn rejects_escape_and_keeps_previous_package_on_failed_commit() {
        let temp = tempfile::tempdir().unwrap();
        let commit = "a".repeat(40);
        let cancelled = AtomicBool::new(false);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in REQUIRED.iter().copied().chain(["nested/../escape"]) {
            zip.start_file(
                format!("{REPOSITORY}-{commit}/{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        let unpacked = temp.path().join("package");
        assert!(extract(zip.finish().unwrap(), &unpacked, &commit, &cancelled).is_err());
        assert!(!unpacked.join("escape").exists());
        let installed = temp.path().join(REPOSITORY);
        let backup = temp.path().join(".previous");
        fs::create_dir(&installed).unwrap();
        fs::write(installed.join("sentinel"), b"old version").unwrap();
        assert!(commit_package(&temp.path().join("missing"), &installed, &backup).is_err());
        assert_eq!(
            fs::read(installed.join("sentinel")).unwrap(),
            b"old version"
        );
    }
}
