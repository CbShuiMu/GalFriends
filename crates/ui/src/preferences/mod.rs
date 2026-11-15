//! 设置分类、草稿与保存编排；选项业务校验在 core，动画在 web。
mod connection_view;
pub(crate) mod connections;
pub(crate) mod theme;
mod view;
use crate::{requests, state::App};
use galfriends_core::preferences::Environment;
use serde_json::json;
pub(crate) use view::render;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Section {
    Sources,
    Ai,
    #[default]
    Display,
    System,
    Paths,
    About,
}
impl Section {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Sources => "sources",
            Self::Ai => "ai",
            Self::Display => "display",
            Self::System => "system",
            Self::Paths => "paths",
            Self::About => "about",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Sources => "prefs.sources",
            Self::Ai => "prefs.ai",
            Self::Display => "prefs.display",
            Self::System => "prefs.system",
            Self::Paths => "prefs.paths",
            Self::About => "about",
        }
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub connections: connections::State,
    pub section: Section,
    pub environment: Option<Environment>,
    pub theme_needs_full_render: bool,
}

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    if connections::handle(a, action, value) {
        return true;
    }
    match action {
        "settings-section" => {
            a.preferences.section = match value {
                "sources" => Section::Sources,
                "ai" => Section::Ai,
                "system" => Section::System,
                "paths" => Section::Paths,
                "about" => Section::About,
                _ => Section::Display,
            };
            if a.preferences.section == Section::Paths && a.preferences.environment.is_none() {
                load_environment(a);
            }
            if matches!(a.preferences.section, Section::Sources | Section::Ai) {
                connections::load(a);
            }
        }
        "settings-environment" => load_environment(a),
        "settings-ai_provider"
        | "settings-language"
        | "settings-font_size"
        | "settings-search_source"
        | "settings-tag_source" => {
            let key = action.trim_start_matches("settings-");
            if key == "tag_source" {
                a.library_ui.tags.clear();
                a.library_ui.checked.clear();
            }
            requests::mutate(a, key, "", value);
        }
        _ => return false,
    }
    true
}

fn load_environment(a: &mut App) {
    requests::send(
        a,
        requests::Kind::Environment,
        "settings_environment",
        json!({}),
    );
}
