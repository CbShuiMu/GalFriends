use super::Section;
use crate::{
    state::App,
    widgets::{brand, esc, icon, icon_button, source_icon},
};

pub(crate) fn render(a: &App) -> String {
    let section = a.preferences.section;
    let mut html = format!(
        r#"<div class="preferences-workspace"><nav class="preferences-nav" aria-label="{}">"#,
        a.t("settings")
    );
    for item in [
        Section::Sources,
        Section::Ai,
        Section::Display,
        Section::System,
        Section::Paths,
        Section::About,
    ] {
        html.push_str(&format!(r#"<button class="preference-tab {}" data-action="settings-section" data-value="{}" {} {}><span>{}</span></button>"#,
            if item == section { "active" } else { "" }, item.key(),
            if item == section { "aria-current=\"page\"" } else { "" },
            disabled(a), a.t(item.label())));
    }
    html.push_str(&format!(r#"</nav><div class="preferences-content" role="region" aria-label="{}" data-settings-section="{}">{} </div></div>"#,
        a.t(section.label()), section.key(), match section {
            Section::Sources => super::connection_view::sources(a),
            Section::Ai => super::connection_view::ai(a),
            Section::Display => display(a),
            Section::System => system(a),
            Section::Paths => paths(a),
            Section::About => about(a),
        }));
    html
}

fn disabled(a: &App) -> &'static str {
    if a.pending.is_some() { "disabled" } else { "" }
}

pub(super) fn segmented(
    a: &App,
    key: &str,
    label: &str,
    current: &str,
    options: &[(&str, &str)],
) -> String {
    let mut html = format!(
        r#"<div id="settings-{key}" class="segmented-control" role="group" aria-label="{}"><span class="segmented-thumb" aria-hidden="true"></span>"#,
        esc(label)
    );
    for (value, text) in options {
        let selected = *value == current;
        html.push_str(&format!(r#"<button type="button" class="segment" data-action="settings-{key}" data-value="{value}" aria-pressed="{selected}" tabindex="{}" {}>{}{}</button>"#,
            if selected { "0" } else { "-1" }, disabled(a),
            if key.ends_with("source") { source_icon(value) } else { String::new() }, esc(text)));
    }
    html.push_str("</div>");
    html
}

fn row(label: &str, control: &str) -> String {
    format!(
        r#"<div class="preference-row"><h3>{}</h3>{control}</div>"#,
        esc(label)
    )
}

