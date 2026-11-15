//! 显式检查本地 waifu2x 执行链；不修改角色库与原始样本。
fn main() -> Result<(), String> {
    use base64::Engine;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(root.join("setsuna-album/cutouts/21037.png")).map_err(|e| e.to_string())?;
    let result = galfriends_core::artwork::enhance(&root.join("tools/waifu2x"), &bytes)?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(
            result
                .strip_prefix("data:image/png;base64,")
                .ok_or("Invalid result URL")?,
        )
        .map_err(|e| e.to_string())?;
    let image = image::load_from_memory(&png).map_err(|e| e.to_string())?;
    if image.width() != 910 || image.height() != 1620 || !image.color().has_alpha() {
        return Err("Unexpected output".into());
    }
    println!(
        "waifu2x runtime pipeline: 910x1620 RGBA verified, {} bytes",
        png.len()
    );
    Ok(())
}
