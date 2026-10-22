//! XJTU TOP transport. Network uncertainty never becomes an executable tool event.
use crate::client_common::{Prompt, ResponseEvent, ResponseStream};
use crate::responses_metadata::{CodexResponsesMetadata, CodexResponsesRequestKind};
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

fn prepare_transport(
    prompt: &Prompt,
    compaction: bool,
) -> anyhow::Result<(protocol::PreparedRequest, Value, Vec<Tool>)> {
    let history = serde_json::to_value(&prompt.input)?;
    // Compaction is declared by core metadata, never inferred from user text or
    // an empty tools field (normal incremental turns may declare tools in history).
    if compaction {
        return Ok((
            protocol::prepare_compaction_request(&prompt.base_instructions.text, &history)?,
            history,
            vec![],
        ));
    }
    let mut specs = serde_json::to_value(&prompt.tools)?
        .as_array()
        .cloned()
        .unwrap_or_default();
    for item in &prompt.input {
        if let ResponseItem::AdditionalTools { tools, .. } = item {
            specs.extend(tools.clone());
        }
    }
    let tools = protocol::tools_from_specs(&specs)?;
    let prepared = protocol::prepare_request(
        &prompt.base_instructions.text,
        &history,
        &tools,
        prompt.output_schema.as_ref(),
    )?;
    Ok((prepared, history, tools))
}

fn parse_reply(
    raw: &str,
    tools: &[Tool],
    history: &Value,
    schema: Option<&Value>,
    compaction: bool,
) -> anyhow::Result<AgentReply> {
    let reply = AgentReply::parse(raw, if compaction { &[] } else { tools })?;
    if !compaction {
        validate_tool_inputs(&reply, tools)?;
        protocol::validate_final_outcome(history, &reply)?;
        if let (Some(schema), AgentReply::Final { text, .. }) = (schema, &reply) {
            let v: Value =
                serde_json::from_str(text).map_err(|_| anyhow::anyhow!("最终 text 不是 JSON"))?;
            anyhow::ensure!(
                jsonschema_validate(schema, &v)?,
                "最终 text 未通过输出 schema 校验"
            );
        }
    }
    // A handoff summary records successes, failures and pending work; it is not
    // the original task's completion claim or its structured final deliverable.
    Ok(reply)
}

