//! Explicit user-selected provider. Never infer a fallback from network failures.
use crate::{auth, setup};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{self, IsTerminal, Write},
    path::Path,
};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiProfile {
    pub base_url: String,
    pub model: String,
    pub wire_api: String,
    pub supports_images: bool,
    pub context_window: u64,
    token: String,
}
impl std::fmt::Debug for ApiProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiProfile([redacted])")
    }
}
impl Drop for ApiProfile {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}
impl ApiProfile {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.base_url).context("API 地址格式无效")?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))),
            "API 地址须为 HTTPS（本机服务可用 HTTP）"
        );
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "API 地址不能包含凭据、查询参数或片段"
        );
        ensure!(
            matches!(self.wire_api.as_str(), "responses" | "chat"),
            "协议须为 responses 或 chat"
        );
        ensure!(
            !self.model.trim().is_empty()
                && self.model.len() < 256
                && !self.model.chars().any(char::is_control),
            "模型名称无效"
        );
        ensure!(
            !self.token.trim().is_empty()
                && self.token.len() < 16384
                && !self.token.chars().any(char::is_control),
            "Token 为空或格式无效"
        );
        ensure!(
            (4096..=2_000_000).contains(&self.context_window),
            "上下文预算须在 4096–2000000 之间"
        );
        Ok(())
    }
    pub fn provider_info(&self) -> Value {
        json!({"name":"自定义 API", "base_url":self.base_url.trim_end_matches('/'),
            "wire_api":if self.wire_api=="chat" {"xjtu_top"} else {"responses"},
            "experimental_bearer_token":self.token,"requires_openai_auth":false,
            "supports_websockets":false,"request_max_retries":0,"stream_max_retries":0})
    }
    pub fn catalog(&self) -> Value {
        let s = setup::Settings {
            account_id: "custom-api".into(),
            workspace_id: "custom-api".into(),
            workspace_name: "自定义 API".into(),
            model: self.model.clone(),
            models: vec![crate::api::Model {
                name: self.model.clone(),
                service_id: "custom-api".into(),
                upstream_name: self.model.clone(),
                source_type: "custom".into(),
                features: if self.supports_images {
                    vec!["vision".into()]
                } else {
                    vec![]
                },
                verified_image_input: false,
            }],
        };
        let mut v = setup::catalog(&s);
        v["models"][0]["description"] = json!("自定义 API · 用户配置");
        v["models"][0]["context_window"] = json!(self.context_window);
        v["models"][0]["model_messages"]["instructions_template"] = json!(
            "You are XJCode, a local coding agent. Inspect files, obey AGENTS.md, execute tools, verify changes and report actual results. Prefer concise Chinese explanations."
        );
        v
    }
}
pub fn active(home: &Path) -> Result<String> {
    match std::fs::read_to_string(home.join("selected-provider")) {
        Ok(s) => {
            let s = s.trim();
            ensure!(
                matches!(s, "xjtu" | "custom"),
                "服务选择文件无效，请运行 xjcode provider --select"
            );
            Ok(s.into())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok("xjtu".into()),
        Err(e) => Err(e.into()),
    }
}
pub fn is_custom(home: &Path) -> Result<bool> {
    Ok(active(home)? == "custom")
}
pub fn load(home: &Path) -> Result<ApiProfile> {
    let bytes = std::fs::read(home.join("custom-api.dpapi"))
        .context("尚未配置备用 API，请运行 xjcode login --api")?;
    ensure!(bytes.len() < 65536, "API 配置大小异常");
    let mut plain = auth::unprotect(&bytes)?;
    let parsed = serde_json::from_slice::<ApiProfile>(&plain);
    plain.zeroize();
    let p = parsed.context("API 配置损坏，请重新配置")?;
    p.validate()?;
    Ok(p)
}
pub fn save(home: &Path, p: &ApiProfile) -> Result<()> {
    p.validate()?;
    let mut plain = serde_json::to_vec(p)?;
    let encrypted = auth::protect(&plain);
    plain.zeroize();
    auth::atomic_write(&home.join("custom-api.dpapi"), &encrypted?)
}
pub fn select(home: &Path, id: &str) -> Result<()> {
    ensure!(matches!(id, "xjtu" | "custom"), "未知服务入口");
    if id == "xjtu" {
        let s = setup::load(home)?;
        let c = auth::load_at(home)?;
        ensure!(c.account_id == s.account_id, "校园账号与配置不一致");
        setup::save(home, &s)?;
    } else {
        configure(home, &load(home)?)?;
    }
    auth::atomic_write(&home.join("selected-provider"), id.as_bytes())
}
pub fn configure(home: &Path, p: &ApiProfile) -> Result<()> {
    p.validate()?;
    let file = home.join("config.toml");
    let old = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc = old
        .parse::<toml_edit::DocumentMut>()
        .context("config.toml 格式错误，未覆盖")?;
    doc["model"] = toml_edit::value(&p.model);
    doc["model_provider"] = toml_edit::value("custom");
    doc["model_catalog_json"] =
        toml_edit::value(home.join("custom-models.json").to_string_lossy().as_ref());
    doc["model_context_window"] = toml_edit::value(p.context_window as i64);
    // XJCode owns update discovery; do not contact upstream Codex release services.
    doc["check_for_update_on_startup"] = toml_edit::value(false);
    doc["web_search"] = toml_edit::value("disabled");
    for table in ["analytics", "feedback", "features"] {
        ensure!(
            doc.get(table).is_none_or(|v| v.is_table_like()),
            "config.toml 表格式无效，未覆盖"
        );
    }
    for table in ["analytics", "feedback"] {
        doc[table]["enabled"] = toml_edit::value(false);
    }
    for feature in ["memories", "apps", "guardian_approval", "daemon_auto_start"] {
        doc["features"][feature] = toml_edit::value(false);
    }
    doc["allow_login_shell"] = toml_edit::value(false);
    auth::atomic_write(
        &home.join("custom-models.json"),
        &serde_json::to_vec_pretty(&p.catalog())?,
    )?;
    auth::atomic_write(&file, doc.to_string().as_bytes())
}
pub fn model_allowed(home: &Path, model: &str) -> Result<bool> {
    if is_custom(home)? {
        Ok(load(home)?.model == model)
    } else {
        Ok(setup::load(home)?.models.iter().any(|m| m.name == model))
    }
}
pub fn selected_info(home: &Path) -> Result<(String, Value)> {
    if is_custom(home)? {
        Ok(("custom".into(), load(home)?.provider_info()))
    } else {
        Ok((
            "xjtu".into(),
            json!({"name":"交晓智","wire_api":"xjtu_top","base_url":crate::api::ENDPOINT,"requires_openai_auth":false,"request_max_retries":0,"stream_max_retries":0}),
        ))
    }
}
fn prompt(label: &str) -> Result<String> {
    ensure!(
        io::stdin().is_terminal(),
        "无交互输入，请提供 --base-url、--model、--wire-api 和 --key-stdin"
    );
    print!("{label}：");
    io::stdout().flush()?;
    let mut s = String::new();
    ensure!(io::stdin().read_line(&mut s)? > 0, "配置已取消");
    Ok(s.trim().into())
}
#[cfg(windows)]
fn secret() -> Result<String> {
    use windows_sys::Win32::System::Console::*;
    struct Restore(windows_sys::Win32::Foundation::HANDLE, u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.0, self.1);
            }
        }
    }
    ensure!(
        io::stdin().is_terminal(),
        "非终端输入须显式指定 --key-stdin"
    );
    print!("API Token（隐藏输入）：");
    io::stdout().flush()?;
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut mode = 0;
    ensure!(
        unsafe { GetConsoleMode(handle, &mut mode) } != 0,
        "无法隐藏 Token 输入"
    );
    let _restore = Restore(handle, mode);
    ensure!(
        unsafe { SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT) } != 0,
        "无法隐藏 Token 输入"
    );
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    println!();
    Ok(s.trim().into())
}
#[cfg(not(windows))]
fn secret() -> Result<String> {
    bail!("仅支持 Windows")
}
pub fn wizard(home: &Path, args: &[std::ffi::OsString]) -> Result<()> {
    let mut base = None;
    let mut model = None;
    let mut wire = None;
    let mut stdin_key = false;
    let mut images = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--base-url") => {
                base = Some(
                    it.next()
                        .and_then(|v| v.to_str())
                        .context("缺少 API 地址")?
                        .to_owned(),
                )
            }
            Some("--model") => {
                model = Some(
                    it.next()
                        .and_then(|v| v.to_str())
                        .context("缺少模型名称")?
                        .to_owned(),
                )
            }
            Some("--wire-api") => {
                wire = Some(
                    it.next()
                        .and_then(|v| v.to_str())
                        .context("缺少协议")?
                        .to_owned(),
                )
            }
            Some("--key-stdin") => stdin_key = true,
            Some("--images") => images = true,
            _ => bail!(
                "用法：xjcode login --api [--base-url 地址 --model 名称 --wire-api responses|chat --key-stdin] [--images]"
            ),
        }
    }
    let base_url = match base {
        Some(v) => v,
        None => prompt("API 基础地址（例如 https://服务地址/v1）")?,
    };
    let model = match model {
        Some(v) => v,
        None => prompt("模型名称")?,
    };
    let wire_api = match wire {
        Some(v) => v,
        None => prompt("协议（responses 或 chat）")?,
    };
    let token = if stdin_key {
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        s.trim().into()
    } else {
        secret()?
    };
    let p = ApiProfile {
        base_url,
        model,
        wire_api,
        token,
        supports_images: images,
        context_window: 32768,
    };
    auth::mark_home(home)?;
    // Validate existing configuration before replacing the saved profile.
    configure(home, &p)?;
    save(home, &p)?;
    auth::atomic_write(&home.join("selected-provider"), b"custom")?;
    println!(
        "已切换到自定义 API · {} · {}。未发起模型调用；可运行 xjcode doctor --probe 检查。",
        p.model, p.wire_api
    );
    Ok(())
}

