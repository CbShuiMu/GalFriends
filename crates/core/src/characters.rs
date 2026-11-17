//! 展示字段是来源快照的投影；跨源关联使用明确外部 ID 或用户选择。
mod expressions;
mod import_progress;
pub use expressions::{ExpressionChoice, ExpressionPreview, ExpressionScan, ExpressionVariant};
pub use import_progress::ImportProgress;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 允许使用的数据源。批评空间只提供作品资料。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Bangumi,
    Kungal,
    Vndb,
    Erogamescape,
}
impl Source {
    /// 稳定协议标识。
    pub fn key(self) -> &'static str {
        match self {
            Self::Bangumi => "bangumi",
            Self::Kungal => "kungal",
            Self::Vndb => "vndb",
            Self::Erogamescape => "erogamescape",
        }
    }
}
/// 保留别名种类，不按规范化后的姓名删除不同种类别名。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Alias {
    pub kind: String,
    pub name: String,
}
/// 标签保留分组、来源与剧透属性。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub group: String,
    pub source: Source,
    pub spoiler: bool,
    pub sexual: bool,
}
/// 原始来源保持 Bangumi URL；展示图可指向透明 PNG，超分结果独立保存。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Artwork {
    pub id: String,
    pub title: String,
    pub url: String,
    pub original: String,
    /// 下载的原始文件，以 data URL 原样归档；旧记录没有离线原图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_data: Option<String>,
    pub spoiler: bool,
    #[serde(default)]
    pub enhanced: Option<String>,
}
/// 一次来源详情和所有关联响应的完整快照。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub source: Source,
    pub id: String,
    pub url: String,
    pub data: Value,
}
/// 一个角色，不依赖游戏目录；照片列表顺序与选中立绘独立。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub aliases: Vec<Alias>,
    pub tags: Vec<Tag>,
    /// 本地标签显示调整，不覆盖 API 标签或响应快照。
    #[serde(default)]
    pub tag_presentation: crate::tags::Presentation,
    /// 手动替换的标签来源；缺省跟随设置中的自动语言规则。
    #[serde(default)]
    pub tag_source: Option<Source>,
    pub artworks: Vec<Artwork>,
    pub snapshots: Vec<Snapshot>,
    /// 旧版与 GalGear 安装条目的关联，保留归档兼容。
    #[serde(default)]
    pub game_links: Vec<crate::game_library::Link>,
    /// 按鲲 Gal 作品 ID 存放用户选择的本地目录，独立于 GalGear 库。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub work_folders: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub selected_art: usize,
    #[serde(default)]
    pub favorite: bool,
}
impl Character {
    /// 来源提供的罗马字，用于界面展示和角色资料目录命名。
    pub fn romanized_name(&self) -> Option<&str> {
        self.aliases
            .iter()
            .filter(|alias| {
                matches!(
                    alias.kind.as_str(),
                    "罗马字" | "羅馬字" | "romanized" | "latin"
                )
            })
            .map(|alias| alias.name.trim())
            .chain(
                self.snapshots
                    .iter()
                    .filter(|snapshot| snapshot.source == Source::Kungal)
                    .filter_map(|snapshot| {
                        let data = &snapshot.data;
                        data["character"]["latin"]
                            .as_str()
                            .or_else(|| data["latin"].as_str())
                    })
                    .map(str::trim),
            )
            .find(|name| !name.is_empty())
    }

    /// 展示立绘；索引失效时回退首图，不丢失原始图片。
    pub fn artwork(&self) -> Option<&Artwork> {
        self.artworks
            .get(self.selected_art)
            .or_else(|| self.artworks.first())
    }
    /// 已确认同一角色或核对外部 ID 后合并来源，保留已有立绘与收藏。
    pub fn merge(&mut self, other: Character) {
        for a in other.aliases {
            if !self
                .aliases
                .iter()
                .any(|v| v.kind == a.kind && v.name == a.name)
            {
                self.aliases.push(a);
            }
        }
        for s in &other.snapshots {
            self.snapshots
                .retain(|old| old.source != s.source || old.id != s.id);
        }
        let sources: Vec<_> = other.snapshots.iter().map(|s| s.source).collect();
        self.tags.retain(|t| !sources.contains(&t.source));
        self.tags.extend(other.tags);
        self.snapshots.extend(other.snapshots);
        if !other.artworks.is_empty() && self.artworks.is_empty() {
            self.artworks = other.artworks;
        }
        if self.summary.is_empty() {
            self.summary = other.summary;
        }
    }
}
/// 持久设置，旧文件缺字段时仅补该字段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub color_theme: String,
    pub font_size: u8,
    pub language: String,
    pub search_source: Source,
    pub ai_provider: String,
    pub tag_source: Source,
    #[serde(default = "automatic_tags")]
    pub tag_source_automatic: bool,
    pub proxy: String,
    /// 应用检查更新所用的 GitHub Releases 页面。
    pub release_page: String,
    /// 用户选择的 GalGear catalog.json；空值关闭联动。
    pub galgear_library_path: String,
    /// 库排序模式和独立角色顺序，旧库缺省保留添加顺序。
    pub library_order: crate::library_order::Order,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            color_theme: "pink".into(),
            font_size: 16,
            language: "zh-CN".into(),
            search_source: Source::Bangumi,
            ai_provider: "openai".into(),
            tag_source: Source::Kungal,
            tag_source_automatic: true,
            proxy: String::new(),
            release_page: crate::updates::RELEASE_PAGE.into(),
            galgear_library_path: String::new(),
            library_order: Default::default(),
        }
    }
}
fn automatic_tags() -> bool {
    true
}
impl Settings {
    /// 自动标签源跟随界面语言，手动全局选择优先。
    pub fn effective_tag_source(&self) -> Source {
        if self.tag_source_automatic {
            crate::tags::language_source(&self.language)
        } else {
            self.tag_source
        }
    }
}
/// 独立角色库与命名收藏夹。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub characters: Vec<Character>,
    #[serde(default)]
    pub collections: BTreeMap<String, Vec<String>>,
    /// 缺省为平铺收藏夹；只记录有父级的项，以兼容已有资料文件。
    #[serde(default)]
    pub collection_parents: BTreeMap<String, String>,
    #[serde(default)]
    pub settings: Settings,
}
/// 立绘选择的轻量命令与响应，不携带图片或完整角色库。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkSelection {
    pub id: String,
    pub selected_art: usize,
}
/// 单个或批量收藏提交的结果，仅携带角色身份和已保存状态。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteUpdate {
    pub ids: Vec<String>,
    pub favorite: bool,
}
impl FavoriteUpdate {
    /// 应用已确认的收藏状态；已移除的角色不会由迟到结果重新创建。
    pub fn apply(&self, library: &mut Library) {
        for character in &mut library.characters {
            if self.ids.contains(&character.id) {
                character.favorite = self.favorite;
            }
        }
    }
}
/// 搜索候选不是自动合并依据。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub source: Source,
    pub id: String,
    pub name: String,
    pub subtitle: String,
    /// 来源明确提供的罗马字名称；缺失或与主名重复时不展示，不自动音译。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub romanized_name: Option<String>,
    /// 搜索预览使用 Bangumi 返回的角色图片；缺图不影响候选身份与导入。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<String>,
}

