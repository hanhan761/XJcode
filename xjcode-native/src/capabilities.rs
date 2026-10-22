//! Image capability evidence is local and scoped to an authorized deployment.
//! It extends a model's declared capabilities; it never creates a model grant.
use crate::{api::Model, auth::atomic_write, setup::Settings};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

const FILE: &str = "image-capabilities.json";
const MAX_BYTES: u64 = 128 * 1024;

#[derive(Serialize, Deserialize)]
struct VerifiedModel {
    name: String,
    service_id: String,
    upstream_name: String,
    source_type: String,
}
#[derive(Serialize, Deserialize)]
struct Evidence {
    schema_version: u32,
    method: String,
    account_id: String,
    workspace_id: String,
    tested_at_utc: String,
    fixture_sha256: Vec<String>,
    models: Vec<VerifiedModel>,
}

fn valid_proofs(proofs: &[String]) -> bool {
    proofs.len() == 2
        && proofs[0] != proofs[1]
        && proofs
            .iter()
            .all(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Ignore absent, corrupt or differently scoped optional evidence. A cached
/// boolean in xjtu.json alone is never enough to authorize image input.
pub fn apply(home: &Path, settings: &mut Settings) {
    for model in &mut settings.models {
        model.verified_image_input = false;
    }
    let file = home.join(FILE);
    if !std::fs::metadata(&file)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_BYTES)
    {
        return;
    }
    let Ok(bytes) = std::fs::read(file) else {
        return;
    };
    let Ok(evidence) = serde_json::from_slice::<Evidence>(&bytes) else {
        return;
    };
    if evidence.schema_version != 1
        || evidence.method != "two-independent-pixel-codes-v1"
        || evidence.account_id != settings.account_id
        || evidence.workspace_id != settings.workspace_id
        || !valid_proofs(&evidence.fixture_sha256)
    {
        return;
    }
    for model in &mut settings.models {
        model.verified_image_input = evidence.models.iter().any(|tested| {
            tested.name == model.name
                && tested.service_id == model.service_id
                && tested.upstream_name == model.upstream_name
                && tested.source_type == model.source_type
        });
    }
}

/// Called by the explicit diagnostic only after two different pixel codes were
/// recognized. Models must come from a fresh authorized platform catalog.
pub fn record(
    home: &Path,
    settings: &Settings,
    authorized: &[Model],
    confirmed: &[String],
    fixture_sha256: Vec<String>,
) -> Result<()> {
    ensure!(
        valid_proofs(&fixture_sha256),
        "two independent image proofs required"
    );
    let names: HashSet<_> = confirmed.iter().collect();
    ensure!(names.len() == confirmed.len(), "duplicate model evidence");
    ensure!(
        confirmed
            .iter()
            .all(|name| authorized.iter().any(|model| &model.name == name)),
        "image evidence cannot grant new models"
    );
    let models = authorized
        .iter()
        .filter(|model| names.contains(&model.name))
        .map(|model| VerifiedModel {
            name: model.name.clone(),
            service_id: model.service_id.clone(),
            upstream_name: model.upstream_name.clone(),
            source_type: model.source_type.clone(),
        })
        .collect();
    atomic_write(
        &home.join(FILE),
        &serde_json::to_vec_pretty(&Evidence {
            schema_version: 1,
            method: "two-independent-pixel-codes-v1".into(),
            account_id: settings.account_id.clone(),
            workspace_id: settings.workspace_id.clone(),
            tested_at_utc: chrono::Utc::now().to_rfc3339(),
            fixture_sha256,
            models,
        })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            account_id: "account".into(),
            workspace_id: "workspace".into(),
            workspace_name: "test".into(),
            model: "test-model".into(),
            models: vec![Model {
                name: "test-model".into(),
                service_id: "deployment".into(),
                upstream_name: "upstream".into(),
                source_type: "external".into(),
                features: vec![],
                verified_image_input: false,
            }],
        }
    }
    fn proofs() -> Vec<String> {
        vec!["a".repeat(64), "b".repeat(64)]
    }
    #[test]
    fn verified_capability_survives_save_refresh_but_does_not_change_the_grant_or_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let mut original = settings();
        record(
            dir.path(),
            &original,
            &original.models,
            &[original.model.clone()],
            proofs(),
        )
        .unwrap();
        apply(dir.path(), &mut original);
        assert!(original.models[0].supports_images());
        assert!(!original.models[0].platform_supports_images());
        assert!(original.models[0].features.is_empty());
        crate::setup::save(dir.path(), &original).unwrap();
        let loaded = crate::setup::load(dir.path()).unwrap();
        assert!(loaded.models[0].verified_image_input);
        assert_eq!(loaded.models.len(), 1);
        let mut refreshed = settings();
        crate::setup::save(dir.path(), &refreshed).unwrap();
        apply(dir.path(), &mut refreshed);
        assert!(refreshed.models[0].supports_images());
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("models.json")).unwrap())
                .unwrap();
        assert_eq!(
            catalog["models"][0]["input_modalities"],
            serde_json::json!(["text", "image"])
        );
    }
    #[test]
    fn evidence_cannot_cross_accounts_workspaces_names_or_model_deployments() {
        let dir = tempfile::tempdir().unwrap();
        let original = settings();
        record(
            dir.path(),
            &original,
            &original.models,
            &[original.model.clone()],
            proofs(),
        )
        .unwrap();
        for field in 0..6 {
            let mut other = original.clone();
            match field {
                0 => other.account_id.push('x'),
                1 => other.workspace_id.push('x'),
                2 => other.models[0].name.push('x'),
                3 => other.models[0].service_id.push('x'),
                4 => other.models[0].upstream_name.push('x'),
                _ => other.models[0].source_type.push('x'),
            }
            other.models[0].verified_image_input = true;
            apply(dir.path(), &mut other);
            assert!(!other.models[0].supports_images(), "field {field}");
        }
    }
    #[test]
    fn absent_corrupt_or_incomplete_evidence_cannot_enable_an_image_model() {
        let dir = tempfile::tempdir().unwrap();
        for bytes in ["{}", "not JSON", "{\"schema_version\":999}"] {
            std::fs::write(dir.path().join(FILE), bytes).unwrap();
            let mut config = settings();
            config.models[0].verified_image_input = true;
            apply(dir.path(), &mut config);
            assert!(!config.models[0].supports_images());
        }
        let config = settings();
        assert!(
            record(
                dir.path(),
                &config,
                &config.models,
                &["ungranted".into()],
                proofs()
            )
            .is_err()
        );
        assert!(
            record(
                dir.path(),
                &config,
                &config.models,
                &[config.model.clone()],
                vec!["a".repeat(64); 2]
            )
            .is_err()
        );
    }
}
