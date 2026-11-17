use super::{Release, SkillStatus, VERSION, executable::Asset};
use galfriends_i18n::Message;
use reqwest::{Url, blocking::Client};
use semver::Version;
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path, time::Duration};

const SKILL_REFS: &str = "https://github.com/CbShuiMu/Galgear-GalgameReverse-Skill.git/info/refs?service=git-upload-pack";
const REF_LIMIT: u64 = 1024 * 1024;

fn repository_path(page: &str) -> Result<String, Message> {
    let invalid = || Message::new("update.invalid_page");
    let url = Url::parse(page.trim()).map_err(|_| invalid())?;
    let path = url.path().trim_end_matches('/');
    let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(&parts[..], [_, _, "releases"])
        || parts[..2].iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err(invalid());
    }
    Ok(format!("{}/{}", parts[0], parts[1]))
}

/// 校验发行页输入并归一成仅含 GitHub 仓库路径的 HTTPS URL。
pub fn release_page(page: &str) -> Result<String, Message> {
    Ok(format!(
        "https://github.com/{}/releases",
        repository_path(page)?
    ))
}

fn parse_version(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

#[derive(Deserialize)]
struct PublishedRelease {
    tag_name: String,
    body: Option<String>,
    draft: bool,
    #[serde(default)]
    assets: Vec<PublishedAsset>,
}

#[derive(Deserialize)]
struct PublishedAsset {
    name: String,
    size: u64,
    digest: Option<String>,
}

/// 从同一发行记录取得的版本信息与可校验资产。资产留在桌面端，不接受网页传入。
#[derive(Debug)]
pub struct CheckedRelease {
    pub summary: Release,
    pub asset: Option<Asset>,
}

/// 读取公开发行记录并以语义版本选出最近发布列表中版本最高的一项，包含预览版。
pub fn check_release(page: &str, proxy: &str) -> Result<Option<CheckedRelease>, Message> {
    let repo = repository_path(page)?;
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    let current = parse_version(VERSION).ok_or_else(|| Message::new("update.invalid_version"))?;
    let releases = github_json(&url, proxy, "update.check_failed")?;
    Ok(select_release(releases, &repo, &current))
}

fn select_release(
    releases: Vec<PublishedRelease>,
    repo: &str,
    current: &Version,
) -> Option<CheckedRelease> {
    releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| Some((parse_version(&release.tag_name)?, release)))
        .max_by(|(left, _), (right, _)| left.cmp_precedence(right))
        .map(|(version, release)| {
            let asset = release.assets.into_iter().find_map(|asset| {
                asset
                    .name
                    .eq_ignore_ascii_case("GalFriends.exe")
                    .then_some(())?;
                let url = format!(
                    "https://github.com/{repo}/releases/download/{}/{}",
                    release.tag_name, asset.name
                );
                Asset::from_release(url, asset.size, asset.digest.as_deref()?)
            });
            CheckedRelease {
                summary: Release {
                    tag: release.tag_name,
                    notes: release.body,
                    available: version.cmp_precedence(current).is_gt(),
                    installable: cfg!(all(windows, target_arch = "x86_64")) && asset.is_some(),
                },
                asset,
            }
        })
}

/// 读取现有 Skill 旁的 GalGear commit 标记；手动放置的副本可能没有版本标记。
pub fn skill_status(skill_directory: Option<&Path>) -> SkillStatus {
    let commit = skill_directory.and_then(|path| {
        let marker = path.parent()?.join(".galgear-commit");
        let mut sha = String::new();
        File::open(marker)
            .ok()?
            .take(128)
            .read_to_string(&mut sha)
            .ok()?;
        let sha = sha.trim();
        valid_commit(sha).then(|| sha.to_ascii_lowercase())
    });
    SkillStatus {
        installed: skill_directory.is_some(),
        commit,
        update_available: false,
    }
}

/// 比较本地 Skill 与上游 main，缺少可识别版本时标记为可更新。
pub fn check_skill(status: &mut SkillStatus, proxy: &str) -> Result<(), Message> {
    let commit = latest_skill_commit(proxy)?;
    status.update_available = status
        .commit
        .as_ref()
        .is_none_or(|local| !local.eq_ignore_ascii_case(&commit));
    Ok(())
}

pub(super) fn latest_skill_commit(proxy: &str) -> Result<String, Message> {
    // Git 引用发现只读取 main，无需安装 Git，也不消耗发行 API 的配额。
    let client = github_client(proxy, "update.skill_failed")?;
    let response = super::executable::fetch(&client, SKILL_REFS, "update.skill_failed")?;
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .is_none_or(|header| header != "application/x-git-upload-pack-advertisement")
    {
        return Err(Message::new("update.invalid_version"));
    }
    let mut body = Vec::new();
    response
        .take(REF_LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|error| Message::detail("update.skill_failed", error))?;
    if body.len() as u64 > REF_LIMIT {
        return Err(Message::new("update.invalid_version"));
    }
    parse_main_ref(&body).ok_or_else(|| Message::new("update.invalid_version"))
}

