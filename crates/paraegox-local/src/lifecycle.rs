//! Narrow managed-local lifecycle for the existing chat composition.
//!
//! This module is deliberately not a generic service manager. One hidden
//! supervisor owns exactly one existing DeveloperLocal chat composition, one
//! private control socket, and one durable lifecycle record. Public clients
//! never treat a PID as authority and never signal a process directly.

use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::future;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nix::unistd::{Gid, Uid, chown, setsid};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::{Instant, sleep, timeout};

use crate::composition::{
    HeadlessLifecycleControlV1, PreparedHeadlessChatV1, VerifiedLocalDeploymentProjectionV1,
    prepare_headless_chat, run_prepared_headless_chat,
};
use crate::config::{LocalLifecycleActionV1, LocalManagedChatConfigV1};
use crate::error::LocalProcessError;

pub(crate) const LOCAL_CHAT_SUPERVISOR_MODE_V1: &str = "__local-chat-supervisor-v1";
pub(crate) const EXPECTED_CONFIG_COMMITMENT_OPTION: &str = "--expected-config-commitment";
pub(crate) const EXPECTED_GENERATION_OPTION: &str = "--expected-generation";
pub(crate) const LOCAL_CHAT_SUPERVISOR_CONTENTION_EXIT_CODE_V1: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalChatSupervisorResultV1 {
    Completed,
    Contended,
}

const OPERATOR_DIRECTORY: &str = "operator-v1";
const OWNER_LOCK_FILE: &str = "owner.lock";
const RECORD_FILE: &str = "lifecycle-v1.json";
const RECORD_TEMP_FILE: &str = "lifecycle-v1.tmp";
const CONTROL_SOCKET_FILE: &str = "control-v1.sock";
const RECORD_SCHEMA_VERSION: u16 = 1;
const INTERNAL_PROTOCOL_VERSION: u8 = 1;
const INTERNAL_REQUEST_BYTES: usize = 38;
const INTERNAL_DEPLOY_QUERY_BYTES: usize = INTERNAL_REQUEST_BYTES + 16;
const MAX_RECORD_BYTES: u64 = 4 * 1024;
const MAX_INTERNAL_RESPONSE_BYTES: usize = 4 * 1024;
const MAX_DOWN_WAITERS: usize = 16;
const MAX_PORTABLE_UNIX_SOCKET_PATH_BYTES: usize = 103;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(120);
const SHUTDOWN_RESPONSE_TIMEOUT: Duration = Duration::from_secs(125);
const START_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONCURRENT_DIRECTORY_INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(3);
const LOCAL_OWNER_STACK_BYTES: usize = 16 * 1024 * 1024;
const INTERNAL_MAGIC: [u8; 4] = *b"PXLO";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalLifecycleStateV1 {
    NeverStarted,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Unknown,
}

impl LocalLifecycleStateV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NeverStarted => "never_started",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalLifecycleDiagnosticV1 {
    code: &'static str,
    message: &'static str,
}

impl LocalLifecycleDiagnosticV1 {
    pub(crate) const fn code(self) -> &'static str {
        self.code
    }

    pub(crate) const fn message(self) -> &'static str {
        self.message
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalLifecycleObservationV1 {
    state: LocalLifecycleStateV1,
    generation: Option<Box<str>>,
    changed: bool,
    owner_readiness_observed: bool,
    diagnostic: Option<LocalLifecycleDiagnosticV1>,
}

impl LocalLifecycleObservationV1 {
    pub(crate) const fn ok(&self) -> bool {
        self.diagnostic.is_none()
            && !matches!(
                self.state,
                LocalLifecycleStateV1::Failed | LocalLifecycleStateV1::Unknown
            )
    }

    pub(crate) const fn state(&self) -> LocalLifecycleStateV1 {
        self.state
    }

    pub(crate) fn generation(&self) -> Option<&str> {
        self.generation.as_deref()
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }

    pub(crate) const fn owner_readiness_observed(&self) -> bool {
        self.owner_readiness_observed
    }

    pub(crate) const fn diagnostic(&self) -> Option<LocalLifecycleDiagnosticV1> {
        self.diagnostic
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self.diagnostic {
            Some(value) if value.code == "PXLC-LIFECYCLE-CONFIGURATION" => 2,
            Some(_) => 1,
            None if matches!(
                self.state,
                LocalLifecycleStateV1::Failed | LocalLifecycleStateV1::Unknown
            ) =>
            {
                1
            }
            None => 0,
        }
    }

    fn with_changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    fn never_started() -> Self {
        Self {
            state: LocalLifecycleStateV1::NeverStarted,
            generation: None,
            changed: false,
            owner_readiness_observed: false,
            diagnostic: None,
        }
    }

    fn unknown(generation: Option<Box<str>>, readiness: bool) -> Self {
        Self::unknown_with(
            generation,
            readiness,
            LocalProcessError::LifecycleReconcileRequired,
        )
    }

    fn unknown_with(
        generation: Option<Box<str>>,
        readiness: bool,
        error: LocalProcessError,
    ) -> Self {
        Self {
            state: LocalLifecycleStateV1::Unknown,
            generation,
            changed: false,
            owner_readiness_observed: readiness,
            diagnostic: Some(diagnostic(error)),
        }
    }

    fn failed(error: LocalProcessError) -> Self {
        Self::failed_with_evidence(None, false, error)
    }

