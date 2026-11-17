//! 离线表情子进程与派生图提交规则；不依赖桌面运行时。
mod catalog;
pub mod discovery;
use crate::characters::{Artwork, ExpressionVariant, Library};
pub use catalog::{Choice, choices};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

/// 角色专属表情索引；沿用归档目录协议，删除角色时随整个目录清理。
pub fn catalog_path(root: &Path, id: &str) -> Result<std::path::PathBuf, String> {
    let path = crate::archive::character_directory(root, id)?.join("expressions/catalog.json");
    for ancestor in path.ancestors() {
        crate::archive::check_entry(ancestor)?;
    }
    Ok(path)
}

/// 读取已保存的 PNG 或安装目录内的静态样本。
fn image_bytes(url: &str, assets: &Path) -> Result<Vec<u8>, String> {
    if let Some(bytes) = super::png_data(url)? {
        return Ok(bytes);
    }
    let root = assets.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(url).canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(root) {
        return Err("Invalid local artwork path".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}

/// 用户确认后仅合成所选表情，在独立临时目录验证输出尺寸与 alpha。
pub fn compose(
    python: &Path,
    script: &Path,
    assets: &Path,
    art: &Artwork,
    choice: &Choice,
) -> Result<ExpressionVariant, String> {
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = image_bytes(&art.url, assets)?;
    let display = image_bytes(art.enhanced.as_deref().unwrap_or(&art.url), assets)?;
    let original = super::decode(&display, 4096)?.to_rgba8();
    fs::write(temporary.path().join("reference.png"), input).map_err(|e| e.to_string())?;
    fs::write(temporary.path().join("display.png"), display).map_err(|e| e.to_string())?;
    let output = temporary.path().join("output");
    let mut command = Command::new(python);
    command
        .arg("-B")
        .arg(script)
        .arg("--compose")
        .arg("--reference")
        .arg(temporary.path().join("reference.png"))
        .arg("--display")
        .arg(temporary.path().join("display.png"))
        .arg("--catalog")
        .arg(&choice.catalog)
        .arg("--character")
        .arg(&choice.character_id)
        .arg("--pack")
        .arg(&choice.digest)
        .arg("--variant")
        .arg(
            choice
                .preview
                .layers
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
        .arg("--output")
        .arg(&output);
    super::run(&mut command, "Expression composition", temporary.path()).map_err(|error| {
        if error.contains("expression.no_match") {
            "expression.no_match".into()
        } else {
            error
        }
    })?;
    let paths = fs::read_dir(&output)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if paths.len() != 1 {
        return Err("Invalid expression output".into());
    }
    #[derive(Deserialize)]
    struct ResultFile {
        status: String,
        variants: Vec<VariantFile>,
    }
    #[derive(Deserialize)]
    struct VariantFile {
        file: String,
        thumbnail: String,
        layers: Vec<u32>,
    }
    let result: ResultFile = serde_json::from_slice(
        &fs::read(paths[0].join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if result.status != "complete"
        || result.variants.len() != 1
        || result.variants[0].layers != choice.preview.layers
    {
        return Err("Incomplete expression output".into());
    }
    let mut variants = result
        .variants
        .into_iter()
        .map(|entry| {
            for name in [&entry.file, &entry.thumbnail] {
                if Path::new(name).components().count() != 1 || !name.ends_with(".png") {
                    return Err("Invalid expression filename".into());
                }
            }
            let bytes = fs::read(paths[0].join(entry.file)).map_err(|e| e.to_string())?;
            let image = super::decode(&bytes, 4096)?.to_rgba8();
            if image.dimensions() != original.dimensions()
                || image
                    .pixels()
                    .zip(original.pixels())
                    .any(|(a, b)| a[3] != b[3])
            {
                return Err("Expression changed canvas or transparency".into());
            }
            let thumbnail = fs::read(paths[0].join(entry.thumbnail)).map_err(|e| e.to_string())?;
            super::decode(&thumbnail, 512)?;
            Ok(ExpressionVariant {
                layers: entry.layers,
                image: super::data_url("png", &bytes),
                thumbnail: super::data_url("png", &thumbnail),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    variants
        .pop()
        .ok_or_else(|| "Missing composed expression".into())
}

/// 确认时拒绝已删除、已切换或已重新处理的立绘，避免启动无用的合成。
pub fn validate_input(
    library: &Library,
    id: &str,
    index: usize,
    expected: &Artwork,
) -> Result<(), String> {
    let character = library
        .characters
        .iter()
        .find(|c| c.id == id)
        .ok_or("expression.changed")?;
    if character.selected_art != index || character.artworks.get(index) != Some(expected) {
        return Err("expression.changed".into());
    }
    Ok(())
}

/// 在已有短事务中追加并选中派生图；拒绝处理期间已切换或重处理的输入。
pub fn apply(
    library: &mut Library,
    id: &str,
    index: usize,
    expected: &Artwork,
    variant: &ExpressionVariant,
) -> Result<(), String> {
    let character = library
        .characters
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or("expression.changed")?;
    let derived_id = format!(
        "expression-{:x}",
        Sha256::digest(format!("{}{}", expected.id, variant.image).as_bytes())
    );
    if character.artworks.get(index) != Some(expected) {
        return Err("expression.changed".into());
    }
    if character.artwork().is_some_and(|a| a.id == derived_id) {
        return Ok(());
    }
    if character.selected_art != index {
        return Err("expression.changed".into());
    }
    let selected = character.artworks.iter().position(|a| a.id == derived_id);
    character.selected_art = if let Some(selected) = selected {
        selected
    } else {
        let mut artwork = expected.clone();
        artwork.id = derived_id;
        artwork.title = format!(
            "{} · {}",
            artwork.title,
            variant
                .layers
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" + ")
        );
        artwork.url.clone_from(&variant.image);
        artwork.enhanced = None;
        character.artworks.push(artwork);
        character.artworks.len() - 1
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::characters::{Artwork, Character, ExpressionVariant, Library};

    #[test]
    fn apply_preserves_original_is_retryable_and_rejects_changed_selection() {
        let art = Artwork {
            id: "a".into(),
            url: "original".into(),
            ..Default::default()
        };
        let variant = ExpressionVariant {
            layers: vec![1],
            image: "derived".into(),
            thumbnail: "face".into(),
        };
        let mut library = Library {
            characters: vec![Character {
                id: "c".into(),
                artworks: vec![art.clone()],
                ..Default::default()
            }],
            ..Default::default()
        };
        apply(&mut library, "c", 0, &art, &variant).unwrap();
        apply(&mut library, "c", 0, &art, &variant).unwrap();
        assert_eq!(library.characters[0].artworks.len(), 2);
        assert_eq!(library.characters[0].artworks[0], art);
        assert_eq!(library.characters[0].selected_art, 1);
        let before = library.clone();
        let other = ExpressionVariant {
            layers: vec![2],
            image: "other".into(),
            thumbnail: "other face".into(),
        };
        assert!(apply(&mut library, "c", 0, &art, &other).is_err());
        assert_eq!(library, before);
    }
}
