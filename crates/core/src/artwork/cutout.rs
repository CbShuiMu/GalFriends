//! 调用 Python 主人物抠图入口；复用原 ISNet Anime 边缘、alpha 和样本修正参数。
use super::{data_url, decode, enhance, run};
use crate::characters::Artwork;
use image::{GenericImageView, ImageFormat};
use std::{path::Path, path::PathBuf, process::Command};

/// 本地抠图运行环境。模型必须已准备好，导入过程中不隐式安装或下载依赖。
pub struct CutoutEngine {
    pub python: PathBuf,
    pub script: PathBuf,
    pub model: PathBuf,
    /// 既有样本的前景修正，按 Bangumi 相册图片 ID 匹配。
    pub keep: PathBuf,
}

impl CutoutEngine {
    fn cutout(&self, id: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
        // 相册 ID 同时用于匹配 keep.json，绝不允许成为任意文件路径。
        if id != "portrait" && (id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit())) {
            return Err("Invalid artwork ID".into());
        }
        let original = decode(bytes, 4096)?;
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let input = directory.path().join("originals");
        let output = directory.path().join("cutouts");
        std::fs::create_dir(&input).map_err(|e| e.to_string())?;
        // 保持下载字节和 EXIF，让原脚本执行方向修正；PNG 编码仅用于透明输入直通。
        if original.color().has_alpha() && original.pixels().any(|(_, _, p)| p[3] != 255) {
            if !original.pixels().any(|(_, _, p)| p[3] != 0) {
                return Err("Artwork has no visible foreground".into());
            }
            let mut png = std::io::Cursor::new(Vec::new());
            original
                .write_to(&mut png, ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            return Ok(png.into_inner());
        }
        let extension = format(bytes)?;
        std::fs::write(input.join(format!("{id}.{extension}")), bytes)
            .map_err(|e| e.to_string())?;
        let mut command = Command::new(&self.python);
        command
            .arg("-B")
            .arg(&self.script)
            .arg("cutout")
            .arg(&input)
            .arg(&output)
            .arg("--model")
            .arg(&self.model)
            .arg("--keep")
            .arg(&self.keep)
            .args(["--threads", "2"]);
        run(&mut command, "Character foreground", directory.path())?;
        let result = std::fs::read(output.join(format!("{id}.png"))).map_err(|e| e.to_string())?;
        let cutout = decode(&result, 4096)?;
        // EXIF 转向可以交换宽高；算法不裁剪或拉伸原始画布。
        if ![original.dimensions(), (original.height(), original.width())]
            .contains(&cutout.dimensions())
            || !cutout.color().has_alpha()
            || !cutout.pixels().any(|(_, _, p)| p[3] == 0)
            || !cutout.pixels().any(|(_, _, p)| p[3] != 0)
        {
            return Err("Invalid cutout dimensions or foreground alpha".into());
        }
        Ok(result)
    }
}

fn format(bytes: &[u8]) -> Result<&'static str, String> {
    match image::guess_format(bytes).map_err(|e| e.to_string())? {
        ImageFormat::Png => Ok("png"),
        ImageFormat::Jpeg => Ok("jpeg"),
        ImageFormat::WebP => Ok("webp"),
        _ => Err("Unsupported artwork format".into()),
    }
}

/// 顺序执行去背景和 2× 超分，返回含原始文件与两个独立派生图的立绘。
/// 任一步失败均不修改传入记录；调用方只能在全部立绘成功后提交角色。
pub fn prepare(
    engine: &CutoutEngine,
    waifu2x: &Path,
    artwork: &Artwork,
    bytes: &[u8],
) -> Result<Artwork, String> {
    prepare_with_progress(engine, waifu2x, artwork, bytes, |_| {})
}

/// 单张立绘当前的处理步骤，不包含下载与角色保存。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingStage {
    CuttingOut,
    Upscaling,
}