pub(crate) async fn stream(
    prompt: &Prompt,
    model_info: &ModelInfo,
    responses_metadata: &CodexResponsesMetadata,
) -> Result<ResponseStream> {
    let compaction = matches!(
        responses_metadata.request_kind,
        Some(CodexResponsesRequestKind::Compaction(_))
    );
    let (prepared, history, tools) = prepare_transport(prompt, compaction).map_err(invalid)?;
    let schema = prompt.output_schema.clone();
    let model_name = model_info.slug.clone();
    let (tx, rx_event) = mpsc::channel(64);
    let cancelled = CancellationToken::new();
    let consumer_dropped = cancelled.clone();
    tokio::spawn(async move {
        let work = async {
            let home = auth::home()?;
            if xjcode_native::provider::is_custom(&home)? {
                let profile=xjcode_native::provider::load(&home)?;
                anyhow::ensure!(profile.wire_api=="chat" && profile.model==model_name,"API 入口或模型已变更，请重新启动 XJCode");
                let raw=xjcode_native::provider::chat(&profile,&prepared.text,&prepared.images).await?;
                let reply=parse_reply(&raw,&tools,&history,schema.as_ref(),compaction)?;
                return events(reply,&tools);
            }
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
            let base = prepared.text;
            tracing::debug!(
                target: "xjcode_context_usage",
                request_bytes = base.len(),
                image_count = prepared.images.len(),
                compaction,
                tool_count = tools.len(),
                "bounded TOP request prepared"
            );
            let mut request = base.clone();
            for attempt in 0..3 {
                let raw = client
                    .complete_with_images(&settings.workspace_id, model, &request, &prepared.images)
                    .await?;
                let parsed = parse_reply(&raw, &tools, &history, schema.as_ref(), compaction);
                match parsed {
                    Ok(reply) => return events(reply, &tools),
                    Err(e) => {
                        let log = client.record_protocol_failure(
                            model,
                            request.len(),
                            raw.len(),
                            attempt + 1,
                            &e,
                        );
                        if attempt < 2 {
                            request = if compaction {
                                protocol::repair_compaction_prompt(&base, &raw, &e.to_string())?
                            } else {
                                protocol::repair_prompt(&base, &raw, &e.to_string())?
                            };
                        } else {
                            let note = log
                                .map(|path| format!("\n诊断记录：{}", path.display()))
                                .unwrap_or_default();
                            let stage = if compaction {
                                "上下文压缩摘要生成失败"
                            } else {
                                "模型回复校验失败"
                            };
                            anyhow::bail!(
                                "{stage}：{e}；已尝试 2 次回复纠错，该轮工具未执行。已有后台任务可能仍在运行，可用 /ps 查看；继续前先检查现有任务，避免重复启动。{note}"
                            );
                        }
                    }
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
fn validate_tool_inputs(reply: &AgentReply, tools: &[Tool]) -> anyhow::Result<()> {
    if let AgentReply::ToolCalls { calls, .. } = reply {
        for call in calls {
            if tools.iter().any(|t| {
                t.name == "apply_patch" && t.kind == "custom" && t.qualified_name() == call.name
            }) {
                codex_apply_patch::parse_patch(call.arguments.as_str().ok_or_else(|| anyhow::anyhow!("apply_patch input 必须是字符串"))?)
                    .map_err(|e| anyhow::anyhow!("apply_patch 语法错误：{e}。新增文件的每行（含空行）必须以 + 开头，修改须使用 @@ 与空格/减号/加号前缀。修复后再返回工具调用。"))?;
            }
        }
    }
    Ok(())
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
        // Preserve text verbatim: callers may require a JSON output schema.
        AgentReply::Final { text, .. } => (text, vec![]),
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
    fn stored_completion_never_bypasses_protocol_or_patch_validation() {
        let tools =
            protocol::tools_from_specs(&[json!({"type":"custom","name":"apply_patch"})]).unwrap();
        for raw in [
            r#"{"type":"tool_calls","calls":[{"name":"unregistered","arguments":{}}]}"#.to_owned(),
            json!({"type":"tool_calls","calls":[{"name":"apply_patch","arguments":{"input":"*** Begin Patch\n*** Add File: bad.py\nmissing plus\n*** End Patch"}}]}).to_string(),
            "{\"type\":\"tool_calls\"".to_owned(),
        ] {
            let stored = json!({"MessageList":[
                {"AuthorRole":2,"WorkspaceID":"w","SessionID":"s","MessageID":"m","QueryID":"q","Content":"input"},
                {"AuthorRole":1,"WorkspaceID":"w","SessionID":"s","MessageID":"m","QueryID":"q","Content":raw,"TokenInfo":{"StartTime":1,"EndTime":2,"FinishReason":"stop"}}
            ]});
            let confirmed = xjcode_native::api::confirmed_stored_reply(&stored,"w","s","m","input").unwrap();
            assert!(parse_reply(confirmed,&tools,&json!([]),None,false).is_err());
        }
    }

    #[test]
    fn incomplete_large_batch_is_never_dispatched_and_recovery_emits_only_new_step() {
        let tools = protocol::tools_from_specs(&[
            json!({"type":"function","name":"exec_command","parameters":{"type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]}}),
            json!({"type":"custom","name":"apply_patch"}),
        ]).unwrap();
        let raw = format!(
            "{{\"type\":\"tool_calls\",\"calls\":[{{\"name\":\"exec_command\",\"arguments\":{{\"cmd\":\"must_not_run\"}}}},{{\"name\":\"apply_patch\",\"arguments\":{{\"input\":\"{}",
            "unfinished code ".repeat(1000)
        );
        let history = json!([]);
        let error = parse_reply(&raw, &tools, &history, None, false).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<protocol::ReplyFormatError>()
                .unwrap()
                .kind,
            "protocol_long_eof"
        );
        let request =
            protocol::repair_prompt("preserve original task", &raw, &error.to_string()).unwrap();
        assert!(request.contains("该轮所有工具均未执行"));
        let patch = "*** Begin Patch\n*** Add File: small_step.py\n+def prepare():\n+    return {}\n*** End Patch";
        let repaired = json!({"type":"tool_calls","calls":[{"name":"apply_patch","arguments":{"input":patch}}]}).to_string();
        let reply = parse_reply(&repaired, &tools, &history, None, false).unwrap();
        let output = events(reply, &tools).unwrap();
        let calls = output
            .iter()
            .filter_map(|event| match event {
                ResponseEvent::OutputItemDone(ResponseItem::CustomToolCall {
                    name, input, ..
                }) => Some((name, input)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "apply_patch");
        assert_eq!(calls[0].1, patch);
        assert!(!output.iter().any(|event| matches!(
            event,
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })
        )));
    }

    #[test]
    fn compaction_disables_historical_tools_and_next_turn_restores_them() {
        let history = json!([
            {"type":"additional_tools","role":"developer","tools":[{"type":"function","name":"exec_command","parameters":{"type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"],"additionalProperties":false}}]},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"CONTEXT CHECKPOINT COMPACTION: keep F:/task and do not retrain."}]}
        ]);
        let prompt = Prompt {
            input: serde_json::from_value(history.clone()).unwrap(),
            ..Default::default()
        };
        let raw = r#"{"type":"tool_calls","calls":[{"name":"exec_command","arguments":{"cmd":"read existing result"}}]}"#;
        let (summary, preserved, tools) = prepare_transport(&prompt, true).unwrap();
        assert!(tools.is_empty());
        assert_eq!(preserved, serde_json::to_value(&prompt.input).unwrap());
        assert!(summary.text.contains("\"request_purpose\":\"compaction\""));
        assert!(parse_reply(raw, &tools, &history, None, true).is_err());
        let (_, _, tools) = prepare_transport(&prompt, false).unwrap();
        assert_eq!(
            tools.len(),
            1,
            "an empty incremental specs field must not disable historical declarations"
        );
        assert!(parse_reply(raw, &tools, &history, None, false).is_ok());
        // The user text above cannot select compaction mode; core metadata owns that decision.
    }

    #[test]
    fn compaction_can_summarize_failed_work_without_claiming_task_completion() {
        let history = json!([
            {"type":"function_call","call_id":"failed","name":"exec_command","arguments":"{}"},
            {"type":"function_call_output","call_id":"failed","output":"Chunk ID: fixture\nProcess exited with code 1\nOutput:\nreport not generated"}
        ]);
        let raw =
            r#"{"type":"final","text":"已有权重保留，报告生成失败，后续继续修复；不要重新训练。"}"#;
        let task_schema = json!({"type":"object","required":["task_report"]});
        assert!(parse_reply(raw, &[], &history, None, false).is_err());
        assert!(parse_reply(raw, &[], &history, Some(&task_schema), false).is_err());
        let summary = parse_reply(raw, &[], &history, Some(&task_schema), true).unwrap();
        let output = events(summary, &[]).unwrap();
        assert!(matches!(
            output.last(),
            Some(ResponseEvent::Completed {
                end_turn: Some(true),
                ..
            })
        ));
        assert!(!output.iter().any(|event| matches!(
            event,
            ResponseEvent::OutputItemDone(
                ResponseItem::FunctionCall { .. } | ResponseItem::CustomToolCall { .. }
            )
        )));
        assert!(parse_reply(r#"{"type":"final","text":""}"#, &[], &history, None, true).is_err());
    }

    #[test]
    fn rejects_missing_add_line_prefix_before_execution() {
        let tools =
            protocol::tools_from_specs(&[json!({"type":"custom","name":"apply_patch"})]).unwrap();
        for (line, valid) in [("<!DOCTYPE html>", false), ("+<!DOCTYPE html>", true)] {
            let raw = json!({"type":"tool_calls","calls":[{"name":"apply_patch","arguments":{"input":format!("*** Begin Patch\n*** Add File: page.html\n{line}\n*** End Patch")}}]}).to_string();
            let reply = AgentReply::parse(&raw, &tools).unwrap();
            assert_eq!(validate_tool_inputs(&reply, &tools).is_ok(), valid);
        }
    }
    #[test]
    fn final_emits_complete_message() {
        let events = events(
            AgentReply::Final {
                text: "完成".into(),
                blocked: false,
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
                features: vec![],
                verified_image_input: false,
            }],
        };
        let _: codex_protocol::openai_models::ModelsResponse =
            serde_json::from_value(setup::catalog(&settings)).unwrap();
    }
}
