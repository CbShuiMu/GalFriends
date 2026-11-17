//! 后台提取的独立状态与全局消息；不占用表情选择/替换请求槽。
use crate::{
    state::{APP, App},
    widgets::{esc, icon, progress_notice},
};
use galfriends_core::characters::ExpressionScan;
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen::prelude::*;

#[derive(Default)]
pub(crate) struct State {
    statuses: BTreeMap<String, ExpressionScan>,
    expired: BTreeSet<u32>,
    cancelling: BTreeSet<String>,
    request: Option<u32>,
}

pub(crate) fn active(a: &App, id: &str) -> bool {
    a.expression_scans
        .statuses
        .get(id)
        .is_some_and(|s| s.stage != "finished")
}

pub(crate) fn cancelling(a: &App, id: &str) -> bool {
    a.expression_scans.cancelling.contains(id)
}

pub(crate) fn pause(a: &mut App, id: &str) {
    if !active(a, id) || cancelling(a, id) {
        return;
    }
    a.next_id = a.next_id.wrapping_add(1);
    a.expression_scans.request = Some(a.next_id);
    a.expression_scans.cancelling.insert(id.into());
    crate::bridge::request(
        a.next_id,
        "expression_scan_pause",
        &serde_json::json!({"id":id}).to_string(),
    );
}

pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    if a.expression_scans.request != Some(id) {
        return false;
    }
    a.expression_scans.request = None;
    if !success {
        a.error = Some(payload.into());
        a.expression_scans.cancelling.clear();
    }
    true
}

fn text(a: &App, status: &ExpressionScan) -> String {
    let name = a
        .library
        .characters
        .iter()
        .find(|c| c.id == status.character_id)
        .map(|c| c.name.as_str())
        .unwrap_or("");
    let key = if status.stage == "finished" {
        format!("expression.scan_{}", status.result)
    } else if cancelling(a, &status.character_id) {
        "expression.scan_pausing".into()
    } else {
        format!("expression.scan_{}", status.stage)
    };
    let count = if status.total > 0 {
        format!(" {}/{}", status.current, status.total)
    } else {
        String::new()
    };
    format!("{name} · {}{count}", a.t(&key))
}

#[wasm_bindgen(js_namespace = ["window", "GalFriendsBridge"])]
extern "C" {
    fn render_expression_progress(id: u32, text: &str);
}

/// 同代任务完成后忽略迟到阶段；关闭选择窗口不丢弃后台任务状态。
#[wasm_bindgen]
pub fn expression_scan_progress(payload: &str) {
    let Ok(status) = serde_json::from_str::<ExpressionScan>(payload) else {
        return;
    };
    APP.with(|cell| {
        let mut a = cell.borrow_mut();
        let previous = a.expression_scans.statuses.get(&status.character_id);
        if previous.is_some_and(|old| {
            old.task > status.task || (old.task == status.task && old.stage == "finished")
        }) {
            return;
        }
        let initial = previous.is_none_or(|old| old.task != status.task);
        let finished = status.stage == "finished";
        a.expression_scans
            .statuses
            .insert(status.character_id.clone(), status.clone());
        if finished {
            a.expression_scans.cancelling.remove(&status.character_id);
            crate::expressions::refresh(&mut a, &status.character_id);
        }
        if initial || finished {
            crate::view::render(&mut a);
        } else {
            render_expression_progress(status.task, &text(&a, &status));
        }
    });
}

pub(crate) fn expire_notice(a: &mut App, task: u32) -> bool {
    if a.expression_scans
        .statuses
        .values()
        .any(|s| s.task == task && s.stage == "finished")
    {
        a.expression_scans.expired.insert(task);
        true
    } else {
        false
    }
}

pub(crate) fn notices(a: &App) -> String {
    a.expression_scans.statuses.values().filter(|s| !a.expression_scans.expired.contains(&s.task)).map(|status| {
        let message = text(a, status);
        if status.stage != "finished" { progress_notice(&format!("expression-{}", status.task), &message) }
        else {
            let failed = status.result == "failed";
            format!(r#"<div class="notice {}" role="{}" data-notice-scope="expression" data-notice-id="{}" data-notice-duration="{}">{}<span class="notice-message">{}</span></div>"#,
                if failed {"error"} else {""}, if failed {"alert"} else {"status"}, status.task,
                if failed {6000} else {3000}, icon(if failed {"error"} else {"check"}), esc(&message))
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::{active, expire_notice, notices};
    use crate::state::App;
    use galfriends_core::characters::ExpressionScan;
    #[test]
    fn background_notice_survives_dialog_close_and_expires_only_after_completion() {
        let mut app = App::default();
        app.expression_scans.statuses.insert(
            "c".into(),
            ExpressionScan {
                character_id: "c".into(),
                task: 4,
                stage: "extracting".into(),
                current: 1,
                total: 3,
                ..Default::default()
            },
        );
        assert!(active(&app, "c"));
        assert!(notices(&app).contains("1/3"));
        assert!(!notices(&app).contains("<button"));
        assert!(!expire_notice(&mut app, 4));
        app.expressions = Default::default();
        assert!(active(&app, "c"));
        let status = app.expression_scans.statuses.get_mut("c").unwrap();
        status.stage = "finished".into();
        status.result = "complete".into();
        assert!(!active(&app, "c"));
        assert!(!expire_notice(&mut app, 3));
        assert!(expire_notice(&mut app, 4));
        assert!(notices(&app).is_empty());
    }
}
