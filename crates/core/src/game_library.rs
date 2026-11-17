//! GalGear 只读目录与角色作品的关联。作品身份来自鲲 Gal 的外部 ID 与封面摘要。
#[cfg(feature = "desktop")]
mod storage;
use crate::characters::{Character, Source};
use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
pub use storage::{load, validate_work_folder};

/// 训练页选项中区分手动目录和 GalGear 安装 ID 的前缀。
pub const LOCAL_GAME_PREFIX: &str = "local:";

/// 返回仍属于该角色出演作品的手动目录；旧快照关系失效时不再提供给训练页。
pub fn work_folder<'a>(character: &'a Character, work_id: &str) -> Option<&'a str> {
    character
        .work_folders
        .get(work_id)
        .filter(|_| works(character).iter().any(|work| work.id == work_id))
        .map(String::as_str)
}

/// 保存关联前确认作品仍然属于此角色。
pub fn set_work_folder(
    character: &mut Character,
    work_id: String,
    folder: String,
) -> Result<(), String> {
    if !works(character).iter().any(|work| work.id == work_id) {
        return Err("game_library.changed".into());
    }
    character.work_folders.insert(work_id, folder);
    Ok(())
}

/// 来源快照中的出演作品；不改写原始响应。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub source: Source,
    pub id: String,
    pub name: String,
    pub translated_name: String,
    pub cover_url: String,
    pub cover_hashes: Vec<String>,
    pub site_id: String,
    pub identities: Vec<Identity>,
    pub kind: u64,
}

/// 兼容旧归档中的手动关系。当前自动关联不使用此字段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub catalog_path: String,
    pub source: Source,
    pub work_id: String,
    pub game_id: String,
}

/// 游戏条目明确记录的外部作品身份。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub source: Source,
    pub id: String,
}

/// GalGear 库的轻量只读投影，不携带凭据或游戏启动配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub available: bool,
    pub identities: Vec<Identity>,
    #[serde(default)]
    pub cover_hash: String,
}

/// 一次完整读取的目录快照。空配置与读取失败由命令层分别处理。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub path: String,
    pub games: Vec<Game>,
}

/// 从鲲 Gal 出演快照投影作品，保持完整响应与剧透标记独立保存。
pub fn works(character: &Character) -> Vec<Work> {
    let mut works = Vec::new();
    for snapshot in character
        .snapshots
        .iter()
        .filter(|s| s.source == Source::Kungal)
    {
        for appearance in snapshot.data["appearances"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if appearance["spoiler"].as_str().is_some_and(|s| s != "none") {
                continue;
            }
            let value = snapshot.data["work_details"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|work| work["id"] == appearance["work"]["id"])
                .unwrap_or(&appearance["work"]);
            let Some(id) = value["id"].as_str().filter(|id| numeric_id(id)) else {
                continue;
            };
            if works.iter().any(|w: &Work| w.id == id) {
                continue;
            }
            let mut identities = vec![Identity {
                source: Source::Kungal,
                id: id.into(),
            }];
            for reference in value["refs"].as_array().into_iter().flatten() {
                let source = match reference["source"].as_str() {
                    Some("bangumi") => Source::Bangumi,
                    Some("vndb") => Source::Vndb,
                    _ => continue,
                };
                if let Some(id) = reference["external_id"].as_str() {
                    let valid = if source == Source::Vndb {
                        id.strip_prefix('v').is_some_and(numeric_id)
                    } else {
                        numeric_id(id)
                    };
                    if valid {
                        identities.push(Identity {
                            source,
                            id: id.into(),
                        });
                    }
                }
            }
            let site_id = value["claim"]["site_work_id"]
                .as_str()
                .filter(|id| value["claim"]["site"] == "kungal" && numeric_id(id))
                .unwrap_or("");
            let cover_url = value["cover"]["url"]
                .as_str()
                .filter(|url| cover_url_allowed(url))
                .unwrap_or("");
            works.push(Work {
                source: Source::Kungal,
                id: id.into(),
                name: value["display_name"].as_str().unwrap_or("").into(),
                translated_name: value["localized"]["zh-Hans"]["value"]
                    .as_str()
                    .unwrap_or("")
                    .into(),
                cover_url: cover_url.into(),
                cover_hashes: std::iter::once(&value["cover"])
                    .chain(value["covers"].as_array().into_iter().flatten())
                    .filter_map(|image| image["hash"].as_str().filter(|s| valid_hash(s)))
                    .map(str::to_ascii_lowercase)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                site_id: site_id.into(),
                identities,
                kind: if value["medium"] == "galgame" { 4 } else { 0 },
            });
        }
    }
    // 共享封面不能单独证明作品身份，清除该匹配依据，来源 ID 仍可正常关联。
    let mut uses = std::collections::BTreeMap::new();
    for work in &works {
        for hash in &work.cover_hashes {
            *uses.entry(hash.clone()).or_insert(0) += 1;
        }
    }
    for work in &mut works {
        work.cover_hashes.retain(|hash| uses[hash] == 1);
    }
    works
}

