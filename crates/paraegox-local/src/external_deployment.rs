//! Internal CLI boundary for the Artifact-bound external deployment path.
//!
//! This module owns only exact grammar dispatch, ordered preflight projection,
//! and the stable 19-key JSON envelope. Durable Controller and Runtime work is
//! delegated through typed owner seams; no frame bytes or filesystem handles
//! are serialized here.

use std::{
    ffi::{OsStr, OsString},
    io::Write,
    str::FromStr,
};

#[cfg(unix)]
use std::path::PathBuf;

use paraegox_artifact::{ArtifactObjectRefV1, MaterializationReceiptRefV1};
#[cfg(unix)]
use paraegox_deployment::{
    ArtifactDeploymentOperationIdV1, DeveloperArtifactExternalControllerAuthorityBindingV1,
    DeveloperArtifactExternalControllerAuthorityRecheckFailureV1,
    DeveloperArtifactExternalControllerAuthorityV1, DeveloperArtifactExternalControllerFailureV1,
    DeveloperArtifactExternalControllerInvocationV1, DeveloperArtifactExternalControllerPhaseV1,
    DeveloperArtifactExternalControllerProjectionV1, DeveloperArtifactExternalControllerRequestV1,
    DeveloperArtifactExternalControllerV1,
};
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

#[cfg(unix)]
pub(crate) fn parse_supervisor_request(
    config: &config::LocalManagedChatConfigV1,
    object_ref: &OsStr,
    receipt_ref: &OsStr,
    operation_id: &OsStr,
) -> Result<DeveloperArtifactExternalControllerRequestV1, LocalProcessError> {
    let object_ref = ArtifactObjectRefV1::from_str(
        object_ref
            .to_str()
            .ok_or(LocalProcessError::ArtifactExternalDeployArtifact)?,
    )
    .map_err(|_| LocalProcessError::ArtifactExternalDeployArtifact)?;
    let receipt_ref = MaterializationReceiptRefV1::from_str(
        receipt_ref
            .to_str()
            .ok_or(LocalProcessError::ArtifactExternalDeployMaterializationReceipt)?,
    )
    .map_err(|_| LocalProcessError::ArtifactExternalDeployMaterializationReceipt)?;
    let operation_id =
        config::ArtifactExternalDeploymentOperationIdInputV1::parse_hidden(operation_id)?;
    let operation_id = ArtifactDeploymentOperationIdV1::try_from_bytes(*operation_id.as_bytes())
        .ok_or(LocalProcessError::ArtifactExternalDeployOwner)?;
    let commitment =
        paraegox_artifact::ArtifactConfigCommitmentV1::try_from_bytes(config.config_commitment())
            .map_err(|_| LocalProcessError::LifecycleConfiguration)?;
    DeveloperArtifactExternalControllerRequestV1::try_new(
        operation_id,
        commitment,
        object_ref,
        receipt_ref,
    )
    .ok_or(LocalProcessError::ArtifactExternalDeployArtifact)
}

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
    error: Option<LocalProcessError>,
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
            error: Some(error),
        }
    }

    #[cfg(unix)]
    fn from_controller(
        projection: &DeveloperArtifactExternalControllerProjectionV1,
        state: &'static str,
        terminal_outcome: Option<&'static str>,
        error: Option<LocalProcessError>,
    ) -> Self {
        Self {
            changed: Some(false),
            operation_id: Some(lower_hex(projection.operation_id().as_bytes())),
            state: Some(state),
            profile: Some(PROFILE),
            artifact_object_ref: Some(projection.object_ref().to_string()),
            materialization_receipt_ref: Some(projection.materialization_receipt_ref().to_string()),
            generation: projection
                .lifecycle_generation()
                .map(|value| lower_hex(&value)),
            deployment_revision: projection
                .deployment_revision()
                .map(|value| value.get().to_string()),
            controller_snapshot_sequence: projection
                .committed_controller_snapshot_sequence()
                .map(|value| value.get().to_string()),
            deployment_receipt_ref: projection.deployment_receipt_ref().map(ToOwned::to_owned),
            runtime_apply_request_digest: projection
                .runtime_apply_request_digest()
                .map(|value| lower_hex(value.as_bytes())),
            runtime_terminal_receipt_digest: projection
                .runtime_terminal_receipt_digest()
                .map(|value| lower_hex(value.as_bytes())),
            terminal_outcome,
            error,
        }
    }
}

