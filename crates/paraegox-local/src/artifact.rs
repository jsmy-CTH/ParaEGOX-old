//! Internal Artifact F0 CLI adapter.
//!
//! Canonical bytes and materialization state remain owned by
//! `paraegox-artifact`. This module owns only the four exact local command
//! adapters, the offline build publication, and the exact JSON projection.

use std::{
    ffi::{OsStr, OsString},
    io::Write,
    path::{Component, Path, PathBuf},
};

use paraegox_artifact::ArtifactOperationIdV1;
use serde::Serialize;

use crate::{
    config::{self, ArtifactCommandV1, ArtifactJsonIntentV1},
    error::LocalProcessError,
};
#[cfg(not(unix))]
use crate::config::ConfigError;

const ARTIFACT_OUTPUT_SCHEMA_VERSION: u16 = 1;
const PROFILE: &str = "developer-local-echo-prefix-v1";
const RUNTIME_KIND: &str = "managed_model_data_v1";
const ADAPTER_ABI: &str = "bounded-text-model-data-v1";
const TARGET_PROFILE: &str = "developer-local-managed-model-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArtifactFailureV1 {
    changed: Option<bool>,
    error: LocalProcessError,
}

impl ArtifactFailureV1 {
    const fn new(changed: Option<bool>, error: LocalProcessError) -> Self {
        Self { changed, error }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactPairProjectionV1 {
    changed: bool,
    object_ref: String,
    payload_length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactMaterializationProjectionV1 {
    changed: Option<bool>,
    operation_id: String,
    state: Option<&'static str>,
    object_ref: Option<String>,
    receipt_ref: Option<String>,
    error: Option<LocalProcessError>,
}

#[derive(Serialize)]
struct ArtifactDiagnosticJsonV1<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct ArtifactPairJsonLineV1<'a> {
    schema_version: u16,
    command: &'static str,
    ok: bool,
    changed: Option<bool>,
    profile: Option<&'static str>,
    artifact_object_ref: Option<&'a str>,
    payload_length: Option<u64>,
    runtime_kind: Option<&'static str>,
    adapter_abi: Option<&'static str>,
    target_profile: Option<&'static str>,
    diagnostics: Vec<ArtifactDiagnosticJsonV1<'a>>,
}

#[derive(Serialize)]
struct ArtifactMaterializationJsonLineV1<'a> {
    schema_version: u16,
    command: &'static str,
    ok: bool,
    changed: Option<bool>,
    operation_id: Option<&'a str>,
    state: Option<&'static str>,
    artifact_object_ref: Option<&'a str>,
    materialization_receipt_ref: Option<&'a str>,
    diagnostics: Vec<ArtifactDiagnosticJsonV1<'a>>,
}

/// Dispatches one already-recognized Artifact command and writes at most one
/// compact JSON object. Return values are the frozen public exit codes.
pub(crate) fn dispatch_to(
    output: &mut impl Write,
    intent: ArtifactJsonIntentV1,
    arguments: &[OsString],
) -> u8 {
    let command = match config::parse_artifact_command(intent, arguments) {
        Ok(command) => command,
        Err(error) => {
            return write_preparse_error(output, intent, LocalProcessError::Configuration(error));
        }
    };

    #[cfg(not(unix))]
    {
        let _ = command;
        write_preparse_error(
            output,
            intent,
            LocalProcessError::Configuration(ConfigError::UnsupportedPlatform),
        )
    }

    #[cfg(unix)]
    {
        dispatch_unix(output, intent, command)
    }
}

fn write_preparse_error(
    output: &mut impl Write,
    intent: ArtifactJsonIntentV1,
    error: LocalProcessError,
) -> u8 {
    let exit_code = error.exit_code();
    let result = match intent {
        ArtifactJsonIntentV1::Build | ArtifactJsonIntentV1::Inspect => {
            write_pair_error(output, intent.command(), Some(false), error)
        }
        ArtifactJsonIntentV1::Materialize | ArtifactJsonIntentV1::MaterializationQuery => {
            write_materialization_error(output, intent.command(), Some(false), None, None, error)
        }
    };
    if result.is_ok() { exit_code } else { 1 }
}

fn write_pair_success(
    output: &mut impl Write,
    command: &'static str,
    projection: &ArtifactPairProjectionV1,
) -> Result<(), LocalProcessError> {
    write_json_line(
        output,
        &ArtifactPairJsonLineV1 {
            schema_version: ARTIFACT_OUTPUT_SCHEMA_VERSION,
            command,
            ok: true,
            changed: Some(projection.changed),
            profile: Some(PROFILE),
            artifact_object_ref: Some(&projection.object_ref),
            payload_length: Some(projection.payload_length),
            runtime_kind: Some(RUNTIME_KIND),
            adapter_abi: Some(ADAPTER_ABI),
            target_profile: Some(TARGET_PROFILE),
            diagnostics: Vec::new(),
        },
    )
}

fn write_pair_error(
    output: &mut impl Write,
    command: &'static str,
    changed: Option<bool>,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    write_json_line(
        output,
        &ArtifactPairJsonLineV1 {
            schema_version: ARTIFACT_OUTPUT_SCHEMA_VERSION,
            command,
            ok: false,
            changed,
            profile: None,
            artifact_object_ref: None,
            payload_length: None,
            runtime_kind: None,
            adapter_abi: None,
            target_profile: None,
            diagnostics: vec![ArtifactDiagnosticJsonV1 {
                code: error.code(),
                message: error.message(),
            }],
        },
    )
}

fn write_materialization_projection(
    output: &mut impl Write,
    command: &'static str,
    projection: &ArtifactMaterializationProjectionV1,
) -> Result<(), LocalProcessError> {
    let diagnostic = projection.error.map(|error| ArtifactDiagnosticJsonV1 {
        code: error.code(),
        message: error.message(),
    });
    write_json_line(
        output,
        &ArtifactMaterializationJsonLineV1 {
            schema_version: ARTIFACT_OUTPUT_SCHEMA_VERSION,
            command,
            ok: projection.error.is_none(),
            changed: projection.changed,
            operation_id: Some(&projection.operation_id),
            state: projection.state,
            artifact_object_ref: projection.object_ref.as_deref(),
            materialization_receipt_ref: projection.receipt_ref.as_deref(),
            diagnostics: diagnostic.into_iter().collect(),
        },
    )
}

fn write_materialization_error(
    output: &mut impl Write,
    command: &'static str,
    changed: Option<bool>,
    operation_id: Option<&str>,
    state: Option<&'static str>,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    write_json_line(
        output,
        &ArtifactMaterializationJsonLineV1 {
            schema_version: ARTIFACT_OUTPUT_SCHEMA_VERSION,
            command,
            ok: false,
            changed,
            operation_id,
            state,
            artifact_object_ref: None,
            materialization_receipt_ref: None,
            diagnostics: vec![ArtifactDiagnosticJsonV1 {
                code: error.code(),
                message: error.message(),
            }],
        },
    )
}

