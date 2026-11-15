//! 收藏与取消收藏共用索引事务，提交失败时保留内存状态。
use crate::{
    characters::{FavoriteUpdate, Library},
    library_ops,
};
use std::path::Path;

/// 原子保存单个或批量收藏并返回轻量结果，调用方须持有角色库事务锁。
/// 全部身份验证通过后才提交。旧整库格式首次写入会先准备完整归档。
pub fn set_favorites(
    path: &Path,
    library: &mut Library,
    ids: &[String],
    favorite: bool,
) -> Result<FavoriteUpdate, String> {
    let targets = library_ops::validate_targets(library, ids)?;
    super::ensure_writable(path)?;
    let mut index = match super::index::read(path)? {
        Some(index) => index,
        None => super::index::prepare(path, library, None)?,
    };
    for id in &targets {
        let entry = index
            .characters
            .iter_mut()
            .find(|entry| entry.id == *id)
            .ok_or("Character missing from library index")?;
        entry.favorite = Some(favorite);
    }
    super::write_index(path, &index)?
        .persist(path)
        .map_err(|e| e.to_string())?;
    let update = FavoriteUpdate {
        ids: targets.into_iter().map(str::to_owned).collect(),
        favorite,
    };
    update.apply(library);
    Ok(update)
}
