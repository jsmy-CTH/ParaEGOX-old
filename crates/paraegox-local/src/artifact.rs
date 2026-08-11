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

use paraegox_artifact::{
    ArtifactObjectRefV1, ArtifactOperationIdV1, MaterializationReceiptRefV1,
    VerifiedMaterializationReadBundleV1,
};
use serde::Serialize;

#[cfg(not(unix))]
use crate::config::ConfigError;
use crate::{
    config::{self, ArtifactCommandV1, ArtifactJsonIntentV1},
    error::LocalProcessError,
};

const ARTIFACT_OUTPUT_SCHEMA_VERSION: u16 = 1;
const PROFILE: &str = "developer-local-echo-prefix-v1";
const RUNTIME_KIND: &str = "managed_model_data_v1";
const ADAPTER_ABI: &str = "bounded-text-model-data-v1";
const TARGET_PROFILE: &str = "developer-local-managed-model-v1";

/// Reopens and verifies the exact ArtifactStore Receipt chain consumed by
/// D0b. This retains the existing config/authority adapter as the only local
/// bridge into `paraegox-artifact`; deployment code receives only the fully
/// owned verified bundle and never a store path or lock guard.
#[cfg(unix)]
pub(crate) fn read_verified_materialization_for_deployment(
    config_path: &Path,
    object_ref: ArtifactObjectRefV1,
    receipt_ref: MaterializationReceiptRefV1,
) -> Result<VerifiedMaterializationReadBundleV1, LocalProcessError> {
    unix::read_verified_materialization_for_deployment(config_path, object_ref, receipt_ref)
}

#[cfg(unix)]
pub(crate) fn ensure_deployment_execution_identity() -> Result<(), LocalProcessError> {
    unix::ensure_execution_identity()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArtifactFailureV1 {
    changed: Option<bool>,
    error: LocalProcessError,
}

#[cfg(any(unix, test))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ArtifactPairReadFailuresV1 {
    path: bool,
    profile: bool,
    compatibility: bool,
    io: bool,
}

#[cfg(any(unix, test))]
impl ArtifactPairReadFailuresV1 {
    fn record(&mut self, error: LocalProcessError) {
        match error {
            LocalProcessError::ArtifactPath => self.path = true,
            LocalProcessError::ArtifactProfile => self.profile = true,
            LocalProcessError::ArtifactCompatibility => self.compatibility = true,
            _ => self.io = true,
        }
    }

    const fn result(&self) -> Result<(), LocalProcessError> {
        if self.path {
            Err(LocalProcessError::ArtifactPath)
        } else if self.profile {
            Err(LocalProcessError::ArtifactProfile)
        } else if self.compatibility {
            Err(LocalProcessError::ArtifactCompatibility)
        } else if self.io {
            Err(LocalProcessError::ArtifactIo)
        } else {
            Ok(())
        }
    }
}

impl ArtifactFailureV1 {
    const fn new(changed: Option<bool>, error: LocalProcessError) -> Self {
        Self { changed, error }
    }
}

#[cfg(any(unix, test))]
const fn map_existing_build_read_failure(error: LocalProcessError) -> ArtifactFailureV1 {
    ArtifactFailureV1::new(
        Some(false),
        match error {
            LocalProcessError::ArtifactCompatibility | LocalProcessError::ArtifactPath => {
                LocalProcessError::ArtifactPath
            }
            _ => error,
        },
    )
}

#[cfg(unix)]
const fn map_artifact_path_errno(error: nix::errno::Errno) -> LocalProcessError {
    match error {
        nix::errno::Errno::ELOOP
        | nix::errno::Errno::ENOTDIR
        | nix::errno::Errno::ENOENT
        | nix::errno::Errno::ENAMETOOLONG
        | nix::errno::Errno::EACCES
        | nix::errno::Errno::EPERM => LocalProcessError::ArtifactPath,
        _ => LocalProcessError::ArtifactIo,
    }
}

#[cfg(unix)]
const fn map_artifact_presence_errno(error: nix::errno::Errno) -> Result<bool, LocalProcessError> {
    if matches!(error, nix::errno::Errno::ENOENT) {
        Ok(false)
    } else {
        Err(map_artifact_path_errno(error))
    }
}

#[cfg(unix)]
fn map_artifact_metadata_error(error: std::io::Error) -> LocalProcessError {
    error
        .raw_os_error()
        .map_or(LocalProcessError::ArtifactIo, |raw| {
            map_artifact_path_errno(nix::errno::Errno::from_raw(raw))
        })
}

#[cfg(unix)]
fn artifact_regular_read_flags(
    inspect_noatime: bool,
) -> Result<nix::fcntl::OFlag, LocalProcessError> {
    let flags =
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_CLOEXEC | nix::fcntl::OFlag::O_NOFOLLOW;
    if !inspect_noatime {
        return Ok(flags);
    }
    #[cfg(target_os = "linux")]
    {
        Ok(flags | nix::fcntl::OFlag::O_NOATIME)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LocalProcessError::ArtifactIo)
    }
}

#[cfg(any(unix, test))]
const fn classify_build_pre_effect_failure(
    path_failed: bool,
    compatibility_failed: bool,
    staging_present: bool,
    io_failed: bool,
) -> Option<ArtifactFailureV1> {
    if path_failed {
        Some(ArtifactFailureV1::new(
            Some(false),
            LocalProcessError::ArtifactPath,
        ))
    } else if compatibility_failed {
        Some(ArtifactFailureV1::new(
            Some(false),
            LocalProcessError::ArtifactCompatibility,
        ))
    } else if staging_present {
        Some(ArtifactFailureV1::new(
            Some(false),
            LocalProcessError::ArtifactUncertain,
        ))
    } else if io_failed {
        Some(ArtifactFailureV1::new(
            Some(false),
            LocalProcessError::ArtifactIo,
        ))
    } else {
        None
    }
}

#[cfg(any(unix, test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildMkdirEvidenceV1 {
    NotChecked,
    KnownStage,
    Structural,
    Unknown,
}