    fn failed_with_evidence(
        generation: Option<Box<str>>,
        readiness: bool,
        error: LocalProcessError,
    ) -> Self {
        Self {
            state: LocalLifecycleStateV1::Failed,
            generation,
            changed: false,
            owner_readiness_observed: readiness,
            diagnostic: Some(diagnostic(error)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalDeployProjectionV1 {
    controller_revision: u64,
    controller_snapshot_sequence: u64,
    runtime_apply_request_digest: [u8; 32],
    runtime_terminal_receipt_digest: [u8; 32],
    model_agent_replayed: bool,
}

impl LocalDeployProjectionV1 {
    pub(crate) const fn controller_revision(self) -> u64 {
        self.controller_revision
    }

    pub(crate) const fn controller_snapshot_sequence(self) -> u64 {
        self.controller_snapshot_sequence
    }

    pub(crate) const fn runtime_apply_request_digest(self) -> [u8; 32] {
        self.runtime_apply_request_digest
    }

    pub(crate) const fn runtime_terminal_receipt_digest(self) -> [u8; 32] {
        self.runtime_terminal_receipt_digest
    }

    pub(crate) const fn model_agent_replayed(self) -> bool {
        self.model_agent_replayed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalDeployObservationV1 {
    generation: Box<str>,
    changed: bool,
    projection: LocalDeployProjectionV1,
}

impl LocalDeployObservationV1 {
    pub(crate) fn generation(&self) -> &str {
        &self.generation
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }

    pub(crate) const fn projection(&self) -> LocalDeployProjectionV1 {
        self.projection
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        generation: &str,
        changed: bool,
        controller_revision: u64,
        controller_snapshot_sequence: u64,
        runtime_apply_request_digest: [u8; 32],
        runtime_terminal_receipt_digest: [u8; 32],
    ) -> Self {
        Self {
            generation: generation.into(),
            changed,
            projection: LocalDeployProjectionV1 {
                controller_revision,
                controller_snapshot_sequence,
                runtime_apply_request_digest,
                runtime_terminal_receipt_digest,
                model_agent_replayed: false,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalDeployFailureV1 {
    error: LocalProcessError,
    changed: Option<bool>,
}

impl LocalDeployFailureV1 {
    pub(crate) const fn error(self) -> LocalProcessError {
        self.error
    }

    pub(crate) const fn changed(self) -> Option<bool> {
        self.changed
    }

    const fn before_effect(error: LocalProcessError) -> Self {
        Self {
            error,
            changed: Some(false),
        }
    }

    const fn after_up(error: LocalProcessError, up_changed: bool) -> Self {
        Self {
            error,
            changed: if up_changed { None } else { Some(false) },
        }
    }

    const fn uncertain(error: LocalProcessError) -> Self {
        Self {
            error,
            changed: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LifecycleRecordV1 {
    schema_version: u16,
    config_commitment: Box<str>,
    generation: Box<str>,
    state: LocalLifecycleStateV1,
    owner_readiness_observed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InternalObservationV1 {
    state: LocalLifecycleStateV1,
    generation: Option<Box<str>>,
    changed: bool,
    owner_readiness_observed: bool,
    diagnostic_code: Option<Box<str>>,
}

impl InternalObservationV1 {
    fn from_record(record: &LifecycleRecordV1, changed: bool) -> Self {
        Self {
            state: record.state,
            generation: Some(record.generation.clone()),
            changed,
            owner_readiness_observed: record.owner_readiness_observed,
            diagnostic_code: matches!(record.state, LocalLifecycleStateV1::Failed)
                .then(|| LocalProcessError::LifecycleStartup.code().into()),
        }
    }

    fn into_public(self) -> LocalLifecycleObservationV1 {
        let diagnostic = self.diagnostic_code.as_deref().map(|code| {
            if code == LocalProcessError::LifecycleStartup.code() {
                diagnostic(LocalProcessError::LifecycleStartup)
            } else if code == LocalProcessError::LifecycleShutdown.code() {
                diagnostic(LocalProcessError::LifecycleShutdown)
            } else {
                diagnostic(LocalProcessError::LifecycleReconcileRequired)
            }
        });
        LocalLifecycleObservationV1 {
            state: self.state,
            generation: self.generation,
            changed: self.changed,
            owner_readiness_observed: self.owner_readiness_observed,
            diagnostic,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InternalActionV1 {
    Status,
    Down,
    Deploy,
}

impl InternalActionV1 {
    const fn wire(self) -> u8 {
        match self {
            Self::Status => b'S',
            Self::Down => b'D',
            Self::Deploy => b'P',
        }
    }

    const fn decode(value: u8) -> Option<Self> {
        match value {
            b'S' => Some(Self::Status),
            b'D' => Some(Self::Down),
            b'P' => Some(Self::Deploy),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InternalRequestV1 {
    action: InternalActionV1,
    expected_generation: Option<[u8; 16]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InternalDeployResponseV1 {
    generation: Box<str>,
    deployment_revision: u64,
    controller_snapshot_sequence: u64,
    runtime_apply_request_digest: Box<str>,
    runtime_terminal_receipt_digest: Box<str>,
    terminal_outcome: Box<str>,
    fabric_replayed: bool,
    model_agent_replayed: bool,
}

impl InternalDeployResponseV1 {
    fn from_verified(
        generation: &str,
        projection: VerifiedLocalDeploymentProjectionV1,
    ) -> Self {
        Self {
            generation: generation.into(),
            deployment_revision: projection.controller_revision(),
            controller_snapshot_sequence: projection.controller_snapshot_sequence(),
            runtime_apply_request_digest: lower_hex(&projection.runtime_apply_request_digest())
                .into_boxed_str(),
            runtime_terminal_receipt_digest: lower_hex(
                &projection.runtime_terminal_receipt_digest(),
            )
            .into_boxed_str(),
            terminal_outcome: "active_ready".into(),
            fabric_replayed: projection.fabric_replayed(),
            model_agent_replayed: projection.model_agent_replayed(),
        }
    }

    fn into_projection(
        self,
        expected_generation: [u8; 16],
    ) -> Result<LocalDeployProjectionV1, LocalProcessError> {
        let observed_generation =
            decode_generation(&self.generation).map_err(|_| LocalProcessError::LocalDeployEvidence)?;
        let runtime_apply_request_digest = decode_lower_hex_32(&self.runtime_apply_request_digest)
            .map_err(|_| LocalProcessError::LocalDeployEvidence)?;
        let runtime_terminal_receipt_digest =
            decode_lower_hex_32(&self.runtime_terminal_receipt_digest)
                .map_err(|_| LocalProcessError::LocalDeployEvidence)?;
        if observed_generation != expected_generation
            || self.deployment_revision == 0
            || self.controller_snapshot_sequence == 0
            || self.terminal_outcome.as_ref() != "active_ready"
            || runtime_apply_request_digest.iter().all(|byte| *byte == 0)
            || runtime_terminal_receipt_digest
                .iter()
                .all(|byte| *byte == 0)
        {
            return Err(LocalProcessError::LocalDeployEvidence);
        }
        Ok(LocalDeployProjectionV1 {
            controller_revision: self.deployment_revision,
            controller_snapshot_sequence: self.controller_snapshot_sequence,
            runtime_apply_request_digest,
            runtime_terminal_receipt_digest,
            model_agent_replayed: self.model_agent_replayed,
        })
    }
}

struct LifecyclePathsV1 {
    root: PathBuf,
    lock: PathBuf,
    record: PathBuf,
    temporary: PathBuf,
    socket: PathBuf,
}

impl LifecyclePathsV1 {
    fn from_state_root(state_root: &Path) -> Result<Self, LocalProcessError> {
        let root = state_root.join(OPERATOR_DIRECTORY);
        let socket = root.join(CONTROL_SOCKET_FILE);
        if socket.as_os_str().as_bytes().len() > MAX_PORTABLE_UNIX_SOCKET_PATH_BYTES {
            return Err(LocalProcessError::LifecycleState);
        }
        Ok(Self {
            lock: root.join(OWNER_LOCK_FILE),
            record: root.join(RECORD_FILE),
            temporary: root.join(RECORD_TEMP_FILE),
            socket,
            root,
        })
    }
}

struct HeadlessControlV1 {
    events: UnboundedSender<SupervisorEventV1>,
    shutdown: Receiver<()>,
}

impl HeadlessLifecycleControlV1 for HeadlessControlV1 {
    fn mark_ready(
        &mut self,
        deployment: Option<VerifiedLocalDeploymentProjectionV1>,
    ) -> Result<(), LocalProcessError> {
        self.events
            .send(SupervisorEventV1::Ready(deployment))
            .map_err(|_| LocalProcessError::LifecycleControl)
    }

    fn wait_for_shutdown(&mut self) -> Result<(), LocalProcessError> {
        self.shutdown
            .recv()
            .map_err(|_| LocalProcessError::LifecycleControl)
    }
}

enum SupervisorEventV1 {
    Ready(Option<VerifiedLocalDeploymentProjectionV1>),
    Exited(Result<(), LocalProcessError>),
}

struct OwnedSocketV1 {
    path: PathBuf,
    device: u64,
    inode: u64,
    removed: bool,
}

impl OwnedSocketV1 {
    fn capture(path: &Path) -> Result<Self, LocalProcessError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LifecycleState)?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != Uid::effective().as_raw()
            || metadata.gid() != Gid::effective().as_raw()
        {
            return Err(LocalProcessError::LifecycleState);
        }
        Ok(Self {
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
            removed: false,
        })
    }

    fn validate_private_mode(&self) -> Result<(), LocalProcessError> {
        let metadata =
            fs::symlink_metadata(&self.path).map_err(|_| LocalProcessError::LifecycleControl)?;
        if !metadata.file_type().is_socket()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.uid() != Uid::effective().as_raw()
            || metadata.gid() != Gid::effective().as_raw()
            || metadata.permissions().mode() & 0o7777 != 0o600
        {
            return Err(LocalProcessError::LifecycleControl);
        }
        Ok(())
    }

    fn remove_owned(&mut self) -> Result<(), LocalProcessError> {
        if self.removed {
            return Ok(());
        }
        let metadata =
            fs::symlink_metadata(&self.path).map_err(|_| LocalProcessError::LifecycleShutdown)?;
        if !metadata.file_type().is_socket()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.uid() != Uid::effective().as_raw()
            || metadata.gid() != Gid::effective().as_raw()
        {
            return Err(LocalProcessError::LifecycleShutdown);
        }
        fs::remove_file(&self.path).map_err(|_| LocalProcessError::LifecycleShutdown)?;
        sync_directory(
            self.path
                .parent()
                .ok_or(LocalProcessError::LifecycleShutdown)?,
        )
        .map_err(|_| LocalProcessError::LifecycleShutdown)?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for OwnedSocketV1 {
    fn drop(&mut self) {
        let _ = self.remove_owned();
    }
}

pub(crate) fn decode_config_commitment_hex(
    value: &std::ffi::OsStr,
) -> Result<[u8; 32], LocalProcessError> {
    let bytes = value.as_bytes();
    if bytes.len() != 64 {
        return Err(LocalProcessError::LifecycleConfiguration);
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        decoded[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(decoded)
}

pub(crate) fn decode_generation_hex(
    value: &std::ffi::OsStr,
) -> Result<[u8; 16], LocalProcessError> {
    let value = value
        .to_str()
        .ok_or(LocalProcessError::LifecycleConfiguration)?;
    decode_generation(value)
}

pub(crate) fn run_up(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    validate_execution_identity()?;
    let admission_deadline = Instant::now() + STARTUP_TIMEOUT;
    let terminal_observation = loop {
        let observation = observe(config)?;
        if live_owner_transition_in_progress(config, &observation)? {
            if Instant::now() >= admission_deadline {
                return Ok(observation);
            }
            thread::sleep(START_POLL_INTERVAL);
            continue;
        }
        match observation {
            observation
                if matches!(
                    observation.state(),
                    LocalLifecycleStateV1::Running | LocalLifecycleStateV1::Starting
                ) =>
            {
                return wait_until_running(config, None, None, admission_deadline);
            }
            observation
                if matches!(
                    observation.state(),
                    LocalLifecycleStateV1::Failed | LocalLifecycleStateV1::Unknown
                ) =>
            {
                return Ok(observation);
            }
            observation if observation.state() == LocalLifecycleStateV1::Stopping => {
                return Ok(LocalLifecycleObservationV1::unknown(
                    observation.generation,
                    observation.owner_readiness_observed,
                ));
            }
            _ => break observation,
        }
    };
    if Instant::now() >= admission_deadline {
        return Ok(LocalLifecycleObservationV1::unknown(
            terminal_observation.generation,
            terminal_observation.owner_readiness_observed,
        ));
    }

    let executable = std::env::current_exe().map_err(|_| LocalProcessError::LifecycleStartup)?;
    let requested_generation = new_generation()?;
    let mut child = Command::new(executable)
        .arg(LOCAL_CHAT_SUPERVISOR_MODE_V1)
        .arg("--config")
        .arg(config.source_path())
        .arg(EXPECTED_CONFIG_COMMITMENT_OPTION)
        .arg(lower_hex(&config.config_commitment()))
        .arg(EXPECTED_GENERATION_OPTION)
        .arg(&requested_generation)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    wait_until_running(
        config,
        Some(&requested_generation),
        Some(&mut child),
        admission_deadline,
    )
}

pub(crate) fn run_status(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    validate_execution_identity()?;
    observe(config)
}

pub(crate) fn run_down(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    validate_execution_identity()?;
    if let Some(drift) = config_authority_drift(config)? {
        return Ok(drift);
    }
    match query_live(config, InternalActionV1::Down) {
        Ok(observation) => Ok(observation.into_public()),
        Err(LocalProcessError::LifecycleUnavailable) => {
            let observation = observe_disk(config)?;
            if matches!(
                observation.state(),
                LocalLifecycleStateV1::NeverStarted | LocalLifecycleStateV1::Stopped
            ) {
                Ok(observation.with_changed(false))
            } else {
                Ok(observation)
            }
        }
        Err(error) => config_drift_after_control_failure(config, error),
    }
}

/// Ensures the sole compiled-in deterministic deployment is running, then
/// performs exactly one generation-bound read of the supervisor's verified
/// terminal projection. The query never retries and never starts a second
/// Controller or re-applies desired state.
pub(crate) fn run_local_deploy(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalDeployObservationV1, LocalDeployFailureV1> {
    let up = match run_up(config) {
        Ok(observation) => observation,
        Err(error)
            if matches!(
                error,
                LocalProcessError::UnsafeExecutionIdentity
                    | LocalProcessError::LifecycleConfiguration
            ) =>
        {
            return Err(LocalDeployFailureV1::before_effect(error));
        }
        Err(_) => {
            return Err(LocalDeployFailureV1::uncertain(
                LocalProcessError::LocalDeployLifecycle,
            ));
        }
    };
    if !up.ok()
        || up.state() != LocalLifecycleStateV1::Running
        || !up.owner_readiness_observed()
    {
        return Err(local_deploy_non_running_failure(&up));
    }
    let generation = up.generation().ok_or_else(|| {
        LocalDeployFailureV1::after_up(LocalProcessError::LocalDeployEvidence, up.changed())
    })?;
    let expected_generation = decode_generation(generation).map_err(|_| {
        LocalDeployFailureV1::after_up(LocalProcessError::LocalDeployEvidence, up.changed())
    })?;
    let response = match query_local_deployment(config, expected_generation) {
        Ok(response) => response,
        Err(error) => {
            let error = match config_authority_drift(config) {
                Ok(Some(_)) => LocalProcessError::LifecycleConfiguration,
                Ok(None) => error,
                Err(_) => LocalProcessError::LocalDeployQuery,
            };
            return Err(LocalDeployFailureV1::after_up(error, up.changed()));
        }
    };
    let projection = response
        .into_projection(expected_generation)
        .map_err(|error| LocalDeployFailureV1::after_up(error, up.changed()))?;
    Ok(LocalDeployObservationV1 {
        generation: generation.into(),
        changed: local_deploy_changed(up.changed(), projection.model_agent_replayed()),
        projection,
    })
}

fn local_deploy_non_running_failure(
    up: &LocalLifecycleObservationV1,
) -> LocalDeployFailureV1 {
    if up.diagnostic().is_some_and(|diagnostic| {
        diagnostic.code() == LocalProcessError::LifecycleConfiguration.code()
    }) {
        return LocalDeployFailureV1::before_effect(LocalProcessError::LifecycleConfiguration);
    }
    // A failed/non-Running observation cannot prove this invocation's
    // candidate generation was never accepted: down plus a successor up can
    // replace the durable record before the original child is observed.
    // Never turn that ambiguity into `changed = false`.
    LocalDeployFailureV1::uncertain(LocalProcessError::LocalDeployLifecycle)
}

const fn local_deploy_changed(up_changed: bool, model_agent_replayed: bool) -> bool {
    up_changed && !model_agent_replayed
}

pub(crate) fn run_supervisor(
    config: LocalManagedChatConfigV1,
    expected_config_commitment: [u8; 32],
    expected_generation: [u8; 16],
) -> Result<LocalChatSupervisorResultV1, LocalProcessError> {
    validate_execution_identity()?;
    if config.config_commitment() != expected_config_commitment
        || expected_generation.iter().all(|byte| *byte == 0)
    {
        return Err(LocalProcessError::LifecycleConfiguration);
    }
    // Resolve the configured provider Secret exactly once before detaching or
    // creating any operator/domain state. The move-only prepared value carries
    // it across the lifecycle boundary without placing it in argv or a record.
    let prepared = prepare_headless_chat(config.clone().into_owner_config())?;
    setsid().map_err(|_| LocalProcessError::LifecycleStartup)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    runtime.block_on(run_supervisor_async(config, prepared, expected_generation))
}

/// Builds a fail-closed public projection after an operation-level failure.
/// The record is used only for bounded generation/readiness evidence; it can
/// never upgrade the failure to Running or Stopped.
pub(crate) fn project_failure(
    config: &LocalManagedChatConfigV1,
    _action: LocalLifecycleActionV1,
    error: LocalProcessError,
) -> LocalLifecycleObservationV1 {
    let record = read_failure_evidence(config).ok().flatten();
    let generation = record.as_ref().map(|record| record.generation.clone());
    let readiness = record
        .as_ref()
        .is_some_and(|record| record.owner_readiness_observed);
    // A shared durable state cannot prove that this particular failed request
    // accepted the mutation. Only a successful correlated control response may
    // set `changed`; failure projection therefore stays conservative.
    let changed = false;
    let state = if error == LocalProcessError::LifecycleStartup
        && record
            .as_ref()
            .is_some_and(|record| record.state == LocalLifecycleStateV1::Failed)
    {
        LocalLifecycleStateV1::Failed
    } else {
        LocalLifecycleStateV1::Unknown
    };
    LocalLifecycleObservationV1 {
        state,
        generation,
        changed,
        owner_readiness_observed: readiness,
        diagnostic: Some(diagnostic(error)),
    }
}

fn read_failure_evidence(
    config: &LocalManagedChatConfigV1,
) -> Result<Option<LifecycleRecordV1>, LocalProcessError> {
    let paths = LifecyclePathsV1::from_state_root(config.state_root())?;
    if !config.state_root().exists() {
        return Ok(None);
    }
    validate_existing_private_directory(config.state_root())?;
    if !paths.root.exists() {
        return Ok(None);
    }
    validate_existing_private_directory(&paths.root)?;
    let record = read_record_optional(&paths.record)?;
    Ok(record.filter(|record| validate_record_for_config(record, config).is_ok()))
}

fn live_owner_transition_in_progress(
    config: &LocalManagedChatConfigV1,
    observation: &LocalLifecycleObservationV1,
) -> Result<bool, LocalProcessError> {
    if observation.state() != LocalLifecycleStateV1::Unknown
        || !observation.diagnostic().is_some_and(|value| {
            value.code() == LocalProcessError::LifecycleReconcileRequired.code()
        })
    {
        return Ok(false);
    }
    live_owner_lock_is_held(config)
}

async fn run_supervisor_async(
    config: LocalManagedChatConfigV1,
    prepared: PreparedHeadlessChatV1,
    expected_generation: [u8; 16],
) -> Result<LocalChatSupervisorResultV1, LocalProcessError> {
    let canonical_state_root = ensure_private_state_root(config.state_root())?;
    let paths = LifecyclePathsV1::from_state_root(&canonical_state_root)?;
    ensure_private_directory(&paths.root)?;
    validate_operator_entries(&paths)?;
    let _owner_lock = match acquire_owner_lock(&paths.lock) {
        Ok(owner_lock) => owner_lock,
        Err(LocalProcessError::LifecycleUnavailable) => {
            return Ok(LocalChatSupervisorResultV1::Contended);
        }
        Err(error) => return Err(error),
    };

    let prior = read_record_optional(&paths.record)?;
    if paths.temporary.exists() {
        return Err(LocalProcessError::LifecycleReconcileRequired);
    }
    if let Some(record) = prior.as_ref() {
        validate_record_for_config(record, &config)?;
        if record.state != LocalLifecycleStateV1::Stopped {
            return Err(LocalProcessError::LifecycleReconcileRequired);
        }
    }
    remove_terminal_stale_socket(&paths.socket)?;
    let standard_listener = std::os::unix::net::UnixListener::bind(&paths.socket)
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    let mut owned_socket = OwnedSocketV1::capture(&paths.socket)?;
    fs::set_permissions(&paths.socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    owned_socket.validate_private_mode()?;
    standard_listener
        .set_nonblocking(true)
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    sync_directory(&paths.root)?;
    let listener = UnixListener::from_std(standard_listener)
        .map_err(|_| LocalProcessError::LifecycleControl)?;

    // Signal receivers are installed before the composition thread can start
    // any owner. A failure here therefore cannot strand a live composition.
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| LocalProcessError::SignalHandling)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| LocalProcessError::SignalHandling)?;

    let generation = lower_hex(&expected_generation);
    let mut record = LifecycleRecordV1 {
        schema_version: RECORD_SCHEMA_VERSION,
        config_commitment: lower_hex(&config.config_commitment()).into_boxed_str(),
        generation: generation.into_boxed_str(),
        state: LocalLifecycleStateV1::Starting,
        owner_readiness_observed: false,
    };
    publish_record(&paths, &record)?;

    let (events_tx, events_rx) = unbounded_channel();
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let composition = spawn_composition(prepared, events_tx, shutdown_rx)?;
    let mut down_waiters = Vec::new();
    let mut shutdown_deadline = None;
    let supervisor_result = supervise(
        SupervisorContextV1 {
            listener: &listener,
            paths: &paths,
            record: &mut record,
            shutdown: shutdown_tx.clone(),
            interrupt: &mut interrupt,
            terminate: &mut terminate,
            shutdown_deadline: &mut shutdown_deadline,
            down_waiters: &mut down_waiters,
        },
        events_rx,
    )
    .await;
    drop(listener);

    if supervisor_result.is_err() && !composition.is_finished() {
        if shutdown_deadline.is_none() {
            shutdown_deadline = Some(Instant::now() + SHUTDOWN_TIMEOUT);
        }
        let _ = shutdown_tx.send(());
    }
    let remaining_shutdown_budget = shutdown_deadline.map_or(SHUTDOWN_TIMEOUT, |deadline| {
        deadline.saturating_duration_since(Instant::now())
    });
    let owner_finished = wait_for_thread_exit(&composition, remaining_shutdown_budget).await;
    let owner_result = if owner_finished {
        match composition.join() {
            Ok(result) => result,
            Err(_) => Err(LocalProcessError::LifecycleShutdown),
        }
    } else {
        drop(composition);
        Err(LocalProcessError::LifecycleShutdown)
    };
    let mut final_result = supervisor_result.and(owner_result);

    // The public stopped result is emitted only after the owner thread has
    // joined and the exact owned socket has been unlinked and directory-synced.
    // A failed cleanup is durable Unknown, never a successful Stopped claim.
    if owned_socket.remove_owned().is_err() {
        final_result = Err(LocalProcessError::LifecycleShutdown);
    }
    record.state = if final_result.is_ok() {
        LocalLifecycleStateV1::Stopped
    } else {
        LocalLifecycleStateV1::Unknown
    };
    if publish_record(&paths, &record).is_err() {
        record.state = LocalLifecycleStateV1::Unknown;
        let _ = publish_record(&paths, &record);
        final_result = Err(LocalProcessError::LifecycleState);
    }

    let mut observation = InternalObservationV1::from_record(&record, false);
    if let Err(error) = final_result {
        observation.diagnostic_code = Some(error.code().into());
    }
    for (mut waiter, changed) in down_waiters {
        let mut waiter_observation = observation.clone();
        waiter_observation.changed = changed;
        let _ = write_internal_observation(&mut waiter, &waiter_observation).await;
    }
    final_result.map(|()| LocalChatSupervisorResultV1::Completed)
}

async fn wait_for_thread_exit(
    composition: &JoinHandle<Result<(), LocalProcessError>>,
    budget: Duration,
) -> bool {
    let deadline = Instant::now() + budget;
    while !composition.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(START_POLL_INTERVAL).await;
    }
    true
}

fn spawn_composition(
    prepared: PreparedHeadlessChatV1,
    events: UnboundedSender<SupervisorEventV1>,
    shutdown: Receiver<()>,
) -> Result<JoinHandle<Result<(), LocalProcessError>>, LocalProcessError> {
    thread::Builder::new()
        .name("paraegox-local-owner".to_owned())
        .stack_size(LOCAL_OWNER_STACK_BYTES)
        .spawn(move || {
            let mut control = HeadlessControlV1 {
                events: events.clone(),
                shutdown,
            };
            let result = run_prepared_headless_chat(prepared, &mut control);
            let _ = events.send(SupervisorEventV1::Exited(result));
            result
        })
        .map_err(|_| LocalProcessError::LifecycleStartup)
}

struct SupervisorContextV1<'a> {
    listener: &'a UnixListener,
    paths: &'a LifecyclePathsV1,
    record: &'a mut LifecycleRecordV1,
    shutdown: Sender<()>,
    interrupt: &'a mut tokio::signal::unix::Signal,
    terminate: &'a mut tokio::signal::unix::Signal,
    shutdown_deadline: &'a mut Option<Instant>,
    down_waiters: &'a mut Vec<(UnixStream, bool)>,
}

async fn supervise(
    context: SupervisorContextV1<'_>,
    mut events: UnboundedReceiver<SupervisorEventV1>,
) -> Result<(), LocalProcessError> {
    let SupervisorContextV1 {
        listener,
        paths,
        record,
        shutdown,
        interrupt,
        terminate,
        shutdown_deadline,
        down_waiters,
    } = context;
    let expected_uid = Uid::effective().as_raw();
    let expected_gid = Gid::effective().as_raw();
    let commitment = decode_lower_hex_32(&record.config_commitment)?;
    let startup_deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut stopping = false;
    let mut failure = None;
    let mut deployment_projection = None;

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut stream, _) = accepted.map_err(|_| LocalProcessError::LifecycleControl)?;
                if !peer_matches(&stream, expected_uid, expected_gid) {
                    continue;
                }
                let request = match read_internal_request(&mut stream, commitment).await {
                    Ok(request) => request,
                    Err(_) => continue,
                };
                match request.action {
                    InternalActionV1::Status => {
                        let _ = write_internal_observation(
                            &mut stream,
                            &InternalObservationV1::from_record(record, false),
                        )
                        .await;
                    }
                    InternalActionV1::Down => {
                        if down_waiters.len() >= MAX_DOWN_WAITERS {
                            continue;
                        }
                        let changed = !stopping;
                        down_waiters.push((stream, changed));
                        if !stopping {
                            stopping = true;
                            *shutdown_deadline = Some(Instant::now() + SHUTDOWN_TIMEOUT);
                            record.state = LocalLifecycleStateV1::Stopping;
                            publish_record(paths, record)?;
                            if shutdown.send(()).is_err() {
                                failure = Some(LocalProcessError::LifecycleShutdown);
                            }
                        }
                    }
                    InternalActionV1::Deploy => {
                        let Some(expected_generation) = request.expected_generation else {
                            continue;
                        };
                        let Some(response) = deployment_response_for_request(
                            record,
                            stopping,
                            expected_generation,
                            deployment_projection,
                        ) else {
                            continue;
                        };
                        let _ = write_internal_deploy_response(&mut stream, &response).await;
                    }
                }
            }
            event = events.recv() => {
                match event {
                    Some(SupervisorEventV1::Ready(deployment)) => {
                        // Readiness is a monotonic historical latch even when
                        // shutdown won the race. Keep Stopping, but durably
                        // remember that this generation crossed the boundary.
                        deployment_projection = deployment;
                        apply_ready_observation(record, stopping);
                        publish_record(paths, record)?;
                    }
                    Some(SupervisorEventV1::Exited(owner_result)) => {
                        let clean = owner_result.is_ok() && stopping && failure.is_none();
                        let owner_failure = if stopping {
                            LocalProcessError::LifecycleShutdown
                        } else {
                            LocalProcessError::LifecycleStartup
                        };
                        return if clean {
                            Ok(())
                        } else {
                            Err(failure.unwrap_or(owner_failure))
                        };
                    }
                    None => return Err(LocalProcessError::LifecycleControl),
                }
            }
            signal = interrupt.recv(), if !stopping => {
                if signal.is_none() {
                    return Err(LocalProcessError::SignalHandling);
                }
                stopping = true;
                *shutdown_deadline = Some(Instant::now() + SHUTDOWN_TIMEOUT);
                record.state = LocalLifecycleStateV1::Stopping;
                publish_record(paths, record)?;
                shutdown.send(()).map_err(|_| LocalProcessError::LifecycleShutdown)?;
            }
            signal = terminate.recv(), if !stopping => {
                if signal.is_none() {
                    return Err(LocalProcessError::SignalHandling);
                }
                stopping = true;
                *shutdown_deadline = Some(Instant::now() + SHUTDOWN_TIMEOUT);
                record.state = LocalLifecycleStateV1::Stopping;
                publish_record(paths, record)?;
                shutdown.send(()).map_err(|_| LocalProcessError::LifecycleShutdown)?;
            }
            () = sleep_until_deadline(startup_deadline), if !stopping && !record.owner_readiness_observed => {
                stopping = true;
                *shutdown_deadline = Some(Instant::now() + SHUTDOWN_TIMEOUT);
                failure = Some(LocalProcessError::LifecycleStartup);
                record.state = LocalLifecycleStateV1::Stopping;
                publish_record(paths, record)?;
                shutdown.send(()).map_err(|_| LocalProcessError::LifecycleShutdown)?;
            }
            () = sleep_until_optional_deadline(*shutdown_deadline), if stopping => {
                record.state = LocalLifecycleStateV1::Unknown;
                publish_record(paths, record)?;
                return Err(LocalProcessError::LifecycleShutdown);
            }
        }
    }
}

fn apply_ready_observation(record: &mut LifecycleRecordV1, stopping: bool) {
    if !stopping {
        record.state = LocalLifecycleStateV1::Running;
    }
    record.owner_readiness_observed = true;
}

fn deployment_response_for_request(
    record: &LifecycleRecordV1,
    stopping: bool,
    expected_generation: [u8; 16],
    projection: Option<VerifiedLocalDeploymentProjectionV1>,
) -> Option<InternalDeployResponseV1> {
    if stopping
        || record.state != LocalLifecycleStateV1::Running
        || !record.owner_readiness_observed
        || decode_generation(&record.generation).ok()? != expected_generation
    {
        return None;
    }
    projection.map(|projection| {
        InternalDeployResponseV1::from_verified(&record.generation, projection)
    })
}

async fn sleep_until_deadline(deadline: Instant) {
    tokio::time::sleep_until(deadline).await;
}

async fn sleep_until_optional_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => future::pending().await,
    }
}

fn wait_until_running(
    config: &LocalManagedChatConfigV1,
    mut requested_generation: Option<&str>,
    mut child: Option<&mut Child>,
    deadline: Instant,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    runtime.block_on(async {
        loop {
            let child_status = match child.as_deref_mut() {
                Some(handle) => handle
                    .try_wait()
                    .map_err(|_| LocalProcessError::LifecycleStartup)?,
                None => None,
            };
            if let Some(status) = child_status {
                if status.code() == Some(2) {
                    return Err(LocalProcessError::LifecycleConfiguration);
                }
                let contended = child_exit_was_contention(status.code());
                let observed = observe_async(config).await?;
                if !contended {
                    let changed =
                        generation_was_accepted(requested_generation, observed.generation());
                    return Ok(match observed.state() {
                        LocalLifecycleStateV1::Failed | LocalLifecycleStateV1::Unknown => {
                            observed.with_changed(changed)
                        }
                        _ => LocalLifecycleObservationV1::failed_with_evidence(
                            observed.generation,
                            observed.owner_readiness_observed,
                            LocalProcessError::LifecycleStartup,
                        )
                        .with_changed(changed),
                    });
                }
                // Only the typed hidden contention result authorizes this
                // invocation to follow a different generation. Ordinary
                // child failures (including Secret validation) never borrow
                // another request's successful owner admission.
                child = None;
                requested_generation = None;
                if Instant::now() < deadline
                    && live_owner_transition_in_progress(config, &observed)?
                {
                    // This child lost to an owner that has not yet replaced a
                    // prior terminal record. Follow the winner without keeping
                    // this request's unaccepted generation correlation.
                    child = None;
                    requested_generation = None;
                    sleep(START_POLL_INTERVAL).await;
                    continue;
                }
                match observed.state() {
                    LocalLifecycleStateV1::Running => {
                        return Ok(observed.with_changed(false));
                    }
                    LocalLifecycleStateV1::Starting => {
                        // Another same-config supervisor won the owner lock.
                        // Follow its bounded readiness without claiming that
                        // this invocation accepted the generation.
                        continue;
                    }
                    LocalLifecycleStateV1::Failed | LocalLifecycleStateV1::Unknown => {
                        return Ok(observed.with_changed(false));
                    }
                    LocalLifecycleStateV1::Stopping => {
                        return Ok(LocalLifecycleObservationV1::unknown(
                            observed.generation,
                            observed.owner_readiness_observed,
                        ));
                    }
                    LocalLifecycleStateV1::NeverStarted => {
                        if live_owner_lock_is_held(config)? {
                            // A same-config contender can win the lock before
                            // publishing its Starting record or binding the
                            // control socket. Follow that owner without
                            // attributing its mutation to this invocation.
                            continue;
                        }
                        return Ok(LocalLifecycleObservationV1::failed(
                            LocalProcessError::LifecycleStartup,
                        ));
                    }
                    LocalLifecycleStateV1::Stopped => {
                        if live_owner_lock_is_held(config)? {
                            continue;
                        }
                        return Ok(LocalLifecycleObservationV1::failed_with_evidence(
                            observed.generation,
                            observed.owner_readiness_observed,
                            LocalProcessError::LifecycleStartup,
                        ));
                    }
                }
            }
            let observation = observe_async(config).await?;
            if Instant::now() < deadline && live_owner_transition_in_progress(config, &observation)?
            {
                sleep(START_POLL_INTERVAL).await;
                continue;
            }
            if awaiting_requested_generation(
                child.is_some(),
                requested_generation,
                observation.generation(),
            ) {
                if Instant::now() >= deadline {
                    return Ok(LocalLifecycleObservationV1::unknown(
                        observation.generation,
                        observation.owner_readiness_observed,
                    ));
                }
                sleep(START_POLL_INTERVAL).await;
                continue;
            }
            match observation.state() {
                LocalLifecycleStateV1::Running => {
                    let changed =
                        generation_was_accepted(requested_generation, observation.generation());
                    return Ok(observation.with_changed(changed));
                }
                LocalLifecycleStateV1::Failed | LocalLifecycleStateV1::Unknown => {
                    let changed =
                        generation_was_accepted(requested_generation, observation.generation());
                    return Ok(observation.with_changed(changed));
                }
                LocalLifecycleStateV1::Stopping => {
                    let changed =
                        generation_was_accepted(requested_generation, observation.generation());
                    return Ok(LocalLifecycleObservationV1::unknown(
                        observation.generation,
                        observation.owner_readiness_observed,
                    )
                    .with_changed(changed));
                }
                LocalLifecycleStateV1::NeverStarted
                | LocalLifecycleStateV1::Starting
                | LocalLifecycleStateV1::Stopped => {}
            }
            if Instant::now() >= deadline {
                let changed =
                    generation_was_accepted(requested_generation, observation.generation());
                return Ok(LocalLifecycleObservationV1::unknown(
                    observation.generation,
                    observation.owner_readiness_observed,
                )
                .with_changed(changed));
            }
            sleep(START_POLL_INTERVAL).await;
        }
    })
}

fn generation_was_accepted(requested: Option<&str>, observed: Option<&str>) -> bool {
    matches!((requested, observed), (Some(requested), Some(observed)) if requested == observed)
}

fn child_exit_was_contention(exit_code: Option<i32>) -> bool {
    exit_code == Some(i32::from(LOCAL_CHAT_SUPERVISOR_CONTENTION_EXIT_CODE_V1))
}

fn awaiting_requested_generation(
    child_pending: bool,
    requested: Option<&str>,
    observed: Option<&str>,
) -> bool {
    child_pending && requested.is_some() && !generation_was_accepted(requested, observed)
}

fn observe(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    runtime.block_on(observe_async(config))
}

async fn observe_async(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    if let Some(drift) = config_authority_drift(config)? {
        return Ok(drift);
    }
    match query_live_async(config, InternalActionV1::Status).await {
        Ok(observation) => Ok(observation.into_public()),
        Err(LocalProcessError::LifecycleUnavailable) => observe_disk(config),
        Err(error) => config_drift_after_control_failure(config, error),
    }
}

fn config_drift_after_control_failure(
    config: &LocalManagedChatConfigV1,
    error: LocalProcessError,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    match config_authority_drift(config)? {
        Some(drift) => Ok(drift),
        None => Err(error),
    }
}

fn config_authority_drift(
    config: &LocalManagedChatConfigV1,
) -> Result<Option<LocalLifecycleObservationV1>, LocalProcessError> {
    let paths = LifecyclePathsV1::from_state_root(config.state_root())?;
    if !config.state_root().exists() {
        return Ok(None);
    }
    validate_existing_private_directory(config.state_root())?;
    if !paths.root.exists() {
        return Ok(None);
    }
    validate_existing_private_directory(&paths.root)?;
    let Some(record) = read_record_optional(&paths.record)? else {
        return Ok(None);
    };
    if validate_record_for_config(&record, config).is_ok() {
        return Ok(None);
    }
    Ok(Some(LocalLifecycleObservationV1::unknown_with(
        Some(record.generation),
        record.owner_readiness_observed,
        LocalProcessError::LifecycleConfiguration,
    )))
}

fn observe_disk(
    config: &LocalManagedChatConfigV1,
) -> Result<LocalLifecycleObservationV1, LocalProcessError> {
    let paths = LifecyclePathsV1::from_state_root(config.state_root())?;
    if !config.state_root().exists() {
        return Ok(LocalLifecycleObservationV1::never_started());
    }
    validate_existing_private_directory(config.state_root())?;
    if !paths.root.exists() {
        return Ok(LocalLifecycleObservationV1::never_started());
    }
    validate_existing_private_directory(&paths.root)?;
    let Some(record) = read_record_optional(&paths.record)? else {
        if paths.temporary.exists() {
            return Ok(LocalLifecycleObservationV1::unknown(None, false));
        }
        return Ok(LocalLifecycleObservationV1::never_started());
    };
    if validate_record_for_config(&record, config).is_err() {
        return Ok(LocalLifecycleObservationV1::unknown_with(
            Some(record.generation),
            record.owner_readiness_observed,
            LocalProcessError::LifecycleConfiguration,
        ));
    }
    if paths.temporary.exists() {
        return Ok(LocalLifecycleObservationV1::unknown(
            Some(record.generation),
            record.owner_readiness_observed,
        ));
    }
    let lock_held = owner_lock_is_held(&paths.lock)?;
    let observation = match record.state {
        LocalLifecycleStateV1::Starting if lock_held => {
            InternalObservationV1::from_record(&record, false).into_public()
        }
        LocalLifecycleStateV1::Stopped | LocalLifecycleStateV1::Failed if !lock_held => {
            InternalObservationV1::from_record(&record, false).into_public()
        }
        _ => LocalLifecycleObservationV1::unknown(
            Some(record.generation),
            record.owner_readiness_observed,
        ),
    };
    Ok(observation)
}

fn query_live(
    config: &LocalManagedChatConfigV1,
    action: InternalActionV1,
) -> Result<InternalObservationV1, LocalProcessError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    runtime.block_on(query_live_async(config, action))
}

async fn query_live_async(
    config: &LocalManagedChatConfigV1,
    action: InternalActionV1,
) -> Result<InternalObservationV1, LocalProcessError> {
    let paths = LifecyclePathsV1::from_state_root(config.state_root())?;
    let mut stream = match timeout(CLIENT_IO_TIMEOUT, UnixStream::connect(&paths.socket)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error))
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(LocalProcessError::LifecycleUnavailable);
        }
        Err(_) => return Err(LocalProcessError::LifecycleUnavailable),
        Ok(Err(_)) => return Err(LocalProcessError::LifecycleControl),
    };
    let credentials = stream
        .peer_cred()
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    if credentials.uid() != Uid::effective().as_raw()
        || credentials.gid() != Gid::effective().as_raw()
    {
        return Err(LocalProcessError::LifecycleControl);
    }
    let request = encode_internal_request(action, config.config_commitment());
    timeout(CLIENT_IO_TIMEOUT, stream.write_all(&request))
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)?
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    stream
        .shutdown()
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    let read_budget = if action == InternalActionV1::Down {
        SHUTDOWN_RESPONSE_TIMEOUT
    } else {
        CLIENT_IO_TIMEOUT
    };
    let mut response = Vec::new();
    let response_limit = u64::try_from(MAX_INTERNAL_RESPONSE_BYTES + 1)
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    timeout(
        read_budget,
        (&mut stream)
            .take(response_limit)
            .read_to_end(&mut response),
    )
    .await
    .map_err(|_| LocalProcessError::LifecycleControl)?
    .map_err(|_| LocalProcessError::LifecycleControl)?;
    if response.len() > MAX_INTERNAL_RESPONSE_BYTES {
        return Err(LocalProcessError::LifecycleControl);
    }
    serde_json::from_slice(&response).map_err(|_| LocalProcessError::LifecycleControl)
}

fn query_local_deployment(
    config: &LocalManagedChatConfigV1,
    expected_generation: [u8; 16],
) -> Result<InternalDeployResponseV1, LocalProcessError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    runtime.block_on(query_local_deployment_async(config, expected_generation))
}

async fn query_local_deployment_async(
    config: &LocalManagedChatConfigV1,
    expected_generation: [u8; 16],
) -> Result<InternalDeployResponseV1, LocalProcessError> {
    let paths = LifecyclePathsV1::from_state_root(config.state_root())
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    let mut stream = timeout(CLIENT_IO_TIMEOUT, UnixStream::connect(&paths.socket))
        .await
        .map_err(|_| LocalProcessError::LocalDeployQuery)?
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    let credentials = stream
        .peer_cred()
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    if credentials.uid() != Uid::effective().as_raw()
        || credentials.gid() != Gid::effective().as_raw()
    {
        return Err(LocalProcessError::LocalDeployQuery);
    }
    let request = encode_internal_deploy_query(
        config.config_commitment(),
        expected_generation,
    );
    timeout(CLIENT_IO_TIMEOUT, stream.write_all(&request))
        .await
        .map_err(|_| LocalProcessError::LocalDeployQuery)?
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    stream
        .shutdown()
        .await
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    let mut response = Vec::new();
    let response_limit = u64::try_from(MAX_INTERNAL_RESPONSE_BYTES + 1)
        .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    timeout(
        CLIENT_IO_TIMEOUT,
        (&mut stream)
            .take(response_limit)
            .read_to_end(&mut response),
    )
    .await
    .map_err(|_| LocalProcessError::LocalDeployQuery)?
    .map_err(|_| LocalProcessError::LocalDeployQuery)?;
    if response.is_empty() || response.len() > MAX_INTERNAL_RESPONSE_BYTES {
        return Err(LocalProcessError::LocalDeployQuery);
    }
    serde_json::from_slice(&response).map_err(|_| LocalProcessError::LocalDeployEvidence)
}

fn encode_internal_request(action: InternalActionV1, commitment: [u8; 32]) -> [u8; 38] {
    let mut request = [0_u8; INTERNAL_REQUEST_BYTES];
    request[..4].copy_from_slice(&INTERNAL_MAGIC);
    request[4] = INTERNAL_PROTOCOL_VERSION;
    request[5] = action.wire();
    request[6..].copy_from_slice(&commitment);
    request
}

fn encode_internal_deploy_query(
    commitment: [u8; 32],
    expected_generation: [u8; 16],
) -> [u8; INTERNAL_DEPLOY_QUERY_BYTES] {
    let mut request = [0_u8; INTERNAL_DEPLOY_QUERY_BYTES];
    request[..INTERNAL_REQUEST_BYTES]
        .copy_from_slice(&encode_internal_request(InternalActionV1::Deploy, commitment));
    request[INTERNAL_REQUEST_BYTES..].copy_from_slice(&expected_generation);
    request
}

async fn read_internal_request(
    stream: &mut UnixStream,
    expected_commitment: [u8; 32],
) -> Result<InternalRequestV1, LocalProcessError> {
    let mut request = [0_u8; INTERNAL_REQUEST_BYTES];
    timeout(CLIENT_IO_TIMEOUT, stream.read_exact(&mut request))
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)?
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    if request[..4] != INTERNAL_MAGIC
        || request[4] != INTERNAL_PROTOCOL_VERSION
        || request[6..] != expected_commitment
    {
        return Err(LocalProcessError::LifecycleConfiguration);
    }
    let action = InternalActionV1::decode(request[5]).ok_or(LocalProcessError::LifecycleControl)?;
    let expected_generation = if action == InternalActionV1::Deploy {
        let mut generation = [0_u8; 16];
        timeout(CLIENT_IO_TIMEOUT, stream.read_exact(&mut generation))
            .await
            .map_err(|_| LocalProcessError::LifecycleControl)?
            .map_err(|_| LocalProcessError::LifecycleControl)?;
        if generation.iter().all(|byte| *byte == 0) {
            return Err(LocalProcessError::LifecycleControl);
        }
        Some(generation)
    } else {
        None
    };
    Ok(InternalRequestV1 {
        action,
        expected_generation,
    })
}

async fn write_internal_observation(
    stream: &mut UnixStream,
    observation: &InternalObservationV1,
) -> Result<(), LocalProcessError> {
    let wire = serde_json::to_vec(observation).map_err(|_| LocalProcessError::LifecycleControl)?;
    if wire.len() > MAX_INTERNAL_RESPONSE_BYTES {
        return Err(LocalProcessError::LifecycleControl);
    }
    timeout(CLIENT_IO_TIMEOUT, stream.write_all(&wire))
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)?
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    stream
        .shutdown()
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)
}

async fn write_internal_deploy_response(
    stream: &mut UnixStream,
    response: &InternalDeployResponseV1,
) -> Result<(), LocalProcessError> {
    let wire = serde_json::to_vec(response).map_err(|_| LocalProcessError::LifecycleControl)?;
    if wire.is_empty() || wire.len() > MAX_INTERNAL_RESPONSE_BYTES {
        return Err(LocalProcessError::LifecycleControl);
    }
    timeout(CLIENT_IO_TIMEOUT, stream.write_all(&wire))
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)?
        .map_err(|_| LocalProcessError::LifecycleControl)?;
    stream
        .shutdown()
        .await
        .map_err(|_| LocalProcessError::LifecycleControl)
}

fn peer_matches(stream: &UnixStream, uid: u32, gid: u32) -> bool {
    stream
        .peer_cred()
        .is_ok_and(|credentials| credentials.uid() == uid && credentials.gid() == gid)
}

fn validate_execution_identity() -> Result<(), LocalProcessError> {
    if Uid::effective().is_root() || Gid::effective().as_raw() == 0 {
        return Err(LocalProcessError::UnsafeExecutionIdentity);
    }
    Ok(())
}

fn ensure_private_state_root(path: &Path) -> Result<PathBuf, LocalProcessError> {
    let uid = Uid::effective().as_raw();
    let gid = Gid::effective().as_raw();
    let (created, concurrent_creation) = match fs::symlink_metadata(path) {
        Ok(metadata) => (
            false,
            metadata_could_be_private_directory_initialization(&metadata),
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(LocalProcessError::LifecycleState)?;
            validate_existing_path_chain(parent)?;
            let canonical_parent =
                fs::canonicalize(parent).map_err(|_| LocalProcessError::LifecycleState)?;
            if canonical_parent != parent {
                return Err(LocalProcessError::LifecycleState);
            }
            match DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {
                    chown(path, None, Some(Gid::from_raw(gid)))
                        .map_err(|_| LocalProcessError::LifecycleState)?;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                        .map_err(|_| LocalProcessError::LifecycleState)?;
                    sync_directory(parent)?;
                    (true, false)
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (false, true),
                Err(_) => return Err(LocalProcessError::LifecycleState),
            }
        }
        Err(_) => return Err(LocalProcessError::LifecycleState),
    };
    validate_existing_path_chain(path)?;
    if concurrent_creation {
        validate_concurrently_created_private_directory(path)?;
    } else {
        validate_existing_private_directory(path)?;
    }
    let canonical = fs::canonicalize(path).map_err(|_| LocalProcessError::LifecycleState)?;
    if canonical != path {
        return Err(LocalProcessError::LifecycleState);
    }
    if created {
        sync_directory(
            canonical
                .parent()
                .ok_or(LocalProcessError::LifecycleState)?,
        )?;
    }
    if uid == 0 || gid == 0 {
        return Err(LocalProcessError::UnsafeExecutionIdentity);
    }
    Ok(canonical)
}

fn ensure_private_directory(path: &Path) -> Result<(), LocalProcessError> {
    let gid = Gid::effective().as_raw();
    let concurrent_creation = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata_could_be_private_directory_initialization(&metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {
                    chown(path, None, Some(Gid::from_raw(gid)))
                        .map_err(|_| LocalProcessError::LifecycleState)?;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                        .map_err(|_| LocalProcessError::LifecycleState)?;
                    sync_directory(path.parent().ok_or(LocalProcessError::LifecycleState)?)?;
                    false
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => true,
                Err(_) => return Err(LocalProcessError::LifecycleState),
            }
        }
        Err(_) => return Err(LocalProcessError::LifecycleState),
    };
    if concurrent_creation {
        validate_concurrently_created_private_directory(path)
    } else {
        validate_existing_private_directory(path)
    }
}

fn validate_concurrently_created_private_directory(path: &Path) -> Result<(), LocalProcessError> {
    let deadline = Instant::now() + CONCURRENT_DIRECTORY_INITIALIZATION_TIMEOUT;
    loop {
        let metadata = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LifecycleState)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != Uid::effective().as_raw()
            || metadata.permissions().mode() & 0o7077 != 0
        {
            return Err(LocalProcessError::LifecycleState);
        }
        match validate_existing_private_directory(path) {
            Ok(()) => return Ok(()),
            Err(_) if Instant::now() < deadline => thread::sleep(START_POLL_INTERVAL),
            Err(error) => return Err(error),
        }
    }
}

fn metadata_could_be_private_directory_initialization(metadata: &fs::Metadata) -> bool {
    let mode = metadata.permissions().mode() & 0o7777;
    !metadata.file_type().is_symlink()
        && metadata.is_dir()
        && metadata.uid() == Uid::effective().as_raw()
        && mode & 0o7077 == 0
        && (metadata.gid() != Gid::effective().as_raw() || mode != 0o700)
}

fn validate_existing_private_directory(path: &Path) -> Result<(), LocalProcessError> {
    let before = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LifecycleState)?;
    if before.file_type().is_symlink()
        || !before.is_dir()
        || before.uid() != Uid::effective().as_raw()
        || before.gid() != Gid::effective().as_raw()
        || before.permissions().mode() & 0o7777 != 0o700
    {
        return Err(LocalProcessError::LifecycleState);
    }
    let canonical = fs::canonicalize(path).map_err(|_| LocalProcessError::LifecycleState)?;
    if canonical != path {
        return Err(LocalProcessError::LifecycleState);
    }
    let after = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LifecycleState)?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err(LocalProcessError::LifecycleState);
    }
    Ok(())
}

