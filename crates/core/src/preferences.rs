//! 设置值校验与环境信息协议；桌面路径探测由平台层负责。
use crate::characters::{Settings, Source};
use serde::{Deserialize, Serialize};

/// 主题保存的轻量响应；不携带角色图片或其他设置，避免覆盖并发更新。
#[derive(Debug, Serialize, Deserialize)]
pub struct Appearance {
    pub theme: String,
    pub color_theme: String,
}

/// 跨窗口同步的显示设置；不传输角色图片、代理或服务配置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayPreferences {
    pub language: String,
    pub theme: String,
    pub color_theme: String,
    pub font_size: u8,
}

impl From<&Settings> for DisplayPreferences {
    fn from(settings: &Settings) -> Self {
        Self {
            language: settings.language.clone(),
            theme: settings.theme.clone(),
            color_theme: settings.color_theme.clone(),
            font_size: settings.font_size,
        }
    }
}

/// 只读环境信息。路径来自当前 GalFriends 运行时，不读取 GalGear 配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub library_path: String,
    pub cache_path: String,
    pub waifu2x_path: Option<String>,
}

/// 校验并修改一个离散设置；失败时保持原值。调用方负责原子持久化。
pub fn set(settings: &mut Settings, key: &str, value: &str) -> Result<(), String> {
    match key {
        "ai_provider" if crate::connections::Service::parse(value).is_some_and(|s| s.is_ai()) => {
            settings.ai_provider = value.into()
        }
        "theme" if matches!(value, "light" | "dark") => settings.theme = value.into(),
        "color_theme" if matches!(value, "pink" | "white-pink" | "white") => {
            settings.color_theme = value.into();
        }
        "font_size" => {
            settings.font_size = match value {
                "14" => 14,
                "16" => 16,
                "18" => 18,
                _ => return Err("Invalid font size".into()),
            };
        }
        "language" if matches!(value, "zh-CN" | "ja-JP" | "en-US") => {
            settings.language = value.into();
        }
        "search_source" => {
            settings.search_source = match value {
                "bangumi" => Source::Bangumi,
                "kungal" => Source::Kungal,
                "vndb" => Source::Vndb,
                _ => return Err("Invalid character search source".into()),
            };
        }
        "tag_source" => {
            if value == "auto" {
                settings.tag_source_automatic = true;
                return Ok(());
            }
            settings.tag_source = match value {
                "kungal" => Source::Kungal,
                "vndb" => Source::Vndb,
                _ => return Err("Invalid tag source".into()),
            };
            settings.tag_source_automatic = false;
        }
        _ => return Err("Invalid setting".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::set;
    use crate::characters::{Settings, Source};

    #[test]
    fn old_settings_keep_existing_values_and_default_only_new_fields() {
        let settings: Settings = serde_json::from_str(
            r#"{"theme":"dark","language":"ja-JP","tag_source":"vndb","proxy":"http://localhost:7890"}"#,
        )
        .unwrap();
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.language, "ja-JP");
        assert_eq!(settings.tag_source, Source::Vndb);
        assert_eq!(settings.proxy, "http://localhost:7890");
        assert_eq!(settings.color_theme, "pink");
        assert_eq!(settings.font_size, 16);
        assert_eq!(settings.search_source, Source::Bangumi);
    }

    #[test]
    fn unsupported_sources_and_appearance_values_do_not_modify_settings() {
        let mut settings = Settings::default();
        set(&mut settings, "color_theme", "white-pink").unwrap();
        set(&mut settings, "font_size", "18").unwrap();
        set(&mut settings, "search_source", "vndb").unwrap();
        let before = serde_json::to_value(&settings).unwrap();
        for (key, value) in [
            ("theme", "system"),
            ("color_theme", "unknown"),
            ("font_size", "180"),
            ("search_source", "erogamescape"),
            ("tag_source", "bangumi"),
            ("language", "unknown"),
        ] {
            assert!(set(&mut settings, key, value).is_err());
            assert_eq!(serde_json::to_value(&settings).unwrap(), before);
        }
    }
}
