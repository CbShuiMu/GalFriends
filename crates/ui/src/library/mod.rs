//! 角色卡片和库页面；工具栏动作、查询及草稿各自维护。
pub(crate) mod actions;
mod collection_actions;
mod collections;
pub(crate) mod order;
mod query;
mod selection;
pub(crate) mod state;
mod tag_filter;
mod toolbar;
use crate::{
    state::{App, Page},
    widgets::{esc, favorite_button, icon},
};
use galfriends_core::characters::Character;

pub use order::library_drag_ids;
pub use selection::library_marquee;
pub(crate) use tag_filter::set_tag_query;

fn dialog_start(a: &App, title: &str, busy: bool) -> String {
    format!(
        r#"<div class="editor-overlay library-overlay"><section class="editor-dialog library-dialog" role="dialog" aria-modal="true" aria-labelledby="library-dialog-title"><header><h2 id="library-dialog-title">{}</h2><button type="button" class="icon-button" data-action="library-panel-close" title="{}" aria-label="{}" {}>{}</button></header><div class="library-dialog-body">"#,
        a.t(title),
        a.t("close"),
        a.t("close"),
        if busy { "disabled" } else { "" },
        icon("close")
    )
}

pub(crate) fn favorite_control(a: &App, c: &Character) -> String {
    let active = c.favorite
        || a.library
            .collections
            .values()
            .any(|ids| ids.contains(&c.id));
    favorite_button(&c.id, active, a.t("favorite"), a.pending.is_some())
}

fn poster(a: &App, c: &Character) -> String {
    a.artwork_selection
        .artwork(c)
        .map(|a| {
            format!(
                r#"<img src="{}" alt="" loading="lazy">"#,
                esc(a.enhanced.as_deref().unwrap_or(&a.url))
            )
        })
        .unwrap_or_else(|| {
            format!(
                r#"<span class="portrait-placeholder">{}</span>"#,
                icon("person")
            )
        })
}
pub(crate) fn home_card(a: &App, c: &Character, selected: bool) -> String {
    format!(
        r#"<button class="home-game {}" data-action="select" data-value="{}" aria-label="{}" aria-pressed="{}"><span class="poster">{}</span></button>"#,
        if selected { "selected" } else { "" },
        esc(&c.id),
        esc(&c.name),
        selected,
        poster(a, c)
    )
}
pub(crate) fn card(a: &App, c: &Character, selected: bool) -> String {
    let face_action = if a.library_ui.batch {
        "batch-check"
    } else {
        "detail"
    };
    let overlay = if a.library_ui.batch {
        String::new()
    } else {
        format!(
            r#"<div class="card-overlay"><div class="card-rail"><div class="card-heading"><h3><button class="card-title" data-action="detail" data-value="{}" title="{}">{}</button></h3></div><div class="card-actions"><button type="button" class="card-action companion" data-action="companion-open" data-value="{}" title="{}" aria-label="{}" {}>{}</button>{}<button class="card-action delete" data-action="card-remove" data-value="{}" title="{}" aria-label="{}" {}>{}</button></div></div></div>"#,
            esc(&c.id),
            esc(&c.name),
            esc(&c.name),
            esc(&c.id),
            a.t("companion.open"),
            a.t("companion.open"),
            if a.pending.is_some() { "disabled" } else { "" },
            icon("mic"),
            favorite_control(a, c),
            esc(&c.id),
            a.t("remove"),
            a.t("remove"),
            if a.pending.is_some() { "disabled" } else { "" },
            icon("trash")
        )
    };
    format!(
        r#"<article class="game-card {} {}" data-character-id="{}"><button class="game-face" data-action="{face_action}" data-value="{}" aria-label="{}" {} {}><span class="poster">{}{}</span></button>{overlay}</article>"#,
        if selected { "is-selected" } else { "" },
        if a.library_ui.batch {
            "is-selectable"
        } else {
            ""
        },
        esc(&c.id),
        esc(&c.id),
        esc(&c.name),
        if order::enabled(a) {
            format!(
                r#"title="{}" aria-description="{}""#,
                esc(a.t("library_reorder_hint")),
                esc(a.t("library_reorder_hint"))
            )
        } else {
            String::new()
        },
        if a.library_ui.batch {
            format!(r#"aria-pressed="{selected}""#)
        } else {
            String::new()
        },
        poster(a, c),
        selection::check()
    )
}
pub(crate) fn page(a: &App) -> String {
    if a.page == Page::Favorites {
        return collections::page(a);
    }
    let chars = query::visible(a);
    let mut html = format!(
        r#"<section class="library-page">{}"#,
        toolbar::render(a, chars.len())
    );
    html.push_str(&cards(a));
    html.push_str("</section>");
    html
}
fn cards(a: &App) -> String {
    let chars = query::visible(a);
    let mut html = format!(
        r#"<div class="game-grid" data-custom-sort="{}">"#,
        order::enabled(a)
    );
    for c in &chars {
        html.push_str(&card(a, c, a.library_ui.checked.contains(&c.id)));
    }
    html.push_str("</div>");
    if chars.is_empty() {
        let filtered = !a.filter.is_empty() || !a.library_ui.tags.is_empty();
        html.push_str(&format!(
            r#"<div class="empty-state">{}<p>{}</p></div>"#,
            icon("person"),
            a.t(if filtered {
                "no_local_results"
            } else if a.page == Page::Favorites {
                "empty_collection"
            } else {
                "empty"
            })
        ));
    }
    html
}
pub(crate) fn panel(a: &App) -> String {
    match &a.library_ui.panel {
        state::Panel::Collections { query, ids, single } => {
            collections::picker(a, query, ids, *single)
        }
        state::Panel::FolderEdit { .. }
        | state::Panel::FolderDelete { .. }
        | state::Panel::AddCharacters => collections::panel(a),
        state::Panel::Tags { selected, query } => tag_filter::panel(a, selected, query),
        state::Panel::Closed => String::new(),
    }
}
