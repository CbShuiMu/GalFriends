use galfriends_core::characters::{Candidate, Character, Library};
use galfriends_i18n::{Language, tr};
use std::cell::RefCell;
#[derive(Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Home,
    Library,
    Favorites,
    Voice,
    Emotions,
    Settings,
    About,
    Detail,
}
#[derive(Default)]
pub(crate) struct App {
    pub library: Library,
    pub page: Page,
    pub selected: String,
    pub filter: String,
    pub library_ui: crate::library::state::State,
    pub preferences: crate::preferences::State,
    pub detail_tags: crate::detail_tags::State,
    pub voice: crate::voice::State,
    pub game_library: crate::game_library::State,
    pub emotions: crate::emotions::State,
    pub tag_fetch: crate::tag_fetch::State,
    pub tag_source_target: Option<String>,
    pub artwork_selection: crate::artwork_selection::State,
    pub expressions: crate::expressions::State,
    pub expression_scans: crate::expression_scans::State,
    pub import_progress: galfriends_core::characters::ImportProgress,
    pub collection: String,
    pub search_open: bool,
    pub search_source: String,
    pub query: String,
    pub results: Vec<Candidate>,
    pub searched: bool,
    pub link_target: Option<String>,
    pub pending: Option<(u32, crate::requests::Kind)>,
    pub next_id: u32,
    pub error: Option<String>,
    pub notice: bool,
    pub notice_revision: u32,
    pub loaded: bool,
    pub proxy_draft: String,
    pub collection_draft: String,
    pub confirm_remove: bool,
}
impl App {
    pub fn current(&self) -> Option<&Character> {
        self.library
            .characters
            .iter()
            .find(|c| c.id == self.selected)
            .or_else(|| self.library.characters.first())
    }
    pub fn t(&self, key: &str) -> &'static str {
        tr(self.language(), key)
    }
    pub fn language(&self) -> Language {
        Language::from_code(&self.library.settings.language).unwrap_or_default()
    }
}
thread_local! {pub(crate) static APP:RefCell<App>=RefCell::new(App::default());}
