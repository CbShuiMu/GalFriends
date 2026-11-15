//! 桌面角色资料和双语对白协议；不包含窗口或 DOM 状态。
pub mod emotions;
use crate::{preferences::DisplayPreferences, voice::Engine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 每种引擎独立保留模型目录，切换不会覆盖另一种引擎的绑定。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(from = "StoredModels")]
pub struct VoiceModels {
    pub selected: Engine,
    pub models: BTreeMap<Engine, String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredModels {
    Legacy(String),
    Current {
        selected: Engine,
        models: BTreeMap<Engine, String>,
    },
}
impl From<StoredModels> for VoiceModels {
    fn from(value: StoredModels) -> Self {
        match value {
            StoredModels::Legacy(path) => Self {
                selected: Engine::IndexTts25,
                models: BTreeMap::from([(Engine::IndexTts25, path)]),
            },
            StoredModels::Current { selected, models } => Self { selected, models },
        }
    }
}
impl VoiceModels {
    /// 当前引擎的绑定；尚未绑定时返回空串供界面进入配置状态。
    pub fn directory(&self) -> &str {
        self.models
            .get(&self.selected)
            .map(String::as_str)
            .unwrap_or_default()
    }
}

#[cfg(feature = "desktop")]
pub(crate) mod persona;
#[cfg(feature = "desktop")]
pub mod storage;

/// 对话窗口的背景与展示资料；立绘及语音目录留在本地。
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub introduction: String,
    pub artwork: String,
    pub language: String,
    pub theme: String,
    pub color_theme: String,
    #[serde(default)]
    pub font_size: u8,
    pub model_dir: String,
    #[serde(default)]
    pub voice: VoiceModels,
}

impl Profile {
    /// 只同步显示设置，保留角色身份、立绘和语音绑定。
    pub fn apply_display_preferences(&mut self, preferences: &DisplayPreferences) {
        self.language.clone_from(&preferences.language);
        self.theme.clone_from(&preferences.theme);
        self.color_theme.clone_from(&preferences.color_theme);
        self.font_size = preferences.font_size;
    }
}

/// 同一回复的中文译文、日语原文和经过校验的日语配音段落。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Dialogue {
    #[serde(default)]
    pub emotion: emotions::Emotion,
    pub chinese: String,
    pub japanese: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    segments: Vec<SpeechSegment>,
}

/// 一段应以同一种情绪朗读的日语台词。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeechSegment {
    pub text: String,
    pub emotion: emotions::Emotion,
}

impl Dialogue {
    /// 返回按朗读顺序排列的情绪段落。旧回复和整段回复使用 japanese 与 emotion。
    pub fn speech_segments(&self) -> &[SpeechSegment] {
        &self.segments
    }
}

/// 一个角色的一轮已完成对话；失败或取消的请求不追加历史。
#[derive(Clone, Serialize, Deserialize)]
pub struct Turn {
    pub user: String,
    pub reply: Dialogue,
}

pub(crate) const MAX_SPEECH_SEGMENTS: usize = 5;

/// 接受旧版日语字符串或带情绪的段落数组。用段落原文生成显示文本，避免配音与界面不一致。
pub fn parse_dialogue(text: &str) -> Result<Dialogue, String> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Japanese {
        Whole(String),
        Segmented(Vec<SpeechSegment>),
    }
    #[derive(Deserialize)]
    struct GeneratedDialogue {
        chinese: String,
        japanese: Japanese,
        #[serde(default)]
        emotion: emotions::Emotion,
    }
    const INVALID: &str = "companion.invalid_reply";
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(text)
        .trim();
    let GeneratedDialogue {
        chinese,
        japanese,
        mut emotion,
    } = serde_json::from_str(text).map_err(|_| INVALID)?;
    let mut segments: Vec<SpeechSegment> = Vec::new();
    let japanese = match japanese {
        Japanese::Whole(text) => text,
        Japanese::Segmented(parts) => {
            if !(1..=MAX_SPEECH_SEGMENTS).contains(&parts.len()) {
                return Err(INVALID.into());
            }
            for part in parts {
                if !contains_kana(&part.text) {
                    return Err(INVALID.into());
                }
                if let Some(last) = segments
                    .last_mut()
                    .filter(|last| last.emotion == part.emotion)
                {
                    last.text.push_str(&part.text);
                } else {
                    segments.push(part);
                }
            }
            if let [part] = segments.as_slice() {
                emotion = part.emotion;
            }
            segments.iter().map(|part| part.text.as_str()).collect()
        }
    };
    if chinese.trim().is_empty()
        || chinese.chars().count() > 600
        || japanese.chars().count() > 300
        || !contains_kana(&japanese)
    {
        return Err(INVALID.into());
    }
    Ok(Dialogue {
        chinese,
        japanese,
        emotion,
        segments,
    })
}

