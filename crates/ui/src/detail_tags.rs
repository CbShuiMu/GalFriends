//! 角色标签的显示编辑、筛选和排序；只提交意图，响应成功后关闭草稿。
use crate::{
    requests::{self, Kind},
    state::{App, Page},
    widgets::{esc, icon_button},
};
use galfriends_core::tags;
use serde_json::json;

#[derive(Default)]
pub(crate) struct State {
    pub editing: Option<String>,
    pub name: String,
    pub removing: Option<String>,
}

pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    if !action.starts_with("detail-tag") {
        return false;
    }
    let Some(c) = a.current() else {
        return true;
    };
    let id = c.id.clone();
    let selected = tags::visible(c, a.library.settings.effective_tag_source())
        .into_iter()
        .find(|tag| tag.key == value);
    match action {
        "detail-tag-add" => {
            a.detail_tags.editing = Some(String::new());
            a.detail_tags.name.clear();
        }
        "detail-tag-source" => {
            let name = c.name.clone();
            let source = tags::source(c, a.library.settings.effective_tag_source());
            a.tag_source_target = Some(id);
            a.link_target = None;
            a.query = name;
            a.search_source = source.key().into();
            a.search_open = true;
            a.results.clear();
            a.searched = false;
        }
        "detail-tag-retry" => {
            let source = tags::source(c, a.library.settings.effective_tag_source());
            crate::tag_fetch::retry(a, &id, source);
        }
        "detail-tag" => {
            if let Some(tag) = selected {
                let name = tag.name.to_owned();
                a.library_ui = Default::default();
                a.library_ui.tags.insert(name);
                a.filter.clear();
                a.collection.clear();
                a.page = Page::Library;
            }
        }
        "detail-tag-edit" => {
            if let Some(tag) = selected {
                let name = tag.name.to_owned();
                let key = tag.key;
                a.detail_tags.name = name;
                a.detail_tags.editing = Some(key);
            }
        }
        "detail-tag-cancel" => a.detail_tags.editing = None,
        "detail-tag-remove" => {
            if let Some(tag) = selected {
                a.detail_tags.removing = Some(tag.key);
            }
        }
        "detail-tag-remove-cancel" => a.detail_tags.removing = None,
        "detail-tag-save"
        | "detail-tag-remove-confirm"
        | "detail-tag-reset"
        | "detail-tag-reorder" => {
            let (operation, payload) = match action {
                "detail-tag-save" => {
                    let Some(key) = &a.detail_tags.editing else {
                        return true;
                    };
                    if a.detail_tags.name.trim().is_empty() {
                        return true;
                    }
                    (
                        if key.is_empty() {
                            "tag-add"
                        } else {
                            "tag-rename"
                        },
                        json!({"key":key,"name":a.detail_tags.name}),
                    )
                }
                "detail-tag-remove-confirm" => {
                    let Some(key) = &a.detail_tags.removing else {
                        return true;
                    };
                    ("tag-hide", json!({"key":key}))
                }
                "detail-tag-reorder" => {
                    let Ok(movement) = serde_json::from_str::<serde_json::Value>(value) else {
                        return true;
                    };
                    if movement["id"].as_str() != Some(&id) {
                        return true;
                    }
                    (
                        "tag-reorder",
                        json!({"key":movement["tag"],"before":movement["before"],"source":tags::source(c, a.library.settings.effective_tag_source())}),
                    )
                }
                _ => ("tag-reset", json!({})),
            };
            requests::send(
                a,
                Kind::Tags,
                "mutate_library",
                json!({"action":operation,"id":id,"value":payload.to_string()}),
            );
        }
        _ => {}
    }
    true
}

