use crate::{
    library::{query, state::Sort},
    state::App,
    widgets::{esc, icon, icon_button},
};

pub(super) fn render(a: &App, count: usize) -> String {
    let busy = a.pending.is_some();
    let b = |action: &str, label: &str, symbol: &str, class: &str, disabled: bool| {
        icon_button(action, "", a.t(label), symbol, class, disabled)
    };
    let mut h = format!(
        r#"<div class="library-bar {}" role="toolbar" aria-label="{}">"#,
        if a.library_ui.batch { "selecting" } else { "" },
        a.t("library")
    );
    if a.library_ui.batch {
        let empty = a.library_ui.checked.is_empty();
        h.push_str(&format!(
            r#"<span class="selection-total">{}<strong>{}</strong> {}</span>"#,
            icon("check"),
            a.library_ui.checked.len(),
            a.t("selected")
        ));
        h.push_str(&b(
            "batch-all",
            "select_all",
            "done_all",
            "quiet",
            busy || count == 0,
        ));
        h.push_str(&b(
            "batch-clear",
            "clear_selection",
            "close",
            "quiet",
            busy || empty,
        ));
        h.push_str("<span class=\"bar-spacer\"></span>");
        h.push_str(&b(
            "batch-collection",
            "in_collection",
            "folder_special",
            "secondary",
            busy || empty,
        ));
        if !a.collection.is_empty() {
            h.push_str(&icon_button(
                "batch-detach",
                &a.collection,
                a.t("remove_collection"),
                "close",
                "secondary",
                busy || empty,
            ));
        }
        h.push_str(&b(
            "batch-remove",
            "remove",
            "trash",
            "quiet danger",
            busy || empty,
        ));
        h.push_str(&b(
            "batch-toggle",
            "finish_selection",
            "check",
            "primary",
            busy,
        ));
    } else {
        h.push_str(&format!(r#"<div class="library-stats"><span><strong>{}</strong>{}</span></div><div class="bar-controls"><label class="bar-search {}" for="library-filter" title="{}">{}<input id="library-filter" type="search" data-input="filter" value="{}" placeholder="{}" aria-label="{}" autocomplete="off"></label>"#,if count==a.library.characters.len(){count.to_string()}else{format!("{count} / {}",a.library.characters.len())},a.t("character_count"),if a.filter.is_empty(){""}else{"has-query"},a.t("all_names"),icon("search"),esc(&a.filter),a.t("all_names"),a.t("query")));
        h.push_str(&format!(r#"<button class="button secondary tag-filter-trigger {}" data-action="tag-filter-open" title="{}">{} {}</button>"#,if a.library_ui.tags.is_empty(){""}else{"active"},a.t("filter_tags"),icon("filter_plus"),a.t("filter_tags")));
        if !a.library_ui.tags.is_empty() {
            h.push_str(&b(
                "tag-clear",
                "clear_filters",
                "filter_cancel",
                "quiet",
                false,
            ));
        }
        h.push_str(&b(
            "batch-toggle",
            "select_characters",
            "checklist",
            "secondary",
            busy,
        ));
        h.push_str(&format!(r#"<div class="add-menu" data-expanded="false"><button id="library-add-trigger" type="button" class="button primary icon-only add-trigger" aria-label="{}" title="{}" aria-expanded="false" aria-controls="library-add-options" {}>{}</button><div class="add-surface"><div id="library-add-options" class="add-options" inert>{}</div></div></div>"#,a.t("add_menu"),a.t("add_menu"),if busy {"disabled"}else{""},icon("plus"),icon_button("search","",a.t("import_character"),"person","quiet",busy)));
        h.push_str(&b(
            "refresh-library",
            "refresh_library",
            "refresh",
            "secondary",
            busy,
        ));
        h.push_str("<span class=\"sort-direction\">");
        h.push_str(&b(
            "sort-direction",
            if a.library_ui.descending {
                "sort_ascending"
            } else {
                "sort_descending"
            },
            if a.library_ui.descending {
                "arrow_downward"
            } else {
                "arrow_upward"
            },
            "quiet",
            false,
        ));
        h.push_str("</span>");
        h.push_str(&sort_control(a));
        h.push_str("</div>");
    }
    h.push_str("</div>");
    h
}
fn sort_control(a: &App) -> String {
    let items = [
        (Sort::Added, "added", "sort_added"),
        (Sort::Name, "name", "sort_name"),
        (Sort::Source, "source", "sort_source"),
    ];
    let current = items
        .iter()
        .find(|(sort, _, _)| *sort == a.library_ui.sort)
        .map(|(_, _, label)| a.t(label))
        .unwrap_or_default();
    let open = a.library_ui.sort_open;
    let mut h = format!(
        r#"<div class="choice-control"><button type="button" id="library-sort" class="choice-trigger {}" role="combobox" aria-label="{}" aria-expanded="{}" aria-haspopup="listbox" aria-controls="sort-options" data-action="sort-toggle"><span class="choice-copy"><span>{}</span></span>{}</button>"#,
        if open { "open" } else { "" },
        a.t("sort"),
        open,
        current,
        icon("chevron")
    );
    if open {
        h.push_str(&format!(
            r#"<div id="sort-options" class="choice-popup" role="listbox" aria-label="{}">"#,
            a.t("sort")
        ));
        for (sort, value, label) in items {
            let selected = a.library_ui.sort == sort;
            h.push_str(&format!(r#"<button class="choice-option {}" role="option" aria-selected="{}" data-action="sort-pick" data-value="{}">{}<span class="choice-check">{}</span></button>"#,if selected {"selected"}else{""},selected,value,a.t(label),if selected {icon("check")}else{String::new()}));
        }
        h.push_str("</div>");
    }
    h.push_str("</div>");
    h
}
pub(super) fn tag_panel(a: &App) -> String {
    let mut h = format!(
        r#"<div class="editor-overlay"><section class="editor-dialog small-dialog" role="dialog" aria-modal="true" aria-labelledby="library-panel-title"><header class="section-heading"><h2 id="library-panel-title">{}</h2>{}</header>"#,
        a.t("filter_tags"),
        icon_button(
            "library-panel-close",
            "",
            a.t("close"),
            "close",
            "quiet",
            false
        )
    );
    h.push_str(&format!(
        r#"<p class="muted">{} · {}</p><div class="tag-filter-options">"#,
        a.library.settings.effective_tag_source().key(),
        a.t("safe_tags")
    ));
    let available = query::available_tags(a);
    for tag in &available {
        let selected = a.library_ui.tags.contains(*tag);
        h.push_str(&format!(r#"<button class="filter-chip {}" data-action="tag-toggle" data-value="{}" aria-pressed="{}">{}</button>"#,if selected {"selected"}else{""},esc(tag),selected,esc(tag)));
    }
    if available.is_empty() {
        h.push_str(&format!("<p class=\"muted\">{}</p>", a.t("no_tags")));
    }
    h.push_str("</div></section></div>");
    h
}
