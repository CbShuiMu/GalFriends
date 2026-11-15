//! 为 GalGear 宿主传递本应用的 AI 配置；凭据只进入 worker 标准输入。
use crate::commands::AppState;
use galfriends_core::connections::{Service, credentials};
use galfriends_i18n::Language;
use serde_json::{Value, json};
use std::path::PathBuf;
use tauri::Manager;

pub(super) fn configure(
    app: &tauri::AppHandle,
    character_id: &str,
    payload: &mut Value,
) -> Result<(), String> {
    let (name, service, proxy, language, credentials_path) = {
        let state = app.state::<AppState>();
        let guard = state.library.lock().map_err(|e| e.to_string())?;
        let library = guard.as_ref().map_err(Clone::clone)?;
        let character = library
            .characters
            .iter()
            .find(|c| c.id == character_id)
            .ok_or("voice.character_required")?;
        (
            character.name.clone(),
            Service::parse(&library.settings.ai_provider).ok_or("voice.agent_provider")?,
            library.settings.proxy.clone(),
            library.settings.language.clone(),
            state.credentials_path.clone(),
        )
    };
    // 当前 GalGear 工具宿主只支持这两个供应商；不把其他服务密钥发往兼容端点。
    if !matches!(service, Service::Openai | Service::Deepseek) {
        return Err("voice.agent_provider".into());
    }
    let credentials = credentials::load(&credentials_path, service)
        .map_err(|e| e.render(Language::from_code(&language).unwrap_or_default()))?;
    if credentials.token.is_empty() || credentials.model.is_empty() {
        return Err("companion.ai_required".into());
    }
    let host = std::env::var_os("GALFRIENDS_GALGEAR_HOST")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../GalGear/crates/core/src/ai_summary")
        });
    if !host.join("agent_host.py").is_file() || !host.join("agent_transport.py").is_file() {
        return Err("voice.agent_missing".into());
    }
    payload["host_dir"] = json!(host.canonicalize().map_err(|e| e.to_string())?);
    payload["character_name"] = json!(name);
    payload["agent"] = json!({"provider":service.key(), "token":credentials.token,
        "model":credentials.model, "proxy":proxy});
    Ok(())
}