/// 与 `prepare` 使用同一处理链，在开始去背景和超分前同步通知调用方。
/// 通知仅用于进度展示，失败时不会报告尚未开始的后续步骤。
pub fn prepare_with_progress(
    engine: &CutoutEngine,
    waifu2x: &Path,
    artwork: &Artwork,
    bytes: &[u8],
    progress: impl Fn(ProcessingStage),
) -> Result<Artwork, String> {
    prepare_with(
        artwork,
        bytes,
        |bytes| {
            progress(ProcessingStage::CuttingOut);
            engine.cutout(&artwork.id, bytes)
        },
        |bytes| {
            progress(ProcessingStage::Upscaling);
            enhance(waifu2x, bytes)
        },
    )
}

/// 核对处理期间的图片版本后，一次替换派生图与离线原图，保留当前标题等元数据。
/// 全部校验在赋值前完成；调用方仍须在自己的短保存事务内调用。
pub fn replace_processed(
    current: &mut [Artwork],
    original: &[Artwork],
    prepared: Vec<Artwork>,
) -> Result<(), String> {
    if current.len() != original.len()
        || prepared.len() != original.len()
        || current
            .iter()
            .zip(original)
            .zip(&prepared)
            .any(|((a, b), result)| {
                a.id != b.id
                    || a.original != b.original
                    || a.original_data != b.original_data
                    || a.url != b.url
                    || a.enhanced != b.enhanced
                    || result.id != b.id
                    || result.original != b.original
            })
    {
        return Err("Artwork changed during processing; please retry".into());
    }
    for (target, result) in current.iter_mut().zip(prepared) {
        target.original_data = result.original_data;
        target.url = result.url;
        target.enhanced = result.enhanced;
    }
    Ok(())
}

