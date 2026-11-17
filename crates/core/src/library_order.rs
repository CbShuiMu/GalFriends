//! 自定顺序的补齐与整组移动规则；不改变角色的添加顺序或资料，持久化由 store 负责。
use crate::characters::Character;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 库工具栏支持的排序方式，旧库默认保留添加顺序。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Added,
    Name,
    Source,
    Custom,
}

/// 排序设置与自定角色 ID 顺序；不含角色资料或图片，可作为轻量响应。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Order {
    pub sort: Sort,
    pub descending: bool,
    pub custom_order: Vec<String>,
}

fn name_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 保留已保存的顺序，删除失效/重复 ID，新角色按规范化姓名及 ID 排序后追加。
/// 沿用 GalGear 的补齐规则；角色没有游戏品牌，初始化只按姓名排列。
pub fn reconcile(order: &mut Order, characters: &[Character]) {
    let known: BTreeSet<_> = characters.iter().map(|c| c.id.as_str()).collect();
    let mut seen = BTreeSet::new();
    order
        .custom_order
        .retain(|id| known.contains(id.as_str()) && seen.insert(id.clone()));
    let mut remaining: Vec<_> = characters
        .iter()
        .filter(|c| !seen.contains(&c.id))
        .collect();
    remaining.sort_by_cached_key(|c| (name_key(&c.name), c.id.as_str()));
    order
        .custom_order
        .extend(remaining.into_iter().map(|c| c.id.clone()));
}

/// 按已有顺序移动整组角色；筛选隐藏的角色保持相对位置，None 表示移至库末尾。
/// 空选择、未知身份或将整组放到自身之前均拒绝，失败不修改传入顺序。
pub fn reorder(
    order: &mut Order,
    characters: &[Character],
    ids: &[String],
    before: Option<&str>,
) -> Result<(), String> {
    let known: BTreeSet<_> = characters.iter().map(|c| c.id.as_str()).collect();
    let selected: BTreeSet<_> = ids.iter().map(String::as_str).collect();
    if selected.is_empty() || selected.iter().any(|id| !known.contains(id)) {
        return Err("library.order_invalid_characters".into());
    }
    if before.is_some_and(|id| !known.contains(id) || selected.contains(id)) {
        return Err("library.order_invalid_destination".into());
    }
    reconcile(order, characters);
    let moving: Vec<_> = order
        .custom_order
        .iter()
        .filter(|id| selected.contains(id.as_str()))
        .cloned()
        .collect();
    order
        .custom_order
        .retain(|id| !selected.contains(id.as_str()));
    let index = before
        .and_then(|id| order.custom_order.iter().position(|item| item == id))
        .unwrap_or(order.custom_order.len());
    order.custom_order.splice(index..index, moving);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Order, reconcile, reorder};
    use crate::characters::Character;

    fn characters() -> Vec<Character> {
        ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(|id| Character {
                id: id.into(),
                name: id.into(),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn moves_group_in_saved_order_and_preserves_filtered_characters() {
        let characters = characters();
        let mut order = Order::default();
        reorder(
            &mut order,
            &characters,
            &["d".into(), "b".into()],
            Some("a"),
        )
        .unwrap();
        assert_eq!(order.custom_order, ["b", "d", "a", "c", "e"]);
        reorder(&mut order, &characters, &["b".into(), "d".into()], None).unwrap();
        assert_eq!(order.custom_order, ["a", "c", "e", "b", "d"]);
        reorder(&mut order, &characters, &["d".into()], Some("e")).unwrap();
        assert_eq!(order.custom_order, ["a", "c", "d", "e", "b"]);
    }

    #[test]
    fn invalid_or_stale_moves_leave_order_unchanged() {
        let characters = characters();
        let original = Order {
            custom_order: vec!["b".into(), "a".into()],
            ..Default::default()
        };
        for (ids, before) in [
            (vec![], None),
            (vec!["missing".into()], None),
            (vec!["b".into()], Some("b")),
            (vec!["b".into()], Some("missing")),
        ] {
            let mut order = original.clone();
            assert!(reorder(&mut order, &characters, &ids, before).is_err());
            assert_eq!(order, original);
        }
    }

    #[test]
    fn reconcile_keeps_existing_positions_and_appends_new_names() {
        let mut order = Order {
            custom_order: vec!["d".into(), "a".into(), "d".into(), "removed".into()],
            ..Default::default()
        };
        reconcile(&mut order, &characters());
        assert_eq!(order.custom_order, ["d", "a", "b", "c", "e"]);
        let mut changed = characters();
        changed[0].name = "Z".into();
        reconcile(&mut order, &changed);
        assert_eq!(order.custom_order, ["d", "a", "b", "c", "e"]);
    }
}
