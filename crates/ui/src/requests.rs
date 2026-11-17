use crate::{bridge, state::App};
use serde_json::{Value, json};
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Load,
    LibraryOrder,
    Save,
    Favorite,
    BatchFavorite,
    Remove,
    Theme,
    Batch,
    Folder,
    Search,
    Import,
    Artwork,
    ExpressionPreview,
    ExpressionApply,
    Selection,
    Open,
    SaveReleasePage,
    Environment,
    Tags,
    TagSource,
    WorkFolder,
}
pub(crate) fn send(a: &mut App, kind: Kind, command: &str, payload: Value) {
    if matches!(kind, Kind::Import) {
        a.import_progress = Default::default();
    }
    a.next_id = a.next_id.wrapping_add(1);
    a.pending = Some((a.next_id, kind));
    bridge::request(a.next_id, command, &payload.to_string());
}
pub(crate) fn mutate(a: &mut App, action: &str, id: &str, value: &str) {
    if matches!(action, "theme" | "color_theme") {
        send(
            a,
            Kind::Theme,
            "set_appearance",
            json!({"key":action,"value":value}),
        );
        return;
    }
    send(
        a,
        if action == "remove" {
            Kind::Remove
        } else if action == "favorite" {
            Kind::Favorite
        } else {
            Kind::Save
        },
        "mutate_library",
        json!({"action":action,"id":id,"value":value}),
    );
}

pub(crate) fn connection(
    a: &mut App,
    request: crate::preferences::connections::Request,
    command: &str,
    payload: Value,
) {
    a.next_id = a.next_id.wrapping_add(1);
    a.preferences.connections.pending.insert(a.next_id, request);
    bridge::request(a.next_id, command, &payload.to_string());
}

pub(crate) fn check_app_update(a: &mut App, startup: bool) {
    a.next_id = a.next_id.wrapping_add(1);
    a.preferences.updates.app_request = Some((a.next_id, startup));
    bridge::request(a.next_id, "check_app_update", "{}");
}

pub(crate) fn check_skill_update(a: &mut App) {
    a.next_id = a.next_id.wrapping_add(1);
    a.preferences.updates.skill_request = Some(a.next_id);
    bridge::request(a.next_id, "check_skill_update", "{}");
}

pub(crate) fn update_job(a: &mut App, command: &str, mut payload: Value) -> u32 {
    a.next_id = a.next_id.wrapping_add(1);
    payload["requestId"] = json!(a.next_id);
    bridge::request(a.next_id, command, &payload.to_string());
    a.next_id
}

pub(crate) fn voice(a: &mut App, action: &str) {
    send_voice(
        a,
        action,
        "voice_run",
        json!({"action":action,"input":a.voice.input}),
    );
}

pub(crate) fn voice_download(a: &mut App, model: galfriends_core::voice::BaseModel) {
    a.voice.download_engine = model.engine();
    send_voice(a, "download", "voice_download", json!({"model":model}));
}

fn send_voice(a: &mut App, action: &str, command: &str, mut payload: Value) {
    a.next_id = a.next_id.wrapping_add(1);
    a.voice.pending = Some((a.next_id, action.to_owned()));
    if action == "train" {
        a.voice.start_training(a.next_id);
    }
    a.voice.error = None;
    a.voice.completed = None;
    a.voice.progress = Default::default();
    payload["requestId"] = json!(a.next_id);
    bridge::request(a.next_id, command, &payload.to_string());
}
