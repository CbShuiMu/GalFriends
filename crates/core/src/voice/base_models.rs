//! 固定来源的权重安装。下载内容通过大小和 SHA-256 校验后才替换目标文件。
use super::{BaseModel, Progress};
use crate::{archive::check_entry, companion::storage::write_json, sources::client_builder};
use reqwest::{
    StatusCode,
    blocking::Client,
    header::{CONTENT_RANGE, RANGE},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Deserialize, Serialize)]
struct FileInfo {
    path: String,
    sha256: String,
    size: u64,
}

#[derive(Deserialize, Serialize)]
struct Provenance {
    repository: String,
    revision: String,
    files: Vec<FileInfo>,
}

#[derive(Deserialize)]
struct Group {
    directory: String,
    record: String,
    #[serde(flatten)]
    provenance: Provenance,
}

/// 将所选权重写入应用的语音目录。取消时保留已经校验的文件。
pub fn install(
    root: &Path,
    model: BaseModel,
    proxy: &str,
    cancel: &AtomicBool,
    progress: impl Fn(Progress),
) -> Result<(), String> {
    let manifest: BTreeMap<String, Group> =
        serde_json::from_str(include_str!("base-models.json")).map_err(|e| e.to_string())?;
    let keys: &[&str] = match model {
        BaseModel::Index => &["index", "bert", "camp", "bigvgan"],
        BaseModel::V5Turbo => &["v5"],
        BaseModel::V2Pro => &["v2"],
        BaseModel::Irodori => &["base", "codec"],
    };
    let client = client_builder(proxy)?
        .timeout(Duration::from_secs(120))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let groups = keys
        .iter()
        .map(|key| manifest.get(*key).ok_or("voice.download_failed"))
        .collect::<Result<Vec<_>, _>>()?;
    let gpt = matches!(model, BaseModel::V2Pro | BaseModel::V5Turbo);
    let total = groups
        .iter()
        .map(|group| group.provenance.files.len() as u32)
        .sum::<u32>()
        + u32::from(gpt);
    let mut current = 0;
    for group in groups {
        let directory = root.join(&group.directory);
        let provenance = &group.provenance;
        for file in &provenance.files {
            current += 1;
            progress(Progress {
                stage: "download_check".into(),
                current,
                total,
            });
            ensure_file(&directory, file, cancel, || {
                progress(Progress {
                    stage: "downloading".into(),
                    current,
                    total,
                });
                let url = format!(
                    "https://huggingface.co/{}/resolve/{}/{}",
                    provenance.repository, provenance.revision, file.path
                );
                (&client, url)
            })?;
        }
        check_cancel(cancel)?;
        write_json(&directory.join(&group.record), provenance)?;
    }
    if gpt {
        let directory = root.join("gpt-sovits/GPT_SoVITS/pretrained_models/fast_langdetect");
        let source = "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.bin";
        let file = FileInfo {
            path: "lid.176.bin".into(),
            size: 131_266_198,
            sha256: "7e69ec5451bc261cc7844e49e4792a85d7f09c06789ec800fc4a44aec362764e".into(),
        };
        progress(Progress {
            stage: "download_check".into(),
            current: total,
            total,
        });
        ensure_file(&directory, &file, cancel, || {
            progress(Progress {
                stage: "downloading".into(),
                current: total,
                total,
            });
            (&client, source.into())
        })?;
        check_cancel(cancel)?;
        write_json(
            &directory.join("verified-model.json"),
            &serde_json::json!({
                "source": source, "size": file.size, "sha256": file.sha256,
                "md5": "01810bc59c6a3d2b79c79e6336612f65"
            }),
        )?;
    }
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("voice.cancelled".into());
    }
    Ok(())
}

fn existing_matches(path: &Path, info: &FileInfo, cancel: &AtomicBool) -> Result<bool, String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    if file.metadata().map_err(|e| e.to_string())?.len() != info.size {
        return Ok(false);
    }
    let mut digest = Sha256::new();
    copy_checked(&mut file, &mut io::sink(), &mut digest, info.size, cancel)?;
    Ok(format!("{:x}", digest.finalize()) == info.sha256)
}

fn ensure_file<'a>(
    directory: &Path,
    info: &FileInfo,
    cancel: &AtomicBool,
    download_source: impl FnOnce() -> (&'a Client, String),
) -> Result<(), String> {
    check_cancel(cancel)?;
    let path = directory.join(&info.path);
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    if existing_matches(&path, info, cancel)? {
        return Ok(());
    }
    let parent = path.parent().ok_or("voice.download_failed")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let (client, url) = download_source();
    let mut digest = Sha256::new();
    let mut offset = 0;
    while offset < info.size {
        check_cancel(cancel)?;
        let end = (offset + 8 * 1024 * 1024).min(info.size) - 1;
        let mut response = client
            .get(&url)
            .header(RANGE, format!("bytes={offset}-{end}"))
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|e| format!("voice.download_failed: {} ({})", info.path, e.without_url()))?;
        let expected_range = format!("bytes {offset}-{end}/{}", info.size);
        let size = match response.status() {
            StatusCode::OK if offset == 0 => info.size,
            StatusCode::PARTIAL_CONTENT
                if response
                    .headers()
                    .get(CONTENT_RANGE)
                    .is_some_and(|value| value == expected_range.as_str()) =>
            {
                end - offset + 1
            }
            _ => return Err(format!("voice.download_failed: {}", info.path)),
        };
        copy_checked(
            &mut response,
            temporary.as_file_mut(),
            &mut digest,
            size,
            cancel,
        )?;
        offset += size;
    }
    if format!("{:x}", digest.finalize()) != info.sha256 {
        return Err(format!("voice.download_hash: {}", info.path));
    }
    check_cancel(cancel)?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    check_entry(&path)?;
    temporary.persist(&path).map_err(|e| e.to_string())?;
    Ok(())
}

fn copy_checked(
    source: &mut impl Read,
    destination: &mut impl Write,
    digest: &mut Sha256,
    size: u64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut buffer = vec![0; 128 * 1024];
    let mut read = 0;
    loop {
        check_cancel(cancel)?;
        let count = source.read(&mut buffer).map_err(|e| e.to_string())?;
        read += count as u64;
        if read > size || (count == 0 && read != size) {
            return Err("voice.download_size".into());
        }
        if count == 0 {
            return Ok(());
        }
        destination
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        digest.update(&buffer[..count]);
    }
}
