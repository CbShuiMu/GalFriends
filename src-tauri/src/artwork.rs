//! 桌面图片运行环境定位；下载和图片处理交给 core，不持有角色库锁。
use galfriends_core::{
    artwork::{self, CutoutEngine, ProcessingStage},
    characters::{Artwork, Character, ImportProgress, Library},
    sources::missing_bangumi_portrait,
};
use std::path::{Path, PathBuf};
use tauri::Manager;

pub(crate) fn cutout_engine(resources: &Path) -> Result<CutoutEngine, String> {
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let roots = [resources, dev.as_path()];
    let locate = |relative: &str| {
        roots
            .iter()
            .map(|root| root.join(relative))
            .find(|path| path.is_file())
            .ok_or_else(|| format!("Missing {relative}; run scripts/setup-cutout.ps1"))
    };
    let model = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is unavailable")?
        .join("GalFriends/models/isnet-anime.onnx");
    if !model.is_file() {
        return Err("ISNet Anime model missing; run scripts/setup-cutout.ps1".into());
    }
    Ok(CutoutEngine {
        python: locate(".venv/Scripts/python.exe")?,
        script: locate("character_foreground.py")?,
        model,
        keep: locate("setsuna-album/keep.json")?,
    })
}

pub(crate) fn prepare_character(
    mut character: Character,
    resources: &Path,
    proxy: &str,
    progress: impl Fn(ImportProgress),
) -> Result<Character, String> {
    character.artworks = prepare_artworks(&character.artworks, None, resources, proxy, progress)?;
    Ok(character)
}

fn prepare_artworks(
    artworks: &[Artwork],
    extra: Option<&Artwork>,
    resources: &Path,
    proxy: &str,
    progress: impl Fn(ImportProgress),
) -> Result<Vec<Artwork>, String> {
    let total = artworks.len() + usize::from(extra.is_some());
    if total == 0 {
        return Ok(Vec::new());
    }
    let cutout = cutout_engine(resources)?;
    let waifu2x =
        crate::preferences::waifu2x_directory(resources).ok_or("waifu2x not installed")?;
    // 处理的只是待导入记录；全部成功才交回命令事务，失败不会保存半个角色。
    artworks
        .iter()
        .chain(extra)
        .enumerate()
        .map(|(index, art)| {
            let current = index + 1;
            progress(ImportProgress::Downloading { current, total });
            let bytes = artwork::original_bytes(proxy, art)
                .map_err(|error| format!("Artwork {}: {error}", art.id))?;
            artwork::prepare_with_progress(&cutout, &waifu2x, art, &bytes, |stage| {
                progress(match stage {
                    ProcessingStage::CuttingOut => ImportProgress::CuttingOut { current, total },
                    ProcessingStage::Upscaling => ImportProgress::Upscaling { current, total },
                });
            })
            .map_err(|error| format!("Artwork {}: {error}", art.id))
        })
        .collect()
}

#[tauri::command]
pub(crate) async fn reprocess_artworks(
    app: tauri::AppHandle,
    id: String,
) -> Result<Library, String> {
    let library = crate::commands::read(&app.state::<crate::commands::AppState>())?;
    let character = library
        .characters
        .into_iter()
        .find(|c| c.id == id)
        .ok_or("Character not found")?;
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    let language =
        galfriends_i18n::Language::from_code(&library.settings.language).unwrap_or_default();
    crate::diagnostics::spawn_background("reprocess_artworks", move || {
        let portrait = missing_bangumi_portrait(&character.snapshots, &character.artworks);
        let mut prepared = prepare_artworks(
            &character.artworks,
            portrait.as_ref(),
            &resources,
            &library.settings.proxy,
            |_| {},
        )
        .map_err(|detail| {
            galfriends_i18n::format_text(
                language,
                "error.artwork_reprocess",
                &[("detail", &detail)],
            )
        })?;
        let extra = if portrait.is_some() {
            prepared.pop()
        } else {
            None
        };
        crate::commands::update(&app.state::<crate::commands::AppState>(), |library| {
            let current = library
                .characters
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or("Character removed during artwork processing")?;
            if portrait.is_some()
                && missing_bangumi_portrait(&current.snapshots, &current.artworks) != portrait
            {
                return Err("Bangumi portrait changed during processing; please retry".into());
            }
            artwork::replace_processed(&mut current.artworks, &character.artworks, prepared)?;
            // 补到末尾，保持处理过程中选中的立绘索引不变。
            current.artworks.extend(extra);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
