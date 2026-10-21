use crate::auth::Credentials;
use crate::diagnostics::{
    ChatReadback, ConnectRetry, ProtocolFailure, RequestContext, RequestFailure, RequestProgress,
    ResponseProgress, ServerDetails, StreamProgress,
};
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use reqwest::{Client, Response, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const ENDPOINT: &str = "http://ai.xjtu.edu.cn:30040/";
const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_STREAM: usize = 8 * 1024 * 1024;
const MAX_ERROR_JSON: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);

// Persisting the complete context is a write, not a small catalog/identity read.
// Give that one operation more time, without replaying an uncertain write.
#[derive(Clone, Copy)]
struct RequestTimeouts {
    control: Duration,
    message_write: Duration,
    chat: Duration,
}
impl Default for RequestTimeouts {
    fn default() -> Self {
        Self {
            control: Duration::from_secs(20),
            message_write: Duration::from_secs(60),
            chat: Duration::from_secs(180),
        }
    }
}
impl RequestTimeouts {
    fn for_request(self, service: &str, action: &str, stream: bool) -> Duration {
        if stream {
            self.chat
        } else if service == "aigw" && action == "BatchCreateMessages" {
            self.message_write
        } else {
            self.control
        }
    }
}

enum StreamReadFailure {
    Network(reqwest::Error),
    Limit,
}

