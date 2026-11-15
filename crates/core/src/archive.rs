//! 软件目录内的角色资料归档。普通保存追加版本，明确删除角色时清除整个专属目录。
use crate::characters::Character;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn character_directory(root: &Path, id: &str) -> PathBuf {
    root.join(format!("character-{}", digest(id.as_bytes())))
}

/// 拒绝符号链接及 Windows 重解析点，避免归档写入外部位置。
pub(crate) fn check_entry(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let reparse = false;
            if meta.file_type().is_symlink() || reparse {
                return Err(format!(
                    "Archive path must not be a link: {}",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn directory(path: &Path) -> Result<(), String> {
    check_entry(path)?;
    fs::create_dir_all(path).map_err(|e| e.to_string())
}

/// 保存完整角色版本（含别名、来源原始响应、图片地址及超分数据）。
/// 身份与内容摘要作为路径，不使用外部姓名/ID 作为文件名；原有版本永不覆盖。
/// 调用方应在当前角色库原子提交前调用；失败不能发布新库。
pub fn prepare(root: &Path, characters: &[Character]) -> Result<(), String> {
    directory(root)?;
    for character in characters {
        save_character(root, character)?;
    }
    Ok(())
}

pub(crate) fn save_character(root: &Path, character: &Character) -> Result<String, String> {
    directory(root)?;
    let folder = character_directory(root, &character.id);
    directory(&folder)?;
    let bytes = serde_json::to_vec_pretty(character).map_err(|e| e.to_string())?;
    let hash = digest(&bytes);
    let path = folder.join(format!("profile-{hash}.json"));
    check_entry(&path)?;
    if path.exists() {
        if fs::read(&path).map_err(|e| e.to_string())? != bytes {
            return Err(format!(
                "Archive content does not match its digest: {}",
                path.display()
            ));
        }
        return Ok(hash);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary
        .persist_noclobber(&path)
        .map_err(|e| e.to_string())?;
    Ok(hash)
}

pub(crate) fn read_character(root: &Path, id: &str, hash: &str) -> Result<Character, String> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid character profile digest".into());
    }
    check_entry(root)?;
    let folder = character_directory(root, id);
    check_entry(&folder)?;
    let path = folder.join(format!("profile-{hash}.json"));
    check_entry(&path)?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if digest(&bytes) != hash {
        return Err("Character profile content does not match its digest".into());
    }
    let character: Character = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if character.id != id {
        return Err("Character profile identity does not match library".into());
    }
    Ok(character)
}

/// 删除前检查整个专属目录，拒绝链接及越界目标；全部目标通过后才开始清理。
pub(crate) fn removal_paths(root: &Path, ids: &[String]) -> Result<Vec<PathBuf>, String> {
    check_entry(root)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let paths: Vec<_> = ids
        .iter()
        .map(|id| character_directory(&root, id))
        .collect();
    for path in &paths {
        validate_tree(&root, path)?;
    }
    Ok(paths)
}

fn validate_tree(root: &Path, path: &Path) -> Result<(), String> {
    check_entry(path)?;
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let resolved = path.canonicalize().map_err(|e| e.to_string())?;
    if resolved == root || !resolved.starts_with(root) {
        return Err("Character archive is outside its data directory".into());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            validate_tree(root, &entry.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
}

pub(crate) fn remove(root: &Path, ids: &[String]) -> Result<(), String> {
    for path in removal_paths(root, ids)? {
        // 只删除由身份摘要构造且已验证在根目录内的目录，不读取记录中的任意图片路径。
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(error) = result
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(format!(
                "Cannot delete character archive {}: {error}",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use crate::characters::sample;
    #[test]
    fn versions_keep_complete_data_and_untrusted_identity_stays_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let mut character = sample().unwrap();
        character.id = "../../outside:character".into();
        let original = serde_json::to_value(&character).unwrap();
        prepare(dir.path(), &[character.clone()]).unwrap();
        let folder = std::fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let record = std::fs::read_dir(&folder)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&record).unwrap()).unwrap(),
            original
        );
        character.favorite = true;
        prepare(dir.path(), &[character.clone()]).unwrap();
        prepare(dir.path(), &[character]).unwrap();
        prepare(dir.path(), &[]).unwrap();
        assert_eq!(std::fs::read_dir(folder).unwrap().count(), 2);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn archive_failure_preserves_current_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut library = crate::store::load(&path).unwrap();
        crate::store::save(&path, &library).unwrap();
        let before = std::fs::read(&path).unwrap();
        let folder = dir
            .path()
            .join(format!("character-{}", super::digest(b"blocked")));
        std::fs::write(folder, b"not a directory").unwrap();
        library.characters[0].id = "blocked".into();
        assert!(crate::store::save(&path, &library).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
