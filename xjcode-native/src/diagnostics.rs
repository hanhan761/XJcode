//! Bounded request diagnostics. Never persist request bodies, response messages or credentials.
use crate::auth::Credentials;
use chrono::Utc;
use reqwest::header::HeaderMap;
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
};

const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
static LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Default)]
pub(crate) struct RequestContext<'a> {
    pub model: Option<&'a str>,
    pub prompt_bytes: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct ProtocolFailure {
    utc_time: String,
    action: &'static str,
    kind: &'static str,
    model: Option<String>,
    prompt_bytes: usize,
    reply_bytes: usize,
    attempt: usize,
    line: Option<usize>,
    column: Option<usize>,
}

impl ProtocolFailure {
    pub fn new(
        model: &str,
        prompt_bytes: usize,
        reply_bytes: usize,
        attempt: usize,
        error: &anyhow::Error,
        credentials: &Credentials,
    ) -> Self {
        let format = error.downcast_ref::<crate::protocol::ReplyFormatError>();
        Self {
            utc_time: Utc::now().to_rfc3339(),
            action: "ReplyProtocol",
            kind: format.map_or("protocol_validation", |e| e.kind),
            model: safe_model_label(model, credentials),
            prompt_bytes,
            reply_bytes,
            attempt,
            line: format.map(|e| e.line),
            column: format.map(|e| e.column),
        }
    }

    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        write_record(self, path)
    }
}

#[derive(Serialize)]
pub(crate) struct ChatReadback {
    utc_time: String,
    action: &'static str,
    kind: &'static str,
    model: Option<String>,
    prompt_bytes: Option<usize>,
    reply_bytes: Option<usize>,
}
impl ChatReadback {
    pub fn new(
        context: RequestContext<'_>,
        reply_bytes: Option<usize>,
        credentials: &Credentials,
    ) -> Self {
        Self {
            utc_time: Utc::now().to_rfc3339(),
            action: "ChatReadback",
            kind: if reply_bytes.is_some() {
                "stored_completion_matched"
            } else {
                "completion_unconfirmed"
            },
            model: context
                .model
                .and_then(|model| safe_model_label(model, credentials)),
            prompt_bytes: context.prompt_bytes,
            reply_bytes,
        }
    }
    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        write_record(self, path)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ServerDetails {
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub retry_after_seconds: Option<u64>,
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct StreamProgress {
    pub received_bytes: usize,
    pub received_events: usize,
    pub reply_bytes: usize,
    pub elapsed_ms: u64,
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct ResponseProgress {
    pub received_bytes: usize,
    pub elapsed_ms: u64,
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct RequestProgress {
    pub elapsed_ms: u64,
    pub timeout_ms: u64,
    pub connect_timeout_ms: u64,
    pub attempts: usize,
}

#[derive(Serialize)]
pub(crate) struct ConnectRetry {
    utc_time: String,
    action: String,
    kind: &'static str,
    model: Option<String>,
    prompt_bytes: Option<usize>,
    request: RequestProgress,
}
impl ConnectRetry {
    pub fn new(
        action: &str,
        context: RequestContext<'_>,
        request: RequestProgress,
        credentials: &Credentials,
    ) -> Self {
        Self {
            utc_time: Utc::now().to_rfc3339(),
            action: action.into(),
            kind: "connect_retry_before_send",
            model: context.model.and_then(|v| safe_model_label(v, credentials)),
            prompt_bytes: context.prompt_bytes,
            request,
        }
    }
    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        write_record(self, path)
    }
}

fn safe_identifier(value: &str, credentials: &Credentials) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        || [
            &credentials.access_key,
            &credentials.secret,
            &credentials.account_id,
        ]
        .iter()
        .any(|secret| !secret.is_empty() && value.contains(secret.as_str()))
    {
        return None;
    }
    Some(value.to_string())
}

fn safe_model_label(value: &str, credentials: &Credentials) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || [
            &credentials.access_key,
            &credentials.secret,
            &credentials.account_id,
        ]
        .iter()
        .any(|secret| !secret.is_empty() && value.contains(secret.as_str()))
    {
        return None;
    }
    Some(value.to_string())
}

impl ServerDetails {
    pub fn from_headers(headers: &HeaderMap, credentials: &Credentials) -> Self {
        let request_id = [
            "x-request-id",
            "x-correlation-id",
            "x-amzn-requestid",
            "x-trace-id",
        ]
        .iter()
        .find_map(|name| {
            headers
                .get(*name)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| safe_identifier(v, credentials))
        });
        let retry_after_seconds = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|seconds| *seconds <= 86_400);
        Self {
            request_id,
            retry_after_seconds,
            code: None,
        }
    }

    pub fn include_json(&mut self, value: &Value, credentials: &Credentials) {
        self.code = ["/ResponseMetadata/Error/Code", "/error/code"]
            .iter()
            .find_map(|pointer| {
                value
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .and_then(|v| safe_identifier(v, credentials))
            });
        if self.request_id.is_none() {
            self.request_id = [
                "/ResponseMetadata/RequestId",
                "/ResponseMetadata/RequestID",
                "/request_id",
            ]
            .iter()
            .find_map(|pointer| {
                value
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .and_then(|v| safe_identifier(v, credentials))
            });
        }
    }
}

