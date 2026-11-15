//! 显式运行本地抠图 → 超分 → 临时角色库保存链；不修改原图、样本或真实角色库。
use base64::{Engine, engine::general_purpose::STANDARD};
use galfriends_core::{
    artwork::{self, CutoutEngine},
    characters::{Artwork, Library},
    store,
};
use image::GenericImageView;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    // 模拟 Tauri 的资源路径：Windows canonicalize 会产生 \\?\ 前缀。
    // 可传入实际桌面工具目录，避免只验证无此前缀的开发目录。
    let waifu2x = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/waifu2x"))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    println!("Checking waifu2x runtime: {}", waifu2x.display());
    let local = std::env::var_os("LOCALAPPDATA").ok_or("Missing LOCALAPPDATA")?;
    let engine = CutoutEngine {
        python: root.join(".venv/Scripts/python.exe"),
        script: root.join("character_foreground.py"),
        model: PathBuf::from(local).join("GalFriends/models/isnet-anime.onnx"),
        keep: root.join("setsuna-album/keep.json"),
    };
    // 第二个参数可验证复杂背景夹具，仍只写临时库，不联网或更新真实角色库。
    let fixture = std::env::args_os().nth(2).map(PathBuf::from);
    let bytes = std::fs::read(
        fixture
            .as_deref()
            .unwrap_or(&root.join("setsuna-album/originals/21037.jpg")),
    )
    .map_err(|e| e.to_string())?;
    let mut character = galfriends_core::characters::sample()?;
    let source = character
        .artworks
        .iter()
        .find(|art| art.id == "21037")
        .ok_or("Missing sample")?
        .clone();
    let mut input = Artwork {
        url: source.original.clone(),
        enhanced: None,
        ..source
    };
    if fixture.is_some() {
        input.id = "portrait".into();
        input.original = "https://lain.bgm.tv/pic/crt/l/local-test-fixture.jpg".into();
    }
    let prepared = artwork::prepare(&engine, &waifu2x, &input, &bytes)?;
    let cutout = artwork::png_data(&prepared.url)?.ok_or("Missing cutout")?;
    let cutout_image = image::load_from_memory(&cutout).map_err(|e| e.to_string())?;
    if fixture.is_none() {
        let expected =
            image::open(root.join("setsuna-album/cutouts/21037.png")).map_err(|e| e.to_string())?;
        if cutout_image.to_rgba8() != expected.to_rgba8() {
            return Err("Cutout differs from the original script and keep parameters".into());
        }
    }
    let enhanced_bytes = STANDARD
        .decode(
            prepared
                .enhanced
                .as_deref()
                .ok_or("Missing enhanced")?
                .split_once(',')
                .ok_or("Invalid enhanced URL")?
                .1,
        )
        .map_err(|e| e.to_string())?;
    let enhanced = image::load_from_memory(&enhanced_bytes).map_err(|e| e.to_string())?;
    if enhanced.dimensions() != (cutout_image.width() * 2, cutout_image.height() * 2)
        || !enhanced.color().has_alpha()
        || !enhanced.pixels().any(|(_, _, pixel)| pixel[3] == 0)
        || !enhanced.pixels().any(|(_, _, pixel)| pixel[3] == 255)
    {
        return Err("Invalid enhanced size or alpha".into());
    }
    if STANDARD
        .decode(
            prepared
                .original_data
                .as_deref()
                .ok_or("Missing original")?
                .split_once(',')
                .ok_or("Invalid original URL")?
                .1,
        )
        .map_err(|e| e.to_string())?
        != bytes
    {
        return Err("Original bytes changed".into());
    }
    character.artworks = vec![prepared];
    character.selected_art = 0;
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = directory.path().join("characters/library.json");
    let library = Library {
        characters: vec![character],
        ..Default::default()
    };
    store::save(&path, &library)?;
    let restored = store::load(&path)?;
    if serde_json::to_value(restored).map_err(|e| e.to_string())?
        != serde_json::to_value(library).map_err(|e| e.to_string())?
    {
        return Err("Artwork archive roundtrip changed data".into());
    }
    println!(
        "PASS: original retained; valid cutout and 2x RGBA upscale; library/archive roundtrip in temporary directory (legacy pixel comparison: {})",
        fixture.is_none()
    );
    Ok(())
}
