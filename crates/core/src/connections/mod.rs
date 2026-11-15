//! 数据源与 AI 连接配置；密钥仅存系统凭据库，UI 只接收状态和模型。
use serde::{Deserialize, Serialize};

#[cfg(feature = "desktop")]
pub mod chat;
#[cfg(feature = "desktop")]
pub mod client;
#[cfg(feature = "desktop")]
pub mod credentials;
#[cfg(feature = "desktop")]
pub mod oauth;

/// 固定服务白名单，控制请求端点和凭据命名空间。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    Bangumi,
    Kungal,
    Vndb,
    Openai,
    Deepseek,
    Qwen,
    Kimi,
}
impl Service {
    /// 支持模型发现的 AI 服务。
    pub const AI: [Self; 4] = [Self::Openai, Self::Deepseek, Self::Qwen, Self::Kimi];
    /// 支持可选凭据的角色来源。
    pub const SOURCES: [Self; 3] = [Self::Bangumi, Self::Kungal, Self::Vndb];
    /// 所有可保存凭据的服务，不含公开作品查询。
    pub const ALL: [Self; 7] = [
        Self::Bangumi,
        Self::Kungal,
        Self::Vndb,
        Self::Openai,
        Self::Deepseek,
        Self::Qwen,
        Self::Kimi,
    ];
    /// 桥接和凭据目标使用的稳定标识。
    pub fn key(self) -> &'static str {
        match self {
            Self::Bangumi => "bangumi",
            Self::Kungal => "kungal",
            Self::Vndb => "vndb",
            Self::Openai => "openai",
            Self::Deepseek => "deepseek",
            Self::Qwen => "qwen",
            Self::Kimi => "kimi",
        }
    }
    /// 仅接受白名单标识，未知服务返回 None。
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
    /// 是否支持模型配置。
    pub fn is_ai(self) -> bool {
        Self::AI.contains(&self)
    }
    /// 服务官方名称，用于品牌和系统凭据标签。
    pub fn name(self) -> &'static str {
        match self {
            Self::Bangumi => "Bangumi",
            Self::Kungal => "KUN Gal",
            Self::Vndb => "VNDB",
            Self::Openai => "OpenAI",
            Self::Deepseek => "DeepSeek",
            Self::Qwen => "Qwen",
            Self::Kimi => "Kimi",
        }
    }
    /// 复用 GalGear 授权服务的协议标识；不影响本应用凭据命名空间。
    pub fn oauth_key(self) -> Option<&'static str> {
        match self {
            Self::Bangumi => Some("bangumi"),
            Self::Kungal => Some("nextmoe"),
            _ => None,
        }
    }
}

/// 返回前端的非敏感连接状态。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub service: Service,
    pub configured: bool,
    pub model: String,
}
/// 服务端模型列表中的可选择项。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub name: String,
}
/// 单次服务操作响应，不包含 Token 或 OAuth 轮换信息。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply {
    pub status: Option<Status>,
    pub models: Option<Vec<Model>>,
}

#[cfg(feature = "desktop")]
pub mod emotion_recommendations;