/// 只接受非零十进制目录 ID，供固定站点导航和作品关系共用。
pub fn numeric_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 20
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|n| n > 0)
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())
}

/// 只向已接入的鲲 Gal 图片主机加载封面。
pub fn cover_url_allowed(url: &str) -> bool {
    url.strip_prefix("https://image.kungal.iloveren.link/")
        .is_some_and(|path| !path.is_empty() && !path.contains(['\\', '<', '>', '\"']))
}

/// 以来源身份或原始封面 SHA-256 精确匹配，多个作品共用封面时由调用方排除歧义。
fn matches(work: &Work, game: &Game) -> bool {
    work.kind == 4
        && (game
            .identities
            .iter()
            .any(|id| work.identities.contains(id))
            || (!game.cover_hash.is_empty() && work.cover_hashes.contains(&game.cover_hash)))
}

/// 一个作品的默认安装目录，优先选择可用目录，库中顺序相同时保持稳定。
pub fn work_installation<'a>(work: &Work, catalog: &'a Catalog) -> Option<&'a Game> {
    catalog
        .games
        .iter()
        .filter(|game| matches(work, game))
        .min_by_key(|game| !game.available)
}

/// 当前角色全部出演游戏的已入库安装条目，每个安装只返回一次。
pub fn linked_games<'a>(character: &Character, catalog: &'a Catalog) -> Vec<&'a Game> {
    let works = works(character);
    catalog
        .games
        .iter()
        .filter(|game| works.iter().any(|work| matches(work, game)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Catalog, Game, Identity, Link, cover_url_allowed, linked_games, matches, works};
    use crate::characters::{Character, Snapshot, Source};
    #[test]
    fn kungal_appearances_match_exact_ids_and_cover_hashes_only() {
        let hash = "a".repeat(64);
        let character = Character {
            snapshots: vec![Snapshot {
                source: Source::Kungal,
                id: "9".into(),
                url: String::new(),
                data: serde_json::json!({"appearances":[{"spoiler":"none","work":{
                    "id":"2514","medium":"galgame","display_name":"Title",
                    "cover":{"hash":hash,"url":"https://image.kungal.iloveren.link/cover.webp"},
                    "claim":{"site":"kungal","site_work_id":"2514"},
                    "refs":[{"source":"bangumi","external_id":"536007"}]}}]}),
            }],
            game_links: vec![Link {
                catalog_path: "catalog".into(),
                source: Source::Bangumi,
                work_id: "536007".into(),
                game_id: "homonym".into(),
            }],
            ..Default::default()
        };
        let mut game = Game {
            id: "by-id".into(),
            title: "Different title".into(),
            folder: String::new(),
            available: true,
            identities: vec![Identity {
                source: Source::Bangumi,
                id: "536007".into(),
            }],
            cover_hash: String::new(),
        };
        let mut catalog = Catalog {
            path: "catalog".into(),
            games: vec![game.clone()],
        };
        game.id = "by-cover".into();
        game.identities.clear();
        game.cover_hash = hash;
        catalog.games.push(game.clone());
        game.id = "homonym".into();
        game.title = "Title".into();
        game.cover_hash.clear();
        catalog.games.push(game);
        assert_eq!(linked_games(&character, &catalog).len(), 2);
        assert_eq!(works(&character)[0].site_id, "2514");
        let mut bangumi_only = character;
        bangumi_only.snapshots[0].source = Source::Bangumi;
        assert!(works(&bangumi_only).is_empty());
    }

    #[test]
    fn enriched_work_ids_resolve_shared_covers_without_guessing() {
        let hash = "c".repeat(64);
        let character = Character {
            snapshots: vec![Snapshot {
                source: Source::Kungal,
                id: "9".into(),
                url: String::new(),
                data: serde_json::json!({
                    "appearances":[{"work":{"id":"1"}},{"work":{"id":"2"}},
                        {"spoiler":"major","work":{"id":"3","medium":"galgame"}}],
                    "work_details":[
                        {"id":"1","medium":"galgame","display_name":"First", "covers":[{"hash":hash}]},
                        {"id":"2","medium":"galgame","display_name":"Second", "cover":{"hash":hash}}
                    ]
                }),
            }],
            ..Default::default()
        };
        let works = works(&character);
        assert_eq!(works.len(), 2);
        assert_eq!(works[0].name, "First");
        let mut game = Game {
            id: "game".into(),
            title: "First".into(),
            folder: String::new(),
            available: true,
            identities: Vec::new(),
            cover_hash: hash,
        };
        assert!(!works.iter().any(|work| matches(work, &game)));
        game.identities.push(Identity {
            source: Source::Kungal,
            id: "2".into(),
        });
        assert!(!matches(&works[0], &game));
        assert!(matches(&works[1], &game));
        assert!(!cover_url_allowed(
            "https://image.kungal.iloveren.link.evil.test/a.webp"
        ));
    }
}
