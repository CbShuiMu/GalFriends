//! 排序动作与拖拽身份校验；几何和动画由 DOM 处理，保存成功才更新 Rust 顺序。
use crate::{
    library::{query, state::Panel},
    requests::{self, Kind},
    state::{APP, App, Page},
};
use galfriends_core::library_order::Sort;
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::prelude::wasm_bindgen;

pub(super) fn enabled(a: &App) -> bool {
    a.loaded
        && a.page == Page::Library
        && a.pending.is_none()
        && a.library_ui.panel == Panel::Closed
        && !a.search_open
        && !a.confirm_remove
        && a.library.settings.library_order.sort == Sort::Custom
}

fn drag_ids<'a>(a: &'a App, id: &str) -> Vec<&'a str> {
    if !enabled(a) {
        return Vec::new();
    }
    let visible = query::visible(a);
    if !visible.iter().any(|c| c.id == id) {
        return Vec::new();
    }
    let group = a.library_ui.batch && a.library_ui.checked.contains(id);
    visible
        .into_iter()
        .filter(|c| {
            if group {
                a.library_ui.checked.contains(&c.id)
            } else {
                c.id == id
            }
        })
        .map(|c| c.id.as_str())
        .collect()
}

/// 返回当前可见的单张卡片或所选整组身份，忙碌或非自定模式返回空数组。
#[wasm_bindgen]
pub fn library_drag_ids(id: &str) -> String {
    APP.with(|state| serde_json::to_string(&drag_ids(&state.borrow(), id)).unwrap_or_default())
}

#[derive(Deserialize)]
struct Movement {
    ids: Vec<String>,
    before: Option<String>,
}

pub(super) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    match action {
        "sort-pick" => {
            let sort = match value {
                "added" => Sort::Added,
                "name" => Sort::Name,
                "source" => Sort::Source,
                "custom" => Sort::Custom,
                _ => return true,
            };
            a.library_ui.sort_open = false;
            if a.pending.is_none() {
                requests::send(
                    a,
                    Kind::LibraryOrder,
                    "set_library_sort",
                    json!({"sort":sort,"descending":a.library.settings.library_order.descending}),
                );
            }
        }
        "sort-direction" => {
            let order = &a.library.settings.library_order;
            if a.pending.is_none() && order.sort != Sort::Custom {
                requests::send(
                    a,
                    Kind::LibraryOrder,
                    "set_library_sort",
                    json!({"sort":order.sort,"descending":!order.descending}),
                );
            }
        }
        "library-reorder" => {
            if !enabled(a) {
                return true;
            }
            let Ok(movement) = serde_json::from_str::<Movement>(value) else {
                return true;
            };
            let visible = query::visible(a);
            if movement.ids.is_empty()
                || movement
                    .ids
                    .iter()
                    .any(|id| !visible.iter().any(|c| &c.id == id))
                || movement.before.as_ref().is_some_and(|id| {
                    movement.ids.contains(id) || !visible.iter().any(|c| &c.id == id)
                })
            {
                return true;
            }
            requests::send(
                a,
                Kind::LibraryOrder,
                "reorder_characters",
                json!({"ids":movement.ids,"before":movement.before}),
            );
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{drag_ids, handle};
    use crate::{
        library::state::Panel,
        state::{App, Page},
    };
    use galfriends_core::{characters::Character, library_order::Sort};

    #[test]
    fn drag_uses_visible_selection_in_custom_order_and_blocks_other_contexts() {
        let mut a = App {
            loaded: true,
            page: Page::Library,
            ..Default::default()
        };
        a.library.characters = ["a", "b", "c"]
            .into_iter()
            .map(|id| Character {
                id: id.into(),
                name: id.into(),
                ..Default::default()
            })
            .collect();
        a.library.settings.library_order.sort = Sort::Custom;
        a.library.settings.library_order.custom_order = vec!["c".into(), "a".into(), "b".into()];
        a.library_ui.batch = true;
        a.library_ui.checked = ["a".into(), "c".into(), "missing".into()].into();
        assert_eq!(drag_ids(&a, "a"), ["c", "a"]);
        assert_eq!(drag_ids(&a, "b"), ["b"]);
        a.filter = "a".into();
        assert_eq!(drag_ids(&a, "a"), ["a"]);
        assert!(drag_ids(&a, "c").is_empty());
        handle(&mut a, "library-reorder", r#"{"ids":["c"],"before":"a"}"#);
        assert!(a.pending.is_none());
        a.library_ui.panel = Panel::AddCharacters;
        assert!(drag_ids(&a, "a").is_empty());
        a.library_ui.panel = Panel::Closed;
        a.page = Page::Favorites;
        assert!(drag_ids(&a, "a").is_empty());
    }
}
