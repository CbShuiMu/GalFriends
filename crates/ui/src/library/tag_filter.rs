//! 标签筛选弹窗保存独立草稿，确认后更新角色库。
use super::{query, state::Panel};
use crate::{
    state::App,
    widgets::{esc, icon},
};
use std::{collections::BTreeSet, fmt::Write};

pub(crate) fn set_tag_query(a: &mut App, value: &str) -> bool {
    let Panel::Tags { query, .. } = &mut a.library_ui.panel else {
        return false;
    };
    *query = value.into();
    true
}

pub(super) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    match action {
        "tag-filter-open" => {
            a.library_ui.sort_open = false;
            a.library_ui.panel = Panel::Tags {
                selected: a.library_ui.tags.clone(),
                query: String::new(),
            };
        }
        "tag-filter-add" => {
            if query::available_tags(a).contains(value)
                && let Panel::Tags { selected, query } = &mut a.library_ui.panel
            {
                selected.insert(value.into());
                query.clear();
            }
        }
        "tag-filter-remove" => {
            if let Panel::Tags { selected, .. } = &mut a.library_ui.panel {
                selected.remove(value);
            }
        }
        "tag-filter-apply" => {
            if let Panel::Tags { selected, .. } = &mut a.library_ui.panel {
                a.library_ui.tags = std::mem::take(selected);
                a.library_ui.checked.clear();
                a.library_ui.panel = Panel::Closed;
            }
        }
        "tag-clear" => {
            a.library_ui.tags.clear();
            a.library_ui.checked.clear();
        }
        _ => return false,
    }
    true
}

pub(super) fn panel(a: &App, selected: &BTreeSet<String>, query: &str) -> String {
    let mut h = super::dialog_start(a, "filter_tags", false);
    write!(h, r#"<div class="tag-search"><label class="edit-field"><span>{}</span><input id="tag-query" data-input="tag-query" inputmode="search" value="{}" placeholder="{}" autocomplete="off" data-initial-focus {}></label>"#,
        a.t("query"), esc(query), a.t("tag_filter.search"), if query.trim().is_empty() { "" } else { r#"aria-controls="tag-query-results""# }).unwrap();
    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        let mut matches: Vec<_> = query::available_tags(a)
            .into_iter()
            .filter(|name| !selected.contains(*name) && name.to_lowercase().contains(&query))
            .collect();
        matches.sort_by_cached_key(|name| name.to_lowercase());
        h.push_str(r#"<div id="tag-query-results" class="tag-search-results">"#);
        if matches.is_empty() {
            write!(h, r#"<p class="muted">{}</p>"#, a.t("tag_filter.no_match")).unwrap();
        }
        for name in matches {
            write!(h, r#"<button type="button" class="button quiet tag-search-option" data-action="tag-filter-add" data-value="{}">{}</button>"#, esc(name), esc(name)).unwrap();
        }
        h.push_str("</div>");
    }
    h.push_str("</div>");
    if !selected.is_empty() {
        write!(
            h,
            r#"<div class="tag-filter-selected" aria-label="{}">"#,
            a.t("tag_filter.selected")
        )
        .unwrap();
        for name in selected {
            let label = a.t("tag_filter.remove").replace("{name}", name);
            write!(h, r#"<span class="selected-tag"><span>{}</span><button type="button" class="icon-button" data-action="tag-filter-remove" data-value="{}" aria-label="{}" title="{}">{}</button></span>"#, esc(name), esc(name), esc(&label), esc(&label), icon("close")).unwrap();
        }
        h.push_str("</div>");
    }
    write!(h, r#"<p class="tag-filter-help muted">{}</p></div><footer><button type="button" class="button quiet" data-action="library-panel-close">{}<span>{}</span></button><button type="button" class="button primary" data-action="tag-filter-apply">{}<span>{}</span></button></footer></section></div>"#,
        a.t("tag_filter.help"), icon("close"), a.t("cancel"), icon("check"), a.t("confirm")).unwrap();
    h
}

#[cfg(test)]
mod tests {
    use crate::{library::actions::handle, state::App};
    use galfriends_core::characters::Character;

    #[test]
    fn tag_filter_draft_only_changes_the_library_after_confirmation() {
        let mut a = App::default();
        let mut c = Character::default();
        c.tag_presentation.added.push("Alpha".into());
        a.library.characters.push(c);
        a.library_ui.tags.insert("Existing".into());

        handle(&mut a, "tag-filter-open", "");
        handle(&mut a, "tag-filter-remove", "Existing");
        handle(&mut a, "tag-filter-add", "Alpha");
        handle(&mut a, "library-panel-close", "");
        assert_eq!(a.library_ui.tags, ["Existing".into()].into());

        handle(&mut a, "tag-filter-open", "");
        handle(&mut a, "tag-filter-remove", "Existing");
        handle(&mut a, "tag-filter-add", "invalid");
        handle(&mut a, "tag-filter-add", "Alpha");
        handle(&mut a, "tag-filter-apply", "");
        assert_eq!(a.library_ui.tags, ["Alpha".into()].into());
    }
}
