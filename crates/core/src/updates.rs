//! 发行版本、下载校验和私有 Skill 安装；不依赖桌面运行时。
use serde::{Deserialize, Serialize};

/// 旧设置文件默认使用的 GalFriends 发行页。
pub const RELEASE_PAGE: &str = "https://github.com/CbShuiMu/GalFriends/releases";
/// GalFriends 在构建时使用的 workspace 版本。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// 解包 Skill 的上游仓库。
pub const SKILL_REPOSITORY: &str = "https://github.com/CbShuiMu/Galgear-GalgameReverse-Skill";

/// GitHub 发布的最新可识别版本及其纯文本发布说明。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Release {
    pub tag: String,
    pub notes: Option<String>,
    pub available: bool,
    #[serde(default)]
    pub installable: bool,
}

/// 当前解包 Skill 的本地状态及其与上游的版本差异。
#[derive(Debug, Deserialize, Serialize)]
pub struct SkillStatus {
    pub installed: bool,
    pub commit: Option<String>,
    pub update_available: bool,
}

/// Skill 安装完成后的实际版本与是否更换了资源。
#[derive(Debug, Deserialize, Serialize)]
pub struct SkillInstall {
    pub status: SkillStatus,
    pub changed: bool,
}

/// 安装阶段由后台实际步骤产生，桌面和前端共用传输协议。
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Progress {
    Checking,
    Downloading { percent: Option<u8> },
    Verifying,
    Extracting,
    Installing,
    Restarting,
}

#[cfg(feature = "desktop")]
mod github;
#[cfg(feature = "desktop")]
pub use github::{CheckedRelease, check_release, check_skill, release_page, skill_status};
#[cfg(feature = "desktop")]
pub mod executable;
#[cfg(feature = "desktop")]
pub mod skill;

#[cfg(feature = "desktop")]
fn check_cancel(cancel: &std::sync::atomic::AtomicBool) -> Result<(), galfriends_i18n::Message> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(galfriends_i18n::Message::new("update.cancelled"));
    }
    Ok(())
}
