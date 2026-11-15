//! 角色永久删除的持久意图与中断恢复；文件清理和库替换全部完成后才报告成功。
use crate::{archive, characters::Library};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[cfg(test)]
mod tests;

fn journal_path(path: &Path) -> Result<PathBuf, String> {
    let root = path.parent().ok_or("Missing data directory")?;
    archive::check_entry(root)?;
    let journal = root.join(".pending-character-deletion.json");
    archive::check_entry(&journal)?;
    Ok(journal)
}

pub(super) fn pending(path: &Path) -> Result<Option<Vec<String>>, String> {
    match fs::read(journal_path(path)?) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("Cannot read pending character deletion: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub(super) fn commit(path: &Path, next: &Library, ids: &[String]) -> Result<(), String> {
    if let Some(pending) = pending(path)?
        && pending != ids
    {
        return Err("Another character deletion is incomplete; reload the library first".into());
    }
    let root = path.parent().ok_or("Missing data directory")?;
    // 库序列化、保留角色归档和全体删除路径检查必须先成功，才写入不可逆删除意图。
    let prepared = super::prepare(path, next)?;
    archive::removal_paths(root, ids)?;
    let journal = journal_path(path)?;
    if !journal.exists() {
        let mut temp = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut temp, ids).map_err(|e| e.to_string())?;
        temp.flush().map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist_noclobber(&journal)
            .map_err(|e| e.to_string())?;
    }
    // 清理中断时不撤销删除意图，否则重启会用旧库重新生成已删除的资料。
    // 正常写入在意图存在时被阻止；重试同一删除或加载恢复后才能继续。
    archive::remove(root, ids)?;
    prepared.persist(path).map_err(|e| e.to_string())?;
    fs::remove_file(journal).map_err(|e| format!("Cannot finish character deletion: {e}"))?;
    Ok(())
}

pub(super) fn recover(path: &Path) -> Result<Option<Library>, String> {
    let Some(ids) = pending(path)? else {
        return Ok(None);
    };
    archive::check_entry(path)?;
    // 有未完成删除时，缺失/损坏的库必须报错，不能回退样本或旧迁移备份。
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let mut library = super::index::decode(path, &bytes, &ids)?;
    for members in library.collections.values_mut() {
        members.retain(|id| !ids.contains(id));
    }
    commit(path, &library, &ids)?;
    Ok(Some(library))
}
