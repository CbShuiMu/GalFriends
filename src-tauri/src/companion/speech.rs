//! 将已验证的日语对白及角色本地参考交给一个合成任务。
use galfriends_core::{
    companion::{
        Dialogue, Profile,
        emotions::{Configuration, Emotion, storage as references},
        storage,
    },
    voice::{Engine, Reply, process},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
use tauri::{AppHandle, Manager};

pub(super) fn synthesize(
    app: &AppHandle,
    profile: &Profile,
    reply: &Dialogue,
    cancel: Arc<AtomicBool>,
) -> Result<String, String> {
    let engine = profile.voice.selected;
    storage::validate_model(Path::new(&profile.model_dir), &profile.id, engine)?;
    let _lease = app
        .state::<crate::voice::Tasks>()
        .acquire("speech", 0, cancel.clone(), None)?;
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("companion-audio");
    let config = references::load(
        &super::emotions::config_path(app, &profile.id)?,
        &profile.id,
        &profile.model_dir,
    )?;
    let mut input = json!({
        "character_id": profile.id,
        "model_dir": profile.model_dir,
        "output_dir": cache,
        "language": "ja",
    });
    let segments = reply.speech_segments();
    if engine == Engine::GptSovitsV5Turbo && segments.len() > 1 {
        input["segments"] = segments
            .iter()
            .map(|part| {
                let reference = speech_reference(&config, part.emotion)?;
                Ok::<_, String>(json!({"text": part.text, "reference": reference}))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into();
    } else {
        input["text"] = json!(reply.japanese);
        input["reference"] = speech_reference(&config, reply.emotion)?.unwrap_or(Value::Null);
    }
    let worker =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/core/src/voice/python/worker.py");
    let result: Reply = process::run(
        &crate::voice::runtime(engine),
        &worker,
        "speak",
        &input,
        &cancel,
        |_| {},
    )?;
    let path = PathBuf::from(result.artifact);
    if !path.starts_with(&cache) {
        return Err("companion.audio_invalid".into());
    }
    let audio = storage::audio_data(&path)?;
    // 只清理刚生成并核对过的单个 WAV，不递归删除外部目录。
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    Ok(audio)
}

fn speech_reference(config: &Configuration, emotion: Emotion) -> Result<Option<Value>, String> {
    config
        .reference_for(emotion)
        .map(|reference| {
            let path = references::verified_path(Path::new(&config.source_dir), reference)?;
            Ok::<_, String>(json!({
                "path": path,
                "text": reference.candidate.text,
                "sha256": reference.sha256,
                "source_dir": config.source_dir,
            }))
        })
        .transpose()
}
