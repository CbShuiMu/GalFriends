use crate::{
    archive::character_directory,
    characters::{Library, Source},
    library_ops::{BatchOperation, apply_batch},
    store,
};
use std::{fs, path::Path};

fn remove(library: &Library, ids: &[String]) -> Library {
    let mut next = library.clone();
    apply_batch(&mut next, BatchOperation::Remove, ids, "").unwrap();
    next
}

fn fixture(path: &Path) -> Library {
    let mut library = store::load(path).unwrap();
    library.characters[0].id = "../../outside:role".into();
    library.characters[0].artworks[0].enhanced = Some("data:image/png;base64,private".into());
    let mut retained = library.characters[0].clone();
    retained.id = "retained".into();
    retained.artworks[0].enhanced = None;
    library.characters.push(retained);
    library.collections.insert(
        "folder".into(),
        library.characters.iter().map(|c| c.id.clone()).collect(),
    );
    store::save(path, &library).unwrap();
    library.characters[0].favorite = true;
    store::save(path, &library).unwrap();
    library
}

#[test]
fn deletion_purges_all_versions_and_assets_but_keeps_other_characters_and_external_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("characters");
    let path = root.join("library.json");
    let mut library = fixture(&path);
    let mut second = library.characters[0].clone();
    second.id = "second".into();
    library.characters.push(second);
    store::save(&path, &library).unwrap();
    let ids = vec![library.characters[0].id.clone(), "second".into()];
    let target = character_directory(&root, &ids[0]).unwrap();
    assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
    fs::create_dir(target.join("images")).unwrap();
    fs::write(target.join("images/original.png"), b"original").unwrap();
    fs::write(target.join("images/enhanced.png"), b"enhanced").unwrap();
    let outside = dir.path().join("original.png");
    fs::write(&outside, b"external source").unwrap();
    let retained = character_directory(&root, "retained").unwrap();
    let retained_file = fs::read_dir(&retained)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let retained_bytes = fs::read(&retained_file).unwrap();
    let next = remove(&library, &ids);
    store::save_changes(&path, &library, &next).unwrap();
    for id in &ids {
        assert!(!character_directory(&root, id).unwrap().exists());
    }
    let restored = store::load(&path).unwrap();
    assert_eq!(restored.characters.len(), 1);
    assert_eq!(restored.characters[0].id, "retained");
    assert_eq!(restored.collections["folder"], ["retained"]);
    assert_eq!(fs::read(&retained_file).unwrap(), retained_bytes);
    assert_eq!(fs::read(&outside).unwrap(), b"external source");
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("base64,private")
    );
    assert!(super::pending(&path).unwrap().is_none());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
}

#[test]
fn failed_validation_keeps_archives_and_current_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let library = fixture(&path);
    let before = fs::read(&path).unwrap();
    let mut next = remove(&library, &[library.characters[0].id.clone()]);
    next.settings.tag_source = Source::Bangumi;
    assert!(store::save_changes(&path, &library, &next).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        fs::read_dir(character_directory(dir.path(), &library.characters[0].id).unwrap())
            .unwrap()
            .count(),
        2
    );
    assert!(super::pending(&path).unwrap().is_none());
}

#[test]
fn deleting_last_character_stays_empty_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let library = store::load(&path).unwrap();
    store::save(&path, &library).unwrap();
    let next = remove(&library, &[library.characters[0].id.clone()]);
    store::save_changes(&path, &library, &next).unwrap();
    assert!(store::load(&path).unwrap().characters.is_empty());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn locked_archive_reports_failure_and_same_deletion_can_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let library = fixture(&path);
    let id = library.characters[0].id.clone();
    let folder = character_directory(dir.path(), &id).unwrap();
    let record = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(record)
        .unwrap();
    let next = remove(&library, &[id]);
    let before = fs::read(&path).unwrap();
    assert!(store::save_changes(&path, &library, &next).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(store::save(&path, &library).is_err());
    assert!(store::load(&path).is_err());
    drop(lock);
    store::save_changes(&path, &library, &next).unwrap();
    assert!(!folder.exists());
    assert_eq!(store::load(&path).unwrap().characters.len(), 1);
}

#[cfg(windows)]
#[test]
fn failed_library_replace_recovers_on_load_without_recreating_deleted_archives() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let library = fixture(&path);
    let id = library.characters[0].id.clone();
    let next = remove(&library, std::slice::from_ref(&id));
    let before = fs::read(&path).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(store::save_changes(&path, &library, &next).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!character_directory(dir.path(), &id).unwrap().exists());
    drop(lock);
    let recovered = store::load(&path).unwrap();
    assert_eq!(recovered.characters.len(), 1);
    assert_eq!(recovered.collections["folder"], ["retained"]);
    assert!(!character_directory(dir.path(), &id).unwrap().exists());
    assert!(super::pending(&path).unwrap().is_none());
}

#[cfg(windows)]
#[test]
fn junction_inside_archive_rejects_whole_batch_before_deleting_anything() {
    use std::os::windows::process::CommandExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("characters");
    let path = root.join("library.json");
    let library = fixture(&path);
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("original.png"), b"keep").unwrap();
    let link = character_directory(&root, "retained")
        .unwrap()
        .join("images");
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:GALFRIENDS_TEST_LINK -Target $env:GALFRIENDS_TEST_TARGET | Out-Null"])
        .env("GALFRIENDS_TEST_LINK", &link).env("GALFRIENDS_TEST_TARGET", &outside)
        .creation_flags(0x08000000).status().unwrap();
    assert!(status.success());
    let ids: Vec<_> = library.characters.iter().map(|c| c.id.clone()).collect();
    let before = fs::read(&path).unwrap();
    assert!(store::save_changes(&path, &library, &remove(&library, &ids)).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        fs::read_dir(character_directory(&root, &ids[0]).unwrap())
            .unwrap()
            .count(),
        2
    );
    assert_eq!(fs::read(outside.join("original.png")).unwrap(), b"keep");
    assert!(super::pending(&path).unwrap().is_none());
    fs::remove_dir(link).unwrap();
}

#[test]
fn incomplete_deletion_never_falls_back_to_sample_or_legacy_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    let legacy = dir.path().join("legacy.json");
    fs::write(super::journal_path(&path).unwrap(), br#"["deleted"]"#).unwrap();
    fs::write(&legacy, serde_json::to_vec(&Library::default()).unwrap()).unwrap();
    assert!(store::load_for_migration(&path, &legacy).is_err());
    assert!(!path.exists());
}
