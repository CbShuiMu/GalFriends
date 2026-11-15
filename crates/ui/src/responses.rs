use crate::{requests::Kind, state::APP, view};
use wasm_bindgen::prelude::wasm_bindgen;
/// 请求 ID 排除迟到响应；失败保留已有库与输入。
#[wasm_bindgen]
pub fn resolve(id: u32, success: bool, payload: &str) {
    APP.with(|s| {
        let mut a = s.borrow_mut();
        if crate::game_library::resolve(&mut a, id, success, payload) {
            view::render(&mut a);
            return;
        }
        if crate::emotions::resolve(&mut a, id, success, payload) {
            view::render(&mut a);
            return;
        }
        if crate::voice::resolve(&mut a, id, success, payload) {
            view::render(&mut a);
            return;
        }
        if crate::tag_fetch::resolve(&mut a, id, success, payload) {
            view::render(&mut a);
            return;
        }
        if crate::preferences::connections::resolve(&mut a, id, success, payload) {
            view::render(&mut a);
            return;
        }
        let Some((expected, kind)) = a.pending else {
            return;
        };
        if id != expected {
            return;
        }
        a.pending = None;
        a.notice_revision = a.notice_revision.wrapping_add(1);
        if matches!(kind, Kind::Theme) {
            crate::preferences::theme::resolve(&mut a, success, payload);
            crate::tag_fetch::schedule(&mut a);
            return;
        }
        if matches!(kind, Kind::Selection) {
            crate::artwork_selection::resolve(&mut a, success, payload);
            crate::tag_fetch::schedule(&mut a);
            view::render(&mut a);
            return;
        }
        let result: Result<(), String> = (|| {
            if !success {
                return Err(payload.into());
            }
            match kind {
                Kind::Favorite | Kind::BatchFavorite => {
                    let update: galfriends_core::characters::FavoriteUpdate =
                        serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    update.apply(&mut a.library);
                    if matches!(kind, Kind::BatchFavorite) {
                        a.library_ui.checked.clear();
                    }
                }
                Kind::Load
                | Kind::Save
                | Kind::Remove
                | Kind::Import
                | Kind::Artwork
                | Kind::Batch
                | Kind::Folder
                | Kind::Tags
                | Kind::TagSource => {
                    let previous_game_library = a.library.settings.galgear_library_path.clone();
                    let previous_proxy = a.library.settings.proxy.clone();
                    a.library = serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    a.loaded = true;
                    if a.library.settings.galgear_library_path != previous_game_library {
                        a.game_library.draft = None;
                        if a.voice.pending.is_none() && !a.voice.input.game_id.is_empty() {
                            crate::game_library::clear_source(&mut a);
                        }
                        crate::game_library::load(&mut a);
                    }
                    if matches!(kind, Kind::TagSource) {
                        a.search_open = false;
                        a.tag_source_target = None;
                        a.detail_tags = Default::default();
                    }
                    if matches!(kind, Kind::Tags) {
                        a.detail_tags = Default::default();
                    }
                    if !a.library.characters.iter().any(|c| c.id == a.selected) {
                        a.selected = a
                            .library
                            .characters
                            .first()
                            .map(|c| c.id.clone())
                            .unwrap_or_default();
                    }
                    // 切换配色等无关设置，不应丢失用户尚未保存的代理输入。
                    if matches!(kind, Kind::Load)
                        || a.proxy_draft == previous_proxy
                        || a.library.settings.proxy != previous_proxy
                    {
                        a.proxy_draft = a.library.settings.proxy.clone();
                    }
                    a.notice = !matches!(kind, Kind::Load | Kind::Theme);
                    if matches!(kind, Kind::Folder) {
                        if let crate::library::state::Panel::FolderEdit {
                            original: Some(old),
                            ..
                        } = &a.library_ui.panel
                            && &a.collection == old
                        {
                            a.collection = a.collection_draft.trim().into();
                        }
                        if !a.collection.is_empty()
                            && !a.library.collections.contains_key(&a.collection)
                        {
                            a.collection.clear();
                        }
                    }
                    if matches!(kind, Kind::Remove | Kind::Batch | Kind::Folder) {
                        a.library_ui.checked.clear();
                        a.library_ui.checked_folders.clear();
                        a.library_ui.picker.clear();
                        a.library_ui.panel = crate::library::state::Panel::Closed;
                        a.confirm_remove = false;
                    }
                    if matches!(kind, Kind::Remove) && a.page == crate::state::Page::Detail {
                        a.page = crate::state::Page::Library;
                        a.detail_tags = Default::default();
                    }
                    if matches!(kind, Kind::Import) && a.search_open {
                        a.search_open = false;
                        a.page = crate::state::Page::Library;
                    }
                }
                Kind::Search => {
                    a.results = serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    a.searched = true;
                }
                Kind::Open => {}
                Kind::Selection | Kind::Theme => {
                    unreachable!("Partial responses are handled above")
                }
                Kind::Environment => {
                    a.preferences.environment =
                        Some(serde_json::from_str(payload).map_err(|e| e.to_string())?);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            a.error = Some(error);
        }
        crate::tag_fetch::schedule(&mut a);
        view::render(&mut a);
    });
}
