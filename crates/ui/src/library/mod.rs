//! 角色卡片和库页面；工具栏动作、查询及草稿各自维护。
pub(crate) mod actions;
mod collection_actions;
mod collections;
mod query;
pub(crate) mod state;
mod toolbar;
use crate::{
    state::{App, Page},
    widgets::{esc, favorite_button, icon},
};
use galfriends_core::characters::Character;

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
            r#"<div class="card-overlay"><div class="card-rail"><div class="card-heading"><h3><button class="card-title" data-action="detail" data-value="{}" title="{}">{}</button></h3></div><div class="card-actions">{}<button class="card-action delete" data-action="card-remove" data-value="{}" title="{}" aria-label="{}" {}>{}</button></div></div></div>"#,
            esc(&c.id),
            esc(&c.name),
            esc(&c.name),
            favorite_button(
                &c.id,
                c.favorite,
                a.t(if c.favorite {
                    "unfavorite_action"
                } else {
                    "favorite"
                }),
                a.pending.is_some()
            ),
            esc(&c.id),
            a.t("remove"),
            a.t("remove"),
            if a.pending.is_some() { "disabled" } else { "" },
            icon("trash")
        )
    };
    format!(
        r#"<article class="game-card {} {}"><button class="game-face" data-action="{face_action}" data-value="{}" aria-label="{}" aria-pressed="{selected}"><span class="poster">{}</span></button>{overlay}</article>"#,
        if selected { "selected" } else { "" },
        if a.library_ui.batch {
            "is-selectable"
        } else {
            ""
        },
        esc(&c.id),
        esc(&c.name),
        poster(a, c)
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
    let mut html = String::from(r#"<div class="game-grid">"#);
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
        state::Panel::Collections { query } => collections::picker(a, query),
        state::Panel::FolderEdit { .. }
        | state::Panel::FolderDelete { .. }
        | state::Panel::AddCharacters => collections::panel(a),
        state::Panel::Tags => toolbar::tag_panel(a),
        state::Panel::Closed => String::new(),
    }
}