fn validate_existing_path_chain(path: &Path) -> Result<(), LocalProcessError> {
    if !path.is_absolute() {
        return Err(LocalProcessError::LifecycleState);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|_| LocalProcessError::LifecycleState)?;
                if metadata.file_type().is_symlink() {
                    return Err(LocalProcessError::LifecycleState);
                }
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(LocalProcessError::LifecycleState);
            }
        }
    }
    Ok(())
}

fn validate_operator_entries(paths: &LifecyclePathsV1) -> Result<(), LocalProcessError> {
    for entry in fs::read_dir(&paths.root).map_err(|_| LocalProcessError::LifecycleState)? {
        let entry = entry.map_err(|_| LocalProcessError::LifecycleState)?;
        let name = entry.file_name();
        let admitted = name.as_os_str() == OsStr::new(OWNER_LOCK_FILE)
            || name.as_os_str() == OsStr::new(RECORD_FILE)
            || name.as_os_str() == OsStr::new(RECORD_TEMP_FILE)
            || name.as_os_str() == OsStr::new(CONTROL_SOCKET_FILE);
        if !admitted {
            return Err(LocalProcessError::LifecycleState);
        }
    }
    Ok(())
}

fn acquire_owner_lock(path: &Path) -> Result<File, LocalProcessError> {
    let (file, created) = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => (file, true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (
            OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
                .open(path)
                .map_err(|_| LocalProcessError::LifecycleState)?,
            false,
        ),
        Err(_) => return Err(LocalProcessError::LifecycleState),
    };
    if created {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| LocalProcessError::LifecycleState)?;
        file.sync_all()
            .map_err(|_| LocalProcessError::LifecycleState)?;
        sync_directory(path.parent().ok_or(LocalProcessError::LifecycleState)?)?;
    }
    validate_private_file(path, &file)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(LocalProcessError::LifecycleUnavailable),
        Err(TryLockError::Error(_)) => Err(LocalProcessError::LifecycleState),
    }
}

