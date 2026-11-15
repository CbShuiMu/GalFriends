//! 角色库的筛选、排序和多选草稿，不进入持久化模型。
use std::collections::BTreeSet;

#[derive(Default, PartialEq, Eq)]
pub(crate) enum Sort {
    #[default]
    Added,
    Name,
    Source,
}
#[derive(Default, PartialEq, Eq)]
pub(crate) enum Panel {
    #[default]
    Closed,
    Tags,
    Collections {
        query: String,
    },
    FolderEdit {
        original: Option<String>,
        parent: String,
    },
    FolderDelete {
        names: Vec<String>,
    },
    AddCharacters,
}
#[derive(Default)]
pub(crate) struct State {
    pub sort: Sort,
    pub descending: bool,
    pub tags: BTreeSet<String>,
    pub batch: bool,
    pub checked: BTreeSet<String>,
    pub checked_folders: BTreeSet<String>,
    pub picker: BTreeSet<String>,
    pub picker_query: String,
    pub show_favorites: bool,
    pub sort_open: bool,
    pub panel: Panel,
}
