//! Internal CLI boundary for the Artifact-bound external deployment path.
//!
//! This module owns only exact grammar dispatch, ordered preflight projection,
//! and the stable 19-key JSON envelope. Durable Controller and Runtime work is
//! delegated through typed owner seams; no frame bytes or filesystem handles
//! are serialized here.

use std::{ffi::OsString, io::Write, str::FromStr};

use paraegox_artifact::{ArtifactObjectRefV1, MaterializationReceiptRefV1};
use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
    ArtifactExecutionBindingV1, artifact_execution_profile_commitment_v1,
};
use serde::Serialize;

use crate::{
    artifact,
    config::{
        self, ArtifactExternalDeployCommandV1, ArtifactExternalDeploymentJsonIntentV1,
        ArtifactExternalDeploymentOperationIdInputV1, ArtifactExternalDeploymentQueryCommandV1,
    },
    error::LocalProcessError,
};

const OUTPUT_SCHEMA_VERSION: u16 = 1;
const PROFILE: &str = "developer-local-echo-prefix-v1";

#[derive(Serialize)]
struct DiagnosticJsonV1<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct ExternalDeploymentJsonLineV1<'a> {
    schema_version: u16,
    command: &'static str,
    mode: &'static str,
    ok: bool,
    changed: Option<bool>,
    operation_id: Option<&'a str>,
    state: Option<&'static str>,
    profile: Option<&'static str>,
    artifact_object_ref: Option<&'a str>,
    materialization_receipt_ref: Option<&'a str>,
    generation: Option<&'a str>,
    deployment_revision: Option<&'a str>,
    controller_snapshot_sequence: Option<&'a str>,
    deployment_receipt_ref: Option<&'a str>,
    runtime_apply_request_digest: Option<&'a str>,
    runtime_terminal_receipt_digest: Option<&'a str>,
    terminal_outcome: Option<&'static str>,
    current_health_checked: bool,
    diagnostics: Vec<DiagnosticJsonV1<'a>>,
}

struct ProjectionV1 {
    changed: Option<bool>,
    operation_id: Option<String>,
    state: Option<&'static str>,
    profile: Option<&'static str>,
    artifact_object_ref: Option<String>,
    materialization_receipt_ref: Option<String>,
    generation: Option<String>,
    deployment_revision: Option<String>,
    controller_snapshot_sequence: Option<String>,
    deployment_receipt_ref: Option<String>,
    runtime_apply_request_digest: Option<String>,
    runtime_terminal_receipt_digest: Option<String>,
    terminal_outcome: Option<&'static str>,
    error: LocalProcessError,
}

impl ProjectionV1 {
    fn error(
        operation_id: Option<ArtifactExternalDeploymentOperationIdInputV1>,
        error: LocalProcessError,
    ) -> Self {
        Self {
            changed: Some(false),
            operation_id: operation_id.map(operation_id_text),
            state: None,
            profile: None,
            artifact_object_ref: None,
            materialization_receipt_ref: None,
            generation: None,
            deployment_revision: None,
            controller_snapshot_sequence: None,
            deployment_receipt_ref: None,
            runtime_apply_request_digest: None,
            runtime_terminal_receipt_digest: None,
            terminal_outcome: None,
            error,
        }
    }

    fn verified_preflight(
        operation_id: ArtifactExternalDeploymentOperationIdInputV1,
        object_ref: ArtifactObjectRefV1,
        receipt_ref: MaterializationReceiptRefV1,
        error: LocalProcessError,
    ) -> Self {
        Self {
            changed: Some(false),
            operation_id: Some(operation_id_text(operation_id)),
            state: None,
            profile: Some(PROFILE),
            artifact_object_ref: Some(object_ref.to_string()),
            materialization_receipt_ref: Some(receipt_ref.to_string()),
            generation: None,
            deployment_revision: None,
            controller_snapshot_sequence: None,
            deployment_receipt_ref: None,
            runtime_apply_request_digest: None,
            runtime_terminal_receipt_digest: None,
            terminal_outcome: None,
            error,
        }
    }
}