/// Dispatches one already-recognized external deployment command. Query uses
/// the read-only Controller facade; deploy still fails closed after complete
/// preflight until the resident lifecycle owner can retain its exclusive gate.
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
    let exit_code = projection.error.map_or(0, LocalProcessError::exit_code);
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
    ProjectionV1::error(
        Some(operation_id),
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
    let authority_config = match config::parse_artifact_store_authority_config(command.config()) {
        Ok(value) => value,
        Err(error) => return ProjectionV1::error(Some(operation_id), error.into()),
    };
    let Some(controller_operation_id) =
        ArtifactDeploymentOperationIdV1::try_from_bytes(*operation_id.as_bytes())
    else {
        return ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployOwner,
        );
    };
    let mut authority = RevalidatingExternalControllerAuthority::new(&authority_config);
    project_controller_query(
        operation_id,
        DeveloperArtifactExternalControllerV1::query(&mut authority, controller_operation_id),
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
    let diagnostics = projection.error.map_or_else(Vec::new, |error| {
        vec![DiagnosticJsonV1 {
            code: error.code(),
            message: error.message(),
        }]
    });
    serde_json::to_writer(
        &mut *output,
        &ExternalDeploymentJsonLineV1 {
            schema_version: OUTPUT_SCHEMA_VERSION,
            command,
            mode: "local",
            ok: projection.error.is_none(),
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
            diagnostics,
        },
    )
    .map_err(|_| LocalProcessError::ArtifactExternalDeployJsonOutput)?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|_| LocalProcessError::ArtifactExternalDeployJsonOutput)
}

fn operation_id_text(operation_id: ArtifactExternalDeploymentOperationIdInputV1) -> String {
    lower_hex(operation_id.as_bytes())
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use core::fmt::Write as _;
        write!(&mut text, "{byte:02x}").expect("writing into String cannot fail");
    }
    text
}

#[cfg(unix)]
pub(crate) struct RevalidatingExternalControllerAuthority {
    config_path: PathBuf,
}

#[cfg(unix)]
impl RevalidatingExternalControllerAuthority {
    fn new(config: &config::LocalArtifactStoreAuthorityConfigV1) -> Self {
        Self::from_path(config.source_path().to_path_buf())
    }

    pub(crate) const fn from_path(config_path: PathBuf) -> Self {
        Self { config_path }
    }
}

#[cfg(unix)]
impl DeveloperArtifactExternalControllerAuthorityV1 for RevalidatingExternalControllerAuthority {
    fn revalidate(
        &mut self,
    ) -> Result<
        DeveloperArtifactExternalControllerAuthorityBindingV1,
        DeveloperArtifactExternalControllerAuthorityRecheckFailureV1,
    > {
        let config = config::parse_artifact_store_authority_config(&self.config_path)
            .map_err(map_controller_authority_config_error)?;
        DeveloperArtifactExternalControllerAuthorityBindingV1::try_new(
            config.state_root().to_path_buf(),
            config.config_commitment(),
        )
    }
}

#[cfg(unix)]
pub(crate) fn admit_under_lifecycle_owner(
    config: &config::LocalManagedChatConfigV1,
    request: &DeveloperArtifactExternalControllerRequestV1,
) -> Result<(), LocalProcessError> {
    let current = config::parse_artifact_store_authority_config(config.source_path())?;
    if current.config_commitment().as_bytes() != &config.config_commitment()
        || request.config_commitment() != current.config_commitment()
    {
        return Err(LocalProcessError::LifecycleConfiguration);
    }
    let mut authority = RevalidatingExternalControllerAuthority::new(&current);
    let invocation =
        DeveloperArtifactExternalControllerV1::admit_under_lifecycle_owner(&mut authority, request);
    if invocation.changed() != Some(true) {
        return Err(LocalProcessError::ArtifactExternalDeployOwner);
    }
    let projection = invocation
        .into_result()
        .map_err(map_controller_owner_failure)?;
    if projection.phase() != DeveloperArtifactExternalControllerPhaseV1::Admitted
        || projection.operation_id() != request.operation_id()
        || projection.object_ref() != request.object_ref()
        || projection.materialization_receipt_ref() != request.materialization_receipt_ref()
    {
        return Err(LocalProcessError::ArtifactExternalDeployOwner);
    }
    Ok(())
}

