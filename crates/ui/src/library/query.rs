//! 本地搜索涵盖全部别名；筛选只使用用户当前选择的无剧透标签源。
use crate::{
    library::state::Sort,
    state::{App, Page},
};
use galfriends_core::{
    characters::Character,
    tags::{self, DisplayTag},
};
use std::collections::BTreeSet;

pub(super) fn safe_tags<'a>(a: &App, c: &'a Character) -> impl Iterator<Item = DisplayTag<'a>> {
    tags::visible(c, a.library.settings.effective_tag_source()).into_iter()
}
pub(super) fn available_tags(a: &App) -> BTreeSet<&str> {
    a.library
        .characters
        .iter()
        .flat_map(|c| safe_tags(a, c).map(|t| t.name))
        .collect()
}
pub(super) fn visible(a: &App) -> Vec<&Character> {
    if a.page == Page::Favorites && a.collection.is_empty() && !a.library_ui.show_favorites {
        return Vec::new();
    }
    let q = a.filter.to_lowercase();
    let mut chars: Vec<_> = a
        .library
        .characters
        .iter()
        .filter(|c| {
            let in_folder = if !a.collection.is_empty() {
                a.library
                    .collections
                    .get(&a.collection)
                    .is_some_and(|ids| ids.contains(&c.id))
            } else {
                a.page != Page::Favorites || c.favorite
            };
            in_folder
                && (c.name.to_lowercase().contains(&q)
                    || c.aliases.iter().any(|n| n.name.to_lowercase().contains(&q)))
                && a.library_ui
                    .tags
                    .iter()
                    .all(|name| safe_tags(a, c).any(|t| t.name == name))
        })
        .collect();
    match a.library_ui.sort {
        Sort::Added => {}
        Sort::Name => chars.sort_by_cached_key(|c| c.name.to_lowercase()),
        Sort::Source => chars.sort_by_key(|c| {
            c.snapshots
                .first()
                .map(|s| s.source.key())
                .unwrap_or_default()
        }),
    }
    if a.library_ui.descending {
        chars.reverse();
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::visible;
    use crate::{library::state::Sort, state::App};
    use galfriends_core::characters::{Source, Tag, sample};
    #[test]
    fn sorts_without_mutating_library_and_excludes_spoiler_and_other_source_tags() {
        let mut a = App::default();
        let mut first = sample().unwrap();
        first.name = "Z".into();
        first.tags = vec![Tag {
            name: "secret".into(),
            group: String::new(),
            source: Source::Kungal,
            spoiler: true,
            sexual: false,
        }];
        let mut second = first.clone();
        second.id = "other".into();
        second.name = "A".into();
        second.tags[0].spoiler = false;
        a.library.characters = vec![first, second];
        a.library_ui.sort = Sort::Name;
        assert_eq!(visible(&a)[0].name, "A");
        assert_eq!(a.library.characters[0].name, "Z");
        a.library_ui.descending = true;
        assert_eq!(visible(&a)[0].name, "Z");
        a.library_ui.tags.insert("secret".into());
        assert_eq!(visible(&a).len(), 1);
        a.library.settings.language = "en-US".into();
        assert!(visible(&a).is_empty());
    }
}
