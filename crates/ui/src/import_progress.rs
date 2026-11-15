//! 导入进度只更新提示片段，避免每张立绘的阶段变化重建页面与焦点。
use crate::{
    bridge,
    requests::Kind,
    state::{APP, App},
    widgets::progress_notice,
};
use galfriends_core::characters::ImportProgress;
use wasm_bindgen::prelude::wasm_bindgen;

/// 接收当前请求的阶段通知；忽略已结束、其他请求及格式错误的通知。
#[wasm_bindgen]
pub fn import_progress(id: u32, payload: &str) {
    APP.with(|state| {
        let mut app = state.borrow_mut();
        if accept(&mut app, id, payload) {
            bridge::render_import_progress(id, &message(&app));
        }
    });
}

fn accept(app: &mut App, id: u32, payload: &str) -> bool {
    if !matches!(app.pending, Some((expected, Kind::Import)) if expected == id) {
        return false;
    }
    let Ok(progress) = serde_json::from_str(payload) else {
        return false;
    };
    if app.import_progress == progress {
        return false;
    }
    app.import_progress = progress;
    true
}

fn message(app: &App) -> String {
    let (key, position) = match app.import_progress {
        ImportProgress::Importing => ("import_processing", None),
        ImportProgress::Downloading { current, total } => {
            ("import.downloading", Some((current, total)))
        }
        ImportProgress::CuttingOut { current, total } => {
            ("import.cutting_out", Some((current, total)))
        }
        ImportProgress::Upscaling { current, total } => {
            ("import.upscaling", Some((current, total)))
        }
        ImportProgress::Saving => ("import.saving", None),
    };
    let mut text = app.t(key).to_owned();
    if let Some((current, total)) = position.filter(|(current, total)| *total > 1 && *current > 0) {
        text.push_str(&format!(" ({current}/{total})"));
    }
    text
}

pub(crate) fn notice(app: &App, id: u32) -> String {
    progress_notice(&format!("import-{id}"), &message(app))
}

#[cfg(test)]
mod tests {
    use super::{accept, message};
    use crate::{requests::Kind, state::App};
    use galfriends_core::characters::ImportProgress;

    #[test]
    fn late_invalid_and_duplicate_progress_cannot_replace_current_stage() {
        let mut app = App {
            pending: Some((7, Kind::Import)),
            ..Default::default()
        };
        let payload = r#"{"stage":"downloading","current":1,"total":3}"#;
        assert!(!accept(&mut app, 6, payload));
        assert!(!accept(&mut app, 7, "invalid"));
        assert!(accept(&mut app, 7, payload));
        assert!(!accept(&mut app, 7, payload));
        app.pending = None;
        assert!(!accept(&mut app, 7, r#"{"stage":"saving"}"#));
        app.pending = Some((8, Kind::Save));
        assert!(!accept(&mut app, 8, r#"{"stage":"saving"}"#));
        assert_eq!(
            app.import_progress,
            ImportProgress::Downloading {
                current: 1,
                total: 3
            }
        );
    }

    #[test]
    fn stage_messages_are_localized_and_only_count_multiple_artworks() {
        let mut app = App::default();
        for language in ["zh-CN", "ja-JP", "en-US"] {
            app.library.settings.language = language.into();
            for (progress, key) in [
                (ImportProgress::Importing, "import_processing"),
                (
                    ImportProgress::Downloading {
                        current: 1,
                        total: 1,
                    },
                    "import.downloading",
                ),
                (
                    ImportProgress::CuttingOut {
                        current: 1,
                        total: 1,
                    },
                    "import.cutting_out",
                ),
                (
                    ImportProgress::Upscaling {
                        current: 1,
                        total: 1,
                    },
                    "import.upscaling",
                ),
                (ImportProgress::Saving, "import.saving"),
            ] {
                app.import_progress = progress;
                assert_eq!(message(&app), app.t(key));
                assert_ne!(message(&app), key);
            }
        }
        app.import_progress = ImportProgress::Upscaling {
            current: 2,
            total: 3,
        };
        assert!(message(&app).ends_with(" (2/3)"));
    }
}