#[cfg(unix)]
fn map_controller_owner_failure(
    failure: DeveloperArtifactExternalControllerFailureV1,
) -> LocalProcessError {
    match failure {
        DeveloperArtifactExternalControllerFailureV1::UnsafePath => {
            LocalProcessError::Configuration(config::ConfigError::InvalidStateRoot)
        }
        DeveloperArtifactExternalControllerFailureV1::ConfigurationMismatch => {
            LocalProcessError::LifecycleConfiguration
        }
        DeveloperArtifactExternalControllerFailureV1::Conflict => {
            LocalProcessError::ArtifactExternalDeployConflict
        }
        DeveloperArtifactExternalControllerFailureV1::ReplaceRequired => {
            LocalProcessError::ArtifactExternalDeployReplaceRequired
        }
        DeveloperArtifactExternalControllerFailureV1::Contended
        | DeveloperArtifactExternalControllerFailureV1::PublicationUncertain(_) => {
            LocalProcessError::ArtifactExternalDeployUncertain
        }
        DeveloperArtifactExternalControllerFailureV1::NotFound
        | DeveloperArtifactExternalControllerFailureV1::Owner
        | DeveloperArtifactExternalControllerFailureV1::Io => {
            LocalProcessError::ArtifactExternalDeployOwner
        }
    }
}

#[cfg(unix)]
fn map_controller_authority_config_error(
    error: config::ConfigError,
) -> DeveloperArtifactExternalControllerAuthorityRecheckFailureV1 {
    match error {
        config::ConfigError::InvalidStateRoot | config::ConfigError::StateRootTooLong => {
            DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::UnsafePath
        }
        config::ConfigError::ConfigFileRead => {
            DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::Io
        }
        _ => DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::Configuration,
    }
}

#[cfg(unix)]
fn project_controller_query(
    operation_id: ArtifactExternalDeploymentOperationIdInputV1,
    invocation: DeveloperArtifactExternalControllerInvocationV1,
) -> ProjectionV1 {
    if invocation.changed() != Some(false) {
        return ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployOwner,
        );
    }
    match invocation.into_result() {
        Ok(projection) => project_controller_state(&projection),
        Err(failure) => project_controller_query_failure(operation_id, failure),
    }
}

#[cfg(unix)]
fn project_controller_state(
    projection: &DeveloperArtifactExternalControllerProjectionV1,
) -> ProjectionV1 {
    let (state, terminal_outcome, error) = match projection.phase() {
        DeveloperArtifactExternalControllerPhaseV1::Admitted => ("admitted", None, None),
        DeveloperArtifactExternalControllerPhaseV1::Committed => ("committed", None, None),
        DeveloperArtifactExternalControllerPhaseV1::Applying => ("applying", None, None),
        DeveloperArtifactExternalControllerPhaseV1::ActiveReady => {
            ("active_ready", Some("active_ready"), None)
        }
        DeveloperArtifactExternalControllerPhaseV1::Failed => (
            "failed",
            Some("failed"),
            Some(LocalProcessError::ArtifactExternalDeployFailed),
        ),
        DeveloperArtifactExternalControllerPhaseV1::Uncertain => (
            "uncertain",
            Some("uncertain"),
            Some(LocalProcessError::ArtifactExternalDeployUncertain),
        ),
    };
    ProjectionV1::from_controller(projection, state, terminal_outcome, error)
}

#[cfg(unix)]
fn project_controller_query_failure(
    operation_id: ArtifactExternalDeploymentOperationIdInputV1,
    failure: DeveloperArtifactExternalControllerFailureV1,
) -> ProjectionV1 {
    match failure {
        DeveloperArtifactExternalControllerFailureV1::UnsafePath => ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::Configuration(config::ConfigError::InvalidStateRoot),
        ),
        DeveloperArtifactExternalControllerFailureV1::ConfigurationMismatch => ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::LifecycleConfiguration,
        ),
        DeveloperArtifactExternalControllerFailureV1::Conflict => ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployConflict,
        ),
        DeveloperArtifactExternalControllerFailureV1::ReplaceRequired => ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployReplaceRequired,
        ),
        DeveloperArtifactExternalControllerFailureV1::NotFound => ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployNotFound,
        ),
        DeveloperArtifactExternalControllerFailureV1::Contended => {
            minimal_uncertain_projection(operation_id)
        }
        DeveloperArtifactExternalControllerFailureV1::PublicationUncertain(Some(projection)) => {
            ProjectionV1::from_controller(
                &projection,
                "uncertain",
                Some("uncertain"),
                Some(LocalProcessError::ArtifactExternalDeployUncertain),
            )
        }
        DeveloperArtifactExternalControllerFailureV1::PublicationUncertain(None) => {
            minimal_uncertain_projection(operation_id)
        }
        DeveloperArtifactExternalControllerFailureV1::Owner
        | DeveloperArtifactExternalControllerFailureV1::Io => ProjectionV1::error(
            Some(operation_id),
            LocalProcessError::ArtifactExternalDeployOwner,
        ),
    }
}

