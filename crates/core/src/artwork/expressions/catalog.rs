//! 只读原始表情索引和归档缩略图，不启动 Python、不读取目标立绘、不生成文件。
use crate::{archive::check_entry, characters::ExpressionChoice};
use serde::Deserialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// 后端保存的素材身份。应用时使用此记录，不接受前端提供的路径或图层。
#[derive(Clone)]
pub struct Choice {
    pub preview: ExpressionChoice,
    pub(super) catalog: PathBuf,
    pub(super) character_id: String,
    pub(super) digest: String,
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    let mut data = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > limit {
        return Err("Expression catalog file too large".into());
    }
    Ok(data)
}

#[derive(Deserialize)]
struct Catalog {
    format_version: u32,
    character_id: String,
    packs: Vec<Pack>,
    #[serde(default)]
    unique: Option<Vec<Unique>>,
}
#[derive(Deserialize)]
struct Unique {
    id: String,
    digest: String,
    index: usize,
}
#[derive(Deserialize)]
struct Pack {
    digest: String,
}
#[derive(Deserialize)]
struct Previews {
    format_version: u32,
    source: String,
    variants: Vec<Preview>,
}
#[derive(Deserialize)]
struct Preview {
    layers: Vec<u32>,
    thumbnail: String,
}

/// 读取已生成的游戏原始表情，缺少素材返回空列表，旧版缺少缩略图须重新归档。
pub fn choices(path: &Path, character_id: &str) -> Result<Vec<Choice>, String> {
    for ancestor in path.ancestors() {
        check_entry(ancestor)?;
    }
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(Vec::new());
    }
    let catalog: Catalog =
        serde_json::from_slice(&read(path, 1024 * 1024)?).map_err(|e| e.to_string())?;
    if catalog.format_version != 1
        || catalog.character_id != character_id
        || catalog.packs.len() > 128
    {
        return Err("Invalid character expression catalog".into());
    }
    let root = path.parent().ok_or("Missing expression directory")?;
    if let Some(unique) = &catalog.unique
        && (unique.len() > 512
            || unique.iter().any(|item| {
                item.id.len() > 128 || !catalog.packs.iter().any(|pack| pack.digest == item.digest)
            }))
    {
        return Err("Invalid unique expression index".into());
    }
    let mut result = Vec::new();
    for pack in catalog.packs {
        if pack.digest.len() != 64
            || !pack
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid expression digest".into());
        }
        let directory = root.join("packs").join(&pack.digest);
        let index = directory.join("choices.json");
        if !index.try_exists().map_err(|e| e.to_string())? {
            return Err("expression.materials_update".into());
        }
        let previews: Previews =
            serde_json::from_slice(&read(&index, 1024 * 1024)?).map_err(|e| e.to_string())?;
        if previews.format_version != 1
            || previews.source.len() > 256
            || previews.variants.is_empty()
            || previews.variants.len() > 64
            || result.len() + previews.variants.len() > 512
        {
            return Err("Invalid expression previews".into());
        }
        for (index, preview) in previews.variants.into_iter().enumerate() {
            let identity = if let Some(unique) = &catalog.unique {
                let Some(item) = unique
                    .iter()
                    .find(|item| item.digest == pack.digest && item.index == index)
                else {
                    continue;
                };
                item.id.clone()
            } else {
                format!("{}:{index}", pack.digest)
            };
            if Path::new(&preview.thumbnail).components().count() != 1
                || !preview.thumbnail.ends_with(".png")
                || preview.layers.is_empty()
                || preview.layers.len() > 64
            {
                return Err("Invalid expression preview entry".into());
            }
            let bytes = read(
                &directory.join("choices").join(&preview.thumbnail),
                512 * 1024,
            )?;
            super::super::decode(&bytes, 512)?;
            result.push(Choice {
                preview: ExpressionChoice {
                    id: identity,
                    source: previews.source.clone(),
                    layers: preview.layers,
                    thumbnail: super::super::data_url("png", &bytes),
                },
                catalog: path.to_owned(),
                character_id: character_id.into(),
                digest: pack.digest.clone(),
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::choices;
    use std::{fs, io::Cursor};
    #[test]
    fn browsing_reads_saved_previews_without_target_artwork_or_runtime_and_rejects_escape() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog.json");
        assert!(choices(&path, "c").unwrap().is_empty());
        let digest = "a".repeat(64);
        fs::write(&path, serde_json::to_vec(&serde_json::json!({"format_version":1,"character_id":"c","packs":[{"digest":digest}]})).unwrap()).unwrap();
        let pack = temporary.path().join("packs").join(&digest);
        fs::create_dir_all(pack.join("choices")).unwrap();
        assert_eq!(
            choices(&path, "c").err().as_deref(),
            Some("expression.materials_update")
        );
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        fs::write(pack.join("choices/000.png"), bytes.into_inner()).unwrap();
        let index = serde_json::json!({"format_version":1,"source":"original.pna","variants":[{"layers":[1],"thumbnail":"000.png"}]});
        fs::write(
            pack.join("choices.json"),
            serde_json::to_vec(&index).unwrap(),
        )
        .unwrap();
        let result = choices(&path, "c").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].preview.source, "original.pna");
        assert_eq!(result[0].preview.layers, [1]);
        assert!(choices(&path, "other").is_err());
        let mut invalid = index;
        invalid["variants"][0]["thumbnail"] = "../000.png".into();
        fs::write(
            pack.join("choices.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(choices(&path, "c").is_err());
    }
}
