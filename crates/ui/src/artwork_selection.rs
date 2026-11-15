//! 预览与已保存选择分离；写入串行执行，连续点选只接续每个角色的最后一次选择。
use crate::{requests, state::App};
use galfriends_core::characters::{Artwork, ArtworkSelection, Character};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct State {
    previews: BTreeMap<String, usize>,
    in_flight: Option<ArtworkSelection>,
}

impl State {
    pub fn selected(&self, character: &Character) -> usize {
        self.previews
            .get(&character.id)
            .copied()
            .unwrap_or(character.selected_art)
    }

    pub fn artwork<'a>(&self, character: &'a Character) -> Option<&'a Artwork> {
        character
            .artworks
            .get(self.selected(character))
            .or_else(|| character.artworks.first())
    }

    fn complete(
        &mut self,
        library: &mut galfriends_core::characters::Library,
        result: Result<ArtworkSelection, String>,
    ) -> Result<(), String> {
        let expected = self
            .in_flight
            .take()
            .ok_or("Missing artwork selection request")?;
        if self.previews.get(&expected.id) == Some(&expected.selected_art) {
            self.previews.remove(&expected.id);
        }
        let saved = result?;
        if saved != expected {
            return Err("Unexpected artwork selection response".into());
        }
        if let Some(character) = library.characters.iter_mut().find(|c| c.id == saved.id) {
            character.selected_art = saved.selected_art;
        }
        Ok(())
    }
}

pub(crate) fn select(a: &mut App, value: &str) {
    let Ok(index) = value.parse::<usize>() else {
        return;
    };
    let Some(character) = a.current() else {
        return;
    };
    if index >= character.artworks.len() || a.artwork_selection.selected(character) == index {
        return;
    }
    let id = character.id.clone();
    a.artwork_selection.previews.insert(id, index);
    send_next(a);
}

fn send_next(a: &mut App) {
    if a.pending.is_some() {
        return;
    }
    let Some((id, &selected_art)) = a.artwork_selection.previews.first_key_value() else {
        return;
    };
    let selection = ArtworkSelection {
        id: id.clone(),
        selected_art,
    };
    requests::send(
        a,
        requests::Kind::Selection,
        "select_artwork",
        serde_json::json!({"selection":selection}),
    );
    a.artwork_selection.in_flight = Some(selection);
}

pub(crate) fn resolve(a: &mut App, success: bool, payload: &str) {
    let result = if success {
        serde_json::from_str(payload).map_err(|e| e.to_string())
    } else {
        Err(payload.into())
    };
    a.error = a.artwork_selection.complete(&mut a.library, result).err();
    send_next(a);
}

#[cfg(test)]
mod tests {
    use super::State;
    use galfriends_core::characters::{ArtworkSelection, Character, Library};

    #[test]
    fn rapid_selection_keeps_latest_preview_and_failure_restores_last_commit() {
        let mut library = Library {
            characters: vec![Character {
                id: "c".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let first = ArtworkSelection {
            id: "c".into(),
            selected_art: 1,
        };
        let last = ArtworkSelection {
            id: "c".into(),
            selected_art: 2,
        };
        let mut state = State::default();
        state.previews.insert("c".into(), 2);
        state.in_flight = Some(first.clone());
        state.complete(&mut library, Ok(first)).unwrap();
        assert_eq!(library.characters[0].selected_art, 1);
        assert_eq!(state.selected(&library.characters[0]), 2);
        state.in_flight = Some(last);
        assert!(
            state
                .complete(&mut library, Err("disk full".into()))
                .is_err()
        );
        assert_eq!(state.selected(&library.characters[0]), 1);
        assert!(state.previews.is_empty());
    }
}
