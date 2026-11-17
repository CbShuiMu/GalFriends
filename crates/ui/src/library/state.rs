//! 角色库的筛选、多选和工具栏弹层草稿；排序保存在共用库设置中。
use std::collections::BTreeSet;

#[derive(Default, PartialEq, Eq)]
pub(crate) enum Panel {
    #[default]
    Closed,
    Tags {
        selected: BTreeSet<String>,
        query: String,
    },
    Collections {
        query: String,
        ids: BTreeSet<String>,
        single: bool,
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