fn owner_lock_is_held(path: &Path) -> Result<bool, LocalProcessError> {
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(LocalProcessError::LifecycleState);
        }
        Err(_) => return Err(LocalProcessError::LifecycleState),
    };
    validate_private_file(path, &file)?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(_)) => Err(LocalProcessError::LifecycleState),
    }
}

fn live_owner_lock_is_held(config: &LocalManagedChatConfigV1) -> Result<bool, LocalProcessError> {
    let paths = LifecyclePathsV1::from_state_root(config.state_root())?;
    if !config.state_root().exists() || !paths.root.exists() || !paths.lock.exists() {
        return Ok(false);
    }
    validate_existing_private_directory(config.state_root())?;
    validate_existing_private_directory(&paths.root)?;
    owner_lock_is_held(&paths.lock)
}

fn validate_private_file(path: &Path, file: &File) -> Result<(), LocalProcessError> {
    let before = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LifecycleState)?;
    let opened = file
        .metadata()
        .map_err(|_| LocalProcessError::LifecycleState)?;
    let after = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LifecycleState)?;
    for metadata in [&before, &opened, &after] {
        if !metadata.is_file()
            || metadata.uid() != Uid::effective().as_raw()
            || metadata.gid() != Gid::effective().as_raw()
            || metadata.permissions().mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(LocalProcessError::LifecycleState);
        }
    }
    if before.dev() != opened.dev()
        || before.ino() != opened.ino()
        || opened.dev() != after.dev()
        || opened.ino() != after.ino()
    {
        return Err(LocalProcessError::LifecycleState);
    }
    Ok(())
}