pub async fn chat(
    p: &ApiProfile,
    text: &str,
    images: &[crate::images::InputImage],
) -> Result<String> {
    use eventsource_stream::Eventsource;
    use futures_util::StreamExt;
    p.validate()?;
    crate::images::validate(images)?;
    ensure!(
        images.is_empty() || p.supports_images,
        "此 API 未启用图片能力；重新配置时可指定 --images"
    );
    let mut content = vec![json!({"type":"text","text":text})];
    content.extend(
        images
            .iter()
            .map(|i| json!({"type":"image_url","image_url":{"url":i.data_url()}})),
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .build()?;
    let response = client
        .post(format!(
            "{}/chat/completions",
            p.base_url.trim_end_matches('/')
        ))
        .bearer_auth(&p.token)
        .json(
            &json!({"model":p.model,"messages":[{"role":"user","content":content}],"stream":true}),
        )
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("API 连接失败或超时；该轮工具未执行"))?;
    ensure!(
        response.status().is_success(),
        "API 返回 HTTP {}；请检查地址、Token 与模型",
        response.status().as_u16()
    );
    let mut stream = response.bytes_stream().eventsource();
    let mut text = String::new();
    let mut finished = false;
    let mut done = false;
    let mut total = 0;
    while let Some(event) = stream.next().await {
        let event = event.map_err(|_| anyhow::anyhow!("API 流中断；该轮工具未执行"))?;
        total += event.data.len();
        ensure!(total <= 8 * 1024 * 1024, "API 回复超过限制");
        if event.data == "[DONE]" {
            done = true;
            break;
        }
        let value: Value = serde_json::from_str(&event.data).context("API 流格式无效")?;
        ensure!(
            value.get("error").is_none(),
            "API 返回流错误；该轮工具未执行"
        );
        if let Some(choice) = value["choices"].as_array().and_then(|a| a.first()) {
            if let Some(part) = choice["delta"]["content"].as_str() {
                text.push_str(part);
            }
            if !choice["finish_reason"].is_null() {
                ensure!(
                    choice["finish_reason"] == "stop",
                    "API 未正常完成回复；该轮工具未执行"
                );
                finished = true;
            }
        }
    }
    ensure!(finished && done, "API 回复不完整；该轮工具未执行");
    ensure!(!text.is_empty(), "API 回复为空");
    Ok(text)
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod transport_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> ApiProfile {
        ApiProfile {
            base_url: "https://example.org/v1".into(),
            model: "fixture".into(),
            wire_api: "chat".into(),
            token: "fixture-secret".into(),
            supports_images: false,
            context_window: 32768,
        }
    }
    #[test]
    fn validates_endpoint_and_redacts_secret() {
        let mut p = profile();
        assert!(p.validate().is_ok());
        assert!(!format!("{p:?}").contains("fixture-secret"));
        for url in [
            "http://example.org/v1",
            "https://user:secret@example.org/v1",
            "https://example.org/v1?key=x",
        ] {
            p.base_url = url.into();
            assert!(p.validate().is_err());
        }
    }
    #[test]
    fn profile_is_encrypted_and_selection_is_explicit() {
        let d = tempfile::tempdir().unwrap();
        let p = profile();
        save(d.path(), &p).unwrap();
        let raw = std::fs::read(d.path().join("custom-api.dpapi")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("fixture-secret"));
        assert_eq!(load(d.path()).unwrap().model, "fixture");
        assert!(!is_custom(d.path()).unwrap());
        select(d.path(), "custom").unwrap();
        assert!(model_allowed(d.path(), "fixture").unwrap());
        assert!(!model_allowed(d.path(), "other").unwrap());
        assert!(
            !std::fs::read_to_string(d.path().join("config.toml"))
                .unwrap()
                .contains("fixture-secret")
        );
    }
}
