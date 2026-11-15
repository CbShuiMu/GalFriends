//! 移植 GalGear 的 Windows Credential Manager 存储；GalFriends 使用独立目标名。
use super::{Service, Status, oauth::Exchange};
use galfriends_i18n::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// 一个服务的完整凭据，模型与密钥一起原子替换，避免部分保存。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Credentials {
    pub token: String,
    pub model: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub oauth_redirect_uri: String,
    pub oauth_exchange: Exchange,
}
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials([redacted])")
    }
}

/// 凭据目标名绑定 GalFriends 配置路径，不读取或覆盖 GalGear 条目。
pub fn target(config: &Path, service: Service) -> String {
    let normalized = config.to_string_lossy().replace('/', "\\").to_lowercase();
    format!(
        "GalFriends/{}/{:x}",
        service.key(),
        Sha256::digest(normalized.as_bytes())
    )
}

/// 仅读取是否配置和已选模型，不将密钥返回 UI。
pub fn status(config: &Path, service: Service) -> Result<Status, Message> {
    let value = load(config, service)?;
    Ok(Status {
        service,
        configured: !value.token.is_empty(),
        model: value.model,
    })
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// 仅写入系统凭据库，失败不替换旧凭据。
#[cfg(windows)]
pub fn save(config: &Path, service: Service, value: &Credentials) -> Result<(), Message> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
        CredWriteW,
    };
    let mut bytes = serde_json::to_vec(value).map_err(|_| Message::new("auth.store_error"))?;
    if bytes.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        bytes.fill(0);
        return Err(Message::new("auth.too_long"));
    }
    let mut name = wide(&target(config, service));
    let mut user = wide(service.name());
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: name.as_mut_ptr(),
        UserName: user.as_mut_ptr(),
        CredentialBlobSize: bytes.len() as u32,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: 所有指针均指向本次同步调用期间有效的缓冲区，名称以 NUL 结尾。
    let ok = unsafe { CredWriteW(&credential, 0) } != 0;
    bytes.fill(0);
    if ok {
        Ok(())
    } else {
        Err(Message::new("auth.store_error"))
    }
}

/// 读取本应用凭据；未保存返回空值，存储错误不会降级为未配置。
#[cfg(windows)]
pub fn load(config: &Path, service: Service) -> Result<Credentials, Message> {
    use windows_sys::Win32::{
        Foundation::{ERROR_NOT_FOUND, GetLastError},
        Security::Credentials::{CRED_TYPE_GENERIC, CredFree, CredReadW},
    };
    let name = wide(&target(config, service));
    let mut pointer = std::ptr::null_mut();
    // SAFETY: 名称以 NUL 结尾，输出指针有效；失败时不解引用。
    if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
        // SAFETY: 紧随失败调用读取当前线程的 Win32 错误。
        return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            Ok(Credentials::default())
        } else {
            Err(Message::new("auth.store_error"))
        };
    }
    // SAFETY: 成功的 CredReadW 返回有效结构和指定长度缓冲区；解析完成后恰好释放一次。
    unsafe {
        let credential = &*pointer;
        let result = if credential.CredentialBlobSize == 0 {
            Ok(Credentials::default())
        } else {
            serde_json::from_slice(std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            ))
            .map_err(|_| Message::new("auth.store_error"))
        };
        CredFree(pointer.cast());
        result
    }
}

/// 仅清除此应用和服务的凭据，已不存在视为成功。
#[cfg(windows)]
pub fn clear(config: &Path, service: Service) -> Result<(), Message> {
    use windows_sys::Win32::{
        Foundation::{ERROR_NOT_FOUND, GetLastError},
        Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW},
    };
    let name = wide(&target(config, service));
    // SAFETY: 目标名称是有效的 NUL 终止缓冲区；读取错误前不执行其他 Win32 调用。
    if unsafe {
        CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) != 0 || GetLastError() == ERROR_NOT_FOUND
    } {
        Ok(())
    } else {
        Err(Message::new("auth.store_error"))
    }
}
#[cfg(not(windows))]
pub fn load(_: &Path, _: Service) -> Result<Credentials, Message> {
    Err(Message::new("auth.platform"))
}
#[cfg(not(windows))]
pub fn save(_: &Path, _: Service, _: &Credentials) -> Result<(), Message> {
    Err(Message::new("auth.platform"))
}
#[cfg(not(windows))]
pub fn clear(_: &Path, _: Service) -> Result<(), Message> {
    Err(Message::new("auth.platform"))
}

#[cfg(test)]
mod tests {
    use super::{Credentials, Service, target};
    use std::path::Path;
    #[test]
    fn secret_debug_is_redacted_and_targets_are_isolated() {
        let value = Credentials {
            token: "private-access".into(),
            refresh_token: "private-refresh".into(),
            ..Default::default()
        };
        assert!(!format!("{value:?}").contains("private"));
        let first = target(Path::new("E:/GalFriends/library.json"), Service::Bangumi);
        assert!(first.starts_with("GalFriends/"));
        assert_ne!(
            first,
            target(Path::new("E:/GalFriends/library.json"), Service::Vndb)
        );
        assert_ne!(
            first,
            target(Path::new("E:/other/library.json"), Service::Bangumi)
        );
    }
    #[cfg(windows)]
    #[test]
    fn isolated_vault_record_roundtrips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("library.json");
        let value = Credentials {
            token: "fixture-not-a-real-key".into(),
            model: "fixture-model".into(),
            ..Default::default()
        };
        super::save(&config, Service::Openai, &value).unwrap();
        let loaded = super::load(&config, Service::Openai);
        let cleared = super::clear(&config, Service::Openai);
        assert_eq!(loaded.unwrap().model, "fixture-model");
        cleared.unwrap();
        assert!(!super::status(&config, Service::Openai).unwrap().configured);
    }
}
