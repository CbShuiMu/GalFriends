//! 本地搜索涵盖全部别名；筛选只使用用户当前选择的无剧透标签源。
use crate::state::{App, Page};
use galfriends_core::{
    characters::Character,
    library_order::Sort,
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
    let order = &a.library.settings.library_order;
    match order.sort {
        Sort::Custom => {
            let positions: std::collections::BTreeMap<_, _> = order
                .custom_order
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            chars.sort_by_key(|c| positions.get(c.id.as_str()).copied().unwrap_or(usize::MAX));
            return chars;
        }
        Sort::Added => {}
        Sort::Name => chars.sort_by_cached_key(|c| c.name.to_lowercase()),
        Sort::Source => chars.sort_by_key(|c| {
            c.snapshots
                .first()
                .map(|s| s.source.key())
                .unwrap_or_default()
        }),
    }
    if order.descending {
        chars.reverse();
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::visible;
    use crate::state::App;
    use galfriends_core::library_order::Sort;
    #[test]
    fn custom_order_ignores_direction_and_retains_hidden_positions() {
        let mut a = App::default();
        a.library.characters = ["a", "b", "c"]
            .into_iter()
            .map(|id| galfriends_core::characters::Character {
                id: id.into(),
                name: id.into(),
                ..Default::default()
            })
            .collect();
        a.library.settings.library_order.sort = Sort::Custom;
        a.library.settings.library_order.descending = true;
        a.library.settings.library_order.custom_order = vec!["c".into(), "b".into(), "a".into()];
        assert_eq!(
            visible(&a)
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
        a.filter = "b".into();
        assert_eq!(visible(&a)[0].id, "b");
        assert_eq!(
            a.library.settings.library_order.custom_order,
            ["c", "b", "a"]
        );
    }
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
        a.library.settings.library_order.sort = Sort::Name;
        assert_eq!(visible(&a)[0].name, "A");
        assert_eq!(a.library.characters[0].name, "Z");
        a.library.settings.library_order.descending = true;
        assert_eq!(visible(&a)[0].name, "Z");
        a.library_ui.tags.insert("secret".into());
        assert_eq!(visible(&a).len(), 1);
        a.library.settings.language = "en-US".into();
        assert!(visible(&a).is_empty());
    }
}
