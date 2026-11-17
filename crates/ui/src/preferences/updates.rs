//! 设置内的发行页草稿、后台版本检查与更新弹窗。
use crate::{
    bridge,
    requests::{self, Kind},
    state::App,
    widgets::{esc, icon, icon_button, progress_notice},
};
use galfriends_core::updates::{
    Progress, Release, SKILL_REPOSITORY, SkillInstall, SkillStatus, VERSION,
};
use galfriends_i18n::{Message, format_text};
use serde_json::json;
use wasm_bindgen::prelude::wasm_bindgen;

#[derive(PartialEq, Eq, Clone, Copy)]
enum Target {
    App,
    Skill,
}

struct Job {
    id: u32,
    target: Target,
    progress: Progress,
    cancel: Option<u32>,
}

impl Job {
    fn cancellable(&self) -> bool {
        self.cancel.is_none()
            && !matches!(self.progress, Progress::Installing | Progress::Restarting)
    }
}

enum Notice {
    Success(&'static str),
    Failure(Message),
}

#[derive(Default)]
pub(crate) struct State {
    pub draft: Option<String>,
    pub app_request: Option<(u32, bool)>,
    pub skill_request: Option<u32>,
    release: Option<Release>,
    skill: Option<SkillStatus>,
    dialog_open: bool,
    job: Option<Job>,
    notice: Option<(u32, Notice)>,
}

impl State {
    pub(crate) fn reset_release(&mut self) {
        self.app_request = None;
        self.release = None;
        self.dialog_open = false;
        self.notice = None;
    }

    pub(crate) fn dismiss(&mut self) {
        if !self.installing_app() {
            self.dialog_open = false;
        }
    }

    fn installing_app(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.target == Target::App)
    }

    fn available(&self) -> bool {
        self.release
            .as_ref()
            .is_some_and(|release| release.available)
    }
}

pub(super) fn handle(a: &mut App, action: &str) -> bool {
    let state = &mut a.preferences.updates;
    match action {
        "save-release-page" if state.job.is_none() => {
            let value = state
                .draft
                .clone()
                .unwrap_or_else(|| a.library.settings.release_page.clone());
            requests::send(
                a,
                Kind::SaveReleasePage,
                "mutate_library",
                json!({"action":"release_page","id":"","value":value}),
            );
        }
        "check-app-update" if state.job.is_none() => {
            if state.app_request.is_none() {
                state.notice = None;
                requests::check_app_update(a, false);
            }
        }
        "update-dismiss" => state.dismiss(),
        "install-app-update" if state.job.is_none() => {
            if state.dialog_open
                && let Some(release) = state
                    .release
                    .as_ref()
                    .filter(|r| r.available && r.installable)
            {
                let payload = json!({"tag":release.tag});
                start_job(a, Target::App, "install_app_update", payload);
            }
        }
        "install-skill-update" if state.job.is_none() => {
            state.skill_request = None;
            start_job(a, Target::Skill, "install_skill_update", json!({}));
        }
        "update-cancel-install" => {
            if let Some(job) = state.job.as_mut().filter(|job| job.cancellable()) {
                a.next_id = a.next_id.wrapping_add(1);
                job.cancel = Some(a.next_id);
                bridge::request(
                    a.next_id,
                    "cancel_update",
                    &json!({"requestId":job.id}).to_string(),
                );
            }
        }
        _ => return false,
    }
    true
}

fn start_job(a: &mut App, target: Target, command: &str, payload: serde_json::Value) {
    let id = requests::update_job(a, command, payload);
    a.preferences.updates.notice = None;
    a.preferences.updates.job = Some(Job {
        id,
        target,
        progress: Progress::Checking,
        cancel: None,
    });
}

pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    let state = &mut a.preferences.updates;
    if let Some(job) = &mut state.job {
        if job.id == id {
            finish_job(state, id, success, payload);
            return true;
        }
        if job.cancel == Some(id) {
            if !success {
                job.cancel = None;
                state.notice = Some((id, Notice::Failure(Message::from_error(payload))));
            }
            return true;
        }
    }
    if state.skill_request == Some(id) {
        state.skill_request = None;
        state.skill = if success {
            serde_json::from_str(payload).ok()
        } else {
            None
        };
        return true;
    }
    let Some((expected, startup)) = state.app_request else {
        return false;
    };
    if id != expected {
        return false;
    }
    state.app_request = None;
    let result = if success {
        serde_json::from_str(payload).map_err(|error| Message::detail("update.check_failed", error))
    } else {
        Err(Message::from_error(payload))
    };
    match result {
        Ok(release) => {
            state.release = release;
            state.dialog_open = !startup || state.available();
        }
        Err(error) if !startup => state.notice = Some((id, Notice::Failure(error))),
        Err(_) => {} // 启动检查失败不打断打开本地库。
    }
    true
}

fn finish_job(state: &mut State, id: u32, success: bool, payload: &str) {
    let Some(mut job) = state.job.take() else {
        return;
    };
    if success && job.target == Target::App {
        // 交接成功后的主窗口即将退出，继续锁定本次更新操作。
        job.progress = Progress::Restarting;
        state.job = Some(job);
        return;
    }
    let notice = if success {
        match serde_json::from_str::<SkillInstall>(payload) {
            Ok(result) => {
                state.skill = Some(result.status);
                Notice::Success(if result.changed {
                    "update.skill_installed"
                } else {
                    "update.skill_current"
                })
            }
            Err(error) => Notice::Failure(Message::detail("update.skill_install_failed", error)),
        }
    } else {
        let error = Message::from_error(payload);
        if error.key == "update.cancelled" {
            Notice::Success("update.cancelled")
        } else {
            Notice::Failure(error)
        }
    };
    state.notice = Some((id, notice));
}

fn job_text(a: &App, job: &Job) -> String {
    if job.cancel.is_some() && !matches!(job.progress, Progress::Installing | Progress::Restarting)
    {
        return a.t("update.cancelling").into();
    }
    let text = a.t(match job.progress {
        Progress::Checking => "update.checking",
        Progress::Downloading { .. } => "update.downloading",
        Progress::Verifying => "update.verifying",
        Progress::Extracting => "update.extracting",
        Progress::Installing => "update.installing",
        Progress::Restarting => "update.restarting",
    });
    if let Progress::Downloading {
        percent: Some(percent),
    } = job.progress
    {
        format!("{text} {percent}%")
    } else {
        text.into()
    }
}

/// 接收当前安装任务的阶段。迟到进度不会恢复已结束的任务。
#[wasm_bindgen]
pub fn update_progress(id: u32, payload: &str) {
    let Ok(progress) = serde_json::from_str(payload) else {
        return;
    };
    crate::state::APP.with(|cell| {
        let mut a = cell.borrow_mut();
        let Some(job) = a
            .preferences
            .updates
            .job
            .as_mut()
            .filter(|job| job.id == id)
        else {
            return;
        };
        let previous = job.cancellable();
        job.progress = progress;
        if previous != job.cancellable() {
            crate::view::render(&mut a);
        } else if let Some(job) = &a.preferences.updates.job {
            bridge::render_update_progress(id, &job_text(&a, job));
        }
    });
}

pub(crate) fn expire_notice(a: &mut App, id: u32) -> bool {
    let state = &mut a.preferences.updates;
    if state
        .notice
        .as_ref()
        .is_some_and(|(expected, _)| id == *expected)
    {
        state.notice = None;
        return true;
    }
    false
}

