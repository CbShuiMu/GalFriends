//! 独立 WASM 窗口的角色对话状态；DOM 桥接只负责输入、播放和窗口操作。
use crate::{
    bridge,
    widgets::{esc, icon_button},
};
use galfriends_core::{
    companion::{Dialogue, Profile, Turn, VoiceModels},
    preferences::DisplayPreferences,
    voice::{Engine, TrainedModel},
};
use galfriends_i18n::{Language, tr};
use serde_json::json;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy)]
enum Request {
    Profile,
    Chat,
    Speech,
    Models,
    Bind,
    Cancel,
}
#[derive(Default)]
struct State {
    profile: Option<Profile>,
    display_preferences: Option<DisplayPreferences>,
    history: Vec<Turn>,
    draft: String,
    submitted: String,
    models: Vec<TrainedModel>,
    models_failed: bool,
    engine: Engine,
    audio: String,
    error: Option<String>,
    bound_notice: bool,
    notice_id: u32,
    next: u32,
    pending: Option<(u32, Request)>,
    settings: bool,
}
thread_local! { static PET: RefCell<State> = RefCell::new(State::default()); }

/// 保存最新立绘供后续重绘使用，DOM 图片由桥接原位更新。
#[wasm_bindgen]
pub fn companion_artwork_changed(artwork: &str) {
    PET.with(|state| {
        if let Some(profile) = &mut state.borrow_mut().profile {
            profile.artwork = artwork.into();
        }
    });
}
#[wasm_bindgen(js_namespace = GalFriendsBridge)]
extern "C" {
    fn play_audio(data: &str);
    fn stop_audio();
}

impl State {
    fn apply_display_preferences(&mut self, preferences: DisplayPreferences) {
        if let Some(profile) = &mut self.profile {
            profile.apply_display_preferences(&preferences);
        }
        self.display_preferences = Some(preferences);
    }

    fn set_profile(&mut self, mut profile: Profile) {
        // 订阅先于资料请求；迟到的初始快照不能覆盖期间收到的显示设置。
        if let Some(preferences) = &self.display_preferences {
            profile.apply_display_preferences(preferences);
        }
        self.engine = profile.voice.selected;
        self.settings = profile.model_dir.is_empty();
        self.profile = Some(profile);
    }

