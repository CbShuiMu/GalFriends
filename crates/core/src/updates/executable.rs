//! 限定来源的 EXE 下载、校验与可回滚的同卷替换。
use super::{Progress, check_cancel};
use galfriends_i18n::Message;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::AtomicBool,
    time::Duration,
};

const MAX_EXE: u64 = 1024 * 1024 * 1024;

/// GitHub 发行资产中的 URL、字节数和 SHA-256。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    url: String,
    size: u64,
    sha256: String,
}

impl Asset {
    pub(super) fn from_release(url: String, size: u64, digest: &str) -> Option<Self> {
        let sha256 = digest.strip_prefix("sha256:")?;
        if !(64..=MAX_EXE).contains(&size)
            || sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        Some(Self {
            url,
            size,
            sha256: sha256.to_ascii_lowercase(),
        })
    }
}

fn error(detail: impl ToString) -> Message {
    Message::detail("update.install_failed", detail)
}

pub(super) fn download_client(proxy: &str, key: &str) -> Result<Client, Message> {
    let error = |detail| Message::detail(key, detail);
    crate::sources::client_builder(proxy)
        .map_err(error)?
        .https_only(true)
        .timeout(Duration::from_secs(1200))
        .redirect(reqwest::redirect::Policy::custom(|redirect| {
            let url = redirect.url();
            if redirect.previous().len() < 5
                && url.scheme() == "https"
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                            | "codeload.github.com"
                    )
                )
            {
                redirect.follow()
            } else {
                redirect.error("Unexpected GitHub download redirect")
            }
        }))
        .build()
        .map_err(|e| error(e.without_url().to_string()))
}

pub(super) fn fetch(client: &Client, url: &str, key: &str) -> Result<Response, Message> {
    client
        .get(url)
        .send()
        .and_then(Response::error_for_status)
        .map_err(|e| Message::detail(key, e.without_url()))
}

/// 先保存到新文件，再验证字节数、SHA-256 和 Windows x64 PE 头。
pub fn download(
    asset: &Asset,
    proxy: &str,
    output: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<(), Message> {
    progress(Progress::Downloading { percent: Some(0) });
    let client = download_client(proxy, "update.install_failed")?;
    let mut response = fetch(&client, &asset.url, "update.install_failed")?;
    if response
        .content_length()
        .is_some_and(|bytes| bytes != asset.size)
    {
        return Err(Message::new("update.invalid_asset"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(error)?;
    let mut received = 0;
    let mut last = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = response.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > asset.size {
            return Err(Message::new("update.invalid_asset"));
        }
        file.write_all(&buffer[..count]).map_err(error)?;
        let percent = (received * 100 / asset.size) as u8;
        if percent != last {
            last = percent;
            progress(Progress::Downloading {
                percent: Some(percent),
            });
        }
    }
    file.sync_all().map_err(error)?;
    drop(file);
    progress(Progress::Verifying);
    verify(output, asset)?;
    check_cancel(cancel)
}

/// 独立更新进程在替换前重新核验已暂存的文件。
pub fn verify(path: &Path, asset: &Asset) -> Result<(), Message> {
    let invalid = || Message::new("update.invalid_asset");
    crate::archive::check_entry(path).map_err(error)?;
    let mut file = File::open(path).map_err(error)?;
    let meta = file.metadata().map_err(error)?;
    if !meta.is_file() || meta.len() != asset.size || !(64..=MAX_EXE).contains(&asset.size) {
        return Err(invalid());
    }
    let mut dos = [0; 64];
    file.read_exact(&mut dos).map_err(error)?;
    let offset = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    if !dos.starts_with(b"MZ") || offset < 64 || offset + 6 > asset.size {
        return Err(invalid());
    }
    file.seek(SeekFrom::Start(offset)).map_err(error)?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe).map_err(error)?;
    if pe != *b"PE\0\0\x64\x86" {
        return Err(invalid());
    }
    file.rewind().map_err(error)?;
    let mut hash = Sha256::new();
    let count = std::io::copy(&mut file.take(asset.size + 1), &mut hash).map_err(error)?;
    if count != asset.size || format!("{:x}", hash.finalize()) != asset.sha256 {
        return Err(invalid());
    }
    Ok(())
}

/// 备份原 EXE 后使用同卷重命名替换。要求调用方验证了目录及进程已退出。
pub fn replace(executable: &Path, staged: &Path, backup: &Path) -> Result<(), Message> {
    for path in [executable, staged, backup] {
        crate::archive::check_entry(path).map_err(error)?;
    }
    let mut old = File::open(executable).map_err(error)?;
    let mut saved = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(backup)
        .map_err(error)?;
    std::io::copy(&mut old, &mut saved).map_err(error)?;
    saved.sync_all().map_err(error)?;
    drop((old, saved));
    fs::rename(staged, executable).map_err(error)
}

/// 新程序无法启动时，原子恢复已刷盘的原 EXE。
pub fn restore(executable: &Path, backup: &Path) -> Result<(), Message> {
    for path in [executable, backup] {
        crate::archive::check_entry(path).map_err(error)?;
    }
    fs::rename(backup, executable).map_err(error)
}

/// 验证更新暂存区和替换目标没有链接或重解析点。
pub fn check_entry(path: &Path) -> Result<(), Message> {
    crate::archive::check_entry(path).map_err(error)
}

/// 比较交接程序与待退出的原 EXE，拒绝过期或指向其他程序的交接。
pub fn same_binary(left: &Path, right: &Path) -> Result<bool, Message> {
    check_entry(left)?;
    check_entry(right)?;
    let mut left = File::open(left).map_err(error)?;
    let mut right = File::open(right).map_err(error)?;
    let size = left.metadata().map_err(error)?.len();
    if size > MAX_EXE || size != right.metadata().map_err(error)?.len() {
        return Ok(false);
    }
    let mut a = [0; 64 * 1024];
    let mut b = [0; 64 * 1024];
    let mut remaining = size;
    while remaining != 0 {
        let count = remaining.min(a.len() as u64) as usize;
        left.read_exact(&mut a[..count]).map_err(error)?;
        right.read_exact(&mut b[..count]).map_err(error)?;
        if a[..count] != b[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{Asset, Sha256, replace, restore, verify};
    use sha2::Digest;
    use std::fs;

    #[test]
    fn validates_digest_then_replaces_and_restores_only_the_executable() {
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("renamed.exe");
        let stage = root.path().join("payload.exe");
        let backup = root.path().join("previous.exe");
        let data = root.path().join("library.json");
        fs::write(&exe, b"old exe").unwrap();
        fs::write(&data, b"user data").unwrap();
        let mut next = vec![0; 128];
        next[..2].copy_from_slice(b"MZ");
        next[60] = 64;
        next[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        let asset = Asset::from_release(
            String::new(),
            128,
            &format!("sha256:{:x}", Sha256::digest(&next)),
        )
        .unwrap();
        fs::write(&stage, &next).unwrap();
        verify(&stage, &asset).unwrap();
        next[127] = 1;
        fs::write(&stage, &next).unwrap();
        assert!(verify(&stage, &asset).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old exe");
        next[127] = 0;
        fs::write(&stage, &next).unwrap();
        replace(&exe, &stage, &backup).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), next);
        restore(&exe, &backup).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"old exe");
        assert_eq!(fs::read(&data).unwrap(), b"user data");
    }
}