pub(crate) fn notice(a: &App) -> String {
    let state = &a.preferences.updates;
    let mut html = String::new();
    if let Some((id, notice)) = &state.notice {
        let (status, role, duration, symbol, message) = match notice {
            Notice::Success(key) => ("", "status", 3000, "check", a.t(key).into()),
            Notice::Failure(error) => ("error", "alert", 6000, "error", error.render(a.language())),
        };
        html.push_str(&format!(
            r#"<div class="notice {status}" role="{role}" data-notice-scope="update" data-notice-id="{id}" data-notice-duration="{duration}">{}<span>{}</span></div>"#,
            icon(symbol), esc(&message)
        ));
    }
    if let Some((id, false)) = state.app_request {
        html.push_str(&progress_notice(
            &format!("update-{id}"),
            a.t("update.checking"),
        ));
    }
    if let Some(job) = &state.job {
        html.push_str(&progress_notice(
            &format!("update-{}", job.id),
            &job_text(a, job),
        ));
    }
    html
}

pub(super) fn has_update(a: &App) -> bool {
    let state = &a.preferences.updates;
    state.available()
        || state
            .skill
            .as_ref()
            .is_some_and(|skill| skill.update_available)
}

pub(super) fn settings(a: &App) -> String {
    let state = &a.preferences.updates;
    let page = state
        .draft
        .as_deref()
        .unwrap_or(&a.library.settings.release_page);
    let skill_status = match &state.skill {
        Some(skill) if !skill.installed => a.t("update.skill_missing").into(),
        Some(skill) => skill.commit.as_ref().map_or_else(
            || a.t("update.skill_local").into(),
            |sha| sha.chars().take(8).collect::<String>(),
        ),
        None => a
            .t(if state.skill_request.is_some() {
                "update.checking"
            } else {
                "update.skill_unknown"
            })
            .into(),
    };
    let disabled = if a.pending.is_some() || state.job.is_some() {
        "disabled"
    } else {
        ""
    };
    let checking = if state.app_request.is_some() {
        "disabled"
    } else {
        disabled
    };
    let skill_cancel = state
        .job
        .as_ref()
        .filter(|job| job.target == Target::Skill)
        .map_or_else(String::new, |job| cancel_button(a, job));
    format!(
        r#"<section class="settings-card update-card">
<div class="settings-card-heading"><h2>GalFriends</h2><div class="update-badges">{}<span class="version-pill">v{VERSION}</span></div></div>
<form data-submit="save-release-page">
<label for="release-page">{}</label><input id="release-page" data-input="release-page" value="{}" spellcheck="false" {disabled}>
<div class="update-actions">
<button id="save-release-page" type="submit" class="button secondary" {disabled}>{}{}</button>
<button type="button" class="button quiet" data-action="check-app-update" {checking}>{}{}</button>
</div>
</form>
</section>
<section class="settings-card update-card">
<div class="settings-card-heading"><h2>Skill</h2><div class="update-badges">{}<span class="version-pill">{}</span></div></div>
<label for="skill-repository">{}</label><input id="skill-repository" value="{SKILL_REPOSITORY}" readonly>
<div class="update-actions"><button type="button" class="button quiet" data-action="install-skill-update" {disabled}>{}{}</button>{skill_cancel}</div>
<p>{}</p>
</section>"#,
        pending_badge(a, state.available()),
        a.t("update.release_page"),
        esc(page),
        icon("save"),
        a.t("save"),
        icon("update"),
        a.t("prefs.updates"),
        pending_badge(
            a,
            state
                .skill
                .as_ref()
                .is_some_and(|skill| skill.update_available)
        ),
        esc(&skill_status),
        a.t("update.skill_repository"),
        icon("update"),
        a.t("prefs.updates"),
        a.t("update.skill_help")
    )
}

fn cancel_button(a: &App, job: &Job) -> String {
    let disabled = if job.cancellable() { "" } else { "disabled" };
    format!(
        r#"<button type="button" class="button quiet" data-action="update-cancel-install" {disabled}>{}</button>"#,
        a.t("cancel")
    )
}

fn pending_badge(a: &App, available: bool) -> String {
    if available {
        format!(
            r#"<span class="version-pill">{}</span>"#,
            a.t("update.pending")
        )
    } else {
        String::new()
    }
}

