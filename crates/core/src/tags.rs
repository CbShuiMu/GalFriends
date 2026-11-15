//! 标签的本地显示名、隐藏与顺序；来源标签和原始响应始终保留。
use crate::characters::{Character, Source, Tag};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 与来源数据分离的本地显示配置，缺省保持原顺序与名称。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Presentation {
    pub names: BTreeMap<String, String>,
    pub hidden: BTreeSet<String>,
    pub order: Vec<String>,
    /// 用户添加的标签独立于任何 API 来源。
    pub added: Vec<String>,
}

/// 用于详情、卡片和筛选器的同一标签投影。
pub struct DisplayTag<'a> {
    pub key: String,
    pub name: &'a str,
    pub original: Option<&'a Tag>,
}

/// 中文使用鲲 Gal，日文和英文使用 VNDB。
pub fn language_source(language: &str) -> Source {
    if language == "zh-CN" {
        Source::Kungal
    } else {
        Source::Vndb
    }
}

/// 角色的手动选择优先于全局默认来源。
pub fn source(character: &Character, default: Source) -> Source {
    character.tag_source.unwrap_or(default)
}

/// 来源、分组与名称共同标识标签；长度前缀防止分隔符碰撞。
pub fn key(tag: &Tag) -> String {
    format!(
        "{}:{}:{}{}",
        tag.source.key(),
        tag.group.len(),
        tag.group,
        tag.name
    )
}

/// 返回当前来源的安全可见标签，不修改底层数据。
pub fn visible(character: &Character, source: Source) -> Vec<DisplayTag<'_>> {
    let source = self::source(character, source);
    let mut tags: Vec<_> = character
        .tags
        .iter()
        .filter(|tag| tag.source == source && !tag.spoiler && !tag.sexual)
        .filter_map(|tag| {
            let key = key(tag);
            (!character.tag_presentation.hidden.contains(&key)).then(|| DisplayTag {
                name: character
                    .tag_presentation
                    .names
                    .get(&key)
                    .map(String::as_str)
                    .unwrap_or(&tag.name),
                key,
                original: Some(tag),
            })
        })
        .collect();
    tags.extend(
        character
            .tag_presentation
            .added
            .iter()
            .map(|name| DisplayTag {
                key: format!("local:{name}"),
                name,
                original: None,
            }),
    );
    tags.sort_by_key(|tag| {
        character
            .tag_presentation
            .order
            .iter()
            .position(|key| key == &tag.key)
            .unwrap_or(usize::MAX)
    });
    tags
}

#[derive(Deserialize)]
struct Change {
    #[serde(default)]
    key: String,
    #[serde(default)]
    name: String,
    before: Option<String>,
    source: Option<Source>,
}

