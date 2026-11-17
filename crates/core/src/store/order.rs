//! 排序只更新轻量索引；失败不发布内存，也不生成新的角色资料版本。
use crate::{
    characters::Library,
    library_order::{self, Order, Sort},
};
use std::path::Path;

/// 保存排序模式与方向。调用方须持有库事务锁，旧库首次写入会准备完整归档。
pub fn set_library_sort(
    path: &Path,
    library: &mut Library,
    sort: Sort,
    descending: bool,
) -> Result<Order, String> {
    let mut order = library.settings.library_order.clone();
    order.sort = sort;
    order.descending = descending;
    if sort == Sort::Custom {
        library_order::reconcile(&mut order, &library.characters);
    }
    persist(path, library, order)
}

/// 自定模式下原子移动角色，成功后返回已保存顺序；调用方须持有库事务锁。
pub fn reorder_characters(
    path: &Path,
    library: &mut Library,
    ids: &[String],
    before: Option<&str>,
) -> Result<Order, String> {
    let mut order = library.settings.library_order.clone();
    if order.sort != Sort::Custom {
        return Err("library.order_not_custom".into());
    }
    library_order::reorder(&mut order, &library.characters, ids, before)?;
    persist(path, library, order)
}

fn persist(path: &Path, library: &mut Library, order: Order) -> Result<Order, String> {
    super::ensure_writable(path)?;
    let mut index = match super::index::read(path)? {
        Some(index) => index,
        None => super::index::prepare(path, library, None)?,
    };
    index.settings.library_order = order.clone();
    super::write_index(path, &index)?
        .persist(path)
        .map_err(|e| e.to_string())?;
    library.settings.library_order = order.clone();
    Ok(order)
}

#[cfg(test)]
mod tests {
    use crate::{
        archive,
        characters::{Character, Library},
        library_order::Sort,
        store,
    };
    use std::fs;

    #[test]
    fn legacy_sort_migrates_and_reorder_only_updates_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let original = Library {
            characters: ["c", "a", "b"]
                .into_iter()
                .map(|id| Character {
                    id: id.into(),
                    name: id.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut legacy = serde_json::to_value(&original).unwrap();
        legacy["settings"]
            .as_object_mut()
            .unwrap()
            .remove("library_order");
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut library = store::load(&path).unwrap();
        assert_eq!(library.settings.library_order.sort, Sort::Added);
        store::set_library_sort(&path, &mut library, Sort::Custom, true).unwrap();
        let directory = archive::character_directory(dir.path(), "c").unwrap();
        let snapshot = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = fs::read(&snapshot).unwrap();
        let order =
            store::reorder_characters(&path, &mut library, &["c".into()], Some("a")).unwrap();
        assert_eq!(order.custom_order, ["c", "a", "b"]);
        assert_eq!(store::load(&path).unwrap(), library);
        assert_eq!(library.characters, original.characters);
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
        assert_eq!(fs::read(snapshot).unwrap(), bytes);
        store::set_library_sort(&path, &mut library, Sort::Name, false).unwrap();
        assert!(store::reorder_characters(&path, &mut library, &["a".into()], None).is_err());
        store::set_library_sort(&path, &mut library, Sort::Custom, false).unwrap();
        assert_eq!(library.settings.library_order.custom_order, ["c", "a", "b"]);
    }

    #[test]
    fn failed_write_preserves_memory_and_existing_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut library = Library {
            characters: vec![Character {
                id: "a".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        store::set_library_sort(&path, &mut library, Sort::Custom, false).unwrap();
        fs::write(&path, b"corrupt index").unwrap();
        let original = library.clone();
        assert!(store::reorder_characters(&path, &mut library, &["a".into()], None).is_err());
        assert!(store::set_library_sort(&path, &mut library, Sort::Name, true).is_err());
        assert_eq!(library, original);
        assert_eq!(fs::read(&path).unwrap(), b"corrupt index");
    }

    #[cfg(windows)]
    #[test]
    fn locked_index_can_retry_and_pending_deletion_blocks_reordering() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let mut library = Library {
            characters: ["a", "b"]
                .into_iter()
                .map(|id| Character {
                    id: id.into(),
                    name: id.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        store::set_library_sort(&path, &mut library, Sort::Custom, false).unwrap();
        let original = library.clone();
        let bytes = fs::read(&path).unwrap();
        // Windows 允许读取但拒绝替换，复现同步软件或其他进程短暂占用库文件。
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(store::reorder_characters(&path, &mut library, &["b".into()], Some("a")).is_err());
        assert_eq!(library, original);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(lock);
        store::reorder_characters(&path, &mut library, &["b".into()], Some("a")).unwrap();
        assert_eq!(library.settings.library_order.custom_order, ["b", "a"]);
        let saved = library.clone();
        fs::write(
            dir.path().join(".pending-character-deletion.json"),
            br#"["a"]"#,
        )
        .unwrap();
        assert!(store::reorder_characters(&path, &mut library, &["a".into()], Some("b")).is_err());
        assert!(store::set_library_sort(&path, &mut library, Sort::Added, true).is_err());
        assert_eq!(library, saved);
    }
}
