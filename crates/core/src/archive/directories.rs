//! 以来源罗马字命名，身份摘要防止同名角色共用目录。
use super::{check_entry, digest, validate_tree};
use crate::characters::Character;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// 按身份定位专属目录。兼容旧摘要目录，姓名更新后保持既有罗马字路径。
pub fn character_directory(root: &Path, id: &str) -> Result<PathBuf, String> {
    check_entry(root)?;
    let hash = digest(id.as_bytes());
    let legacy = root.join(format!("character-{hash}"));
    check_entry(&legacy)?;
    let mut found = legacy
        .try_exists()
        .map_err(|e| e.to_string())?
        .then_some(legacy.clone());
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(legacy),
        Err(error) => return Err(error.to_string()),
    };
    let suffix = format!("--{hash}");
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(&suffix))
        {
            let path = entry.path();
            check_entry(&path)?;
            if !path.is_dir() || found.replace(path).is_some() {
                return Err("Multiple or invalid directories for one character identity".into());
            }
        }
    }
    Ok(found.unwrap_or(legacy))
}

fn romanized_component(character: &Character) -> Option<String> {
    let name = character.romanized_name().or_else(|| {
        let name = character.name.trim();
        (name.is_ascii() && name.bytes().any(|b| b.is_ascii_alphabetic())).then_some(name)
    })?;
    let safe: String = name
        .chars()
        .take(64)
        .map(|ch| {
            if ch.is_control()
                || matches!(
                    ch,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '.'
                )
            {
                ' '
            } else {
                ch
            }
        })
        .collect();
    let safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    safe.chars().any(char::is_alphabetic).then_some(safe)
}

pub(super) fn prepare(
    root: &Path,
    character: &Character,
    migrate: bool,
) -> Result<PathBuf, String> {
    let current = character_directory(root, &character.id)?;
    let hash = digest(character.id.as_bytes());
    if current != root.join(format!("character-{hash}")) {
        return Ok(current);
    }
    if !migrate && current.try_exists().map_err(|e| e.to_string())? {
        return Ok(current);
    }
    let Some(name) = romanized_component(character) else {
        return Ok(current);
    };
    let named = root.join(format!("{name}--{hash}"));
    check_entry(&named)?;
    if current.try_exists().map_err(|e| e.to_string())? {
        if !current.is_dir() {
            return Err(format!(
                "Invalid character directory: {}",
                current.display()
            ));
        }
        // 旧版允许把模型或素材输出到任意位置。含自定义数据的目录保留原路径，
        // 避免使映射、模型绑定或历史资料中的绝对路径失效。
        for entry in fs::read_dir(&current).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name = name.to_str().unwrap_or_default();
            let built_in = matches!(
                name,
                "expressions" | "voice-model.json" | "emotion-references.json"
            ) || (name.starts_with("profile-") && name.ends_with(".json"));
            if !built_in {
                return Ok(current);
            }
        }
        validate_tree(&root.canonicalize().map_err(|e| e.to_string())?, &current)?;
        fs::rename(&current, &named).map_err(|e| e.to_string())?;
    }
    Ok(named)
}
