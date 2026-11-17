//! 角色库的批量内存变更；删除的归档清理由 store 提交事务完成。
use crate::characters::{FavoriteUpdate, Library};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
/// 角色库支持的批量操作；收藏为赋值操作，重复提交不会反向切换。
pub enum BatchOperation {
    Favorite,
    Unfavorite,
    Collect,
    Detach,
    Remove,
}

/// 先校验全部目标再修改；未知角色或收藏夹不会产生部分变更。
pub fn apply_batch(
    library: &mut Library,
    operation: BatchOperation,
    ids: &[String],
    collection: &str,
) -> Result<(), String> {
    let targets = validate_targets(library, ids)?;
    match operation {
        BatchOperation::Favorite | BatchOperation::Unfavorite => {
            FavoriteUpdate {
                ids: targets.into_iter().map(str::to_owned).collect(),
                favorite: matches!(operation, BatchOperation::Favorite),
            }
            .apply(library);
        }
        BatchOperation::Collect | BatchOperation::Detach => {
            if matches!(operation, BatchOperation::Collect)
                && !crate::collections::accepts_characters(library, collection)
            {
                return Err("Choose a leaf collection".into());
            }
            let members = library
                .collections
                .get_mut(collection)
                .ok_or("Collection not found")?;
            if matches!(operation, BatchOperation::Collect) {
                for id in ids {
                    if !members.contains(id) {
                        members.push(id.clone());
                    }
                }
            } else {
                members.retain(|id| !targets.contains(id.as_str()));
            }
        }
        BatchOperation::Remove => {
            library
                .settings
                .library_order
                .custom_order
                .retain(|id| !targets.contains(id.as_str()));
            library
                .characters
                .retain(|c| !targets.contains(c.id.as_str()));
            for members in library.collections.values_mut() {
                members.retain(|id| !targets.contains(id.as_str()));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_targets<'a>(
    library: &Library,
    ids: &'a [String],
) -> Result<BTreeSet<&'a str>, String> {
    let targets: BTreeSet<_> = ids.iter().map(String::as_str).collect();
    if targets.is_empty() {
        return Err("No characters selected".into());
    }
    if targets
        .iter()
        .any(|id| !library.characters.iter().any(|c| c.id == *id))
    {
        return Err("Character not found".into());
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::{BatchOperation, apply_batch};
    use crate::characters::{Library, sample};
    #[test]
    fn validates_all_targets_before_changing_anything() {
        let mut l = Library::default();
        let c = sample().unwrap();
        let ids = [c.id.clone(), "missing".into()];
        l.characters.push(c);
        assert!(apply_batch(&mut l, BatchOperation::Favorite, &ids, "").is_err());
        assert!(!l.characters[0].favorite);
        assert!(apply_batch(&mut l, BatchOperation::Collect, &ids[..1], "missing").is_err());
        assert!(l.collections.is_empty());
    }
    #[test]
    fn membership_is_idempotent_and_removal_cleans_all_collections() {
        let mut l = Library::default();
        let c = sample().unwrap();
        let ids = [c.id.clone()];
        l.characters.push(c);
        l.collections.insert("a".into(), vec![]);
        for _ in 0..2 {
            apply_batch(&mut l, BatchOperation::Collect, &ids, "a").unwrap();
        }
        assert_eq!(l.collections["a"], ids);
        apply_batch(&mut l, BatchOperation::Favorite, &ids, "").unwrap();
        assert!(l.characters[0].favorite);
        apply_batch(&mut l, BatchOperation::Remove, &ids, "").unwrap();
        assert!(l.characters.is_empty());
        assert!(l.collections["a"].is_empty());
    }
}