fn parse_main_ref(mut bytes: &[u8]) -> Option<String> {
    let service = read_packet(&mut bytes)??;
    if service.strip_suffix(b"\n").unwrap_or(service) != b"# service=git-upload-pack"
        || read_packet(&mut bytes)?.is_some()
    {
        return None;
    }
    let mut main = None;
    while let Some(line) = read_packet(&mut bytes)? {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let text = std::str::from_utf8(line).ok()?;
        let (sha, name) = text.split_once(' ')?;
        let name = name.split('\0').next()?;
        if name == "refs/heads/main" {
            if !valid_commit(sha) || main.is_some() {
                return None;
            }
            main = Some(sha.to_ascii_lowercase());
        }
    }
    if bytes.is_empty() { main } else { None }
}

// Some(None) 表示协议边界 0000，None 表示损坏或截断的记录。
fn read_packet<'a>(bytes: &mut &'a [u8]) -> Option<Option<&'a [u8]>> {
    let (header, rest) = bytes.split_at_checked(4)?;
    let size = usize::from_str_radix(std::str::from_utf8(header).ok()?, 16).ok()?;
    if size == 0 {
        *bytes = rest;
        return Some(None);
    }
    let (packet, tail) = rest.split_at_checked(size.checked_sub(4)?)?;
    *bytes = tail;
    Some(Some(packet))
}

pub(super) fn valid_commit(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn github_json<T: serde::de::DeserializeOwned>(
    url: &str,
    proxy: &str,
    error_key: &str,
) -> Result<T, Message> {
    let error = |detail: String| Message::detail(error_key, detail);
    let client = github_client(proxy, error_key)?;
    let value = crate::sources::request(
        client
            .get(url)
            .header("Accept", "application/vnd.github+json"),
    )
    .map_err(error)?;
    serde_json::from_value(value).map_err(|e| error(e.to_string()))
}

fn github_client(proxy: &str, error_key: &str) -> Result<Client, Message> {
    let error = |detail: String| Message::detail(error_key, detail);
    crate::sources::client_builder(proxy)
        .map_err(error)?
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| error(e.without_url().to_string()))
}

#[cfg(test)]
mod tests {
    use super::{Version, parse_main_ref, release_page, select_release};
    use serde_json::json;

    #[test]
    fn links_use_the_saved_github_repository_and_a_semver_tag() {
        assert_eq!(
            release_page(" https://github.com/Owner/GalFriends/releases/ ").unwrap(),
            "https://github.com/Owner/GalFriends/releases"
        );
        for page in [
            "http://github.com/owner/repo/releases",
            "https://github.com.evil.test/owner/repo/releases",
            "https://user@github.com/owner/repo/releases",
            "https://github.com/owner/repo/releases?token=value",
            "https://github.com/owner/repo/releases/tag/v1.0.0",
        ] {
            assert!(release_page(page).is_err(), "{page}");
        }
    }

    #[test]
    fn update_check_includes_published_previews_and_skips_drafts() {
        let releases = json!([
            {"tag_name":"v9.0.0","draft":true,"body":"draft"},
            {"tag_name":"v0.9.0","draft":false,"body":"older"},
            {"tag_name":"v0.10.0-rc.2","draft":false,"prerelease":true,"body":"preview",
             "assets":[{"name":"GalFriends.exe","size":4096,"digest":format!("sha256:{}", "a".repeat(64))}]},
            {"tag_name":"v0.10.0-rc.1","draft":false,"body":"earlier preview"}
        ]);
        let release = select_release(
            serde_json::from_value(releases).unwrap(),
            "Owner/GalFriends",
            &Version::new(0, 9, 0),
        )
        .unwrap();
        assert!(release.summary.available);
        assert_eq!(release.summary.tag, "v0.10.0-rc.2");
        assert_eq!(release.summary.notes.as_deref(), Some("preview"));
        assert!(release.asset.is_some());
    }

    #[test]
    fn git_advertisement_uses_only_the_complete_main_ref() {
        let sha = "c".repeat(40);
        let mut body =
            format!("001e# service=git-upload-pack\n0000003d{sha} refs/heads/main\n0000");
        assert_eq!(parse_main_ref(body.as_bytes()), Some(sha));
        body.truncate(body.len() - 1);
        assert_eq!(parse_main_ref(body.as_bytes()), None);
    }
}
