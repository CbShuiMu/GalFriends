//! 登记训练输出根目录，浏览时只读取模型元数据并验证同一角色与引擎。
use super::{Engine, TrainedModel};
use crate::{
    archive::check_entry,
    companion::{VoiceModels, storage},
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

/// 调用方串行化登记；旧安装没有清单时从空清单开始。
pub fn register(file: &Path, output: &Path) -> Result<(), String> {
    checked_absolute(output)?;
    if !output.is_dir() {
        return Err("voice.output_missing".into());
    }
    let mut roots = storage::read_output_roots(file)?;
    if !roots.iter().any(|root| root == output) {
        roots.push(output.to_owned());
        storage::write_json(file, &roots)?;
    }
    Ok(())
}

fn checked_absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err("voice.path_required".into());
    }
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    Ok(())
}

/// 只看登记目录及其直接子目录。旧绑定所在的训练输出目录可直接发现其相邻模型。
pub fn models(
    file: &Path,
    id: &str,
    engine: Engine,
    bindings: &VoiceModels,
) -> Result<Vec<TrainedModel>, String> {
    let mut roots: BTreeSet<_> = storage::read_output_roots(file)?.into_iter().collect();
    let mut candidates = BTreeSet::<PathBuf>::new();
    for path in bindings.models.values().map(Path::new) {
        if let Some(parent) = path.parent()
            && checked_absolute(path).is_ok()
        {
            roots.insert(parent.to_owned());
            candidates.insert(path.to_owned());
        }
    }
    for root in roots {
        // 可移动磁盘离线或目录被移除时，其余模型仍可选择。
        if checked_absolute(&root).is_err() {
            continue;
        }
        if root.join("model.json").is_file() {
            candidates.insert(root.clone());
        }
        if let Ok(children) = fs::read_dir(&root) {
            candidates.extend(
                children
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .map(|entry| entry.path()),
            );
        }
    }
    Ok(candidates
        .into_iter()
        .filter(|path| storage::validate_model(path, id, engine).is_ok())
        .filter_map(|path| {
            Some(TrainedModel {
                label: path.file_name()?.to_str()?.into(),
                directory: path.to_str()?.into(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{models, register};
    use crate::{companion::VoiceModels, voice::Engine};
    use std::{fs, path::Path};

    fn fixture(root: &Path, name: &str, character: &str, verification_only: bool) {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        for name in ["adapter.pth", "reference.wav"] {
            fs::write(directory.join(name), []).unwrap();
        }
        fs::write(
            directory.join("model.json"),
            serde_json::json!({
                "format": "galfriends-indextts25-lora-v1",
                "character_id": character, "language": "ja",
                "verification_only": verification_only,
                "adapter": "adapter.pth", "reference": "reference.wav"
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn registered_and_previously_bound_outputs_only_offer_matching_completed_models() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let output = root.join("old-training");
        fixture(&output, "complete", "character", false);
        fixture(&output, "preview", "character", true);
        fixture(&output, "other", "different-character", false);
        let file = root.join("output-directories.json");
        let mut bindings = VoiceModels::default();
        let engine = Engine::IndexTts25;
        let labels = |bindings: &VoiceModels| {
            models(&file, "character", engine, bindings)
                .unwrap()
                .into_iter()
                .map(|model| model.label)
                .collect::<Vec<_>>()
        };
        assert!(labels(&bindings).is_empty());
        bindings
            .models
            .insert(engine, output.join("complete").to_str().unwrap().into());
        assert_eq!(labels(&bindings), ["complete"]);
        register(&file, &output).unwrap();
        assert_eq!(labels(&VoiceModels::default()), ["complete"]);
        assert!(
            models(&file, "character", Engine::GptSovitsV2Pro, &bindings)
                .unwrap()
                .is_empty()
        );
    }
}
