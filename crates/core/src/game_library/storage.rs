//! 仅打开用户选定库中的 catalog.json 和 settings.json，不写入 GalGear。
use super::{Catalog, Game, Identity};
use crate::characters::Source;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// 仅接受存在的绝对目录。逐级检查，防止通过上级链接跳出所选路径。
pub fn validate_work_folder(path: &Path) -> Result<&str, String> {
    let invalid = || "game_library.invalid_folder".to_owned();
    if !path.is_absolute()
        || path
            .components()
            .any(|part| part == std::path::Component::ParentDir)
    {
        return Err(invalid());
    }
    for entry in path.ancestors().filter(|entry| entry.is_absolute()) {
        crate::archive::check_entry(entry).map_err(|_| invalid())?;
    }
    if !path.is_dir() {
        return Err(invalid());
    }
    path.to_str().ok_or_else(invalid)
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Metadata {
    url: String,
    links: Vec<MetadataLink>,
    cover_url: Option<String>,
}
#[derive(Deserialize)]
struct MetadataLink {
    url: String,
}
#[derive(Deserialize)]
struct Entry {
    id: String,
    title: String,
    folder: String,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    metadata: Option<Metadata>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Edit {
    title: Option<String>,
    metadata: Option<Metadata>,
    tag_source_url: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Settings {
    manual_games: Vec<Entry>,
    cover_overrides: BTreeMap<String, String>,
    excluded_games: Vec<String>,
    excluded_paths: Vec<String>,
    excluded_entries: BTreeMap<String, Entry>,
    game_edits: BTreeMap<String, Edit>,
}
#[derive(Deserialize)]
struct Entries {
    games: Vec<Entry>,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("GalGear library exceeds 64 MiB".into());
    }
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn catalog_path(input: &Path) -> Result<PathBuf, String> {
    let path = if input.is_dir() {
        if input.join("portable.flag").is_file() {
            input.join("data/catalog.json")
        } else {
            input.join("catalog.json")
        }
    } else {
        input.to_owned()
    };
    if path
        .file_name()
        .is_none_or(|name| !name.eq_ignore_ascii_case("catalog.json"))
    {
        return Err("Select GalGear catalog.json or its data directory".into());
    }
    path.canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn identity(url: &str) -> Option<Identity> {
    let url = reqwest::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let (source, id) = match url.host_str()? {
        "bgm.tv" | "bangumi.tv" | "chii.in" => (Source::Bangumi, path.strip_prefix("/subject/")?),
        "vndb.org" | "www.vndb.org" => (Source::Vndb, path.strip_prefix('/')?),
        "developer.nextmoe.dev" => (Source::Kungal, path.strip_prefix("/explore/work/")?),
        "api.nextmoe.dev" => (Source::Kungal, path.strip_prefix("/v2/catalog/works/")?),
        "www.kungal.com" | "kungal.com" => (Source::Kungal, path.strip_prefix("/galgame/")?),
        _ => return None,
    };
    let digits = if source == Source::Vndb {
        id.strip_prefix('v')?
    } else {
        id
    };
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.parse::<u64>().ok()? == 0
    {
        return None;
    }
    Some(Identity {
        source,
        id: if source == Source::Vndb {
            format!("v{}", digits.parse::<u64>().ok()?)
        } else {
            digits.parse::<u64>().ok()?.to_string()
        },
    })
}

/// 从安装版数据目录、便携目录或 catalog.json 读取当前可见游戏。
/// 文件损坏时报错，绝不回写或把错误转换为空库。
pub fn load(input: &Path) -> Result<Catalog, String> {
    let path = catalog_path(input)?;
    let settings_path = path.with_file_name("settings.json");
    let settings: Settings = if settings_path.try_exists().map_err(|e| e.to_string())? {
        read(&settings_path)?
    } else {
        Settings::default()
    };
    let mut entries: Entries = read(&path)?;
    for entry in settings.manual_games {
        if !entries.games.iter().any(|game| game.id == entry.id) {
            entries.games.push(entry);
        }
    }
    let mut games = Vec::new();
    for entry in entries.games {
        if entry.id.is_empty() || games.iter().any(|game: &Game| game.id == entry.id) {
            return Err("Invalid or duplicate GalGear game ID".into());
        }
        if settings.excluded_games.contains(&entry.id)
            || settings
                .excluded_paths
                .iter()
                .any(|folder| folder.eq_ignore_ascii_case(&entry.folder))
            || settings.excluded_entries.values().any(|excluded| {
                settings.excluded_games.contains(&excluded.id)
                    && (excluded.folder.eq_ignore_ascii_case(&entry.folder)
                        || excluded.executable.as_ref().is_some_and(|exe| {
                            entry
                                .executable
                                .as_ref()
                                .is_some_and(|path| path.eq_ignore_ascii_case(exe))
                        }))
            })
        {
            continue;
        }
        let edit = settings.game_edits.get(&entry.id);
        let metadata = edit
            .and_then(|edit| edit.metadata.as_ref())
            .or(entry.metadata.as_ref());
        let mut game = project(&entry, metadata, edit);
        if let Some(path) = settings.cover_overrides.get(&entry.id) {
            // GalGear 选封面后保存 source-<原始下载内容 SHA-256>.jpg，升级图片不改变此记录。
            if let Some(hash) = path
                .rsplit(['/', '\\'])
                .next()
                .and_then(|name| name.strip_prefix("source-"))
                .and_then(|name| name.strip_suffix(".jpg"))
                && super::valid_hash(hash)
            {
                game.cover_hash = hash.to_ascii_lowercase();
            }
        }
        games.push(game);
    }
    Ok(Catalog {
        path: path.to_string_lossy().into_owned(),
        games,
    })
}

fn project(entry: &Entry, metadata: Option<&Metadata>, edit: Option<&Edit>) -> Game {
    let mut identities = Vec::new();
    if let Some(metadata) = metadata {
        for url in std::iter::once(metadata.url.as_str())
            .chain(metadata.links.iter().map(|link| link.url.as_str()))
        {
            if let Some(identity) = identity(url)
                && !identities.contains(&identity)
            {
                identities.push(identity);
            }
        }
    }
    if let Some(identity) = edit
        .and_then(|edit| edit.tag_source_url.as_deref())
        .and_then(identity)
        && !identities.contains(&identity)
    {
        identities.push(identity);
    }
    let cover_hash = metadata
        .and_then(|m| m.cover_url.as_deref())
        .filter(|url| super::cover_url_allowed(url))
        .and_then(|url| reqwest::Url::parse(url).ok())
        .and_then(|url| {
            Path::new(url.path())
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| super::valid_hash(s))
        .unwrap_or_default()
        .to_ascii_lowercase();
    Game {
        cover_hash,
        id: entry.id.clone(),
        title: edit
            .and_then(|edit| edit.title.as_ref())
            .unwrap_or(&entry.title)
            .clone(),
        folder: entry.folder.clone(),
        available: Path::new(&entry.folder).is_absolute() && Path::new(&entry.folder).is_dir(),
        identities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_folder_survives_library_reload_and_rejects_missing_paths() {
        use crate::characters::{Character, Library, Snapshot};
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Game Directory");
        std::fs::create_dir(&folder).unwrap();
        let folder = validate_work_folder(&folder).unwrap().to_owned();
        let mut character = Character {
            id: "kungal:9".into(),
            snapshots: vec![Snapshot {
                source: Source::Kungal,
                id: "9".into(),
                url: String::new(),
                data: serde_json::json!({"appearances":[{"work":{"id":"42","medium":"galgame"}}]}),
            }],
            ..Default::default()
        };
        assert!(
            super::super::set_work_folder(&mut character, "43".into(), folder.clone()).is_err()
        );
        super::super::set_work_folder(&mut character, "42".into(), folder.clone()).unwrap();
        let library_path = root.path().join("characters/library.json");
        let library = Library {
            characters: vec![character],
            ..Default::default()
        };
        crate::store::save(&library_path, &library).unwrap();
        let reloaded = crate::store::load(&library_path).unwrap();
        assert_eq!(
            super::super::work_folder(&reloaded.characters[0], "42"),
            Some(folder.as_str())
        );
        assert!(validate_work_folder(&root.path().join("missing")).is_err());
        assert!(
            validate_work_folder(&root.path().join("Game Directory/../Game Directory")).is_err()
        );
    }

    #[test]
    fn portable_and_installed_read_the_same_format_and_honor_saved_edits() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("data")).unwrap();
        std::fs::write(root.path().join("portable.flag"), "").unwrap();
        let data = root.path().join("data");
        std::fs::write(data.join("catalog.json"), r#"{"games":[{"id":"a","title":"Same title","folder":"missing","metadata":{"url":"https://bgm.tv/subject/1"}},{"id":"gone","title":"Removed","folder":"gone"}]}"#).unwrap();
        std::fs::write(data.join("settings.json"), r#"{"excluded_games":["gone"],"game_edits":{"a":{"metadata":{"url":"https://bangumi.tv/subject/2"}}},"manual_games":[{"id":"manual","title":"Added","folder":"missing"}]}"#).unwrap();
        let portable = load(root.path()).unwrap();
        let installed = load(&data.join("catalog.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&portable).unwrap(),
            serde_json::to_value(installed).unwrap()
        );
        assert_eq!(portable.games.len(), 2);
        assert_eq!(portable.games[0].identities[0].id, "2");
        assert!(!portable.games[0].available);
        std::fs::write(data.join("settings.json"), "broken").unwrap();
        assert!(load(&data).is_err());
    }
    #[test]
    fn urls_require_exact_source_hosts_and_subject_paths() {
        assert!(identity("https://bgm.tv.evil.test/subject/1").is_none());
        assert!(identity("https://bgm.tv/character/1").is_none());
        assert!(identity("https://bgm.tv/subject/1/extra").is_none());
        assert_eq!(identity("https://vndb.org/v17").unwrap().id, "v17");
        assert_eq!(
            identity("https://developer.nextmoe.dev/explore/work/2514?nsfw=1")
                .unwrap()
                .source,
            Source::Kungal
        );
        assert_eq!(
            identity("https://www.kungal.com/galgame/2514").unwrap().id,
            "2514"
        );
        assert!(identity("https://www.kungal.com.evil.test/galgame/2514").is_none());
        assert!(identity("https://developer.nextmoe.dev/explore/character/2514").is_none());
    }

    #[test]
    fn reads_independent_tag_and_content_addressed_cover_selection() {
        let root = tempfile::tempdir().unwrap();
        let hash = "b".repeat(64);
        std::fs::write(root.path().join("catalog.json"), r#"{"games":[{"id":"game","title":"title","folder":"missing","metadata":{"url":"https://bgm.tv/subject/1"}}]}"#).unwrap();
        let settings = serde_json::json!({"game_edits":{"game":{"tag_source_url":"https://developer.nextmoe.dev/explore/work/2514?nsfw=1"}},
            "cover_overrides":{"game":format!("C:\\GalGear\\covers\\source-{hash}.jpg")}});
        std::fs::write(root.path().join("settings.json"), settings.to_string()).unwrap();
        let game = &load(root.path()).unwrap().games[0];
        assert_eq!(game.cover_hash, hash);
        assert!(game.identities.contains(&Identity {
            source: Source::Kungal,
            id: "2514".into()
        }));
    }
}