/// Dispatches one already-recognized external deployment command. Until the
/// Controller facade accepts ownership, a fully verified request fails closed
/// as owner-unavailable; it never fabricates admitted or terminal progress.
pub(crate) fn dispatch_to(
    output: &mut impl Write,
    intent: ArtifactExternalDeploymentJsonIntentV1,
    arguments: &[OsString],
) -> u8 {
    let operation_id = config::artifact_external_preparsed_operation_id(intent, arguments);
    let projection = match intent {
        ArtifactExternalDeploymentJsonIntentV1::Deploy => {
            match config::parse_artifact_external_deploy(arguments) {
                Ok(command) => run_deploy_preflight(command),
                Err(error) => ProjectionV1::error(operation_id, error.into()),
            }
        }
        ArtifactExternalDeploymentJsonIntentV1::Query => {
            match config::parse_artifact_external_deployment_query(arguments) {
                Ok(command) => run_query_preflight(command),
                Err(error) => ProjectionV1::error(operation_id, error.into()),
            }
        }
    };
    let exit_code = projection.error.exit_code();
    if write_projection(output, intent.command(), &projection).is_ok() {
        exit_code
    } else {
        1
    }
}

#[cfg(unix)]
fn run_deploy_preflight(command: ArtifactExternalDeployCommandV1) -> ProjectionV1 {
    let operation_id = command.operation_id();
    if let Err(error) = artifact::ensure_deployment_execution_identity() {
        return ProjectionV1::error(Some(operation_id), error);
    }
    if let Err(error) = config::parse_artifact_store_authority_config(command.config()) {
        return ProjectionV1::error(Some(operation_id), error.into());
    }
    let object_ref = match ArtifactObjectRefV1::from_str(command.artifact_object_ref()) {
        Ok(value) => value,
        Err(_) => {
            return ProjectionV1::error(
                Some(operation_id),
                LocalProcessError::ArtifactExternalDeployArtifact,
            );
        }
    };
    let receipt_ref =
        match MaterializationReceiptRefV1::from_str(command.materialization_receipt_ref()) {
            Ok(value) => value,
            Err(_) => {
                return ProjectionV1::error(
                    Some(operation_id),
                    LocalProcessError::ArtifactExternalDeployMaterializationReceipt,
                );
            }
        };
    let bundle = match artifact::read_verified_materialization_for_deployment(
        command.config(),
        object_ref,
        receipt_ref,
    ) {
        Ok(value) => value,
        Err(error) => return ProjectionV1::error(Some(operation_id), error),
    };
    if bundle.pair().is_none()
        || ArtifactExecutionBindingV1::try_new(
            object_ref,
            receipt_ref,
            artifact_execution_profile_commitment_v1(),
        )
        .is_err()
    {
        return ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployArtifact,
        );
    }
    drop(bundle);
    ProjectionV1::verified_preflight(
        operation_id,
        object_ref,
        receipt_ref,
        LocalProcessError::ArtifactExternalDeployOwner,
    )
}

#[cfg(not(unix))]
fn run_deploy_preflight(command: ArtifactExternalDeployCommandV1) -> ProjectionV1 {
    ProjectionV1::error(
        Some(command.operation_id()),
        LocalProcessError::Configuration(config::ConfigError::UnsupportedPlatform),
    )
}

#[cfg(unix)]
fn run_query_preflight(command: ArtifactExternalDeploymentQueryCommandV1) -> ProjectionV1 {
    let operation_id = command.operation_id();
    if let Err(error) = artifact::ensure_deployment_execution_identity() {
        return ProjectionV1::error(Some(operation_id), error);
    }
    if let Err(error) = config::parse_artifact_store_authority_config(command.config()) {
        return ProjectionV1::error(Some(operation_id), error.into());
    }
    ProjectionV1::error(
        Some(operation_id),
        LocalProcessError::ArtifactExternalDeployOwner,
    )
}

#[cfg(not(unix))]
fn run_query_preflight(command: ArtifactExternalDeploymentQueryCommandV1) -> ProjectionV1 {
    ProjectionV1::error(
        Some(command.operation_id()),
        LocalProcessError::Configuration(config::ConfigError::UnsupportedPlatform),
    )
}

