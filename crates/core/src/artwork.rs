//! Bangumi 立绘下载、去背景与超分。原图和派生图分开，处理成功后才返回结果。
mod cutout;
pub mod expressions;
use base64::{Engine, engine::general_purpose::STANDARD};
pub use cutout::{
    CutoutEngine, ProcessingStage, prepare, prepare_with_progress, replace_processed,
};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
/// 对一张已验证的 PNG/JPEG/WebP 执行 2× 超分；失败不发布结果。
pub fn enhance(engine: &Path, bytes: &[u8]) -> Result<String, String> {
    let original = decode(bytes, 4096)?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = dir.path().join("input.png");
    let output = dir.path().join("output.png");
    original.save(&input).map_err(|e| e.to_string())?;
    let mut command = Command::new(engine.join("waifu2x-ncnn-vulkan.exe"));
    command
        .current_dir(engine)
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["-n", "1", "-s", "2", "-t", "128", "-j", "1:1:1", "-m"])
        // waifu2x 会把带 \\?\ 前缀的绝对模型路径误当成相对路径，再拼一次工具目录。
        // 工作目录固定为 engine，因此模型始终使用相对路径；保留可执行文件的原路径。
        .arg("models-cunet");
    run(&mut command, "waifu2x", dir.path())?;
    let bytes = std::fs::read(output).map_err(|e| e.to_string())?;
    let enhanced = decode(&bytes, 8192)?;
    if enhanced.width() != original.width() * 2 || enhanced.height() != original.height() * 2 {
        return Err("Invalid waifu2x output size".into());
    }
    if original.color().has_alpha() && !enhanced.color().has_alpha() {
        return Err("waifu2x lost alpha channel".into());
    }
    Ok(data_url("png", &bytes))
}

fn decode(bytes: &[u8], max_side: u32) -> Result<image::DynamicImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

fn data_url(format: &str, bytes: &[u8]) -> String {
    format!("data:image/{format};base64,{}", STANDARD.encode(bytes))
}

/// 读取应用已保存的透明 PNG，供重复超分复用，避免重新处理远程原图。
pub fn png_data(url: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(encoded) = url.strip_prefix("data:image/png;base64,") else {
        return Ok(None);
    };
    if encoded.len() > 128 * 1024 * 1024 {
        return Err("Artwork too large".into());
    }
    let bytes = STANDARD.decode(encoded).map_err(|e| e.to_string())?;
    decode(&bytes, 4096)?;
    Ok(Some(bytes))
}

/// 读取归档的原始文件，供重新抠图使用；旧记录无归档时才下载 Bangumi 原图。
/// 已保存数据损坏时报告错误，不退回旧抠图或静默重新下载。
pub fn original_bytes(proxy: &str, art: &crate::characters::Artwork) -> Result<Vec<u8>, String> {
    if !crate::sources::bangumi_image(&art.original) {
        return Err("Only Bangumi artwork is allowed".into());
    }
    let Some(data) = &art.original_data else {
        return download(proxy, &art.original);
    };
    let (prefix, encoded) = data.split_once(',').ok_or("Invalid archived original")?;
    if !matches!(
        prefix,
        "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
    ) || encoded.len() > 128 * 1024 * 1024
    {
        return Err("Invalid archived original format or size".into());
    }
    let bytes = STANDARD.decode(encoded).map_err(|e| e.to_string())?;
    decode(&bytes, 4096)?;
    Ok(bytes)
}

fn run(command: &mut Command, name: &str, temporary: &Path) -> Result<(), String> {
    // 写到临时文件避免子进程 stderr 管道塞满；只在失败时读取有限诊断。
    let log_path = temporary.join("process.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    command.stdout(Stdio::null()).stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00004000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot launch {name}: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let mut detail = String::new();
                    if let Ok(file) = std::fs::File::open(&log_path) {
                        // 诊断读取失败不能覆盖原始退出状态。
                        let _ = file.take(4096).read_to_string(&mut detail);
                    }
                    return Err(format!("{name} failed: {status}: {}", detail.trim()));
                }
                break;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{name} timed out"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Ok(())
}
/// 远程素材严格限制到 Bangumi，拒绝重定向到其他来源。
pub fn download(proxy: &str, url: &str) -> Result<Vec<u8>, String> {
    if !crate::sources::bangumi_image(url) {
        return Err("Only Bangumi artwork is allowed".into());
    }
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(35))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if crate::sources::bangumi_image(attempt.url().as_str()) && attempt.previous().len() < 5
            {
                attempt.follow()
            } else {
                attempt.error("Invalid image redirect")
            }
        }));
    if !proxy.is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?);
    }
    let response = builder
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .map_err(|e| e.without_url().to_string())?
        .error_for_status()
        .map_err(|e| e.without_url().to_string())?;
    let mut bytes = Vec::new();
    response
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Artwork too large".into());
    }
    Ok(bytes)
}