#[derive(Serialize)]
pub(crate) struct RequestFailure {
    pub utc_time: String,
    pub action: String,
    pub kind: &'static str,
    pub http_status: Option<u16>,
    pub model: Option<String>,
    pub prompt_bytes: Option<usize>,
    pub request_bytes: usize,
    pub upstream_code: Option<String>,
    pub request_id: Option<String>,
    pub retry_after_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseProgress>,
}

impl RequestFailure {
    pub fn new(
        action: &str,
        kind: &'static str,
        status: Option<u16>,
        context: RequestContext<'_>,
        request_bytes: usize,
        details: ServerDetails,
        credentials: &Credentials,
    ) -> Self {
        Self {
            utc_time: Utc::now().to_rfc3339(),
            action: action.to_string(),
            kind,
            http_status: status,
            model: context.model.and_then(|v| safe_model_label(v, credentials)),
            prompt_bytes: context.prompt_bytes,
            request_bytes,
            upstream_code: details.code,
            request_id: details.request_id,
            retry_after_seconds: details.retry_after_seconds,
            request: None,
            stream: None,
            response: None,
        }
    }

    pub fn with_stream(mut self, progress: StreamProgress) -> Self {
        self.stream = Some(progress);
        self
    }

    pub fn with_response(mut self, progress: ResponseProgress) -> Self {
        self.response = Some(progress);
        self
    }

    pub fn with_request(mut self, progress: RequestProgress) -> Self {
        self.request = Some(progress);
        self
    }

    fn advice(&self) -> String {
        if self.kind.starts_with("response_") {
            return match self.kind {
                "response_timeout" => "已收到接口响应头，但响应体读取超时，结果未确认".into(),
                "response_network" => "接口响应体读取中断，结果未确认".into(),
                "response_limit" => "接口响应体超过客户端大小限制，结果未确认".into(),
                "response_empty" => "接口响应体为空，结果未确认".into(),
                "response_json" => "接口响应体不是完整有效的 JSON，结果未确认".into(),
                "response_envelope" => "接口响应缺少有效的 Result，结果未确认".into(),
                _ => "接口响应体异常，结果未确认".into(),
            };
        }
        if self.kind.starts_with("stream_") {
            return match self.kind {
                "stream_timeout" => "模型回复流读取超时，结果未确认".into(),
                "stream_network" => "模型回复流连接中断，结果未确认".into(),
                "stream_limit" => "模型回复流超过客户端大小限制，结果未确认".into(),
                "stream_utf8" => "模型回复流包含无效 UTF-8 编码，结果未确认".into(),
                "stream_parse" => "模型回复流不符合 SSE 格式，结果未确认".into(),
                "stream_event" => "模型回复事件内容无效或模型未正常结束，结果未确认".into(),
                "stream_content_type" => "Chat 响应不是 SSE 事件流，结果未确认".into(),
                "stream_empty" => "模型回复流未提供有效文本，结果未确认".into(),
                "stream_unfinished" => {
                    "模型回复流缺少结束确认，存储记录也未核对通过，结果未确认".into()
                }
                _ => "模型回复流异常，结果未确认".into(),
            };
        }
        if self.kind == "api" {
            return "平台返回业务错误，请结合错误码检查模型授权和服务状态".into();
        }
        match self.http_status {
            Some(401) => "登录凭据无效或已过期，请运行 xjcode login --force 重新登录".into(),
            Some(403) => "当前账号、工作区或模型权限不足，请检查授权".into(),
            Some(413) => "请求内容过大，请压缩会话或减少工具输出后再试".into(),
            Some(429) => match self.retry_after_seconds {
                Some(seconds) => format!("请求受到限流，服务端建议至少等待 {seconds} 秒后再试"),
                None => "请求受到限流，请稍后再试".into(),
            },
            Some(500..=599) => "模型服务返回异常，不能据此判断登录失效。可先运行 xjcode doctor --probe 检查短调用；若短调用正常而原会话失败，再压缩会话或手动切换授权模型".into(),
            Some(300..=399) => "服务返回重定向，客户端未跟随，请核对校园服务入口".into(),
            Some(_) => "请求被服务端拒绝，请结合错误码核对模型和请求内容".into(),
            None if self.kind == "timeout" && self.action == "BatchCreateMessages" => "写入上下文消息超时，尚未取得写入确认；本次尚未发起 Chat，不能据此判断模型能力或上下文已满。消息可能已被平台接收，请勿自动重发".into(),
            None if self.kind == "timeout" => "发送请求或等待响应头超时，结果未确认，请检查校园网络和模型服务".into(),
            None if self.kind == "connect_timeout" => "连接校园服务超时，连接未建立，HTTP 请求尚未发送；请检查校园网络或学校 VPN".into(),
            None if self.kind == "connect" => "未能建立校园服务连接，HTTP 请求尚未发送；请检查校园网络或学校 VPN".into(),
            None => "网络请求失败，结果未确认，请检查校园网络和模型服务".into(),
        }
    }