fn contains_kana(text: &str) -> bool {
    text.chars().any(|c| ('ぁ'..='ヶ').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::parse_dialogue;
    #[test]
    fn accepts_emotions_and_preserves_old_dialogue() {
        use super::emotions::Emotion;
        assert_eq!(
            parse_dialogue(r#"{"chinese":"你好","japanese":"こんにちは。"}"#)
                .unwrap()
                .emotion,
            Emotion::Normal
        );
        assert_eq!(
            parse_dialogue(r#"{"chinese":"开心","japanese":"嬉しいですね。","emotion":"happy"}"#)
                .unwrap()
                .emotion,
            Emotion::Happy
        );
        assert!(
            parse_dialogue(r#"{"chinese":"你好","japanese":"こんにちは。","emotion":"invented"}"#)
                .is_err()
        );
    }
    #[test]
    fn rejects_chinese_only_and_incomplete_replies() {
        assert!(parse_dialogue(r#"{"chinese":"你好","japanese":"你好"}"#).is_err());
        assert!(parse_dialogue(r#"{"japanese":"こんにちは"}"#).is_err());
        assert!(parse_dialogue(r#"{"chinese":"你好","japanese":"こんにちは。"}"#).is_ok());
    }

    #[test]
    fn speech_segments_preserve_transitions_and_merge_adjacent_identical_emotions() {
        use super::emotions::Emotion::{Happy, Shy};
        let reply = parse_dialogue(
            r#"{"chinese":"你来啦！太好了。……我才没有在等你。","emotion":"happy","japanese":[
                {"text":"来てくれたんだ！","emotion":"happy"},
                {"text":"よかった。","emotion":"happy"},
                {"text":"……待ってたわけじゃないよ。","emotion":"shy"}]}"#,
        )
        .unwrap();
        let parts = reply.speech_segments();
        assert_eq!(
            parts
                .iter()
                .map(|part| (part.text.as_str(), part.emotion))
                .collect::<Vec<_>>(),
            [
                ("来てくれたんだ！よかった。", Happy),
                ("……待ってたわけじゃないよ。", Shy)
            ]
        );
        assert_eq!(
            reply.japanese,
            "来てくれたんだ！よかった。……待ってたわけじゃないよ。"
        );

        let reply = parse_dialogue(
            r#"{"chinese":"太好了！","japanese":[{"text":"よかった！","emotion":"happy"}]}"#,
        )
        .unwrap();
        assert_eq!(reply.emotion, Happy);
    }

    #[test]
    fn rejects_invalid_speech_segments_before_reading_them_aloud() {
        use serde_json::json;
        for parts in [
            json!([]),
            json!([{"text":"ありがとう。","emotion":"unknown"}]),
            json!([{"text":"ありがとう。","emotion":"happy"},{"text":"你好","emotion":"normal"}]),
            json!([{"text":"ありがとう。"}]),
            json!(vec![json!({"text":"やった！","emotion":"happy"}); 6]),
            json!([{"text":"あ".repeat(150),"emotion":"happy"},
                {"text":"い".repeat(151),"emotion":"sad"}]),
        ] {
            let response = json!({"chinese":"你好","japanese":parts});
            assert_eq!(
                parse_dialogue(&response.to_string()).unwrap_err(),
                "companion.invalid_reply"
            );
        }
    }
}
