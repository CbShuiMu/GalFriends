use crate::{
    archive,
    characters::{Artwork, Character, Library},
    store,
};
use std::fs;

fn fixture() -> Library {
    Library {
        characters: vec![Character {
            id: "bangumi:1".into(),
            artworks: vec![
                Artwork {
                    url: "x".repeat(1_000_000),
                    enhanced: Some("y".repeat(1_000_000)),
                    ..Default::default()
                },
                Artwork {
                    url: "second.png".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn legacy_library_migrates_losslessly_and_selection_only_changes_small_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let original = fixture();
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let mut library = store::load(&path).unwrap();
    assert_eq!(library, original);
    store::select_artwork(&path, &mut library, "bangumi:1", 1).unwrap();
    assert!(fs::metadata(&path).unwrap().len() < 2000);
    assert_eq!(store::load(&path).unwrap(), library);
    let folder = archive::character_directory(dir.path(), "bangumi:1");
    let snapshot = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = fs::read(&snapshot).unwrap();
    assert_eq!(
        serde_json::from_slice::<Character>(&bytes).unwrap(),
        original.characters[0]
    );
    store::select_artwork(&path, &mut library, "bangumi:1", 0).unwrap();
    assert_eq!(fs::read_dir(folder).unwrap().count(), 1);
    assert_eq!(fs::read(snapshot).unwrap(), bytes);
    assert_eq!(store::load(&path).unwrap(), original);
}

#[test]
fn later_full_save_preserves_selection_and_other_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    store::save(&path, &library).unwrap();
    store::select_artwork(&path, &mut library, "bangumi:1", 1).unwrap();
    let mut next = library.clone();
    next.characters[0].favorite = true;
    next.settings.theme = "dark".into();
    store::save_changes(&path, &library, &next).unwrap();
    assert_eq!(store::load(&path).unwrap(), next);
}

#[test]
fn favorites_migrate_legacy_data_and_keep_profiles_through_later_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    library.characters[0].favorite = true;
    fs::write(&path, serde_json::to_vec(&library).unwrap()).unwrap();
    let ids = ["bangumi:1".to_owned()];
    store::set_favorites(&path, &mut library, &ids, false).unwrap();
    assert_eq!(store::load(&path).unwrap(), library);
    let folder = archive::character_directory(dir.path(), &ids[0]);
    let snapshot = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = fs::read(&snapshot).unwrap();
    assert!(
        serde_json::from_slice::<Character>(&bytes)
            .unwrap()
            .favorite
    );

    // 模拟现有 v2 索引：缺少字段必须恢复档案中的 true。
    let mut index: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    index["characters"][0]
        .as_object_mut()
        .unwrap()
        .remove("favorite");
    fs::write(&path, serde_json::to_vec(&index).unwrap()).unwrap();
    library = store::load(&path).unwrap();
    assert!(library.characters[0].favorite);
    for favorite in [false, true, true, false] {
        let update = store::set_favorites(&path, &mut library, &ids, favorite).unwrap();
        assert_eq!(
            serde_json::to_value(update).unwrap(),
            serde_json::json!({
                "ids": ["bangumi:1"], "favorite": favorite
            })
        );
        assert_eq!(store::load(&path).unwrap(), library);
    }
    store::select_artwork(&path, &mut library, &ids[0], 1).unwrap();
    let mut next = library.clone();
    next.settings.font_size = 18;
    store::save_changes(&path, &library, &next).unwrap();
    assert_eq!(store::load(&path).unwrap(), next);
    assert_eq!(fs::read_dir(folder).unwrap().count(), 1);
    assert_eq!(fs::read(snapshot).unwrap(), bytes);
    assert!(fs::metadata(path).unwrap().len() < 2000);
}

#[test]
fn batch_favorites_validate_all_targets_and_pending_deletion_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    library.characters.push(Character {
        id: "bangumi:2".into(),
        ..Default::default()
    });
    store::save(&path, &library).unwrap();
    let before = fs::read(&path).unwrap();
    for ids in [vec![], vec!["bangumi:1".into(), "missing".into()]] {
        assert!(store::set_favorites(&path, &mut library, &ids, true).is_err());
        assert!(library.characters.iter().all(|c| !c.favorite));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    let ids = ["bangumi:1".into(), "bangumi:2".into(), "bangumi:1".into()];
    let update = store::set_favorites(&path, &mut library, &ids, true).unwrap();
    assert_eq!(update.ids, ids[..2]);
    assert!(
        store::load(&path)
            .unwrap()
            .characters
            .iter()
            .all(|c| c.favorite)
    );
    fs::write(
        dir.path().join(".pending-character-deletion.json"),
        br#"["bangumi:1"]"#,
    )
    .unwrap();
    let before = fs::read(&path).unwrap();
    assert!(store::set_favorites(&path, &mut library, &ids, false).is_err());
    assert!(library.characters.iter().all(|c| c.favorite));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn favorites_never_access_profiles_and_failed_commit_preserves_state_for_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    store::save(&path, &library).unwrap();
    let folder = archive::character_directory(dir.path(), "bangumi:1");
    let snapshot = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = fs::read(&snapshot).unwrap();
    let profile_lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&snapshot)
        .unwrap();
    let ids = ["bangumi:1".into()];
    store::set_favorites(&path, &mut library, &ids, true).unwrap();
    let before = fs::read(&path).unwrap();
    let index_lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(store::set_favorites(&path, &mut library, &ids, false).is_err());
    assert!(library.characters[0].favorite);
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(index_lock);
    store::set_favorites(&path, &mut library, &ids, false).unwrap();
    drop(profile_lock);
    assert_eq!(fs::read_dir(folder).unwrap().count(), 1);
    assert_eq!(fs::read(snapshot).unwrap(), bytes);
    assert_eq!(store::load(&path).unwrap(), library);
}

#[test]
fn appearance_migrates_legacy_library_and_returns_only_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    library.characters[0].favorite = true;
    library.characters[0].selected_art = 1;
    library
        .collections
        .insert("favorites".into(), vec!["bangumi:1".into()]);
    fs::write(&path, serde_json::to_vec(&library).unwrap()).unwrap();
    let appearance = store::set_appearance(&path, &mut library, "theme", "dark").unwrap();
    assert_eq!(
        serde_json::to_value(appearance).unwrap(),
        serde_json::json!({
            "theme": "dark", "color_theme": "pink"
        })
    );
    assert!(fs::metadata(&path).unwrap().len() < 2000);
    assert_eq!(store::load(&path).unwrap(), library);
    store::set_appearance(&path, &mut library, "color_theme", "white-pink").unwrap();
    assert_eq!(store::load(&path).unwrap(), library);
    assert_eq!(
        fs::read_dir(archive::character_directory(dir.path(), "bangumi:1"))
            .unwrap()
            .count(),
        1
    );
    let before = fs::read(&path).unwrap();
    for (key, value) in [
        ("theme", "invalid"),
        ("color_theme", "invalid"),
        ("language", "en-US"),
    ] {
        assert!(store::set_appearance(&path, &mut library, key, value).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    fs::write(
        dir.path().join(".pending-character-deletion.json"),
        br#"["bangumi:1"]"#,
    )
    .unwrap();
    assert!(store::set_appearance(&path, &mut library, "theme", "light").is_err());
    assert_eq!(library.settings.theme, "dark");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn appearance_commit_failure_preserves_memory_and_disk_and_can_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    store::save(&path, &library).unwrap();
    let before = fs::read(&path).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(store::set_appearance(&path, &mut library, "theme", "dark").is_err());
    assert_eq!(library.settings.theme, "light");
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(lock);
    store::set_appearance(&path, &mut library, "theme", "dark").unwrap();
    assert_eq!(store::load(&path).unwrap().settings.theme, "dark");
}

#[test]
fn missing_corrupt_or_unsafe_profile_is_reported_without_resetting_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    store::save(&path, &fixture()).unwrap();
    let original = fs::read(&path).unwrap();
    let folder = archive::character_directory(dir.path(), "bangumi:1");
    let snapshot = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&snapshot, b"corrupt").unwrap();
    assert!(store::load(&path).is_err());
    fs::remove_file(snapshot).unwrap();
    assert!(store::load(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["characters"][0]["profile"] = "../../outside".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(store::load(&path).is_err());
    value["format_version"] = 99.into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(store::load(&path).is_err());
}

#[cfg(windows)]
#[test]
fn selection_and_settings_save_do_not_read_or_rewrite_unchanged_profiles() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    store::save(&path, &library).unwrap();
    let folder = archive::character_directory(dir.path(), "bangumi:1");
    let snapshot = fs::read_dir(folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(snapshot)
        .unwrap();
    store::select_artwork(&path, &mut library, "bangumi:1", 1).unwrap();
    let mut next = library.clone();
    next.settings.theme = "dark".into();
    store::save_changes(&path, &library, &next).unwrap();
    store::set_appearance(&path, &mut next, "theme", "light").unwrap();
    drop(lock);
    assert_eq!(store::load(&path).unwrap(), next);
}

#[cfg(windows)]
#[test]
fn failed_selection_commit_keeps_memory_and_disk_and_can_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let mut library = fixture();
    store::save(&path, &library).unwrap();
    let before = fs::read(&path).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(store::select_artwork(&path, &mut library, "bangumi:1", 1).is_err());
    assert_eq!(library.characters[0].selected_art, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(lock);
    store::select_artwork(&path, &mut library, "bangumi:1", 1).unwrap();
    assert_eq!(store::load(&path).unwrap().characters[0].selected_art, 1);
    assert!(store::select_artwork(&path, &mut library, "bangumi:1", 999).is_err());
    fs::write(
        dir.path().join(".pending-character-deletion.json"),
        br#"["bangumi:1"]"#,
    )
    .unwrap();
    assert!(store::select_artwork(&path, &mut library, "bangumi:1", 0).is_err());
}