#[cfg(unix)]
const fn classify_build_mkdir_failure(
    error: nix::errno::Errno,
    evidence: BuildMkdirEvidenceV1,
) -> ArtifactFailureV1 {
    if matches!(error, nix::errno::Errno::EEXIST) {
        match evidence {
            BuildMkdirEvidenceV1::KnownStage => {
                ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactUncertain)
            }
            BuildMkdirEvidenceV1::Structural => {
                ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath)
            }
            BuildMkdirEvidenceV1::NotChecked | BuildMkdirEvidenceV1::Unknown => {
                ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain)
            }
        }
    } else if matches!(
        map_artifact_path_errno(error),
        LocalProcessError::ArtifactPath
    ) {
        ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath)
    } else {
        ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain)
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
    let preparsed_operation_id = config::artifact_preparsed_operation_id(intent, arguments);
    let command = match config::parse_artifact_command(intent, arguments) {
        Ok(command) => command,
        Err(error) => {
            return write_preparse_error(
                output,
                intent,
                preparsed_operation_id,
                LocalProcessError::Configuration(error),
            );
        }
    };

    #[cfg(not(unix))]
    {
        let operation_id = command_operation_id(&command);
        let _ = command;
        write_preparse_error(
            output,
            intent,
            operation_id,
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
    operation_id: Option<ArtifactOperationIdV1>,
    error: LocalProcessError,
) -> u8 {
    let exit_code = error.exit_code();
    let operation_id = operation_id.map(operation_id_text);
    let result = match intent {
        ArtifactJsonIntentV1::Build | ArtifactJsonIntentV1::Inspect => {
            write_pair_error(output, intent.command(), Some(false), error)
        }
        ArtifactJsonIntentV1::Materialize | ArtifactJsonIntentV1::MaterializationQuery => {
            write_materialization_error(
                output,
                intent.command(),
                Some(false),
                operation_id.as_deref(),
                None,
                error,
            )
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
    serde_json::to_writer(&mut *output, value)
        .map_err(|_| LocalProcessError::ArtifactJsonOutput)?;
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
    #[cfg(not(target_os = "linux"))]
    if matches!(intent, ArtifactJsonIntentV1::Inspect) {
        return write_preparse_error(
            output,
            intent,
            command_operation_id(&command),
            LocalProcessError::Configuration(config::ConfigError::UnsupportedPlatform),
        );
    }

    if let Err(error) = ensure_execution_identity() {
        return write_preparse_error(output, intent, command_operation_id(&command), error);
    }

    match command {
        ArtifactCommandV1::Build {
            source,
            output: path,
        } => {
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

fn command_operation_id(command: &ArtifactCommandV1) -> Option<ArtifactOperationIdV1> {
    match command {
        ArtifactCommandV1::Materialize { operation_id, .. }
        | ArtifactCommandV1::MaterializationQuery { operation_id, .. } => Some(*operation_id),
        ArtifactCommandV1::Build { .. } | ArtifactCommandV1::Inspect { .. } => None,
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
            let exit_code = projection.error.map_or(0, LocalProcessError::exit_code);
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
    let value = value.to_str().ok_or(LocalProcessError::ArtifactPath)?;
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
        ArtifactManifestProfileClassificationV1, ArtifactManifestV1, ArtifactObjectRefV1,
        ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1,
        ArtifactStoreAuthorityV1, ArtifactStoreChangeV1, ArtifactStoreFailureV1,
        ArtifactStoreInvocationV1, ArtifactStoreOperationStateV1, ArtifactStoreOperationViewV1,
        ArtifactStoreV1, MaterializationOperationV1, MaterializationReceiptRefV1,
        MaterializationRequestV1, VerifiedArtifactPairV1, VerifiedMaterializationReadBundleV1,
    };
    use rustix::fs::{RenameFlags, renameat_with};
    use sha2::{Digest as _, Sha256};

    use super::{
        ArtifactFailureV1, ArtifactMaterializationProjectionV1, ArtifactPairProjectionV1,
        ArtifactPairReadFailuresV1, BuildMkdirEvidenceV1, artifact_path_from_os,
        artifact_regular_read_flags, classify_build_mkdir_failure,
        classify_build_pre_effect_failure, lexical_absolute_path, lower_hex,
        map_artifact_metadata_error, map_artifact_path_errno, map_artifact_presence_errno,
        map_existing_build_read_failure, operation_id_text,
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

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BuildPublicationPreconditionV1 {
        Empty,
        FinalAppeared,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum MaterializeAuthorityPreflightV1 {
        Continue,
        Immediate,
        Deferred,
    }

    pub(super) fn classify_materialize_authority_preflight(
        failure: Option<&ArtifactStoreFailureV1>,
    ) -> MaterializeAuthorityPreflightV1 {
        match failure {
            None | Some(ArtifactStoreFailureV1::NotFound) => {
                MaterializeAuthorityPreflightV1::Continue
            }
            Some(ArtifactStoreFailureV1::PublicationUncertain { .. }) => {
                MaterializeAuthorityPreflightV1::Continue
            }
            Some(
                ArtifactStoreFailureV1::UnsafePath | ArtifactStoreFailureV1::ConfigurationMismatch,
            ) => MaterializeAuthorityPreflightV1::Immediate,
            Some(
                ArtifactStoreFailureV1::Conflict
                | ArtifactStoreFailureV1::Capacity
                | ArtifactStoreFailureV1::Contended
                | ArtifactStoreFailureV1::Owner
                | ArtifactStoreFailureV1::Io,
            ) => MaterializeAuthorityPreflightV1::Deferred,
        }
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
        if staging_name.len() != STAGING_NAME_BYTES || OsStr::new(&staging_name) == output_leaf {
            return Err(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactPath,
            ));
        }
        let mut output_io_failed = false;
        let mut pinned = match open_pinned_parent(&output, true) {
            Ok(pinned) => Some(pinned),
            Err(LocalProcessError::ArtifactIo) => {
                output_io_failed = true;
                None
            }
            Err(error) => return Err(ArtifactFailureV1::new(Some(false), error)),
        };
        let mut final_exists = None;
        let mut staging_exists = None;
        if let Some(output_parent) = pinned.as_ref() {
            if let Err(error) = validate_native_path_limits(
                &output_parent.parent,
                output_text,
                output_leaf,
                &staging_name,
            ) {
                if error == LocalProcessError::ArtifactIo {
                    output_io_failed = true;
                } else {
                    return Err(ArtifactFailureV1::new(Some(false), error));
                }
            }
            match named_directory_exists(&output_parent.parent, output_leaf) {
                Ok(exists) => final_exists = Some(exists),
                Err(LocalProcessError::ArtifactIo) => output_io_failed = true,
                Err(error) => return Err(ArtifactFailureV1::new(Some(false), error)),
            }
            match named_directory_exists(&output_parent.parent, OsStr::new(&staging_name)) {
                Ok(exists) => staging_exists = Some(exists),
                Err(LocalProcessError::ArtifactIo) => output_io_failed = true,
                Err(error) => return Err(ArtifactFailureV1::new(Some(false), error)),
            }
        }
        let mut final_path_failed = false;
        let mut final_io_failed = false;
        let mut final_pair = None;
        if matches!(final_exists, Some(true)) {
            match probe_existing_final(
                &pinned
                    .as_ref()
                    .expect("observed final has pinned output parent")
                    .parent,
                output_leaf,
            ) {
                Ok(pair) => final_pair = Some(pair),
                Err(LocalProcessError::ArtifactPath) => final_path_failed = true,
                Err(_) => final_io_failed = true,
            }
        }

        let mut source_path_failed = false;
        let mut compatibility_failed = false;
        let mut source_io_failed = false;
        let payload = match read_source(&source) {
            Ok(payload) => Some(payload),
            Err(LocalProcessError::ArtifactPath) => {
                source_path_failed = true;
                None
            }
            Err(LocalProcessError::ArtifactCompatibility) => {
                compatibility_failed = true;
                None
            }
            Err(_) => {
                source_io_failed = true;
                None
            }
        };
        let pair = if let Some(payload) = payload.as_deref() {
            match VerifiedArtifactPairV1::from_payload(payload) {
                Ok(pair) => Some(pair),
                Err(_) => {
                    compatibility_failed = true;
                    None
                }
            }
        } else {
            None
        };
        if final_pair.is_some() && compatibility_failed {
            final_path_failed = true;
        }
        if let (Some(existing), Some(requested)) = (final_pair.as_ref(), pair.as_ref())
            && (existing.manifest_bytes() != requested.manifest_bytes()
                || existing.payload() != requested.payload())
        {
            final_path_failed = true;
        }
        if let Some(failure) = classify_build_pre_effect_failure(
            source_path_failed || final_path_failed,
            compatibility_failed,
            matches!(staging_exists, Some(true)),
            output_io_failed || source_io_failed || final_io_failed,
        ) {
            return Err(failure);
        }
        let pair = pair.expect("successful build preflight has verified pair");
        let mut pinned = pinned
            .take()
            .expect("successful build preflight has output parent");
        let final_exists = final_exists.expect("successful build preflight has final observation");
        let expected_manifest = pair.manifest_bytes();

        if final_exists {
            settle_existing_final(
                &mut pinned,
                &output,
                output_leaf,
                &staging_name,
                expected_manifest,
                pair.payload(),
            )?;
            return Ok(pair_projection(false, &pair));
        }

        if revalidate_parent_for_publication(
            &pinned,
            &output,
            output_leaf,
            &staging_name,
            expected_manifest,
            pair.payload(),
        )
        .map_err(|error| ArtifactFailureV1::new(Some(false), error))?
            == BuildPublicationPreconditionV1::FinalAppeared
        {
            settle_existing_final(
                &mut pinned,
                &output,
                output_leaf,
                &staging_name,
                expected_manifest,
                pair.payload(),
            )?;
            return Ok(pair_projection(false, &pair));
        }
        if let Err(error) = mkdirat(&pinned.parent.file, staging_name.as_str(), directory_mode()) {
            let evidence = if error == nix::errno::Errno::EEXIST {
                match observe_build_precondition(
                    &pinned.parent,
                    output_leaf,
                    &staging_name,
                    expected_manifest,
                    pair.payload(),
                ) {
                    Err(LocalProcessError::ArtifactUncertain) => BuildMkdirEvidenceV1::KnownStage,
                    Err(LocalProcessError::ArtifactPath) => BuildMkdirEvidenceV1::Structural,
                    Ok(_) | Err(_) => BuildMkdirEvidenceV1::Unknown,
                }
            } else {
                BuildMkdirEvidenceV1::NotChecked
            };
            return Err(classify_build_mkdir_failure(error, evidence));
        }

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
        let pair = read_verified_pair(&manifest, &payload, true, false)
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
        let mut authority = RevalidatingArtifactAuthority::new(&config);
        let authority_preflight = ArtifactStoreV1::query(&mut authority, operation_id);
        let deferred_authority =
            match classify_materialize_authority_preflight(authority_preflight.result().err()) {
                MaterializeAuthorityPreflightV1::Continue => None,
                MaterializeAuthorityPreflightV1::Immediate => {
                    drop(authority);
                    return Ok(project_invocation(operation_id, authority_preflight));
                }
                MaterializeAuthorityPreflightV1::Deferred => Some(authority_preflight),
            };
        lexical_absolute_path(manifest)
            .and_then(|()| lexical_absolute_path(payload))
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let pair = read_verified_pair(manifest, payload, false, true)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        if let Some(invocation) = deferred_authority {
            drop(authority);
            drop(pair);
            return Ok(project_invocation(operation_id, invocation));
        }
        let request = MaterializationRequestV1::new(
            operation_id,
            config.config_commitment(),
            pair.object_ref(),
        );
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

    pub(super) fn read_verified_materialization_for_deployment(
        config_path: &Path,
        object_ref: ArtifactObjectRefV1,
        receipt_ref: MaterializationReceiptRefV1,
    ) -> Result<VerifiedMaterializationReadBundleV1, LocalProcessError> {
        let config = config::parse_artifact_store_authority_config(config_path)?;
        let mut authority = RevalidatingArtifactAuthority::new(&config);
        let result = ArtifactStoreV1::read_verified(&mut authority, object_ref, receipt_ref);
        drop(authority);
        result.map_err(|failure| match failure {
            paraegox_artifact::ArtifactStoreReadFailureV1::ConfigurationMismatch => {
                LocalProcessError::LifecycleConfiguration
            }
            paraegox_artifact::ArtifactStoreReadFailureV1::ReferenceMismatch
            | paraegox_artifact::ArtifactStoreReadFailureV1::NotFound => {
                LocalProcessError::ArtifactExternalDeployMaterializationReceipt
            }
            paraegox_artifact::ArtifactStoreReadFailureV1::Contended
            | paraegox_artifact::ArtifactStoreReadFailureV1::Owner
            | paraegox_artifact::ArtifactStoreReadFailureV1::Io => {
                LocalProcessError::ArtifactExternalDeployOwner
            }
        })
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
        ) -> Result<ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1>
        {
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
            ConfigError::InvalidStateRoot | ConfigError::StateRootTooLong => {
                ArtifactStoreAuthorityRecheckFailureV1::UnsafePath
            }
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
            ArtifactStoreOperationStateV1::Uncertain => Some(LocalProcessError::ArtifactUncertain),
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
            ArtifactStoreFailureV1::ConfigurationMismatch => {
                (None, None, None, LocalProcessError::LifecycleConfiguration)
            }
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
            ArtifactStoreFailureV1::Owner => (None, None, None, LocalProcessError::ArtifactOwner),
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
        digest.update(
            u64::try_from(output.len())
                .expect("path bound fits u64")
                .to_be_bytes(),
        );
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
        let metadata = file.metadata().map_err(map_artifact_metadata_error)?;
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
                && (metadata.uid() != uid || metadata.gid() != gid || mode != DIRECTORY_MODE_BITS))
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
        .map_err(map_artifact_path_errno)?;
        directory_from_owned(owned, strict)
    }

    fn revalidate_directory(
        directory: &DirectoryHandle,
        strict: bool,
    ) -> Result<(), LocalProcessError> {
        let metadata = directory
            .file
            .metadata()
            .map_err(map_artifact_metadata_error)?;
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
        .map_err(map_artifact_path_errno)?;
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
            Err(error) => map_artifact_presence_errno(error),
        }
    }

    fn validate_native_path_limits(
        parent: &DirectoryHandle,
        output: &str,
        leaf: &OsStr,
        staging: &str,
    ) -> Result<(), LocalProcessError> {
        let name_max = fpathconf(&parent.file, PathconfVar::NAME_MAX)
            .map_err(map_artifact_path_errno)?
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(LocalProcessError::ArtifactPath)?;
        let path_max = fpathconf(&parent.file, PathconfVar::PATH_MAX)
            .map_err(map_artifact_path_errno)?
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(LocalProcessError::ArtifactPath)?;
        if leaf.as_bytes().len() > name_max
            || staging.len() > name_max
            || output.len() >= path_max
            || output
                .rfind('/')
                .and_then(|separator| separator.checked_add(1)?.checked_add(staging.len()))
                .is_none_or(|length| length >= path_max)
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
        .map_err(map_artifact_path_errno)?;
        let mut stream = Dir::from_fd(owned).map_err(map_artifact_path_errno)?;
        let mut names = BTreeSet::new();
        for entry in stream.iter() {
            let entry = entry.map_err(map_artifact_path_errno)?;
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
        read_regular_at_with_mode(parent, name, maximum, strict_mode, false)
    }

    fn read_regular_at_with_mode(
        parent: &DirectoryHandle,
        name: &OsStr,
        maximum: usize,
        strict_mode: bool,
        inspect_noatime: bool,
    ) -> Result<(Box<[u8]>, FileIdentity, File), LocalProcessError> {
        let flags = artifact_regular_read_flags(inspect_noatime)?;
        let owned =
            openat(&parent.file, name, flags, Mode::empty()).map_err(map_artifact_path_errno)?;
        let mut file = File::from(owned);
        let metadata = file.metadata().map_err(map_artifact_metadata_error)?;
        validate_regular_metadata(&metadata, strict_mode, None)?;
        let identity = FileIdentity::from_metadata(&metadata);
        let mut bytes = Vec::with_capacity(maximum.min(4096));
        std::io::Read::by_ref(&mut file)
            .take(u64::try_from(maximum + 1).expect("small bound"))
            .read_to_end(&mut bytes)
            .map_err(|_| LocalProcessError::ArtifactIo)?;
        if bytes.len() > maximum {
            return Err(LocalProcessError::ArtifactCompatibility);
        }
        Ok((bytes.into_boxed_slice(), identity, file))
    }

    fn read_pair_member(
        parent: &DirectoryHandle,
        name: &OsStr,
        maximum: usize,
        strict_mode: bool,
        inspect_noatime: bool,
    ) -> Result<(Box<[u8]>, FileIdentity), LocalProcessError> {
        let (bytes, identity, file) =
            read_regular_at_with_mode(parent, name, maximum, strict_mode, inspect_noatime)?;
        drop(file);
        Ok((bytes, identity))
    }

    fn collect_pair_evidence<T>(
        result: Result<T, LocalProcessError>,
        failures: &mut ArtifactPairReadFailuresV1,
    ) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                failures.record(error);
                None
            }
        }
    }

    fn record_manifest_contract(
        manifest: Option<&(Box<[u8]>, FileIdentity)>,
        classify_profile: bool,
        failures: &mut ArtifactPairReadFailuresV1,
    ) {
        let Some((bytes, _)) = manifest else {
            return;
        };
        let profile = ArtifactManifestV1::classify_profile(bytes);
        if profile == ArtifactManifestProfileClassificationV1::Mismatch && classify_profile {
            failures.record(LocalProcessError::ArtifactProfile);
        } else if profile == ArtifactManifestProfileClassificationV1::Mismatch
            || ArtifactManifestV1::decode(bytes).is_err()
        {
            failures.record(LocalProcessError::ArtifactCompatibility);
        }
    }

    fn record_payload_contract(
        payload: Option<&(Box<[u8]>, FileIdentity)>,
        failures: &mut ArtifactPairReadFailuresV1,
    ) {
        if payload.is_some_and(|(bytes, _)| ArtifactManifestV1::from_payload(bytes).is_err()) {
            failures.record(LocalProcessError::ArtifactCompatibility);
        }
    }

    fn record_pair_contract(
        manifest: Option<&(Box<[u8]>, FileIdentity)>,
        payload: Option<&(Box<[u8]>, FileIdentity)>,
        failures: &mut ArtifactPairReadFailuresV1,
    ) -> Option<VerifiedArtifactPairV1> {
        let (Some((manifest, _)), Some((payload, _))) = (manifest, payload) else {
            return None;
        };
        match VerifiedArtifactPairV1::verify(manifest, payload) {
            Ok(pair) => Some(pair),
            Err(_) => {
                failures.record(LocalProcessError::ArtifactCompatibility);
                None
            }
        }
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
        inspect_noatime: bool,
        classify_profile: bool,
    ) -> Result<VerifiedArtifactPairV1, LocalProcessError> {
        let mut failures = ArtifactPairReadFailuresV1::default();
        if let Err(error) = lexical_absolute_path(manifest_path) {
            failures.record(error);
        }
        if let Err(error) = lexical_absolute_path(payload_path) {
            failures.record(error);
        }
        failures.result()?;

        let manifest_name = manifest_path
            .file_name()
            .expect("lexically valid absolute manifest has a leaf");
        let payload_name = payload_path
            .file_name()
            .expect("lexically valid absolute payload has a leaf");
        let manifest_parent =
            collect_pair_evidence(open_pinned_parent(manifest_path, false), &mut failures);
        let payload_parent =
            collect_pair_evidence(open_pinned_parent(payload_path, false), &mut failures);
        let manifest = manifest_parent.as_ref().and_then(|parent| {
            collect_pair_evidence(
                read_pair_member(
                    &parent.parent,
                    manifest_name,
                    MANIFEST_BYTES,
                    false,
                    inspect_noatime,
                ),
                &mut failures,
            )
        });
        let payload = payload_parent.as_ref().and_then(|parent| {
            collect_pair_evidence(
                read_pair_member(
                    &parent.parent,
                    payload_name,
                    MAX_SOURCE_BYTES,
                    false,
                    inspect_noatime,
                ),
                &mut failures,
            )
        });
        record_manifest_contract(manifest.as_ref(), classify_profile, &mut failures);
        record_payload_contract(payload.as_ref(), &mut failures);
        let _ = record_pair_contract(manifest.as_ref(), payload.as_ref(), &mut failures);

        let current_manifest_parent = manifest_parent.as_ref().and_then(|parent| {
            let current =
                collect_pair_evidence(open_pinned_parent(manifest_path, false), &mut failures);
            if current
                .as_ref()
                .is_some_and(|value| value.parent.identity != parent.parent.identity)
            {
                failures.record(LocalProcessError::ArtifactPath);
            }
            current
        });
        let current_payload_parent = payload_parent.as_ref().and_then(|parent| {
            let current =
                collect_pair_evidence(open_pinned_parent(payload_path, false), &mut failures);
            if current
                .as_ref()
                .is_some_and(|value| value.parent.identity != parent.parent.identity)
            {
                failures.record(LocalProcessError::ArtifactPath);
            }
            current
        });
        let current_manifest = current_manifest_parent.as_ref().and_then(|parent| {
            collect_pair_evidence(
                read_pair_member(
                    &parent.parent,
                    manifest_name,
                    MANIFEST_BYTES,
                    false,
                    inspect_noatime,
                ),
                &mut failures,
            )
        });
        let current_payload = current_payload_parent.as_ref().and_then(|parent| {
            collect_pair_evidence(
                read_pair_member(
                    &parent.parent,
                    payload_name,
                    MAX_SOURCE_BYTES,
                    false,
                    inspect_noatime,
                ),
                &mut failures,
            )
        });
        record_manifest_contract(current_manifest.as_ref(), classify_profile, &mut failures);
        record_payload_contract(current_payload.as_ref(), &mut failures);
        let verified = record_pair_contract(
            current_manifest.as_ref(),
            current_payload.as_ref(),
            &mut failures,
        );

        if let (Some((initial, initial_identity)), Some((current, current_identity))) =
            (manifest.as_ref(), current_manifest.as_ref())
            && (current_identity != initial_identity || current != initial)
        {
            failures.record(LocalProcessError::ArtifactPath);
        }
        if let (Some((initial, initial_identity)), Some((current, current_identity))) =
            (payload.as_ref(), current_payload.as_ref())
            && (current_identity != initial_identity || current != initial)
        {
            failures.record(LocalProcessError::ArtifactPath);
        }
        failures.result()?;
        Ok(verified.expect("successful pair preflight has verified current pair"))
    }

    fn write_new_exact(
        parent: &DirectoryHandle,
        name: &str,
        bytes: &[u8],
    ) -> Result<FileIdentity, LocalProcessError> {
        let owned = openat(
            &parent.file,
            name,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
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
    ) -> Result<(File, FileIdentity), LocalProcessError> {
        let (bytes, identity, file) =
            read_regular_at(parent, OsStr::new(name), expected.len(), true)?;
        if bytes.as_ref() != expected || expected_identity.is_some_and(|value| value != identity) {
            return Err(LocalProcessError::ArtifactPath);
        }
        Ok((file, identity))
    }

    fn open_build_directory(
        parent: &DirectoryHandle,
        name: &OsStr,
    ) -> Result<DirectoryHandle, LocalProcessError> {
        let directory = open_directory_at(parent, name, true)?;
        exact_names(&directory, &[MANIFEST_NAME, PAYLOAD_NAME])?;
        Ok(directory)
    }

    fn probe_existing_final(
        parent: &DirectoryHandle,
        output_leaf: &OsStr,
    ) -> Result<VerifiedArtifactPairV1, LocalProcessError> {
        let final_dir = open_build_directory(parent, output_leaf)?;
        let mut failures = ArtifactPairReadFailuresV1::default();
        let manifest = collect_pair_evidence(
            read_pair_member(
                &final_dir,
                OsStr::new(MANIFEST_NAME),
                MANIFEST_BYTES,
                true,
                false,
            )
            .map_err(|error| map_existing_build_read_failure(error).error),
            &mut failures,
        );
        let payload = collect_pair_evidence(
            read_pair_member(
                &final_dir,
                OsStr::new(PAYLOAD_NAME),
                MAX_SOURCE_BYTES,
                true,
                false,
            )
            .map_err(|error| map_existing_build_read_failure(error).error),
            &mut failures,
        );
        if manifest
            .as_ref()
            .is_some_and(|(bytes, _)| ArtifactManifestV1::decode(bytes).is_err())
        {
            failures.record(LocalProcessError::ArtifactPath);
        }
        if payload
            .as_ref()
            .is_some_and(|(bytes, _)| ArtifactManifestV1::from_payload(bytes).is_err())
        {
            failures.record(LocalProcessError::ArtifactPath);
        }
        let verified = match (manifest.as_ref(), payload.as_ref()) {
            (Some((manifest, _)), Some((payload, _))) => {
                match VerifiedArtifactPairV1::verify(manifest, payload) {
                    Ok(pair) => Some(pair),
                    Err(_) => {
                        failures.record(LocalProcessError::ArtifactPath);
                        None
                    }
                }
            }
            _ => None,
        };
        failures.result()?;
        Ok(verified.expect("successful final probe has verified pair"))
    }

    fn observe_build_precondition(
        parent: &DirectoryHandle,
        output_leaf: &OsStr,
        staging_name: &str,
        expected_manifest: &[u8],
        expected_payload: &[u8],
    ) -> Result<BuildPublicationPreconditionV1, LocalProcessError> {
        let mut path_failed = false;
        let mut io_failed = false;
        let final_exists = match named_directory_exists(parent, output_leaf) {
            Ok(value) => Some(value),
            Err(LocalProcessError::ArtifactPath) => {
                path_failed = true;
                None
            }
            Err(_) => {
                io_failed = true;
                None
            }
        };
        let staging_exists = match named_directory_exists(parent, OsStr::new(staging_name)) {
            Ok(value) => Some(value),
            Err(LocalProcessError::ArtifactPath) => {
                path_failed = true;
                None
            }
            Err(_) => {
                io_failed = true;
                None
            }
        };
        if matches!(final_exists, Some(true)) {
            match probe_existing_final(parent, output_leaf) {
                Ok(pair)
                    if pair.manifest_bytes() == expected_manifest
                        && pair.payload() == expected_payload => {}
                Ok(_) | Err(LocalProcessError::ArtifactPath) => path_failed = true,
                Err(_) => io_failed = true,
            }
        }
        if path_failed {
            Err(LocalProcessError::ArtifactPath)
        } else if matches!(staging_exists, Some(true)) {
            Err(LocalProcessError::ArtifactUncertain)
        } else if io_failed {
            Err(LocalProcessError::ArtifactIo)
        } else if matches!(final_exists, Some(true)) {
            Ok(BuildPublicationPreconditionV1::FinalAppeared)
        } else {
            Ok(BuildPublicationPreconditionV1::Empty)
        }
    }

    fn require_build_staging_absent(
        parent: &DirectoryHandle,
        staging_name: &str,
    ) -> Result<(), LocalProcessError> {
        if named_directory_exists(parent, OsStr::new(staging_name))? {
            return Err(LocalProcessError::ArtifactUncertain);
        }
        Ok(())
    }

    fn settle_existing_final(
        pinned: &mut PinnedParent,
        output: &Path,
        output_leaf: &OsStr,
        staging_name: &str,
        manifest: &[u8],
        payload: &[u8],
    ) -> Result<(), ArtifactFailureV1> {
        if observe_build_precondition(&pinned.parent, output_leaf, staging_name, manifest, payload)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?
            != BuildPublicationPreconditionV1::FinalAppeared
        {
            return Err(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactPath,
            ));
        }
        let final_dir = open_build_directory(&pinned.parent, output_leaf)
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let final_identity = final_dir.identity;
        let mut failures = ArtifactPairReadFailuresV1::default();
        let manifest_read = collect_pair_evidence(
            verify_named_bytes(&final_dir, MANIFEST_NAME, None, manifest)
                .map_err(|error| map_existing_build_read_failure(error).error),
            &mut failures,
        );
        let payload_read = collect_pair_evidence(
            verify_named_bytes(&final_dir, PAYLOAD_NAME, None, payload)
                .map_err(|error| map_existing_build_read_failure(error).error),
            &mut failures,
        );
        failures
            .result()
            .map_err(|error| ArtifactFailureV1::new(Some(false), error))?;
        let (manifest_file, manifest_identity) =
            manifest_read.expect("successful replay preflight has manifest");
        let (payload_file, payload_identity) =
            payload_read.expect("successful replay preflight has payload");
        manifest_file
            .sync_all()
            .and_then(|()| payload_file.sync_all())
            .and_then(|()| final_dir.file.sync_all())
            .and_then(|()| pinned.parent.file.sync_all())
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let reopened_parent = reopen_parent(pinned)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        require_build_staging_absent(&reopened_parent, staging_name)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let reopened_final = open_build_directory(&reopened_parent, output_leaf)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        if reopened_final.identity != final_identity {
            return Err(ArtifactFailureV1::new(
                None,
                LocalProcessError::ArtifactUncertain,
            ));
        }
        verify_named_bytes(
            &reopened_final,
            MANIFEST_NAME,
            Some(manifest_identity),
            manifest,
        )
        .and_then(|_| {
            verify_named_bytes(
                &reopened_final,
                PAYLOAD_NAME,
                Some(payload_identity),
                payload,
            )
        })
        .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let resolved_parent = open_pinned_parent(output, true)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        if resolved_parent.parent.identity != reopened_parent.identity {
            return Err(ArtifactFailureV1::new(
                None,
                LocalProcessError::ArtifactUncertain,
            ));
        }
        require_build_staging_absent(&resolved_parent.parent, staging_name)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        let current_final = open_build_directory(&resolved_parent.parent, output_leaf)
            .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        if current_final.identity != final_identity
            || current_final.identity != reopened_final.identity
        {
            return Err(ArtifactFailureV1::new(
                None,
                LocalProcessError::ArtifactUncertain,
            ));
        }
        verify_named_bytes(
            &current_final,
            MANIFEST_NAME,
            Some(manifest_identity),
            manifest,
        )
        .and_then(|_| {
            verify_named_bytes(
                &current_final,
                PAYLOAD_NAME,
                Some(payload_identity),
                payload,
            )
        })
        .map_err(|_| ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain))?;
        pinned.parent = reopened_parent;
        Ok(())
    }

    fn revalidate_parent_for_publication(
        pinned: &PinnedParent,
        output: &Path,
        output_leaf: &OsStr,
        staging_name: &str,
        manifest: &[u8],
        payload: &[u8],
    ) -> Result<BuildPublicationPreconditionV1, LocalProcessError> {
        revalidate_directory(&pinned.parent, true)?;
        let path_parent = open_pinned_parent(output, true)?;
        if path_parent.parent.identity != pinned.parent.identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        observe_build_precondition(&pinned.parent, output_leaf, staging_name, manifest, payload)
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
        pinned
            .parent
            .file
            .sync_all()
            .map_err(|_| LocalProcessError::ArtifactIo)?;
        let staging = open_directory_at(&pinned.parent, OsStr::new(staging_name), true)?;
        exact_names(&staging, &[])?;
        let staging_identity = staging.identity;
        let manifest_identity = write_new_exact(&staging, MANIFEST_NAME, manifest)?;
        verify_named_bytes(&staging, MANIFEST_NAME, Some(manifest_identity), manifest)?;
        let payload_identity = write_new_exact(&staging, PAYLOAD_NAME, payload)?;
        verify_named_bytes(&staging, PAYLOAD_NAME, Some(payload_identity), payload)?;
        exact_names(&staging, &[MANIFEST_NAME, PAYLOAD_NAME])?;
        staging
            .file
            .sync_all()
            .map_err(|_| LocalProcessError::ArtifactIo)?;
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
        let rename_candidate = open_directory_at(&pinned.parent, OsStr::new(staging_name), true)?;
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
        pinned
            .parent
            .file
            .sync_all()
            .map_err(|_| LocalProcessError::ArtifactIo)?;
        let reopened_parent = reopen_parent(pinned)?;
        require_build_staging_absent(&reopened_parent, staging_name)?;
        let final_dir = open_build_directory(&reopened_parent, output_leaf)?;
        let resolved_parent = open_pinned_parent(output, true)?;
        if resolved_parent.parent.identity != reopened_parent.identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        if final_dir.identity != staging_identity {
            return Err(LocalProcessError::ArtifactPath);
        }
        require_build_staging_absent(&resolved_parent.parent, staging_name)?;
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
    fn pair_read_failure_order_is_path_profile_compatibility_then_io() {
        let cases = [
            (
                [
                    LocalProcessError::ArtifactIo,
                    LocalProcessError::ArtifactCompatibility,
                    LocalProcessError::ArtifactProfile,
                    LocalProcessError::ArtifactPath,
                ],
                LocalProcessError::ArtifactPath,
            ),
            (
                [
                    LocalProcessError::ArtifactIo,
                    LocalProcessError::ArtifactCompatibility,
                    LocalProcessError::ArtifactProfile,
                    LocalProcessError::ArtifactProfile,
                ],
                LocalProcessError::ArtifactProfile,
            ),
            (
                [
                    LocalProcessError::ArtifactIo,
                    LocalProcessError::ArtifactCompatibility,
                    LocalProcessError::ArtifactCompatibility,
                    LocalProcessError::ArtifactIo,
                ],
                LocalProcessError::ArtifactCompatibility,
            ),
            (
                [
                    LocalProcessError::ArtifactIo,
                    LocalProcessError::ArtifactIo,
                    LocalProcessError::ArtifactIo,
                    LocalProcessError::ArtifactIo,
                ],
                LocalProcessError::ArtifactIo,
            ),
        ];
        for (inputs, expected) in cases {
            let mut failures = ArtifactPairReadFailuresV1::default();
            for input in inputs {
                failures.record(input);
            }
            assert_eq!(failures.result(), Err(expected));
        }
    }

    #[cfg(unix)]
    #[test]
    fn materialize_authority_preflight_preserves_configuration_precedence() {
        use super::unix::{
            MaterializeAuthorityPreflightV1, classify_materialize_authority_preflight,
        };
        use paraegox_artifact::ArtifactStoreFailureV1;

        assert_eq!(
            classify_materialize_authority_preflight(None),
            MaterializeAuthorityPreflightV1::Continue
        );
        for failure in [
            ArtifactStoreFailureV1::NotFound,
            ArtifactStoreFailureV1::PublicationUncertain { operation: None },
        ] {
            assert_eq!(
                classify_materialize_authority_preflight(Some(&failure)),
                MaterializeAuthorityPreflightV1::Continue
            );
        }
        for failure in [
            ArtifactStoreFailureV1::UnsafePath,
            ArtifactStoreFailureV1::ConfigurationMismatch,
        ] {
            assert_eq!(
                classify_materialize_authority_preflight(Some(&failure)),
                MaterializeAuthorityPreflightV1::Immediate
            );
        }
        for failure in [
            ArtifactStoreFailureV1::Conflict,
            ArtifactStoreFailureV1::Capacity,
            ArtifactStoreFailureV1::Contended,
            ArtifactStoreFailureV1::Owner,
            ArtifactStoreFailureV1::Io,
        ] {
            assert_eq!(
                classify_materialize_authority_preflight(Some(&failure)),
                MaterializeAuthorityPreflightV1::Deferred
            );
        }
    }

    #[test]
    fn existing_build_replay_preserves_pre_barrier_read_taxonomy() {
        assert_eq!(
            map_existing_build_read_failure(LocalProcessError::ArtifactIo),
            ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactIo)
        );
        for case in ["short manifest", "short payload", "same-length byte drift"] {
            assert_eq!(
                map_existing_build_read_failure(LocalProcessError::ArtifactPath),
                ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath),
                "{case}"
            );
        }
        for case in ["oversize manifest", "oversize payload"] {
            assert_eq!(
                map_existing_build_read_failure(LocalProcessError::ArtifactCompatibility),
                ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath),
                "{case}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn artifact_errno_mapping_and_presence_are_typed() {
        for error in [
            nix::errno::Errno::ELOOP,
            nix::errno::Errno::ENOTDIR,
            nix::errno::Errno::ENOENT,
            nix::errno::Errno::ENAMETOOLONG,
            nix::errno::Errno::EACCES,
            nix::errno::Errno::EPERM,
        ] {
            assert_eq!(
                map_artifact_path_errno(error),
                LocalProcessError::ArtifactPath
            );
        }
        for error in [
            nix::errno::Errno::EIO,
            nix::errno::Errno::EMFILE,
            nix::errno::Errno::ENFILE,
            nix::errno::Errno::EINTR,
        ] {
            assert_eq!(
                map_artifact_path_errno(error),
                LocalProcessError::ArtifactIo
            );
        }
        assert_eq!(
            map_artifact_presence_errno(nix::errno::Errno::ENOENT),
            Ok(false)
        );
        assert_eq!(
            map_artifact_presence_errno(nix::errno::Errno::EACCES),
            Err(LocalProcessError::ArtifactPath)
        );
        assert_eq!(
            map_artifact_presence_errno(nix::errno::Errno::EIO),
            Err(LocalProcessError::ArtifactIo)
        );
    }

    #[test]
    fn build_pre_effect_failure_order_is_path_compatibility_staging_then_io() {
        assert_eq!(
            classify_build_pre_effect_failure(true, true, true, true),
            Some(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactPath,
            ))
        );
        assert_eq!(
            classify_build_pre_effect_failure(false, true, true, true),
            Some(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactCompatibility,
            ))
        );
        assert_eq!(
            classify_build_pre_effect_failure(false, false, true, true),
            Some(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactUncertain,
            ))
        );
        assert_eq!(
            classify_build_pre_effect_failure(false, false, false, true),
            Some(ArtifactFailureV1::new(
                Some(false),
                LocalProcessError::ArtifactIo,
            ))
        );
        assert_eq!(
            classify_build_pre_effect_failure(false, false, false, false),
            None
        );
    }

    #[test]
    fn build_final_and_staging_combination_order_is_total() {
        let path = Some(ArtifactFailureV1::new(
            Some(false),
            LocalProcessError::ArtifactPath,
        ));
        let uncertain = Some(ArtifactFailureV1::new(
            Some(false),
            LocalProcessError::ArtifactUncertain,
        ));
        assert_eq!(
            classify_build_pre_effect_failure(true, false, true, false),
            path
        );
        assert_eq!(
            classify_build_pre_effect_failure(true, true, false, false),
            path
        );
        assert_eq!(
            classify_build_pre_effect_failure(false, false, true, false),
            uncertain
        );
        assert_eq!(
            classify_build_pre_effect_failure(false, false, true, true),
            uncertain
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_mkdir_failure_has_exact_changed_classification() {
        assert_eq!(
            classify_build_mkdir_failure(
                nix::errno::Errno::EEXIST,
                BuildMkdirEvidenceV1::KnownStage,
            ),
            ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactUncertain)
        );
        assert_eq!(
            classify_build_mkdir_failure(
                nix::errno::Errno::EEXIST,
                BuildMkdirEvidenceV1::Structural,
            ),
            ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath)
        );
        assert_eq!(
            classify_build_mkdir_failure(nix::errno::Errno::EEXIST, BuildMkdirEvidenceV1::Unknown,),
            ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain)
        );
        assert_eq!(
            classify_build_mkdir_failure(
                nix::errno::Errno::ELOOP,
                BuildMkdirEvidenceV1::NotChecked,
            ),
            ArtifactFailureV1::new(Some(false), LocalProcessError::ArtifactPath)
        );
        assert_eq!(
            classify_build_mkdir_failure(nix::errno::Errno::EIO, BuildMkdirEvidenceV1::NotChecked,),
            ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain)
        );
        assert_eq!(
            classify_build_mkdir_failure(
                nix::errno::Errno::EINTR,
                BuildMkdirEvidenceV1::NotChecked,
            ),
            ArtifactFailureV1::new(None, LocalProcessError::ArtifactUncertain)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn inspect_read_flags_require_noatime_without_fallback() {
        let inspect = artifact_regular_read_flags(true).expect("Linux O_NOATIME");
        assert!(inspect.contains(nix::fcntl::OFlag::O_NOATIME));
        let ordinary = artifact_regular_read_flags(false).expect("ordinary read flags");
        assert!(!ordinary.contains(nix::fcntl::OFlag::O_NOATIME));
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn inspect_is_platform_rejected_before_identity_without_noatime() {
        let arguments = [
            "artifact",
            "inspect",
            "--manifest",
            "/tmp/manifest.pxam",
            "--payload",
            "/tmp/payload.bin",
            "--json",
        ]
        .map(OsString::from);
        let mut output = Vec::new();
        assert_eq!(
            dispatch_to(&mut output, ArtifactJsonIntentV1::Inspect, &arguments),
            2
        );
        assert_eq!(
            String::from_utf8(output).expect("UTF-8 JSON"),
            "{\"schema_version\":1,\"command\":\"artifact.inspect\",\"ok\":false,\"changed\":false,\"profile\":null,\"artifact_object_ref\":null,\"payload_length\":null,\"runtime_kind\":null,\"adapter_abi\":null,\"target_profile\":null,\"diagnostics\":[{\"code\":\"PXLC-PLATFORM-UNSUPPORTED\",\"message\":\"DeveloperLocal modes require the Unix DeveloperLocal platform\"}]}\n"
        );
        assert_eq!(
            artifact_regular_read_flags(true),
            Err(LocalProcessError::ArtifactIo)
        );
    }

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

    #[cfg(unix)]
    #[test]
    fn fixed_materialization_shapes_preserve_operation_id_on_non_utf8_paths() {
        use std::os::unix::ffi::OsStringExt;

        const OPERATION_ID: &str = "a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2";
        const MATERIALIZE_ERROR: &str = "{\"schema_version\":1,\"command\":\"artifact.materialize\",\"ok\":false,\"changed\":false,\"operation_id\":\"a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2\",\"state\":null,\"artifact_object_ref\":null,\"materialization_receipt_ref\":null,\"diagnostics\":[{\"code\":\"PXLC-ARG-NON-UTF8\",\"message\":\"arguments must be valid UTF-8\"}]}\n";
        const QUERY_ERROR: &str = "{\"schema_version\":1,\"command\":\"artifact.materialization.query\",\"ok\":false,\"changed\":false,\"operation_id\":\"a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2\",\"state\":null,\"artifact_object_ref\":null,\"materialization_receipt_ref\":null,\"diagnostics\":[{\"code\":\"PXLC-ARG-NON-UTF8\",\"message\":\"arguments must be valid UTF-8\"}]}\n";

        let materialize = [
            "artifact",
            "materialize",
            "--config",
            "/private/paraegox.toml",
            "--manifest",
            "/private/manifest.pxam",
            "--payload",
            "/private/payload.bin",
            "--operation-id",
            OPERATION_ID,
            "--json",
        ]
        .map(OsString::from)
        .to_vec();
        for index in [3, 5, 7] {
            let mut arguments = materialize.clone();
            arguments[index] = OsString::from_vec(vec![0xff]);
            let mut output = Vec::new();
            assert_eq!(
                dispatch_to(&mut output, ArtifactJsonIntentV1::Materialize, &arguments,),
                2
            );
            assert_eq!(
                String::from_utf8(output).expect("UTF-8 JSON"),
                MATERIALIZE_ERROR
            );
        }

        let mut query = [
            "artifact",
            "materialization",
            "query",
            "--config",
            "/private/paraegox.toml",
            "--operation-id",
            OPERATION_ID,
            "--json",
        ]
        .map(OsString::from)
        .to_vec();
        query[4] = OsString::from_vec(vec![0xff]);
        let mut output = Vec::new();
        assert_eq!(
            dispatch_to(
                &mut output,
                ArtifactJsonIntentV1::MaterializationQuery,
                &query,
            ),
            2
        );
        assert_eq!(String::from_utf8(output).expect("UTF-8 JSON"), QUERY_ERROR);
    }
}
