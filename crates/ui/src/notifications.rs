//! 短消息的到期确认；浏览器计时，Rust 清理状态，避免重绘复活旧消息。
use crate::state::{APP, App};
use wasm_bindgen::prelude::wasm_bindgen;

/// 仅清除仍属于指定代次的短消息；迟到计时器不能影响新消息或进行中的任务。
#[wasm_bindgen]
pub fn expire_notice(scope: &str, revision: u32) -> bool {
    APP.with(|state| expire(&mut state.borrow_mut(), scope, revision))
}

fn expire(app: &mut App, scope: &str, revision: u32) -> bool {
    match scope {
        "app" if app.loaded && app.notice_revision == revision => {
            app.error = None;
            app.notice = false;
            true
        }
        "emotion" => crate::emotions::expire_notice(app, revision),
        "voice" => crate::voice::expire_notice(app, revision),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::expire;
    use crate::{requests::Kind, state::App};

    #[test]
    fn expiry_clears_only_matching_messages_and_keeps_active_work() {
        let mut app = App {
            loaded: true,
            notice: true,
            notice_revision: 2,
            pending: Some((9, Kind::Import)),
            ..Default::default()
        };
        assert!(!expire(&mut app, "app", 1));
        assert!(app.notice);
        assert!(expire(&mut app, "app", 2));
        assert!(!app.notice);
        assert!(app.pending.is_some());
        app.error = Some("failed".into());
        app.notice_revision = 3;
        assert!(!expire(&mut app, "app", 2));
        assert!(app.error.is_some());
        assert!(expire(&mut app, "app", 3));
        assert!(app.error.is_none());
    }
}
