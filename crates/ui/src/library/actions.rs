//! 工具栏动作统一在此编排；批量写入以单个后端事务确认。
use crate::{
    library::{query, state::Panel},
    requests::{self, Kind},
    state::App,
};
use serde_json::json;

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    if super::order::handle(a, action, value)
        || super::collection_actions::handle(a, action, value)
        || super::tag_filter::handle(a, action, value)
    {
        return true;
    }
    match action {
        "sort-toggle" => a.library_ui.sort_open = !a.library_ui.sort_open,
        "sort-close" => a.library_ui.sort_open = false,
        "library-panel-close" => a.library_ui.panel = Panel::Closed,
        "batch-toggle" => {
            a.library_ui.batch = !a.library_ui.batch;
            a.library_ui.checked.clear();
            a.library_ui.checked_folders.clear();
        }
        "batch-check" => {
            if a.library_ui.batch
                && a.library.characters.iter().any(|c| c.id == value)
                && !a.library_ui.checked.remove(value)
            {
                a.library_ui.checked.insert(value.into());
            }
        }
        "batch-all" => {
            a.library_ui.checked = query::visible(a).iter().map(|c| c.id.clone()).collect()
        }
        "batch-clear" => {
            a.library_ui.checked.clear();
            a.library_ui.checked_folders.clear();
        }
        "batch-remove" => a.confirm_remove = !a.library_ui.checked.is_empty(),
        "batch-favorite" | "batch-unfavorite" | "batch-detach" | "batch-confirm-remove" => {
            if a.library_ui.checked.is_empty() {
                return true;
            }
            let operation = match action {
                "batch-favorite" => "favorite",
                "batch-unfavorite" => "unfavorite",
                "batch-detach" => "detach",
                _ => "remove",
            };
            requests::send(
                a,
                if matches!(action, "batch-favorite" | "batch-unfavorite") {
                    Kind::BatchFavorite
                } else {
                    Kind::Batch
                },
                "batch_characters",
                json!({"operation":operation,"ids":a.library_ui.checked,"collection":value}),
            );
        }
        "refresh-library" => requests::send(a, Kind::Load, "load_library", json!({})),
        _ => return false,
    }
    true
}
