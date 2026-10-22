//! Explicit diagnostic for every *currently authorized* model. No tools or task
//! scripts run. Only synthetic fixtures are sent; credentials stay inside TopClient.
//! Image metadata is bypassed on a cloned Model only for this diagnostic, so an
//! absent platform feature flag cannot make a capable model untestable.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use xjcode_native::{api::TopClient, auth, images::InputImage, setup};

#[derive(Deserialize)]
struct Fixture {
    file: PathBuf,
    code: String,
    sha256: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("fixture/report directory required")?,
    );
    let fixtures: Vec<Fixture> =
        serde_json::from_slice(&std::fs::read(root.join("fixtures.json"))?)?;
    ensure!(fixtures.len() == 2, "two independent fixtures required");
    let home = auth::home()?;
    let settings = setup::load(&home)?;
    let credentials = auth::load_at(&home)?;
    ensure!(
        settings.account_id == credentials.account_id,
        "account mismatch"
    );
    let client = TopClient::new(credentials)?;
    let models = client.models(&settings.workspace_id).await?;
    ensure!(!models.is_empty(), "no authorized models");
    for fixture in &fixtures {
        ensure!(
            hex::encode(Sha256::digest(std::fs::read(&fixture.file)?)) == fixture.sha256,
            "fixture hash mismatch"
        );
    }
    let mode = std::env::args().nth(2).unwrap_or_default();
    if mode == "--apply-results" {
        let report: Value = serde_json::from_slice(&std::fs::read(root.join("results.json"))?)?;
        let snapshot: Value =
            serde_json::from_slice(&std::fs::read(root.join("private-catalog.json"))?)?;
        ensure!(
            snapshot["account_id"] == settings.account_id
                && snapshot["workspace_id"] == settings.workspace_id,
            "report account/workspace mismatch"
        );
        let old_models: Vec<xjcode_native::api::Model> =
            serde_json::from_value(snapshot["models"].clone())?;
        let rows = report["models"]
            .as_array()
            .context("missing model results")?;
        ensure!(
            rows.len() == old_models.len()
                && report["tested_count"].as_u64() == Some(old_models.len() as u64),
            "incomplete report"
        );
        let mut confirmed = Vec::new();
        for row in rows {
            let name = row["model"].as_str().context("missing model name")?;
            let previous = old_models
                .iter()
                .find(|model| model.name == name)
                .context("not in original grant")?;
            let current = models.iter().find(|model| {
                model.name == name
                    && model.service_id == previous.service_id
                    && model.upstream_name == previous.upstream_name
                    && model.source_type == previous.source_type
            });
            let valid_rounds = row["rounds"].as_array().is_some_and(|rounds| {
                rounds.len() == 2
                    && rounds.iter().enumerate().all(|(index, result)| {
                        result["fixture_sha256"] == fixtures[index].sha256
                            && result["status"] == "recognized"
                            && result["reply"].as_str().is_some_and(|reply| {
                                reply
                                    .to_ascii_uppercase()
                                    .split(|c: char| !c.is_ascii_alphanumeric())
                                    .any(|code| code == fixtures[index].code)
                            })
                    })
            });
            if current.is_some() && valid_rounds {
                confirmed.push(name.to_string());
            }
        }
        xjcode_native::capabilities::record(
            &home,
            &settings,
            &models,
            &confirmed,
            fixtures
                .iter()
                .map(|fixture| fixture.sha256.clone())
                .collect(),
        )?;
        println!(
            "{}",
            json!({"verified_models":confirmed,"default_model_unchanged":true})
        );
        return Ok(());
    }
    if mode == "--text-controls" {
        let mut report: Value = serde_json::from_slice(&std::fs::read(root.join("results.json"))?)?;
        let rows = report["models"].as_array_mut().context("missing results")?;
        for row in rows.iter_mut() {
            if row["text_control"].is_object() || row["rounds"][0]["status"] != "service_error" {
                continue;
            }
            let model = models
                .iter()
                .find(|model| row["model"] == model.name)
                .context("model no longer authorized")?;
            let clock = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(90),
                client.complete(
                    &settings.workspace_id,
                    model,
                    "Reply exactly XJCODE_TEXT_CONTROL_OK",
                ),
            )
            .await;
            row["text_control"] = match result {
                Ok(Ok(reply)) => {
                    json!({"status":"responded","seconds":clock.elapsed().as_secs_f64(),"reply":reply.chars().take(200).collect::<String>()})
                }
                Ok(Err(error)) => {
                    json!({"status":"service_error","seconds":clock.elapsed().as_secs_f64(),"error":error.to_string()})
                }
                Err(_) => json!({"status":"timeout","seconds":clock.elapsed().as_secs_f64()}),
            };
            println!(
                "{}",
                json!({"model":model.name,"text_control":row["text_control"]["status"]})
            );
            std::io::stdout().flush()?;
        }
        report["updated_at_utc"] = json!(chrono::Utc::now().to_rfc3339());
        std::fs::write(
            root.join("results.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        return Ok(());
    }
    ensure!(mode.is_empty(), "unknown diagnostic mode");
    ensure!(
        !root.join("results.json").exists(),
        "existing report preserved; use a fresh fixture directory or an explicit report mode"
    );
    // This file is local evidence for binding future capability decisions to the
    // exact account, workspace and model deployment. Never copy it to public docs.
    std::fs::write(
        root.join("private-catalog.json"),
        serde_json::to_vec_pretty(&json!({
            "account_id": settings.account_id, "workspace_id": settings.workspace_id,
            "models": models
        }))?,
    )?;
    let images: Vec<_> = fixtures
        .iter()
        .map(|fixture| -> Result<InputImage> {
            let bytes = std::fs::read(&fixture.file)?;
            InputImage::from_data_url(&format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ))
        })
        .collect::<Result<_>>()?;
    let started = chrono::Utc::now().to_rfc3339();
    let mut rows = Vec::<Value>::new();
    for (index, model) in models.iter().enumerate() {
        let mut probe_model = model.clone();
        probe_model.verified_image_input = true;
        let mut rounds = Vec::new();
        for (round, image) in images.iter().enumerate() {
            let clock = Instant::now();
            let request = client.complete_with_images(
                &settings.workspace_id, &probe_model,
                "这是图片输入检查。请直接观察附图，只写出图中黑色的8位大写字母数字代码，再用一句中文描述左右两个图形的形状和颜色。代码只在图片像素中，没有在文字里提供。如果未收到图片，请明确回答未收到，不要猜测。不要调用工具，不读取本地文件。",
                std::slice::from_ref(image)
            );
            let result = tokio::time::timeout(Duration::from_secs(90), request).await;
            let row = match result {
                Ok(Ok(reply)) => {
                    let passed = reply
                        .to_ascii_uppercase()
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .any(|word| word == fixtures[round].code);
                    json!({"round":round+1, "status":if passed {"recognized"} else {"not_recognized"},
                        "seconds":clock.elapsed().as_secs_f64(), "reply":reply.chars().take(800).collect::<String>(),
                        "fixture_sha256":fixtures[round].sha256})
                }
                Ok(Err(error)) => {
                    json!({"round":round+1,"status":"service_error","seconds":clock.elapsed().as_secs_f64(),"error":error.to_string()})
                }
                Err(_) => {
                    json!({"round":round+1,"status":"timeout","seconds":clock.elapsed().as_secs_f64(),"error":"90s diagnostic budget; request not retried; upstream result unknown"})
                }
            };
            println!(
                "{}",
                json!({"index":index+1,"total":models.len(),"model":model.name,"round":round+1,"status":row["status"],"seconds":row["seconds"]})
            );
            std::io::stdout().flush()?;
            let passed = row["status"] == "recognized";
            rounds.push(row);
            if !passed {
                break;
            }
        }
        let confirmed = rounds.len() == 2 && rounds.iter().all(|row| row["status"] == "recognized");
        rows.push(
            json!({"model":model.name, "platform_declared_vision":model.platform_supports_images(),
            "confirmed_image_input":confirmed,"rounds":rounds}),
        );
        std::fs::write(
            root.join("results.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version":1,"started_at_utc":started,"updated_at_utc":chrono::Utc::now().to_rfc3339(),
                "authorized_count":models.len(),"tested_count":rows.len(),"models":rows,
                "method":"random-code-only-in-pixels; two independent fixtures required; no retries; no tools"
            }))?,
        )?;
    }
    Ok(())
}
