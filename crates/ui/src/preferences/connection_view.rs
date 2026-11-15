//! GalGear 数据源卡片、AI 双列卡片与可搜索模型选择器。
use super::{connections::Provider, view::segmented};
use crate::{
    state::App,
    widgets::{esc, icon, icon_button, source_icon},
};
use galfriends_core::connections::Service;
use galfriends_i18n::Language;

fn language(a: &App) -> Language {
    Language::from_code(&a.library.settings.language).unwrap_or_default()
}
fn disabled(value: bool) -> &'static str {
    if value { "disabled" } else { "" }
}
fn status(a: &App, service: Service, p: &Provider) -> String {
    let key = if a.preferences.connections.busy(service) {
        "ai.loading"
    } else if !a.preferences.connections.loaded {
        "connection.unknown"
    } else if p.configured {
        if service.is_ai() {
            "ai.configured"
        } else {
            "sources.saved"
        }
    } else if service.is_ai() {
        "ai.not_configured"
    } else {
        "sources.public"
    };
    format!(
        r#"<span class="connection-status {}" role="status">{}</span>"#,
        if p.configured { "connected" } else { "" },
        a.t(key)
    )
}
fn errors(a: &App, p: &Provider) -> String {
    if let Some(error) = &p.error {
        format!(
            r#"<p class="account-notice" role="alert">{}</p>"#,
            esc(&error.render(language(a)))
        )
    } else if let Some(notice) = p.notice {
        format!(
            r#"<p class="connection-notice" role="status">{} {}</p>"#,
            icon("check"),
            a.t(notice)
        )
    } else {
        String::new()
    }
}
fn load_error(a: &App) -> String {
    if let Some(error) = &a.preferences.connections.error {
        format!(
            r#"<div class="ai-load-error"><p class="account-notice" role="alert">{}</p>{}</div>"#,
            esc(&error.render(language(a))),
            icon_button(
                "connection-retry",
                "",
                a.t("retry"),
                "refresh",
                "quiet",
                false
            )
        )
    } else {
        String::new()
    }
}
fn token(a: &App, service: Service, p: &Provider, busy: bool) -> String {
    let key = service.key();
    format!(
        r#"<label class="edit-field" for="connection-token-{key}"><span>Token {}</span><input id="connection-token-{key}" data-input="connection-token:{key}" type="password" value="{}" autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="1024" placeholder="{}" {}></label>"#,
        if service.is_ai() {
            "<small>API Key</small>"
        } else {
            ""
        },
        esc(&p.token),
        a.t(if p.configured {
            "sources.keep_saved"
        } else if service.is_ai() {
            "ai.token_placeholder"
        } else {
            "sources.optional"
        }),
        disabled(busy)
    )
}
pub(super) fn sources(a: &App) -> String {
    let mut html = format!(
        r#"<section class="settings-card source-defaults"><div class="source-default"><h2>{}</h2>{}</div><div class="source-default"><h2>{}</h2>{}</div></section>"#,
        a.t("prefs.default_source"),
        segmented(
            a,
            "search_source",
            a.t("prefs.default_source"),
            a.library.settings.search_source.key(),
            &[
                ("vndb", "VNDB"),
                ("bangumi", "Bangumi"),
                ("kungal", a.t("prefs.kungal"))
            ]
        ),
        a.t("tag_source"),
        segmented(
            a,
            "tag_source",
            a.t("tag_source"),
            if a.library.settings.tag_source_automatic {
                "auto"
            } else {
                a.library.settings.tag_source.key()
            },
            &[
                ("auto", a.t("tags.auto")),
                ("kungal", a.t("prefs.kungal")),
                ("vndb", "VNDB")
            ],
        )
    );
    html.push_str(&load_error(a));
    for source in ["kungal", "bangumi", "vndb", "erogamescape"] {
        let name = match source {
            "kungal" => a.t("prefs.kungal"),
            "erogamescape" => a.t("prefs.erogamescape"),
            "vndb" => "VNDB",
            _ => "Bangumi",
        };
        let Some(service) = Service::parse(source) else {
            html.push_str(&format!(r#"<section class="settings-card source-card" data-service="{source}"><div class="panel-heading"><h2 class="source-card-title">{}{name}</h2><span class="connection-status">{}</span></div><p class="field-help">{}</p><div class="account-actions">{}</div></section>"#,source_icon(source),a.t("sources.public"),a.t("works_only"),icon_button("connection-open",source,a.t("connection.open_website"),"open_in_new","quiet",false)));
            continue;
        };
        let p = &a.preferences.connections.providers[&service];
        let busy = a.pending.is_some()
            || a.preferences.connections.busy(service)
            || !a.preferences.connections.loaded;
        html.push_str(&format!(r#"<section class="settings-card source-card" data-service="{source}"><div class="panel-heading"><h2 class="source-card-title">{}{name}</h2>{}</div>{}"#,source_icon(source),status(a,service,p),token(a,service,p,busy)));
        html.push_str(&format!(
            r#"<p class="field-help">{}</p>{}<div class="account-actions">{}"#,
            a.t(if service == Service::Kungal {
                "connection.kungal_help"
            } else {
                "connection.source_hint"
            }),
            errors(a, p),
            icon_button(
                "connection-save",
                source,
                a.t("auth.save_check"),
                "save",
                "primary",
                busy || !p.has_token()
            )
        ));
        if service.oauth_key().is_some() {
            let logging = a.preferences.connections.logging_in(service);
            html.push_str(&icon_button(
                if logging {
                    "connection-cancel"
                } else {
                    "connection-login"
                },
                source,
                a.t(if logging {
                    "sources.oauth_cancel"
                } else {
                    "sources.oauth_login"
                }),
                if logging { "close" } else { "open_in_new" },
                "secondary",
                !logging && busy,
            ));
        } else {
            html.push_str(&icon_button(
                "connection-open",
                source,
                a.t("connection.open_website"),
                "open_in_new",
                "quiet",
                false,
            ));
        }
        html.push_str(&icon_button(
            "connection-clear",
            source,
            a.t("auth.clear"),
            "close",
            "quiet",
            busy || !p.has_token(),
        ));
        html.push_str("</div></section>");
    }
    html.push_str(&format!(
        r#"<p class="field-help ai-vault-note">{}</p>"#,
        a.t("auth.vault")
    ));
    html
}

pub(super) fn ai(a: &App) -> String {
    let mut html = format!(
        r#"<div class="ai-active-provider"><span class="ai-active-label">{}</span><div id="settings-ai_provider" class="segmented-control ai-provider-switch" role="group" aria-label="{}"><span class="segmented-thumb" aria-hidden="true"></span>"#,
        a.t("ai.active_provider"),
        a.t("ai.active_provider")
    );
    for service in Service::AI {
        let key = service.key();
        let selected = a.library.settings.ai_provider == key;
        html.push_str(&format!(r#"<button type="button" class="segment" data-action="settings-ai_provider" data-value="{key}" aria-pressed="{selected}" tabindex="{}" {}><img class="ai-switch-logo ai-logo-{key}" src="assets/{key}.svg" alt=""><span>{}</span></button>"#,if selected{"0"}else{"-1"},disabled(a.pending.is_some()),service.name()));
    }
    html.push_str("</div></div>");
    html.push_str(&load_error(a));
    html.push_str(r#"<div class="ai-provider-grid">"#);
    for service in Service::AI {
        let key = service.key();
        let p = &a.preferences.connections.providers[&service];
        let busy = a.pending.is_some()
            || a.preferences.connections.busy(service)
            || !a.preferences.connections.loaded;
        html.push_str(&format!(r#"<section class="settings-card ai-provider-card" data-service="{key}" aria-labelledby="ai-title-{key}"><div class="ai-provider-heading"><img class="ai-provider-logo ai-logo-{key}" src="assets/{key}.svg" alt=""><div class="ai-provider-name"><h2 id="ai-title-{key}">{}</h2>{}</div></div>{}<div class="ai-model-heading"><label for="model-{key}">{}</label>{}</div>{}"#,service.name(),status(a,service,p),token(a,service,p,busy),a.t("ai.model"),icon_button("connection-models",key,a.t("ai.refresh"),"refresh","quiet",busy||!p.has_token()),model_picker(a,service,p,busy)));
        let hint = if a.preferences.connections.busy(service) {
            a.t("ai.loading").into()
        } else if !p.loaded {
            a.t("ai.models_hint").into()
        } else if p.models.is_empty() {
            a.t("ai.empty").into()
        } else if p.selected.is_empty() {
            a.t("ai.select_model_hint").into()
        } else {
            a.t("ai.model_count")
                .replace("{count}", &p.models.len().to_string())
        };
        html.push_str(&format!(r#"<p class="field-help ai-model-help" role="status">{}</p>{}<div class="account-actions ai-provider-actions">{}{}</div></section>"#,esc(&hint),errors(a,p),icon_button("connection-save",key,a.t("ai.save"),"save","primary",busy||!p.can_save()),icon_button("connection-clear",key,a.t("auth.clear"),"close","quiet",busy||!p.has_token())));
    }
    html.push_str(&format!(
        r#"</div><p class="field-help ai-vault-note">{}</p>"#,
        a.t("ai.vault")
    ));
    html
}

fn model_picker(a: &App, service: Service, p: &Provider, busy: bool) -> String {
    let key = service.key();
    let open = a.preferences.connections.model_menu == Some(service);
    let label = if p.selected.is_empty() {
        a.t("ai.model_placeholder")
    } else {
        &p.selected
    };
    let mut html = format!(
        r#"<div class="choice-control model-picker"><button id="model-{key}" class="choice-trigger {}" data-action="model-open" data-value="{key}" aria-label="{}" aria-haspopup="listbox" aria-expanded="{open}" aria-controls="model-list-{key}" {}><span class="choice-copy"><span>{}</span></span>{}</button>"#,
        if open { "open" } else { "" },
        a.t("ai.model"),
        disabled(busy || !p.loaded || p.models.is_empty()),
        esc(label),
        icon("chevron")
    );
    if open {
        let query = a.preferences.connections.model_query.to_lowercase();
        html.push_str(&format!(r#"<div class="choice-popup model-popup"><input id="model-query" data-input="model-query" value="{}" placeholder="{}" aria-label="{}" autocomplete="off"><div id="model-list-{key}" class="model-options" role="listbox" aria-label="{}">"#,esc(&a.preferences.connections.model_query),a.t("connection.search_models"),a.t("connection.search_models"),a.t("ai.model")));
        let mut count = 0;
        for model in &p.models {
            if !model.id.to_lowercase().contains(&query)
                && !model.name.to_lowercase().contains(&query)
            {
                continue;
            }
            count += 1;
            let selected = model.id == p.selected;
            html.push_str(&format!(r#"<button class="choice-option {}" role="option" aria-selected="{selected}" data-action="model-select" data-value="{}"><span>{}</span><span class="choice-check">{}</span></button>"#,if selected{"selected"}else{""},esc(&model.id),esc(&model.id),if selected{icon("check")}else{String::new()}));
        }
        if count == 0 {
            html.push_str(&format!(
                r#"<p class="field-help">{}</p>"#,
                a.t("no_results")
            ));
        }
        html.push_str("</div></div>");
    }
    html.push_str("</div>");
    html
}
