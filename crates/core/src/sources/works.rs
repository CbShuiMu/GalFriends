//! 按出演关系中已经确认的作品 ID 批量补齐封面和跨站标识。
use super::{CatalogClient, array, request};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn details(client: &CatalogClient, appearances: &[Value]) -> Result<Vec<Value>, String> {
    let ids: Vec<_> = appearances
        .iter()
        .filter_map(|a| a["work"]["id"].as_str())
        .filter(|id| crate::game_library::numeric_id(id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut values = Vec::new();
    for chunk in ids.chunks(100) {
        let page = request(
            client
                .get("https://api.nextmoe.dev/v2/catalog/works")
                .query(&[
                    ("ids", chunk.join(",").as_str()),
                    ("include", "refs,covers"),
                    ("nsfw", "true"),
                    ("limit", "100"),
                ]),
        )?;
        values.extend(array(&page, "items")?.iter().cloned());
    }
    Ok(values)
}
