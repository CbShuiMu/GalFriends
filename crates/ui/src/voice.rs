//! 麦克风工作台的草稿、独立请求和页面；训练不阻塞角色库操作。
use crate::{
    bridge, requests,
    state::{APP, App, Page},
    view,
    widgets::{esc, icon, icon_button, progress_notice},
};
use galfriends_core::voice::{BaseModel, Engine, Input, Progress, Reply, Update};
use wasm_bindgen::prelude::wasm_bindgen;

pub(crate) struct State {
    pub input: Input,
    pub reply: Reply,
    pub pending: Option<(u32, String)>,
    pub cancel_id: Option<u32>,
    pub progress: Progress,
    pub error: Option<String>,
    pub loaded: bool,
    pub completed: Option<String>,
    pub download_open: bool,
    pub download_engine: Option<Engine>,
    console: TrainingConsole,
    notice_id: u32,
}

#[derive(Default)]
struct TrainingConsole {
    request_id: Option<u32>,
    output: String,
    result: Option<&'static str>,
}

impl TrainingConsole {
    fn append(&mut self, output: &str) {
        let mut chars = output.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\x1b' if chars.next_if_eq(&'[').is_some() => {
                    // 上游终端颜色和光标定位序列不应混入可复制的日志文本。
                    while let Some(code) = chars.next_if(|code| (' '..='~').contains(code)) {
                        if ('@'..='~').contains(&code) {
                            break;
                        }
                    }
                }
                '\r' if chars.peek() != Some(&'\n') => self.output.push('\n'),
                '\n' | '\t' => self.output.push(ch),
                ch if !ch.is_control() => self.output.push(ch),
                _ => {}
            }
        }
        const LIMIT: usize = 24 * 1024;
        if self.output.len() > LIMIT {
            let start = self.output.ceil_char_boundary(self.output.len() - LIMIT);
            self.output.drain(..start);
        }
    }
}