fn write_json_line(
    output: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), LocalProcessError> {
    serde_json::to_writer(&mut *output, value).map_err(|_| LocalProcessError::ArtifactJsonOutput)?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|_| LocalProcessError::ArtifactJsonOutput)
}

#[cfg(unix)]
fn dispatch_unix(
    output: &mut impl Write,
    intent: ArtifactJsonIntentV1,
    command: ArtifactCommandV1,
) -> u8 {
    if let Err(error) = ensure_execution_identity() {
        return write_preparse_error(output, intent, error);
    }

    match command {
        ArtifactCommandV1::Build { source, output: path } => {
            let result = run_build(&source, &path);
            finish_pair_command(output, intent.command(), result)
        }
        ArtifactCommandV1::Inspect { manifest, payload } => {
            let result = run_inspect(&manifest, &payload);
            finish_pair_command(output, intent.command(), result)
        }
        ArtifactCommandV1::Materialize {
            config,
            manifest,
            payload,
            operation_id,
        } => finish_materialization_command(
            output,
            intent.command(),
            operation_id,
            run_materialize(&config, &manifest, &payload, operation_id),
        ),
        ArtifactCommandV1::MaterializationQuery {
            config,
            operation_id,
        } => finish_materialization_command(
            output,
            intent.command(),
            operation_id,
            run_query(&config, operation_id),
        ),
    }
}

fn finish_pair_command(
    output: &mut impl Write,
    command: &'static str,
    result: Result<ArtifactPairProjectionV1, ArtifactFailureV1>,
) -> u8 {
    match result {
        Ok(projection) => {
            if write_pair_success(output, command, &projection).is_ok() {
                0
            } else {
                1
            }
        }
        Err(failure) => {
            let exit_code = failure.error.exit_code();
            if write_pair_error(output, command, failure.changed, failure.error).is_ok() {
                exit_code
            } else {
                1
            }
        }
    }
}

fn finish_materialization_command(
    output: &mut impl Write,
    command: &'static str,
    operation_id: ArtifactOperationIdV1,
    result: Result<ArtifactMaterializationProjectionV1, ArtifactFailureV1>,
) -> u8 {
    let operation_id = operation_id_text(operation_id);
    match result {
        Ok(projection) => {
            let exit_code = projection
                .error
                .map_or(0, LocalProcessError::exit_code);
            if write_materialization_projection(output, command, &projection).is_ok() {
                exit_code
            } else {
                1
            }
        }
        Err(failure) => {
            let exit_code = failure.error.exit_code();
            if write_materialization_error(
                output,
                command,
                failure.changed,
                Some(&operation_id),
                None,
                failure.error,
            )
            .is_ok()
            {
                exit_code
            } else {
                1
            }
        }
    }
}

fn operation_id_text(operation_id: ArtifactOperationIdV1) -> String {
    lower_hex(operation_id.as_bytes())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn lexical_absolute_path(path: &Path) -> Result<(), LocalProcessError> {
    let text = path.to_str().ok_or(LocalProcessError::ArtifactPath)?;
    if text == "/"
        || !text.starts_with('/')
        || text.ends_with('/')
        || text.contains("//")
        || text.as_bytes().contains(&0)
        || text[1..]
            .split('/')
            .any(|segment| segment == "." || segment == "..")
        || path
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(LocalProcessError::ArtifactPath);
    }
    Ok(())
}

fn artifact_path_from_os(value: &OsStr) -> Result<PathBuf, LocalProcessError> {
    let value = value
        .to_str()
        .ok_or(LocalProcessError::ArtifactPath)?;
    let path = PathBuf::from(value);
    lexical_absolute_path(&path)?;
    Ok(path)
}

