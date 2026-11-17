//! 角色与已训练日语模型的显式绑定；文件只记录路径，不复制或修改模型。
use crate::archive::check_entry;
use crate::{companion::VoiceModels, voice::Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("companion.model_invalid".into());
    }
    Ok(bytes)
}

/// 读取语音目录登记文件，兼容尚未登记输出目录的安装。
pub(crate) fn read_output_roots(path: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    match fs::metadata(path) {
        Ok(_) => {
            serde_json::from_slice(&read_bounded(path, 1024 * 1024)?).map_err(|e| e.to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// 稳定窗口标识，不将外部角色 ID 直接作为文件名或窗口标签。
pub fn label(id: &str) -> String {
    format!("companion-{:x}", Sha256::digest(id.as_bytes()))
}

/// 绑定属于角色专属目录，删除角色时随既有归档清理，不追踪外部模型目录。
pub fn binding_path(root: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    Ok(crate::archive::character_directory(root, id)?.join("voice-model.json"))
}

/// 验证角色身份、正式训练标记和固定文件名；拒绝重解析路径。
pub fn validate_model(directory: &Path, id: &str, engine: Engine) -> Result<(), String> {
    if !directory.is_absolute()
        || directory
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err("companion.model_invalid".into());
    }
    for path in directory.ancestors() {
        check_entry(path)?;
    }
    let files: &[&str] = match engine {
        Engine::IndexTts25 => &["model.json", "adapter.pth", "reference.wav"],
        Engine::GptSovitsV5Turbo | Engine::GptSovitsV2Pro => &[
            "model.json",
            "gpt.ckpt",
            "sovits.pth",
            "reference.wav",
            "reference.txt",
        ],
    };
    for file in files {
        let path = directory.join(file);
        check_entry(&path)?;
        if !path.is_file() {
            return Err("companion.model_invalid".into());
        }
    }
    let bytes = read_bounded(&directory.join("model.json"), 65536)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "companion.model_invalid")?;
    let format_matches = match engine {
        Engine::IndexTts25 => {
            value["format"] == "galfriends-indextts25-lora-v1" && value["adapter"] == "adapter.pth"
        }
        Engine::GptSovitsV5Turbo | Engine::GptSovitsV2Pro => {
            let format = if engine == Engine::GptSovitsV2Pro {
                "galfriends-gpt-sovits-v2pro-v1"
            } else {
                "galfriends-gpt-sovits-v5turbo-v1"
            };
            value["format"] == format
                && value["gpt"] == "gpt.ckpt"
                && value["sovits"] == "sovits.pth"
                && value["transcript"] == "reference.txt"
        }
    };
    if !format_matches
        || value["character_id"] != id
        || value["language"] != "ja"
        || value["verification_only"] != false
        || value["reference"] != "reference.wav"
    {
        return Err("companion.model_invalid".into());
    }
    Ok(())
}

/// 不存在的绑定文件表示尚未配置；损坏文件不会静默重置。
pub fn bindings(path: &Path) -> Result<BTreeMap<String, VoiceModels>, String> {
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// 调用方串行化绑定事务；刷盘并原子替换，失败不发布新绑定。
pub fn bind(
    path: &Path,
    id: &str,
    directory: &Path,
    engine: Engine,
) -> Result<VoiceModels, String> {
    validate_model(directory, id, engine)?;
    let mut values = bindings(path)?;
    let selection = values.entry(id.into()).or_default();
    selection.selected = engine;
    selection
        .models
        .insert(engine, directory.to_string_lossy().into_owned());
    let selection = selection.clone();
    write_json(path, &values)?;
    Ok(selection)
}

/// 在已经串行化的事务内刷盘并原子替换 JSON 文件。
pub(crate) fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    let parent = path.parent().ok_or("companion.model_invalid")?;
    check_entry(parent)?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temp, value).map_err(|e| e.to_string())?;
    temp.flush()
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// 将已验证生成的短 WAV 编码给前端，不暴露任意本地文件读取入口。
pub fn audio_data(path: &Path) -> Result<String, String> {
    use base64::Engine;
    check_entry(path)?;
    let bytes = read_bounded(path, 8 * 1024 * 1024)?;
    if !bytes.starts_with(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("companion.audio_invalid".into());
    }
    Ok(format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::{bind, bindings, validate_model};
    use crate::voice::Engine;
    use serde_json::json;
    use std::fs;
    #[test]
    fn engine_switch_preserves_legacy_index_binding_and_rejects_wrong_format() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("voice-model.json");
        fs::write(&path, r#"{"setsuna":"F:\\old-index-model"}"#).unwrap();
        let legacy = bindings(&path).unwrap();
        assert_eq!(legacy["setsuna"].selected, Engine::IndexTts25);
        let model = root.path().join("gsv");
        fs::create_dir(&model).unwrap();
        for file in ["gpt.ckpt", "sovits.pth", "reference.wav", "reference.txt"] {
            fs::write(model.join(file), b"fixture").unwrap();
        }
        fs::write(model.join("model.json"), json!({"format":"galfriends-gpt-sovits-v5turbo-v1","character_id":"setsuna","language":"ja","verification_only":false,"gpt":"gpt.ckpt","sovits":"sovits.pth","reference":"reference.wav","transcript":"reference.txt"}).to_string()).unwrap();
        let selected = bind(&path, "setsuna", &model, Engine::GptSovitsV5Turbo).unwrap();
        assert_eq!(selected.selected, Engine::GptSovitsV5Turbo);
        assert_eq!(selected.models[&Engine::IndexTts25], r"F:\old-index-model");
        assert_eq!(bindings(&path).unwrap()["setsuna"].models.len(), 2);
        assert!(validate_model(&model, "setsuna", Engine::IndexTts25).is_err());
        assert!(validate_model(&model, "yuuka", Engine::GptSovitsV5Turbo).is_err());
    }
    #[test]
    fn identity_language_and_verification_guard_atomic_binding() {
        let root = tempfile::tempdir().unwrap();
        let model = root.path().join("model");
        fs::create_dir(&model).unwrap();
        fs::write(model.join("adapter.pth"), b"fixture").unwrap();
        fs::write(model.join("reference.wav"), b"fixture").unwrap();
        let mut metadata = json!({"format":"galfriends-indextts25-lora-v1","character_id":"setsuna","language":"ja","verification_only":false,"adapter":"adapter.pth","reference":"reference.wav"});
        fs::write(model.join("model.json"), metadata.to_string()).unwrap();
        let path = root.path().join("companions.json");
        bind(&path, "setsuna", &model, Engine::IndexTts25).unwrap();
        let original = fs::read(&path).unwrap();
        assert!(bind(&path, "yuuka", &model, Engine::IndexTts25).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(bindings(&path).unwrap().len(), 1);
        for (key, value) in [
            ("language", json!("zh")),
            ("verification_only", json!(true)),
            ("adapter", json!("../adapter.pth")),
        ] {
            let old = metadata[key].clone();
            metadata[key] = value;
            fs::write(model.join("model.json"), metadata.to_string()).unwrap();
            assert!(validate_model(&model, "setsuna", Engine::IndexTts25).is_err());
            metadata[key] = old;
        }
        fs::write(&path, "broken").unwrap();
        assert!(bindings(&path).is_err());
    }
}