impl State {
    pub(crate) fn start_training(&mut self, request_id: u32) {
        self.console = TrainingConsole {
            request_id: Some(request_id),
            ..Default::default()
        };
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            input: Input {
                language: "ja".into(),
                epochs: 3,
                ..Default::default()
            },
            reply: Reply::default(),
            pending: None,
            cancel_id: None,
            progress: Progress::default(),
            error: None,
            loaded: false,
            completed: None,
            download_open: false,
            download_engine: None,
            console: TrainingConsole::default(),
            notice_id: 0,
        }
    }
}
pub(crate) fn load(a: &mut App) {
    if a.voice.input.character_id.is_empty() {
        a.voice.input.character_id = a
            .library
            .characters
            .first()
            .map(|c| c.id.clone())
            .unwrap_or_default();
    }
    if !a.voice.loaded && a.voice.pending.is_none() {
        requests::voice(a, "status");
    }
}
pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    let task = match action {
        "voice-download-open" => {
            a.voice.download_open = a.voice.pending.is_none();
            return true;
        }
        "voice-download-close" => {
            a.voice.download_open = false;
            return true;
        }
        "voice-download" => {
            if a.voice.download_open
                && a.voice.pending.is_none()
                && let Some(model) = BaseModel::ALL
                    .into_iter()
                    .find(|model| model.code() == value)
            {
                a.voice.download_open = false;
                requests::voice_download(a, model);
            }
            return true;
        }
        "voice-register" => "register",
        "voice-refresh" => "status",
        "voice-extract" => {
            let Some(action) = identification_action(
                &a.voice.input.manifest,
                crate::game_library::has_selected_game(a),
            ) else {
                return true;
            };
            action
        }
        "voice-skill-extract" if crate::game_library::has_selected_game(a) => "skill_extract",
        "voice-inspect" => "inspect",
        "voice-train" => "train",
        "voice-cancel" => {
            if let Some((request_id, _)) = &a.voice.pending
                && a.voice.cancel_id.is_none()
            {
                a.next_id = a.next_id.wrapping_add(1);
                a.voice.cancel_id = Some(a.next_id);
                bridge::request(
                    a.next_id,
                    "voice_cancel",
                    &serde_json::json!({"requestId":request_id}).to_string(),
                );
            }
            return true;
        }
        _ => return false,
    };
    if a.voice.pending.is_none() {
        requests::voice(a, task);
    }
    true
}
pub(crate) fn input(a: &mut App, name: &str, value: &str) -> bool {
    if !name.starts_with("voice-") {
        return false;
    }
    if a.voice.pending.is_some() {
        return true;
    }
    match name {
        "voice-engine" => {
            if let Some(engine) = Engine::from_code(value) {
                a.voice.input.engine = engine;
                a.voice.reply.ready = false;
                a.voice.reply.artifact.clear();
                a.voice.completed = None;
                a.voice.loaded = false;
                requests::voice(a, "status");
            }
        }
        "voice-character" => {
            a.voice.input.character_id = value.into();
            a.voice.console = TrainingConsole::default();
            crate::game_library::clear_source(a);
        }
        "voice-linked-game" => crate::game_library::select_game(a, value),
        "voice-manifest" => {
            a.voice.input.manifest = value.into();
            a.voice.reply.speakers.clear();
            a.voice.input.speaker.clear();
        }
        "voice-speaker" => a.voice.input.speaker = value.into(),
        "voice-language" => a.voice.input.language = value.into(),
        "voice-epochs" => a.voice.input.epochs = value.parse().unwrap_or(0),
        _ => {}
    }
    view::render(a);
    true
}
pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    if a.voice.cancel_id == Some(id) {
        if !success {
            a.voice.cancel_id = None;
            a.voice.notice_id = id;
            a.voice.error = Some(payload.into());
        }
        return true;
    }
    let Some((expected, _)) = &a.voice.pending else {
        return false;
    };
    if *expected != id {
        return false;
    }
    let (_, action) = a.voice.pending.take().unwrap_or_default();
    a.voice.cancel_id = None;
    a.voice.notice_id = id;
    a.voice.error = None;
    let download_engine = a.voice.download_engine.take();
    let result = if success {
        serde_json::from_str::<Reply>(payload).map_err(|e| e.to_string())
    } else {
        Err(payload.into())
    };
    match result {
        Ok(reply) => {
            if action == "status" || download_engine == Some(a.voice.input.engine) {
                a.voice.reply.ready = reply.ready;
                a.voice.loaded = true;
            } else if action == "train" {
                a.voice.console.result = Some("voice.completed");
                a.voice.reply.artifact = reply.artifact;
                a.voice.reply.samples = reply.samples;
                a.voice.reply.excluded = reply.excluded;
            } else if matches!(action.as_str(), "extract" | "skill_extract" | "inspect") {
                a.voice.reply.artifact.clear();
                a.voice.reply.samples = reply.samples;
                let previous_speaker = (a.voice.input.manifest == reply.manifest)
                    .then_some(a.voice.input.speaker.as_str());
                a.voice.input.speaker = reply
                    .speakers
                    .iter()
                    .find(|speaker| Some(speaker.name.as_str()) == previous_speaker)
                    .or_else(|| (reply.speakers.len() == 1).then(|| &reply.speakers[0]))
                    .map(|speaker| speaker.name.clone())
                    .unwrap_or_default();
                a.voice.input.manifest = reply.manifest;
                a.voice.reply.speakers = reply.speakers;
                a.voice.reply.excluded = reply.excluded;
            }
            if action != "status" {
                a.voice.completed = Some(action);
            }
        }
        Err(e) => {
            if action == "train" {
                if e == "voice.cancelled" {
                    a.voice.console.result = Some("voice.console_cancelled");
                } else {
                    a.voice.console.result = Some("voice.failed");
                    a.voice.console.append("\n");
                    a.voice.console.append(&e);
                }
            }
            a.voice.error = Some(e);
        }
    }
    true
}

/// 忽略已完成任务的迟到进度；离开页面后保留状态，回到页面可继续取消。
#[wasm_bindgen]
pub fn voice_progress(id: u32, payload: &str) {
    APP.with(|state| {
        let mut a = state.borrow_mut();
        if a.voice.cancel_id.is_some()
            || a.voice
                .pending
                .as_ref()
                .is_none_or(|(expected, _)| *expected != id)
        {
            return;
        }
        match serde_json::from_str(payload) {
            Ok(Update::Phase(progress)) => {
                a.voice.progress = progress;
                bridge::render_voice_progress(id, &progress_message(&a));
            }
            Ok(Update::Log { log }) if a.voice.console.request_id == Some(id) => {
                a.voice.console.append(&log);
                bridge::render_voice_log(id, &a.voice.console.output);
            }
            _ => {}
        }
    });
}

