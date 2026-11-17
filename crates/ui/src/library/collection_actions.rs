//! 收藏夹导航、编辑和成员选择草稿；保存均由后端事务确认。
use crate::{
    library::{collections, state::Panel},
    requests::{self, Kind},
    state::App,
};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    match action {
        "collection-picker" => {
            let single = !value.is_empty();
            let ids = if !single {
                a.library_ui.checked.clone()
            } else if a.library.characters.iter().any(|c| c.id == value) {
                BTreeSet::from([value.into()])
            } else {
                return true;
            };
            if !ids.is_empty() {
                a.library_ui.panel = Panel::Collections {
                    query: String::new(),
                    ids,
                    single,
                };
            }
        }
        "collection-pick" => {
            if let Panel::Collections { ids, .. } = &a.library_ui.panel
                && galfriends_core::collections::accepts_characters(&a.library, value)
            {
                let payload = json!({"operation":"collect","ids":ids,"collection":value});
                requests::send(a, Kind::Batch, "batch_characters", payload);
            }
        }
        "collection-star" => {
            if let Panel::Collections {
                ids, single: true, ..
            } = &a.library_ui.panel
                && let Some(id) = ids.first().cloned()
            {
                requests::mutate(a, "favorite", &id, "");
            }
        }
        "folder-open" | "favorites-open" => {
            a.collection = if action == "folder-open" {
                value.into()
            } else {
                String::new()
            };
            a.library_ui.show_favorites = action == "favorites-open";
            a.library_ui.batch = false;
            a.library_ui.checked.clear();
            a.library_ui.checked_folders.clear();
            a.filter.clear();
            a.library_ui.tags.clear();
        }
        "folder-new" => {
            a.collection_draft.clear();
            a.library_ui.panel = Panel::FolderEdit {
                original: None,
                parent: value.into(),
            };
        }
        "folder-edit" => {
            a.collection_draft = a.collection.clone();
            a.library_ui.panel = Panel::FolderEdit {
                original: Some(a.collection.clone()),
                parent: a
                    .library
                    .collection_parents
                    .get(&a.collection)
                    .cloned()
                    .unwrap_or_default(),
            };
        }
        "folder-save" => {
            if let Panel::FolderEdit { original, parent } = &a.library_ui.panel {
                let payload =
                    json!({"original":original,"name":a.collection_draft,"parent":parent});
                requests::send(a, Kind::Folder, "save_collection", payload);
            }
        }
        "folder-select" => {
            if !a.library_ui.checked_folders.remove(value) {
                a.library_ui.checked_folders.insert(value.into());
            }
        }
        "folders-delete-selected" => {
            if !a.library_ui.checked_folders.is_empty() {
                a.library_ui.panel = Panel::FolderDelete {
                    names: a.library_ui.checked_folders.iter().cloned().collect(),
                };
            }
        }
        "folders-delete-confirm" => {
            if let Panel::FolderDelete { names } = &a.library_ui.panel {
                let payload = json!({"names":names});
                requests::send(a, Kind::Folder, "delete_collections", payload);
            }
        }
        "folder-add-characters" => {
            a.library_ui.picker.clear();
            a.library_ui.picker_query.clear();
            a.library_ui.panel = Panel::AddCharacters;
        }
        "picker-character" => {
            if !a.library_ui.picker.remove(value) {
                a.library_ui.picker.insert(value.into());
            }
        }
        "picker-save" => {
            if !a.library_ui.picker.is_empty() {
                let payload = json!({"operation":"collect","ids":a.library_ui.picker,"collection":a.collection});
                requests::send(a, Kind::Batch, "batch_characters", payload);
            }
        }
        "folder-select-all" => {
            a.library_ui.checked_folders = collections::children(a).into_iter().cloned().collect();
            a.library_ui.checked = super::query::visible(a)
                .iter()
                .map(|c| c.id.clone())
                .collect();
        }
        _ => return false,
    }
    true
}