fn read_record_optional(path: &Path) -> Result<Option<LifecycleRecordV1>, LocalProcessError> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(LocalProcessError::LifecycleState),
    };
    validate_private_file(path, &file)?;
    let metadata = file
        .metadata()
        .map_err(|_| LocalProcessError::LifecycleState)?;
    if metadata.len() > MAX_RECORD_BYTES {
        return Err(LocalProcessError::LifecycleState);
    }
    let mut wire = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut wire)
        .map_err(|_| LocalProcessError::LifecycleState)?;
    if wire.len() as u64 > MAX_RECORD_BYTES {
        return Err(LocalProcessError::LifecycleState);
    }
    let record: LifecycleRecordV1 =
        serde_json::from_slice(&wire).map_err(|_| LocalProcessError::LifecycleState)?;
    validate_record_shape(&record)?;
    Ok(Some(record))
}

fn validate_record_shape(record: &LifecycleRecordV1) -> Result<(), LocalProcessError> {
    if record.schema_version != RECORD_SCHEMA_VERSION
        || decode_lower_hex_32(&record.config_commitment).is_err()
        || decode_generation(&record.generation).is_err()
        || matches!(record.state, LocalLifecycleStateV1::NeverStarted)
    {
        return Err(LocalProcessError::LifecycleState);
    }
    Ok(())
}

