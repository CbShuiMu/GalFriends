//! 命名收藏夹的两级结构与编辑规则，保留旧平铺格式及角色资料。
use crate::characters::Library;
use std::collections::BTreeSet;

/// 只有叶子收藏夹可以添加角色；已有父级中的成员仍保留。
pub fn accepts_characters(library: &Library, name: &str) -> bool {
    library.collections.contains_key(name)
        && !library
            .collection_parents
            .values()
            .any(|parent| parent == name)
}

/// 新建或编辑收藏夹。验证名字、父级与深度后再修改，不覆盖同名项。
pub fn save(
    library: &mut Library,
    original: Option<&str>,
    name: &str,
    parent: &str,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("Invalid collection name".into());
    }
    if let Some(old) = original
        && !library.collections.contains_key(old)
    {
        return Err("Collection not found".into());
    }
    if original != Some(name) && library.collections.contains_key(name) {
        return Err("Collection name already exists".into());
    }
    if !parent.is_empty() {
        if !library.collections.contains_key(parent)
            || library.collection_parents.contains_key(parent)
            || parent == name
            || original == Some(parent)
        {
            return Err("Invalid collection parent".into());
        }
        if original.is_some_and(|old| library.collection_parents.values().any(|p| p == old)) {
            return Err("Collection depth exceeds two levels".into());
        }
    }
    let members = original
        .and_then(|old| library.collections.remove(old))
        .unwrap_or_default();
    if let Some(old) = original {
        library.collection_parents.remove(old);
        for p in library.collection_parents.values_mut() {
            if p == old {
                *p = name.into();
            }
        }
    }
    library.collections.insert(name.into(), members);
    if !parent.is_empty() {
        library
            .collection_parents
            .insert(name.into(), parent.into());
    }
    Ok(())
}

/// 删除选中收藏夹及其子项，仅删除收藏关系，不删除角色或原图。
pub fn remove(library: &mut Library, names: &[String]) -> Result<(), String> {
    if names.is_empty() || names.iter().any(|n| !library.collections.contains_key(n)) {
        return Err("Collection not found".into());
    }
    let mut removed: BTreeSet<String> = names.iter().cloned().collect();
    loop {
        let descendants: Vec<_> = library
            .collection_parents
            .iter()
            .filter(|(_, p)| removed.contains(*p))
            .map(|(n, _)| n.clone())
            .collect();
        let before = removed.len();
        removed.extend(descendants);
        if before == removed.len() {
            break;
        }
    }
    library.collections.retain(|n, _| !removed.contains(n));
    library
        .collection_parents
        .retain(|n, p| !removed.contains(n) && !removed.contains(p));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{remove, save};
    use crate::characters::{Library, sample};
    #[test]
    fn rename_parent_keeps_children_and_members_and_rejects_cycles() {
        let mut l = Library::default();
        save(&mut l, None, "parent", "").unwrap();
        save(&mut l, None, "child", "parent").unwrap();
        l.collections
            .get_mut("child")
            .unwrap()
            .push("character".into());
        let before = serde_json::to_value(&l).unwrap();
        assert!(save(&mut l, Some("parent"), "parent", "child").is_err());
        assert!(save(&mut l, None, "parent", "").is_err());
        assert_eq!(before, serde_json::to_value(&l).unwrap());
        save(&mut l, Some("parent"), "renamed", "").unwrap();
        assert_eq!(l.collection_parents["child"], "renamed");
        assert_eq!(l.collections["child"], vec!["character"]);
    }
    #[test]
    fn deleting_tree_preserves_characters_and_unrelated_collections() {
        let mut l = Library::default();
        let c = sample().unwrap();
        l.characters.push(c.clone());
        save(&mut l, None, "a", "").unwrap();
        save(&mut l, None, "b", "a").unwrap();
        save(&mut l, None, "c", "").unwrap();
        l.collections.get_mut("b").unwrap().push(c.id);
        remove(&mut l, &["a".into()]).unwrap();
        assert_eq!(l.collections.keys().collect::<Vec<_>>(), vec!["c"]);
        assert!(l.collection_parents.is_empty());
        assert_eq!(l.characters.len(), 1);
    }
    #[test]
    fn old_flat_library_loads_without_parent_map() {
        let l: Library =
            serde_json::from_str(r#"{"characters":[],"collections":{"旧收藏夹":[]}}"#).unwrap();
        assert!(l.collection_parents.is_empty());
        assert!(super::accepts_characters(&l, "旧收藏夹"));
    }
}
