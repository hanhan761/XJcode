//! First-party stable feed. No model credentials or task content are sent.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ORIGIN: &str = "https://xjcode.wenshanzhike.com";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Latest {
    pub schema: u32,
    pub version: String,
    pub manifest_sha256: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    latest: Option<Latest>,
    #[serde(default)]
    checked: i64,
    #[serde(default)]
    dismissed: Option<String>,
}
pub fn version(s: &str) -> Result<[u64; 3]> {
    let p = s.split('.').collect::<Vec<_>>();
    ensure!(
        p.len() == 3
            && p.iter()
                .all(|s| !s.is_empty() && s.len() < 10 && s.bytes().all(|b| b.is_ascii_digit())),
        "版本号无效"
    );
    Ok([p[0].parse()?, p[1].parse()?, p[2].parse()?])
}
pub fn hash_valid(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Latest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && hash_valid(&self.manifest_sha256),
            "更新清单无效"
        );
        version(&self.version)?;
        Ok(())
    }
}
fn cache(home: &Path) -> Cache {
    std::fs::read(home.join("update-state.json"))
        .ok()
        .filter(|v| v.len() < 65536)
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default()
}
fn store(home: &Path, c: &Cache) -> Result<()> {
    crate::auth::atomic_write(&home.join("update-state.json"), &serde_json::to_vec(c)?)
}
pub fn cached_notice(home: &Path) -> Option<String> {
    let c = cache(home);
    let l = c.latest?;
    l.validate().ok()?;
    if version(&l.version).ok()? > version(VERSION).ok()?
        && c.dismissed.as_deref() != Some(&l.version)
    {
        Some(l.version)
    } else {
        None
    }
}
pub fn due(home: &Path) -> bool {
    let now = chrono::Utc::now().timestamp();
    let checked = cache(home).checked;
    checked > now || now - checked >= 86400
}
pub fn dismiss(home: &Path, v: &str) -> Result<()> {
    version(v)?;
    let mut c = cache(home);
    c.dismissed = Some(v.into());
    store(home, &c)
}
pub fn client(timeout: u64) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(timeout))
        .build()?)
}
pub async fn check(home: &Path) -> Result<Latest> {
    let data = download(&client(3)?, &format!("{ORIGIN}/latest.json"), 65536).await?;
    let latest: Latest = serde_json::from_slice(&data).context("更新清单格式错误")?;
    latest.validate()?;
    let mut c = cache(home);
    c.latest = Some(latest.clone());
    c.checked = chrono::Utc::now().timestamp();
    store(home, &c)?;
    Ok(latest)
}
pub async fn download(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let u = reqwest::Url::parse(url)?;
    ensure!(
        u.scheme() == "https"
            && u.host_str() == Some("xjcode.wenshanzhike.com")
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none(),
        "更新只允许官网 HTTPS 地址"
    );
    let mut r = client
        .get(u)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("无法连接 XJCode 更新服务器"))?;
    ensure!(
        r.status().is_success(),
        "更新服务器返回 HTTP {}",
        r.status().as_u16()
    );
    ensure!(
        r.content_length().is_none_or(|n| n <= limit as u64),
        "更新文件超过大小限制"
    );
    let mut data = Vec::new();
    while let Some(b) = r
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("更新下载中断"))?
    {
        ensure!(data.len() + b.len() <= limit, "更新文件超过大小限制");
        data.extend_from_slice(&b);
    }
    Ok(data)
}
pub async fn artifact(
    client: &reqwest::Client,
    url: &str,
    hash: &str,
    to: &Path,
    limit: u64,
) -> Result<()> {
    ensure!(hash_valid(hash), "更新文件校验值无效");
    let u = reqwest::Url::parse(url)?;
    ensure!(
        u.scheme() == "https"
            && u.host_str() == Some("xjcode.wenshanzhike.com")
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none(),
        "更新只允许官网 HTTPS 地址"
    );
    let mut response = client
        .get(u)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("更新下载失败"))?;
    ensure!(
        response.status().is_success(),
        "更新下载返回 HTTP {}",
        response.status().as_u16()
    );
    ensure!(
        response.content_length().is_none_or(|n| n <= limit),
        "更新文件过大"
    );
    let mut file = std::fs::File::create(to)?;
    let mut sha = Sha256::new();
    let mut size = 0u64;
    use std::io::Write;
    while let Some(bytes) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("更新下载中断"))?
    {
        size += bytes.len() as u64;
        ensure!(size <= limit, "更新文件过大");
        sha.update(&bytes);
        file.write_all(&bytes)?;
    }
    file.sync_all()?;
    ensure!(
        hex::encode(sha.finalize()).eq_ignore_ascii_case(hash),
        "更新文件校验失败，原安装保持可用"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_version_comparison() {
        assert!(version("0.1.10").unwrap() > version("0.1.9").unwrap());
        for v in ["../a", "1.2", "1.2.3.4", "1.2.-3", "1.2.3-beta"] {
            assert!(version(v).is_err());
        }
    }
    #[test]
    fn dismissal_scoped_to_version() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Cache {
            latest: Some(Latest {
                schema: 1,
                version: "9.0.0".into(),
                manifest_sha256: "a".repeat(64),
            }),
            checked: chrono::Utc::now().timestamp(),
            dismissed: None,
        };
        store(d.path(), &c).unwrap();
        assert!(!due(d.path()));
        assert_eq!(cached_notice(d.path()).as_deref(), Some("9.0.0"));
        dismiss(d.path(), "9.0.0").unwrap();
        assert!(cached_notice(d.path()).is_none());
        c.latest.as_mut().unwrap().version = "9.0.1".into();
        store(d.path(), &c).unwrap();
        assert!(cached_notice(d.path()).is_some());
    }
}
