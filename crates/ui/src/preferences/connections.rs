//! 服务配置草稿与独立请求；单个服务等待授权时，本地页面仍可操作。
use crate::{requests, state::App};
use galfriends_core::connections::{Model, Reply, Service, Status};
use galfriends_i18n::Message;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Provider {
    pub configured: bool,
    pub token: String,
    pub selected: String,
    pub models: Vec<Model>,
    pub loaded: bool,
    pub error: Option<Message>,
    pub notice: Option<&'static str>,
}
impl Provider {
    pub fn has_token(&self) -> bool {
        self.configured || !self.token.trim().is_empty()
    }
    pub fn can_save(&self) -> bool {
        self.has_token() && self.loaded && self.models.iter().any(|m| m.id == self.selected)
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Request {
    Statuses,
    Models(Service),
    Save(Service),
    Clear(Service),
    Login(Service),
    Cancel,
    Open,
}
impl Request {
    fn service(self) -> Option<Service> {
        match self {
            Self::Models(s) | Self::Save(s) | Self::Clear(s) | Self::Login(s) => Some(s),
            _ => None,
        }
    }
}
pub(crate) struct State {
    pub providers: BTreeMap<Service, Provider>,
    pub loaded: bool,
    pub error: Option<Message>,
    pub pending: BTreeMap<u32, Request>,
    pub model_menu: Option<Service>,
    pub model_query: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            providers: Service::ALL
                .into_iter()
                .map(|s| (s, Provider::default()))
                .collect(),
            loaded: false,
            error: None,
            pending: BTreeMap::new(),
            model_menu: None,
            model_query: String::new(),
        }
    }
}
impl State {
    pub fn busy(&self, service: Service) -> bool {
        self.pending
            .values()
            .any(|r| matches!(r, Request::Statuses) || r.service() == Some(service))
    }
    pub fn logging_in(&self, service: Service) -> bool {
        self.pending
            .values()
            .any(|r| matches!(r,Request::Login(s) if *s==service))
    }
}
pub(crate) fn load(a: &mut App) {
    if !a.preferences.connections.loaded
        && !a
            .preferences
            .connections
            .pending
            .values()
            .any(|r| matches!(r, Request::Statuses))
    {
        a.preferences.connections.error = None;
        requests::connection(a, Request::Statuses, "connection_statuses", json!({}));
    }
}
pub(crate) fn handle(a: &mut App, action: &str, value: &str) -> bool {
    match action {
        "connection-retry" => {
            a.preferences.connections.loaded = false;
            load(a);
            return true;
        }
        "connection-open" => {
            requests::connection(
                a,
                Request::Open,
                "open_connection_website",
                json!({"service":value}),
            );
            return true;
        }
        "model-close" => {
            a.preferences.connections.model_menu = None;
            return true;
        }
        "model-select" => {
            if let Some(service) = a.preferences.connections.model_menu
                && !a.preferences.connections.busy(service)
                && let Some(p) = a.preferences.connections.providers.get_mut(&service)
                && p.models.iter().any(|m| m.id == value)
            {
                p.selected = value.into();
                p.notice = None;
            }
            a.preferences.connections.model_menu = None;
            return true;
        }
        _ => {}
    }
    if !matches!(
        action,
        "connection-models"
            | "connection-save"
            | "connection-clear"
            | "connection-login"
            | "connection-cancel"
            | "model-open"
    ) {
        return false;
    }
    let Some(service) = Service::parse(value) else {
        return true;
    };
    if action == "connection-cancel" {
        if a.preferences.connections.logging_in(service) {
            requests::connection(
                a,
                Request::Cancel,
                "cancel_connection",
                json!({"service":service}),
            );
        }
        return true;
    }
    if a.preferences.connections.busy(service) {
        return true;
    }
    let Some(p) = a.preferences.connections.providers.get_mut(&service) else {
        return true;
    };
    if action == "model-open" {
        if p.loaded && !p.models.is_empty() {
            a.preferences.connections.model_menu = Some(service);
            a.preferences.connections.model_query.clear();
        }
        return true;
    }
    let (request, operation) = match action {
        "connection-models" if p.has_token() => (Request::Models(service), "models"),
        "connection-save"
            if if service.is_ai() {
                p.can_save()
            } else {
                p.has_token()
            } =>
        {
            (Request::Save(service), "save")
        }
        "connection-clear" => (Request::Clear(service), "clear"),
        "connection-login" if service.oauth_key().is_some() => (Request::Login(service), "login"),
        _ => return true,
    };
    p.error = None;
    p.notice = None;
    let payload = json!({"service":service,"action":operation,"token":p.token,"model":p.selected});
    requests::connection(a, request, "connection_action", payload);
    true
}
pub(crate) fn input(a: &mut App, name: &str, value: &str) -> bool {
    if name == "model-query" {
        a.preferences.connections.model_query = value.into();
        return true;
    }
    let Some(service) = name
        .strip_prefix("connection-token:")
        .and_then(Service::parse)
    else {
        return false;
    };
    if !a.preferences.connections.busy(service)
        && let Some(p) = a.preferences.connections.providers.get_mut(&service)
    {
        p.token = value.into();
        p.loaded = false;
        p.models.clear();
        p.selected.clear();
        p.error = None;
        p.notice = None;
        a.preferences.connections.model_menu = None;
    }
    true
}
pub(crate) fn resolve(a: &mut App, id: u32, success: bool, payload: &str) -> bool {
    let Some(request) = a.preferences.connections.pending.remove(&id) else {
        return false;
    };
    let result: Result<(), Message> = (|| {
        if !success {
            return Err(Message::from_error(payload));
        }
        match request {
            Request::Statuses => {
                let statuses: Vec<Status> = serde_json::from_str(payload)
                    .map_err(|_| Message::new("connection.invalid_response"))?;
                if Service::ALL
                    .iter()
                    .any(|service| !statuses.iter().any(|s| s.service == *service))
                {
                    return Err(Message::new("connection.invalid_response"));
                }
                for status in statuses {
                    if let Some(p) = a.preferences.connections.providers.get_mut(&status.service) {
                        p.configured = status.configured;
                        if p.token.is_empty() && !p.loaded {
                            p.selected = status.model;
                        }
                    }
                }
                a.preferences.connections.loaded = true;
                a.preferences.connections.error = None;
            }
            Request::Cancel | Request::Open => {}
            _ => {
                let reply: Reply = serde_json::from_str(payload)
                    .map_err(|_| Message::new("connection.invalid_response"))?;
                if let Some(service) = request.service()
                    && let Some(p) = a.preferences.connections.providers.get_mut(&service)
                {
                    if matches!(request, Request::Models(_)) {
                        let models = reply
                            .models
                            .ok_or_else(|| Message::new("connection.invalid_response"))?;
                        if !models.iter().any(|m| m.id == p.selected) {
                            p.selected.clear();
                        }
                        p.models = models;
                        p.loaded = true;
                    } else {
                        let status = reply
                            .status
                            .filter(|s| s.service == service)
                            .ok_or_else(|| Message::new("connection.invalid_response"))?;
                        if matches!(request, Request::Clear(_)) {
                            *p = Provider::default();
                        }
                        p.configured = status.configured;
                        p.selected = status.model;
                        p.token.clear();
                        p.notice = Some(if matches!(request, Request::Clear(_)) {
                            "connection.cleared"
                        } else if matches!(request, Request::Login(_)) {
                            "sources.logged_in"
                        } else {
                            "saved"
                        });
                    }
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Some(service) = request.service()
            && let Some(p) = a.preferences.connections.providers.get_mut(&service)
        {
            p.error = Some(error);
        } else {
            a.preferences.connections.error = Some(error);
        }
    }
    true
}
