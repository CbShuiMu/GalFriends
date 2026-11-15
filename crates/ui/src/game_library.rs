//! 出演作品与游戏目录的异步只读视图，训练输入由用户显式选择。
use crate::{
    bridge, requests,
    state::{App, Page},
    widgets::{esc, icon_button},
};
use galfriends_core::game_library::{self, Catalog};
use serde_json::json;

#[derive(Default)]
pub(crate) struct State {
    pub draft: Option<String>,
    pub catalog: Option<Catalog>,
    pending: Option<(u32, String)>,
}

// 仅在启动载入设置、路径变更或主动刷新时读取磁盘。
pub(crate) fn load(a: &mut App) {
    let path = &a.library.settings.galgear_library_path;
    if a.game_library
        .pending
        .as_ref()
        .is_some_and(|(_, pending)| pending == path)
    {
        return;
    }
    a.game_library.catalog = None;
    if path.is_empty() {
        a.game_library.pending = None;
        return;
    }
    a.next_id = a.next_id.wrapping_add(1);
    a.game_library.pending = Some((a.next_id, path.clone()));
    bridge::request(a.next_id, "game_library_load", "{}");
}

pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    if a.game_library
        .pending
        .as_ref()
        .is_none_or(|(expected, _)| *expected != id)
    {
        return false;
    }
    let Some((_, path)) = a.game_library.pending.take() else {
        return false;
    };
    if path != a.library.settings.galgear_library_path {
        return true;
    }
    let result = if success {
        serde_json::from_str(payload).map_err(|e| e.to_string())
    } else {
        Err(payload.to_owned())
    };
    match result {
        Ok(catalog) => a.game_library.catalog = Some(catalog),
        Err(error) => {
            a.error = Some(format!("{} · {error}", a.t("game_library.failed")));
            a.notice_revision = a.notice_revision.wrapping_add(1);
        }
    }
    true
}

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    match action {
        "save-game-library" => {
            let path = a
                .game_library
                .draft
                .as_deref()
                .unwrap_or(&a.library.settings.galgear_library_path)
                .to_owned();
            requests::mutate(a, "galgear_library_path", "", &path);
        }
        "game-library-refresh" => {
            load(a);
            if a.page == Page::Detail && a.pending.is_none() && !a.selected.is_empty() {
                let id = a.selected.clone();
                requests::send(
                    a,
                    requests::Kind::Save,
                    "refresh_character_works",
                    json!({"id":id}),
                );
            }
        }
        "work-open" => requests::send(a, requests::Kind::Open, "open_work", json!({"id":value})),
        "game-library-settings" => {
            a.page = Page::Settings;
            crate::preferences::handle(a, "settings-section", "paths");
        }
        _ => return false,
    }
    true
}

pub(crate) fn input(a: &mut App, name: &str, value: &str) -> bool {
    if name == "galgear-library-path" {
        a.game_library.draft = Some(value.into());
        return true;
    }
    false
}

pub(crate) fn select_game(a: &mut App, id: &str) {
    let game = a.game_library.catalog.as_ref().and_then(|catalog| {
        a.library
            .characters
            .iter()
            .find(|character| character.id == a.voice.input.character_id)
            .and_then(|character| {
                game_library::linked_games(character, catalog)
                    .into_iter()
                    .find(|game| game.id == id && game.available)
                    .map(|game| (game.id.clone(), game.folder.clone(), catalog.path.clone()))
            })
    });
    clear_source(a);
    if let Some((id, folder, path)) = game {
        a.voice.input.game_id = id;
        a.voice.input.game_dir = folder;
        a.voice.input.catalog_path = path;
    }
}

pub(crate) fn clear_source(a: &mut App) {
    a.voice.input.game_id.clear();
    a.voice.input.game_dir.clear();
    a.voice.input.catalog_path.clear();
    a.voice.input.manifest.clear();
    a.voice.input.speaker.clear();
    a.voice.reply.speakers.clear();
    a.voice.reply.artifact.clear();
    a.voice.completed = None;
}

fn status(a: &App) -> &'static str {
    if a.library.settings.galgear_library_path.is_empty() {
        a.t("game_library.configure")
    } else if a.game_library.pending.is_some() {
        a.t("loading")
    } else if a.game_library.catalog.is_none() {
        a.t("game_library.failed")
    } else {
        a.t("game_library.no_match")
    }
}

