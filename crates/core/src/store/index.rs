//! 当前库保存版本引用、立绘选择与收藏状态；完整资料使用不可变角色归档。
use crate::{
    archive,
    characters::{Library, Settings},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Serialize, Deserialize)]
pub(super) struct Entry {
    pub id: String,
    pub profile: String,
    pub selected_art: usize,
    // 旧版索引缺失时沿用档案值，不能默认为未收藏。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Index {
    pub format_version: u32,
    pub characters: Vec<Entry>,
    pub collections: BTreeMap<String, Vec<String>>,
    pub collection_parents: BTreeMap<String, String>,
    pub settings: Settings,
}

fn version(bytes: &[u8]) -> Result<Option<u32>, String> {
    #[derive(Deserialize)]
    struct Header {
        format_version: Option<u32>,
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    match header.format_version {
        None | Some(2) => Ok(header.format_version),
        Some(_) => Err("Unsupported library format version".into()),
    }
}

pub(super) fn read(path: &Path) -> Result<Option<Index>, String> {
    archive::check_entry(path.parent().ok_or("Missing data directory")?)?;
    archive::check_entry(path)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if version(&bytes)?.is_none() {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}

// 删除恢复必须先跳过已确认删除的身份，其归档可能已经清理完毕。
pub(super) fn decode(path: &Path, bytes: &[u8], removed: &[String]) -> Result<Library, String> {
    if version(bytes)?.is_none() {
        let mut library: Library = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        library.characters.retain(|c| !removed.contains(&c.id));
        return Ok(library);
    }
    let index: Index = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let root = path.parent().ok_or("Missing data directory")?;
    let mut characters = Vec::with_capacity(index.characters.len());
    for entry in index.characters {
        if removed.contains(&entry.id) {
            continue;
        }
        if characters
            .iter()
            .any(|c: &crate::characters::Character| c.id == entry.id)
        {
            return Err("Duplicate character identity in library".into());
        }
        let mut character = archive::read_character(root, &entry.id, &entry.profile)?;
        character.selected_art = entry.selected_art;
        if let Some(favorite) = entry.favorite {
            character.favorite = favorite;
        }
        characters.push(character);
    }
    Ok(Library {
        characters,
        collections: index.collections,
        collection_parents: index.collection_parents,
        settings: index.settings,
    })
}

pub(super) fn prepare(
    path: &Path,
    library: &Library,
    previous: Option<&Library>,
) -> Result<Index, String> {
    let root = path.parent().ok_or("Missing data directory")?;
    let existing = read(path)?;
    let mut characters = Vec::with_capacity(library.characters.len());
    for character in &library.characters {
        let unchanged = previous.and_then(|l| l.characters.iter().find(|c| c.id == character.id))
            == Some(character);
        let entry = existing
            .as_ref()
            .and_then(|i| i.characters.iter().find(|e| e.id == character.id));
        let profile = match entry.filter(|_| unchanged) {
            Some(entry) => entry.profile.clone(),
            None => archive::save_character(root, character)?,
        };
        characters.push(Entry {
            id: character.id.clone(),
            profile,
            selected_art: character.selected_art,
            favorite: Some(character.favorite),
        });
    }
    Ok(Index {
        format_version: 2,
        characters,
        collections: library.collections.clone(),
        collection_parents: library.collection_parents.clone(),
        settings: library.settings.clone(),
    })
}
