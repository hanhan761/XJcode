use aes::cipher::{AsyncStreamCipher, BlockDecryptMut, KeyIvInit};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub access_key: String,
    pub secret: String,
    pub account_id: String,
}
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials([REDACTED])")
    }
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.access_key.zeroize();
        self.secret.zeroize();
    }
}
impl Credentials {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (8..=256).contains(&self.access_key.len())
                && (8..=256).contains(&self.secret.len())
                && !self.account_id.is_empty()
                && self.account_id.len() <= 512,
            "登录返回的凭据格式无效"
        );
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoginResult {
    pub nonce: String,
    pub account_id: String,
    pub tenant_id: String,
    pub access_key: String,
    pub encrypted_secret: String,
}
pub fn decode_secret(secret: &str, tenant: &str) -> Result<String> {
    let bytes = if let Some(code) = secret.strip_prefix("$aes-cfb$") {
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(code)
            .context("密钥编码无效")?;
        ensure!(bytes.len() > 16, "加密密钥过短");
        let key = Sha256::digest(tenant.as_bytes());
        let iv = bytes[..16].to_vec();
        cfb_mode::Decryptor::<aes::Aes256>::new_from_slices(&key, &iv)
            .map_err(|_| anyhow::anyhow!("密钥解密失败"))?
            .decrypt(&mut bytes[16..]);
        bytes[16..].to_vec()
    } else if let Some(code) = secret.strip_prefix("$aes-cbc$") {
        let bytes = hex::decode(code).context("密钥编码无效")?;
        ensure!(tenant.is_ascii(), "租户标识格式不支持");
        let mut key = [b'0'; 16];
        let n = tenant.len().min(16);
        key[..n].copy_from_slice(&tenant.as_bytes()[..n]);
        cbc::Decryptor::<aes::Aes128>::new(&key.into(), &key.into())
            .decrypt_padded_vec_mut::<aes::cipher::block_padding::Pkcs7>(&bytes)
            .map_err(|_| anyhow::anyhow!("密钥解密失败"))?
    } else {
        ensure!(
            !secret.starts_with('$'),
            "平台密钥加密格式已变化，请更新 XJCode"
        );
        secret.as_bytes().to_vec()
    };
    String::from_utf8(bytes).context("解密后的密钥不是有效文本")
}
pub fn home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("XJCODE_HOME") {
        let p = PathBuf::from(p);
        ensure!(p.is_absolute(), "XJCODE_HOME 必须是绝对路径");
        return Ok(p);
    }
    Ok(
        PathBuf::from(std::env::var_os("USERPROFILE").context("找不到 Windows 用户目录")?)
            .join(".xjcode"),
    )
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("无效保存路径")?;
    std::fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|_| anyhow::anyhow!("无法原子保存 XJCode 文件"))?;
    Ok(())
}
pub fn save_at(home: &Path, c: &Credentials) -> Result<()> {
    c.validate()?;
    let mut plain = serde_json::to_vec(c)?;
    let protected = protect(&plain);
    plain.zeroize();
    atomic_write(&home.join("credentials.dpapi"), &protected?)
}
pub fn load_at(home: &Path) -> Result<Credentials> {
    let raw =
        std::fs::read(home.join("credentials.dpapi")).context("尚未登录，请运行 xjcode login")?;
    ensure!(raw.len() <= 16384, "凭据文件大小异常");
    let mut plain = unprotect(&raw)?;
    let parsed = serde_json::from_slice::<Credentials>(&plain);
    plain.zeroize();
    let c = parsed.context("凭据文件损坏，请重新登录")?;
    c.validate()?;
    Ok(c)
}
pub fn logout_at(home: &Path) -> Result<()> {
    match std::fs::remove_file(home.join("credentials.dpapi")) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
#[cfg(windows)]
fn crypt(data: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into()?,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    ensure!(
        ok != 0,
        "Windows DPAPI 无法处理凭据，请用原 Windows 用户重新登录"
    );
    let out = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) }.zeroize();
    unsafe {
        LocalFree(output.pbData as _);
    };
    Ok(out)
}
#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    crypt(data, false)
}
#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    crypt(data, true)
}
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("XJCode 凭据存储仅支持 Windows")
}
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("XJCode 凭据存储仅支持 Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_redacted() {
        let c = Credentials {
            access_key: "test-access".into(),
            secret: "test-secret".into(),
            account_id: "test-account".into(),
        };
        assert!(!format!("{c:?}").contains("test"));
    }
    #[test]
    fn cfb_roundtrip_matches_portal() {
        let tenant = "tenant-fixture";
        let key = Sha256::digest(tenant.as_bytes());
        let iv = [3u8; 16];
        let mut text = b"fixture-secret-123".to_vec();
        cfb_mode::Encryptor::<aes::Aes256>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt(&mut text);
        let mut bytes = iv.to_vec();
        bytes.extend(text);
        let encoded = format!(
            "{}{}",
            "$aes-cfb$",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        assert_eq!(
            decode_secret(&encoded, tenant).unwrap(),
            "fixture-secret-123"
        );
        assert!(decode_secret("$sm4$unexpected", tenant).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn vault_roundtrip_and_tamper() {
        let dir = tempfile::tempdir().unwrap();
        let c = Credentials {
            access_key: "fixture-access".into(),
            secret: "fixture-secret".into(),
            account_id: "fixture-account".into(),
        };
        save_at(dir.path(), &c).unwrap();
        assert_eq!(load_at(dir.path()).unwrap().secret, c.secret);
        let p = dir.path().join("credentials.dpapi");
        let mut raw = std::fs::read(&p).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("fixture-secret"));
        let last = raw.len() - 1;
        raw[last] ^= 42;
        std::fs::write(p, raw).unwrap();
        assert!(load_at(dir.path()).is_err());
        logout_at(dir.path()).unwrap();
    }
}
