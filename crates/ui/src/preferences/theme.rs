//! 主题保存只交换配色与控件片段，不生成包含立绘的页面 HTML。
use crate::{bridge, requests, state::App, view, widgets::esc};
use galfriends_core::preferences::Appearance;

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    let (key, value) = match action {
        "theme" => (
            "theme",
            if a.library.settings.theme == "dark" {
                "light"
            } else {
                "dark"
            },
        ),
        "settings-theme" => ("theme", value),
        "settings-color_theme" => ("color_theme", value),
        _ => return false,
    };
    a.preferences.theme_needs_full_render = false;
    requests::mutate(a, key, "", value);
    bridge::render_theme(&html(a));
    true
}

fn apply(a: &mut App, success: bool, payload: &str) -> Result<(), String> {
    if !success {
        return Err(payload.into());
    }
    let appearance: Appearance = serde_json::from_str(payload).map_err(|e| e.to_string())?;
    a.library.settings.theme = appearance.theme;
    a.library.settings.color_theme = appearance.color_theme;
    Ok(())
}

pub(crate) fn resolve(a: &mut App, success: bool, payload: &str) {
    if let Err(error) = apply(a, success, payload) {
        a.error = Some(error);
        view::render(a);
    } else if a.preferences.theme_needs_full_render {
        view::render(a);
    } else {
        bridge::render_theme(&html(a));
    }
}

fn html(a: &App) -> String {
    format!(
        r#"<div data-theme="{}" data-color-theme="{}">{}{}</div>"#,
        esc(&a.library.settings.theme),
        esc(&a.library.settings.color_theme),
        view::theme_button(a),
        super::view::appearance(a),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        requests::Kind,
        state::{App, Page},
    };

    #[test]
    fn theme_patch_is_bounded_and_contains_no_portraits_in_any_page() {
        let mut a = App {
            loaded: true,
            ..Default::default()
        };
        let mut character = galfriends_core::characters::sample().unwrap();
        character.artworks[0].enhanced = Some("image-payload".repeat(1_000_000));
        a.library.characters.push(character);
        for page in [
            Page::Home,
            Page::Library,
            Page::Favorites,
            Page::Detail,
            Page::Settings,
        ] {
            a.page = page;
            for pending in [None, Some((1, Kind::Theme))] {
                a.pending = pending;
                let html = super::html(&a);
                assert!(html.len() < 5000);
                assert!(!html.contains("image-payload"));
                assert!(html.contains("disabled") == pending.is_some());
            }
        }
    }

    #[test]
    fn response_changes_only_appearance_and_failure_keeps_saved_theme() {
        let mut a = App::default();
        a.library.settings.proxy = "http://localhost:7890".into();
        a.proxy_draft = "unsaved".into();
        super::apply(&mut a, true, r#"{"theme":"dark","color_theme":"white"}"#).unwrap();
        assert_eq!(a.library.settings.theme, "dark");
        assert_eq!(a.library.settings.color_theme, "white");
        assert_eq!(a.library.settings.proxy, "http://localhost:7890");
        assert_eq!(a.proxy_draft, "unsaved");
        for (success, payload) in [(false, "save failed"), (true, "broken")] {
            assert!(super::apply(&mut a, success, payload).is_err());
            assert_eq!(a.library.settings.theme, "dark");
        }
    }
}