    fn t(&self, key: &str) -> &'static str {
        let language = self
            .profile
            .as_ref()
            .and_then(|p| Language::from_code(&p.language))
            .unwrap_or_default();
        tr(language, key)
    }
    fn send(&mut self, kind: Request, command: &str, payload: serde_json::Value) {
        self.next = self.next.wrapping_add(1);
        self.pending = Some((self.next, kind));
        bridge::request(self.next, command, &payload.to_string());
    }
    fn load_models(&mut self) {
        self.models.clear();
        self.models_failed = false;
        self.send(
            Request::Models,
            "companion_models",
            json!({"engine": self.engine}),
        );
    }

    fn model_settings(&self, profile: &Profile, busy: bool) -> String {
        let disabled = if busy { "disabled" } else { "" };
        let engines = Engine::ALL
            .into_iter()
            .map(|engine| option(engine.code(), engine.name(), engine == self.engine))
            .collect::<String>();
        let bound = if profile.voice.selected == self.engine {
            profile.voice.directory()
        } else {
            ""
        };
        let placeholder = if matches!(self.pending, Some((_, Request::Models))) {
            "loading"
        } else if self.models_failed {
            "companion.models_failed"
        } else if self.models.is_empty() {
            "companion.no_models"
        } else {
            "companion.choose_model"
        };
        let mut models = format!(
            r#"<option value="" disabled {}>{}</option>"#,
            if self.models.iter().any(|model| model.directory == bound) {
                ""
            } else {
                "selected"
            },
            self.t(placeholder)
        );
        for model in &self.models {
            models.push_str(&option(
                &model.directory,
                &model.label,
                model.directory == bound,
            ));
        }
        format!(
            r#"<div class="pet-profile"><label for="pet-engine">{}</label><select id="pet-engine" data-input="pet-engine" {disabled}>{engines}</select><div class="pet-model-label"><label for="pet-model">{}</label>{}</div><select id="pet-model" data-input="pet-model" {}>{models}</select></div>"#,
            self.t("voice.engine"),
            self.t("companion.model"),
            icon_button(
                "refresh-models",
                "",
                self.t("companion.refresh_models"),
                "refresh",
                "",
                busy
            ),
            if self.models.is_empty() {
                "disabled"
            } else {
                disabled
            },
        )
    }

    fn render(&self) {
        let Some(p) = &self.profile else {
            bridge::render(&format!(
                "<main class=pet-loading><p>{}</p>{}</main>",
                esc(self.error.as_deref().unwrap_or(self.t("loading"))),
                icon_button("retry", "", self.t("retry"), "refresh", "", false)
            ));
            return;
        };
        let busy = self.pending.is_some();
        let cancellable = matches!(self.pending, Some((_, Request::Chat | Request::Speech)));
        let mut html = format!(
            r#"<main class="pet" data-font-size="{}" data-theme="{}" data-color-theme="{}" lang="{}"><img class="pet-art" src="{}" alt="{}" draggable="false"><header class="pet-handle"><strong data-drag title="{}">{}</strong><button data-native="pin" aria-label="{}" title="{}" aria-pressed="true">⌖</button>{}<button data-native="close" aria-label="{}" title="{}">×</button></header><section class="pet-dialogue" aria-label="{}">"#,
            p.font_size,
            esc(&p.theme),
            esc(&p.color_theme),
            esc(&p.language),
            esc(&p.artwork),
            esc(&p.name),
            self.t("companion.drag"),
            esc(&p.name),
            self.t("companion.pin"),
            self.t("companion.pin"),
            icon_button(
                "settings",
                "",
                self.t("companion.profile"),
                "settings",
                "",
                busy
            ),
            self.t("close"),
            self.t("close"),
            self.t("companion.dialogue")
        );
        if self.settings {
            html.push_str(&self.model_settings(p, busy));
        }
        html.push_str("<div class=pet-messages role=log aria-live=polite>");
        if self.history.is_empty() {
            html.push_str(&format!(
                "<p class=pet-empty>{}</p>",
                self.t("companion.greeting")
            ));
        }
        for turn in &self.history {
            html.push_str(&format!(r#"<p class="pet-user">{}</p><article><p lang="zh-CN">{}</p><p lang="ja" class="pet-japanese">{}</p></article>"#,esc(&turn.user),esc(&turn.reply.chinese),esc(&turn.reply.japanese)));
        }
        html.push_str("</div>");
        let notice = if let Some(error) = &self.error {
            let known = [
                "companion.ai_required",
                "emotion.changed",
                "emotion.invalid_data",
                "emotion.duration_invalid",
                "companion.model_invalid",
                "companion.invalid_reply",
                "companion.input_invalid",
                "companion.audio_invalid",
                "companion.play_failed",
                "voice.busy",
                "voice.cancelled",
                "voice.character_required",
                "voice.timeout",
            ];
            let text = if known.contains(&error.as_str()) {
                self.t(error).to_owned()
            } else {
                format!("{} · {}", self.t("companion.failed"), error)
            };
            Some((text, true))
        } else {
            self.bound_notice
                .then(|| (self.t("companion.bound").into(), false))
        };
        let status = match self.pending {
            Some((_, Request::Chat)) => self.t("companion.thinking"),
            Some((_, Request::Speech)) => self.t("companion.speaking"),
            Some(_) => self.t("loading"),
            None => self.t("companion.enter"),
        };
        html.push_str(&format!(r#"<form><textarea id="pet-input" aria-label="{}" placeholder="{}" maxlength="2000" rows="2" {}>{}</textarea><footer><small role="status">{}</small>{}{}</footer></form></section>"#,
            self.t("companion.input"),self.t("companion.input"),if busy{"disabled"}else{""},esc(&self.draft),status,
            icon_button(if cancellable{"cancel"}else{"replay"},"",self.t(if cancellable{"cancel"}else{"companion.replay"}),if cancellable{"close"}else{"mic"},"",!cancellable && (busy || self.history.is_empty())),
            icon_button("send","",self.t("companion.send"),"send","",busy)));
        if let Some((message, error)) = notice {
            html.push_str(&format!(
                r#"<div class="notice-stack"><div class="notice{}" role="{}" data-notice-scope="pet" data-notice-id="{}" data-notice-duration="{}"><span class="notice-message">{}</span></div></div>"#,
                if error { " error" } else { "" },
                if error { "alert" } else { "status" },
                self.notice_id, if error { 6000 } else { 3000 }, esc(&message)
            ));
        }
        html.push_str("</main>");
        bridge::render(&html);
    }
}

fn option(value: &str, label: &str, selected: bool) -> String {
    format!(
        r#"<option value="{}" {}>{}</option>"#,
        esc(value),
        if selected { "selected" } else { "" },
        esc(label)
    )
}

/// 初始化独立桌面窗口；不加载主应用页面和整库。
#[wasm_bindgen]
pub fn companion_start() {
    bridge::install_panic_hook();
    PET.with(|s| {
        let mut s = s.borrow_mut();
        s.send(Request::Profile, "companion_profile", json!({}));
        s.render();
    });
}
/// 接收已保存的显示设置，不占用或替换聊天、合成与绑定请求。
#[wasm_bindgen]
pub fn companion_display_changed(payload: &str) -> Result<(), JsValue> {
    let preferences: DisplayPreferences =
        serde_json::from_str(payload).map_err(|error| JsValue::from_str(&error.to_string()))?;
    PET.with(|state| {
        let mut state = state.borrow_mut();
        if state.display_preferences.as_ref() != Some(&preferences) {
            state.apply_display_preferences(preferences);
            if state.profile.is_some() {
                state.render();
            }
        }
    });
    Ok(())
}

/// 输入草稿不触发重绘，保留 IME 和光标。
#[wasm_bindgen]
pub fn companion_input(field: &str, value: &str) {
    PET.with(|s| {
        let mut s = s.borrow_mut();
        if s.pending.is_some() {
            return;
        }
        match field {
            "pet-engine" => {
                if let Some(engine) = Engine::from_code(value)
                    && engine != s.engine
                {
                    s.engine = engine;
                    s.error = None;
                    s.load_models();
                    s.render();
                }
            }
            "pet-input" => s.draft = value.into(),
            "pet-model" if s.models.iter().any(|model| model.directory == value) => {
                let engine = s.engine;
                s.error = None;
                s.send(
                    Request::Bind,
                    "companion_bind",
                    json!({"directory":value,"engine":engine}),
                );
                s.render();
            }
            _ => {}
        }
    });
}
/// 角色窗口动作和请求顺序由 Rust 管理。
#[wasm_bindgen]
pub fn companion_action(action: &str) {
    PET.with(|s| {
        let mut s = s.borrow_mut();
        if s.pending.is_some()
            && !(action == "cancel"
                && matches!(s.pending, Some((_, Request::Chat | Request::Speech))))
        {
            return;
        }
        s.error = None;
        match action {
            "retry" => s.send(Request::Profile, "companion_profile", json!({})),
            "send" => {
                if s.draft.trim().is_empty() {
                    return;
                }
                stop_audio();
                s.audio.clear();
                s.submitted = s.draft.trim().into();
                let text = s.submitted.clone();
                s.send(Request::Chat, "companion_chat", json!({"text":text}));
            }
            "cancel" => {
                stop_audio();
                s.send(Request::Cancel, "companion_cancel", json!({}));
            }
            "settings" => {
                s.settings = !s.settings;
                if s.settings {
                    s.load_models();
                }
            }
            "refresh-models" => s.load_models(),
            "replay" => {
                if s.audio.is_empty() {
                    s.send(Request::Speech, "companion_speak", json!({}));
                } else {
                    play_audio(&s.audio);
                }
            }
            _ => {}
        }
        s.render();
    });
}
/// 请求序号拒绝取消后迟到的回复，文字成功后再请求日语音频。
#[wasm_bindgen]
pub fn companion_resolve(id: u32, success: bool, payload: &str) {
    PET.with(|s| {
        let mut s = s.borrow_mut();
        let Some((expected, kind)) = s.pending else {
            return;
        };
        if expected != id {
            return;
        }
        s.pending = None;
        let result: Result<(), String> = (|| {
            if !success {
                return Err(payload.into());
            }
            match kind {
                Request::Profile => {
                    let p: Profile = serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    s.set_profile(p);
                    if s.settings {
                        s.load_models();
                    }
                }
                Request::Chat => {
                    let reply: Dialogue =
                        serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    let user = s.submitted.clone();
                    s.history.push(Turn { user, reply });
                    s.draft.clear();
                    if s.history.len() > 6 {
                        s.history.remove(0);
                    }
                    s.send(Request::Speech, "companion_speak", json!({}));
                }
                Request::Speech => {
                    s.audio = serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    play_audio(&s.audio);
                }
                Request::Models => {
                    s.models = serde_json::from_str(payload).map_err(|e| e.to_string())?;
                }
                Request::Bind => {
                    let selection: VoiceModels =
                        serde_json::from_str(payload).map_err(|e| e.to_string())?;
                    if let Some(p) = s.profile.as_mut() {
                        p.model_dir = selection.directory().to_owned();
                        p.voice = selection;
                    }
                    stop_audio();
                    s.audio.clear();
                    s.notice_id = id;
                    s.bound_notice = true;
                }
                Request::Cancel => {
                    s.notice_id = id;
                    s.bound_notice = false;
                    s.error = Some("voice.cancelled".into());
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            if matches!(kind, Request::Models) {
                s.models_failed = true;
            }
            s.notice_id = id;
            s.bound_notice = false;
            s.error = Some(error);
        }
        s.render();
    });
}
/// 清理当前窗口中已到期的通知。
#[wasm_bindgen]
pub fn companion_expire_notice(scope: &str, id: u32) -> bool {
    PET.with(|state| {
        let mut s = state.borrow_mut();
        if scope != "pet" || s.notice_id != id {
            return false;
        }
        s.error = None;
        s.bound_notice = false;
        true
    })
}
/// 浏览器自动播放限制或设备故障不会误报成合成失败。
#[wasm_bindgen]
pub fn companion_audio_failed() {
    PET.with(|s| {
        let mut s = s.borrow_mut();
        s.next = s.next.wrapping_add(1);
        s.notice_id = s.next;
        s.bound_notice = false;
        s.error = Some("companion.play_failed".into());
        s.render();
    });
}

#[cfg(test)]
mod display_tests {
    use super::{Request, State};
    use galfriends_core::{
        companion::{Dialogue, Profile, Turn},
        preferences::DisplayPreferences,
        voice::Engine,
    };

    #[test]
    fn display_changes_preserve_active_conversation_and_model_selection() {
        let mut state = State {
            profile: Some(Profile {
                id: "character-1".into(),
                artwork: "local-artwork".into(),
                model_dir: "bound-model".into(),
                ..Default::default()
            }),
            history: vec![Turn {
                user: "question".into(),
                reply: serde_json::from_str::<Dialogue>(r#"{"chinese":"答复","japanese":"返事"}"#)
                    .unwrap(),
            }],
            draft: "unsent draft".into(),
            submitted: "question".into(),
            audio: "existing-audio".into(),
            engine: Engine::GptSovitsV2Pro,
            pending: Some((7, Request::Speech)),
            settings: true,
            ..Default::default()
        };
        state.apply_display_preferences(DisplayPreferences {
            language: "en-US".into(),
            theme: "dark".into(),
            color_theme: "white".into(),
            font_size: 18,
        });
        let profile = state.profile.as_ref().unwrap();
        assert_eq!(
            (
                &*profile.language,
                &*profile.theme,
                &*profile.color_theme,
                profile.font_size
            ),
            ("en-US", "dark", "white", 18)
        );
        assert_eq!(
            (&*profile.id, &*profile.artwork, &*profile.model_dir),
            ("character-1", "local-artwork", "bound-model")
        );
        assert_eq!(state.draft, "unsent draft");
        assert_eq!(state.submitted, "question");
        assert_eq!(state.history[0].reply.japanese, "返事");
        assert_eq!(state.audio, "existing-audio");
        assert_eq!(state.engine, Engine::GptSovitsV2Pro);
        assert!(matches!(state.pending, Some((7, Request::Speech))));
        assert!(state.settings);
    }

    #[test]
    fn late_initial_profile_keeps_latest_display_event() {
        let mut state = State::default();
        for language in ["en-US", "ja-JP"] {
            state.apply_display_preferences(DisplayPreferences {
                language: language.into(),
                theme: "dark".into(),
                color_theme: "white-pink".into(),
                font_size: 14,
            });
        }
        state.set_profile(Profile {
            language: "zh-CN".into(),
            theme: "light".into(),
            color_theme: "pink".into(),
            font_size: 16,
            model_dir: "bound-model".into(),
            ..Default::default()
        });
        let profile = state.profile.as_ref().unwrap();
        assert_eq!(
            (
                &*profile.language,
                &*profile.theme,
                &*profile.color_theme,
                profile.font_size
            ),
            ("ja-JP", "dark", "white-pink", 14)
        );
        assert_eq!(
            state.t("close"),
            galfriends_i18n::tr(galfriends_i18n::Language::Japanese, "close")
        );
    }
}
