//! 沿用 GalGear 的同目录临时文件、刷盘、原子替换机制。
use crate::characters::{Library, sample};
use std::{fs, io::Write, path::Path};
mod deletion;
mod favorites;
mod index;
mod order;
pub use favorites::set_favorites;
pub use order::{reorder_characters, set_library_sort};
#[cfg(test)]
mod index_tests;
/// 仅首次不存在时加载样本；损坏文件报错，不重置用户库。
pub fn load(path: &Path) -> Result<Library, String> {
    if let Some(library) = deletion::recover(path)? {
        return Ok(library);
    }
    read(path)
}

fn read(path: &Path) -> Result<Library, String> {
    crate::archive::check_entry(path)?;
    match fs::read(path) {
        Ok(bytes) => {
            index::decode(path, &bytes, &[]).map_err(|e| format!("Cannot read library: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Library {
            characters: vec![sample()?],
            ..Default::default()
        }),
        Err(e) => Err(e.to_string()),
    }
}
/// 成功替换后调用方才能更新内存；同一实例由命令层串行化。
pub fn save(path: &Path, library: &Library) -> Result<(), String> {
    ensure_writable(path)?;
    prepare(path, library)?
        .persist(path)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 拒绝在未完成删除期间创建角色专属配置，调用方须持有库事务。
pub fn ensure_writable(path: &Path) -> Result<(), String> {
    if deletion::pending(path)?.is_some() {
        return Err(
            "Character deletion is incomplete; retry deletion or reload the library".into(),
        );
    }
    Ok(())
}

/// 原子保存变更；删除角色还会清除其专属归档目录及历史版本。
/// 调用方必须串行化并仅在成功后发布内存。删除中断会留下意图记录，重试或加载时续做。
pub fn save_changes(path: &Path, previous: &Library, next: &Library) -> Result<(), String> {
    let removed: Vec<_> = previous
        .characters
        .iter()
        .filter(|c| !next.characters.iter().any(|n| n.id == c.id))
        .map(|c| c.id.clone())
        .collect();
    if removed.is_empty() {
        ensure_writable(path)?;
        let index = index::prepare(path, next, Some(previous))?;
        write_index(path, &index)?
            .persist(path)
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        deletion::commit(path, next, &removed)
    }
}

fn prepare(path: &Path, library: &Library) -> Result<tempfile::NamedTempFile, String> {
    let index = index::prepare(path, library, None)?;
    write_index(path, &index)
}

fn write_index(path: &Path, index: &index::Index) -> Result<tempfile::NamedTempFile, String> {
    if !matches!(
        index.settings.tag_source,
        crate::characters::Source::Kungal | crate::characters::Source::Vndb
    ) {
        return Err("Unsupported tag source".into());
    }
    let parent = path.parent().ok_or("Missing data directory")?;
    crate::archive::check_entry(parent)?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    crate::archive::check_entry(path)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temp, index).map_err(|e| e.to_string())?;
    temp.flush().map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    Ok(temp)
}

/// 仅原子更新主题设置，成功后才发布内存并返回轻量响应。
/// 调用方须共用库事务锁；旧库首次写入仍准备完整归档，不复制或比较图片。
pub fn set_appearance(
    path: &Path,
    library: &mut Library,
    key: &str,
    value: &str,
) -> Result<crate::preferences::Appearance, String> {
    if !matches!(key, "theme" | "color_theme") {
        return Err("Invalid appearance setting".into());
    }
    let mut settings = library.settings.clone();
    crate::preferences::set(&mut settings, key, value)?;
    ensure_writable(path)?;
    let mut index = match index::read(path)? {
        Some(index) => index,
        None => index::prepare(path, library, None)?,
    };
    index.settings = settings.clone();
    write_index(path, &index)?
        .persist(path)
        .map_err(|e| e.to_string())?;
    library.settings = settings;
    Ok(crate::preferences::Appearance {
        theme: library.settings.theme.clone(),
        color_theme: library.settings.color_theme.clone(),
    })
}

/// 仅原子更新轻量库索引中的立绘选择，成功后才修改内存。
/// 调用方须与其他库操作共用事务锁；旧库首次操作会先准备完整归档。
pub fn select_artwork(
    path: &Path,
    library: &mut Library,
    id: &str,
    selected_art: usize,
) -> Result<(), String> {
    ensure_writable(path)?;
    let character = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("Character not found")?;
    if selected_art >= character.artworks.len() {
        return Err("Invalid artwork".into());
    }
    if character.selected_art == selected_art {
        return Ok(());
    }
    let mut index = match index::read(path)? {
        Some(index) => index,
        None => index::prepare(path, library, None)?,
    };
    let entry = index
        .characters
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Character missing from library index")?;
    entry.selected_art = selected_art;
    write_index(path, &index)?
        .persist(path)
        .map_err(|e| e.to_string())?;
    if let Some(character) = library.characters.iter_mut().find(|c| c.id == id) {
        character.selected_art = selected_art;
    }
    Ok(())
}

/// 新目录存在时优先使用，损坏不回退；首次迁移只读取旧库，不覆盖旧文件。
pub fn load_for_migration(path: &Path, legacy: &Path) -> Result<Library, String> {
    crate::archive::check_entry(path.parent().ok_or("Missing data directory")?)?;
    crate::archive::check_entry(path)?;
    if path.exists() || deletion::pending(path)?.is_some() || !legacy.exists() {
        let library = load(path)?;
        if path.exists() {
            crate::archive::migrate_directories(
                path.parent().ok_or("Missing data directory")?,
                &library.characters,
            )?;
        }
        Ok(library)
    } else {
        load(legacy)
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn migration_preserves_legacy_and_never_falls_back_from_a_corrupt_new_library() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("legacy/library.json");
        let new = dir.path().join("software/characters/library.json");
        let mut library = super::load(&old).unwrap();
        library.characters[0].favorite = true;
        super::save(&old, &library).unwrap();
        let original = std::fs::read(&old).unwrap();
        let migrated = super::load_for_migration(&new, &old).unwrap();
        super::save(&new, &migrated).unwrap();
        assert!(super::load(&new).unwrap().characters[0].favorite);
        assert_eq!(std::fs::read(&old).unwrap(), original);
        std::fs::write(&new, b"corrupt").unwrap();
        assert!(super::load_for_migration(&new, &old).is_err());
    }
    #[test]
    fn corrupt_file_is_not_reset() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("library.json");
        std::fs::write(&p, b"broken").unwrap();
        assert!(super::load(&p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"broken");
    }
    #[test]
    fn favorite_and_settings_survive_restart() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("library.json");
        let mut l = super::load(&p).unwrap();
        l.characters[0].favorite = true;
        l.settings.theme = "dark".into();
        super::save(&p, &l).unwrap();
        let l = super::load(&p).unwrap();
        assert!(l.characters[0].favorite);
        assert_eq!(l.settings.theme, "dark");
    }
}
