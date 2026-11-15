//! Rust/WASM 负责全部页面与交互状态，JavaScript 只处理 DOM 和 Tauri 传输。
mod artwork_selection;
mod bridge;
mod companion;
mod detail_tags;
mod emotions;
mod game_library;
mod icons;
mod import_progress;
mod library;
mod notifications;
mod preferences;
mod requests;
mod responses;
mod state;
mod tag_fetch;
mod view;
mod voice;
mod widgets;
use serde_json::json;
use state::{APP, Page};
use wasm_bindgen::prelude::wasm_bindgen;

/// 初始化页面并读取桌面角色库。
#[wasm_bindgen]
pub fn start() {
    APP.with(|s| {
        let mut a = s.borrow_mut();
        requests::send(&mut a, requests::Kind::Load, "load_library", json!({}));
        view::render(&mut a);
    });
}
/// 接收桥接后的控件动作。所有持久化由命令结果确认。
#[wasm_bindgen]
pub fn dispatch(action: &str, value: &str) {
    APP.with(|s| {
        let mut a = s.borrow_mut();
        if !a.loaded && action != "retry" {
            return;
        }
        if a.pending.is_some()
            && !(action == "art"
                && a.pending
                    .is_some_and(|(_, kind)| matches!(kind, requests::Kind::Selection)))
            && !matches!(
                action,
                "close-search"
                    | "sort-close"
                    | "nav"
                    | "select"
                    | "detail"
                    | "back"
                    | "voice-cancel"
                    | "voice-download-close"
                    | "emotion-cancel"
            )
        {
            return;
        }
        a.error = None;
        a.notice = false;
        if preferences::theme::handle(&mut a, action, value) {
            return;
        }
        if library::actions::handle(&mut a, action, value)
            || preferences::handle(&mut a, action, value)
            || detail_tags::handle(&mut a, action, value)
            || emotions::handle(&mut a, action, value)
            || voice::handle(&mut a, action, value)
            || game_library::handle(&mut a, action, value)
        {
            view::render(&mut a);
            return;
        }
        match action {
            "companion-open" => {
                let id = a.selected.clone();
                requests::send(
                    &mut a,
                    requests::Kind::Open,
                    "companion_open",
                    json!({"id":id}),
                );
            }
            "nav" => {
                a.voice.download_open = false;
                a.detail_tags = Default::default();
                a.page = match value {
                    "library" => Page::Library,
                    "favorites" => Page::Favorites,
                    "voice" => Page::Voice,
                    "emotions" => Page::Emotions,
                    "settings" => Page::Settings,
                    "about" => Page::About,
                    _ => Page::Home,
                };
                a.filter.clear();
                a.collection.clear();
                a.library_ui = Default::default();
                a.preferences.connections.model_menu = None;
                if a.page == Page::Emotions {
                    emotions::load(&mut a, None);
                }
                if a.page == Page::Voice {
                    voice::load(&mut a);
                }
            }
            "select" => a.selected = value.into(),
            "detail" => {
                a.detail_tags = Default::default();
                a.selected = value.into();
                a.page = Page::Detail;
            }
            "back" => a.page = Page::Library,
            "search" | "link" => {
                a.tag_source_target = None;
                a.library_ui.panel = library::state::Panel::Closed;
                a.library_ui.sort_open = false;
                a.search_open = true;
                a.link_target = (action == "link").then(|| a.selected.clone());
                a.results.clear();
                a.searched = false;
                a.query = if action == "link" {
                    a.current().map(|c| c.name.clone()).unwrap_or_default()
                } else {
                    String::new()
                };
                a.search_source = a.library.settings.search_source.key().into();
            }
            "close-search" => {
                a.search_open = false;
                a.link_target = None;
                a.tag_source_target = None;
            }
            "search-source" => {
                if a.tag_source_target.is_some() && !matches!(value, "kungal" | "vndb") {
                    return;
                }
                if matches!(value, "bangumi" | "kungal" | "vndb" | "erogamescape")
                    && a.search_source != value
                {
                    a.search_source = value.into();
                    a.results.clear();
                    a.searched = false;
                }
            }
            "query" => {
                let source = a.search_source.clone();
                let query = a.query.clone();
                a.searched = false;
                requests::send(
                    &mut a,
                    requests::Kind::Search,
                    "search_characters",
                    json!({"source":source,"query":query}),
                );
            }
            "import" => {
                if let Some((source, id)) = value.split_once('|') {
                    if let Some(target) = a.tag_source_target.clone() {
                        requests::send(
                            &mut a,
                            requests::Kind::TagSource,
                            "update_character_tags",
                            json!({"id":target,"source":source,"sourceId":id}),
                        );
                        view::render(&mut a);
                        return;
                    }
                    let target = a.link_target.clone();
                    requests::send(
                        &mut a,
                        requests::Kind::Import,
                        "import_character",
                        json!({"source":source,"id":id,"target":target}),
                    );
                }
            }
            "open" => {
                if let Some((source, id)) = value.split_once('|') {
                    requests::send(
                        &mut a,
                        requests::Kind::Open,
                        "open_source",
                        json!({"source":source,"id":id}),
                    );
                }
            }
            "enhance" => {
                let id = a.selected.clone();
                requests::send(
                    &mut a,
                    requests::Kind::Save,
                    "enhance_artwork",
                    json!({"id":id}),
                );
            }
            "reprocess-artworks" => {
                let id = a.selected.clone();
                requests::send(
                    &mut a,
                    requests::Kind::Artwork,
                    "reprocess_artworks",
                    json!({"id":id}),
                );
            }
            "favorite" => {
                let id = if value.is_empty() {
                    a.selected.clone()
                } else {
                    value.into()
                };
                requests::mutate(&mut a, "favorite", &id, "");
            }
            "art" => {
                artwork_selection::select(&mut a, value);
            }
            "save-proxy" => {
                let proxy = a.proxy_draft.clone();
                requests::mutate(&mut a, "proxy", "", &proxy);
            }
            "collection" => {
                a.collection = value.into();
                a.library_ui.checked.clear();
            }
            "collect" => {
                let id = a.selected.clone();
                requests::mutate(&mut a, "collect", &id, value);
            }
            "remove" => a.confirm_remove = true,
            "card-remove" => {
                a.selected = value.into();
                a.confirm_remove = true;
            }
            "cancel-remove" => a.confirm_remove = false,
            "confirm-remove" => {
                if a.library_ui.batch {
                    library::actions::handle(&mut a, "batch-confirm-remove", "");
                    view::render(&mut a);
                    return;
                }
                let id = a.selected.clone();
                requests::mutate(&mut a, "remove", &id, "");
            }
            "retry" => requests::send(&mut a, requests::Kind::Load, "load_library", json!({})),
            _ => {}
        }
        view::render(&mut a);
    });
}
/// 接收文本与选择器值；输入草稿不触发网络。
#[wasm_bindgen]
pub fn input_changed(name: &str, value: &str) {
    APP.with(|s| {
        let mut a = s.borrow_mut();
        if name == "emotion-target" {
            emotions::load(&mut a, Some(value));
            view::render(&mut a);
            return;
        }
        if game_library::input(&mut a, name, value) {
            return;
        }
        if voice::input(&mut a, name, value) {
            return;
        }
        if preferences::connections::input(&mut a, name, value) {
            view::render(&mut a);
            return;
        }
        match name {
            "detail-tag-input" => {
                a.detail_tags.name = value.into();
                view::render(&mut a);
            }
            "query" => a.query = value.into(),
            "filter" => {
                a.filter = value.into();
                a.library_ui.checked.clear();
                view::render(&mut a);
            }
            "sort" => {
                a.library_ui.sort = match value {
                    "name" => library::state::Sort::Name,
                    "source" => library::state::Sort::Source,
                    _ => library::state::Sort::Added,
                };
                view::render(&mut a);
            }
            "proxy" => a.proxy_draft = value.into(),
            "collection-name" => a.collection_draft = value.into(),
            "folder-query" => {
                if let library::state::Panel::Collections { query } = &mut a.library_ui.panel {
                    *query = value.into();
                    view::render(&mut a);
                }
            }
            "picker-query" => {
                a.library_ui.picker_query = value.into();
                view::render(&mut a);
            }
            "folder-parent" => {
                if let library::state::Panel::FolderEdit { parent, .. } = &mut a.library_ui.panel {
                    *parent = value.into();
                }
            }
            "language" | "tag_source" => {
                if a.pending.is_none() {
                    if name == "tag_source" {
                        a.library_ui.tags.clear();
                        a.library_ui.checked.clear();
                    }
                    requests::mutate(&mut a, name, "", value);
                    view::render(&mut a);
                }
            }
            _ => {}
        }
    });
}
pub use responses::resolve;
