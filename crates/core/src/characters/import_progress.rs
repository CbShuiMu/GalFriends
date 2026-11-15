//! 导入阶段的共享传输协议；不持久化，不包含桌面或界面依赖。
use serde::{Deserialize, Serialize};

/// 后端正在执行的阶段。图片序号从 1 开始；完成与失败仍以命令响应为准。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ImportProgress {
    #[default]
    Importing,
    Downloading {
        current: usize,
        total: usize,
    },
    CuttingOut {
        current: usize,
        total: usize,
    },
    Upscaling {
        current: usize,
        total: usize,
    },
    Saving,
}