pub(crate) fn voice_select(a: &App) -> String {
    let busy = a.voice.pending.is_some();
    let mut html = format!(
        r#"<label class="voice-field"><span>{}</span><select data-input="voice-linked-game" {}><option value="">{}</option>"#,
        a.t("game_library.linked_games"),
        if busy { "disabled" } else { "" },
        a.t("voice.choose")
    );
    let mut count = 0;
    if let Some(catalog) = &a.game_library.catalog
        && let Some(character) = a
            .library
            .characters
            .iter()
            .find(|c| c.id == a.voice.input.character_id)
    {
        for game in game_library::linked_games(character, catalog) {
            count += 1;
            html.push_str(&format!(
                r#"<option value="{}" {} {}>{} · {}{}</option>"#,
                esc(&game.id),
                if game.id == a.voice.input.game_id {
                    "selected"
                } else {
                    ""
                },
                if game.available { "" } else { "disabled" },
                esc(&game.title),
                esc(&game.folder),
                if game.available {
                    ""
                } else {
                    a.t("game_library.unavailable")
                }
            ));
        }
    }
    html.push_str("</select></label>");
    if count == 0 {
        html.push_str(&format!("<p>{}</p>", status(a)));
    }
    html.push_str(&format!(
        r#"<div class="voice-actions">{}{}</div>"#,
        icon_button(
            "game-library-settings",
            "",
            a.t("prefs.paths"),
            "settings",
            "icon-button",
            false
        ),
        icon_button(
            "game-library-refresh",
            "",
            a.t("refresh"),
            "refresh",
            "icon-button",
            a.game_library.pending.is_some() || busy
        )
    ));
    html
}

pub(crate) fn detail(a: &App) -> String {
    let Some(character) = a.current() else {
        return String::new();
    };
    let works = game_library::works(character);
    let mut html = format!(
        r#"<section class="character-works"><header class="section-heading"><h2>{}</h2>{}</header>"#,
        a.t("game_library.works"),
        icon_button(
            "game-library-refresh",
            "",
            a.t("refresh"),
            "refresh",
            "icon-button",
            a.game_library.pending.is_some() || a.pending.is_some()
        ),
    );
    if works.is_empty() {
        html.push_str(&format!("<p>{}</p>", a.t("game_library.no_works")));
    }
    html.push_str(r#"<div class="character-work-grid">"#);
    for work in works {
        let name = if a.library.settings.language == "zh-CN" && !work.translated_name.is_empty() {
            &work.translated_name
        } else {
            &work.name
        };
        let cover = if work.cover_url.is_empty() {
            format!(
                r#"<span class="work-cover work-cover-empty" aria-hidden="true">{}</span>"#,
                crate::widgets::icon("games")
            )
        } else {
            format!(
                r#"<img class="work-cover" src="{}" alt="" loading="lazy" referrerpolicy="no-referrer">"#,
                esc(&work.cover_url)
            )
        };
        html.push_str(&format!(r#"<article class="character-work">{cover}"#));
        if work.site_id.is_empty() {
            html.push_str(&format!(
                r#"<span class="work-title" title="{}">{}</span>"#,
                esc(name),
                esc(name)
            ));
        } else {
            html.push_str(&format!(r#"<button type="button" class="work-title" data-action="work-open" data-value="{}" title="{}">{}</button>"#, esc(&work.site_id), esc(name), esc(name)));
        }
        html.push_str("</article>");
    }
    html.push_str("</div></section>");
    html
}

#[cfg(test)]
mod tests {
    use super::{resolve, select_game};
    use crate::state::App;
    use galfriends_core::{
        characters::{Character, Snapshot, Source},
        game_library::{Catalog, Game, Identity},
    };

    #[test]
    fn late_catalog_does_not_replace_a_new_library_or_publish_an_error() {
        let mut app = App::default();
        app.library.settings.galgear_library_path = "new".into();
        app.game_library.pending = Some((7, "old".into()));
        assert!(!resolve(&mut app, 6, true, "{}"));
        assert!(app.game_library.pending.is_some());
        assert!(resolve(&mut app, 7, false, "old catalog failed"));
        assert!(app.error.is_none());
        assert!(app.game_library.catalog.is_none());
    }

    #[test]
    fn selecting_a_linked_game_clears_previous_material_and_rejects_missing_installations() {
        let mut app = App::default();
        app.library.characters.push(Character {
            id: "character".into(),
            snapshots: vec![Snapshot {
                source: Source::Kungal,
                data: serde_json::json!({"appearances":[{"work":{"id":"1","medium":"galgame"}}]}),
                id: "1".into(),
                url: String::new(),
            }],
            ..Default::default()
        });
        app.game_library.catalog = Some(Catalog {
            path: "catalog".into(),
            games: vec![Game {
                id: "game".into(),
                title: "title".into(),
                folder: "folder".into(),
                available: true,
                cover_hash: String::new(),
                identities: vec![Identity {
                    source: Source::Kungal,
                    id: "1".into(),
                }],
            }],
        });
        app.voice.input.character_id = "character".into();
        app.voice.input.manifest = "previous.json".into();
        app.voice.input.speaker = "previous".into();
        select_game(&mut app, "game");
        assert_eq!(app.voice.input.game_dir, "folder");
        assert_eq!(app.voice.input.catalog_path, "catalog");
        assert!(app.voice.input.manifest.is_empty());
        assert!(app.voice.input.speaker.is_empty());
        app.game_library.catalog.as_mut().unwrap().games[0].available = false;
        select_game(&mut app, "game");
        assert!(app.voice.input.game_id.is_empty());
        assert!(app.voice.input.game_dir.is_empty());
    }
}