// Keep transport eligibility typed. Never infer it from upstream text, and never
// preserve reqwest's raw display string (it can contain a signed request URL).
#[derive(Debug)]
struct RequestTransportError(anyhow::Error);
impl std::fmt::Display for RequestTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
impl std::error::Error for RequestTransportError {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub service_id: String,
    pub upstream_name: String,
    pub source_type: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub verified_image_input: bool,
}
impl Model {
    pub fn supports_images(&self) -> bool {
        self.verified_image_input || self.platform_supports_images()
    }
    pub fn platform_supports_images(&self) -> bool {
        self.features.iter().any(|feature| feature == "vision")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
}

fn mac(key: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key size");
    m.update(bytes);
    m.finalize().into_bytes().to_vec()
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub fn signed_headers(
    c: &Credentials,
    service: &str,
    host: &str,
    query: &str,
    body: &[u8],
    stamp: &str,
) -> Result<HeaderMap> {
    ensure!(
        stamp.len() == 16 && stamp.is_ascii(),
        "Invalid signing timestamp"
    );
    let date = &stamp[..8];
    let body_hash = hex::encode(Sha256::digest(body));
    let mut headers = BTreeMap::from([
        ("content-type", "application/json".to_string()),
        ("host", host.to_string()),
        ("x-content-sha256", body_hash.clone()),
        ("x-date", stamp.to_string()),
        ("x-top-service", service.to_string()),
    ]);
    let canonical_headers = headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();
    let names = headers.keys().copied().collect::<Vec<_>>().join(";");
    let canonical = format!("POST\n/\n{query}\n{canonical_headers}\n{names}\n{body_hash}");
    let scope = format!("{date}/cn-north-1/{service}/request");
    let signing = format!(
        "HMAC-SHA256\n{stamp}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let key = mac(
        &mac(
            &mac(&mac(c.secret.as_bytes(), date.as_bytes()), b"cn-north-1"),
            service.as_bytes(),
        ),
        b"request",
    );
    headers.insert(
        "authorization",
        format!(
            "HMAC-SHA256 Credential={}/{scope}, SignedHeaders={names}, Signature={}",
            c.access_key,
            hex::encode(mac(&key, signing.as_bytes()))
        ),
    );
    let mut result = HeaderMap::new();
    for (k, v) in headers {
        result.insert(reqwest::header::HeaderName::from_static(k), v.parse()?);
    }
    Ok(result)
}

pub struct TopClient {
    client: Client,
    credentials: Credentials,
    endpoint: String,
    diagnostics_path: Option<PathBuf>,
    timeouts: RequestTimeouts,
}
impl TopClient {
    pub fn new(credentials: Credentials) -> Result<Self> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(180))
            .build()?;
        Ok(Self {
            client,
            credentials,
            timeouts: RequestTimeouts::default(),
            endpoint: ENDPOINT.into(),
            diagnostics_path: crate::auth::home()
                .ok()
                .map(|home| home.join("diagnostics").join("requests.jsonl")),
        })
    }
    fn failure(
        &self,
        action: &str,
        kind: &'static str,
        status: Option<u16>,
        context: RequestContext<'_>,
        request_bytes: usize,
        details: ServerDetails,
        progress: RequestProgress,
    ) -> anyhow::Error {
        RequestFailure::new(
            action,
            kind,
            status,
            context,
            request_bytes,
            details,
            &self.credentials,
        )
        .with_request(progress)
        .error(self.diagnostics_path.as_deref())
    }
    async fn request(
        &self,
        service: &str,
        action: &str,
        payload: Value,
        stream: bool,
        context: RequestContext<'_>,
    ) -> Result<Response> {
        let version = if service == "iam" {
            "2024-12-25"
        } else {
            "2023-08-01"
        };
        let mut params = BTreeMap::from([("Action", action), ("Version", version)]);
        if service != "iam" {
            params.insert("X-Account-Id", &self.credentials.account_id);
        }
        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let body = serde_json::to_vec(&payload)?;
        let request_bytes = body.len();
        let timeout = self.timeouts.for_request(service, action, stream);
        let started = Instant::now();
        let progress = |attempts| RequestProgress {
            elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            timeout_ms: timeout.as_millis().try_into().unwrap_or(u64::MAX),
            connect_timeout_ms: CONNECT_TIMEOUT.as_millis() as u64,
            attempts,
        };
        let mut attempt = 0;
        let mut response = loop {
            attempt += 1;
            // A fresh signature and the remaining original budget for each dial.
            // reqwest's typed is_connect flag denotes connector failure, before
            // the HTTP request is dispatched. Never retry a sent/uncertain write.
            let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let mut headers = signed_headers(
                &self.credentials,
                service,
                "ai.xjtu.edu.cn:30040",
                &query,
                &body,
                &stamp,
            )?;
            if stream {
                headers.insert("accept", "text/event-stream".parse()?);
            }
            let remaining = timeout
                .saturating_sub(started.elapsed())
                .max(Duration::from_millis(1));
            match self
                .client
                .post(format!("{}?{query}", self.endpoint))
                .headers(headers)
                .body(body.clone())
                .timeout(remaining)
                .send()
                .await
            {
                Ok(response) => break response,
                Err(error) => {
                    if error.is_connect()
                        && attempt == 1
                        && timeout.saturating_sub(started.elapsed()) > CONNECT_RETRY_DELAY
                    {
                        if let Some(path) = &self.diagnostics_path {
                            let _ = ConnectRetry::new(
                                action,
                                context,
                                progress(attempt),
                                &self.credentials,
                            )
                            .write_to(path);
                        }
                        tokio::time::sleep(CONNECT_RETRY_DELAY).await;
                        continue;
                    }
                    let kind = match (error.is_connect(), error.is_timeout()) {
                        (true, true) => "connect_timeout",
                        (true, false) => "connect",
                        (false, true) => "timeout",
                        _ => "network",
                    };
                    return Err(anyhow::Error::new(RequestTransportError(self.failure(
                        action,
                        kind,
                        None,
                        context,
                        request_bytes,
                        ServerDetails::default(),
                        progress(attempt),
                    ))));
                }
            }
        };
        let status = response.status();
        if !status.is_success() {
            let mut details = ServerDetails::from_headers(response.headers(), &self.credentials);
            // Read only a bounded JSON error envelope, and retain only allowlisted identifiers.
            // Never display or log the upstream message/body, which can echo private input.
            let mut error_json = Vec::new();
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                while let Ok(Some(chunk)) = response.chunk().await {
                    if error_json.len() + chunk.len() > MAX_ERROR_JSON {
                        break;
                    }
                    error_json.extend(chunk);
                }
            })
            .await;
            if let Ok(value) = serde_json::from_slice::<Value>(&error_json) {
                details.include_json(&value, &self.credentials);
            }
            return Err(self.failure(
                action,
                "http",
                Some(status.as_u16()),
                context,
                request_bytes,
                details,
                progress(attempt),
            ));
        }
        Ok(response)
    }
    pub async fn call(&self, service: &str, action: &str, payload: Value) -> Result<Value> {
        self.call_with_context(service, action, payload, RequestContext::default())
            .await
    }
    async fn call_with_context(
        &self,
        service: &str,
        action: &str,
        payload: Value,
        context: RequestContext<'_>,
    ) -> Result<Value> {
        let request_bytes = serde_json::to_vec(&payload)?.len();
        let response = self
            .request(service, action, payload, false, context)
            .await?;
        self.read_json_response(action, context, request_bytes, response)
            .await
    }

    async fn read_json_response(
        &self,
        action: &str,
        context: RequestContext<'_>,
        request_bytes: usize,
        mut response: Response,
    ) -> Result<Value> {
        let status = response.status().as_u16();
        let mut details = ServerDetails::from_headers(response.headers(), &self.credentials);
        let started = Instant::now();
        let failure = |kind, received_bytes, details: ServerDetails| {
            RequestFailure::new(
                action,
                kind,
                Some(status),
                context,
                request_bytes,
                details,
                &self.credentials,
            )
            .with_response(ResponseProgress {
                received_bytes,
                elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            })
            .error(self.diagnostics_path.as_deref())
        };
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            failure(
                if error.is_timeout() {
                    "response_timeout"
                } else {
                    "response_network"
                },
                data.len(),
                details.clone(),
            )
        })? {
            if data.len() + chunk.len() > MAX_JSON {
                return Err(failure("response_limit", data.len(), details));
            }
            data.extend(chunk);
        }
        if data.is_empty() {
            return Err(failure("response_empty", 0, details));
        }
        let value: Value = serde_json::from_slice(&data)
            .map_err(|_| failure("response_json", data.len(), details.clone()))?;
        details.include_json(&value, &self.credentials);
        if value
            .pointer("/ResponseMetadata/Error")
            .filter(|v| !v.is_null())
            .is_some()
        {
            return Err(failure("api", data.len(), details));
        }
        value
            .get("Result")
            .filter(|value| !value.is_null())
            .cloned()
            .ok_or_else(|| failure("response_envelope", data.len(), details))
    }
    pub async fn identity(&self) -> Result<String> {
        let r = self.call("iam", "GetSelfInfo", json!({})).await?;
        field(&r["User"], "ID")
    }
    pub async fn workspaces(&self) -> Result<Vec<Workspace>> {
        let mut all = Vec::new();
        for page in 1..=100 {
            let r = self
                .call(
                    "iam",
                    "ListWorkspace",
                    json!({"ListOpt":{"PageNumber":page,"PageSize":100},"Filter":{}}),
                )
                .await?;
            let items = r["Items"].as_array().context("工作区目录格式错误")?;
            for v in items {
                all.push(Workspace {
                    id: field(v, "ID")?,
                    name: v["Name"].as_str().unwrap_or("未命名工作区").into(),
                });
            }
            if items.is_empty()
                || all.len() >= r["Total"].as_u64().unwrap_or(all.len() as u64) as usize
            {
                return Ok(all);
            }
        }
        bail!("工作区分页超出限制")
    }
    pub async fn models(&self, workspace: &str) -> Result<Vec<Model>> {
        let r = self
            .call(
                "aigw",
                "ListModelByWorkspaceGrant",
                json!({"WorkspaceID":workspace}),
            )
            .await?;
        let mut models = Vec::new();
        for v in r["Items"].as_array().context("模型目录格式错误")? {
            if v["Type"] != "text-generation"
                || v["Status"] != "Running"
                || v["Granted"] != true
                || v["IsPublished"] != true
            {
                continue;
            }
            let m = Model {
                name: field(v, "Name")?,
                service_id: field(v, "ID")?,
                upstream_name: field(v, "ModelName")?,
                source_type: field(v, "PublishSourceType")?,
                features: v["FeaturesConfig"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                verified_image_input: false,
            };
            ensure!(
                !models.iter().any(|x: &Model| x.name == m.name),
                "模型目录存在重名，无法安全选择"
            );
            models.push(m);
        }
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models)
    }
    pub async fn complete(&self, workspace: &str, model: &Model, prompt: &str) -> Result<String> {
        self.complete_with_images(workspace, model, prompt, &[])
            .await
    }

    /// Persist only parse categories, positions and sizes, never model replies.
    pub fn record_protocol_failure(
        &self,
        model: &Model,
        prompt_bytes: usize,
        reply_bytes: usize,
        attempt: usize,
        error: &anyhow::Error,
    ) -> Option<PathBuf> {
        let path = self.diagnostics_path.as_ref()?;
        ProtocolFailure::new(
            &model.name,
            prompt_bytes,
            reply_bytes,
            attempt,
            error,
            &self.credentials,
        )
        .write_to(path)
        .ok()?;
        Some(path.clone())
    }

    pub async fn complete_with_images(
        &self,
        workspace: &str,
        model: &Model,
        prompt: &str,
        images: &[crate::images::InputImage],
    ) -> Result<String> {
        crate::protocol::ensure_request_size(prompt)?;
        crate::images::validate(images)?;
        ensure!(
            images.is_empty() || model.supports_images(),
            "当前交晓智模型未标记为支持识图；请用 /model 选择支持图片的交晓智授权模型。图片未发送，不会降级为文字路径。"
        );
        let context = RequestContext {
            model: Some(&model.name),
            prompt_bytes: Some(prompt.len()),
        };
        let r = self.call_with_context("aigw","CreateConversation",json!({"ConversationName":"XJCode","WorkspaceID":workspace,"Models":[{"ModelServiceID":model.service_id,"ModelName":model.upstream_name,"PublishSourceType":model.source_type}]}), context).await?;
        let info = &r["ConversationInfo"];
        let conversation = field(info, "ConversationID")?;
        let session = field(&info["ProjectList"][0], "SessionID")?;
        let mut input = json!({"SessionID":session,"ContentType":"text","Content":prompt});
        if !images.is_empty() {
            input["ExtendsInfo"] = json!({"Files":images.iter().enumerate().map(|(index,image)|json!({"Url":image.data_url(),"Type":"image","Name":format!("image-{}.{}",index+1,image.extension()),"Size":image.size()})).collect::<Vec<_>>()});
        }
        let r = self.call_with_context("aigw","BatchCreateMessages",json!({"ConversationID":conversation,"WorkspaceID":workspace,"MessageList":[input]}), context).await?;
        let message = field(&r["MessageList"][0], "MessageID")?;
        let payload = json!({"SessionID":session,"MessageID":message,"WorkspaceID":workspace});
        let request_bytes = serde_json::to_vec(&payload)?.len();
        let started = Instant::now();
        let response = match self.request("aigw", "Chat", payload, true, context).await {
            Ok(response) => response,
            Err(error) if error.downcast_ref::<RequestTransportError>().is_some() => {
                // Chat may already have completed even though its response headers
                // never reached us. One readback of this exact new conversation
                // can confirm that result; do not send Chat or its input again.
                let stored = self
                    .call_with_context(
                        "aigw",
                        "GetConversationMessages",
                        json!({"ConversationID":conversation,"WorkspaceID":workspace}),
                        context,
                    )
                    .await;
                let recovered = stored.as_ref().ok().and_then(|stored| {
                    confirmed_stored_reply(stored, workspace, &session, &message, prompt)
                });
                if let Some(path) = &self.diagnostics_path {
                    let _ = ChatReadback::new(context, recovered.map(str::len), &self.credentials)
                        .write_to(path);
                }
                if let Some(text) = recovered {
                    // Return only text. The core still performs protocol, tool
                    // registry/schema, patch grammar and outcome validation.
                    return Ok(text.to_owned());
                }
                return Err(anyhow::anyhow!(
                    "{error}\n已只读回查原会话，未能确认匹配且正常结束的完整回复；未重发 Chat，该轮工具未执行。继续前先检查已有文件和后台作业。"
                ));
            }
            Err(error) => return Err(error),
        };
        let status = response.status().as_u16();
        let details = ServerDetails::from_headers(response.headers(), &self.credentials);
        let stream_failure = |kind, received_bytes, received_events, reply_bytes| {
            RequestFailure::new(
                "Chat",
                kind,
                Some(status),
                context,
                request_bytes,
                details.clone(),
                &self.credentials,
            )
            .with_stream(StreamProgress {
                received_bytes,
                received_events,
                reply_bytes,
                elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            })
            .error(self.diagnostics_path.as_deref())
        };
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("text/event-stream")
        {
            return Err(stream_failure("stream_content_type", 0, 0, 0));
        }
        let mut received_bytes = 0usize;
        let mut stream = response
            .bytes_stream()
            .map(|chunk| match chunk {
                Ok(bytes) => {
                    received_bytes = received_bytes.saturating_add(bytes.len());
                    if received_bytes > MAX_STREAM {
                        Err(StreamReadFailure::Limit)
                    } else {
                        Ok(bytes)
                    }
                }
                Err(error) => Err(StreamReadFailure::Network(error)),
            })
            .eventsource();
        let mut result = StreamAccumulator::default();
        let mut received_events = 0usize;
        let mut failure_kind = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    received_events += 1;
                    match result.push(&event.data) {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(_) => {
                            failure_kind = Some("stream_event");
                            break;
                        }
                    }
                }
                Err(error) => {
                    failure_kind = Some(match error {
                        EventStreamError::Utf8(_) => "stream_utf8",
                        EventStreamError::Parser(_) => "stream_parse",
                        EventStreamError::Transport(StreamReadFailure::Limit) => "stream_limit",
                        EventStreamError::Transport(StreamReadFailure::Network(error)) => {
                            if error.is_timeout() {
                                "stream_timeout"
                            } else {
                                "stream_network"
                            }
                        }
                    });
                    break;
                }
            }
        }
        // Drop the source stream before inspecting its byte counter. Never expose
        // parser/source error strings: they may contain private response content.
        drop(stream);
        if let Some(kind) = failure_kind {
            return Err(stream_failure(
                kind,
                received_bytes,
                received_events,
                result.text.len(),
            ));
        }
        if result.text.trim().is_empty() {
            return Err(stream_failure(
                "stream_empty",
                received_bytes,
                received_events,
                result.text.len(),
            ));
        }
        if !result.finished {
            let stored = self
                .call_with_context(
                    "aigw",
                    "GetConversationMessages",
                    json!({"ConversationID":conversation,"WorkspaceID":workspace}),
                    context,
                )
                .await?;
            if !stored_matches(&stored, &result.text) {
                return Err(stream_failure(
                    "stream_unfinished",
                    received_bytes,
                    received_events,
                    result.text.len(),
                ));
            }
        }
        Ok(result.text)
    }
}
fn field(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .map(str::to_string)
        .with_context(|| format!("响应缺少有效字段 {key}"))
}
#[derive(Default)]
pub struct StreamAccumulator {
    pub text: String,
    pub finished: bool,
}
impl StreamAccumulator {
    pub fn push(&mut self, data: &str) -> Result<bool> {
        if data.trim() == "[DONE]" {
            self.finished = true;
            return Ok(true);
        }
        let v: Value = serde_json::from_str(data).context("事件流 JSON 无效")?;
        ensure!(v.get("error").is_none(), "交晓智事件流报告错误");
        if let Some(choices) = v["choices"].as_array() {
            ensure!(choices.len() <= 1, "不支持多候选流");
            for c in choices {
                if let Some(t) = c.pointer("/delta/content").and_then(Value::as_str) {
                    self.text.push_str(t);
                }
                if let Some(reason) = c["finish_reason"].as_str() {
                    ensure!(reason == "stop", "模型未正常结束：截断或过滤");
                    self.finished = true;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
pub fn stored_matches(stored: &Value, text: &str) -> bool {
    stored
        .get("MessageList")
        .or_else(|| stored.get("Messages"))
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|v| {
                (v["AuthorRole"] == "1" || v["AuthorRole"] == 1)
                    && v["Content"]
                        .as_str()
                        .is_some_and(|s| s.trim() == text.trim())
            })
        })
}

/// Confirm a completed reply to the exact single input in a fresh conversation.
/// Fail closed on missing identity, old/duplicate messages or incomplete output.
pub fn confirmed_stored_reply<'a>(
    stored: &'a Value,
    workspace: &str,
    session: &str,
    message: &str,
    prompt: &str,
) -> Option<&'a str> {
    let items = stored.get("MessageList")?.as_array()?;
    if items.len() != 2 {
        return None;
    }
    let role = |item: &Value, expected: u64| {
        item["AuthorRole"].as_u64() == Some(expected)
            || item["AuthorRole"].as_str() == Some(if expected == 1 { "1" } else { "2" })
    };
    let user = items.iter().find(|item| role(item, 2))?;
    let assistant = items.iter().find(|item| role(item, 1))?;
    let query = user["QueryID"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 512)?;
    if [workspace, session, message]
        .iter()
        .any(|id| id.is_empty() || id.len() > 512)
        || user["WorkspaceID"] != workspace
        || assistant["WorkspaceID"] != workspace
        || user["SessionID"] != session
        || assistant["SessionID"] != session
        || user["MessageID"] != message
        || assistant["QueryID"] != query
        || user["Content"].as_str() != Some(prompt)
        || assistant.pointer("/TokenInfo/FinishReason")?.as_str() != Some("stop")
    {
        return None;
    }
    let start = assistant.pointer("/TokenInfo/StartTime")?.as_u64()?;
    let end = assistant.pointer("/TokenInfo/EndTime")?.as_u64()?;
    if start == 0 || end < start {
        return None;
    }
    assistant["Content"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= MAX_STREAM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Instant,
    };

