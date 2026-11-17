//! 读取绑定模型内的训练素材，验证音频并原子保存角色的情绪选择。
use super::{Candidate, Configuration, Reference};
use crate::{archive::check_entry, companion::storage, voice::Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("emotion.invalid_data".into());
    }
    Ok(bytes)
}

/// 候选路径固定在模型 audio 目录，拒绝绝对路径、路径穿越与重解析点。
pub fn audio_path(directory: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty()
        || id.contains(['/', '\\', ':'])
        || !id.ends_with(".wav")
        || id.starts_with('.')
    {
        return Err("emotion.invalid_data".into());
    }
    let path = directory.join("audio").join(id);
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    Ok(path)
}

/// 从角色的正式训练素材读取 3–10 秒、有日语台词的候选。
pub fn candidates(directory: &Path, character_id: &str) -> Result<Vec<Candidate>, String> {
    let metadata: serde_json::Value =
        serde_json::from_slice(&read(&directory.join("model.json"), 65536)?)
            .map_err(|_| "companion.model_invalid")?;
    let engine = match metadata["format"].as_str() {
        Some("galfriends-gpt-sovits-v5turbo-v1") => Engine::GptSovitsV5Turbo,
        Some("galfriends-gpt-sovits-v2pro-v1") => Engine::GptSovitsV2Pro,
        Some("galfriends-indextts25-lora-v1") => Engine::IndexTts25,
        _ => return Err("companion.model_invalid".into()),
    };
    storage::validate_model(directory, character_id, engine)?;
    #[derive(Deserialize)]
    struct Dataset {
        language: String,
        samples: Vec<Sample>,
    }
    #[derive(Deserialize)]
    struct Sample {
        prepared_audio: String,
        text: String,
        duration: f64,
    }
    let data: Dataset =
        serde_json::from_slice(&read(&directory.join("dataset.json"), 32 * 1024 * 1024)?)
            .map_err(|_| "emotion.invalid_data")?;
    if data.language != "ja" {
        return Err("voice.language_mismatch".into());
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for row in data.samples {
        if !(3.0..=10.0).contains(&row.duration)
            || row.text.chars().count() > 300
            || !row.text.chars().any(|c| ('ぁ'..='ヶ').contains(&c))
        {
            continue;
        }
        // 训练目录可整体移动，只使用清单中原路径的末级文件名。
        let id = row
            .prepared_audio
            .rsplit(['/', '\\'])
            .next()
            .ok_or("emotion.invalid_data")?;
        let path = audio_path(directory, id)?;
        if !path.is_file() {
            return Err("emotion.invalid_data".into());
        }
        if seen.insert(id.to_owned()) {
            result.push(Candidate {
                id: id.into(),
                text: row.text,
                duration: row.duration,
            });
        }
    }
    if result.is_empty() {
        return Err("voice.insufficient_samples".into());
    }
    Ok(result)
}

/// 核对本应用训练器生成的单声道 PCM16 WAV 的真实时长。
fn validate_wave(bytes: &[u8]) -> Result<(), String> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("companion.audio_invalid".into());
    }
    let mut offset = 12;
    let mut rate = None;
    let mut data_len = None;
    while offset + 8 <= bytes.len() {
        let size = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| "companion.audio_invalid")?,
        ) as usize;
        let start = offset + 8;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or("companion.audio_invalid")?;
        if &bytes[offset..offset + 4] == b"fmt " && size >= 16 {
            if bytes[start..start + 4] != [1, 0, 1, 0] || bytes[start + 14..start + 16] != [16, 0] {
                return Err("companion.audio_invalid".into());
            }
            rate = Some(u32::from_le_bytes(
                bytes[start + 4..start + 8]
                    .try_into()
                    .map_err(|_| "companion.audio_invalid")?,
            ));
        }
        if &bytes[offset..offset + 4] == b"data" {
            data_len = Some(size);
        }
        offset = end + size % 2;
    }
    let rate = rate.filter(|r| *r > 0).ok_or("companion.audio_invalid")?;
    let seconds = data_len.ok_or("companion.audio_invalid")? as f64 / (f64::from(rate) * 2.0);
    if !(3.0..=10.0).contains(&seconds) {
        return Err("emotion.duration_invalid".into());
    }
    Ok(())
}