pub(crate) fn dialog(a: &App) -> String {
    let state = &a.preferences.updates;
    if !state.dialog_open
        || a.pending
            .is_some_and(|(_, kind)| matches!(kind, Kind::SaveReleasePage))
    {
        return String::new();
    }
    let available = state.available();
    let installable = state
        .release
        .as_ref()
        .is_some_and(|release| release.installable);
    let summary = state.release.as_ref().map_or_else(
        || a.t("update.no_releases").into(),
        |release| format_text(a.language(), "update.latest", &[("version", &release.tag)]),
    );
    let notes = state.release.as_ref().filter(|r| r.available).map_or_else(String::new, |release| {
        let notes = release.notes.as_deref().map(str::trim).filter(|notes| !notes.is_empty())
            .unwrap_or_else(|| a.t("update.no_notes"));
        format!(
            r#"<section class="update-notes" aria-labelledby="update-notes-title"><h3 id="update-notes-title">{}</h3><div>{}</div></section>"#,
            a.t("update.notes"), esc(notes)
        )
    });
    let hint = if available && installable {
        a.t("update.download_hint")
    } else if available {
        a.t("update.unavailable")
    } else if state.release.is_some() {
        a.t("update.current")
    } else {
        ""
    };
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!("<p>{}</p>", esc(hint))
    };
    let download = if available {
        let disabled = if a.pending.is_some() || state.job.is_some() || !installable {
            "disabled"
        } else {
            ""
        };
        format!(
            r#"<button type="button" class="button primary" data-action="install-app-update" {disabled}>{}{}</button>"#,
            icon("update"),
            a.t("update.download")
        )
    } else {
        String::new()
    };
    let close = if state.installing_app() {
        state
            .job
            .as_ref()
            .map_or_else(String::new, |job| cancel_button(a, job))
    } else {
        format!(
            r#"<button id="update-later" type="button" class="button quiet" data-initial-focus data-action="update-dismiss">{}</button>"#,
            a.t(if available { "update.later" } else { "close" })
        )
    };
    format!(
        r#"<div class="editor-overlay update-overlay">
<section class="editor-dialog update-dialog" role="dialog" aria-modal="true" aria-labelledby="update-title" data-dismiss-action="update-dismiss">
<header class="section-heading"><h2 id="update-title">{}</h2>{}</header>
<div id="update-dialog-body" class="update-dialog-body" tabindex="0" role="region" aria-labelledby="update-title"><p>{}</p>{notes}{hint}</div>
<footer class="dialog-actions">
{close}
{download}</footer></section></div>"#,
        a.t("prefs.updates"),
        icon_button(
            "update-dismiss",
            "",
            a.t("close"),
            "close",
            "",
            state.installing_app()
        ),
        esc(&summary)
    )
}

#[cfg(test)]
mod tests {
    use super::{App, dialog, resolve};

    #[test]
    fn startup_only_prompts_for_new_versions_and_escapes_release_notes() {
        let mut app = App::default();
        app.preferences.updates.app_request = Some((1, true));
        assert!(resolve(
            &mut app,
            1,
            true,
            r#"{"tag":"v0.1.0","available":false}"#
        ));
        assert!(dialog(&app).is_empty());

        app.preferences.updates.app_request = Some((2, true));
        assert!(!resolve(&mut app, 1, true, "null"));
        assert!(resolve(
            &mut app,
            2,
            true,
            r#"{"tag":"v0.2.0","available":true,"installable":true,"notes":"<script>test</script>\n- Changes"}"#
        ));
        let html = dialog(&app);
        assert!(html.contains("&lt;script&gt;test&lt;/script&gt;\n- Changes"));
        assert!(html.contains("data-action=\"install-app-update\""));
        app.preferences.updates.dismiss();
        assert!(dialog(&app).is_empty());
        assert!(app.preferences.updates.available());
    }
}
