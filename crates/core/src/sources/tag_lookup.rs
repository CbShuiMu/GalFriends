//! 标签补全只使用已关联身份及 NextMoe 的显式外部 ID，网络结果提交前由桌面层核对并发状态。
use super::{CatalogClient, array, fetch, identifier, request};
use crate::characters::{Character, Snapshot, Source};
use serde_json::Value;
use std::collections::BTreeSet;

fn data(snapshots: &[Snapshot]) -> Option<&Value> {
    snapshots
        .iter()
        .find(|s| s.source == Source::Kungal)
        .map(|s| s.data.get("character").unwrap_or(&s.data))
}

fn references(value: &Value, source: Source) -> Vec<String> {
    value["refs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["source"] == source.key())
        .filter_map(|r| r["external_id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn matches_identity(value: &Value, snapshots: &[Snapshot]) -> bool {
    snapshots
        .iter()
        .any(|s| references(value, s.source).contains(&s.id))
}

/// 按明确的来源关联读取标签和完整快照。无法唯一定位返回空，由界面提供手动选择。
/// 客户端工厂让凭据始终限定在对应来源主机。
pub fn fetch_for(
    snapshots: &[Snapshot],
    source: Source,
    mut client_for: impl FnMut(Source) -> Result<CatalogClient, String>,
) -> Result<Option<Character>, String> {
    if !matches!(source, Source::Kungal | Source::Vndb) {
        return Err("Unsupported tag source".into());
    }
    let mut ids: Vec<_> = snapshots
        .iter()
        .filter(|s| s.source == source)
        .map(|s| s.id.clone())
        .collect();
    let mut crosswalk = None;
    if ids.is_empty() {
        if let Some(value) = data(snapshots) {
            ids = references(value, source);
        } else {
            let refs = snapshots
                .iter()
                .filter(|s| matches!(s.source, Source::Bangumi | Source::Vndb))
                .take(10)
                .map(|s| format!("{}:{}", s.source.key(), s.id))
                .collect::<Vec<_>>()
                .join(",");
            if refs.is_empty() {
                return Ok(None);
            }
            let client = client_for(Source::Kungal)?;
            let page = request(
                client
                    .get("https://api.nextmoe.dev/v2/catalog/characters")
                    .query(&[("refs", refs.as_str()), ("view", "full"), ("nsfw", "true")]),
            )?;
            let candidates: Vec<_> = array(&page, "items")?
                .iter()
                .filter(|v| matches_identity(v, snapshots))
                .collect();
            if candidates.len() != 1 {
                return Ok(None);
            }
            let id = candidates[0]["id"].as_str().ok_or("Invalid catalog ID")?;
            let linked = fetch(&client, Source::Kungal, id)?;
            ids = if source == Source::Kungal {
                vec![id.to_owned()]
            } else {
                data(&linked.snapshots)
                    .map(|v| references(v, source))
                    .unwrap_or_default()
            };
            crosswalk = Some(linked);
        }
    }
    if ids.is_empty() {
        return Ok(None);
    }
    ids.sort();
    ids.dedup();
    if ids.len() > 20 {
        return Err("Too many linked character IDs".into());
    }
    let mut result = crosswalk.unwrap_or_default();
    if source == Source::Kungal && !result.snapshots.is_empty() {
        return Ok(Some(result));
    }
    let client = client_for(source)?;
    // 一个目录身份可能明确链接多个 VNDB 角色条目，全部保留并合并标签。
    for id in ids {
        identifier(source, &id)?;
        let record = fetch(&client, source, &id)?;
        result.aliases.extend(record.aliases);
        for tag in record.tags {
            if !result.tags.contains(&tag) {
                result.tags.push(tag);
            }
        }
        result.snapshots.extend(record.snapshots);
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::{matches_identity, references};
    use crate::characters::{Character, Snapshot, Source};
    use serde_json::json;

    #[test]
    fn matching_uses_external_ids_and_preserves_multiple_vndb_refs() {
        let mut character = Character::default();
        character.snapshots.push(Snapshot {
            source: Source::Bangumi,
            id: "76897".into(),
            url: String::new(),
            data: json!({}),
        });
        let value = json!({"display_name":"神堂悠果","refs":[
            {"source":"bangumi","external_id":"76897"},
            {"source":"vndb","external_id":"c88889"},
            {"source":"vndb","external_id":"c101382"}]});
        assert!(matches_identity(&value, &character.snapshots));
        assert_eq!(references(&value, Source::Vndb), ["c101382", "c88889"]);
        character.snapshots[0].id = "76896".into();
        character.name = "神堂悠果".into();
        assert!(!matches_identity(&value, &character.snapshots));
    }
}