    fn mock_server(status: u16, headers: &str, body: &str) -> (String, thread::JoinHandle<usize>) {
        mock_json_server(status, headers, body, body.len())
    }

    fn mock_json_server(
        status: u16,
        headers: &str,
        body: &str,
        declared_length: usize,
    ) -> (String, thread::JoinHandle<usize>) {
        mock_json_server_after(status, headers, body, declared_length, Duration::ZERO)
    }

    fn mock_json_server_after(
        status: u16,
        headers: &str,
        body: &str,
        declared_length: usize,
        delay: Duration,
    ) -> (String, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let reply = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
            declared_length
        );
        let server = thread::spawn(move || {
            let mut deadline = Instant::now() + Duration::from_secs(4);
            let mut requests = 0;
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        requests += 1;
                        // Windows accepted sockets can inherit nonblocking mode.
                        // The request reader below is deliberately blocking.
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        // Read the complete request before closing the socket.
                        // Closing with unread body bytes can reset the connection
                        // and turn this HTTP-status test into a network-error race.
                        let mut request = Vec::new();
                        loop {
                            let mut buffer = [0u8; 8192];
                            let count = socket.read(&mut buffer).unwrap();
                            assert_ne!(count, 0);
                            request.extend_from_slice(&buffer[..count]);
                            if let Some(end) =
                                request.windows(4).position(|part| part == b"\r\n\r\n")
                            {
                                let header = String::from_utf8_lossy(&request[..end]);
                                let length = header
                                    .lines()
                                    .find_map(|line| {
                                        let (name, value) = line.split_once(':')?;
                                        name.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse::<usize>().unwrap())
                                    })
                                    .unwrap_or(0);
                                if request.len() >= end + 4 + length {
                                    break;
                                }
                            }
                        }
                        thread::sleep(delay);
                        let _ = socket.write_all(reply.as_bytes());
                        deadline = Instant::now() + Duration::from_millis(400);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("mock accept failed: {e}"),
                }
            }
            requests
        });
        (endpoint, server)
    }

    fn mock_client(endpoint: String, log: PathBuf) -> TopClient {
        TopClient {
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            credentials: Credentials {
                access_key: "fixture-access".into(),
                secret: "fixture-secret".into(),
                account_id: "fixture-account".into(),
            },
            endpoint,
            diagnostics_path: Some(log),
            timeouts: RequestTimeouts::default(),
        }
    }

    #[test]
    fn larger_wait_budget_is_limited_to_the_message_write() {
        let timeouts = RequestTimeouts::default();
        assert_eq!(
            timeouts.for_request("aigw", "BatchCreateMessages", false),
            Duration::from_secs(60)
        );
        assert_eq!(
            timeouts.for_request("aigw", "CreateConversation", false),
            Duration::from_secs(20)
        );
        assert_eq!(
            timeouts.for_request("iam", "GetSelfInfo", false),
            Duration::from_secs(20)
        );
        assert_eq!(
            timeouts.for_request("aigw", "Chat", true),
            Duration::from_secs(180)
        );
    }

    #[tokio::test]
    async fn message_write_can_wait_longer_than_a_control_read_without_replay() {
        let body = json!({"Result":{"MessageList":[{"MessageID":"created"}]}}).to_string();
        let (endpoint, server) =
            mock_json_server_after(200, "", &body, body.len(), Duration::from_millis(200));
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let mut client = mock_client(endpoint, log.clone());
        client.timeouts.control = Duration::from_millis(50);
        client.timeouts.message_write = Duration::from_secs(2);
        let result = client
            .call(
                "aigw",
                "BatchCreateMessages",
                json!({"Content":"private prompt"}),
            )
            .await
            .unwrap();
        assert_eq!(result["MessageList"][0]["MessageID"], "created");
        assert_eq!(server.join().unwrap(), 1);
        assert!(
            !log.exists(),
            "successful slow write was recorded as a failure"
        );
    }

    #[tokio::test]
    async fn uncertain_message_timeout_records_wait_budget_without_replay_or_private_data() {
        let body = json!({"Result":{"MessageList":[{"MessageID":"late"}]}}).to_string();
        let (endpoint, server) =
            mock_json_server_after(200, "", &body, body.len(), Duration::from_millis(500));
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let mut client = mock_client(endpoint, log.clone());
        client.timeouts.message_write = Duration::from_millis(200);
        let error = client
            .call_with_context(
                "aigw",
                "BatchCreateMessages",
                json!({"Content":"fixture-secret private prompt"}),
                RequestContext {
                    model: Some("fixture-model"),
                    prompt_bytes: Some(45844),
                },
            )
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(server.join().unwrap(), 1, "uncertain write was replayed");
        let raw = fs_read(&log);
        let entry: Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(entry["kind"], "timeout");
        assert_eq!(entry["http_status"], Value::Null);
        assert_eq!(entry["request"]["timeout_ms"], 200);
        assert!(entry["request"]["elapsed_ms"].as_u64().unwrap() >= 150);
        assert!(error.contains("写入上下文消息超时") && error.contains("未自动重发"));
        assert!(error.contains("请求耗时=") && error.contains("时限=200ms"));
        assert!(!error.contains("login --force"));
        for secret in ["fixture-secret", "private prompt", "late"] {
            assert!(!format!("{error}{raw}").contains(secret));
        }
    }

    #[tokio::test]
    async fn http_failure_keeps_safe_context_and_is_not_replayed() {
        let body = json!({"ResponseMetadata":{"RequestId":"request-body-456", "Error":{"Code":"ModelBusy", "Message":"fixture-secret private task content"}}}).to_string();
        let (endpoint, server) = mock_server(500, "x-request-id: request-header-123\r\n", &body);
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        let context = RequestContext {
            model: Some("fixture-model"),
            prompt_bytes: Some(80_000),
        };
        let error = client
            .call_with_context(
                "aigw",
                "Chat",
                json!({"content":"private task content"}),
                context,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 500"), "{error}");
        assert!(error.contains("request-header-123"));
        assert!(error.contains("输入=80000"));
        assert!(!error.contains("login --force"));
        assert!(!error.contains("fixture-secret"));
        assert!(!error.contains("private task content"));
        assert_eq!(
            server.join().unwrap(),
            1,
            "uncertain Chat request was replayed"
        );
        let record: Value = serde_json::from_str(fs_read(&log).trim()).unwrap();
        assert_eq!(record["http_status"], 500);
        assert_eq!(record["model"], "fixture-model");
        assert_eq!(record["prompt_bytes"], 80_000);
    }

    fn fs_read(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[tokio::test]
    async fn connector_recovery_sends_one_post_when_the_listener_becomes_available() {
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let signal = log.clone();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(4);
            while !signal.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(signal.exists(), "first connector failure was not observed");
            let listener = TcpListener::bind(address).unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut requests = 0;
            let mut deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        requests += 1;
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = Vec::new();
                        loop {
                            let mut buffer = [0u8; 8192];
                            let n = socket.read(&mut buffer).unwrap();
                            assert!(n > 0);
                            request.extend_from_slice(&buffer[..n]);
                            if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                                let length = String::from_utf8_lossy(&request[..end])
                                    .lines()
                                    .find_map(|line| {
                                        let (n, v) = line.split_once(':')?;
                                        n.eq_ignore_ascii_case("content-length")
                                            .then(|| v.trim().parse::<usize>().unwrap())
                                    })
                                    .unwrap_or(0);
                                if request.len() >= end + 4 + length {
                                    break;
                                }
                            }
                        }
                        assert!(
                            String::from_utf8_lossy(&request)
                                .contains("Action=CreateConversation&")
                        );
                        let body = r#"{"Result":{"confirmed":true}}"#;
                        write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                        deadline = Instant::now() + Duration::from_millis(400);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
            requests
        });
        let mut client = mock_client(format!("http://{address}/"), log.clone());
        client.timeouts.control = Duration::from_secs(3);
        let result = client
            .call(
                "aigw",
                "CreateConversation",
                json!({"Content":"fixture-secret private prompt"}),
            )
            .await
            .unwrap();
        assert_eq!(result["confirmed"], true);
        assert_eq!(server.join().unwrap(), 1, "a POST was replayed");
        let raw = fs_read(&log);
        let records = raw
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["kind"], "connect_retry_before_send");
        assert_eq!(records[0]["request"]["attempts"], 1);
        assert!(!raw.contains("fixture-secret") && !raw.contains("private prompt"));
    }

    #[tokio::test]
    async fn persistent_connector_failure_is_bounded_and_reports_both_time_limits() {
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(format!("http://{address}/"), log.clone());
        let error = client
            .call("aigw", "CreateConversation", json!({}))
            .await
            .unwrap_err()
            .to_string();
        let raw = fs_read(&log);
        let records = raw
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1]["kind"], "connect");
        assert_eq!(records[1]["request"]["attempts"], 2);
        assert_eq!(records[1]["request"]["connect_timeout_ms"], 15000);
        assert!(error.contains("HTTP 请求尚未发送") && error.contains("建连/发送尝试=2"));
        assert!(!raw.contains("127.0.0.1"));
    }

    fn completion_server(truncated: bool) -> (String, thread::JoinHandle<usize>) {
        completion_server_with_capture(truncated, None)
    }

    fn completion_server_with_capture(
        truncated: bool,
        capture: Option<std::sync::mpsc::Sender<Value>>,
    ) -> (String, thread::JoinHandle<usize>) {
        completion_server_with_readback(truncated, capture, None)
    }

    fn completion_server_with_readback(
        truncated: bool,
        mut capture: Option<std::sync::mpsc::Sender<Value>>,
        readback: Option<(u16, Value)>,
    ) -> (String, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let mut requests = 0;
            let mut deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < deadline {
                let (mut socket, _) = match listener.accept() {
                    Ok(socket) => socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("mock accept failed: {error}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0u8; 8192];
                    let count = socket.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(header_end) =
                        request.windows(4).position(|part| part == b"\r\n\r\n")
                    {
                        let header = String::from_utf8_lossy(&request[..header_end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= header_end + 4 + length {
                            break;
                        }
                    }
                }
                requests += 1;
                if readback.is_some() && requests >= 3 {
                    let request = String::from_utf8_lossy(&request);
                    assert!(request.contains(if requests == 3 {
                        "Action=Chat&"
                    } else {
                        "Action=GetConversationMessages&"
                    }));
                    assert!(requests <= 4, "Chat recovery issued additional requests");
                }
                if requests == 3 && readback.as_ref().is_some_and(|(status, _)| *status == 0) {
                    drop(socket); // Simulate losing the response before any headers.
                    deadline = Instant::now() + Duration::from_millis(400);
                    continue;
                }
                if requests == 2
                    && let Some(sender) = capture.take()
                {
                    let end = request
                        .windows(4)
                        .position(|part| part == b"\r\n\r\n")
                        .unwrap()
                        + 4;
                    sender
                        .send(serde_json::from_slice(&request[end..]).unwrap())
                        .unwrap();
                }
                let (content_type, body) = match requests {
                    1 => ("application/json", json!({"Result":{"ConversationInfo":{"ConversationID":"conversation-test","ProjectList":[{"SessionID":"session-test"}]}}}).to_string()),
                    2 => ("application/json", json!({"Result":{"MessageList":[{"MessageID":"message-test"}]}}).to_string()),
                    3 => ("text/event-stream", if truncated {
                        "data: {\"choices\":[{\"delta\":{\"content\":\"fixture-secret private reply\"},\"finish_reason\":null}]}\n\n".into()
                    } else {
                        "data: {\"choices\":[{\"delta\":{\"content\":\"confirmed reply\"},\"finish_reason\":\"stop\"}]}\n\n".into()
                    }),
                    _ => ("application/json", readback.as_ref().map_or_else(|| "{}".into(), |(_, stored)|json!({"Result":stored}).to_string())),
                };
                let status = if requests == 3 {
                    readback.as_ref().map_or(200, |(status, _)| *status)
                } else {
                    200
                };
                let length = body.len() + usize::from(truncated && requests == 3) * 32;
                let header = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nx-request-id: stream-test-123\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(header.as_bytes()).unwrap();
                socket.write_all(body.as_bytes()).unwrap();
                if requests == 3 {
                    deadline = Instant::now() + Duration::from_millis(400);
                }
            }
            requests
        });
        (endpoint, server)
    }

    fn completion_model() -> Model {
        Model {
            name: "fixture-model".into(),
            service_id: "service-test".into(),
            upstream_name: "upstream-test".into(),
            source_type: "test".into(),
            features: vec![],
            verified_image_input: false,
        }
    }

    fn completed_stored_fixture(text: &str) -> Value {
        json!({"MessageList":[
            {"AuthorRole":2,"WorkspaceID":"workspace-test","SessionID":"session-test","MessageID":"message-test","QueryID":"query-test","Content":"exact input"},
            {"AuthorRole":1,"WorkspaceID":"workspace-test","SessionID":"session-test","MessageID":"message-test","QueryID":"query-test","Content":text,"TokenInfo":{"StartTime":100,"EndTime":200,"FinishReason":"stop"}}
        ]})
    }
    #[test]
    fn stored_completion_requires_exact_input_identity_and_normal_finish() {
        let stored = completed_stored_fixture("confirmed private reply");
        assert_eq!(
            confirmed_stored_reply(
                &stored,
                "workspace-test",
                "session-test",
                "message-test",
                "exact input"
            ),
            Some("confirmed private reply")
        );
        for (path, value) in [
            ("/MessageList/0/WorkspaceID", json!("other")),
            ("/MessageList/1/WorkspaceID", json!("other")),
            ("/MessageList/0/SessionID", json!("other")),
            ("/MessageList/1/SessionID", json!("other")),
            ("/MessageList/0/MessageID", json!("other")),
            ("/MessageList/0/QueryID", json!("")),
            ("/MessageList/1/QueryID", json!("older-query")),
            ("/MessageList/0/Content", json!("older input")),
            ("/MessageList/1/Content", json!(" ")),
            ("/MessageList/1/TokenInfo", Value::Null),
            ("/MessageList/1/TokenInfo/FinishReason", json!("length")),
            ("/MessageList/1/TokenInfo/StartTime", json!(0)),
            ("/MessageList/1/TokenInfo/EndTime", json!(99)),
        ] {
            let mut bad = stored.clone();
            *bad.pointer_mut(path).unwrap() = value;
            assert!(
                confirmed_stored_reply(
                    &bad,
                    "workspace-test",
                    "session-test",
                    "message-test",
                    "exact input"
                )
                .is_none(),
                "accepted {path}"
            );
        }
        let mut ambiguous = stored.clone();
        let duplicate = ambiguous["MessageList"][1].clone();
        ambiguous["MessageList"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(
            confirmed_stored_reply(
                &ambiguous,
                "workspace-test",
                "session-test",
                "message-test",
                "exact input"
            )
            .is_none()
        );
    }
    #[tokio::test]
    async fn lost_chat_headers_read_back_the_completed_reply_without_replay() {
        let text = r#"{"type":"final","text":"fixture-secret private reply"}"#;
        let (endpoint, server) =
            completion_server_with_readback(false, None, Some((0, completed_stored_fixture(text))));
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        assert_eq!(
            client
                .complete("workspace-test", &completion_model(), "exact input")
                .await
                .unwrap(),
            text
        );
        assert_eq!(
            server.join().unwrap(),
            4,
            "Chat was replayed instead of one readback"
        );
        let raw = fs_read(&log);
        let records = raw
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records[0]["action"], "Chat");
        assert_eq!(records[1]["kind"], "stored_completion_matched");
        assert_eq!(records[1]["reply_bytes"], text.len());
        for private in [
            "fixture-secret",
            "private reply",
            "exact input",
            "query-test",
            "session-test",
        ] {
            assert!(!raw.contains(private));
        }
    }
    #[tokio::test]
    async fn incomplete_or_failed_readback_preserves_the_original_chat_failure() {
        for stored in [json!({"MessageList":[]}), Value::Null] {
            let (endpoint, server) =
                completion_server_with_readback(false, None, Some((0, stored)));
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("requests.jsonl");
            let client = mock_client(endpoint, log.clone());
            let error = client
                .complete("workspace-test", &completion_model(), "exact input")
                .await
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("Chat")
                    && error.contains("未能确认")
                    && error.contains("未重发 Chat")
            );
            assert_eq!(server.join().unwrap(), 4);
            let raw = fs_read(&log);
            assert!(raw.contains("completion_unconfirmed"));
            assert!(!raw.contains("exact input"));
        }
    }
    #[tokio::test]
    async fn explicit_http_chat_failure_does_not_read_back_or_ignore_the_status() {
        let (endpoint, server) = completion_server_with_readback(
            false,
            None,
            Some((500, completed_stored_fixture("old reply"))),
        );
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        let error = client
            .complete("workspace-test", &completion_model(), "exact input")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 500"));
        assert_eq!(server.join().unwrap(), 3);
        assert!(!fs_read(&log).contains("ChatReadback"));
    }

    #[tokio::test]
    async fn image_request_uses_files_attachment_and_retains_the_text_protocol() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (endpoint, server) = completion_server_with_capture(false, Some(sender));
        let dir = tempfile::tempdir().unwrap();
        let client = mock_client(endpoint, dir.path().join("requests.jsonl"));
        let mut model = completion_model();
        model.features = vec!["vision".into()];
        let url = crate::images::test_image_url();
        let image = crate::images::InputImage::from_data_url(&url).unwrap();
        let result = client
            .complete_with_images(
                "workspace-test",
                &model,
                "Keep this exact text protocol",
                &[image],
            )
            .await
            .unwrap();
        assert_eq!(result, "confirmed reply");
        let payload = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        let message = &payload["MessageList"][0];
        assert_eq!(message["ContentType"], "text");
        assert_eq!(message["Content"], "Keep this exact text protocol");
        assert_eq!(message["ExtendsInfo"]["Files"][0]["Url"], url);
        assert_eq!(message["ExtendsInfo"]["Files"][0]["Type"], "image");
        assert_eq!(
            server.join().unwrap(),
            3,
            "image upload must not replay Chat"
        );
        assert!(!dir.path().join("requests.jsonl").exists());
    }

    #[tokio::test]
    async fn images_for_a_text_only_model_fail_before_any_network_call() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let client = mock_client(
            format!("http://{}/", listener.local_addr().unwrap()),
            dir.path().join("requests.jsonl"),
        );
        let image =
            crate::images::InputImage::from_data_url(&crate::images::test_image_url()).unwrap();
        let error = client
            .complete_with_images(
                "workspace-test",
                &completion_model(),
                "inspect image",
                &[image],
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("未标记为支持识图"));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
        assert!(!dir.path().join("requests.jsonl").exists());
    }

    #[tokio::test]
    async fn oversized_prompt_is_rejected_before_creating_platform_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        let error = client
            .complete(
                "workspace-test",
                &completion_model(),
                &"x".repeat(128 * 1024),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("未发起模型请求"));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!log.exists());
    }

    #[tokio::test]
    async fn interrupted_reply_is_rejected_logged_without_content_and_not_replayed() {
        let (endpoint, server) = completion_server(true);
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        let error = client
            .complete("workspace-test", &completion_model(), "private prompt")
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(
            server.join().unwrap(),
            3,
            "uncertain Chat was replayed or recovered without confirmation"
        );
        assert!(error.contains("stream_network"));
        assert!(error.contains("stream-test-123"));
        assert!(error.contains("未自动重发"));
        let raw = fs_read(&log);
        let entry: Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(entry["kind"], "stream_network");
        assert_eq!(entry["http_status"], 200);
        assert_eq!(entry["model"], "fixture-model");
        assert!(entry["stream"]["received_bytes"].as_u64().unwrap() > 0);
        let combined = format!("{error}{raw}");
        for forbidden in [
            "fixture-secret",
            "private reply",
            "private prompt",
            "workspace-test",
            "conversation-test",
        ] {
            assert!(!combined.contains(forbidden));
        }
    }

    #[tokio::test]
    async fn confirmed_reply_still_completes_without_diagnostic_or_extra_request() {
        let (endpoint, server) = completion_server(false);
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        let reply = client
            .complete("workspace-test", &completion_model(), "private prompt")
            .await
            .unwrap();
        assert_eq!(reply, "confirmed reply");
        assert_eq!(server.join().unwrap(), 3);
        assert!(!log.exists());
    }

    #[tokio::test]
    async fn business_errors_keep_request_id_but_not_response_message() {
        let body = json!({"ResponseMetadata":{"RequestId":"business-request-123", "Error":{"Code":"ModelUnavailable", "Message":"fixture-secret"}}}).to_string();
        let (endpoint, server) = mock_server(200, "", &body);
        let dir = tempfile::tempdir().unwrap();
        let client = mock_client(endpoint, dir.path().join("requests.jsonl"));
        let error = client
            .call("aigw", "CreateConversation", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("business-request-123"));
        assert!(error.contains("ModelUnavailable"));
        assert!(error.contains("平台返回业务错误"));
        assert!(!error.contains("fixture-secret"));
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn json_body_disconnect_preserves_stage_and_progress_without_replay_or_content() {
        let body = "{\"Result\":\"fixture-secret private response";
        let (endpoint, server) = mock_json_server(
            200,
            "x-request-id: json-disconnect-123\r\n",
            body,
            body.len() + 128,
        );
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint, log.clone());
        let error = client
            .call_with_context(
                "aigw",
                "BatchCreateMessages",
                json!({"content":"private prompt"}),
                RequestContext {
                    model: Some("fixture-model"),
                    prompt_bytes: Some(42),
                },
            )
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(server.join().unwrap(), 1, "uncertain API request replayed");
        let raw = fs_read(&log);
        let entry: Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(entry["kind"], "response_network");
        assert_eq!(entry["action"], "BatchCreateMessages");
        assert_eq!(entry["http_status"], 200);
        assert_eq!(entry["model"], "fixture-model");
        assert!(entry["response"]["received_bytes"].as_u64().unwrap() > 0);
        assert!(error.contains("json-disconnect-123"));
        assert!(error.contains("结果未确认"));
        assert!(!error.contains("被服务端拒绝"));
        for secret in ["fixture-secret", "private response", "private prompt"] {
            assert!(!format!("{error}{raw}").contains(secret));
        }
    }

    #[tokio::test]
    async fn json_body_timeout_reports_the_received_status_and_keeps_partial_body_private() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let (release, hold) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 8192];
            socket.read(&mut request).unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nx-request-id: json-timeout-123\r\n\r\n{\"private\":\"fixture-secret\"").unwrap();
            let _ = hold.recv_timeout(Duration::from_secs(5));
        });
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.jsonl");
        let client = mock_client(endpoint.clone(), log.clone());
        let response = client
            .client
            .get(endpoint)
            .timeout(Duration::from_millis(800))
            .send()
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            client.read_json_response("CreateConversation", RequestContext::default(), 2, response),
        )
        .await;
        let _ = release.send(());
        server.join().unwrap();
        let error = result
            .expect("body read did not time out")
            .unwrap_err()
            .to_string();
        let raw = fs_read(&log);
        let entry: Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(entry["kind"], "response_timeout");
        assert_eq!(entry["http_status"], 200);
        assert!(entry["response"]["received_bytes"].as_u64().unwrap() > 0);
        assert!(error.contains("json-timeout-123"));
        assert!(error.contains("响应体读取超时"));
        assert!(!format!("{error}{raw}").contains("fixture-secret"));
    }

    #[tokio::test]
    async fn invalid_empty_oversized_and_missing_json_results_are_diagnosed_before_use() {
        for (body, kind) in [
            ("fixture-secret not JSON".to_string(), "response_json"),
            (String::new(), "response_empty"),
            (json!({"Result":null}).to_string(), "response_envelope"),
            (
                json!({"private":"fixture-secret"}).to_string(),
                "response_envelope",
            ),
            ("x".repeat(MAX_JSON + 1), "response_limit"),
        ] {
            let (endpoint, server) = mock_server(200, "x-request-id: json-invalid-123\r\n", &body);
            let dir = tempfile::tempdir().unwrap();
            let log = dir.path().join("requests.jsonl");
            let client = mock_client(endpoint, log.clone());
            let error = client
                .call("aigw", "CreateConversation", json!({}))
                .await
                .unwrap_err()
                .to_string();
            assert_eq!(server.join().unwrap(), 1);
            let raw = fs_read(&log);
            let entry: Value = serde_json::from_str(raw.trim()).unwrap();
            assert_eq!(entry["kind"], kind);
            assert_eq!(entry["http_status"], 200);
            assert!(error.contains("json-invalid-123"));
            assert!(error.contains("结果未确认"));
            assert!(!format!("{error}{raw}").contains("fixture-secret"));
        }
    }

    #[tokio::test]
    async fn oversized_non_json_errors_still_report_status_and_header_id() {
        let (endpoint, server) = mock_server(
            503,
            "x-request-id: oversized-123\r\n",
            &"x".repeat(MAX_ERROR_JSON + 1),
        );
        let dir = tempfile::tempdir().unwrap();
        let client = mock_client(endpoint, dir.path().join("requests.jsonl"));
        let error = client
            .call("aigw", "Chat", json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 503"));
        assert!(error.contains("oversized-123"));
        assert!(error.len() < 2048);
        assert_eq!(server.join().unwrap(), 1);
    }

    #[tokio::test]
    async fn stalled_error_body_cannot_hide_the_received_http_status() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let (release, hold) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 8192];
            socket.read(&mut buffer).unwrap();
            socket.write_all(b"HTTP/1.1 500 Test\r\nContent-Length: 4096\r\nx-request-id: stalled-123\r\n\r\n{").unwrap();
            let _ = hold.recv_timeout(Duration::from_secs(5));
        });
        let dir = tempfile::tempdir().unwrap();
        let client = mock_client(endpoint, dir.path().join("requests.jsonl"));
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            client.call("aigw", "Chat", json!({})),
        )
        .await;
        let _ = release.send(());
        server.join().unwrap();
        let error = result
            .expect("error metadata read exceeded its time budget")
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 500"));
        assert!(error.contains("stalled-123"));
    }
    #[test]
    fn signature_matches_verified_python_adapter() {
        let c = Credentials {
            access_key: "fixture-access".into(),
            secret: "fixture-secret".into(),
            account_id: "fixture-account".into(),
        };
        let headers = signed_headers(
            &c,
            "aigw",
            "ai.xjtu.edu.cn:30040",
            "Action=Chat&Version=2023-08-01&X-Account-Id=fixture-account",
            b"{}",
            "20261016T000000Z",
        )
        .unwrap();
        assert!(
            headers["authorization"]
                .to_str()
                .unwrap()
                .ends_with("105820c3a23fd5257a66193fe8e239056c19410860bcddf56bdfe4d094f02b9c")
        );
        let iam = signed_headers(
            &c,
            "iam",
            "ai.xjtu.edu.cn:30040",
            "Action=GetSelfInfo&Version=2024-12-25",
            b"{}",
            "20261016T000000Z",
        )
        .unwrap();
        assert_eq!(iam["x-top-service"], "iam");
        assert_ne!(iam["authorization"], headers["authorization"]);
    }
    #[test]
    fn incomplete_stream_requires_exact_stored_reply() {
        let mut stream = StreamAccumulator::default();
        assert!(
            !stream
                .push(r#"{"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}"#)
                .unwrap()
        );
        assert!(!stream.finished);
        assert!(stored_matches(
            &json!({"Messages":[{"AuthorRole":1,"Content":"你好"}]}),
            &stream.text
        ));
        assert!(!stored_matches(
            &json!({"Messages":[{"AuthorRole":0,"Content":"你好"}]}),
            &stream.text
        ));
        assert!(!stored_matches(
            &json!({"Messages":[{"AuthorRole":1,"Content":"你"}]}),
            &stream.text
        ));
        assert!(stream.push("[DONE]").unwrap());
        assert!(stream.finished);
    }
    #[test]
    fn rejects_truncation_and_bad_events() {
        let mut s = StreamAccumulator::default();
        assert!(
            s.push(r#"{"choices":[{"finish_reason":"length"}]}"#)
                .is_err()
        );
        assert!(s.push("{broken").is_err());
        assert!(s.push(r#"{"error":"upstream failed"}"#).is_err());
    }
}
