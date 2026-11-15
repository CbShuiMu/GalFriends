//! 角色语音工作台协议；模型、解包与训练在独立可取消的本地工作进程中执行。
use serde::{Deserialize, Serialize};

/// 独立运行环境及模型格式的标识；旧配置默认使用 IndexTTS。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Engine {
    #[default]
    #[serde(rename = "index_tts25")]
    IndexTts25,
    #[serde(rename = "gpt_sovits_v5turbo")]
    GptSovitsV5Turbo,
    #[serde(rename = "gpt_sovits_v2pro")]
    GptSovitsV2Pro,
}
impl Engine {
    /// 选择控件的稳定顺序，保留旧版 IndexTTS 默认项。
    pub const ALL: [Self; 3] = [
        Self::IndexTts25,
        Self::GptSovitsV5Turbo,
        Self::GptSovitsV2Pro,
    ];
    /// 持久化和桥接协议使用的稳定标识。
    pub fn code(self) -> &'static str {
        match self {
            Self::IndexTts25 => "index_tts25",
            Self::GptSovitsV5Turbo => "gpt_sovits_v5turbo",
            Self::GptSovitsV2Pro => "gpt_sovits_v2pro",
        }
    }
    /// 上游模型的产品名称，不随界面语言翻译。
    pub fn name(self) -> &'static str {
        match self {
            Self::IndexTts25 => "IndexTTS 2.5",
            Self::GptSovitsV5Turbo => "GPT-SoVITS v5Turbo",
            Self::GptSovitsV2Pro => "GPT-SoVITS v2Pro",
        }
    }
    /// 只接受当前支持的协议值，不将未知引擎静默降级。
    pub fn from_code(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|engine| engine.code() == value)
    }
}

#[cfg(feature = "desktop")]
pub mod process;

#[cfg(feature = "desktop")]
pub mod base_models;
#[cfg(feature = "desktop")]
pub mod catalog;

/// 可独立下载的基础权重。Irodori 当前仅提供权重下载。
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseModel {
    Index,
    V5Turbo,
    V2Pro,
    Irodori,
}

impl BaseModel {
    /// 下载弹窗的固定选项。
    pub const ALL: [Self; 4] = [Self::Index, Self::V5Turbo, Self::V2Pro, Self::Irodori];

    /// 桥接使用的标识。
    pub fn code(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::V5Turbo => "v5_turbo",
            Self::V2Pro => "v2_pro",
            Self::Irodori => "irodori",
        }
    }

    /// 当前工作台中使用该权重的引擎。
    pub fn engine(self) -> Option<Engine> {
        match self {
            Self::Index => Some(Engine::IndexTts25),
            Self::V5Turbo => Some(Engine::GptSovitsV5Turbo),
            Self::V2Pro => Some(Engine::GptSovitsV2Pro),
            Self::Irodori => None,
        }
    }

    /// 上游产品名及版本。
    pub fn name(self) -> &'static str {
        self.engine().map_or("Irodori-TTS v4.1 Small", Engine::name)
    }
}

/// 与当前角色和引擎匹配的已训练模型。
#[derive(Deserialize, Serialize)]
pub struct TrainedModel {
    pub directory: String,
    pub label: String,
}

/// 用户明确选择的素材、角色和输出位置；不会从游戏名推断角色身份。
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Input {
    #[serde(default)]
    pub engine: Engine,
    pub character_id: String,
    pub game_dir: String,
    /// 从关联作品选择的安装 ID；解包前由后端重新读取并核对路径。
    #[serde(default)]
    pub game_id: String,
    #[serde(default)]
    pub catalog_path: String,
    pub output_dir: String,
    pub manifest: String,
    pub speaker: String,
    pub language: String,
    pub epochs: u32,
}

/// 来自真实工作步骤的进度。总量为零表示当前步骤不能估计百分比。
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Progress {
    pub stage: String,
    pub current: u32,
    pub total: u32,
}

/// 素材中实际观察到的姓名和独立音频数，不根据文件名前缀猜姓名。
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Speaker {
    pub name: String,
    pub clips: u32,
}

/// 后台步骤完成并验证产物后返回；错误不会包装成成功的空结果。
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Reply {
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub manifest: String,
    #[serde(default)]
    pub speakers: Vec<Speaker>,
    #[serde(default)]
    pub artifact: String,
    #[serde(default)]
    pub samples: u32,
    #[serde(default)]
    pub excluded: u32,
}