fn validate_record_for_config(
    record: &LifecycleRecordV1,
    config: &LocalManagedChatConfigV1,
) -> Result<(), LocalProcessError> {
    validate_record_shape(record)?;
    if decode_lower_hex_32(&record.config_commitment)? != config.config_commitment() {
        return Err(LocalProcessError::LifecycleConfiguration);
    }
    Ok(())
}

fn publish_record(
    paths: &LifecyclePathsV1,
    record: &LifecycleRecordV1,
) -> Result<(), LocalProcessError> {
    let wire = serde_json::to_vec(record).map_err(|_| LocalProcessError::LifecycleState)?;
    if wire.len() as u64 > MAX_RECORD_BYTES {
        return Err(LocalProcessError::LifecycleState);
    }
    let mut temporary = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&paths.temporary)
        .map_err(|_| LocalProcessError::LifecycleState)?;
    let result = (|| {
        temporary
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| LocalProcessError::LifecycleState)?;
        validate_private_file(&paths.temporary, &temporary)?;
        temporary
            .write_all(&wire)
            .and_then(|()| temporary.sync_all())
            .map_err(|_| LocalProcessError::LifecycleState)?;
        fs::rename(&paths.temporary, &paths.record)
            .map_err(|_| LocalProcessError::LifecycleState)?;
        sync_directory(&paths.root)?;
        validate_private_file(&paths.record, &temporary)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&paths.temporary);
    }
    result
}