fn write_projection(
    output: &mut impl Write,
    command: &'static str,
    projection: &ProjectionV1,
) -> Result<(), LocalProcessError> {
    serde_json::to_writer(
        &mut *output,
        &ExternalDeploymentJsonLineV1 {
            schema_version: OUTPUT_SCHEMA_VERSION,
            command,
            mode: "local",
            ok: false,
            changed: projection.changed,
            operation_id: projection.operation_id.as_deref(),
            state: projection.state,
            profile: projection.profile,
            artifact_object_ref: projection.artifact_object_ref.as_deref(),
            materialization_receipt_ref: projection.materialization_receipt_ref.as_deref(),
            generation: projection.generation.as_deref(),
            deployment_revision: projection.deployment_revision.as_deref(),
            controller_snapshot_sequence: projection.controller_snapshot_sequence.as_deref(),
            deployment_receipt_ref: projection.deployment_receipt_ref.as_deref(),
            runtime_apply_request_digest: projection.runtime_apply_request_digest.as_deref(),
            runtime_terminal_receipt_digest: projection.runtime_terminal_receipt_digest.as_deref(),
            terminal_outcome: projection.terminal_outcome,
            current_health_checked: false,
            diagnostics: vec![DiagnosticJsonV1 {
                code: projection.error.code(),
                message: projection.error.message(),
            }],
        },
    )
    .map_err(|_| LocalProcessError::ArtifactExternalDeployJsonOutput)?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|_| LocalProcessError::ArtifactExternalDeployJsonOutput)
}

fn operation_id_text(operation_id: ArtifactExternalDeploymentOperationIdInputV1) -> String {
    let mut text = String::with_capacity(32);
    for byte in operation_id.as_bytes() {
        use core::fmt::Write as _;
        write!(&mut text, "{byte:02x}").expect("writing into String cannot fail");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_error_uses_exact_nineteen_key_envelope() {
        let arguments = vec![
            OsString::from("deploy"),
            OsString::from("--local"),
            OsString::from("--config"),
            OsString::from("/private/tmp/paraegox.toml"),
            OsString::from("--artifact-object-ref"),
            OsString::from("object"),
            OsString::from("--materialization-receipt-ref"),
            OsString::from("receipt"),
            OsString::from("--operation-id"),
            OsString::from("d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1"),
        ];
        let mut output = Vec::new();
        assert_eq!(
            dispatch_to(
                &mut output,
                ArtifactExternalDeploymentJsonIntentV1::Deploy,
                &arguments,
            ),
            2
        );
        assert_eq!(
            String::from_utf8(output).expect("JSON UTF-8"),
            concat!(
                "{\"schema_version\":1,\"command\":\"deploy\",\"mode\":\"local\",",
                "\"ok\":false,\"changed\":false,\"operation_id\":null,\"state\":null,",
                "\"profile\":null,\"artifact_object_ref\":null,",
                "\"materialization_receipt_ref\":null,\"generation\":null,",
                "\"deployment_revision\":null,\"controller_snapshot_sequence\":null,",
                "\"deployment_receipt_ref\":null,\"runtime_apply_request_digest\":null,",
                "\"runtime_terminal_receipt_digest\":null,\"terminal_outcome\":null,",
                "\"current_health_checked\":false,\"diagnostics\":[{",
                "\"code\":\"PXLC-DEPLOY-EXTERNAL-GRAMMAR\",",
                "\"message\":\"external deploy requires the exact artifact and operation arguments\"}]}\n",
            )
        );
    }

    #[test]
    fn query_grammar_error_uses_distinct_command_and_diagnostic() {
        let arguments = vec![
            OsString::from("deployment"),
            OsString::from("operation"),
            OsString::from("query"),
            OsString::from("--operation-id"),
        ];
        let mut output = Vec::new();
        assert_eq!(
            dispatch_to(
                &mut output,
                ArtifactExternalDeploymentJsonIntentV1::Query,
                &arguments,
            ),
            2
        );
        let value: serde_json::Value = serde_json::from_slice(&output).expect("query JSON");
        assert_eq!(value["command"], "deployment.operation.query");
        assert_eq!(
            value["diagnostics"][0]["code"],
            "PXLC-DEPLOYMENT-QUERY-GRAMMAR"
        );
        assert_eq!(value["operation_id"], serde_json::Value::Null);
    }

    #[test]
    fn json_output_failure_never_reuses_compiled_deploy_diagnostic() {
        let projection = ProjectionV1::error(
            Some(ArtifactExternalDeploymentOperationIdInputV1::for_test(
                [0xd1; 16],
            )),
            LocalProcessError::ArtifactExternalDeployJsonOutput,
        );
        let mut output = Vec::new();
        write_projection(&mut output, "deploy", &projection).expect("JSON vector");
        let text = String::from_utf8(output).expect("JSON UTF-8");
        assert!(text.contains("PXLC-DEPLOY-EXTERNAL-JSON-OUTPUT"));
        assert!(!text.contains("PXLC-DEPLOY-JSON-OUTPUT"));
    }
}
