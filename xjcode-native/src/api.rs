use crate::auth::Credentials;
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use reqwest::{Client, Response, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};

pub const ENDPOINT: &str = "http://ai.xjtu.edu.cn:30040/";
const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_STREAM: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub service_id: String,
    pub upstream_name: String,
    pub source_type: String,
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
}
impl TopClient {
    pub fn new(credentials: Credentials) -> Result<Self> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .build()?;
        Ok(Self {
            client,
            credentials,
        })
    }
    async fn request(
        &self,
        service: &str,
        action: &str,
        payload: Value,
        stream: bool,
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
        let response = self
            .client
            .post(format!("{ENDPOINT}?{query}"))
            .headers(headers)
            .body(body)
            .timeout(Duration::from_secs(if stream { 180 } else { 20 }))
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("交晓智 {action}: 网络请求失败或超时，请检查校园网络；未自动重发")
            })?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "交晓智 {action}: HTTP {}；401 请重新登录，403 请检查权限，429 请稍后重试。未自动重发",
                status.as_u16()
            );
        }
        Ok(response)
    }
    pub async fn call(&self, service: &str, action: &str, payload: Value) -> Result<Value> {
        let mut response = self.request(service, action, payload, false).await?;
        let mut data = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("交晓智响应读取失败"))?
        {
            ensure!(
                data.len() + chunk.len() <= MAX_JSON,
                "交晓智响应超出大小限制"
            );
            data.extend(chunk);
        }
        let value: Value = serde_json::from_slice(&data).context("交晓智返回了无效 JSON")?;
        if let Some(error) = value
            .pointer("/ResponseMetadata/Error")
            .filter(|v| !v.is_null())
        {
            let code = error["Code"].as_str().unwrap_or("Unknown");
            let safe_code: String = code
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(64)
                .collect();
            bail!("交晓智 {action}: {safe_code}");
        }
        value
            .get("Result")
            .cloned()
            .context("交晓智响应缺少 Result")
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
        let r = self.call("aigw","CreateConversation",json!({"ConversationName":"XJCode","WorkspaceID":workspace,"Models":[{"ModelServiceID":model.service_id,"ModelName":model.upstream_name,"PublishSourceType":model.source_type}]})).await?;
        let info = &r["ConversationInfo"];
        let conversation = field(info, "ConversationID")?;
        let session = field(&info["ProjectList"][0], "SessionID")?;
        let r = self.call("aigw","BatchCreateMessages",json!({"ConversationID":conversation,"WorkspaceID":workspace,"MessageList":[{"SessionID":session,"ContentType":"text","Content":prompt}]})).await?;
        let message = field(&r["MessageList"][0], "MessageID")?;
        let response = self
            .request(
                "aigw",
                "Chat",
                json!({"SessionID":session,"MessageID":message,"WorkspaceID":workspace}),
                true,
            )
            .await?;
        ensure!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .starts_with("text/event-stream"),
            "Chat 未返回事件流"
        );
        let mut stream = response
            .bytes_stream()
            .scan(0usize, |total, chunk| {
                let result = match chunk {
                    Ok(bytes) => {
                        *total += bytes.len();
                        if *total > MAX_STREAM {
                            Err(std::io::Error::other("stream too large"))
                        } else {
                            Ok(bytes)
                        }
                    }
                    Err(_) => Err(std::io::Error::other("stream interrupted")),
                };
                futures_util::future::ready(Some(result))
            })
            .eventsource();
        let mut result = StreamAccumulator::default();
        while let Some(event) = stream.next().await {
            let event =
                event.map_err(|_| anyhow::anyhow!("交晓智流中断，结果未确认；未自动重发"))?;
            if result.push(&event.data)? {
                break;
            }
        }
        ensure!(!result.text.trim().is_empty(), "交晓智返回空文本");
        if !result.finished {
            let stored = self
                .call(
                    "aigw",
                    "GetConversationMessages",
                    json!({"ConversationID":conversation,"WorkspaceID":workspace}),
                )
                .await?;
            ensure!(
                stored_matches(&stored, &result.text),
                "交晓智流未确认完成；不执行工具，也不重发 Chat"
            );
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

#[cfg(test)]
mod tests {
    use super::*;
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