pub(crate) fn render(a: &App) -> String {
    let s = &a.voice;
    let busy = s.pending.is_some();
    let disabled = if busy { "disabled" } else { "" };
    let can_extract = !busy && crate::game_library::has_selected_game(a);
    let can_identify = !busy && identification_action(&s.input.manifest, can_extract).is_some();
    let field = |key: &str, name: &str, value: &str| {
        format!(
            r#"<label class="voice-field"><span>{}</span><input data-input="voice-{name}" value="{}" {disabled} autocomplete="off" spellcheck="false"></label>"#,
            a.t(key),
            esc(value)
        )
    };
    let mut h = format!(
        r#"<div class="voice-workbench"><section class="settings-card voice-runtime"><div>{}<select data-input="voice-engine" aria-label="{}" {disabled}>{}</select><span class="voice-status">{}</span></div><div class="voice-tools">{}{}{}</div></section><div class="voice-columns"><section class="settings-card voice-game"><header class="voice-card-header"><h2>{}</h2>{}</header><p>{}</p>{}{}<p>{}</p><div class="voice-actions">{}{}</div></section><section class="settings-card"><h2>{}</h2><p>{}</p>{}<div class="voice-actions">{}</div>"#,
        icon("mic"),
        a.t("voice.engine"),
        Engine::ALL
            .into_iter()
            .map(|engine| option(engine.code(), engine.name(), s.input.engine.code()))
            .collect::<String>(),
        a.t(if s.reply.ready {
            "voice.ready"
        } else {
            "voice.runtime_missing"
        }),
        icon_button(
            "voice-refresh",
            "",
            a.t("voice.refresh"),
            "refresh",
            "icon-button",
            busy
        ),
        icon_button(
            "voice-download-open",
            "",
            a.t("voice.download_models"),
            "download",
            "icon-button",
            busy
        ),
        task(a),
        a.t("voice.source"),
        crate::game_library::voice_tools(a),
        a.t("voice.source_hint"),
        character_select(a, disabled),
        crate::game_library::voice_select(a),
        a.t("voice.skill_hint"),
        icon_button(
            "voice-register",
            "",
            a.t("voice.register"),
            "create_new_folder",
            "icon-button",
            busy
        ),
        icon_button(
            "voice-skill-extract",
            "",
            a.t("voice.skill_extract"),
            "upload",
            "button",
            !can_extract
        ),
        a.t("voice.training"),
        a.t("voice.training_hint"),
        field("voice.manifest", "manifest", &s.input.manifest),
        icon_button(
            "voice-extract",
            "",
            a.t("voice.extract"),
            "linked_services",
            "icon-button",
            !can_identify
        )
    );
    h.push_str(&format!(
        r#"<label class="voice-field"><span>{}</span><select data-input="voice-speaker" data-choice-allow-empty {disabled}>"#,
        a.t("voice.speaker")
    ));
    for speaker in &s.reply.speakers {
        h.push_str(&option(
            &speaker.name,
            &format!("{} · {}", speaker.name, speaker.clips),
            &s.input.speaker,
        ));
    }
    h.push_str(&format!(r#"</select></label><div class="voice-options"><label class="voice-field"><span>{}</span><select data-input="voice-language" {disabled}>"#,a.t("voice.language")));
    h.push_str(&option("ja", "日本語", &s.input.language));
    h.push_str(&format!(r#"</select></label><label class="voice-field"><span>{}</span><input type="number" min="1" max="100" data-input="voice-epochs" value="{}" {disabled}></label></div><div class="voice-actions">{}</div>"#,a.t("voice.epochs"),s.input.epochs,
        icon_button("voice-train", "", a.t("voice.train"), "mic", "button primary", busy || !s.reply.ready || s.input.character_id.is_empty() || s.input.speaker.is_empty() || s.input.manifest.is_empty() || !(1..=100).contains(&s.input.epochs))));
    if !s.reply.artifact.is_empty() {
        h.push_str(&format!(
            r#"<p class="voice-path">{} · {}</p>"#,
            a.t("voice.trained_model"),
            esc(&s.reply.artifact)
        ));
    }
    h.push_str("</section>");
    h.push_str(&training_console(a));
    h.push_str("</div></div>");
    h
}

fn identification_action(manifest: &str, linked_game: bool) -> Option<&'static str> {
    if !manifest.trim().is_empty() {
        Some("inspect")
    } else if linked_game {
        Some("skill_extract")
    } else {
        None
    }
}

fn training_console(a: &App) -> String {
    let console = &a.voice.console;
    let request_id = console
        .request_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    let status = if let Some(result) = console.result {
        a.t(result).to_owned()
    } else if console.request_id.is_some() {
        progress_message(a)
    } else {
        a.t("voice.console_idle").to_owned()
    };
    format!(
        r#"<section class="settings-card voice-console" data-voice-console="{request_id}"><h2 id="voice-console-title">{}</h2><div class="voice-console-window"><div class="voice-console-status">{}</div><pre id="voice-console-output" tabindex="0" aria-labelledby="voice-console-title">{}</pre></div></section>"#,
        a.t("voice.console"),
        esc(&status),
        esc(&console.output),
    )
}

pub(crate) fn download_dialog(a: &App) -> String {
    if a.page != Page::Voice || !a.voice.download_open {
        return String::new();
    }
    let mut html = format!(
        r#"<div class="editor-overlay"><section class="editor-dialog small-dialog" role="dialog" aria-modal="true" aria-labelledby="voice-download-title" data-dismiss-action="voice-download-close"><header class="section-heading"><h2 id="voice-download-title">{}</h2>{}</header><div class="voice-download-options">"#,
        a.t("voice.download_models"),
        icon_button(
            "voice-download-close",
            "",
            a.t("close"),
            "close",
            "icon-button",
            false
        )
    );
    for model in BaseModel::ALL {
        let hint = if model.engine().is_some() {
            ""
        } else {
            a.t("voice.weights_only")
        };
        html.push_str(&format!(
            r#"<button class="button" type="button" data-action="voice-download" data-value="{}"><span>{}<small>{}</small></span>{}</button>"#,
            model.code(), model.name(), hint, icon("download")
        ));
    }
    html.push_str("</div></section></div>");
    html
}

pub(crate) fn expire_notice(a: &mut App, id: u32) -> bool {
    if a.voice.notice_id != id {
        return false;
    }
    a.voice.error = None;
    a.voice.completed = None;
    true
}

pub(crate) fn notices(a: &App) -> String {
    let s = &a.voice;
    let mut h = String::new();
    if let Some((id, _)) = s.pending {
        h.push_str(&progress_notice(
            &format!("voice-{id}"),
            &progress_message(a),
        ));
    }
    if let Some(e) = &s.error {
        let key = e.split(':').next().unwrap_or_default();
        let known = matches!(
            key,
            "voice.skill_missing"
                | "voice.agent_missing"
                | "voice.agent_provider"
                | "companion.ai_required"
                | "voice.skill_failed"
                | "game_library.changed"
                | "game_library.unavailable"
                | "game_library.invalid_folder"
                | "voice.cancelled"
                | "voice.timeout"
                | "voice.runtime_missing"
                | "voice.incomplete_features"
                | "voice.incomplete_training"
                | "voice.path_required"
                | "voice.output_overlap"
                | "voice.unsafe_path"
                | "voice.unsupported_engine"
                | "voice.invalid_manifest"
                | "voice.character_required"
                | "voice.insufficient_samples"
                | "voice.busy"
                | "voice.language_mismatch"
                | "voice.gpu_required"
                | "voice.invalid_action"
                | "voice.invalid_training_config"
                | "voice.unsupported_archive"
                | "voice.incomplete_extraction"
                | "voice.ambiguous_audio"
                | "voice.no_mapping"
                | "voice.manifest_too_large"
                | "voice.conflicting_transcripts"
                | "voice.output_missing"
                | "voice.download_failed"
                | "voice.download_hash"
                | "voice.download_size"
        );
        let title = a.t(if known { key } else { "voice.failed" });
        let text = if known && key == e {
            title.to_owned()
        } else {
            format!(
                "{title} · {}",
                e.split_once(':')
                    .filter(|_| known)
                    .map_or(e.as_str(), |(_, detail)| detail.trim())
            )
        };
        let cancelled = key == "voice.cancelled";
        h.push_str(&format!(
            r#"<div class="notice{}" role="{}" data-notice-scope="voice" data-notice-id="{}" data-notice-duration="{}"><span class="notice-message">{}</span></div>"#,
            if cancelled { "" } else { " error" }, if cancelled { "status" } else { "alert" },
            s.notice_id, if cancelled { 3000 } else { 6000 }, esc(&text)
        ));
    } else if let Some(action) = s.completed.as_deref() {
        let message = match action {
            "download" => a.t("voice.download_complete").into(),
            "register" => a.t("voice.registered").into(),
            _ => format!(
                "{} · {} {} · {} {}",
                a.t("voice.completed"),
                a.t("voice.samples"),
                s.reply.samples,
                a.t("voice.excluded"),
                s.reply.excluded
            ),
        };
        h.push_str(&format!(
            r#"<div class="notice" role="status" data-notice-scope="voice" data-notice-id="{}" data-notice-duration="3000"><span class="notice-message">{}</span></div>"#,
            s.notice_id, esc(&message)
        ));
    }
    h
}
fn task(a: &App) -> String {
    let s = &a.voice;
    if s.pending.is_none() {
        return String::new();
    }
    icon_button(
        "voice-cancel",
        "",
        a.t(if s.cancel_id.is_some() {
            "voice.cancelling"
        } else {
            "cancel"
        }),
        "close",
        "icon-button",
        s.cancel_id.is_some(),
    )
}
fn progress_message(a: &App) -> String {
    if a.voice.cancel_id.is_some() {
        return a.t("voice.cancelling").into();
    }
    let progress = &a.voice.progress;
    let stage = match progress.stage.as_str() {
        "download_check" => "voice.download_check",
        "downloading" => "voice.downloading",
        "skill_analyzing" => "voice.skill_analyzing",
        "skill_extracting" => "voice.skill_extracting",
        "extracting" => "voice.extracting",
        "mapping" => "voice.mapping",
        "preparing" => "voice.preparing",
        "training_step" => "voice.training_step",
        "validating" => "voice.validating",
        "saving" => "voice.saving",
        "gsv_phonemes" => "voice.gsv_phonemes",
        "gsv_hubert" => "voice.gsv_hubert",
        "gsv_speaker" => "voice.gsv_speaker",
        "gsv_semantic" => "voice.gsv_semantic",
        "gsv_gpt" => "voice.gsv_gpt",
        "gsv_sovits" => "voice.gsv_sovits",
        _ => "voice.loading",
    };
    if progress.total == 0 {
        a.t(stage).into()
    } else {
        format!("{} {} / {}", a.t(stage), progress.current, progress.total)
    }
}
fn character_select(a: &App, disabled: &str) -> String {
    let mut html = format!(
        r#"<label class="voice-field"><span>{}</span><select data-input="voice-character" data-choice-allow-empty {disabled}>"#,
        a.t("voice.character")
    );
    for character in &a.library.characters {
        html.push_str(&option(
            &character.id,
            &character.name,
            &a.voice.input.character_id,
        ));
    }
    html.push_str("</select></label>");
    html
}
fn option(value: &str, label: &str, selected: &str) -> String {
    format!(
        r#"<option value="{}" {}>{}</option>"#,
        esc(value),
        if selected == value { "selected" } else { "" },
        esc(label)
    )
}

#[cfg(test)]
mod tests {
    use super::{identification_action, notices, render, resolve};
    use crate::state::App;

    #[test]
    fn identification_reuses_mapping_and_requires_linked_game_for_ai_extraction() {
        assert_eq!(
            identification_action("existing.json", false),
            Some("inspect")
        );
        assert_eq!(
            identification_action("existing.json", true),
            Some("inspect")
        );
        assert_eq!(identification_action("  ", true), Some("skill_extract"));
        assert_eq!(identification_action("", false), None);
    }

    #[test]
    fn extraction_populates_speakers_and_selects_only_an_unambiguous_result() {
        let mut app = App::default();
        app.voice.pending = Some((7, "skill_extract".into()));
        let single =
            r#"{"manifest":"map.json","speakers":[{"name":"刹那","clips":123}],"samples":123}"#;
        assert!(!resolve(&mut app, 6, true, single));
        assert!(app.voice.input.manifest.is_empty());
        assert!(resolve(&mut app, 7, true, single));
        assert_eq!(app.voice.input.manifest, "map.json");
        assert_eq!(app.voice.input.speaker, "刹那");
        assert!(render(&app).contains(r#"<option value="刹那" selected>刹那 · 123</option>"#));
        assert!(app.voice.pending.is_none());

        let multiple = r#"{"manifest":"map.json","speakers":[{"name":"刹那","clips":124},{"name":"悠果","clips":99}],"samples":223}"#;
        app.voice.pending = Some((8, "inspect".into()));
        assert!(resolve(&mut app, 8, true, multiple));
        assert_eq!(app.voice.input.speaker, "刹那");
        assert_eq!(app.voice.reply.speakers[0].clips, 124);
        app.voice.input.manifest.clear();
        app.voice.pending = Some((9, "skill_extract".into()));
        assert!(resolve(&mut app, 9, true, multiple));
        assert!(app.voice.input.speaker.is_empty());
        assert_eq!(app.voice.reply.speakers.len(), 2);
        app.voice.pending = Some((10, "inspect".into()));
        assert!(resolve(&mut app, 10, false, "voice.invalid_manifest"));
        assert_eq!(app.voice.input.manifest, "map.json");
        assert_eq!(app.voice.reply.speakers.len(), 2);
    }

    #[test]
    fn training_output_is_escaped_and_manual_directory_inputs_are_absent() {
        let mut app = App::default();
        app.voice.input.game_dir = r#"F:\game<bad>""#.into();
        app.voice.start_training(4);
        app.voice
            .console
            .append("\x1b[32mstep <tag>\x1b[0m\rnext\r\n");
        for language in ["zh-CN", "ja-JP", "en-US"] {
            app.library.settings.language = language.into();
            app.voice.error = Some("Unknown model error <script>".into());
            let page = render(&app);
            assert!(notices(&app).contains("&lt;script&gt;"));
            assert!(page.contains("step &lt;tag&gt;\nnext\n"));
            assert!(!page.contains("[32m"));
            assert!(!page.contains("F:\\game"));
            assert!(!page.contains("data-input=\"voice-output\""));
            assert!(!page.contains("<h1"));
            assert!(!page.contains(r#"<option value="">"#));
            assert!(!page.contains("value=\"zh\""));
            app.voice.pending = Some((4, "train".into()));
            app.voice.progress.stage = "unknown".into();
            assert!(render(&app).contains("voice-cancel"));
        }
    }

    #[test]
    fn late_response_does_not_clear_active_training_or_publish_success() {
        let mut app = App::default();
        app.voice.pending = Some((5, "train".into()));
        assert!(!resolve(&mut app, 4, true, "{}"));
        assert!(app.voice.pending.is_some());
        assert!(resolve(&mut app, 5, false, "voice.cancelled"));
        assert!(app.voice.completed.is_none());
        assert_eq!(app.voice.error.as_deref(), Some("voice.cancelled"));
    }

    #[test]
    fn operation_results_replace_counts_and_do_not_reuse_old_model_paths() {
        let mut app = App::default();
        app.voice.reply.excluded = 1;
        app.voice.pending = Some((5, "train".into()));
        assert!(resolve(
            &mut app,
            5,
            true,
            r#"{"artifact":"model/adapter.pth","samples":1025,"excluded":88}"#
        ));
        assert_eq!(app.voice.reply.excluded, 88);
        assert_eq!(app.voice.reply.samples, 1025);
        app.voice.pending = Some((6, "inspect".into()));
        assert!(resolve(
            &mut app,
            6,
            true,
            r#"{"manifest":"map.json","samples":374}"#
        ));
        assert!(app.voice.reply.artifact.is_empty());
        assert_eq!(app.voice.reply.samples, 374);
    }
}