fn prepare_with(
    artwork: &Artwork,
    bytes: &[u8],
    cutout: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
    upscale: impl FnOnce(&[u8]) -> Result<String, String>,
) -> Result<Artwork, String> {
    if !crate::sources::bangumi_image(&artwork.original) {
        return Err("Only Bangumi artwork is allowed".into());
    }
    let extension = format(bytes)?;
    let transparent = cutout(bytes)?;
    let enhanced = upscale(&transparent)?;
    let mut result = artwork.clone();
    result.original_data = Some(data_url(extension, bytes));
    result.url = data_url("png", &transparent);
    result.enhanced = Some(enhanced);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        CutoutEngine, ProcessingStage, prepare_with, prepare_with_progress, replace_processed,
    };
    use crate::characters::Artwork;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::{io::Cursor, path::PathBuf};

    fn original() -> (Artwork, Vec<u8>) {
        let mut png = Cursor::new(Vec::new());
        RgbaImage::from_pixel(2, 3, Rgba([255, 255, 255, 255]))
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        (
            Artwork {
                id: "123".into(),
                original: "https://lain.bgm.tv/pic/crt/l/example.png".into(),
                url: "remote".into(),
                ..Default::default()
            },
            png.into_inner(),
        )
    }

    #[test]
    fn upscales_cutout_then_retains_original_bytes_and_source() {
        let (art, bytes) = original();
        let result = prepare_with(
            &art,
            &bytes,
            |input| {
                assert_eq!(input, bytes);
                Ok(vec![1, 2, 3])
            },
            |input| {
                assert_eq!(input, [1, 2, 3]);
                Ok("enhanced".into())
            },
        )
        .unwrap();
        assert_eq!(result.original, art.original);
        assert_eq!(result.url, "data:image/png;base64,AQID");
        assert_eq!(result.enhanced.as_deref(), Some("enhanced"));
        assert_eq!(
            STANDARD
                .decode(result.original_data.unwrap().split_once(',').unwrap().1)
                .unwrap(),
            bytes
        );
        assert_eq!(art.url, "remote");
    }

    #[test]
    fn cutout_failure_stops_before_upscale_and_upscale_failure_publishes_nothing() {
        let (art, bytes) = original();
        let error = prepare_with(
            &art,
            &bytes,
            |_| Err("cutout failed".into()),
            |_| panic!("must not upscale"),
        )
        .unwrap_err();
        assert_eq!(error, "cutout failed");
        assert!(
            prepare_with(
                &art,
                &bytes,
                |b| Ok(b.to_vec()),
                |_| Err("upscale failed".into())
            )
            .is_err()
        );
        assert_eq!(art.url, "remote");
        assert!(art.original_data.is_none());
        assert!(art.enhanced.is_none());
    }

    #[test]
    fn existing_alpha_is_preserved_without_model_and_unsafe_ids_are_rejected() {
        let engine = CutoutEngine {
            python: PathBuf::new(),
            script: PathBuf::new(),
            model: PathBuf::new(),
            keep: PathBuf::new(),
        };
        let mut image = RgbaImage::from_pixel(2, 3, Rgba([50, 60, 70, 255]));
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, Rgba([50, 60, 70, 128]));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, ImageFormat::Png).unwrap();
        let result = engine.cutout("123", png.get_ref()).unwrap();
        assert_eq!(image::load_from_memory(&result).unwrap().to_rgba8(), image);
        assert!(engine.cutout("../123", png.get_ref()).is_err());
    }

    #[test]
    fn progress_stops_at_the_failed_step_without_publishing_artwork() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing");
        let engine = CutoutEngine {
            python: missing.clone(),
            script: missing.clone(),
            model: missing.clone(),
            keep: missing.clone(),
        };
        let (art, opaque) = original();
        let stages = std::cell::RefCell::new(Vec::new());
        let record = |stage| stages.borrow_mut().push(stage);
        assert!(prepare_with_progress(&engine, &missing, &art, &opaque, record).is_err());
        assert_eq!(*stages.borrow(), [ProcessingStage::CuttingOut]);

        stages.borrow_mut().clear();
        let mut transparent = RgbaImage::from_pixel(2, 3, Rgba([50, 60, 70, 255]));
        transparent.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        let mut png = Cursor::new(Vec::new());
        transparent.write_to(&mut png, ImageFormat::Png).unwrap();
        assert!(prepare_with_progress(&engine, &missing, &art, png.get_ref(), record).is_err());
        assert_eq!(
            *stages.borrow(),
            [ProcessingStage::CuttingOut, ProcessingStage::Upscaling]
        );
        assert_eq!(art.url, "remote");
        assert!(art.enhanced.is_none());
    }

    #[test]
    fn rejects_non_bangumi_source_and_invalid_input_before_processing() {
        let (mut art, bytes) = original();
        art.original = "https://example.com/art.png".into();
        assert!(
            prepare_with(
                &art,
                &bytes,
                |_| panic!("invalid source"),
                |_| panic!("invalid source")
            )
            .is_err()
        );
        let (art, _) = original();
        assert!(
            prepare_with(
                &art,
                b"not an image",
                |_| panic!("invalid image"),
                |_| panic!("invalid image")
            )
            .is_err()
        );
    }

    #[test]
    fn reprocessing_reads_archived_original_offline_and_rejects_corruption() {
        let (mut art, bytes) = original();
        art.original_data = Some(format!("data:image/png;base64,{}", STANDARD.encode(&bytes)));
        art.url = "old cutout must not be used as input".into();
        assert_eq!(
            crate::artwork::original_bytes("invalid proxy", &art).unwrap(),
            bytes
        );
        art.original_data = Some("data:image/png;base64,broken".into());
        assert!(crate::artwork::original_bytes("invalid proxy", &art).is_err());
        art.original_data = Some("file:///outside.png".into());
        assert!(crate::artwork::original_bytes("", &art).is_err());
    }

    #[test]
    fn replacing_processed_images_checks_whole_batch_before_mutation() {
        let (art, _) = original();
        let before = vec![
            art.clone(),
            Artwork {
                id: "456".into(),
                ..art
            },
        ];
        let mut prepared = before.clone();
        for art in &mut prepared {
            art.url = "new cutout".into();
            art.enhanced = Some("new upscale".into());
        }
        let mut current = before.clone();
        current[1].url = "newer concurrent result".into();
        assert!(replace_processed(&mut current, &before, prepared.clone()).is_err());
        assert_eq!(current[0].url, "remote");
        assert_eq!(current[1].url, "newer concurrent result");
        current[1].url = before[1].url.clone();
        current[0].title = "edited title".into();
        replace_processed(&mut current, &before, prepared).unwrap();
        assert_eq!(current[0].title, "edited title");
        assert!(
            current.iter().all(
                |art| art.url == "new cutout" && art.enhanced.as_deref() == Some("new upscale")
            )
        );
        assert!(replace_processed(&mut current, &before, Vec::new()).is_err());
    }
}
