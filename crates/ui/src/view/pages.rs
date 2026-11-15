use crate::{
    library,
    state::App,
    widgets::{esc, favorite_button, icon, icon_button, source_icon},
};
pub(super) fn home(a: &App) -> String {
    let Some(c) = a.current() else {
        return format!(
            r#"<div class="empty-state"><p>{}</p><button class="button primary" data-action="search">{}</button></div>"#,
            a.t("empty"),
            a.t("search")
        );
    };
    // 只输出目标 URL；DOM 保留已解码图层，直到下一张立绘完成过渡。
    let cover = a
        .artwork_selection
        .artwork(c)
        .map(|p| p.enhanced.as_deref().unwrap_or(&p.url))
        .unwrap_or_default();
    let mut shelf = String::new();
    for character in &a.library.characters {
        shelf.push_str(&library::home_card(a, character, character.id == c.id));
    }
    format!(
        r#"<section class="character-home" data-character="{}"><div class="home-backdrop" data-cover="{}" aria-hidden="true"></div><div class="home-shelf" data-focus-scroll="managed">{shelf}</div><div class="home-feature"><div class="home-copy"><div class="home-name-reading">{}</div><h1>{}</h1><p class="home-description">{}</p><div class="actions"><button class="button primary" data-action="detail" data-value="{}">{} {}</button>{}</div></div></div></section>"#,
        esc(&c.id),
        esc(cover),
        esc(c
            .aliases
            .iter()
            .find(|n| n.kind == "罗马字" || n.kind == "romanized")
            .map(|n| n.name.as_str())
            .unwrap_or_default()),
        esc(&c.name),
        esc(&c.summary),
        esc(&c.id),
        icon("person"),
        a.t("details"),
        favorite_button(
            &c.id,
            c.favorite,
            a.t(if c.favorite {
                "unfavorite_action"
            } else {
                "favorite"
            }),
            a.pending.is_some()
        )
    )
}
pub(super) fn about(a: &App) -> String {
    format!(
        r#"<section class="about-page"><div class="about-mark">{}</div><h1>GalFriends</h1><p class="version">0.1.0</p><p>{}</p><hr><p>{}</p><p>{}</p></section>"#,
        crate::widgets::brand(),
        a.t("about_text"),
        a.t("credits"),
        a.t("sample")
    )
}
pub(super) fn search(a: &App) -> String {
    let disabled = if a.pending.is_some() { "disabled" } else { "" };
    let mut h = format!(
        r#"<div class="editor-overlay"><section class="editor-dialog search-dialog" role="dialog" aria-modal="true" aria-labelledby="search-title"><header class="section-heading"><h2 id="search-title">{}</h2>{}</header><p class="muted">{}</p><form data-submit="query" class="search-form"><div id="search-source" class="segmented-control search-sources" role="group" aria-label="{}"><span class="segmented-thumb" aria-hidden="true"></span>"#,
        a.t(if a.tag_source_target.is_some() {
            "tags.replace_source"
        } else if a.link_target.is_some() {
            "link"
        } else {
            "search"
        }),
        icon_button("close-search", "", a.t("close"), "close", "quiet", false),
        a.t(if a.tag_source_target.is_some() {
            "tags.source_hint"
        } else if a.link_target.is_some() {
            "link_hint"
        } else {
            "sources_hint"
        }),
        a.t("source")
    );
    for (v, label) in [
        ("bangumi", "Bangumi"),
        ("kungal", a.t("prefs.kungal")),
        ("vndb", "VNDB"),
        ("erogamescape", a.t("prefs.erogamescape")),
    ] {
        if a.tag_source_target.is_some() && !matches!(v, "kungal" | "vndb") {
            continue;
        }
        let selected = a.search_source == v;
        h.push_str(&format!(
            r#"<button type="button" class="segment" data-action="search-source" data-value="{v}" aria-pressed="{selected}" tabindex="{}" {disabled}>{}{}</button>"#,
            if selected { "0" } else { "-1" },
            source_icon(v),
            esc(label)
        ));
    }
    h.push_str(&format!(
        r#"</div><div class="search-query-row"><input id="character-query" data-input="query" data-initial-focus value="{}" placeholder="{}" aria-label="{}" maxlength="200" required><button class="button primary icon-only" type="submit" title="{}" aria-label="{}" {disabled}>{}</button></div></form><div class="search-results" aria-busy="{}">"#,
        esc(&a.query), a.t("search_hint"), a.t("search"), a.t("query"), a.t("query"), icon("search"), a.pending.is_some()
    ));
    if a.search_source == "erogamescape" {
        h.push_str(&format!("<p class=\"muted\">{}</p>", a.t("works_only")));
    }
    for r in &a.results {
        let works = r.source == galfriends_core::characters::Source::Erogamescape;
        let portrait = r.portrait.as_deref().map(|url| format!(
            r#"<img src="{}" alt="" loading="lazy" decoding="async" referrerpolicy="no-referrer">"#, esc(url)
        )).unwrap_or_default();
        let romanized = r
            .romanized_name
            .as_deref()
            .map(|name| format!(r#"<p class="result-reading">{}</p>"#, esc(name)))
            .unwrap_or_default();
        h.push_str(&format!(r#"<article class="result-row"><span class="result-symbol" aria-hidden="true">{}{portrait}</span><div><h3>{}</h3>{romanized}<p class="result-source">{} · {}</p></div>{}</article>"#,icon(if works{"games"}else{"person"}),esc(&r.name),r.source.key(),esc(&r.id),icon_button(if works{"open"}else{"import"},&format!("{}|{}",r.source.key(),r.id),a.t(if works{"open_source"}else if a.tag_source_target.is_some(){"tags.replace_source"}else if a.link_target.is_some(){"link"}else{"add"}),if works{"open_in_new"}else{"plus"},"secondary",a.pending.is_some())));
    }
    if a.searched && a.results.is_empty() {
        h.push_str(&format!(
            "<p class=\"empty-state\">{}</p>",
            a.t("no_results")
        ));
    }
    h.push_str("</div></section></div>");
    h
}

#[cfg(test)]
mod tests {
    use crate::state::App;
    use galfriends_core::characters::{Artwork, Candidate, Character};

    #[test]
    fn search_accepts_older_candidates_and_escapes_remote_portrait_attributes() {
        let candidate: Candidate =
            serde_json::from_str(r#"{"source":"bangumi","id":"1","name":"<角色>","subtitle":""}"#)
                .unwrap();
        assert!(candidate.portrait.is_none());
        assert!(candidate.romanized_name.is_none());
        let mut app = App {
            search_source: "bangumi".into(),
            results: vec![candidate],
            ..Default::default()
        };
        assert!(!super::search(&app).contains("referrerpolicy"));
        assert!(!super::search(&app).contains("result-reading"));
        app.results[0].portrait = Some("https://lain.bgm.tv/pic/test.jpg?a=\"&b=1".into());
        app.results[0].romanized_name = Some("Shindou <Yuuka> & Sister".into());
        let html = super::search(&app);
        assert!(html.contains("&lt;角色&gt;"));
        assert!(
            html.contains(r#"<p class="result-reading">Shindou &lt;Yuuka&gt; &amp; Sister</p>"#)
        );
        assert!(html.contains(r#"src="https://lain.bgm.tv/pic/test.jpg?a=&quot;&amp;b=1""#));
        assert!(html.contains(r#"data-action="import" data-value="bangumi|1""#));
    }

    #[test]
    fn home_keeps_a_selection_beyond_the_first_eight_characters_reachable() {
        let mut app = App {
            selected: "character-11".into(),
            ..Default::default()
        };
        app.library.characters = (0..12)
            .map(|index| Character {
                id: format!("character-{index}"),
                name: format!("角色 {index}"),
                ..Default::default()
            })
            .collect();
        let html = super::home(&app);
        assert_eq!(html.matches("data-action=\"select\"").count(), 12);
        assert!(html.contains(
            r#"class="home-game selected" data-action="select" data-value="character-11""#
        ));
    }

    #[test]
    fn home_and_library_cards_follow_the_selected_artwork_and_its_enhancement() {
        let mut app = App::default();
        app.library.characters.push(Character {
            id: "sample".into(),
            artworks: vec![
                Artwork {
                    url: "initial.png".into(),
                    ..Default::default()
                },
                Artwork {
                    url: "selected.png".into(),
                    enhanced: Some("enhanced.png?key=\"&value=1".into()),
                    ..Default::default()
                },
            ],
            selected_art: 1,
            ..Default::default()
        });
        let html = super::home(&app);
        assert!(html.contains(r#"data-cover="enhanced.png?key=&quot;&amp;value=1""#));
        assert!(html.contains(r#"<img src="enhanced.png?key=&quot;&amp;value=1""#));
        assert!(!html.contains(r#"src="initial.png""#));
        assert!(!html.contains(r#"src="selected.png""#));
        let card = crate::library::card(&app, &app.library.characters[0], false);
        assert!(card.contains(r#"<img src="enhanced.png?key=&quot;&amp;value=1""#));

        app.library.characters[0].selected_art = 0;
        let html = super::home(&app);
        assert!(html.contains(r#"data-cover="initial.png""#));
        assert!(html.contains(r#"<img src="initial.png""#));
        let card = crate::library::card(&app, &app.library.characters[0], true);
        assert!(card.contains(r#"<img src="initial.png""#));
    }
}
