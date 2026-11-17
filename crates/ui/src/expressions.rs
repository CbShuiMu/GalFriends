//! 先选择游戏原始表情，确认后才请求替换；错误保留选择供重试。
use crate::{
    requests::{self, Kind},
    state::App,
    widgets::{esc, icon_button},
};
use galfriends_core::characters::ExpressionPreview;
use serde_json::json;

#[derive(Default)]
pub(crate) struct State {
    phase: Phase,
    preview: Option<ExpressionPreview>,
    selected: usize,
    character_id: String,
    refresh_needed: bool,
}

#[derive(Default, PartialEq)]
enum Phase {
    #[default]
    Closed,
    Loading,
    Ready,
    Failed,
}

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    match action {
        "expression-open" => {
            a.expressions = State::default();
            a.expressions.phase = Phase::Loading;
            a.expressions.character_id.clone_from(&a.selected);
            requests::send(
                a,
                Kind::ExpressionPreview,
                "expression_preview",
                json!({"id": a.selected}),
            );
        }
        "expression-close" => a.expressions = State::default(),
        "expression-scan-pause" => {
            let id = a.expressions.character_id.clone();
            crate::expression_scans::pause(a, &id);
        }
        "expression-select" => {
            if let Ok(index) = value.parse::<usize>()
                && a.expressions
                    .preview
                    .as_ref()
                    .is_some_and(|p| index < p.variants.len())
            {
                a.expressions.selected = index;
            }
        }
        "expression-apply" => {
            if let Some(preview) = &a.expressions.preview {
                let payload = json!({"token": preview.token, "index": a.expressions.selected});
                requests::send(a, Kind::ExpressionApply, "expression_apply", payload);
            }
        }
        _ => return false,
    }
    true
}

pub(crate) fn refresh(a: &mut App, id: &str) {
    if a.expressions.character_id == id && a.expressions.phase != Phase::Closed {
        a.expressions.refresh_needed = true;
    }
}

pub(crate) fn refresh_pending(a: &mut App) {
    if a.expressions.refresh_needed && a.expressions.phase == Phase::Ready && a.pending.is_none() {
        a.expressions.refresh_needed = false;
        requests::send(
            a,
            Kind::ExpressionPreview,
            "expression_preview",
            json!({"id":a.expressions.character_id,"discover":false}),
        );
    }
}

pub(crate) fn resolve(a: &mut App, apply: bool, success: bool, payload: &str) {
    let result: Result<(), String> = (|| {
        if !success {
            return Err(payload.into());
        }
        if apply {
            a.library = serde_json::from_str(payload).map_err(|e| e.to_string())?;
            a.expressions = State::default();
            a.notice = true;
        } else {
            if a.selected != a.expressions.character_id || a.page != crate::state::Page::Detail {
                a.expressions = State::default();
                return Ok(());
            }
            let selected = a
                .expressions
                .preview
                .as_ref()
                .and_then(|p| p.variants.get(a.expressions.selected))
                .map(|v| v.id.clone());
            let preview: ExpressionPreview =
                serde_json::from_str(payload).map_err(|e| e.to_string())?;
            a.expressions.selected = selected
                .and_then(|id| preview.variants.iter().position(|v| v.id == id))
                .unwrap_or(0);
            a.expressions.preview = Some(preview);
            a.expressions.phase = Phase::Ready;
        }
        Ok(())
    })();
    if result.is_err() && !apply {
        a.expressions.phase = Phase::Failed;
    }
    a.error = result.err().map(|error| {
        if matches!(
            error.as_str(),
            "expression.changed" | "expression.no_match" | "expression.materials_update"
        ) {
            a.t(&error).into()
        } else {
            error
        }
    });
}

pub(crate) fn dialog(a: &App) -> String {
    if a.expressions.phase == Phase::Closed {
        return String::new();
    }
    let variants = a
        .expressions
        .preview
        .as_ref()
        .map(|p| p.variants.as_slice())
        .unwrap_or_default();
    let busy = a.pending.is_some();
    let mut html = format!(
        r#"<div class="editor-overlay"><section class="editor-dialog expression-dialog" role="dialog" aria-modal="true" aria-labelledby="expression-title" data-dismiss-action="expression-close"><h2 id="expression-title">{}</h2>"#,
        a.t("expression.replace")
    );
    html.push_str(&format!(
        r#"<p class="muted small">{}</p>"#,
        a.t("expression.original_hint")
    ));
    if crate::expression_scans::active(a, &a.expressions.character_id) {
        html.push_str(&format!(
            "<div class=\"expression-tools\">{}</div>",
            icon_button(
                "expression-scan-pause",
                "",
                a.t("expression.scan_pause"),
                "pause",
                "secondary",
                crate::expression_scans::cancelling(a, &a.expressions.character_id)
            )
        ));
    }
    html.push_str("<div class=\"expression-grid\">");
    if a.expressions.phase == Phase::Ready && variants.is_empty() {
        html.push_str(&format!(
            "<p class=\"expression-empty\">{}</p>",
            a.t("expression.empty")
        ));
    }
    for (index, variant) in variants.iter().enumerate() {
        let label = format!(
            "{} {}",
            a.t("expression.variant"),
            variant
                .layers
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" + ")
        );
        html.push_str(&format!(r#"<button class="album-item {}" data-action="expression-select" data-value="{index}" aria-pressed="{}" {}><img src="{}" alt="{}"><span>{}</span><span>{}</span></button>"#,
            if index == a.expressions.selected { "selected" } else { "" }, index == a.expressions.selected,
            if busy { "disabled" } else { "" }, esc(&variant.thumbnail), esc(&label), esc(&label), esc(&variant.source)));
    }
    html.push_str(&format!(
        r#"</div><div class="dialog-actions">{}{}</div></section></div>"#,
        if a.expressions.phase == Phase::Failed {
            icon_button(
                "expression-open",
                "",
                a.t("retry"),
                "refresh",
                "primary",
                busy,
            )
        } else {
            icon_button(
                "expression-apply",
                "",
                a.t("expression.apply"),
                "check",
                "primary",
                busy || variants.is_empty(),
            )
        },
        icon_button(
            "expression-close",
            "",
            a.t("cancel"),
            "close",
            "secondary",
            busy
        )
    ));
    html
}

#[cfg(test)]
mod tests {
    use super::{Phase, State, dialog, resolve};
    use crate::state::{App, Page};

    #[test]
    fn opens_before_loading_distinguishes_empty_and_keeps_failed_apply_preview() {
        let mut app = App {
            selected: "c".into(),
            page: Page::Detail,
            expressions: State {
                phase: Phase::Loading,
                character_id: "c".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(dialog(&app).contains("role=\"dialog\""));
        assert!(!dialog(&app).contains(app.t("expression.empty")));
        resolve(&mut app, false, true, r#"{"token":"t","variants":[]}"#);
        assert!(dialog(&app).contains(app.t("expression.empty")));
        resolve(
            &mut app,
            false,
            true,
            r#"{"token":"t","variants":[{"source":"source.pna","layers":[1],"thumbnail":"face"}]}"#,
        );
        assert!(dialog(&app).contains("src=\"face\""));
        resolve(&mut app, true, false, "expression.changed");
        assert!(app.expressions.preview.is_some());
        assert_eq!(app.error.as_deref(), Some(app.t("expression.changed")));
        app.selected = "other".into();
        resolve(&mut app, false, true, r#"{"token":"t","variants":[]}"#);
        assert!(dialog(&app).is_empty());
    }
}
