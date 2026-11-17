//! 连接命令及每服务任务互斥；网络与授权等待不持有角色库事务锁。
use crate::commands::{self, AppState};
use galfriends_core::{
    characters::Source,
    connections::{
        Reply, Service, Status, client,
        credentials::{self, Credentials},
        oauth,
    },
    sources,
};
use galfriends_i18n::{Language, Message};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::Manager;

#[derive(Default)]
pub(crate) struct Tasks {
    active: Mutex<BTreeMap<Service, Arc<AtomicBool>>>,
}
impl Tasks {
    fn begin(&self, service: Service) -> Result<Arc<AtomicBool>, Message> {
        let mut tasks = self
            .active
            .lock()
            .map_err(|_| Message::new("auth.store_error"))?;
        if tasks.contains_key(&service) {
            return Err(Message::new("sources.oauth_pending"));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        tasks.insert(service, cancelled.clone());
        Ok(cancelled)
    }
    fn finish(&self, service: Service) -> Result<(), Message> {
        self.active
            .lock()
            .map_err(|_| Message::new("auth.store_error"))?
            .remove(&service);
        Ok(())
    }
}

pub(crate) fn catalog_client(
    path: &Path,
    proxy: &str,
    source: Source,
) -> Result<sources::CatalogClient, String> {
    let key = match source {
        Source::Bangumi => "bangumi",
        Source::Kungal => "nextmoe",
        Source::Vndb => "vndb",
        Source::Erogamescape => return sources::catalog_client(proxy, source, ""),
    };
    let value = oauth::load_fresh(
        &client::client(proxy).map_err(|e| e.to_string())?,
        path,
        key,
    )
    .map_err(|e| e.to_string())?;
    sources::catalog_client(proxy, source, &value.token)
}

#[tauri::command]
pub(crate) async fn connection_statuses(app: tauri::AppHandle) -> Result<Vec<Status>, Message> {
    let path = app.state::<AppState>().credentials_path.clone();
    crate::diagnostics::spawn_background("connection_statuses", move || {
        Service::ALL
            .into_iter()
            .map(|s| credentials::status(&path, s))
            .collect()
    })
    .await
    .map_err(|_| Message::new("auth.store_error"))?
}

#[tauri::command]
pub(crate) async fn connection_action(
    app: tauri::AppHandle,
    service: Service,
    action: String,
    token: String,
    model: String,
) -> Result<Reply, Message> {
    let settings = commands::read(&app.state::<AppState>())
        .map_err(|_| Message::new("auth.store_error"))?
        .settings;
    let path = app.state::<AppState>().credentials_path.clone();
    let cancelled = app.state::<Tasks>().begin(service)?;
    let result = crate::diagnostics::spawn_background("connection_action", move || {
        if action == "clear" {
            oauth::transaction(|| credentials::clear(&path, service))?;
            return Ok(Reply {
                status: Some(Status {
                    service,
                    configured: false,
                    model: String::new(),
                }),
                models: None,
            });
        }
        let http = client::client(&settings.proxy)?;
        let mut value = oauth::transaction(|| credentials::load(&path, service))?;
        if action == "login" {
            let source = service
                .oauth_key()
                .ok_or_else(|| Message::new("sources.oauth_unsupported"))?;
            let reply = galfriends_i18n::tr(
                Language::from_code(&settings.language).unwrap_or_default(),
                "sources.oauth_return",
            );
            value = oauth::login(
                &http,
                source,
                value,
                &cancelled,
                |url| open::that_detached(url).map_err(|_| Message::new("sources.oauth_error")),
                reply,
            )?;
            client::check_source(&http, service, &value)?;
        } else {
            if !token.trim().is_empty() {
                value = Credentials {
                    token: token.trim().into(),
                    model: value.model,
                    ..Default::default()
                };
            }
            if value.token.is_empty() {
                return Err(Message::new("connection.no_token"));
            }
            match action.as_str() {
                "models" => {
                    return Ok(Reply {
                        status: None,
                        models: Some(client::models(&http, service, &value.token)?),
                    });
                }
                "save" if service.is_ai() => {
                    let models = client::models(&http, service, &value.token)?;
                    if !models.iter().any(|m| m.id == model) {
                        return Err(Message::new("ai.model_unavailable"));
                    }
                    value.model = model;
                }
                "save" => client::check_source(&http, service, &value)?,
                _ => return Err(Message::new("connection.invalid_service")),
            }
        }
        oauth::transaction(|| {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Message::new("sources.oauth_cancelled"));
            }
            credentials::save(&path, service, &value)
        })?;
        Ok(Reply {
            status: Some(Status {
                service,
                configured: true,
                model: value.model,
            }),
            models: None,
        })
    })
    .await
    .map_err(|_| Message::new("connection.network_error"));
    app.state::<Tasks>().finish(service)?;
    result?
}

#[tauri::command]
pub(crate) fn cancel_connection(app: tauri::AppHandle, service: Service) -> Result<(), Message> {
    if let Some(cancelled) = app
        .state::<Tasks>()
        .active
        .lock()
        .map_err(|_| Message::new("auth.store_error"))?
        .get(&service)
    {
        cancelled.store(true, Ordering::Relaxed);
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn open_connection_website(service: String) -> Result<(), Message> {
    open::that_detached(client::website(&service)?)
        .map_err(|_| Message::new("connection.network_error"))
}
