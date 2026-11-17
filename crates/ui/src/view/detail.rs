use crate::{
    state::App,
    widgets::{esc, icon_button},
};
pub(super) fn page(a: &App) -> String {
    let Some(c) = a.current() else {
        return String::new();
    };
    let mut h = format!(
        r#"<header class="detail-heading">{}</header><div class="character-detail reference-detail" data-detail-game="{}"><aside class="detail-art">"#,
        icon_button("back", "", a.t("back"), "back", "quiet", false),
        esc(&c.id)
    );
    if let Some(art) = a.artwork_selection.artwork(c) {
        h.push_str(&icon_button(
            "expression-open",
            "",
            a.t("expression.replace"),
            "face",
            "detail-expression-button",
            a.pending.is_some(),
        ));
        h.push_str(&format!(
            r#"<img src="{}" alt="{}"><p class="muted">{}</p>"#,
            esc(art.enhanced.as_deref().unwrap_or(&art.url)),
            esc(&c.name),
            esc(&art.title)
        ));
    } else {
        h.push_str(&format!("<p>{}</p>", a.t("no_art")));
    }
    if a.artwork_selection.artwork(c).is_some() {
        h.push_str(&icon_button(
            "reprocess-artworks",
            "",
            a.t("reprocess_artworks"),
            "refresh",
            "secondary",
            a.pending.is_some(),
        ));
        h.push_str(&icon_button(
            "enhance",
            "",
            a.t("enhance"),
            "image",
            "secondary",
            a.pending.is_some(),
        ));
    }
    h.push_str(&format!(
        r#"</aside><div class="profile-content"><div class="profile-heading"><h1>{}</h1>{}</div><section><h2>{}</h2><dl class="aliases">"#,
        esc(&c.name),
        icon_button(
            "companion-open",
            &c.id,
            a.t("companion.open"),
            "mic",
            "quiet",
            a.pending.is_some()
        ),
        a.t("aliases")
    ));
    for alias in &c.aliases {
        if !alias.name.is_empty() {
            h.push_str(&format!(
                "<div><dt>{}</dt><dd>{}</dd></div>",
                esc(&alias.kind),
                esc(&alias.name)
            ));
        }
    }
    h.push_str(&format!(
        r#"</dl></section><section><h2>{}</h2><p class="biography">{}</p></section>"#,
        a.t("summary"),
        esc(&c.summary)
    ));
    h.push_str(&crate::detail_tags::render(a));
    h.push_str(&format!(r#"</div></div><section class="album"><h2>{} <span class="count-badge">{}</span></h2><div class="album-grid">"#,a.t("portraits"),c.artworks.len()));
    for (i, art) in c.artworks.iter().enumerate().filter(|(_, p)| !p.spoiler) {
        h.push_str(&format!(r#"<button class="album-item {}" data-action="art" data-value="{i}" aria-pressed="{}"><img src="{}" alt="{}" loading="lazy"><span>{}</span></button>"#,if i==a.artwork_selection.selected(c){"selected"}else{""},i==a.artwork_selection.selected(c),esc(&art.url),esc(&art.title),esc(&art.title)));
    }
    h.push_str("</div></section>");
    h.push_str(&crate::game_library::detail(a));
    h
}
