//! 自动补全单独跟踪请求，网络等待期间本地操作仍可使用；提交后串行重读最新库。
use crate::{
    bridge,
    requests::{self, Kind},
    state::App,
};
use galfriends_core::{characters::Source, tags};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct State {
    pub pending: Option<(u32, String, Source)>,
    attempted: BTreeSet<(String, String)>,
    pub errors: BTreeMap<(String, String), String>,
    reload: bool,
}

pub(crate) fn schedule(a: &mut App) {
    if !a.loaded || a.pending.is_some() {
        return;
    }
    if a.tag_fetch.reload {
        a.tag_fetch.reload = false;
        requests::send(a, Kind::Load, "load_library", json!({}));
        return;
    }
    if a.tag_fetch.pending.is_some() {
        return;
    }
    let next = a.library.characters.iter().find_map(|c| {
        let source = tags::source(c, a.library.settings.effective_tag_source());
        (!c.snapshots.iter().any(|s| s.source == source)
            && !a
                .tag_fetch
                .attempted
                .contains(&(c.id.clone(), source.key().into())))
        .then(|| (c.id.clone(), source))
    });
    if let Some((id, source)) = next {
        a.next_id = a.next_id.wrapping_add(1);
        a.tag_fetch
            .attempted
            .insert((id.clone(), source.key().into()));
        a.tag_fetch.pending = Some((a.next_id, id.clone(), source));
        bridge::request(
            a.next_id,
            "update_character_tags",
            &json!({"id":id,"source":source,"sourceId":null}).to_string(),
        );
    }
}

pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    if a.tag_fetch.pending.as_ref().is_none_or(|p| p.0 != id) {
        return false;
    }
    if let Some((_, character, source)) = a.tag_fetch.pending.take() {
        if success {
            a.tag_fetch.errors.remove(&(character, source.key().into()));
            // 响应快照可能早于并行收藏/换图操作，重读后再发布，避免覆盖其结果。
            a.tag_fetch.reload = true;
        } else {
            a.tag_fetch
                .errors
                .insert((character, source.key().into()), payload.into());
        }
    }
    schedule(a);
    true
}

pub(crate) fn retry(a: &mut App, id: &str, source: Source) {
    a.tag_fetch
        .attempted
        .remove(&(id.into(), source.key().into()));
    a.tag_fetch.errors.remove(&(id.into(), source.key().into()));
    schedule(a);
}