/// 选择和试听共同核对音频，保存哈希以防同名文件被替换。
pub fn reference(directory: &Path, candidate: &Candidate) -> Result<Reference, String> {
    let bytes = read(&audio_path(directory, &candidate.id)?, 8 * 1024 * 1024)?;
    validate_wave(&bytes)?;
    Ok(Reference {
        candidate: candidate.clone(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

/// 推理前重新验证用户确认的文件内容和台词。
pub fn verified_path(directory: &Path, selected: &Reference) -> Result<PathBuf, String> {
    let current = reference(directory, &selected.candidate)?;
    if current.sha256 != selected.sha256 {
        return Err("emotion.changed".into());
    }
    audio_path(directory, &selected.candidate.id)
}

/// 从角色专属目录恢复共用参考。只有首次配置使用传入目录作为素材来源。
pub fn load(path: &Path, id: &str, directory: &str) -> Result<Configuration, String> {
    if !path.exists() {
        return Ok(Configuration {
            character_id: id.into(),
            source_dir: directory.into(),
            ..Default::default()
        });
    }
    let config: Configuration =
        serde_json::from_slice(&read(path, 1024 * 1024)?).map_err(|_| "emotion.invalid_data")?;
    if config.character_id != id {
        return Err("emotion.invalid_data".into());
    }
    Ok(config)
}

/// 调用方持有角色库短事务后原子保存，失败保留原文件。
pub fn save(path: &Path, configuration: &Configuration) -> Result<(), String> {
    storage::write_json(path, configuration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reopening_restores_all_five_choices_independently_for_each_character() {
        use crate::companion::emotions::Emotion;
        let root = tempfile::tempdir().unwrap();
        let expected = |id: &str, emotion: Emotion| Reference {
            candidate: Candidate {
                id: format!("{id}-{}.wav", emotion.code()),
                text: format!("{id}、こんにちは。"),
                duration: 4.0,
            },
            sha256: format!("{id}-{}-digest", emotion.code()),
        };
        for id in ["first-character", "second-character"] {
            let path = storage::binding_path(root.path(), id)
                .unwrap()
                .with_file_name("emotion-references.json");
            for emotion in Emotion::ALL {
                // 和选用命令一样，每次重新读取并提交单个情绪，避免覆盖其他选择。
                let mut config = load(&path, id, "bound-model").unwrap();
                config.selected.insert(emotion, expected(id, emotion));
                save(&path, &config).unwrap();
            }
        }
        for id in ["first-character", "second-character"] {
            let path = storage::binding_path(root.path(), id)
                .unwrap()
                .with_file_name("emotion-references.json");
            let reopened = load(&path, id, "different-inference-model").unwrap();
            assert_eq!(reopened.source_dir, "bound-model");
            assert_eq!(reopened.selected.len(), 5);
            for emotion in Emotion::ALL {
                assert_eq!(
                    serde_json::to_value(reopened.reference_for(emotion).unwrap()).unwrap(),
                    serde_json::to_value(expected(id, emotion)).unwrap()
                );
            }
        }
    }

    #[test]
    fn saved_reference_survives_reload_and_detects_changed_audio() {
        use crate::companion::emotions::Emotion;
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("audio")).unwrap();
        let path = root.path().join("audio/a.wav");
        let mut wav = b"RIFF".to_vec();
        let length = 32000u32 * 2 * 4;
        wav.extend((36 + length).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend([1, 0, 1, 0]);
        wav.extend(32000u32.to_le_bytes());
        wav.extend(64000u32.to_le_bytes());
        wav.extend([2, 0, 16, 0]);
        wav.extend(b"data");
        wav.extend(length.to_le_bytes());
        wav.resize(44 + length as usize, 1);
        fs::write(&path, &wav).unwrap();
        let candidate = Candidate {
            id: "a.wav".into(),
            text: "嬉しいですね。".into(),
            duration: 4.0,
        };
        let selected = reference(root.path(), &candidate).unwrap();
        let config_path = root.path().join("config.json");
        let directory = root.path().to_str().unwrap();
        let mut config = load(&config_path, "role", directory).unwrap();
        config.selected.insert(Emotion::Happy, selected);
        save(&config_path, &config).unwrap();
        // 旧配置用 model_dir 记录来源。切换模型后仍恢复原音频，下一次写入迁移字段名。
        let mut legacy = serde_json::to_value(&config).unwrap();
        legacy["model_dir"] = legacy
            .as_object_mut()
            .unwrap()
            .remove("source_dir")
            .unwrap();
        fs::write(&config_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let loaded = load(&config_path, "role", "different-inference-model").unwrap();
        assert_eq!(loaded.source_dir, directory);
        save(&config_path, &loaded).unwrap();
        let migrated: serde_json::Value =
            serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        assert_eq!(migrated["source_dir"], directory);
        assert!(migrated.get("model_dir").is_none());
        assert_eq!(
            verified_path(root.path(), &loaded.selected[&Emotion::Happy]).unwrap(),
            path
        );
        assert!(load(&config_path, "another-role", directory).is_err());
        let switched = load(&config_path, "role", "another-model").unwrap();
        assert_eq!(switched.source_dir, directory);
        assert_eq!(
            verified_path(
                Path::new(&switched.source_dir),
                &switched.selected[&Emotion::Happy]
            )
            .unwrap(),
            path
        );
        wav[44] = 2;
        fs::write(&path, wav).unwrap();
        assert!(verified_path(root.path(), &loaded.selected[&Emotion::Happy]).is_err());
        fs::write(&config_path, "broken").unwrap();
        assert!(load(&config_path, "role", directory).is_err());
    }
    #[test]
    fn rejects_path_escape_and_short_wave() {
        for id in ["../x.wav", "..\\x.wav", "C:x.wav", "/x.wav"] {
            assert!(audio_path(Path::new("model"), id).is_err());
        }
        assert!(validate_wave(b"RIFFxxxxWAVE").is_err());
    }
}