/// 校验一个显示操作，成功后修改配置；调用方负责原子保存。
pub fn apply(character: &mut Character, action: &str, value: &str) -> Result<(), String> {
    if action == "tag-reset" {
        let added = std::mem::take(&mut character.tag_presentation.added);
        character.tag_presentation = Presentation::default();
        character.tag_presentation.added = added;
        return Ok(());
    }
    let change: Change = serde_json::from_str(value).map_err(|e| e.to_string())?;
    if matches!(action, "tag-add" | "tag-rename")
        && (change.name.trim().is_empty() || change.name.trim().chars().count() > 120)
    {
        return Err("Invalid tag name".into());
    }
    if action == "tag-add" {
        let name = change.name.trim().to_owned();
        if !character.tag_presentation.added.contains(&name) {
            character.tag_presentation.added.push(name);
        }
        return Ok(());
    }
    let local = character
        .tag_presentation
        .added
        .iter()
        .position(|name| format!("local:{name}") == change.key);
    let tag = character
        .tags
        .iter()
        .find(|tag| key(tag) == change.key && !tag.spoiler && !tag.sexual);
    if tag.is_none() && local.is_none() {
        return Err("Tag not found".into());
    }
    if let Some(index) = local {
        match action {
            "tag-rename" => {
                let name = change.name.trim().to_owned();
                if character
                    .tag_presentation
                    .added
                    .iter()
                    .enumerate()
                    .any(|(i, value)| i != index && value == &name)
                {
                    return Err("Duplicate local tag".into());
                }
                for key in &mut character.tag_presentation.order {
                    if *key == change.key {
                        *key = format!("local:{name}");
                    }
                }
                character.tag_presentation.added[index] = name;
                return Ok(());
            }
            "tag-hide" => {
                character.tag_presentation.added.remove(index);
                character
                    .tag_presentation
                    .order
                    .retain(|key| key != &change.key);
                return Ok(());
            }
            _ => {}
        }
    }
    match action {
        "tag-rename" => {
            let name = change.name.trim();
            if name.is_empty() || name.chars().count() > 120 {
                return Err("Invalid tag name".into());
            }
            character
                .tag_presentation
                .names
                .insert(change.key, name.into());
        }
        "tag-hide" => {
            character.tag_presentation.hidden.insert(change.key);
        }
        "tag-reorder" => {
            let source = tag
                .map(|tag| tag.source)
                .or(change.source)
                .ok_or("Missing tag source")?;
            let mut order: Vec<_> = visible(character, source)
                .into_iter()
                .map(|tag| tag.key)
                .collect();
            let from = order
                .iter()
                .position(|key| key == &change.key)
                .ok_or("Tag not visible")?;
            if change.before.as_ref() == Some(&change.key) {
                return Ok(());
            }
            let target = if let Some(before) = change.before {
                order
                    .iter()
                    .position(|key| *key == before)
                    .ok_or("Invalid tag destination")?
            } else {
                order.len()
            };
            let item = order.remove(from);
            order.insert(target.saturating_sub(usize::from(from < target)), item);
            let other: Vec<_> = character
                .tags
                .iter()
                .filter(|tag| tag.source != source)
                .map(key)
                .collect();
            character
                .tag_presentation
                .order
                .retain(|key| other.contains(key));
            character.tag_presentation.order.extend(order);
        }
        _ => return Err("Invalid tag operation".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_tags_survive_source_changes_and_reset_without_changing_api_data() {
        let mut c = crate::characters::sample().unwrap();
        let snapshots = c.snapshots.clone();
        apply(&mut c, "tag-add", r#"{"name":"  自定义  "}"#).unwrap();
        apply(&mut c, "tag-add", r#"{"name":"自定义"}"#).unwrap();
        assert_eq!(c.tag_presentation.added, ["自定义"]);
        c.tag_source = Some(Source::Vndb);
        assert!(
            visible(&c, Source::Kungal)
                .iter()
                .any(|tag| tag.name == "自定义" && tag.original.is_none())
        );
        apply(
            &mut c,
            "tag-rename",
            r#"{"key":"local:自定义","name":"新标签"}"#,
        )
        .unwrap();
        apply(&mut c, "tag-reset", "").unwrap();
        assert_eq!(c.tag_presentation.added, ["新标签"]);
        assert_eq!(c.snapshots, snapshots);
        apply(&mut c, "tag-hide", r#"{"key":"local:新标签"}"#).unwrap();
        assert!(c.tag_presentation.added.is_empty());
    }

    #[test]
    fn language_defaults_and_manual_overrides_are_explicit() {
        let mut settings = crate::characters::Settings::default();
        for (language, expected) in [
            ("zh-CN", Source::Kungal),
            ("en-US", Source::Vndb),
            ("ja-JP", Source::Vndb),
        ] {
            crate::preferences::set(&mut settings, "language", language).unwrap();
            assert_eq!(settings.effective_tag_source(), expected);
        }
        crate::preferences::set(&mut settings, "tag_source", "kungal").unwrap();
        assert_eq!(settings.effective_tag_source(), Source::Kungal);
        crate::preferences::set(&mut settings, "tag_source", "auto").unwrap();
        assert_eq!(settings.effective_tag_source(), Source::Vndb);
    }
    #[test]
    fn edits_and_order_preserve_sources_and_can_be_reset() {
        let mut character = crate::characters::sample().unwrap();
        character.tags = ["first", "second"]
            .map(|name| Tag {
                name: name.into(),
                group: "group".into(),
                source: Source::Vndb,
                spoiler: false,
                sexual: false,
            })
            .to_vec();
        let original = serde_json::to_value(&character.tags).unwrap();
        let keys: Vec<_> = character.tags.iter().map(key).collect();
        apply(
            &mut character,
            "tag-rename",
            &serde_json::json!({"key":keys[0],"name":"new"}).to_string(),
        )
        .unwrap();
        apply(
            &mut character,
            "tag-reorder",
            &serde_json::json!({"key":keys[1],"before":keys[0]}).to_string(),
        )
        .unwrap();
        assert_eq!(
            visible(&character, Source::Vndb)
                .iter()
                .map(|tag| tag.name)
                .collect::<Vec<_>>(),
            ["second", "new"]
        );
        apply(
            &mut character,
            "tag-hide",
            &serde_json::json!({"key":keys[0]}).to_string(),
        )
        .unwrap();
        assert_eq!(visible(&character, Source::Vndb).len(), 1);
        assert_eq!(serde_json::to_value(&character.tags).unwrap(), original);
        let before = serde_json::to_value(&character).unwrap();
        assert!(
            apply(
                &mut character,
                "tag-reorder",
                &serde_json::json!({"key":keys[1],"before":"missing"}).to_string()
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(&character).unwrap(), before);
        apply(&mut character, "tag-reset", "").unwrap();
        assert_eq!(
            visible(&character, Source::Vndb)
                .iter()
                .map(|tag| tag.name)
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