pub(crate) fn render(a: &App) -> String {
    let Some(c) = a.current() else {
        return String::new();
    };
    let busy = a.pending.is_some();
    let mut html = format!(
        r#"<section class="hero-tags"><h2>{}<span class="source-label">{}</span></h2><div class="game-tags" data-tag-reorder="{}">"#,
        a.t("tags"),
        tags::source(c, a.library.settings.effective_tag_source()).key(),
        !busy && a.detail_tags.editing.is_none()
    );
    let tags = tags::visible(c, a.library.settings.effective_tag_source());
    for tag in &tags {
        if a.detail_tags.editing.as_deref() == Some(&tag.key) {
            html.push_str(&input(a, busy));
        } else {
            html.push_str(&format!(r#"<span class="detail-tag-chip" title="{} · {}">{}<button class="tag-button" data-action="detail-tag" data-value="{}">{}</button>{}</span>"#,esc(tag.original.map(|original| original.group.as_str()).unwrap_or(a.t("tags.local"))),a.t("tags.reorder"),icon_button("detail-tag-edit",&tag.key,a.t("tags.edit"),"edit","tag-edit-button",busy),esc(&tag.key),esc(tag.name),icon_button("detail-tag-remove",&tag.key,a.t("tags.hide"),"close","tag-remove-button",busy)));
        }
    }
    if tags.is_empty() {
        html.push_str(&format!("<p class=\"muted\">{}</p>", a.t("no_tags")));
    }
    html.push_str("<span class=\"tag-inline-actions\">");
    if a.detail_tags.editing.as_deref() == Some("") {
        html.push_str(&input(a, busy));
    } else {
        html.push_str(&icon_button(
            "detail-tag-add",
            "",
            a.t("tags.add"),
            "plus",
            "tag-add-button",
            busy,
        ));
    }
    let fetching = a
        .tag_fetch
        .pending
        .as_ref()
        .is_some_and(|(_, id, _)| id == &c.id);
    html.push_str(&icon_button(
        "detail-tag-source",
        "",
        a.t("tags.replace_source"),
        "database",
        "tag-source-button",
        busy || fetching,
    ));
    html.push_str("</span>");
    let changed = &c.tag_presentation;
    if !changed.names.is_empty() || !changed.hidden.is_empty() || !changed.order.is_empty() {
        html.push_str(&format!(
            r#"<span class="tag-inline-actions">{}</span>"#,
            icon_button(
                "detail-tag-reset",
                "",
                a.t("tags.reset"),
                "refresh",
                "tag-source-button",
                busy
            )
        ));
    }
    let source = tags::source(c, a.library.settings.effective_tag_source());
    if fetching {
        html.push_str(&format!(
            "<p role=\"status\" class=\"muted small\">{}</p>",
            a.t("tags.fetching")
        ));
    } else if !c.snapshots.iter().any(|s| s.source == source)
        && let Some(error) = a.tag_fetch.errors.get(&(c.id.clone(), source.key().into()))
    {
        html.push_str(&format!(
            "<p role=\"status\" class=\"muted small\">{} {}</p>{}",
            a.t("tags.fetch_failed"),
            esc(error),
            icon_button(
                "detail-tag-retry",
                "",
                a.t("tags.retry"),
                "refresh",
                "tag-source-button",
                busy
            )
        ));
    }
    html.push_str(&format!(
        "</div><p class=\"muted small\">{}</p></section>",
        a.t("safe_tags")
    ));
    html
}

pub(crate) fn dialog(a: &App) -> String {
    let Some(key) = &a.detail_tags.removing else {
        return String::new();
    };
    let name = a
        .current()
        .and_then(|c| {
            tags::visible(c, a.library.settings.effective_tag_source())
                .into_iter()
                .find(|tag| &tag.key == key)
                .map(|tag| tag.name)
        })
        .unwrap_or_default();
    format!(
        r#"<div class="editor-overlay"><section class="editor-dialog small-dialog" role="dialog" aria-modal="true" aria-labelledby="tag-remove-title"><h2 id="tag-remove-title">{}</h2><p>{}</p><p>{}</p>{}</section></div>"#,
        a.t("tags.hide"),
        esc(name),
        a.t("tags.keep_original"),
        crate::widgets::confirmation_actions(
            "detail-tag-remove-confirm",
            "detail-tag-remove-cancel",
            a.t("confirm"),
            a.t("cancel"),
            a.pending.is_some()
        )
    )
}

fn input(a: &App, busy: bool) -> String {
    format!(
        r#"<span class="detail-tag-chip tag-input-chip"><span class="tag-input-wrap" data-value="{}"><input id="detail-tag-input" size="1" autocomplete="off" data-input="detail-tag-input" value="{}" maxlength="120" aria-label="{}" {}></span>{}</span>"#,
        esc(&a.detail_tags.name),
        esc(&a.detail_tags.name),
        a.t(if a.detail_tags.editing.as_deref() == Some("") {
            "tags.add"
        } else {
            "tags.edit"
        }),
        if busy { "disabled" } else { "" },
        icon_button(
            "detail-tag-cancel",
            "",
            a.t("cancel"),
            "close",
            "tag-cancel-button",
            busy
        )
    )
}

#[cfg(test)]
mod tests {
    use super::{handle, render};
    use crate::state::{App, Page};

    #[test]
    fn add_and_source_actions_work_with_an_empty_tag_list() {
        let mut a = App {
            page: Page::Detail,
            ..Default::default()
        };
        let c = galfriends_core::characters::sample().unwrap();
        a.selected = c.id.clone();
        a.library.characters.push(c);
        let html = render(&a);
        assert!(html.contains("data-action=\"detail-tag-add\""));
        assert!(html.contains("data-action=\"detail-tag-source\""));
        handle(&mut a, "detail-tag-add", "");
        assert!(render(&a).contains("id=\"detail-tag-input\""));
        handle(&mut a, "detail-tag-cancel", "");
        a.library.settings.language = "ja-JP".into();
        handle(&mut a, "detail-tag-source", "");
        assert_eq!(a.tag_source_target.as_deref(), Some(a.selected.as_str()));
        assert_eq!(a.search_source, "vndb");
    }
}