    pub fn error(&self, log_path: Option<&Path>) -> anyhow::Error {
        let status = self
            .http_status
            .map(|s| format!("HTTP {s}"))
            .unwrap_or_else(|| self.kind.into());
        let mut diagnostic = vec![format!("时间={}", self.utc_time)];
        diagnostic.push(format!("类型={}", self.kind));
        if let Some(model) = &self.model {
            diagnostic.push(format!("模型={model}"));
        }
        if let Some(bytes) = self.prompt_bytes {
            diagnostic.push(format!("输入={bytes} UTF-8字节"));
        }
        diagnostic.push(format!("请求体={} 字节", self.request_bytes));
        if let Some(request) = &self.request {
            diagnostic.push(format!(
                "请求耗时={}ms；时限={}ms；连接时限={}ms；建连/发送尝试={}",
                request.elapsed_ms,
                request.timeout_ms,
                request.connect_timeout_ms,
                request.attempts
            ));
        }
        if let Some(response) = &self.response {
            diagnostic.push(format!(
                "响应={} 字节；耗时={}ms",
                response.received_bytes, response.elapsed_ms
            ));
        }
        if let Some(stream) = &self.stream {
            diagnostic.push(format!(
                "流={} 字节/{}事件；回复={} 字节；耗时={}ms",
                stream.received_bytes,
                stream.received_events,
                stream.reply_bytes,
                stream.elapsed_ms
            ));
        }
        if let Some(code) = &self.upstream_code {
            diagnostic.push(format!("错误码={code}"));
        }
        if let Some(id) = &self.request_id {
            diagnostic.push(format!("请求ID={id}"));
        }
        let log_note = log_path
            .filter(|path| self.write_to(path).is_ok())
            .map(|path| format!("\n诊断记录：{}", path.display()))
            .unwrap_or_default();
        anyhow::anyhow!(
            "交晓智 {}: {}；{}。未自动重发已发送或结果不明的请求\n诊断：{}{}",
            self.action,
            status,
            self.advice(),
            diagnostic.join("；"),
            log_note
        )
    }

    fn write_to(&self, path: &Path) -> std::io::Result<()> {
        write_record(self, path)
    }
}