fn remove_terminal_stale_socket(path: &Path) -> Result<(), LocalProcessError> {
    match StdUnixStream::connect(path) {
        Ok(_) => return Err(LocalProcessError::LifecycleUnavailable),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) => {}
        Err(_) => return Err(LocalProcessError::LifecycleControl),
    }
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_socket()
                && metadata.uid() == Uid::effective().as_raw()
                && metadata.gid() == Gid::effective().as_raw() =>
        {
            fs::remove_file(path).map_err(|_| LocalProcessError::LifecycleState)?;
            sync_directory(path.parent().ok_or(LocalProcessError::LifecycleState)?)
        }
        Ok(_) => Err(LocalProcessError::LifecycleState),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(LocalProcessError::LifecycleState),
    }
}

fn new_generation() -> Result<String, LocalProcessError> {
    let mut generation = [0_u8; 16];
    getrandom::fill(&mut generation).map_err(|_| LocalProcessError::LifecycleStartup)?;
    if generation.iter().all(|byte| *byte == 0) {
        return Err(LocalProcessError::LifecycleStartup);
    }
    Ok(lower_hex(&generation))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(*byte >> 4)]));
        result.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    result
}

fn decode_lower_hex_32(value: &str) -> Result<[u8; 32], LocalProcessError> {
    decode_config_commitment_hex(std::ffi::OsStr::new(value))
}

fn decode_generation(value: &str) -> Result<[u8; 16], LocalProcessError> {
    if value.len() != 32 {
        return Err(LocalProcessError::LifecycleState);
    }
    let mut generation = [0_u8; 16];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        generation[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    if generation.iter().all(|byte| *byte == 0) {
        return Err(LocalProcessError::LifecycleState);
    }
    Ok(generation)
}

const fn hex_nibble(value: u8) -> Result<u8, LocalProcessError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(LocalProcessError::LifecycleConfiguration),
    }
}

fn sync_directory(path: &Path) -> Result<(), LocalProcessError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| LocalProcessError::LifecycleState)
}

