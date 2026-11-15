//! 角色共用的情绪参考音频协议；候选始终在保存的素材来源中解析。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "desktop")]
pub mod storage;

/// 固定五种对白情绪，旧回复缺省为正常。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emotion {
    #[default]
    Normal,
    Happy,
    Sad,
    Angry,
    Shy,
}
impl Emotion {
    /// 按界面显示顺序列出支持的情绪。
    pub const ALL: [Self; 5] = [Self::Normal, Self::Happy, Self::Sad, Self::Angry, Self::Shy];
    /// 用于桥接参数和国际化消息的稳定编号。
    pub fn code(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Happy => "happy",
            Self::Sad => "sad",
            Self::Angry => "angry",
            Self::Shy => "shy",
        }
    }
}

/// 来自同一角色训练数据的短音频；编号为本地 audio 目录下的文件名。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub text: String,
    pub duration: f64,
}

/// 用户确认的参考及内容摘要，文件变化时拒绝继续推理。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub candidate: Candidate,
    pub sha256: String,
}

/// 每个角色共用的推荐和确认结果，切换推理模型保留素材来源与已选参考。
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Configuration {
    pub character_id: String,
    /// 素材来源独立于当前推理模型；兼容旧配置的 model_dir 字段。
    #[serde(alias = "model_dir")]
    pub source_dir: String,
    pub recommendations: BTreeMap<Emotion, Vec<Candidate>>,
    pub selected: BTreeMap<Emotion, Reference>,
}

impl Configuration {
    /// 优先使用对应情绪，缺省回退到正常；都未选时由引擎使用原始参考。
    pub fn reference_for(&self, emotion: Emotion) -> Option<&Reference> {
        self.selected
            .get(&emotion)
            .or_else(|| self.selected.get(&Emotion::Normal))
    }
}

#[cfg(test)]
mod tests {
    use super::{Candidate, Configuration, Emotion, Reference};
    #[test]
    fn routes_all_emotions_to_their_selected_audio_with_normal_fallback() {
        let mut config = Configuration::default();
        assert!(config.reference_for(Emotion::Happy).is_none());
        for emotion in Emotion::ALL {
            config.selected.insert(
                emotion,
                Reference {
                    candidate: Candidate {
                        id: format!("{}.wav", emotion.code()),
                        text: "こんにちは。".into(),
                        duration: 4.0,
                    },
                    sha256: String::new(),
                },
            );
        }
        for emotion in Emotion::ALL {
            assert_eq!(
                config.reference_for(emotion).unwrap().candidate.id,
                format!("{}.wav", emotion.code())
            );
        }
        config.selected.remove(&Emotion::Happy);
        assert_eq!(
            config.reference_for(Emotion::Happy).unwrap().candidate.id,
            "normal.wav"
        );
    }
}

/// 主窗口只接收悬浮角色的轻量身份。
#[derive(Clone, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub name: String,
}

/// 当前悬浮角色及其配置；不传输完整立绘和训练数据。
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PageData {
    pub targets: Vec<Target>,
    pub configuration: Option<Configuration>,
}