fn write_record(value: &impl Serialize, path: &Path) -> std::io::Result<()> {
    let _guard = LOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("invalid diagnostic path"))?;
    fs::create_dir_all(parent)?;
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    if fs::metadata(path).is_ok_and(|m| m.len() + line.len() as u64 > MAX_LOG_BYTES) {
        let previous = path.with_extension("previous.jsonl");
        match fs::remove_file(&previous) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(path, previous)?;
    }
    let mut log = OpenOptions::new().create(true).append(true).open(path)?;
    log.write_all(&line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn credentials() -> Credentials {
        Credentials {
            access_key: "fixture-access".into(),
            secret: "fixture-secret".into(),
            account_id: "fixture-account".into(),
        }
    }
    fn failure(status: u16) -> RequestFailure {
        RequestFailure::new(
            "Chat",
            "http",
            Some(status),
            RequestContext {
                model: Some("qwen-test"),
                prompt_bytes: Some(12345),
            },
            78,
            ServerDetails::default(),
            &credentials(),
        )
    }
    #[test]
    fn server_error_does_not_suggest_credential_or_permission_failures() {
        for status in [500, 502, 503, 504] {
            let text = failure(status).error(None).to_string();
            assert!(text.contains(&format!("HTTP {status}")));
            assert!(text.contains("doctor --probe"));
            assert!(text.contains("输入=12345"));
            let advice = text.lines().next().unwrap();
            for misleading in ["401", "403", "429", "login --force"] {
                assert!(!advice.contains(misleading));
            }
        }
    }
    #[test]
    fn status_advice_and_retry_after_are_specific() {
        assert!(
            failure(401)
                .error(None)
                .to_string()
                .contains("login --force")
        );
        assert!(failure(403).error(None).to_string().contains("权限不足"));
        assert!(
            failure(413)
                .error(None)
                .to_string()
                .contains("请求内容过大")
        );
        let mut f = failure(429);
        f.retry_after_seconds = Some(12);
        assert!(f.error(None).to_string().contains("12 秒"));
        assert!(!f.error(None).to_string().contains("login"));
    }
    #[test]
    fn only_allowlisted_metadata_is_kept_and_credentials_are_rejected() {
        let c = credentials();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer fixture-secret".parse().unwrap());
        headers.insert("x-request-id", "fixture-secret".parse().unwrap());
        headers.insert("retry-after", "15".parse().unwrap());
        let mut details = ServerDetails::from_headers(&headers, &c);
        details.include_json(&json!({"ResponseMetadata":{"RequestId":"safe-request-123", "Error":{"Code":"ModelBusy", "Message":"fixture-secret private prompt"}}, "prompt":"private prompt"}), &c);
        let f = RequestFailure::new(
            "Chat",
            "http",
            Some(500),
            RequestContext::default(),
            16,
            details,
            &c,
        );
        let text = format!("{} {}", f.error(None), serde_json::to_string(&f).unwrap());
        assert!(text.contains("safe-request-123"));
        assert!(text.contains("ModelBusy"));
        assert!(!text.contains("fixture-secret"));
        assert!(!text.contains("private prompt"));
        assert!(!text.contains("authorization"));
    }
    #[test]
    fn rejects_control_characters_and_unbounded_identifiers() {
        let c = credentials();
        for value in [
            "bad\nrequest",
            "<html>error</html>",
            "Bearer token",
            &"a".repeat(129),
        ] {
            assert!(safe_identifier(value, &c).is_none());
        }
        assert_eq!(
            safe_identifier("request-123:abcd", &c).as_deref(),
            Some("request-123:abcd")
        );
        assert_eq!(
            safe_model_label("交晓智 / Qwen", &c).as_deref(),
            Some("交晓智 / Qwen")
        );
        assert!(safe_model_label("model\u{1b}[31m", &c).is_none());
        assert!(safe_model_label("model-fixture-secret", &c).is_none());
    }
    #[test]
    fn diagnostic_log_is_jsonl_and_rotates_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics/requests.jsonl");
        failure(500).write_to(&path).unwrap();
        failure(429).write_to(&path).unwrap();
        let lines = fs::read_to_string(&path).unwrap();
        assert_eq!(lines.lines().count(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(lines.lines().next().unwrap()).unwrap()["prompt_bytes"],
            12345
        );
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_LOG_BYTES)
            .unwrap();
        failure(503).write_to(&path).unwrap();
        assert!(path.with_extension("previous.jsonl").exists());
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn protocol_diagnostics_record_positions_and_attempts_without_reply_or_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics/requests.jsonl");
        let raw = r#"{"type":"final","text":"fixture-secret private reply""#;
        let error = crate::protocol::AgentReply::parse(raw, &[]).unwrap_err();
        ProtocolFailure::new("qwen-test", 800, raw.len(), 2, &error, &credentials())
            .write_to(&path)
            .unwrap();
        let unknown = anyhow::anyhow!("fixture-secret private validation error");
        ProtocolFailure::new(
            "model-fixture-secret",
            900,
            123,
            3,
            &unknown,
            &credentials(),
        )
        .write_to(&path)
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let records = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records[0]["action"], "ReplyProtocol");
        assert_eq!(records[0]["kind"], "protocol_eof");
        assert_eq!(records[0]["attempt"], 2);
        assert_eq!(records[0]["line"], 1);
        assert!(records[0]["column"].as_u64().unwrap() > 0);
        assert_eq!(records[1]["kind"], "protocol_validation");
        assert!(records[1]["model"].is_null());
        assert!(records[1]["line"].is_null());
        for private in [
            "fixture-secret",
            "private reply",
            "private validation",
            "failed_reply",
        ] {
            assert!(!text.contains(private));
        }
    }
}