fn diagnostic(error: LocalProcessError) -> LocalLifecycleDiagnosticV1 {
    LocalLifecycleDiagnosticV1 {
        code: error.code(),
        message: error.message(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_request_binds_action_and_exact_config_commitment() {
        let commitment = [0x5a; 32];
        let status = encode_internal_request(InternalActionV1::Status, commitment);
        let down = encode_internal_request(InternalActionV1::Down, commitment);
        assert_eq!(&status[..4], &INTERNAL_MAGIC);
        assert_eq!(status[4], INTERNAL_PROTOCOL_VERSION);
        assert_eq!(status[5], b'S');
        assert_eq!(&status[6..], &commitment);
        assert_eq!(down[5], b'D');
        assert_ne!(status, down);

        let expected_generation = [0x6b; 16];
        let deploy = encode_internal_deploy_query(commitment, expected_generation);
        assert_eq!(deploy.len(), INTERNAL_DEPLOY_QUERY_BYTES);
        assert_eq!(&deploy[..4], &INTERNAL_MAGIC);
        assert_eq!(deploy[4], INTERNAL_PROTOCOL_VERSION);
        assert_eq!(deploy[5], b'P');
        assert_eq!(&deploy[6..INTERNAL_REQUEST_BYTES], &commitment);
        assert_eq!(&deploy[INTERNAL_REQUEST_BYTES..], &expected_generation);
    }

    #[test]
    fn internal_deploy_response_is_strict_and_preserves_exact_owner_evidence() {
        let generation = [0x41; 16];
        let verified = VerifiedLocalDeploymentProjectionV1::for_test(
            u64::MAX,
            9_007_199_254_740_992,
            [0x52; 32],
            [0x63; 32],
            true,
            false,
        );
        let response = InternalDeployResponseV1::from_verified(&lower_hex(&generation), verified);
        let wire = serde_json::to_vec(&response).expect("internal deploy response");
        assert!(wire.len() <= MAX_INTERNAL_RESPONSE_BYTES);
        let decoded: InternalDeployResponseV1 =
            serde_json::from_slice(&wire).expect("strict internal deploy response");
        let projection = decoded
            .into_projection(generation)
            .expect("verified local deploy projection");
        assert_eq!(projection.controller_revision(), u64::MAX);
        assert_eq!(
            projection.controller_snapshot_sequence(),
            9_007_199_254_740_992
        );
        assert_eq!(projection.runtime_apply_request_digest(), [0x52; 32]);
        assert_eq!(projection.runtime_terminal_receipt_digest(), [0x63; 32]);
        assert!(response.fabric_replayed);
        assert!(!projection.model_agent_replayed());

        let mut with_unknown: serde_json::Value =
            serde_json::from_slice(&wire).expect("internal response value");
        with_unknown
            .as_object_mut()
            .expect("response object")
            .insert("path".to_owned(), serde_json::Value::String("forbidden".to_owned()));
        assert!(serde_json::from_value::<InternalDeployResponseV1>(with_unknown).is_err());

        let mut mismatch = response.clone();
        mismatch.generation = lower_hex(&[0x42; 16]).into_boxed_str();
        assert_eq!(
            mismatch.into_projection(generation),
            Err(LocalProcessError::LocalDeployEvidence)
        );
        let mut invalid_outcome = response;
        invalid_outcome.terminal_outcome = "unknown".into();
        assert_eq!(
            invalid_outcome.into_projection(generation),
            Err(LocalProcessError::LocalDeployEvidence)
        );
    }

    #[test]
    fn deploy_query_is_generation_bound_fails_during_down_and_never_retries() {
        let generation = [0x71; 16];
        let record = LifecycleRecordV1 {
            schema_version: RECORD_SCHEMA_VERSION,
            config_commitment: lower_hex(&[0x72; 32]).into_boxed_str(),
            generation: lower_hex(&generation).into_boxed_str(),
            state: LocalLifecycleStateV1::Running,
            owner_readiness_observed: true,
        };
        let projection = VerifiedLocalDeploymentProjectionV1::for_test(
            7,
            11,
            [0x73; 32],
            [0x74; 32],
            false,
            false,
        );
        assert!(
            deployment_response_for_request(&record, false, generation, Some(projection)).is_some()
        );
        assert!(
            deployment_response_for_request(&record, false, [0x75; 16], Some(projection)).is_none()
        );
        assert!(
            deployment_response_for_request(&record, true, generation, Some(projection)).is_none()
        );
        assert_eq!(record.state, LocalLifecycleStateV1::Running);

        let accepted_then_query_failed =
            LocalDeployFailureV1::after_up(LocalProcessError::LocalDeployQuery, true);
        assert_eq!(accepted_then_query_failed.changed(), None);
        let follower_then_query_failed =
            LocalDeployFailureV1::after_up(LocalProcessError::LocalDeployQuery, false);
        assert_eq!(follower_then_query_failed.changed(), Some(false));

        let successor_replaced_accepted_generation =
            LocalLifecycleObservationV1::failed_with_evidence(
                Some(lower_hex(&[0x76; 16]).into_boxed_str()),
                true,
                LocalProcessError::LifecycleStartup,
            )
            .with_changed(false);
        assert_eq!(
            local_deploy_non_running_failure(&successor_replaced_accepted_generation).changed(),
            None
        );
        let pre_effect_config_drift = LocalLifecycleObservationV1::unknown_with(
            Some(lower_hex(&[0x77; 16]).into_boxed_str()),
            false,
            LocalProcessError::LifecycleConfiguration,
        );
        assert_eq!(
            local_deploy_non_running_failure(&pre_effect_config_drift).changed(),
            Some(false)
        );

        assert!(local_deploy_changed(true, false));
        assert!(!local_deploy_changed(true, true));
        assert!(!local_deploy_changed(false, false));
        assert!(!local_deploy_changed(false, true));

        let source = include_str!("lifecycle.rs");
        let deploy = source
            .split("pub(crate) fn run_local_deploy(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn run_supervisor(").next())
            .expect("bounded local deploy source");
        assert_eq!(deploy.matches("query_local_deployment(").count(), 1);
        assert!(!deploy.contains("loop {"));
        assert!(!deploy.contains("retry"));
    }

    #[test]
    fn record_wire_is_strict_bounded_and_contains_no_process_identity() {
        let record = LifecycleRecordV1 {
            schema_version: RECORD_SCHEMA_VERSION,
            config_commitment: lower_hex(&[0x33; 32]).into_boxed_str(),
            generation: lower_hex(&[0x44; 16]).into_boxed_str(),
            state: LocalLifecycleStateV1::Running,
            owner_readiness_observed: true,
        };
        let wire = serde_json::to_vec(&record).expect("record wire");
        assert!(wire.len() < MAX_RECORD_BYTES as usize);
        assert_eq!(
            serde_json::from_slice::<LifecycleRecordV1>(&wire).expect("strict record"),
            record
        );
        let text = String::from_utf8(wire).expect("UTF-8 record");
        assert!(!text.contains("pid"));
        assert!(!text.contains("path"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn lifecycle_observation_never_upgrades_process_state_to_health() {
        let observation = InternalObservationV1 {
            state: LocalLifecycleStateV1::Running,
            generation: Some(lower_hex(&[0x55; 16]).into_boxed_str()),
            changed: false,
            owner_readiness_observed: true,
            diagnostic_code: None,
        }
        .into_public();
        assert!(observation.ok());
        assert_eq!(observation.state().as_str(), "running");
        assert!(observation.owner_readiness_observed());
        assert!(!observation.changed());
        assert_eq!(observation.diagnostic(), None);
        let source = include_str!("lifecycle.rs");
        let tests_start = source
            .rfind("\n#[cfg(test)]\nmod tests {")
            .expect("lifecycle test module");
        let production = &source[..tests_start];
        assert!(!production.contains("paraegox_runtime_host::service_manager"));
        assert!(!production.contains("kill(Pid"));
        assert!(!production.contains("process_id"));
    }

    #[test]
    fn failed_and_unknown_observations_pair_with_exit_one_without_claiming_mutation() {
        assert!(!generation_was_accepted(None, None));
        assert!(!generation_was_accepted(Some("aa"), None));
        assert!(!generation_was_accepted(None, Some("aa")));
        assert!(generation_was_accepted(Some("aa"), Some("aa")));
        assert!(!generation_was_accepted(Some("aa"), Some("bb")));

        let failed = LocalLifecycleObservationV1::failed(LocalProcessError::LifecycleStartup);
        assert!(!failed.ok());
        assert!(!failed.changed());
        assert_eq!(failed.exit_code(), 1);

        let prior_generation = lower_hex(&[0x65; 16]).into_boxed_str();
        let failed_after_stopped = LocalLifecycleObservationV1::failed_with_evidence(
            Some(prior_generation.clone()),
            true,
            LocalProcessError::LifecycleStartup,
        );
        assert_eq!(
            failed_after_stopped.generation(),
            Some(prior_generation.as_ref())
        );
        assert!(failed_after_stopped.owner_readiness_observed());
        assert!(!failed_after_stopped.changed());
        assert_eq!(failed_after_stopped.exit_code(), 1);

        let unknown_without_diagnostic = InternalObservationV1 {
            state: LocalLifecycleStateV1::Unknown,
            generation: Some(lower_hex(&[0x66; 16]).into_boxed_str()),
            changed: false,
            owner_readiness_observed: true,
            diagnostic_code: None,
        }
        .into_public();
        assert!(!unknown_without_diagnostic.ok());
        assert!(!unknown_without_diagnostic.changed());
        assert_eq!(unknown_without_diagnostic.exit_code(), 1);
    }

    #[test]
    fn only_typed_hidden_contention_can_follow_an_unrelated_generation() {
        assert!(!child_exit_was_contention(None));
        assert!(!child_exit_was_contention(Some(0)));
        assert!(!child_exit_was_contention(Some(1)));
        assert!(!child_exit_was_contention(Some(2)));
        assert!(child_exit_was_contention(Some(i32::from(
            LOCAL_CHAT_SUPERVISOR_CONTENTION_EXIT_CODE_V1,
        ))));

        assert!(awaiting_requested_generation(
            true,
            Some("requested"),
            Some("other"),
        ));
        assert!(awaiting_requested_generation(true, Some("requested"), None,));
        assert!(!awaiting_requested_generation(
            true,
            Some("requested"),
            Some("requested"),
        ));
        assert!(!awaiting_requested_generation(
            false,
            Some("requested"),
            Some("other"),
        ));
        assert!(!awaiting_requested_generation(false, None, None));
    }

    #[test]
    fn hidden_contention_is_minted_only_at_the_exact_owner_lock_boundary() {
        let source = include_str!("lifecycle.rs");
        let asynchronous = source
            .split("async fn run_supervisor_async(")
            .nth(1)
            .and_then(|tail| tail.split("async fn wait_for_thread_exit(").next())
            .expect("bounded lifecycle owner source");
        assert_eq!(
            asynchronous
                .matches("LocalChatSupervisorResultV1::Contended")
                .count(),
            1
        );
        let contention = asynchronous
            .find("return Ok(LocalChatSupervisorResultV1::Contended)")
            .expect("typed owner contention result");
        let owner_lock = asynchronous
            .find("acquire_owner_lock(&paths.lock)")
            .expect("exact owner lock admission");
        let stale_socket = asynchronous
            .find("remove_terminal_stale_socket(&paths.socket)")
            .expect("stale socket check");
        assert!(owner_lock < contention);
        assert!(contention < stale_socket);
    }

    #[test]
    fn concurrent_directory_creation_waits_for_exact_private_metadata_settle() {
        let parent = fs::canonicalize(std::env::temp_dir()).expect("canonical temporary root");
        let unique = new_generation().expect("unique test directory");
        let path = parent.join(format!("paraegox-lifecycle-directory-race-{unique}"));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("temporary private directory");
        chown(&path, None, Some(Gid::effective())).expect("temporary directory group");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .expect("transient directory mode");
        let transient = fs::symlink_metadata(&path).expect("transient directory metadata");
        assert!(metadata_could_be_private_directory_initialization(
            &transient
        ));

        let repaired_path = path.clone();
        let repair = thread::spawn(move || {
            thread::sleep(START_POLL_INTERVAL * 2);
            fs::set_permissions(&repaired_path, fs::Permissions::from_mode(0o700))
        });
        let validation = validate_concurrently_created_private_directory(&path);
        let repair_result = repair.join().expect("metadata repair thread");
        let cleanup = fs::remove_dir(&path);

        repair_result.expect("settled directory permissions");
        assert_eq!(validation, Ok(()));
        cleanup.expect("remove temporary private directory");
    }

    #[test]
    fn operation_failure_projection_never_attributes_an_uncorrelated_mutation() {
        let source = include_str!("lifecycle.rs");
        let projection = source
            .split("pub(crate) fn project_failure(")
            .nth(1)
            .and_then(|tail| tail.split("fn read_failure_evidence(").next())
            .expect("bounded failure projection source");
        assert!(projection.contains("let changed = false;"));
        assert!(!projection.contains("record.state,"));
    }

    #[test]
    fn status_and_down_recheck_config_authority_after_control_failure() {
        let source = include_str!("lifecycle.rs");
        let down = source
            .split("pub(crate) fn run_down(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn run_supervisor(").next())
            .expect("bounded down source");
        assert!(down.contains("config_drift_after_control_failure(config, error)"));

        let status = source
            .split("async fn observe_async(")
            .nth(1)
            .and_then(|tail| tail.split("fn config_drift_after_control_failure(").next())
            .expect("bounded status source");
        assert!(status.contains("config_drift_after_control_failure(config, error)"));

        let recheck = source
            .split("fn config_drift_after_control_failure(")
            .nth(1)
            .and_then(|tail| tail.split("fn config_authority_drift(").next())
            .expect("bounded config recheck source");
        assert!(recheck.contains("config_authority_drift(config)?"));
        assert!(recheck.contains("Some(drift) => Ok(drift)"));
    }

    #[test]
    fn provider_secret_is_prepared_before_detach_or_lifecycle_state_effects() {
        let source = include_str!("lifecycle.rs");
        let supervisor = source
            .split("pub(crate) fn run_supervisor(")
            .nth(1)
            .and_then(|tail| tail.split("pub(crate) fn project_failure(").next())
            .expect("bounded hidden supervisor source");
        let prepare = supervisor
            .find("prepare_headless_chat(config.clone().into_owner_config())?")
            .expect("single-use Secret preparation");
        let detach = supervisor.find("setsid()").expect("session detach");
        let lifecycle = supervisor
            .find("run_supervisor_async(")
            .expect("lifecycle state boundary");
        assert!(prepare < detach);
        assert!(detach < lifecycle);

        let asynchronous = source
            .split("async fn run_supervisor_async(")
            .nth(1)
            .and_then(|tail| tail.split("async fn wait_for_thread_exit(").next())
            .expect("bounded lifecycle owner source");
        assert!(asynchronous.contains("prepared: PreparedHeadlessChatV1"));
        assert!(!asynchronous.contains("std::env::"));
        assert!(!asynchronous.contains("resolve_provisioned_api_key"));
    }

    #[test]
    fn readiness_latch_remains_monotonic_when_shutdown_wins_the_race() {
        let mut record = LifecycleRecordV1 {
            schema_version: RECORD_SCHEMA_VERSION,
            config_commitment: lower_hex(&[0x77; 32]).into_boxed_str(),
            generation: lower_hex(&[0x88; 16]).into_boxed_str(),
            state: LocalLifecycleStateV1::Stopping,
            owner_readiness_observed: false,
        };

        apply_ready_observation(&mut record, true);

        assert_eq!(record.state, LocalLifecycleStateV1::Stopping);
        assert!(record.owner_readiness_observed);
    }

    #[test]
    fn terminal_response_follows_join_owned_socket_cleanup_and_durable_record() {
        let source = include_str!("lifecycle.rs");
        let finalization = source
            .split("async fn run_supervisor_async(")
            .nth(1)
            .and_then(|tail| tail.split("async fn wait_for_thread_exit(").next())
            .expect("bounded supervisor finalization source");
        let joined = finalization
            .find("wait_for_thread_exit(&composition")
            .expect("joined owner wait");
        let socket_cleanup = finalization
            .find("owned_socket.remove_owned()")
            .expect("fallible socket cleanup");
        let terminal_state = finalization
            .find("record.state = if final_result.is_ok()")
            .expect("terminal state classification");
        let durable_record = finalization
            .rfind("publish_record(&paths, &record)")
            .expect("terminal durable record");
        let response = finalization
            .rfind("write_internal_observation(&mut waiter")
            .expect("terminal waiter response");
        assert!(joined < socket_cleanup);
        assert!(socket_cleanup < terminal_state);
        assert!(terminal_state < durable_record);
        assert!(durable_record < response);
    }

    #[test]
    fn status_response_transport_failure_is_request_local() {
        let source = include_str!("lifecycle.rs");
        let supervisor = source
            .split("async fn supervise(")
            .nth(1)
            .and_then(|tail| tail.split("fn apply_ready_observation(").next())
            .expect("bounded supervisor source");
        let status = supervisor
            .split("InternalActionV1::Status => {")
            .nth(1)
            .and_then(|tail| tail.split("InternalActionV1::Down => {").next())
            .expect("bounded status branch");
        assert!(status.contains("let _ = write_internal_observation("));
        assert!(!status.contains(".await?"));
    }
}
