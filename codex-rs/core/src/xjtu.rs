//! XJTU TOP transport. Network uncertainty never becomes an executable tool event.
use crate::client_common::{Prompt, ResponseEvent, ResponseStream};
use codex_protocol::{
    error::{CodexErr, Result},
    models::ResponseItem,
    openai_models::ModelInfo,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use xjcode_native::{
    api::TopClient,
    auth,
    protocol::{self, AgentReply, Tool},
    setup,
};

pub(crate) async fn stream(prompt: &Prompt, model_info: &ModelInfo) -> Result<ResponseStream> {
    let input = prompt.input.clone();
    let instructions = prompt.base_instructions.text.clone();
    let mut specs = serde_json::to_value(&prompt.tools)
        .map_err(invalid)?
        .as_array()
        .cloned()
        .unwrap_or_default();
    for item in &input {
        if let ResponseItem::AdditionalTools { tools, .. } = item {
            specs.extend(tools.clone());
        }
    }
    let tools = protocol::tools_from_specs(&specs).map_err(invalid)?;
    let history = serde_json::to_value(&input).map_err(invalid)?;
    let schema = prompt.output_schema.clone();
    let model_name = model_info.slug.clone();
    let (tx, rx_event) = mpsc::channel(64);
    let cancelled = CancellationToken::new();
    let consumer_dropped = cancelled.clone();
    tokio::spawn(async move {
        let work = async {
            let home = auth::home()?;
            let settings = setup::load(&home)?;
            let credentials = auth::load_at(&home)?;
            anyhow::ensure!(
                settings.account_id == credentials.account_id,
                "账号与配置不一致，请重新登录"
            );
            let model = settings
                .models
                .iter()
                .find(|m| m.name == model_name)
                .ok_or_else(|| {
                    anyhow::anyhow!("所选模型不在交晓智授权目录；运行 xjcode models --refresh")
                })?;
            let client = TopClient::new(credentials)?;
            let base = protocol::render(&instructions, &history, &tools, schema.as_ref())?;
            let mut request = base.clone();
            for attempt in 0..3 {
                let raw = client
                    .complete(&settings.workspace_id, model, &request)
                    .await?;
                let parsed = AgentReply::parse(&raw, &tools).and_then(|reply| {
                    if let (Some(schema), AgentReply::Final { text }) = (&schema, &reply) {
                        let v: Value = serde_json::from_str(text)
                            .map_err(|_| anyhow::anyhow!("最终 text 不是 JSON"))?;
                        let validator = jsonschema_validate(schema, &v)?;
                        anyhow::ensure!(validator, "最终 text 未通过输出 schema 校验");
                    }
                    Ok(reply)
                });
                match parsed {
                    Ok(reply) => return events(reply, &tools),
                    Err(e) if attempt < 2 => {
                        request = format!(
                            "{base}\n上一次响应格式不正确：{e}。请重新生成完整协议对象；尚未执行任何工具。"
                        );
                    }
                    Err(e) => return Err(e),
                }
            }
            anyhow::bail!("模型格式修复次数耗尽")
        };
        let result = tokio::select! { _=cancelled.cancelled()=>return, r=work=>r };
        match result {
            Ok(events) => {
                for event in events {
                    if tx.send(Ok(event)).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Err(invalid(e))).await;
            }
        }
    });
    Ok(ResponseStream {
        rx_event,
        interrupt: None,
        consumer_dropped,
    })
}
fn jsonschema_validate(schema: &Value, value: &Value) -> anyhow::Result<bool> {
    xjcode_native::protocol::validate_value(schema, value)
}
fn invalid(e: impl std::fmt::Display) -> CodexErr {
    CodexErr::InvalidRequest(format!("XJCode: {e}"))
}
fn events(reply: AgentReply, tools: &[Tool]) -> anyhow::Result<Vec<ResponseEvent>> {
    let id = uuid::Uuid::new_v4().to_string();
    let mut events = vec![ResponseEvent::Created {
        response_id: Some(id.clone()),
    }];
    let final_turn = matches!(&reply, AgentReply::Final { .. });
    let (text, calls) = match reply {
        AgentReply::Final { text } => (text, vec![]),
        AgentReply::ToolCalls { text, calls } => (text, calls),
    };
    if !text.is_empty() {
        let item: ResponseItem = serde_json::from_value(
            json!({"type":"message","id":uuid::Uuid::new_v4().to_string(),"role":"assistant","content":[{"type":"output_text","text":text}],"phase":if final_turn {"final_answer"} else {"commentary"}}),
        )?;
        // Added carries an empty text; the delta carries the completed response once.
        let mut added = serde_json::to_value(&item)?;
        added["content"] = json!([]);
        events.push(ResponseEvent::OutputItemAdded(serde_json::from_value(
            added,
        )?));
        events.push(ResponseEvent::OutputTextDelta(text));
        events.push(ResponseEvent::OutputItemDone(item));
    }
    for call in calls {
        let tool = tools
            .iter()
            .find(|t| t.qualified_name() == call.name)
            .ok_or_else(|| anyhow::anyhow!("工具未注册"))?;
        let call_id = format!("xj_{}", uuid::Uuid::new_v4());
        let mut v = json!({"type":"function_call","id":call_id,"call_id":call_id,"name":tool.name,"namespace":tool.namespace,"arguments":serde_json::to_string(&call.arguments)?});
        if tool.kind == "custom" {
            v["type"] = json!("custom_tool_call");
            v["input"] = call.arguments;
            v.as_object_mut().unwrap().remove("arguments");
        } else if tool.kind == "tool_search" {
            v = json!({"type":"tool_search_call","id":call_id,"call_id":call_id,"execution":"client","arguments":call.arguments});
        }
        let item: ResponseItem = serde_json::from_value(v)?;
        events.push(ResponseEvent::OutputItemAdded(item.clone()));
        events.push(ResponseEvent::OutputItemDone(item));
    }
    events.push(ResponseEvent::Completed {
        response_id: id,
        token_usage: None,
        usage_metadata: None,
        end_turn: Some(final_turn),
    });
    Ok(events)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_emits_complete_message() {
        let events = events(
            AgentReply::Final {
                text: "完成".into(),
            },
            &[],
        )
        .unwrap();
        assert!(matches!(
            events.last(),
            Some(ResponseEvent::Completed {
                end_turn: Some(true),
                ..
            })
        ));
        assert_eq!(events.len(), 5);
    }
    #[test]
    fn models_catalog_is_valid() {
        let settings = setup::Settings {
            account_id: "fixture".into(),
            workspace_id: "workspace".into(),
            workspace_name: "fixture".into(),
            model: "test".into(),
            models: vec![xjcode_native::api::Model {
                name: "test".into(),
                service_id: "service".into(),
                upstream_name: "upstream".into(),
                source_type: "external".into(),
            }],
        };
        let _: codex_protocol::openai_models::ModelsResponse =
            serde_json::from_value(setup::catalog(&settings)).unwrap();
    }
}