#[cfg(unix)]
fn minimal_uncertain_projection(
    operation_id: ArtifactExternalDeploymentOperationIdInputV1,
) -> ProjectionV1 {
    let mut projection = ProjectionV1::error(
        Some(operation_id),
        LocalProcessError::ArtifactExternalDeployUncertain,
    );
    projection.state = Some("uncertain");
    projection.terminal_outcome = Some("uncertain");
    projection
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

    #[test]
    fn successful_projection_has_empty_diagnostics() {
        let projection = ProjectionV1 {
            changed: Some(false),
            operation_id: Some("d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1".to_string()),
            state: Some("admitted"),
            profile: Some(PROFILE),
            artifact_object_ref: Some("object".to_string()),
            materialization_receipt_ref: Some("receipt".to_string()),
            generation: None,
            deployment_revision: None,
            controller_snapshot_sequence: None,
            deployment_receipt_ref: None,
            runtime_apply_request_digest: None,
            runtime_terminal_receipt_digest: None,
            terminal_outcome: None,
            error: None,
        };
        let mut output = Vec::new();
        write_projection(&mut output, "deployment.operation.query", &projection)
            .expect("JSON vector");
        let value: serde_json::Value = serde_json::from_slice(&output).expect("query JSON");
        assert_eq!(value["ok"], true);
        assert_eq!(value["changed"], false);
        assert_eq!(value["state"], "admitted");
        assert_eq!(value["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn owner_projection_preserves_only_the_input_operation_id() {
        let projection = ProjectionV1::error(
            Some(ArtifactExternalDeploymentOperationIdInputV1::for_test(
                [0xd1; 16],
            )),
            LocalProcessError::ArtifactExternalDeployOwner,
        );
        let mut output = Vec::new();
        write_projection(&mut output, "deploy", &projection).expect("JSON vector");
        let value: serde_json::Value = serde_json::from_slice(&output).expect("deploy JSON");
        assert_eq!(value["operation_id"], "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1");
        for field in [
            "state",
            "profile",
            "artifact_object_ref",
            "materialization_receipt_ref",
            "generation",
            "deployment_revision",
            "controller_snapshot_sequence",
            "deployment_receipt_ref",
            "runtime_apply_request_digest",
            "runtime_terminal_receipt_digest",
            "terminal_outcome",
        ] {
            assert_eq!(value[field], serde_json::Value::Null, "{field}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_gate_contention_uses_exact_minimal_uncertain_envelope() {
        let operation_id = ArtifactExternalDeploymentOperationIdInputV1::for_test([0xd1; 16]);
        let projection = minimal_uncertain_projection(operation_id);
        let mut output = Vec::new();
        write_projection(&mut output, "deployment.operation.query", &projection)
            .expect("JSON vector");
        assert_eq!(
            String::from_utf8(output).expect("JSON UTF-8"),
            concat!(
                "{\"schema_version\":1,\"command\":\"deployment.operation.query\",",
                "\"mode\":\"local\",\"ok\":false,\"changed\":false,",
                "\"operation_id\":\"d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1\",",
                "\"state\":\"uncertain\",\"profile\":null,",
                "\"artifact_object_ref\":null,\"materialization_receipt_ref\":null,",
                "\"generation\":null,\"deployment_revision\":null,",
                "\"controller_snapshot_sequence\":null,\"deployment_receipt_ref\":null,",
                "\"runtime_apply_request_digest\":null,",
                "\"runtime_terminal_receipt_digest\":null,",
                "\"terminal_outcome\":\"uncertain\",",
                "\"current_health_checked\":false,\"diagnostics\":[{",
                "\"code\":\"PXLC-DEPLOY-UNCERTAIN\",",
                "\"message\":\"deployment operation outcome is uncertain\"}]}\n",
            )
        );
    }
}