/// 从 Bangumi 详情产生投影，原响应原样保留。
pub fn from_bangumi(v: Value) -> Result<Character, String> {
    let id = v["id"]
        .as_u64()
        .ok_or("Invalid Bangumi character ID")?
        .to_string();
    let name = v["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Missing character name")?
        .to_owned();
    let mut aliases = Vec::new();
    for row in v["infobox"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["key"] == "别名")
    {
        if let Some(a) = row["value"].as_array() {
            for a in a {
                aliases.push(Alias {
                    kind: text(a, "k"),
                    name: text(a, "v"),
                });
            }
        } else if let Some(s) = row["value"].as_str() {
            aliases.push(Alias {
                kind: "别名".into(),
                name: s.into(),
            });
        }
    }
    Ok(Character {
        id: format!("bangumi:{id}"),
        name,
        summary: text(&v, "summary"),
        aliases,
        snapshots: vec![Snapshot {
            source: Source::Bangumi,
            id: id.clone(),
            url: format!("https://bgm.tv/character/{id}"),
            data: v,
        }],
        ..Default::default()
    })
}
pub(crate) fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}
/// 离线演示来自已保存 API 快照，图片来自用户已处理的相册。
pub fn sample() -> Result<Character, String> {
    let raw = serde_json::from_str(include_str!("../../../fixtures/setsuna-bangumi.json"))
        .map_err(|e| e.to_string())?;
    let mut c = from_bangumi(raw)?;
    let album: Vec<Value> = serde_json::from_str(include_str!("../../../setsuna-album/album.json"))
        .map_err(|e| e.to_string())?;
    c.artworks = album
        .iter()
        .filter(|a| a["id"] != 21278)
        .map(|a| {
            let id = a["id"].to_string();
            Artwork {
                id: id.clone(),
                title: text(a, "title"),
                url: format!("assets/setsuna/{id}.png"),
                original: text(a, "image"),
                spoiler: a["spoiler"].as_bool().unwrap_or(false),
                enhanced: Some(format!("assets/setsuna/{id}-2x.png")),
                original_data: None,
            }
        })
        .collect();
    // 初始服装来自相册中的第一作，不以最新投稿排序代替初始立绘。
    if let Some(i) = c.artworks.iter().position(|a| a.id == "21037") {
        c.artworks.swap(0, i);
    }
    c.snapshots[0].data = serde_json::json!({"character":c.snapshots[0].data,"subjects":serde_json::from_str::<Value>(include_str!("../../../fixtures/setsuna-subjects.json")).map_err(|e|e.to_string())?,"persons":serde_json::from_str::<Value>(include_str!("../../../fixtures/setsuna-persons.json")).map_err(|e|e.to_string())?,"album":album});
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::{from_bangumi, sample};
    #[test]
    fn sample_keeps_all_alias_kinds_and_unknown_fields() {
        let c = sample().unwrap();
        assert_eq!(c.aliases.len(), 3);
        assert_eq!(c.aliases[1].name, "しんどう せつな");
        assert!(c.snapshots[0].data["character"].get("locked").is_some());
        assert_eq!(c.artworks.len(), 11);
        assert!(!c.artworks.iter().any(|a| a.id == "21278"));
    }
    #[test]
    fn rejects_missing_identity() {
        assert!(from_bangumi(serde_json::json!({"name":"a"})).is_err());
    }
    #[test]
    fn roundtrip_retains_raw_data() {
        let c = sample().unwrap();
        let v = serde_json::to_value(&c).unwrap();
        let restored: super::Character = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(v, serde_json::to_value(restored).unwrap());
    }
    #[test]
    fn old_artwork_without_original_data_remains_readable() {
        let old = serde_json::json!({"id":"1","title":"old","url":"https://lain.bgm.tv/a.png","original":"https://lain.bgm.tv/a.png","spoiler":false});
        let art: super::Artwork = serde_json::from_value(old).unwrap();
        assert!(art.original_data.is_none());
        assert!(art.enhanced.is_none());
        assert_eq!(art.url, art.original);
    }
}
