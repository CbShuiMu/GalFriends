//! 框选的预览由 DOM 绘制，提交时按当前库页面校验角色与收藏夹。
use crate::{
    state::{APP, App, Page},
    view,
    widgets::icon,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use wasm_bindgen::prelude::wasm_bindgen;

pub(super) fn check() -> String {
    format!(
        r#"<span class="selection-check" aria-hidden="true">{}</span>"#,
        icon("check")
    )
}

#[derive(Deserialize)]
struct MarqueeSelection {
    characters: BTreeSet<String>,
    folders: BTreeSet<String>,
}

/// 提交鼠标框选的角色和收藏夹 ID，忽略当前页面不可见的条目。
#[wasm_bindgen]
pub fn library_marquee(payload: &str) {
    let Ok(selection) = serde_json::from_str(payload) else {
        return;
    };
    APP.with(|state| {
        let mut a = state.borrow_mut();
        if a.loaded
            && a.pending.is_none()
            && a.library_ui.panel == super::state::Panel::Closed
            && !a.search_open
            && !a.confirm_remove
            && matches!(a.page, Page::Library | Page::Favorites)
        {
            apply(&mut a, selection);
            view::render(&mut a);
        }
    });
}

fn apply(a: &mut App, mut selection: MarqueeSelection) {
    let visible: BTreeSet<_> = super::query::visible(a)
        .into_iter()
        .map(|c| &c.id)
        .collect();
    selection.characters.retain(|id| visible.contains(id));
    if a.page == Page::Favorites {
        let folders: BTreeSet<_> = super::collections::children(a).into_iter().collect();
        selection.folders.retain(|name| folders.contains(name));
    } else {
        selection.folders.clear();
    }
    a.library_ui.batch = !selection.characters.is_empty() || !selection.folders.is_empty();
    a.library_ui.checked = selection.characters;
    a.library_ui.checked_folders = selection.folders;
    a.library_ui.sort_open = false;
}

#[cfg(test)]
mod tests {
    use super::{MarqueeSelection, apply};
    use crate::state::{App, Page};
    use galfriends_core::characters::Character;

    #[test]
    fn marquee_selects_only_visible_characters_and_direct_child_folders() {
        let mut a = App {
            page: Page::Favorites,
            collection: "parent".into(),
            ..App::default()
        };
        a.library.characters.push(Character {
            id: "outside".into(),
            ..Character::default()
        });
        for name in ["parent", "child", "sibling"] {
            a.library
                .collections
                .insert(name.into(), Default::default());
        }
        a.library
            .collection_parents
            .insert("child".into(), "parent".into());
        apply(
            &mut a,
            MarqueeSelection {
                characters: ["outside".into()].into(),
                folders: ["child".into(), "sibling".into()].into(),
            },
        );
        assert!(a.library_ui.batch);
        assert!(a.library_ui.checked.is_empty());
        assert_eq!(a.library_ui.checked_folders, ["child".into()].into());

        apply(
            &mut a,
            MarqueeSelection {
                characters: Default::default(),
                folders: Default::default(),
            },
        );
        assert!(!a.library_ui.batch);
        assert!(a.library_ui.checked_folders.is_empty());
    }
}
