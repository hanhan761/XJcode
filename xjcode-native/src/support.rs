//! Local-only, bounded and redacted diagnostics for manual support.
use serde_json::Value;
use std::path::Path;
fn label(value: Option<&Value>, max: usize) -> Option<String> {
    let s=value?.as_str()?;
    if s.is_empty() || s.len()>max || !s.bytes().all(|b|b.is_ascii_alphanumeric() || b"-_.:+".contains(&b)) {return None}
    Some(s.to_string())
}
pub fn recent(home: &Path) -> Vec<String> {
    let file=home.join("diagnostics/requests.jsonl");
    let meta=match std::fs::metadata(&file) {Ok(v) if v.len()<=4*1024*1024=>v,_=>return vec![]};
    if !meta.is_file() {return vec![]}
    let content=match std::fs::read_to_string(file) {Ok(v)=>v,Err(_)=>return vec![]};
    let mut lines=content.lines().rev().filter_map(|line| {
        let v:Value=serde_json::from_str(line).ok()?;
        let time=label(v.get("utc_time"),64).unwrap_or_else(||"时间未知".into());
        let action=label(v.get("action"),64).unwrap_or_else(||"unknown".into());
        let kind=label(v.get("kind"),64).unwrap_or_else(||"unknown".into());
        let model=label(v.get("model"),80).map(|m|format!("；模型={m}")).unwrap_or_default();
        let status=v.get("http_status").and_then(Value::as_u64).filter(|n|(100..=599).contains(n)).map(|n|format!("；HTTP {n}")).unwrap_or_default();
        let request_id=label(v.get("request_id"),128).map(|id|format!("；请求ID={id}")).unwrap_or_default();
        let code=label(v.get("upstream_code"),128).map(|id|format!("；错误码={id}")).unwrap_or_default();
        Some(format!("{time}；环节={action}；类型={kind}{model}{status}{code}{request_id}"))
    }).take(10).collect::<Vec<_>>(); lines.reverse(); lines
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn omits_private_fields_and_rejects_untrusted_labels() {
  let dir=tempfile::tempdir().unwrap();std::fs::create_dir(dir.path().join("diagnostics")).unwrap();
  std::fs::write(dir.path().join("diagnostics/requests.jsonl"),"{\"utc_time\":\"2026-11-11T10:00:00Z\",\"action\":\"Chat\",\"kind\":\"http\",\"model\":\"qwen-max\",\"http_status\":503,\"prompt\":\"private prompt\",\"token\":\"secret-token\"}\n{\"action\":\"bad\\ninput\",\"kind\":\"network\"}\n").unwrap();
  let s=recent(dir.path()).join("\n");assert!(s.contains("HTTP 503"));assert!(s.contains("qwen-max"));assert!(!s.contains("private prompt"));assert!(!s.contains("secret-token"));assert!(!s.contains("bad\ninput"));
 }
}