fn display(a: &App) -> String {
    let s = &a.library.settings;
    let font_size = s.font_size.to_string();
    let mut html = String::from(r#"<section class="settings-card">"#);
    for (key, label, current, options) in [
        (
            "language",
            a.t("language"),
            s.language.as_str(),
            vec![
                ("zh-CN", "简体中文"),
                ("ja-JP", "日本語"),
                ("en-US", "English"),
            ],
        ),
        (
            "font_size",
            a.t("prefs.font"),
            &font_size,
            vec![("14", "14px"), ("16", "16px"), ("18", "18px")],
        ),
    ] {
        if key == "font_size" {
            html.push_str(&appearance(a));
        }
        html.push_str(&row(label, &segmented(a, key, label, current, &options)));
    }
    html.push_str("</section>");
    html
}

pub(super) fn appearance(a: &App) -> String {
    let s = &a.library.settings;
    let mut html = String::new();
    for (key, label, current, options) in [
        (
            "color_theme",
            a.t("prefs.color"),
            &s.color_theme,
            vec![
                ("pink", a.t("prefs.pink")),
                ("white-pink", a.t("prefs.white_pink")),
                ("white", a.t("prefs.white")),
            ],
        ),
        (
            "theme",
            a.t("prefs.mode"),
            &s.theme,
            vec![("light", a.t("light")), ("dark", a.t("dark"))],
        ),
    ] {
        html.push_str(&row(label, &segmented(a, key, label, current, &options)));
    }
    html
}

fn system(a: &App) -> String {
    format!(
        r#"<section class="settings-card"><h2>{}</h2><p>{}</p><form class="settings-path-entry" data-submit="save-proxy"><input id="settings-proxy" data-input="proxy" value="{}" aria-label="{}" placeholder="http://127.0.0.1:7890" maxlength="300" autocomplete="off" spellcheck="false"><button type="submit" class="button icon-only" aria-label="{}" title="{}" {}>{}</button></form></section>"#,
        a.t("network"),
        a.t("proxy_hint"),
        esc(&a.proxy_draft),
        a.t("network"),
        a.t("save"),
        a.t("save"),
        disabled(a),
        icon("save")
    )
}

fn paths(a: &App) -> String {
    let Some(environment) = &a.preferences.environment else {
        return format!(
            r#"<section class="settings-card settings-environment-empty"><p>{}</p>{}</section>"#,
            a.t(if a.pending.is_some() {
                "loading"
            } else {
                "prefs.paths_unavailable"
            }),
            icon_button(
                "settings-environment",
                "",
                a.t("retry"),
                "refresh",
                "",
                a.pending.is_some()
            )
        );
    };
    let mut html = format!(
        r#"<section class="settings-card"><div class="preference-row preference-path"><label for="galgear-library-path">{}</label><form class="settings-path-entry" data-submit="save-game-library"><input id="galgear-library-path" data-input="galgear-library-path" value="{}" aria-describedby="galgear-library-hint" autocomplete="off" spellcheck="false" {}><button type="submit" class="button icon-only" aria-label="{}" title="{}" {}>{}</button></form><p id="galgear-library-hint">{}</p></div>"#,
        a.t("game_library.path"),
        esc(a
            .game_library
            .draft
            .as_deref()
            .unwrap_or(&a.library.settings.galgear_library_path)),
        disabled(a),
        a.t("save"),
        a.t("save"),
        disabled(a),
        icon("save"),
        a.t("game_library.path_hint")
    );
    for (key, path) in [
        ("prefs.library_path", &environment.library_path),
        ("prefs.cache_path", &environment.cache_path),
    ] {
        html.push_str(&format!(r#"<div class="preference-row preference-path"><label for="{key}">{}</label><input id="{key}" value="{}" readonly title="{}"></div>"#, a.t(key), esc(path), esc(path)));
    }
    html.push_str("</section>");
    html.push_str(&format!(r#"<section class="settings-card"><div class="settings-card-heading"><h2>waifu2x</h2><span class="tool-status">{}</span>{}</div><p>{}</p>"#,
        a.t(if environment.waifu2x_path.is_some() { "prefs.tool_found" } else { "prefs.tool_missing" }),
        icon_button("settings-environment", "", a.t("refresh"), "refresh", "", a.pending.is_some()), a.t("enhance_hint")));
    if let Some(path) = &environment.waifu2x_path {
        html.push_str(&format!(
            r#"<input class="tool-path" value="{}" readonly aria-label="{}" title="{}">"#,
            esc(path),
            a.t("prefs.tool_path"),
            esc(path)
        ));
    }
    html.push_str("</section>");
    html
}

fn about(a: &App) -> String {
    format!(
        r#"<section class="settings-card preferences-about"><div class="settings-about-brand">{}<div><h2>GalFriends</h2><p>0.1.0</p></div></div><p>{}</p><p>{}</p><p>{}</p></section>"#,
        brand(),
        a.t("about_text"),
        a.t("credits"),
        a.t("sample")
    )
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::{preferences::Section, state::App};
    use galfriends_core::preferences::Environment;

    #[test]
    fn every_section_renders_in_all_languages_with_escaped_environment_paths() {
        let mut app = App::default();
        for language in ["zh-CN", "ja-JP", "en-US"] {
            app.library.settings.language = language.into();
            for section in [
                Section::Sources,
                Section::Ai,
                Section::Display,
                Section::System,
                Section::Paths,
                Section::About,
            ] {
                app.preferences.section = section;
                assert!(render(&app).contains("preferences-content"));
            }
            app.preferences.section = Section::Paths;
            app.preferences.environment = Some(Environment {
                library_path: "E:\\<library>&\".json".into(),
                cache_path: "E:\\cache".into(),
                waifu2x_path: Some("E:\\waifu2x.exe".into()),
            });
            let html = render(&app);
            assert!(html.contains("&lt;library&gt;&amp;&quot;.json"));
            assert!(!html.contains("<library>"));
            app.preferences.environment = None;
        }
    }
}
