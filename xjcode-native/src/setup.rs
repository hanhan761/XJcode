use crate::{api::Model, auth::atomic_write};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub account_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub model: String,
    pub models: Vec<Model>,
}
pub fn load(home: &Path) -> Result<Settings> {
    let mut settings = serde_json::from_slice(
        &std::fs::read(home.join("xjtu.json")).context("请运行 xjcode login 完成模型配置")?,
    )
    .context("XJCode 配置损坏")?;
    crate::capabilities::apply(home, &mut settings);
    Ok(settings)
}
pub fn catalog(s: &Settings) -> Value {
    json!({"models":s.models.iter().map(|m|json!({
        "slug":m.name,"display_name":m.name,"description":if m.verified_image_input {"交晓智原生接口 · 支持图片"} else if m.platform_supports_images() {"交晓智原生接口 · 支持图片（平台声明）"} else {"交晓智原生接口"},
        "supported_reasoning_levels":[],"shell_type":"shell_command","visibility":"list","supported_in_api":true,
        "priority":if m.name==s.model {0} else {10},"availability_nux":null,"upgrade":null,
        "include_skills_usage_instructions":true,"include_plugin_usage_instructions":true,"include_apps_usage_instructions":false,
        "supports_reasoning_summary_parameter":false,"support_verbosity":false,"default_verbosity":null,
        "apply_patch_tool_type":"freeform","truncation_policy":{"mode":"bytes","limit":12000},
        "context_window":32768,"effective_context_window_percent":80,"experimental_supported_tools":[],
        "input_modalities":if m.supports_images() {vec!["text","image"]} else {vec!["text"]},"node_repl_disabled":true,"guardian":{},
        "model_messages":{"instructions_template":"You are XJCode, a coding agent using XJTU models. Use the provided tools to inspect and modify the current project, obey AGENTS.md, verify changes, and report actual results. Never claim execution without tool evidence. Prefer concise Chinese explanations unless the user requests another language."}
    })).collect::<Vec<_>>()})
}
pub fn save(home: &Path, s: &Settings) -> Result<()> {
    let mut scoped = s.clone();
    crate::capabilities::apply(home, &mut scoped);
    let s = &scoped;
    ensure!(
        s.models.iter().any(|m| m.name == s.model),
        "默认模型不在授权目录中"
    );
    let file = home.join("config.toml");
    let old = match std::fs::read_to_string(&file) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context("无法读取 config.toml，未覆盖"),
    };
    let mut doc = old
        .parse::<toml_edit::DocumentMut>()
        .context("config.toml 格式错误，未覆盖")?;
    for key in ["analytics", "feedback", "features", "model_providers"] {
        ensure!(doc.get(key).is_none_or(|v| v.is_table_like()), "config.toml 的 {key} 必须是表，未覆盖");
    }
    if let Some(providers) = doc.get("model_providers").and_then(|v| v.as_table_like()) {
        ensure!(providers.get("xjtu").is_none_or(|v| v.is_table_like()), "xjtu provider 必须是表，未覆盖");
    }
    if doc.get("model_provider").and_then(|v|v.as_str())==Some("custom") { doc.remove("model_context_window"); }
    doc["model"] = toml_edit::value(&s.model);
    doc["model_provider"] = toml_edit::value("xjtu");
    doc["model_catalog_json"] =
        toml_edit::value(home.join("models.json").to_string_lossy().as_ref());
    doc["check_for_update_on_startup"] = toml_edit::value(false);
    doc["web_search"] = toml_edit::value("disabled");
    doc["analytics"]["enabled"] = toml_edit::value(false);
    doc["feedback"]["enabled"] = toml_edit::value(false);
    doc["allow_login_shell"] = toml_edit::value(false);
    doc["model_providers"]["xjtu"]["name"] = toml_edit::value("交晓智");
    doc["model_providers"]["xjtu"]["wire_api"] = toml_edit::value("xjtu_top");
    doc["model_providers"]["xjtu"]["requires_openai_auth"] = toml_edit::value(false);
    doc["model_providers"]["xjtu"]["base_url"] = toml_edit::value(crate::api::ENDPOINT);
    doc["model_providers"]["xjtu"]["stream_max_retries"] = toml_edit::value(0);
    doc["model_providers"]["xjtu"]["request_max_retries"] = toml_edit::value(0);
    doc["features"]["memories"] = toml_edit::value(false);
    doc["features"]["apps"] = toml_edit::value(false);
    doc["features"]["guardian_approval"] = toml_edit::value(false);
    doc["features"]["daemon_auto_start"] = toml_edit::value(false);
    atomic_write(
        &home.join("models.json"),
        &serde_json::to_vec_pretty(&catalog(s))?,
    )?;
    atomic_write(&home.join("xjtu.json"), &serde_json::to_vec_pretty(s)?)?;
    atomic_write(&file, doc.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings { account_id: "fixture".into(), workspace_id: "workspace".into(),
            workspace_name: "工作区".into(), model: "fixture-model".into(),
            models: vec![Model { name: "fixture-model".into(), service_id: "service".into(),
                upstream_name: "upstream".into(), source_type: "external".into(), features: vec![], verified_image_input: false }] }
    }
    #[test]
    fn config_update_preserves_user_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "model_context_window = 64000\n[mcp_servers.local]\ncommand = 'fixture-server'\n").unwrap();
        save(dir.path(), &settings()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        let config = text.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(config["mcp_servers"]["local"]["command"].as_str(), Some("fixture-server"));
        assert_eq!(config["model_providers"]["xjtu"]["wire_api"].as_str(), Some("xjtu_top"));
        assert_eq!(config["analytics"]["enabled"].as_bool(), Some(false));
        assert_eq!(config["model_context_window"].as_integer(),Some(64000));
    }
    #[test]
    fn only_explicit_campus_vision_features_enable_image_inputs() {
        let mut settings = settings();
        settings.models[0].name = "qwen-vl-looking-name".into();
        assert_eq!(catalog(&settings)["models"][0]["input_modalities"], json!(["text"]));
        settings.models[0].features = vec!["vision".into()];
        assert_eq!(catalog(&settings)["models"][0]["input_modalities"], json!(["text", "image"]));
        let mut legacy = serde_json::to_value(&settings).unwrap();
        legacy["models"][0].as_object_mut().unwrap().remove("features");
        let restored: Settings = serde_json::from_value(legacy).unwrap();
        assert!(!restored.models[0].supports_images());
    }
    #[test]
    fn invalid_existing_config_does_not_replace_account_state() {
        for broken in ["[broken", "features = true"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("config.toml"), broken).unwrap();
            std::fs::write(dir.path().join("xjtu.json"), "old-account").unwrap();
            assert!(save(dir.path(), &settings()).is_err());
            assert_eq!(std::fs::read_to_string(dir.path().join("xjtu.json")).unwrap(), "old-account");
            assert!(!dir.path().join("models.json").exists());
        }
    }
}
