//! 表情预览的前后端传输模型。
use serde::{Deserialize, Serialize};

/// 已保存的原始表情列表，token 绑定桌面端持有的选择会话。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpressionPreview {
    pub token: String,
    pub variants: Vec<ExpressionChoice>,
}

/// 游戏原始表情近景；不含与当前立绘融合后的图片。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpressionChoice {
    #[serde(default)]
    pub id: String,
    pub source: String,
    pub layers: Vec<u32>,
    pub thumbnail: String,
}

/// 与弹窗生命周期独立的后台扫描进度；task 排除上一轮任务的迟到事件。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExpressionScan {
    pub character_id: String,
    pub task: u32,
    pub stage: String,
    #[serde(default)]
    pub current: usize,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub error: String,
}

/// 原生图层编号及已合成的透明 PNG。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpressionVariant {
    pub layers: Vec<u32>,
    pub image: String,
    /// 当前立绘上合成的脸部近景，便于比较细微表情。
    pub thumbnail: String,
}
