//! 首次加载及失败恢复的唯一状态区域，复用已有加载请求与错误状态。
use crate::{
    state::App,
    widgets::{esc, icon},
};

pub(super) fn render(a: &App) -> String {
    if let Some(error) = a.error.as_ref().filter(|_| a.pending.is_none()) {
        return format!(
            r#"<section class="startup-state" data-startup="error" aria-labelledby="startup-title"><div class="startup-content"><span class="startup-error-icon" aria-hidden="true">{}</span><h1 id="startup-title" role="alert">{}</h1><details class="startup-error-details"><summary>{}</summary><p>{}</p></details><button class="button secondary" data-action="retry">{} {}</button></div></section>"#,
            icon("error"),
            a.t("startup.failed"),
            a.t("startup.error_details"),
            esc(error),
            icon("refresh"),
            a.t("retry")
        );
    }
    format!(
        r#"<section class="startup-state" data-startup="loading" aria-busy="true"><div class="startup-content" role="status" aria-live="polite" aria-atomic="true"><span class="spinner startup-spinner" aria-hidden="true"></span><h1>{}</h1><p class="startup-description">{}</p></div></section>"#,
        a.t("startup.loading"),
        a.t("startup.description")
    )
}
