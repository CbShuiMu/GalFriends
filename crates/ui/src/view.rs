mod detail;
mod pages;
mod startup;
use crate::{
    bridge, library,
    requests::Kind,
    state::{App, Page},
    widgets::{brand, confirmation_actions, esc, icon, icon_button},
};
use galfriends_core::updates::VERSION;
pub(crate) fn render(a: &mut App) {
    crate::expressions::refresh_pending(a);
    // 导航或后台响应若在主题保存中重绘，完成时需恢复整页的禁用状态。
    if a.pending
        .is_some_and(|(_, kind)| matches!(kind, Kind::Theme))
    {
        a.preferences.theme_needs_full_render = true;
    }
    bridge::render(&html(a));
}

pub(crate) fn theme_button(a: &App) -> String {
    let dark = a.library.settings.theme == "dark";
    icon_button(
        "theme",
        "",
        a.t(if dark { "light" } else { "dark" }),
        if dark { "light_mode" } else { "dark_mode" },
        "nav-item",
        !a.loaded || a.pending.is_some(),
    )
}

fn html(a: &App) -> String {
    let nav = |key: &str, symbol: &str, active: bool| {
        format!(
            r#"<button class="nav-item {}" data-action="nav" data-value="{key}" {} title="{}" aria-label="{}" {}>{}</button>"#,
            if active { "active" } else { "" },
            if active { "aria-current=\"page\"" } else { "" },
            a.t(key),
            a.t(key),
            if a.loaded { "" } else { "disabled" },
            icon(symbol)
        )
    };
    let mut h = format!(
        r#"<div class="app-shell" data-theme="{}" data-language="{}" data-color-theme="{}" data-font-size="{}"><aside class="sidebar"><div class="brand"><span class="brand-mark">{}</span></div><nav class="navigation">{}{}{}{}{}</nav><div class="sidebar-bottom">{}{}{}<div class="sidebar-version"><span>v{VERSION}</span></div></div></aside><main class="main-panel" tabindex="-1" aria-label="{}"><div class="page-scroll {}" data-page="{}">"#,
        esc(&a.library.settings.theme),
        esc(&a.library.settings.language),
        esc(&a.library.settings.color_theme),
        a.library.settings.font_size,
        brand(),
        nav("home", "home", a.page == Page::Home),
        nav("library", "person", a.page == Page::Library),
        nav("favorites", "folder", a.page == Page::Favorites),
        nav("voice", "mic", a.page == Page::Voice),
        nav("emotions", "voice_chat", a.page == Page::Emotions),
        theme_button(a),
        nav("settings", "settings", a.page == Page::Settings),
        nav("about", "about", a.page == Page::About),
        a.t(page_key(&a.page)),
        if !a.loaded {
            "startup-scroll"
        } else if a.page == Page::Home {
            "home-scroll"
        } else {
            ""
        },
        page_key(&a.page)
    );
    if a.loaded {
        h.push_str(&match a.page {
            Page::Home => pages::home(a),
            Page::Library | Page::Favorites => library::page(a),
            Page::Settings => crate::preferences::render(a),
            Page::Voice => crate::voice::render(a),
            Page::Emotions => crate::emotions::render(a),
            Page::About => pages::about(a),
            Page::Detail => detail::page(a),
        });
    } else {
        h.push_str(&startup::render(a));
    }
    h.push_str("</div></main>");
    // 首次加载由内容区统一反馈，避免再输出悬浮提示或未初始化页面的弹窗。
    if !a.loaded {
        h.push_str("</div>");
        return h;
    }
    let dialog_start = h.len();
    if a.search_open {
        h.push_str(&pages::search(a));
    }
    h.push_str(&library::panel(a));
    h.push_str(&crate::detail_tags::dialog(a));
    h.push_str(&crate::expressions::dialog(a));
    h.push_str(&crate::voice::download_dialog(a));
    if a.confirm_remove {
        h.push_str(&format!(r#"<div class="editor-overlay"><section class="editor-dialog small-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-title"><h2 id="confirm-title">{}</h2><p>{}</p>{}</section></div>"#,a.t("remove"),a.t("remove_hint"),confirmation_actions("confirm-remove", "cancel-remove", a.t("confirm"), a.t("cancel"), a.pending.is_some())));
    }
    if h.len() == dialog_start {
        h.push_str(&crate::preferences::updates::dialog(a));
    }
    h.push_str(&notices(a));
    h.push_str("</div>");
    h
}

fn notices(a: &App) -> String {
    let mut h = String::new();
    if let Some(error) = &a.error {
        h.push_str(&format!(
            r#"<div class="notice error" role="alert" data-notice-scope="app" data-notice-id="{}" data-notice-duration="6000">{} <span>{}: {}</span></div>"#,
            a.notice_revision,
            icon("error"),
            a.t("error.operation").replace("{detail}", ""),
            esc(error)
        ));
    } else if let Some((id, Kind::Import)) = a.pending {
        h.push_str(&crate::import_progress::notice(a, id));
    } else if a.pending.is_some_and(|(_, kind)| {
        !matches!(
            kind,
            Kind::Theme | Kind::Selection | Kind::Favorite | Kind::BatchFavorite | Kind::WorkFolder
        )
    }) {
        h.push_str(&format!(
            r#"<div class="notice" role="status"><span class="spinner"></span>{}</div>"#,
            a.t(
                if a.pending
                    .is_some_and(|(_, kind)| matches!(kind, Kind::Artwork))
                {
                    "artwork_processing"
                } else if a
                    .pending
                    .is_some_and(|(_, kind)| matches!(kind, Kind::ExpressionApply))
                {
                    "expression.replacing"
                } else {
                    "loading"
                }
            )
        ));
    } else if a.notice {
        h.push_str(&format!(
            r#"<div class="notice" role="status" data-notice-scope="app" data-notice-id="{}" data-notice-duration="3000">{} {}</div>"#,
            a.notice_revision,
            icon("check"),
            a.t("saved")
        ));
    }
    h.push_str(&crate::emotions::notices(a));
    h.push_str(&crate::expression_scans::notices(a));
    h.push_str(&crate::voice::notices(a));
    h.push_str(&crate::preferences::updates::notice(a));
    if h.is_empty() {
        h
    } else {
        format!(r#"<div class="notice-stack">{h}</div>"#)
    }
}

fn page_key(page: &Page) -> &'static str {
    match page {
        Page::Home => "home",
        Page::Library => "library",
        Page::Favorites => "favorites",
        Page::Voice => "voice",
        Page::Emotions => "emotions",
        Page::Settings => "settings",
        Page::About => "about",
        Page::Detail => "profile",
    }
}

#[cfg(test)]
mod tests {
    use crate::requests::Kind;
    use crate::state::{App, Page};
    #[test]
    fn startup_loading_failure_retry_and_success_have_distinct_feedback() {
        let mut a = App {
            pending: Some((1, Kind::Load)),
            ..Default::default()
        };
        for language in ["zh-CN", "ja-JP", "en-US"] {
            a.library.settings.language = language.into();
            let loading = super::html(&a);
            assert_eq!(loading.matches(r#"role="status""#).count(), 1);
            assert!(loading.contains(a.t("startup.loading")));
            assert!(!loading.contains(r#"data-action="retry""#));
            assert!(!loading.contains(r#"class="notice"#));
            a.pending = None;
            a.error = Some("Cannot read <library> & data".into());
            let failed = super::html(&a);
            assert!(failed.contains(a.t("startup.failed")));
            assert!(failed.contains("Cannot read &lt;library&gt; &amp; data"));
            assert_eq!(failed.matches(r#"data-action="retry""#).count(), 1);
            assert!(!failed.contains("startup-spinner"));
            assert!(!failed.contains(r#"class="notice"#));
            a.pending = Some((2, Kind::Load));
            a.error = None;
            assert!(!super::html(&a).contains(r#"data-startup="error""#));
        }
        a.loaded = true;
        a.pending = None;
        assert!(!super::html(&a).contains("startup-state"));
    }
    #[test]
    fn theme_save_is_quiet_but_errors_and_other_operations_remain_visible() {
        let mut a = App {
            loaded: true,
            pending: Some((1, Kind::Theme)),
            ..Default::default()
        };
        assert!(!super::html(&a).contains("class=\"notice"));
        a.pending = None;
        a.error = Some("save failed".into());
        assert!(super::html(&a).contains("class=\"notice error\""));
        a.error = None;
        a.pending = Some((2, Kind::Save));
        assert!(super::html(&a).contains("class=\"spinner\""));
    }

    #[test]
    fn external_names_are_escaped_in_attributes_and_text() {
        let mut a = App {
            loaded: true,
            ..Default::default()
        };
        let mut c = galfriends_core::characters::sample().unwrap();
        c.name = "<script>bad</script>\"".into();
        a.library.characters.push(c);
        for page in [Page::Home, Page::Library, Page::Detail] {
            a.page = page;
            let html = super::html(&a);
            assert!(!html.contains("<script>bad"));
            assert!(html.contains("&lt;script&gt;bad"));
        }
    }
    #[test]
    fn library_search_finds_kana_alias() {
        let mut a = App {
            loaded: true,
            page: Page::Library,
            filter: "しんどう".into(),
            ..Default::default()
        };
        a.library
            .characters
            .push(galfriends_core::characters::sample().unwrap());
        assert!(super::html(&a).contains("神堂刹那"));
        a.filter = "absent-person".into();
        assert!(!super::html(&a).contains("data-value=\"bangumi:76896\""));
    }
    #[test]
    fn every_page_renders_in_all_languages() {
        let mut a = App {
            loaded: true,
            ..Default::default()
        };
        a.library
            .characters
            .push(galfriends_core::characters::sample().unwrap());
        for lang in ["zh-CN", "en-US", "ja-JP"] {
            a.library.settings.language = lang.into();
            for page in [
                Page::Home,
                Page::Library,
                Page::Favorites,
                Page::Detail,
                Page::Settings,
                Page::About,
            ] {
                a.page = page;
                a.search_open = true;
                assert!(super::html(&a).contains("GalFriends"));
            }
        }
    }
}
