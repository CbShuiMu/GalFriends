//! 对齐 GalGear 的收藏夹视图：根目录、两级文件夹、独立工具栏及编辑弹窗。
use crate::{
    library::state::Panel,
    state::App,
    widgets::{confirmation_actions, esc, icon, icon_button},
};
use galfriends_core::collections::accepts_characters;
use std::{collections::BTreeSet, fmt::Write};

pub(super) fn children(a: &App) -> Vec<&String> {
    if a.library_ui.show_favorites {
        return Vec::new();
    }
    a.library
        .collections
        .keys()
        .filter(|name| {
            a.library
                .collection_parents
                .get(*name)
                .map_or("", String::as_str)
                == a.collection
        })
        .collect()
}
fn toolbar(a: &App) -> String {
    let busy = a.pending.is_some();
    let b = |action: &str, label: &str, symbol: &str, class: &str, disabled: bool| {
        icon_button(action, "", a.t(label), symbol, class, disabled)
    };
    let mut h = format!(
        r#"<div class="library-bar collection-bar {}" role="toolbar" aria-label="{}">"#,
        if a.library_ui.batch { "selecting" } else { "" },
        a.t("favorites")
    );
    if a.library_ui.batch {
        let count = a.library_ui.checked.len() + a.library_ui.checked_folders.len();
        h.push_str(&format!(
            r#"<span class="selection-total"><strong>{}</strong> {}</span>"#,
            count,
            a.t("selected")
        ));
        h.push_str(&b(
            "folder-select-all",
            "select_all",
            "done_all",
            "quiet",
            busy,
        ));
        h.push_str(&b(
            "batch-clear",
            "clear_selection",
            "close",
            "quiet",
            busy || count == 0,
        ));
        h.push_str("<span class=\"bar-spacer\"></span>");
        if !a.library_ui.checked_folders.is_empty() {
            h.push_str(&b(
                "folders-delete-selected",
                "delete_collections",
                "trash",
                "quiet danger",
                busy,
            ));
        }
        if !a.collection.is_empty() || a.library_ui.show_favorites {
            h.push_str(&icon_button(
                if a.library_ui.show_favorites {
                    "batch-unfavorite"
                } else {
                    "batch-detach"
                },
                &a.collection,
                a.t("remove_collection"),
                "trash",
                "quiet danger",
                busy || a.library_ui.checked.is_empty(),
            ));
        }
        h.push_str(&b(
            "batch-toggle",
            "finish_selection",
            "check",
            "primary",
            busy,
        ));
    } else {
        if !a.library_ui.show_favorites {
            if a.collection.is_empty() || !a.library.collection_parents.contains_key(&a.collection)
            {
                h.push_str(&icon_button(
                    "folder-new",
                    &a.collection,
                    a.t(if a.collection.is_empty() {
                        "new_collection"
                    } else {
                        "new_child_collection"
                    }),
                    "create_new_folder",
                    "primary",
                    busy,
                ));
            }
            if !a.collection.is_empty() {
                h.push_str(&b("folder-edit", "edit_collection", "edit", "quiet", busy));
            }
        }
        h.push_str("<span class=\"bar-spacer\"></span>");
        h.push_str(&b(
            "batch-toggle",
            "select_characters",
            "checklist",
            "secondary",
            busy,
        ));
        if galfriends_core::collections::accepts_characters(&a.library, &a.collection) {
            h.push_str(&b(
                "folder-add-characters",
                "add_characters",
                "plus",
                "primary",
                busy,
            ));
        }
    }
    h.push_str("</div>");
    h
}
pub(super) fn page(a: &App) -> String {
    let mut h = String::from(r#"<section class="library-page collections-page">"#);
    if !a.collection.is_empty() || a.library_ui.show_favorites {
        h.push_str(&format!(r#"<nav class="folder-breadcrumbs" aria-label="{}"><button data-action="folder-open" data-value="">{}</button>"#,a.t("collection_path"),a.t("favorites")));
        if let Some(parent) = a.library.collection_parents.get(&a.collection) {
            h.push_str(&format!(r#"<span aria-hidden="true">/</span><button data-action="folder-open" data-value="{}">{}</button>"#,esc(parent),esc(parent)));
        }
        h.push_str(&format!(
            r#"<span aria-hidden="true">/</span><span aria-current="page">{}</span></nav>"#,
            if a.library_ui.show_favorites {
                a.t("starred").into()
            } else {
                esc(&a.collection)
            }
        ));
    }
    h.push_str(&toolbar(a));
    h.push_str("<div class=\"folder-grid\">");
    if a.collection.is_empty() && !a.library_ui.show_favorites {
        h.push_str(&format!(r#"<article class="folder-card"><button class="folder-face" data-action="favorites-open">{}<strong>{}</strong></button></article>"#,icon("folder"),a.t("starred")));
    }
    for name in children(a) {
        let selected = a.library_ui.checked_folders.contains(name);
        write!(h, r#"<article class="folder-card {}" data-folder-id="{}"><button class="folder-face" data-action="{}" data-value="{}" {}>{}<strong>{}</strong>{}</button></article>"#,
            if selected { "is-selected" } else { "" }, esc(name),
            if a.library_ui.batch { "folder-select" } else { "folder-open" }, esc(name),
            if a.library_ui.batch { format!(r#"aria-pressed="{selected}""#) } else { String::new() },
            icon("folder"), esc(name), super::selection::check()).unwrap();
    }
    h.push_str("</div>");
    if !a.collection.is_empty() || a.library_ui.show_favorites {
        h.push_str(&super::cards(a));
    }
    h.push_str("</section>");
    h
}
pub(super) fn picker(a: &App, query: &str, ids: &BTreeSet<String>, single: bool) -> String {
    let busy = a.pending.is_some();
    let disabled = if busy { "disabled" } else { "" };
    let selected = a
        .t("collection_selected_count")
        .replace("{count}", &ids.len().to_string());
    let mut h = super::dialog_start(a, "in_collection", busy);
    write!(
        h,
        r#"<p class="muted">{selected}</p><label class="edit-field"><span>{}</span><input id="folder-query" data-input="folder-query" inputmode="search" value="{}" autocomplete="off" aria-controls="folder-picker-options" data-initial-focus></label><div id="folder-picker-options" class="folder-picker">"#,
        a.t("query"),
        esc(query),
    ).unwrap();
    if single && let Some(c) = a.library.characters.iter().find(|c| ids.contains(&c.id)) {
        let action = a.t(if c.favorite {
            "unfavorite_action"
        } else {
            "favorite"
        });
        let label = format!("{} · {action}", a.t("starred"));
        if label.to_lowercase().contains(&query.trim().to_lowercase()) {
            write!(h, r#"<button type="button" class="button folder-picker-option {}" data-action="collection-star" {disabled}>{}<span>{}</span></button>"#,
                if c.favorite { "is-starred" } else { "" }, icon("heart"), esc(&label)).unwrap();
        }
    }
    let query = query.trim().to_lowercase();
    let mut options: Vec<_> = a
        .library
        .collections
        .keys()
        .filter(|name| accepts_characters(&a.library, name))
        .map(|name| {
            let path = match a.library.collection_parents.get(name) {
                Some(parent) => format!("{parent} / {name}"),
                None => name.clone(),
            };
            (path, name)
        })
        .filter(|(path, _)| path.to_lowercase().contains(&query))
        .collect();
    options.sort_unstable();

    if options.is_empty() {
        h.push_str(&format!(
            r#"<p class="muted">{}</p>"#,
            a.t(if a.library.collections.is_empty() {
                "collection_picker_empty"
            } else {
                "collection_picker_no_matches"
            })
        ));
    }
    for (path, name) in options {
        h.push_str(&format!(
            r#"<button type="button" class="button folder-picker-option" data-action="collection-pick" data-value="{}" {disabled}>{}<span>{}</span></button>"#,
            esc(name),
            icon("folder_special"),
            esc(&path),
        ));
    }
    h.push_str(&format!(
        r#"</div></div><footer><button type="button" class="button quiet" data-action="library-panel-close" {disabled}>{}<span>{}</span></button></footer></section></div>"#,
        icon("close"),
        a.t("close"),
    ));
    h
}

pub(super) fn panel(a: &App) -> String {
    let title = match &a.library_ui.panel {
        Panel::FolderEdit { original, .. } => {
            if original.is_some() {
                "edit_collection"
            } else {
                "new_collection"
            }
        }
        Panel::FolderDelete { .. } => "delete_collections",
        Panel::AddCharacters => "add_characters",
        _ => return String::new(),
    };
    let busy = a.pending.is_some();
    let mut h = format!(
        r#"<div class="editor-overlay"><section class="editor-dialog small-dialog" role="dialog" aria-modal="true" aria-labelledby="collection-dialog-title"><header class="section-heading"><h2 id="collection-dialog-title">{}</h2>{}</header>"#,
        a.t(title),
        if matches!(a.library_ui.panel, Panel::FolderDelete { .. }) {
            String::new()
        } else {
            icon_button(
                "library-panel-close",
                "",
                a.t("close"),
                "close",
                "quiet",
                busy,
            )
        }
    );
    match &a.library_ui.panel {
        Panel::FolderEdit { original, parent } => {
            h.push_str(&format!(r#"<form data-submit="folder-save" class="folder-editor"><label for="folder-name">{}</label><input id="folder-name" data-input="collection-name" value="{}" maxlength="60" required><label for="folder-parent">{}</label><select id="folder-parent" data-input="folder-parent"><option value="">{}</option>"#,a.t("collection_name"),esc(&a.collection_draft),a.t("parent_collection"),a.t("collection_root")));
            let has_children = original
                .as_ref()
                .is_some_and(|old| a.library.collection_parents.values().any(|p| p == old));
            if !has_children {
                for name in a.library.collections.keys().filter(|n| {
                    !a.library.collection_parents.contains_key(*n) && original.as_ref() != Some(*n)
                }) {
                    h.push_str(&format!(
                        r#"<option value="{}" {}>{}</option>"#,
                        esc(name),
                        if name == parent { "selected" } else { "" },
                        esc(name)
                    ));
                }
            }
            h.push_str(&format!(r#"</select><footer class="dialog-actions"><button class="button primary" type="submit" {}>{} {}</button></footer></form>"#,if busy {"disabled"}else{""},icon("save"),a.t("save")));
        }
        Panel::FolderDelete { names } => {
            h.push_str(&format!("<p>{}</p><ul>", a.t("delete_collections_hint")));
            for name in names {
                h.push_str(&format!("<li>{}</li>", esc(name)));
            }
            h.push_str("</ul>");
            h.push_str(&confirmation_actions(
                "folders-delete-confirm",
                "library-panel-close",
                a.t("confirm"),
                a.t("cancel"),
                busy,
            ));
        }
        Panel::AddCharacters => {
            h.push_str(&format!(r#"<input id="picker-query" data-input="picker-query" type="search" value="{}" placeholder="{}" aria-label="{}"><div class="game-membership-picker">"#,esc(&a.library_ui.picker_query),a.t("all_names"),a.t("query")));
            let query = a.library_ui.picker_query.to_lowercase();
            let mut count = 0;
            for c in &a.library.characters {
                if a.library
                    .collections
                    .get(&a.collection)
                    .is_some_and(|ids| ids.contains(&c.id))
                    || !(c.name.to_lowercase().contains(&query)
                        || c.aliases
                            .iter()
                            .any(|n| n.name.to_lowercase().contains(&query)))
                {
                    continue;
                }
                count += 1;
                let selected = a.library_ui.picker.contains(&c.id);
                h.push_str(&format!(r#"<button class="membership-option {}" data-action="picker-character" data-value="{}" aria-pressed="{}"><span class="membership-check">{}</span><span>{}</span></button>"#,if selected {"selected"}else{""},esc(&c.id),selected,if selected {icon("check")}else{String::new()},esc(&c.name)));
            }
            if count == 0 {
                h.push_str(&format!("<p class=\"muted\">{}</p>", a.t("no_candidates")));
            }
            h.push_str(&format!(r#"</div><footer class="dialog-actions"><button class="button primary" data-action="picker-save" {}>{} {}</button></footer>"#,if busy||a.library_ui.picker.is_empty(){"disabled"}else{""},icon("plus"),a.t("add_characters")));
        }
        _ => {}
    }
    h.push_str("</section></div>");
    h
}