#[cfg(unix)]
mod unix {
    use std::{
        collections::BTreeSet,
        ffi::{OsStr, OsString},
        fs::{File, Metadata},
        io::{Read, Write},
        os::{
            fd::OwnedFd,
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::{Component, Path, PathBuf},
    };

    use nix::{
        dir::Dir,
        fcntl::{OFlag, open, openat},
        sys::stat::{Mode, mkdirat},
        unistd::{PathconfVar, fpathconf, getegid, geteuid},
    };
    use paraegox_artifact::{
        ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1,
        ArtifactStoreAuthorityV1, ArtifactStoreChangeV1, ArtifactStoreFailureV1,
        ArtifactStoreInvocationV1, ArtifactStoreOperationStateV1, ArtifactStoreOperationViewV1,
        ArtifactStoreV1, MaterializationOperationV1, MaterializationRequestV1,
        VerifiedArtifactPairV1,
    };
    use rustix::fs::{RenameFlags, renameat_with};
    use sha2::{Digest as _, Sha256};

    use super::{
        ArtifactFailureV1, ArtifactMaterializationProjectionV1, ArtifactPairProjectionV1,
        artifact_path_from_os, lexical_absolute_path, lower_hex, operation_id_text,
    };
    use crate::{
        config::{self, ConfigError, LocalArtifactStoreAuthorityConfigV1},
        error::LocalProcessError,
    };

    const BUILD_OUTPUT_DOMAIN: &[u8] = b"paraegox.artifact.build-output-path.sha256.v1";
    const MANIFEST_NAME: &str = "manifest.pxam";
    const PAYLOAD_NAME: &str = "payload.bin";
    const DIRECTORY_MODE_BITS: u32 = 0o700;
    const FILE_MODE_BITS: u32 = 0o600;
    const MODE_MASK: u32 = 0o7777;
    const MAX_SOURCE_BYTES: usize = 64;
    const MANIFEST_BYTES: usize = 206;
    const STAGING_PREFIX: &str = ".paraegox-artifact-build-v1-";
    const STAGING_SUFFIX: &str = ".initializing";
    const STAGING_NAME_BYTES: usize = 105;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct FileIdentity {
        device: u64,
        inode: u64,
    }

    impl FileIdentity {
        fn from_metadata(metadata: &Metadata) -> Self {
            Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        }
    }

    struct DirectoryHandle {
        file: File,
        identity: FileIdentity,
        owner_uid: u32,
        owner_gid: u32,
    }

    struct PinnedParent {
        ancestor: DirectoryHandle,
        parent_name: OsString,
        parent: DirectoryHandle,
    }

    pub(super) fn ensure_execution_identity() -> Result<(), LocalProcessError> {
        if geteuid().is_root() || getegid().as_raw() == 0 {
            return Err(LocalProcessError::UnsafeExecutionIdentity);
        }
        Ok(())
    }

    pub(super) fn run_build(
        source: &OsStr,
        output: &OsStr,
    ) -> Result<ArtifactPairProjectionV1, ArtifactFailureV1> {
        let source = artifact_path_from_os(source)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let output = artifact_path_from_os(output)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let output_text = output
            .to_str()
            .ok_or_else(|| ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath))?;
        let output_leaf = output
            .file_name()
            .ok_or_else(|| ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath))?;
        let staging_name = build_staging_name(output_text);
        if staging_name.as_bytes().len() != STAGING_NAME_BYTES
            || OsStr::new(&staging_name) == output_leaf
        {
            return Err(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactPath,
            ));
        }
        let mut pinned = open_pinned_parent(&output, true)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        validate_native_path_limits(&pinned.parent, output_text, output_leaf, &staging_name)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;

        let final_exists = named_directory_exists(&pinned.parent, output_leaf)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let staging_exists = named_directory_exists(&pinned.parent, OsStr::new(&staging_name))
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let payload = read_source(&source)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let pair = VerifiedArtifactPairV1::from_payload(&payload).map_err(|_| {
            ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactCompatibility)
        })?;
        let expected_manifest = pair.manifest_bytes();

        if staging_exists {
            return Err(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactUncertain,
            ));
        }

        if final_exists {
            settle_existing_final(
                &mut pinned,
                &output,
                output_leaf,
                expected_manifest,
                pair.payload(),
            )?;
            return Ok(pair_projection(false, &pair));
        }

        revalidate_parent_and_absence(&pinned, &output, output_leaf, &staging_name)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        mkdirat(
            &pinned.parent.file,
            staging_name.as_str(),
            directory_mode(),
        )
        .map_err(|error| {
            ArtifactFailureV1::new(
                Some(false),
                if error == nix::errno::Errno::EEXIST {
                    LocalProcessError::ArtifactUncertain
                } else {
                    LocalProcessError::ArtifactIo
                },
            )
        })?;

        publish_new_build(
            &mut pinned,
            &output,
            output_leaf,
            &staging_name,
            expected_manifest,
            pair.payload(),
        )
        .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        Ok(pair_projection(true, &pair))
    }

    pub(super) fn run_inspect(
        manifest: &OsStr,
        payload: &OsStr,
    ) -> Result<ArtifactPairProjectionV1, ArtifactFailureV1> {
        let manifest = artifact_path_from_os(manifest)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let payload = artifact_path_from_os(payload)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let pair = read_verified_pair(&manifest, &payload)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        Ok(pair_projection(false, &pair))
    }

    pub(super) fn run_materialize(
        config_path: &Path,
        manifest: &Path,
        payload: &Path,
        operation_id: paraegox_artifact::ArtifactOperationIdV1,
    ) -> Result<ArtifactMaterializationProjectionV1, ArtifactFailureV1> {
        let config = config::parse_artifact_store_authority_config(config_path)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error.into()))?;
        lexical_absolute_path(manifest)
            .and_then(|()| lexical_absolute_path(payload))
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let pair = read_verified_pair(manifest, payload)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let request = MaterializationRequestV1::new(
            operation_id,
            config.config_commitment(),
            pair.object_ref(),
        );
        let mut authority = RevalidatingArtifactAuthority::new(&config);
        let invocation = ArtifactStoreV1::materialize(&mut authority, &request, &pair);
        drop(authority);
        drop(pair);
        Ok(project_invocation(operation_id, invocation))
    }

    pub(super) fn run_query(
        config_path: &Path,
        operation_id: paraegox_artifact::ArtifactOperationIdV1,
    ) -> Result<ArtifactMaterializationProjectionV1, ArtifactFailureV1> {
        let config = config::parse_artifact_store_authority_config(config_path)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error.into()))?;
        let mut authority = RevalidatingArtifactAuthority::new(&config);
        let invocation = ArtifactStoreV1::query(&mut authority, operation_id);
        drop(authority);
        Ok(project_invocation(operation_id, invocation))
    }

    fn pair_projection(changed: bool, pair: &VerifiedArtifactPairV1) -> ArtifactPairProjectionV1 {
        ArtifactPairProjectionV1 {
            changed,
            object_ref: pair.object_ref().to_string(),
            payload_length: u64::try_from(pair.payload().len()).expect("payload contract fits u64"),
        }
    }

    struct RevalidatingArtifactAuthority {
        config_path: PathBuf,
    }

    impl RevalidatingArtifactAuthority {
        fn new(config: &LocalArtifactStoreAuthorityConfigV1) -> Self {
            Self {
                config_path: config.source_path().to_path_buf(),
            }
        }
    }

    impl ArtifactStoreAuthorityV1 for RevalidatingArtifactAuthority {
        fn revalidate(
            &mut self,
        ) -> Result<ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1> {
            let config = config::parse_artifact_store_authority_config(&self.config_path)
                .map_err(map_authority_config_error)?;
            ArtifactStoreAuthorityBindingV1::try_new(
                config.state_root().to_path_buf(),
                config.config_commitment(),
            )
        }
    }

    fn map_authority_config_error(error: ConfigError) -> ArtifactStoreAuthorityRecheckFailureV1 {
        match error {
            ConfigError::InvalidStateRoot
            | ConfigError::StateRootTooLong => ArtifactStoreAuthorityRecheckFailureV1::UnsafePath,
            ConfigError::ConfigFileRead => ArtifactStoreAuthorityRecheckFailureV1::Io,
            _ => ArtifactStoreAuthorityRecheckFailureV1::Configuration,
        }
    }

    fn project_invocation(
        operation_id: paraegox_artifact::ArtifactOperationIdV1,
        invocation: ArtifactStoreInvocationV1,
    ) -> ArtifactMaterializationProjectionV1 {
        let changed = change_value(invocation.change());
        match invocation.into_result() {
            Ok(view) => project_view(changed, view),
            Err(failure) => project_store_failure(changed, operation_id, failure),
        }
    }

    const fn change_value(change: ArtifactStoreChangeV1) -> Option<bool> {
        match change {
            ArtifactStoreChangeV1::Unchanged => Some(false),
            ArtifactStoreChangeV1::Changed => Some(true),
            ArtifactStoreChangeV1::Unknown => None,
        }
    }

    fn project_view(
        changed: Option<bool>,
        view: ArtifactStoreOperationViewV1,
    ) -> ArtifactMaterializationProjectionV1 {
        let state = view.state();
        let error = match state {
            ArtifactStoreOperationStateV1::Failed => {
                Some(LocalProcessError::ArtifactMaterializationFailed)
            }
            ArtifactStoreOperationStateV1::Uncertain => {
                Some(LocalProcessError::ArtifactUncertain)
            }
            ArtifactStoreOperationStateV1::Admitted
            | ArtifactStoreOperationStateV1::Materializing
            | ArtifactStoreOperationStateV1::Materialized
            | ArtifactStoreOperationStateV1::AlreadyMaterialized => None,
        };
        ArtifactMaterializationProjectionV1 {
            changed,
            operation_id: operation_id_text(view.operation_id()),
            state: Some(state_text(state)),
            object_ref: Some(view.object_ref().to_string()),
            receipt_ref: view.receipt_ref().map(|receipt| receipt.to_string()),
            error,
        }
    }

    fn project_store_failure(
        changed: Option<bool>,
        operation_id: paraegox_artifact::ArtifactOperationIdV1,
        failure: ArtifactStoreFailureV1,
    ) -> ArtifactMaterializationProjectionV1 {
        let (state, object_ref, receipt_ref, error) = match failure {
            ArtifactStoreFailureV1::UnsafePath => (
                None,
                None,
                None,
                LocalProcessError::Configuration(ConfigError::InvalidStateRoot),
            ),
            ArtifactStoreFailureV1::ConfigurationMismatch => (
                None,
                None,
                None,
                LocalProcessError::LifecycleConfiguration,
            ),
            ArtifactStoreFailureV1::Conflict => {
                (None, None, None, LocalProcessError::ArtifactConflict)
            }
            ArtifactStoreFailureV1::Capacity => {
                (None, None, None, LocalProcessError::ArtifactCapacity)
            }
            ArtifactStoreFailureV1::NotFound => {
                (None, None, None, LocalProcessError::ArtifactNotFound)
            }
            ArtifactStoreFailureV1::Contended => (
                Some("uncertain"),
                None,
                None,
                LocalProcessError::ArtifactUncertain,
            ),
            ArtifactStoreFailureV1::PublicationUncertain { operation } => {
                let (object_ref, receipt_ref) = operation
                    .as_deref()
                    .map(operation_refs)
                    .unwrap_or((None, None));
                (
                    Some("uncertain"),
                    object_ref,
                    receipt_ref,
                    LocalProcessError::ArtifactUncertain,
                )
            }
            ArtifactStoreFailureV1::Owner => {
                (None, None, None, LocalProcessError::ArtifactOwner)
            }
            ArtifactStoreFailureV1::Io => (None, None, None, LocalProcessError::ArtifactIo),
        };
        ArtifactMaterializationProjectionV1 {
            changed,
            operation_id: operation_id_text(operation_id),
            state,
            object_ref,
            receipt_ref,
            error: Some(error),
        }
    }

    fn operation_refs(operation: &MaterializationOperationV1) -> (Option<String>, Option<String>) {
        (
            Some(operation.request().object_ref().to_string()),
            operation
                .receipt()
                .map(paraegox_artifact::MaterializationReceiptRefV1::from_receipt)
                .map(|receipt| receipt.to_string()),
        )
    }

    const fn state_text(state: ArtifactStoreOperationStateV1) -> &'static str {
        match state {
            ArtifactStoreOperationStateV1::Admitted => "admitted",
            ArtifactStoreOperationStateV1::Materializing => "materializing",
            ArtifactStoreOperationStateV1::Materialized => "materialized",
            ArtifactStoreOperationStateV1::AlreadyMaterialized => "already_materialized",
            ArtifactStoreOperationStateV1::Failed => "failed",
            ArtifactStoreOperationStateV1::Uncertain => "uncertain",
        }
    }

    fn build_staging_name(output: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(BUILD_OUTPUT_DOMAIN);
        digest.update(u64::try_from(output.len()).expect("path bound fits u64").to_be_bytes());
        digest.update(output.as_bytes());
        format!(
            "{STAGING_PREFIX}{}{STAGING_SUFFIX}",
            lower_hex(&digest.finalize())
        )
    }

    fn directory_mode() -> Mode {
        Mode::S_IRUSR | Mode::S_IWUSR | Mode::S_IXUSR
    }

    fn file_mode() -> Mode {
        Mode::S_IRUSR | Mode::S_IWUSR
    }

    fn directory_from_owned(
        owned: OwnedFd,
        strict: bool,
    ) -> Result<DirectoryHandle, LocalProcessError> {
        let file = File::from(owned);
        let metadata = file.metadata().map_err(|_| LocalProcessError::ArtifactIo)?;
        validate_directory_metadata(&metadata, strict)?;
        Ok(DirectoryHandle {
            identity: FileIdentity::from_metadata(&metadata),
            file,
            owner_uid: geteuid().as_raw(),
            owner_gid: getegid().as_raw(),
        })
    }

    fn validate_directory_metadata(
        metadata: &Metadata,
        strict: bool,
    ) -> Result<(), LocalProcessError> {
        let uid = geteuid().as_raw();
        let gid = getegid().as_raw();
        let mode = metadata.mode() & MODE_MASK;
        if !metadata.is_dir()
            || (metadata.uid() != uid && metadata.uid() != 0)
            || mode & 0o022 != 0
            || (strict
                && (metadata.uid() != uid
                    || metadata.gid() != gid
                    || mode != DIRECTORY_MODE_BITS))
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn validate_regular_metadata(
        metadata: &Metadata,
        strict_mode: bool,
        expected_len: Option<u64>,
    ) -> Result<(), LocalProcessError> {
        let mode = metadata.mode() & MODE_MASK;
        if !metadata.is_file()
            || metadata.uid() != geteuid().as_raw()
            || metadata.gid() != getegid().as_raw()
            || metadata.nlink() != 1
            || mode & 0o133 != 0
            || (strict_mode && mode != FILE_MODE_BITS)
            || expected_len.is_some_and(|length| metadata.len() != length)
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn open_directory_at(
        parent: &DirectoryHandle,
        name: &OsStr,
        strict: bool,
    ) -> Result<DirectoryHandle, LocalProcessError> {
        revalidate_directory(parent, false)?;
        let owned = openat(
            &parent.file,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| LocalProcessError::ArtifactPath)?;
        directory_from_owned(owned, strict)
    }

    fn revalidate_directory(
        directory: &DirectoryHandle,
        strict: bool,
    ) -> Result<(), LocalProcessError> {
        let metadata = directory
            .file
            .metadata()
            .map_err(|_| LocalProcessError::ArtifactIo)?;
        validate_directory_metadata(&metadata, strict)?;
        if FileIdentity::from_metadata(&metadata) != directory.identity
            || directory.owner_uid != geteuid().as_raw()
            || directory.owner_gid != getegid().as_raw()
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn open_pinned_parent(path: &Path, strict: bool) -> Result<PinnedParent, LocalProcessError> {
        lexical_absolute_path(path)?;
        let parent_path = path.parent().ok_or(LocalProcessError::ArtifactPath)?;
        let components = parent_path
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name.to_os_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if components.is_empty() {
            return Err(LocalProcessError::ArtifactPath);
        }
        let root = open(
            "/",
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| LocalProcessError::ArtifactPath)?;
        let mut current = directory_from_owned(root, false)?;
        for component in &components[..components.len() - 1] {
            current = open_directory_at(&current, component, false)?;
        }
        let parent_name = components.last().expect("nonempty components").clone();
        let parent = open_directory_at(&current, &parent_name, strict)?;
        Ok(PinnedParent {
            ancestor: current,
            parent_name,
            parent,
        })
    }

    fn reopen_parent(pinned: &PinnedParent) -> Result<DirectoryHandle, LocalProcessError> {
        revalidate_directory(&pinned.ancestor, false)?;
        let parent = open_directory_at(&pinned.ancestor, &pinned.parent_name, true)?;
        if parent.identity != pinned.parent.identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(parent)
    }

    fn named_directory_exists(
        parent: &DirectoryHandle,
        name: &OsStr,
    ) -> Result<bool, LocalProcessError> {
        match openat(
            &parent.file,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(owned) => {
                let directory = directory_from_owned(owned, true)?;
                drop(directory);
                Ok(true)
            }
            Err(nix::errno::Errno::ENOENT) => Ok(false),
            Err(_) => Err(LocalProcessError::ArtifactPath),
        }
    }

    fn validate_native_path_limits(
        parent: &DirectoryHandle,
        output: &str,
        leaf: &OsStr,
        staging: &str,
    ) -> Result<(), LocalProcessError> {
        let name_max = fpathconf(&parent.file, PathconfVar::NAME_MAX)
            .map_err(|_| LocalProcessError::ArtifactPath)?
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(LocalProcessError::ArtifactPath)?;
        let path_max = fpathconf(&parent.file, PathconfVar::PATH_MAX)
            .map_err(|_| LocalProcessError::ArtifactPath)?
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(LocalProcessError::ArtifactPath)?;
        if leaf.as_bytes().len() > name_max
            || staging.len() > name_max
            || output.len() >= path_max
            || output
                .rfind('/')
                .and_then(|separator| {
                    separator
                        .checked_add(1)?
                        .checked_add(staging.len())
                })
                .map_or(true, |length| length >= path_max)
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn scan_names(directory: &DirectoryHandle) -> Result<BTreeSet<OsString>, LocalProcessError> {
        revalidate_directory(directory, true)?;
        let owned = openat(
            &directory.file,
            ".",
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| match error {
            nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => {
                LocalProcessError::ArtifactPath
            }
            _ => LocalProcessError::ArtifactIo,
        })?;
        let mut stream = Dir::from_fd(owned).map_err(|_| LocalProcessError::ArtifactIo)?;
        let mut names = BTreeSet::new();
        for entry in stream.iter() {
            let entry = entry.map_err(|_| LocalProcessError::ArtifactIo)?;
            let name = entry.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if name.contains(&0) || !names.insert(OsStr::from_bytes(name).to_os_string()) {
                return Err(LocalProcessError::ArtifactPath);
            }
        }
        Ok(names)
    }

    fn exact_names(
        directory: &DirectoryHandle,
        expected: &[&str],
    ) -> Result<(), LocalProcessError> {
        let actual = scan_names(directory)?;
        let expected = expected.iter().map(OsString::from).collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn read_regular_at(
        parent: &DirectoryHandle,
        name: &OsStr,
        maximum: usize,
        strict_mode: bool,
    ) -> Result<(Box<[u8]>, FileIdentity, File), LocalProcessError> {
        let owned = openat(
            &parent.file,
            name,
            OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| match error {
            nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => {
                LocalProcessError::ArtifactPath
            }
            _ => LocalProcessError::ArtifactIo,
        })?;
        let mut file = File::from(owned);
        let metadata = file.metadata().map_err(|_| LocalProcessError::ArtifactIo)?;
        validate_regular_metadata(&metadata, strict_mode, None)?;
        let identity = FileIdentity::from_metadata(&metadata);
        let mut bytes = Vec::with_capacity(maximum.min(4096));
        file.by_ref()
            .take(u64::try_from(maximum + 1).expect("small bound"))
            .read_to_end(&mut bytes)
            .map_err(|_| LocalProcessError::ArtifactIo)?;
        if bytes.len() > maximum {
            return Err(LocalProcessError::ArtifactCompatibility);
        }
        Ok((bytes.into_boxed_slice(), identity, file))
    }

    fn read_source(path: &Path) -> Result<Box<[u8]>, LocalProcessError> {
        let pinned = open_pinned_parent(path, false)?;
        let leaf = path.file_name().ok_or(LocalProcessError::ArtifactPath)?;
        let (bytes, identity, file) =
            read_regular_at(&pinned.parent, leaf, MAX_SOURCE_BYTES, false)?;
        drop(file);
        let resolved = open_pinned_parent(path, false)?;
        if resolved.parent.identity != pinned.parent.identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        let (current, current_identity, current_file) =
            read_regular_at(&resolved.parent, leaf, MAX_SOURCE_BYTES, false)?;
        drop(current_file);
        if current_identity != identity || current != bytes {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(bytes)
    }

    fn read_verified_pair(
        manifest_path: &Path,
        payload_path: &Path,
    ) -> Result<VerifiedArtifactPairV1, LocalProcessError> {
        lexical_absolute_path(manifest_path)?;
        lexical_absolute_path(payload_path)?;
        let manifest_parent = open_pinned_parent(manifest_path, false)?;
        let payload_parent = open_pinned_parent(payload_path, false)?;
        let manifest_name = manifest_path
            .file_name()
            .ok_or(LocalProcessError::ArtifactPath)?;
        let payload_name = payload_path
            .file_name()
            .ok_or(LocalProcessError::ArtifactPath)?;
        let (manifest, manifest_identity, manifest_file) =
            read_regular_at(&manifest_parent.parent, manifest_name, MANIFEST_BYTES, false)?;
        let (payload, payload_identity, payload_file) =
            read_regular_at(&payload_parent.parent, payload_name, MAX_SOURCE_BYTES, false)?;
        drop(payload_file);
        drop(manifest_file);

        let current_manifest_parent = open_pinned_parent(manifest_path, false)?;
        let current_payload_parent = open_pinned_parent(payload_path, false)?;
        if current_manifest_parent.parent.identity != manifest_parent.parent.identity
            || current_payload_parent.parent.identity != payload_parent.parent.identity
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        let (current_manifest, current_manifest_identity, current_manifest_file) = read_regular_at(
            &current_manifest_parent.parent,
            manifest_name,
            MANIFEST_BYTES,
            false,
        )?;
        let (current_payload, current_payload_identity, current_payload_file) = read_regular_at(
            &current_payload_parent.parent,
            payload_name,
            MAX_SOURCE_BYTES,
            false,
        )?;
        drop(current_payload_file);
        drop(current_manifest_file);
        if current_manifest_identity != manifest_identity
            || current_payload_identity != payload_identity
            || current_manifest != manifest
            || current_payload != payload
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        VerifiedArtifactPairV1::verify(&current_manifest, &current_payload)
            .map_err(|_| LocalProcessError::ArtifactCompatibility)
    }

    fn write_new_exact(
        parent: &DirectoryHandle,
        name: &str,
        bytes: &[u8],
    ) -> Result<FileIdentity, LocalProcessError> {
        let owned = openat(
            &parent.file,
            name,
            OFlag::O_WRONLY
                | OFlag::O_CREAT
                | OFlag::O_EXCL
                | OFlag::O_CLOEXEC
                | OFlag::O_NOFOLLOW,
            file_mode(),
        )
        .map_err(|_| LocalProcessError::ArtifactIo)?;
        let mut file = File::from(owned);
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| LocalProcessError::ArtifactIo)?;
        let metadata = file.metadata().map_err(|_| LocalProcessError::ArtifactIo)?;
        validate_regular_metadata(
            &metadata,
            true,
            Some(u64::try_from(bytes.len()).map_err(|_| LocalProcessError::ArtifactIo)?),
        )?;
        Ok(FileIdentity::from_metadata(&metadata))
    }

    fn verify_named_bytes(
        parent: &DirectoryHandle,
        name: &str,
        expected_identity: Option<FileIdentity>,
        expected: &[u8],
    ) -> Result<File, LocalProcessError> {
        let (bytes, identity, file) =
            read_regular_at(parent, OsStr::new(name), expected.len(), true)?;
        if bytes.as_ref() != expected || expected_identity.is_some_and(|value| value != identity) {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(file)
    }

    fn open_build_directory(
        parent: &DirectoryHandle,
        name: &OsStr,
    ) -> Result<DirectoryHandle, LocalProcessError> {
        let directory = open_directory_at(parent, name, true)?;
        exact_names(&directory, &[MANIFEST_NAME, PAYLOAD_NAME])?;
        Ok(directory)
    }

    fn settle_existing_final(
        pinned: &mut PinnedParent,
        output: &Path,
        output_leaf: &OsStr,
        manifest: &[u8],
        payload: &[u8],
    ) -> Result<(), ArtifactFailureV1> {
        let final_dir = open_build_directory(&pinned.parent, output_leaf)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let manifest_file = verify_named_bytes(&final_dir, MANIFEST_NAME, None, manifest)
            .map_err(|_| ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath))?;
        let payload_file = verify_named_bytes(&final_dir, PAYLOAD_NAME, None, payload)
            .map_err(|_| ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath))?;
        manifest_file
            .sync_all()
            .and_then(|()| payload_file.sync_all())
            .and_then(|()| final_dir.file.sync_all())
            .and_then(|()| pinned.parent.file.sync_all())
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let reopened_parent = reopen_parent(pinned)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let reopened_final = open_build_directory(&reopened_parent, output_leaf)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let resolved_parent = open_pinned_parent(output, true)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        if resolved_parent.parent.identity != reopened_parent.identity {
            return Err(ArtifactFailureV1::new(
                None,
                LocalProcessError::ArtifactUncertain,
            ));
        }
        let current_final = open_build_directory(&resolved_parent.parent, output_leaf)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        if current_final.identity != reopened_final.identity {
            return Err(ArtifactFailureV1::new(
                None,
                LocalProcessError::ArtifactUncertain,
            ));
        }
        verify_named_bytes(&current_final, MANIFEST_NAME, None, manifest)
            .and_then(|file| {
                drop(file);
                verify_named_bytes(&current_final, PAYLOAD_NAME, None, payload)
            })
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        pinned.parent = reopened_parent;
        Ok(())
    }

    fn revalidate_parent_and_absence(
        pinned: &PinnedParent,
        output: &Path,
        output_leaf: &OsStr,
        staging_name: &str,
    ) -> Result<(), LocalProcessError> {
        revalidate_directory(&pinned.parent, true)?;
        let path_parent = open_pinned_parent(output, true)?;
        if path_parent.parent.identity != pinned.parent.identity
            || named_directory_exists(&pinned.parent, output_leaf)?
            || named_directory_exists(&pinned.parent, OsStr::new(staging_name))?
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn revalidate_parent_for_rename(
        pinned: &PinnedParent,
        output: &Path,
        output_leaf: &OsStr,
        staging_name: &str,
    ) -> Result<(), LocalProcessError> {
        revalidate_directory(&pinned.parent, true)?;
        let path_parent = open_pinned_parent(output, true)?;
        if path_parent.parent.identity != pinned.parent.identity
            || named_directory_exists(&pinned.parent, output_leaf)?
            || !named_directory_exists(&pinned.parent, OsStr::new(staging_name))?
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok(())
    }

    fn publish_new_build(
        pinned: &mut PinnedParent,
        output: &Path,
        output_leaf: &OsStr,
        staging_name: &str,
        manifest: &[u8],
        payload: &[u8],
    ) -> Result<(), LocalProcessError> {
        pinned.parent.file.sync_all().map_err(|_| LocalProcessError::ArtifactIo)?;
        let staging = open_directory_at(&pinned.parent, OsStr::new(staging_name), true)?;
        exact_names(&staging, &[])?;
        let staging_identity = staging.identity;
        let manifest_identity = write_new_exact(&staging, MANIFEST_NAME, manifest)?;
        verify_named_bytes(&staging, MANIFEST_NAME, Some(manifest_identity), manifest)?;
        let payload_identity = write_new_exact(&staging, PAYLOAD_NAME, payload)?;
        verify_named_bytes(&staging, PAYLOAD_NAME, Some(payload_identity), payload)?;
        exact_names(&staging, &[MANIFEST_NAME, PAYLOAD_NAME])?;
        staging.file.sync_all().map_err(|_| LocalProcessError::ArtifactIo)?;
        drop(staging);
        let reopened_staging = open_directory_at(&pinned.parent, OsStr::new(staging_name), true)?;
        if reopened_staging.identity != staging_identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        exact_names(&reopened_staging, &[MANIFEST_NAME, PAYLOAD_NAME])?;
        verify_named_bytes(
            &reopened_staging,
            MANIFEST_NAME,
            Some(manifest_identity),
            manifest,
        )?;
        verify_named_bytes(
            &reopened_staging,
            PAYLOAD_NAME,
            Some(payload_identity),
            payload,
        )?;
        revalidate_parent_for_rename(pinned, output, output_leaf, staging_name)?;
        let rename_candidate =
            open_directory_at(&pinned.parent, OsStr::new(staging_name), true)?;
        if rename_candidate.identity != staging_identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        exact_names(&rename_candidate, &[MANIFEST_NAME, PAYLOAD_NAME])?;
        verify_named_bytes(
            &rename_candidate,
            MANIFEST_NAME,
            Some(manifest_identity),
            manifest,
        )?;
        verify_named_bytes(
            &rename_candidate,
            PAYLOAD_NAME,
            Some(payload_identity),
            payload,
        )?;
        renameat_with(
            &pinned.parent.file,
            staging_name,
            &pinned.parent.file,
            output_leaf,
            RenameFlags::NOREPLACE,
        )
        .map_err(|_| LocalProcessError::ArtifactIo)?;
        pinned.parent.file.sync_all().map_err(|_| LocalProcessError::ArtifactIo)?;
        let reopened_parent = reopen_parent(pinned)?;
        let final_dir = open_build_directory(&reopened_parent, output_leaf)?;
        let resolved_parent = open_pinned_parent(output, true)?;
        if resolved_parent.parent.identity != reopened_parent.identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        if final_dir.identity != staging_identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        let current_final = open_build_directory(&resolved_parent.parent, output_leaf)?;
        if current_final.identity != staging_identity
            || current_final.identity != final_dir.identity
        {
            return Err(LocalProcessError::ArtifactPath);
        }
        verify_named_bytes(
            &current_final,
            MANIFEST_NAME,
            Some(manifest_identity),
            manifest,
        )?;
        verify_named_bytes(
            &current_final,
            PAYLOAD_NAME,
            Some(payload_identity),
            payload,
        )?;
        pinned.parent = reopened_parent;
        Ok(())
    }
}

#[cfg(unix)]
use unix::{ensure_execution_identity, run_build, run_inspect, run_materialize, run_query};

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn pair_error_keeps_exact_key_order() {
        let mut output = Vec::new();
        write_pair_error(
            &mut output,
            "artifact.build",
            Some(false),
            LocalProcessError::ArtifactPath,
        )
        .expect("serialize exact pair error");
        assert_eq!(
            String::from_utf8(output).expect("UTF-8 JSON"),
            "{\"schema_version\":1,\"command\":\"artifact.build\",\"ok\":false,\"changed\":false,\"profile\":null,\"artifact_object_ref\":null,\"payload_length\":null,\"runtime_kind\":null,\"adapter_abi\":null,\"target_profile\":null,\"diagnostics\":[{\"code\":\"PXLC-ARTIFACT-PATH\",\"message\":\"artifact path is invalid or unsafe\"}]}\n"
        );
    }

    #[test]
    fn materialization_error_keeps_exact_key_order() {
        let mut output = Vec::new();
        write_materialization_error(
            &mut output,
            "artifact.materialization.query",
            Some(false),
            Some("a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2"),
            None,
            LocalProcessError::ArtifactNotFound,
        )
        .expect("serialize exact materialization error");
        assert_eq!(
            String::from_utf8(output).expect("UTF-8 JSON"),
            "{\"schema_version\":1,\"command\":\"artifact.materialization.query\",\"ok\":false,\"changed\":false,\"operation_id\":\"a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2\",\"state\":null,\"artifact_object_ref\":null,\"materialization_receipt_ref\":null,\"diagnostics\":[{\"code\":\"PXLC-ARTIFACT-NOT-FOUND\",\"message\":\"artifact operation was not found\"}]}\n"
        );
    }
}
