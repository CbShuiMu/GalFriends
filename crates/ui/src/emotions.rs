//! 情绪参考页的请求、选择与试听状态，候选和音频路径由后端校验。
use crate::{
    bridge,
    state::{APP, App, Page},
    widgets::{esc, icon, icon_button, progress_notice},
};
use galfriends_core::companion::emotions::{Configuration, Emotion, PageData};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy)]
enum Request {
    Load,
    Recommend,
    Choose,
    Preview,
}

#[derive(Default)]
pub(crate) struct State {
    data: PageData,
    pending: Option<(u32, Request)>,
    cancel_id: Option<u32>,
    cancelling: bool,
    error: Option<String>,
    message: Option<&'static str>,
    notice_revision: u32,
    audio: String,
    progress: String,
}

fn send(a: &mut App, kind: Request, command: &str, mut payload: Value) {
    a.next_id = a.next_id.wrapping_add(1);
    payload["requestId"] = json!(a.next_id);
    a.emotions.pending = Some((a.next_id, kind));
    a.emotions.notice_revision = a.emotions.notice_revision.wrapping_add(1);
    a.emotions.error = None;
    a.emotions.message = None;
    a.emotions.cancelling = false;
    bridge::request(a.next_id, command, &payload.to_string());
}

pub(crate) fn load(a: &mut App, id: Option<&str>) {
    if a.emotions.pending.is_some() {
        return;
    }
    a.emotions.audio.clear();
    send(a, Request::Load, "emotion_page", json!({"id":id}));
}

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    if !action.starts_with("emotion-") {
        return false;
    }
    if action == "emotion-cancel" {
        if let Some((id, Request::Recommend)) = a.emotions.pending
            && !a.emotions.cancelling
        {
            a.next_id = a.next_id.wrapping_add(1);
            a.emotions.cancel_id = Some(a.next_id);
            a.emotions.cancelling = true;
            bridge::request(
                a.next_id,
                "emotion_cancel",
                &json!({"requestId":id}).to_string(),
            );
        }
        return true;
    }
    if a.emotions.pending.is_some() {
        return true;
    }
    if action == "emotion-refresh" || (action == "emotion-sync" && a.page == Page::Emotions) {
        load(a, None);
        return true;
    }
    if action == "emotion-play-failed" {
        a.emotions.notice_revision = a.emotions.notice_revision.wrapping_add(1);
        a.emotions.error = Some("companion.play_failed".into());
        return true;
    }
    let Some(config) = &a.emotions.data.configuration else {
        return true;
    };
    let id = config.character_id.clone();
    match action {
        "emotion-recommend" => {
            a.emotions.progress.clear();
            send(a, Request::Recommend, "emotion_recommend", json!({"id":id}));
        }
        "emotion-choose" => {
            if let Some((emotion, candidate)) = value.split_once('|') {
                send(
                    a,
                    Request::Choose,
                    "emotion_choose",
                    json!({"id":id,"emotion":emotion,"candidateId":candidate}),
                );
            }
        }
        "emotion-preview" => {
            a.emotions.audio.clear();
            send(
                a,
                Request::Preview,
                "emotion_preview",
                json!({"id":id,"candidateId":value}),
            );
        }
        _ => {}
    }
    true
}

pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    if a.emotions.cancel_id == Some(id) {
        a.emotions.cancel_id = None;
        if !success {
            a.emotions.notice_revision = a.emotions.notice_revision.wrapping_add(1);
            a.emotions.error = Some(payload.into());
            a.emotions.cancelling = false;
        }
        return true;
    }
    let Some((expected, kind)) = a.emotions.pending else {
        return false;
    };
    if id != expected {
        return false;
    }
    a.emotions.pending = None;
    a.emotions.notice_revision = a.emotions.notice_revision.wrapping_add(1);
    a.emotions.error = None;
    a.emotions.cancel_id = None;
    a.emotions.cancelling = false;
    let result: Result<(), String> = (|| {
        if !success {
            return Err(payload.into());
        }
        match kind {
            Request::Load => {
                a.emotions.data = serde_json::from_str(payload).map_err(|e| e.to_string())?
            }
            Request::Recommend | Request::Choose => {
                let config: Configuration =
                    serde_json::from_str(payload).map_err(|e| e.to_string())?;
                a.emotions.data.configuration = Some(config);
                a.emotions.message = Some(if matches!(kind, Request::Recommend) {
                    "emotion.recommended"
                } else {
                    "saved"
                });
            }
            Request::Preview => {
                if a.page == Page::Emotions {
                    a.emotions.audio = serde_json::from_str(payload).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if error == "voice.cancelled" {
            a.emotions.message = Some("emotion.cancelled");
        } else {
            a.emotions.error = Some(error);
        }
    }
    true
}

/// 只更新进度文本，保留试听和焦点。
#[wasm_bindgen]
pub fn emotion_progress(id: u32, batch: usize, total: usize) {
    APP.with(|cell| {
        let mut a = cell.borrow_mut();
        if a.emotions.cancelling
            || !a.emotions.pending.is_some_and(|(expected, kind)| {
                expected == id && matches!(kind, Request::Recommend)
            })
        {
            return;
        }
        a.emotions.progress = format!("{} {batch}/{total}", a.t("emotion.recommending"));
        render_emotion_progress(id, &a.emotions.progress);
    });
}
#[wasm_bindgen(js_namespace = GalFriendsBridge)]
extern "C" {
    fn render_emotion_progress(id: u32, text: &str);
}

pub(crate) fn expire_notice(a: &mut App, revision: u32) -> bool {
    if a.emotions.notice_revision != revision {
        return false;
    }
    a.emotions.error = None;
    a.emotions.message = None;
    true
}

/// 全局通知独立于页面内容，进度随任务结束移除，结果消息到期后清理。
pub(crate) fn notices(a: &App) -> String {
    let state = &a.emotions;
    let mut html = String::new();
    if let Some(error) = &state.error {
        let text = if [
            "emotion.cancelled",
            "emotion.no_target",
            "emotion.model_required",
            "emotion.invalid_data",
            "emotion.duration_invalid",
            "emotion.changed",
            "emotion.no_candidates",
            "companion.model_invalid",
            "companion.invalid_reply",
            "companion.audio_invalid",
            "companion.play_failed",
            "companion.ai_required",
            "companion.busy",
            "companion.failed",
            "voice.cancelled",
            "voice.insufficient_samples",
            "voice.language_mismatch",
            "ai.invalid_provider",
        ]
        .contains(&error.as_str())
        {
            a.t(error)
        } else {
            error
        };
        html.push_str(&format!(
            r#"<div class="notice error" role="alert" data-notice-scope="emotion" data-notice-id="{}" data-notice-duration="6000">{}<span class="notice-message">{}</span></div>"#,
            state.notice_revision, icon("error"), esc(text)
        ));
    } else if let Some(message) = state.message {
        html.push_str(&format!(
            r#"<div class="notice" role="status" data-notice-scope="emotion" data-notice-id="{}" data-notice-duration="3000">{}<span class="notice-message">{}</span></div>"#,
            state.notice_revision, icon(if message == "emotion.cancelled" { "about" } else { "check" }), esc(a.t(message))
        ));
    }
    if let Some((id, kind)) = state.pending {
        let status = if state.cancelling {
            a.t("emotion.cancelling")
        } else if matches!(kind, Request::Recommend) {
            if state.progress.is_empty() {
                a.t("emotion.recommending")
            } else {
                &state.progress
            }
        } else {
            a.t("loading")
        };
        html.push_str(&progress_notice(&format!("emotion-{id}"), status));
    }
    html
}

pub(crate) fn render(a: &App) -> String {
    let state = &a.emotions;
    let busy = state.pending.is_some();
    let config = state.data.configuration.as_ref();
    let ready = config.is_some_and(|c| !c.source_dir.is_empty());
    let mut html = format!(
        r#"<section class="emotion-workbench"><div class="emotion-toolbar"><label for="emotion-target">{}</label><select id="emotion-target" data-input="emotion-target" {}>"#,
        a.t("emotion.target"),
        if busy { "disabled" } else { "" }
    );
    for target in &state.data.targets {
        html.push_str(&format!(
            r#"<option value="{}" {}>{}</option>"#,
            esc(&target.id),
            if config.is_some_and(|c| c.character_id == target.id) {
                "selected"
            } else {
                ""
            },
            esc(&target.name)
        ));
    }
    html.push_str(&format!(
        "</select>{}{}",
        icon_button("emotion-refresh", "", a.t("refresh"), "refresh", "", busy),
        icon_button(
            "emotion-recommend",
            "",
            a.t("emotion.recommend"),
            "sparkle",
            "",
            busy || !ready
        )
    ));
    if matches!(state.pending, Some((_, Request::Recommend))) {
        html.push_str(&format!(
            r#"<button class="button secondary" data-action="emotion-cancel" {}>{}</button>"#,
            if state.cancelling { "disabled" } else { "" },
            a.t("cancel")
        ));
    }
    html.push_str("</div>");
    if !state.audio.is_empty() {
        html.push_str(&format!(
            r#"<audio class="emotion-player" controls autoplay src="{}" aria-label="{}"></audio>"#,
            esc(&state.audio),
            a.t("emotion.preview")
        ));
    }
    if state.data.targets.is_empty() {
        html.push_str(&format!("<p>{}</p></section>", a.t("emotion.no_target")));
        return html;
    }
    if !ready {
        html.push_str(&format!(
            "<p>{}</p></section>",
            a.t("emotion.model_required")
        ));
        return html;
    }
    html.push_str(&format!(
        "<p class=emotion-hint>{}</p><div class=emotion-grid>",
        a.t("emotion.hint")
    ));
    if let Some(config) = config {
        for emotion in Emotion::ALL {
            let title = a.t(&format!("emotion.{}", emotion.code()));
            html.push_str(&format!(
                "<section class=emotion-card aria-label=\"{title}\"><h2>{title}</h2>"
            ));
            let selected = config.selected.get(&emotion);
            if let Some(selected) = selected {
                html.push_str(&format!(
                    "<div class=emotion-selected><span>{}</span><p lang=ja>{}</p>{}</div>",
                    a.t("emotion.selected"),
                    esc(&selected.candidate.text),
                    icon_button(
                        "emotion-preview",
                        &selected.candidate.id,
                        a.t("emotion.preview"),
                        "mic",
                        "",
                        busy
                    )
                ));
            }
            let candidates = config
                .recommendations
                .get(&emotion)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if candidates.is_empty() {
                html.push_str(&format!("<p>{}</p>", a.t("emotion.no_candidates")));
            }
            for c in candidates {
                let chosen = selected.is_some_and(|r| r.candidate.id == c.id);
                html.push_str(&format!(r#"<div class="emotion-candidate"><p lang="ja">{}</p><div><small>{:.1} s</small>{}<button class="button secondary" data-action="emotion-choose" data-value="{}|{}" aria-pressed="{}" {}>{}</button></div></div>"#,esc(&c.text),c.duration,icon_button("emotion-preview", &c.id,a.t("emotion.preview"),"mic","",busy),emotion.code(),esc(&c.id),chosen,if busy || chosen {"disabled"}else{""},a.t(if chosen {"emotion.selected"}else{"emotion.choose"})));
            }
            if !candidates.is_empty() && candidates.len() < 5 {
                html.push_str(&format!("<small>{}</small>", a.t("emotion.insufficient")));
            }
            html.push_str("</section>");
        }
    }
    html.push_str("</div></section>");
    html
}

#[cfg(test)]
mod tests {
    use super::{Request, render, resolve};
    use crate::state::App;
    use galfriends_core::companion::emotions::{
        Candidate, Configuration, Emotion, PageData, Reference, Target,
    };

    #[test]
    fn progress_has_no_buttons_and_result_expiry_keeps_new_messages() {
        let mut app = App::default();
        app.emotions.pending = Some((8, Request::Recommend));
        app.emotions.progress = "正在按台词筛选 3/7".into();
        let notice = super::notices(&app);
        assert!(notice.contains(r#"data-progress-notice="emotion-8""#));
        assert!(notice.contains("正在按台词筛选 3/7"));
        assert!(!notice.contains("<button"));
        assert!(render(&app).contains(r#"data-action="emotion-cancel""#));
        assert!(!render(&app).contains("正在按台词筛选"));
        app.emotions.cancelling = true;
        let notice = super::notices(&app);
        assert!(notice.contains(app.t("emotion.cancelling")));
        assert!(render(&app).contains(r#"data-action="emotion-cancel" disabled"#));
        app.emotions.cancel_id = Some(9);
        assert!(resolve(&mut app, 8, false, "voice.cancelled"));
        assert!(!resolve(&mut app, 9, false, "late cancellation error"));
        assert!(app.emotions.error.is_none());
        let notice = super::notices(&app);
        assert!(notice.contains(app.t("emotion.cancelled")));
        assert!(!notice.contains("progress-notice"));
        assert!(notice.contains(r#"data-notice-duration="3000""#));
        let revision = app.emotions.notice_revision;
        assert!(!super::expire_notice(&mut app, revision.wrapping_sub(1)));
        assert!(app.emotions.message.is_some());
        assert!(super::expire_notice(&mut app, revision));
        assert!(super::notices(&app).is_empty());
    }

    #[test]
    fn recommendations_show_success_only_after_a_valid_final_response() {
        let mut app = App::default();
        app.emotions.pending = Some((1, Request::Recommend));
        assert!(resolve(&mut app, 1, true, "invalid response"));
        assert!(app.emotions.message.is_none());
        assert!(super::notices(&app).contains(r#"role="alert""#));
        app.emotions.pending = Some((2, Request::Recommend));
        let payload = serde_json::to_string(&Configuration::default()).unwrap();
        assert!(resolve(&mut app, 2, true, &payload));
        assert!(app.emotions.error.is_none());
        assert!(super::notices(&app).contains(app.t("emotion.recommended")));
    }

    #[test]
    fn fresh_page_restores_saved_choices_without_requiring_recommendations() {
        let mut config = Configuration {
            character_id: "role".into(),
            source_dir: "bound-model".into(),
            ..Default::default()
        };
        for emotion in Emotion::ALL {
            config.selected.insert(
                emotion,
                Reference {
                    candidate: Candidate {
                        id: format!("{}.wav", emotion.code()),
                        text: format!("{}、こんにちは。", emotion.code()),
                        duration: 4.0,
                    },
                    sha256: "saved-digest".into(),
                },
            );
        }
        let payload = serde_json::to_string(&PageData {
            targets: vec![Target {
                id: "role".into(),
                name: "角色".into(),
            }],
            configuration: Some(config),
        })
        .unwrap();
        let mut reopened = App::default();
        reopened.emotions.pending = Some((1, Request::Load));
        assert!(resolve(&mut reopened, 1, true, &payload));
        assert!(reopened.emotions.pending.is_none());
        let html = render(&reopened);
        assert_eq!(html.matches("class=emotion-selected").count(), 5);
        for emotion in Emotion::ALL {
            assert!(html.contains(&format!("{}、こんにちは。", emotion.code())));
        }
    }

    #[test]
    fn renders_all_emotions_and_errors_without_interpreting_external_text() {
        let mut app = App::default();
        app.emotions.data.targets.push(Target {
            id: "role".into(),
            name: "<role>".into(),
        });
        let mut config = Configuration {
            character_id: "role".into(),
            source_dir: "model".into(),
            ..Default::default()
        };
        for emotion in Emotion::ALL {
            config.recommendations.insert(
                emotion,
                vec![Candidate {
                    id: "a.wav".into(),
                    text: "<script>こんにちは".into(),
                    duration: 4.0,
                }],
            );
        }
        app.emotions.data.configuration = Some(config);
        for language in ["zh-CN", "ja-JP", "en-US"] {
            app.library.settings.language = language.into();
            app.emotions.error = Some("File missing <path>".into());
            let html = render(&app);
            assert_eq!(html.matches("class=emotion-card").count(), 5);
            assert!(html.contains("&lt;script&gt;"));
            assert!(super::notices(&app).contains("File missing &lt;path&gt;"));
            assert!(!html.contains("File missing"));
            assert_eq!(html.matches("emotion-choose").count(), 5);
        }
    }

    #[test]
    fn failed_save_and_late_response_preserve_current_configuration() {
        let mut app = App::default();
        app.emotions.data.configuration = Some(Configuration {
            character_id: "role".into(),
            ..Default::default()
        });
        app.emotions.pending = Some((8, Request::Choose));
        assert!(!resolve(&mut app, 7, true, "{}"));
        assert!(app.emotions.pending.is_some());
        assert!(resolve(&mut app, 8, false, "disk full"));
        assert_eq!(
            app.emotions.data.configuration.unwrap().character_id,
            "role"
        );
        assert_eq!(app.emotions.error.as_deref(), Some("disk full"));
    }
}
