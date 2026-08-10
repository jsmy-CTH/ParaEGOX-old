//! One-shot CLI client for the existing DeveloperLocal Inspection endpoint.
//!
//! This adapter owns no discovery, cache, freshness calculation, health
//! inference, lifecycle mutation, retry, watch, or reconnect policy. It opens
//! one generation-selected owner-private PXIB file, performs one authenticated
//! PXIQ-v2 `Latest` exchange, and projects the already validated PXIS-v2 value
//! into the public CLI JSON shape.

use std::fs::{self, File, OpenOptions};
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use nix::unistd::{Gid, Uid};
use paraegox_inspection::developer_local::{
    DEVELOPER_LOCAL_INSPECTION_BOOTSTRAP_V2_HEADER_BYTES, DeveloperLocalInspectionBootstrapV2,
    MAX_DEVELOPER_LOCAL_INSPECTION_BOOTSTRAP_V2_BYTES, encode_authenticated_request_v2,
};
use paraegox_inspection::protocol::{
    InspectionClientErrorV2, InspectionClientV2, InspectionEndpointErrorV2, InspectionEndpointV2,
    InspectionRequestV2, InspectionResponseOutcomeV2, MAX_INSPECTION_RESPONSE_V2_BYTES,
};
use paraegox_inspection::{
    InspectionFeatureSupportV1, InspectionFreshnessV1, InspectionHealthV1, InspectionLivenessV1,
    InspectionReadinessV1, InspectionReasonV1, InspectionSourceCoordinateV1,
    InspectionSourceOwnerV1, LOCAL_INSPECTION_SNAPSHOT_V2_VERSION, LocalInspectionOverallV1,
    LocalInspectionRecordV1, LocalInspectionSnapshotV2, NodeInspectionRecordV2,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream as TokioUnixStream;
use tokio::time::{Instant, timeout_at};

use crate::error::LocalProcessError;
use crate::lifecycle::LocalInspectionBootstrapLocatorV1;

const PRIVATE_FILE_MODE: u32 = 0o600;
const SOCKET_DIRECTORY_MODE: u32 = 0o2750;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentityV1 {
    device: u64,
    inode: u64,
}

impl FileIdentityV1 {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

struct DeveloperLocalInspectionEndpointV2 {
    runtime: tokio::runtime::Runtime,
    bootstrap: DeveloperLocalInspectionBootstrapV2,
    socket_identity: FileIdentityV1,
    last_failure: Option<LocalProcessError>,
}

impl InspectionEndpointV2 for DeveloperLocalInspectionEndpointV2 {
    fn exchange(
        &mut self,
        canonical_request: &[u8],
    ) -> Result<Box<[u8]>, InspectionEndpointErrorV2> {
        let result = self.runtime.block_on(exchange_once(
            &self.bootstrap,
            self.socket_identity,
            canonical_request,
        ));
        match result {
            Ok(response) => Ok(response),
            Err(error) => {
                self.last_failure = Some(error);
                Err(match error {
                    LocalProcessError::LocalInspectionProtocol => {
                        InspectionEndpointErrorV2::ResponseUnavailable
                    }
                    LocalProcessError::LocalInspectionBootstrap
                    | LocalProcessError::LocalInspectionPeer
                    | LocalProcessError::LocalInspectionIo => {
                        InspectionEndpointErrorV2::Unavailable
                    }
                    _ => InspectionEndpointErrorV2::ResponseUnavailable,
                })
            }
        }
    }
}

/// Performs exactly one authenticated PXIQ-v2 `Latest` exchange against the
/// locator selected by the lifecycle owner.
pub(crate) fn read_latest_snapshot(
    locator: &LocalInspectionBootstrapLocatorV1,
) -> Result<LocalInspectionSnapshotV2, LocalProcessError> {
    let (bootstrap, socket_identity) = load_bootstrap(locator)?;
    let request_id = bootstrap
        .request_id(1)
        .map_err(|_| LocalProcessError::LocalInspectionProtocol)?;
    let projection_id = bootstrap.projection_id();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LocalInspectionIo)?;
    let mut client = InspectionClientV2::new(DeveloperLocalInspectionEndpointV2 {
        runtime,
        bootstrap,
        socket_identity,
        last_failure: None,
    });
    let response = client.latest(request_id, projection_id);
    let endpoint = client.into_endpoint();
    let response = response.map_err(|error| {
        endpoint
            .last_failure
            .unwrap_or_else(|| map_client_error(error))
    })?;
    match response.outcome() {
        InspectionResponseOutcomeV2::Snapshot => response
            .snapshot_value()
            .cloned()
            .ok_or(LocalProcessError::LocalInspectionProtocol),
        InspectionResponseOutcomeV2::NotFound => Err(LocalProcessError::LocalInspectionNotFound),
        InspectionResponseOutcomeV2::NotModified => Err(LocalProcessError::LocalInspectionProtocol),
    }
}

fn map_client_error(error: InspectionClientErrorV2) -> LocalProcessError {
    match error {
        InspectionClientErrorV2::Endpoint(InspectionEndpointErrorV2::Unavailable) => {
            LocalProcessError::LocalInspectionIo
        }
        InspectionClientErrorV2::InvalidRequest(_)
        | InspectionClientErrorV2::Endpoint(InspectionEndpointErrorV2::MalformedRequest)
        | InspectionClientErrorV2::Endpoint(InspectionEndpointErrorV2::ResponseUnavailable)
        | InspectionClientErrorV2::InvalidResponse(_)
        | InspectionClientErrorV2::CorrelationMismatch => {
            LocalProcessError::LocalInspectionProtocol
        }
    }
}

fn load_bootstrap(
    locator: &LocalInspectionBootstrapLocatorV1,
) -> Result<(DeveloperLocalInspectionBootstrapV2, FileIdentityV1), LocalProcessError> {
    let path = locator.path();
    if !is_lexically_absolute_file(path) {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    let expected_uid = Uid::effective().as_raw();
    let expected_gid = Gid::effective().as_raw();
    validate_private_parent(path, expected_uid, expected_gid)?;

    let before =
        fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_bootstrap_metadata(&before, expected_uid, expected_gid)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    let opened = file
        .metadata()
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    let after =
        fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_bootstrap_metadata(&opened, expected_uid, expected_gid)?;
    validate_bootstrap_metadata(&after, expected_uid, expected_gid)?;
    let opened_identity = FileIdentityV1::from_metadata(&opened);
    if FileIdentityV1::from_metadata(&before) != opened_identity
        || FileIdentityV1::from_metadata(&after) != opened_identity
        || opened_identity.device != locator.device()
        || opened_identity.inode != locator.inode()
        || opened.len() != u64::from(locator.content_length())
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }

    let wire = read_bounded_bootstrap(&file)?;
    let content_sha256: [u8; 32] = Sha256::digest(&wire).into();
    if content_sha256 != locator.content_sha256() {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    let bootstrap = DeveloperLocalInspectionBootstrapV2::decode_owned(wire)
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    if bootstrap.server_uid() != expected_uid || bootstrap.server_gid() != expected_gid {
        return Err(LocalProcessError::LocalInspectionPeer);
    }
    if bootstrap.socket_path().parent() != path.parent()
        || !is_lexically_absolute_file(bootstrap.socket_path())
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    validate_private_parent(bootstrap.socket_path(), expected_uid, expected_gid)?;
    let socket = fs::symlink_metadata(bootstrap.socket_path())
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_socket_metadata(&socket, expected_uid, expected_gid)?;
    let socket_identity = FileIdentityV1::from_metadata(&socket);

    let final_metadata =
        fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_bootstrap_metadata(&final_metadata, expected_uid, expected_gid)?;
    if FileIdentityV1::from_metadata(&final_metadata) != opened_identity
        || final_metadata.len() != u64::from(locator.content_length())
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok((bootstrap, socket_identity))
}

fn read_bounded_bootstrap(file: &File) -> Result<Vec<u8>, LocalProcessError> {
    let metadata = file
        .metadata()
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    let length =
        usize::try_from(metadata.len()).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    if !(DEVELOPER_LOCAL_INSPECTION_BOOTSTRAP_V2_HEADER_BYTES
        ..=MAX_DEVELOPER_LOCAL_INSPECTION_BOOTSTRAP_V2_BYTES)
        .contains(&length)
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    let mut wire = Vec::with_capacity(length);
    let read_limit = u64::try_from(MAX_DEVELOPER_LOCAL_INSPECTION_BOOTSTRAP_V2_BYTES + 1)
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    file.take(read_limit)
        .read_to_end(&mut wire)
        .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    if wire.len() != length {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok(wire)
}

fn validate_private_parent(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), LocalProcessError> {
    let parent = path
        .parent()
        .ok_or(LocalProcessError::LocalInspectionBootstrap)?;
    validate_canonical_path_chain(parent)?;
    let before =
        fs::symlink_metadata(parent).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_socket_directory_metadata(&before, expected_uid, expected_gid)?;
    let canonical =
        fs::canonicalize(parent).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    let after =
        fs::symlink_metadata(parent).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_socket_directory_metadata(&after, expected_uid, expected_gid)?;
    if canonical != parent
        || FileIdentityV1::from_metadata(&before) != FileIdentityV1::from_metadata(&after)
        || before.nlink() != after.nlink()
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok(())
}

fn validate_socket_directory_metadata(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), LocalProcessError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.permissions().mode() & 0o7777 != SOCKET_DIRECTORY_MODE
        || metadata.nlink() == 0
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok(())
}

fn validate_canonical_path_chain(path: &Path) -> Result<(), LocalProcessError> {
    if !path.is_absolute() {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
                if metadata.file_type().is_symlink() {
                    return Err(LocalProcessError::LocalInspectionBootstrap);
                }
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(LocalProcessError::LocalInspectionBootstrap);
            }
        }
    }
    Ok(())
}

fn validate_bootstrap_metadata(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), LocalProcessError> {
    if !metadata.file_type().is_file()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.permissions().mode() & 0o7777 != PRIVATE_FILE_MODE
        || metadata.nlink() != 1
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok(())
}

fn validate_socket_metadata(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), LocalProcessError> {
    if !metadata.file_type().is_socket()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.permissions().mode() & 0o7777 != PRIVATE_FILE_MODE
        || metadata.nlink() != 1
    {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok(())
}

async fn exchange_once(
    bootstrap: &DeveloperLocalInspectionBootstrapV2,
    expected_socket_identity: FileIdentityV1,
    canonical_request: &[u8],
) -> Result<Box<[u8]>, LocalProcessError> {
    let request = InspectionRequestV2::decode(canonical_request)
        .map_err(|_| LocalProcessError::LocalInspectionProtocol)?;
    if request.projection_id() != bootstrap.projection_id() {
        return Err(LocalProcessError::LocalInspectionProtocol);
    }
    let wire = encode_authenticated_request_v2(bootstrap.generation_token(), &request)
        .map_err(|_| LocalProcessError::LocalInspectionProtocol)?;
    let deadline = Instant::now() + bootstrap.operation_timeout();
    let mut stream = timeout_at(deadline, TokioUnixStream::connect(bootstrap.socket_path()))
        .await
        .map_err(|_| LocalProcessError::LocalInspectionIo)?
        .map_err(|_| LocalProcessError::LocalInspectionIo)?;
    let credentials = stream
        .peer_cred()
        .map_err(|_| LocalProcessError::LocalInspectionPeer)?;
    if credentials.uid() != bootstrap.server_uid() || credentials.gid() != bootstrap.server_gid() {
        return Err(LocalProcessError::LocalInspectionPeer);
    }
    validate_socket_path_identity(bootstrap.socket_path(), expected_socket_identity)?;
    timeout_at(deadline, stream.write_all(&wire))
        .await
        .map_err(|_| LocalProcessError::LocalInspectionIo)?
        .map_err(|_| LocalProcessError::LocalInspectionIo)?;
    stream
        .shutdown()
        .await
        .map_err(|_| LocalProcessError::LocalInspectionIo)?;

    let mut response_length = [0_u8; 4];
    timeout_at(deadline, stream.read_exact(&mut response_length))
        .await
        .map_err(|_| LocalProcessError::LocalInspectionIo)?
        .map_err(|_| LocalProcessError::LocalInspectionIo)?;
    let response_length = usize::try_from(u32::from_be_bytes(response_length))
        .map_err(|_| LocalProcessError::LocalInspectionProtocol)?;
    if !(1..=MAX_INSPECTION_RESPONSE_V2_BYTES).contains(&response_length) {
        return Err(LocalProcessError::LocalInspectionProtocol);
    }
    let mut response = vec![0_u8; response_length];
    timeout_at(deadline, stream.read_exact(&mut response))
        .await
        .map_err(|_| LocalProcessError::LocalInspectionIo)?
        .map_err(|_| LocalProcessError::LocalInspectionIo)?;
    let mut trailing = [0_u8; 1];
    if timeout_at(deadline, stream.read(&mut trailing))
        .await
        .map_err(|_| LocalProcessError::LocalInspectionIo)?
        .map_err(|_| LocalProcessError::LocalInspectionIo)?
        != 0
    {
        return Err(LocalProcessError::LocalInspectionProtocol);
    }
    validate_socket_path_identity(bootstrap.socket_path(), expected_socket_identity)?;
    Ok(response.into_boxed_slice())
}

fn validate_socket_path_identity(
    path: &Path,
    expected: FileIdentityV1,
) -> Result<(), LocalProcessError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalInspectionBootstrap)?;
    validate_socket_metadata(
        &metadata,
        Uid::effective().as_raw(),
        Gid::effective().as_raw(),
    )?;
    if FileIdentityV1::from_metadata(&metadata) != expected {
        return Err(LocalProcessError::LocalInspectionBootstrap);
    }
    Ok(())
}

fn is_lexically_absolute_file(path: &Path) -> bool {
    if !path.is_absolute() || path.file_name().is_none() || path.as_os_str().as_bytes().contains(&0)
    {
        return false;
    }
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::RootDir)) {
        return false;
    }
    let mut canonical = PathBuf::from("/");
    for component in components {
        let Component::Normal(value) = component else {
            return false;
        };
        canonical.push(value);
    }
    canonical.as_os_str().as_bytes() == path.as_os_str().as_bytes()
}

/// Projects one already strict PXIS-v2 snapshot without changing any
/// owner-supplied freshness, availability, dimension, reason, or coordinate.
#[derive(Serialize)]
pub(crate) struct LocalInspectionSnapshotJsonV1 {
    snapshot_version: u16,
    projection_id: String,
    observation_clock_ref: String,
    projection_revision: String,
    projected_at_nanos: String,
    overall: &'static str,
    projection_digest: String,
    sources: Vec<InspectionSourceJsonV1>,
    node: InspectionNodeJsonV1,
}

#[derive(Serialize)]
struct InspectionSourceJsonV1 {
    owner: &'static str,
    freshness: &'static str,
    subject_ref: String,
    coordinate: Option<InspectionCoordinateJsonV1>,
    observed_at_nanos: Option<String>,
    valid_until_nanos: Option<String>,
    liveness: &'static str,
    readiness: &'static str,
    health: &'static str,
    feature_support: &'static str,
    reason: &'static str,
    owner_fact_digest: Option<String>,
}

#[derive(Serialize)]
struct InspectionNodeJsonV1 {
    freshness: &'static str,
    node_ref: String,
    node_incarnation_ref: String,
    registration_epoch: Option<String>,
    status_sequence: Option<String>,
    observed_at_nanos: Option<String>,
    valid_until_nanos: Option<String>,
    liveness: &'static str,
    readiness: &'static str,
    health: &'static str,
    feature_support: &'static str,
    reason: &'static str,
    node_status_digest: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum InspectionCoordinateJsonV1 {
    AuthorityTenure {
        kind: &'static str,
        tenure_epoch: String,
        fact_sequence: String,
    },
    DeploymentRevision {
        kind: &'static str,
        revision: String,
        fact_sequence: String,
    },
    RuntimeHostEpoch {
        kind: &'static str,
        runtime_host_epoch: String,
        snapshot_sequence: String,
    },
    FabricServiceGeneration {
        kind: &'static str,
        service_generation: String,
        observation_sequence: String,
    },
    AgentServiceGeneration {
        kind: &'static str,
        service_generation: String,
        observation_sequence: String,
    },
}

pub(crate) fn snapshot_json(snapshot: &LocalInspectionSnapshotV2) -> LocalInspectionSnapshotJsonV1 {
    let sources = snapshot
        .base_snapshot()
        .records()
        .iter()
        .copied()
        .map(source_json)
        .collect::<Vec<_>>();
    LocalInspectionSnapshotJsonV1 {
        snapshot_version: LOCAL_INSPECTION_SNAPSHOT_V2_VERSION,
        projection_id: lower_hex(&snapshot.projection_id()),
        observation_clock_ref: lower_hex(snapshot.observation_clock_ref().as_bytes()),
        projection_revision: decimal_string(snapshot.projection_revision()),
        projected_at_nanos: decimal_string(snapshot.projected_at_nanos()),
        overall: overall_name(snapshot.overall()),
        projection_digest: lower_hex(snapshot.projection_digest().as_bytes()),
        sources,
        node: node_json(snapshot.node()),
    }
}

fn source_json(record: LocalInspectionRecordV1) -> InspectionSourceJsonV1 {
    InspectionSourceJsonV1 {
        owner: owner_name(record.owner()),
        freshness: freshness_name(record.freshness()),
        subject_ref: lower_hex(&record.subject_ref()),
        coordinate: record.coordinate().map(coordinate_json),
        observed_at_nanos: optional_decimal_string(record.observed_at_nanos()),
        valid_until_nanos: optional_decimal_string(record.valid_until_nanos()),
        liveness: liveness_name(record.liveness()),
        readiness: readiness_name(record.readiness()),
        health: health_name(record.health()),
        feature_support: feature_support_name(record.feature_support()),
        reason: reason_name(record.reason()),
        owner_fact_digest: record
            .owner_fact_digest()
            .map(|digest| lower_hex(digest.as_bytes())),
    }
}

fn node_json(node: NodeInspectionRecordV2) -> InspectionNodeJsonV1 {
    InspectionNodeJsonV1 {
        freshness: freshness_name(node.freshness()),
        node_ref: lower_hex(&node.node_ref()),
        node_incarnation_ref: lower_hex(&node.node_incarnation_ref()),
        registration_epoch: optional_decimal_string(node.registration_epoch()),
        status_sequence: optional_decimal_string(node.status_sequence()),
        observed_at_nanos: optional_decimal_string(node.observed_at_nanos()),
        valid_until_nanos: optional_decimal_string(node.valid_until_nanos()),
        liveness: liveness_name(node.liveness()),
        readiness: readiness_name(node.readiness()),
        health: health_name(node.health()),
        feature_support: feature_support_name(node.feature_support()),
        reason: reason_name(node.reason()),
        node_status_digest: node
            .node_status_digest()
            .map(|digest| lower_hex(digest.as_bytes())),
    }
}

fn coordinate_json(coordinate: InspectionSourceCoordinateV1) -> InspectionCoordinateJsonV1 {
    match coordinate {
        InspectionSourceCoordinateV1::AuthorityTenure {
            tenure_epoch,
            fact_sequence,
        } => InspectionCoordinateJsonV1::AuthorityTenure {
            kind: "authority_tenure",
            tenure_epoch: decimal_string(tenure_epoch),
            fact_sequence: decimal_string(fact_sequence),
        },
        InspectionSourceCoordinateV1::DeploymentRevision {
            revision,
            fact_sequence,
        } => InspectionCoordinateJsonV1::DeploymentRevision {
            kind: "deployment_revision",
            revision: decimal_string(revision),
            fact_sequence: decimal_string(fact_sequence),
        },
        InspectionSourceCoordinateV1::RuntimeHostEpoch {
            runtime_host_epoch,
            snapshot_sequence,
        } => InspectionCoordinateJsonV1::RuntimeHostEpoch {
            kind: "runtime_host_epoch",
            runtime_host_epoch: decimal_string(runtime_host_epoch),
            snapshot_sequence: decimal_string(snapshot_sequence),
        },
        InspectionSourceCoordinateV1::FabricServiceGeneration {
            service_generation,
            observation_sequence,
        } => InspectionCoordinateJsonV1::FabricServiceGeneration {
            kind: "fabric_service_generation",
            service_generation: decimal_string(service_generation),
            observation_sequence: decimal_string(observation_sequence),
        },
        InspectionSourceCoordinateV1::AgentServiceGeneration {
            service_generation,
            observation_sequence,
        } => InspectionCoordinateJsonV1::AgentServiceGeneration {
            kind: "agent_service_generation",
            service_generation: decimal_string(service_generation),
            observation_sequence: decimal_string(observation_sequence),
        },
    }
}

fn decimal_string(value: u64) -> String {
    value.to_string()
}

fn optional_decimal_string(value: Option<u64>) -> Option<String> {
    value.map(decimal_string)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

const fn owner_name(value: InspectionSourceOwnerV1) -> &'static str {
    match value {
        InspectionSourceOwnerV1::Authority => "authority",
        InspectionSourceOwnerV1::DeploymentController => "deployment_controller",
        InspectionSourceOwnerV1::RuntimeHost => "runtime_host",
        InspectionSourceOwnerV1::FabricService => "fabric_service",
        InspectionSourceOwnerV1::AgentService => "agent_service",
    }
}

const fn freshness_name(value: InspectionFreshnessV1) -> &'static str {
    match value {
        InspectionFreshnessV1::Fresh => "fresh",
        InspectionFreshnessV1::Stale => "stale",
        InspectionFreshnessV1::Partitioned => "partitioned",
        InspectionFreshnessV1::Missing => "missing",
    }
}

const fn liveness_name(value: InspectionLivenessV1) -> &'static str {
    match value {
        InspectionLivenessV1::Unknown => "unknown",
        InspectionLivenessV1::Bootstrapping => "bootstrapping",
        InspectionLivenessV1::Live => "live",
        InspectionLivenessV1::Unresponsive => "unresponsive",
        InspectionLivenessV1::Exited => "exited",
        InspectionLivenessV1::Quarantined => "quarantined",
    }
}

const fn readiness_name(value: InspectionReadinessV1) -> &'static str {
    match value {
        InspectionReadinessV1::Unknown => "unknown",
        InspectionReadinessV1::Ready => "ready",
        InspectionReadinessV1::NotReady => "not_ready",
        InspectionReadinessV1::Degraded => "degraded",
        InspectionReadinessV1::Blocked => "blocked",
    }
}

const fn health_name(value: InspectionHealthV1) -> &'static str {
    match value {
        InspectionHealthV1::Unknown => "unknown",
        InspectionHealthV1::Healthy => "healthy",
        InspectionHealthV1::Degraded => "degraded",
        InspectionHealthV1::Faulted => "faulted",
    }
}

const fn feature_support_name(value: InspectionFeatureSupportV1) -> &'static str {
    match value {
        InspectionFeatureSupportV1::Unknown => "unknown",
        InspectionFeatureSupportV1::AllRequiredSupported => "all_required_supported",
        InspectionFeatureSupportV1::RequiredUnsupported => "required_unsupported",
    }
}

const fn reason_name(value: InspectionReasonV1) -> &'static str {
    match value {
        InspectionReasonV1::None => "none",
        InspectionReasonV1::Bootstrapping => "bootstrapping",
        InspectionReasonV1::DependencyUnavailable => "dependency_unavailable",
        InspectionReasonV1::OwnerReportedDegraded => "owner_reported_degraded",
        InspectionReasonV1::OwnerReportedFailure => "owner_reported_failure",
        InspectionReasonV1::FeatureUnsupported => "feature_unsupported",
        InspectionReasonV1::Quarantined => "quarantined",
        InspectionReasonV1::OutcomeUncertain => "outcome_uncertain",
        InspectionReasonV1::SourceUnknown => "source_unknown",
        InspectionReasonV1::SourceMissing => "source_missing",
        InspectionReasonV1::SourceStale => "source_stale",
        InspectionReasonV1::SourcePartitioned => "source_partitioned",
    }
}

const fn overall_name(value: LocalInspectionOverallV1) -> &'static str {
    match value {
        LocalInspectionOverallV1::Ready => "ready",
        LocalInspectionOverallV1::Degraded => "degraded",
        LocalInspectionOverallV1::Unavailable => "unavailable",
        LocalInspectionOverallV1::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::os::unix::fs::DirBuilderExt as _;
    use std::os::unix::net::UnixListener as StdUnixListener;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use paraegox_inspection::developer_local::{
        DEVELOPER_LOCAL_INSPECTION_REQUEST_V2_BYTES, decode_authenticated_request_v2,
    };
    use paraegox_inspection::protocol::{InspectionRequestKindV2, InspectionRequestV2};
    use paraegox_inspection::{
        InspectionObservationClockRefV1, InspectionSourceAvailabilityV1, InspectionSourceSlotV1,
        LocalInspectionProjectionInputV1, LocalInspectionProjectionInputV2,
        LocalInspectionServiceV2, NodeInspectionFactFieldsV2, NodeInspectionFactV2,
        NodeInspectionSourceSlotV2, OwnerInspectionFactFieldsV1, OwnerInspectionFactV1,
    };
    use paraegox_kernel::digest::Digest32;
    use zeroize::Zeroizing;

    static CLIENT_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct ClientFixture {
        directory: PathBuf,
        bootstrap_path: PathBuf,
        socket_path: PathBuf,
        locator: LocalInspectionBootstrapLocatorV1,
    }

    impl ClientFixture {
        fn new(
            operation_timeout: Duration,
            declared_uid: u32,
            declared_gid: u32,
        ) -> (Self, StdUnixListener, [u8; 32], [u8; 16]) {
            let sequence = CLIENT_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let parent = fs::canonicalize(std::env::temp_dir())
                .expect("canonical Inspection client test root");
            let directory = parent.join(format!(
                "paraegox-m3a-client-{}-{sequence}",
                std::process::id()
            ));
            fs::DirBuilder::new()
                .mode(SOCKET_DIRECTORY_MODE)
                .create(&directory)
                .expect("Inspection socket directory");
            fs::set_permissions(
                &directory,
                fs::Permissions::from_mode(SOCKET_DIRECTORY_MODE),
            )
            .expect("exact Inspection socket directory mode");
            let socket_path = directory.join("i.sock");
            let listener = StdUnixListener::bind(&socket_path).expect("Inspection test listener");
            fs::set_permissions(&socket_path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
                .expect("private Inspection socket mode");

            let projection_id = [0x31; 16];
            let generation_token = [0x42; 32];
            let bootstrap = DeveloperLocalInspectionBootstrapV2::try_new(
                socket_path.clone(),
                projection_id,
                Zeroizing::new(generation_token),
                declared_uid,
                declared_gid,
                operation_timeout,
                Zeroizing::new([0x53; 16]),
            )
            .expect("valid PXIB-v2 fixture");
            let bootstrap_path = directory.join("i.pxib");
            let wire = bootstrap.encode().expect("encode PXIB-v2 fixture");
            fs::write(&bootstrap_path, wire.as_slice()).expect("write PXIB-v2 fixture");
            fs::set_permissions(
                &bootstrap_path,
                fs::Permissions::from_mode(PRIVATE_FILE_MODE),
            )
            .expect("private PXIB-v2 mode");
            let locator = locator_for_file(&bootstrap_path);
            (
                Self {
                    directory,
                    bootstrap_path,
                    socket_path,
                    locator,
                },
                listener,
                generation_token,
                projection_id,
            )
        }
    }

    impl Drop for ClientFixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.bootstrap_path);
            let _ = fs::remove_file(&self.socket_path);
            let _ = fs::remove_dir(&self.directory);
        }
    }

    fn locator_for_file(path: &Path) -> LocalInspectionBootstrapLocatorV1 {
        let wire = fs::read(path).expect("read PXIB fixture");
        let metadata = fs::symlink_metadata(path).expect("PXIB fixture metadata");
        LocalInspectionBootstrapLocatorV1::for_test(
            path.to_path_buf(),
            u32::try_from(wire.len()).expect("bounded PXIB fixture"),
            Sha256::digest(&wire).into(),
            metadata.dev(),
            metadata.ino(),
        )
    }

    #[derive(Clone, Copy)]
    enum ServerBehavior {
        NotFound,
        CorrelationMismatch,
        Trailing,
        Malformed,
        Stall,
    }

    fn serve_one(
        listener: StdUnixListener,
        behavior: ServerBehavior,
        generation_token: [u8; 32],
        projection_id: [u8; 16],
    ) -> JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one Inspection connection");
            let mut authenticated = vec![0_u8; DEVELOPER_LOCAL_INSPECTION_REQUEST_V2_BYTES];
            stream
                .read_exact(&mut authenticated)
                .expect("one complete authenticated PXIQ-v2 request");
            let mut trailing = [0_u8; 1];
            assert_eq!(stream.read(&mut trailing).expect("request EOF"), 0);
            assert_eq!(&authenticated[..32], &generation_token);
            assert_eq!(&authenticated[32..36], b"PXIQ");
            assert_eq!(
                u16::from_be_bytes([authenticated[36], authenticated[37]]),
                2
            );
            assert_eq!(authenticated[44], 1);
            let request = decode_authenticated_request_v2(&authenticated, &generation_token)
                .expect("authenticated PXIQ-v2 request");
            assert_eq!(request.kind(), InspectionRequestKindV2::Latest);
            assert_eq!(request.after_revision(), 0);
            assert_eq!(request.projection_id(), projection_id);

            if matches!(behavior, ServerBehavior::Stall) {
                thread::sleep(Duration::from_millis(200));
                return authenticated;
            }
            if matches!(behavior, ServerBehavior::Malformed) {
                stream
                    .write_all(&1_u32.to_be_bytes())
                    .and_then(|()| stream.write_all(&[0]))
                    .expect("malformed Inspection response");
                return authenticated;
            }

            let response_request = if matches!(behavior, ServerBehavior::CorrelationMismatch) {
                InspectionRequestV2::try_latest([0x64; 16], projection_id)
                    .expect("mismatched valid PXIQ-v2 request")
            } else {
                request
            };
            let service = LocalInspectionServiceV2::try_new(
                projection_id,
                InspectionObservationClockRefV1::try_from_bytes([0x75; 16])
                    .expect("Inspection clock"),
            )
            .expect("empty Inspection service");
            let response = service
                .answer_read_only_v2(&response_request)
                .expect("NotFound PXIP-v2 response");
            let response = response.canonical_wire();
            stream
                .write_all(
                    &u32::try_from(response.len())
                        .expect("bounded PXIP-v2 response")
                        .to_be_bytes(),
                )
                .and_then(|()| stream.write_all(response))
                .expect("PXIP-v2 response");
            if matches!(behavior, ServerBehavior::Trailing) {
                stream.write_all(&[0]).expect("trailing response byte");
            }
            authenticated
        })
    }

    fn observed_source_slot(
        owner: InspectionSourceOwnerV1,
        subject_byte: u8,
        coordinate: InspectionSourceCoordinateV1,
        observation_clock_ref: InspectionObservationClockRefV1,
        observed_at_nanos: u64,
        valid_until_nanos: u64,
    ) -> InspectionSourceSlotV1 {
        let subject_ref = [subject_byte; 16];
        let fact = OwnerInspectionFactV1::try_new(OwnerInspectionFactFieldsV1 {
            owner,
            subject_ref,
            coordinate,
            observation_clock_ref,
            observed_at_nanos,
            valid_until_nanos,
            availability: InspectionSourceAvailabilityV1::Observed,
            liveness: InspectionLivenessV1::Live,
            readiness: InspectionReadinessV1::Ready,
            health: InspectionHealthV1::Healthy,
            feature_support: InspectionFeatureSupportV1::AllRequiredSupported,
            reason: InspectionReasonV1::None,
            owner_fact_digest: Digest32::from_bytes([subject_byte.wrapping_add(0x40); 32]),
        })
        .expect("valid owner Inspection fact");
        InspectionSourceSlotV1::try_new(owner, subject_ref, Some(fact))
            .expect("bound owner Inspection slot")
    }

    #[test]
    fn inspection_client_performs_one_authenticated_latest_and_maps_not_found() {
        let uid = Uid::effective().as_raw();
        let gid = Gid::effective().as_raw();
        let (fixture, listener, token, projection_id) =
            ClientFixture::new(Duration::from_secs(1), uid, gid);
        let server = serve_one(listener, ServerBehavior::NotFound, token, projection_id);
        assert!(matches!(
            read_latest_snapshot(&fixture.locator),
            Err(LocalProcessError::LocalInspectionNotFound)
        ));
        let authenticated = server.join().expect("Inspection server thread");
        assert_eq!(authenticated.len(), 32 + 96);

        let source = include_str!("inspection_client.rs");
        let latest = source
            .split("pub(crate) fn read_latest_snapshot(")
            .nth(1)
            .and_then(|tail| tail.split("fn map_client_error(").next())
            .expect("bounded Inspection client source");
        assert_eq!(latest.matches("client.latest(").count(), 1);
        assert!(!latest.contains("client.watch("));
        assert!(!latest.contains("loop {"));
        assert!(!latest.contains("retry"));
    }

    #[test]
    fn inspection_client_fails_closed_on_correlation_trailing_and_malformed_responses() {
        let uid = Uid::effective().as_raw();
        let gid = Gid::effective().as_raw();
        for behavior in [
            ServerBehavior::CorrelationMismatch,
            ServerBehavior::Trailing,
            ServerBehavior::Malformed,
        ] {
            let (fixture, listener, token, projection_id) =
                ClientFixture::new(Duration::from_secs(1), uid, gid);
            let server = serve_one(listener, behavior, token, projection_id);
            assert!(matches!(
                read_latest_snapshot(&fixture.locator),
                Err(LocalProcessError::LocalInspectionProtocol)
            ));
            assert_eq!(
                server.join().expect("Inspection server thread").len(),
                32 + 96
            );
        }
    }

    #[test]
    fn inspection_client_classifies_timeout_io_and_declared_peer_mismatch() {
        let uid = Uid::effective().as_raw();
        let gid = Gid::effective().as_raw();
        let (timeout_fixture, listener, token, projection_id) =
            ClientFixture::new(Duration::from_millis(25), uid, gid);
        let server = serve_one(listener, ServerBehavior::Stall, token, projection_id);
        assert!(matches!(
            read_latest_snapshot(&timeout_fixture.locator),
            Err(LocalProcessError::LocalInspectionIo)
        ));
        assert_eq!(
            server.join().expect("stalled Inspection server").len(),
            32 + 96
        );

        let (io_fixture, listener, _, _) = ClientFixture::new(Duration::from_millis(100), uid, gid);
        drop(listener);
        assert!(matches!(
            read_latest_snapshot(&io_fixture.locator),
            Err(LocalProcessError::LocalInspectionIo)
        ));

        let different_uid = uid.checked_add(1).filter(|value| *value != 0).unwrap_or(1);
        let (peer_fixture, listener, _, _) =
            ClientFixture::new(Duration::from_millis(100), different_uid, gid);
        assert!(matches!(
            read_latest_snapshot(&peer_fixture.locator),
            Err(LocalProcessError::LocalInspectionPeer)
        ));
        drop(listener);
    }

    #[test]
    fn inspection_client_revalidates_socket_directory_pxib_and_socket_pins() {
        let uid = Uid::effective().as_raw();
        let gid = Gid::effective().as_raw();
        let (fixture, listener, _, _) = ClientFixture::new(Duration::from_millis(100), uid, gid);
        assert!(load_bootstrap(&fixture.locator).is_ok());

        fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(0o700))
            .expect("insecure socket directory mode");
        assert!(matches!(
            load_bootstrap(&fixture.locator),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));
        fs::set_permissions(
            &fixture.directory,
            fs::Permissions::from_mode(SOCKET_DIRECTORY_MODE),
        )
        .expect("restore socket directory mode");

        let canonical = fs::read(&fixture.bootstrap_path).expect("canonical PXIB fixture");
        let mut unsupported_version = canonical.clone();
        unsupported_version[4] ^= 1;
        fs::write(&fixture.bootstrap_path, &unsupported_version)
            .expect("unsupported PXIB version fixture");
        fs::set_permissions(
            &fixture.bootstrap_path,
            fs::Permissions::from_mode(PRIVATE_FILE_MODE),
        )
        .expect("private PXIB fixture mode");
        assert!(matches!(
            load_bootstrap(&locator_for_file(&fixture.bootstrap_path)),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));

        let mut invalid_digest = canonical.clone();
        invalid_digest[96] ^= 1;
        fs::write(&fixture.bootstrap_path, &invalid_digest).expect("invalid PXIB digest fixture");
        assert!(matches!(
            load_bootstrap(&locator_for_file(&fixture.bootstrap_path)),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));

        fs::write(&fixture.bootstrap_path, &canonical).expect("restore canonical PXIB fixture");
        fs::set_permissions(
            &fixture.bootstrap_path,
            fs::Permissions::from_mode(PRIVATE_FILE_MODE),
        )
        .expect("restore private PXIB mode");
        let old_locator = locator_for_file(&fixture.bootstrap_path);
        let last = canonical.len() - 1;
        let mut replacement = canonical.clone();
        replacement[last] ^= 1;
        fs::write(&fixture.bootstrap_path, replacement).expect("replace pinned PXIB content");
        assert!(matches!(
            load_bootstrap(&old_locator),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));

        fs::write(&fixture.bootstrap_path, canonical).expect("restore PXIB before socket test");
        let restored_locator = locator_for_file(&fixture.bootstrap_path);
        let bootstrap_hardlink = fixture.directory.join("i-hardlink.pxib");
        fs::hard_link(&fixture.bootstrap_path, &bootstrap_hardlink).expect("PXIB hardlink fixture");
        assert!(matches!(
            load_bootstrap(&restored_locator),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));
        fs::remove_file(&bootstrap_hardlink).expect("remove PXIB hardlink fixture");

        let socket_hardlink = fixture.directory.join("i-hardlink.sock");
        fs::hard_link(&fixture.socket_path, &socket_hardlink)
            .expect("Inspection socket hardlink fixture");
        assert!(matches!(
            load_bootstrap(&restored_locator),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));
        fs::remove_file(&socket_hardlink).expect("remove socket hardlink fixture");

        fs::set_permissions(&fixture.socket_path, fs::Permissions::from_mode(0o660))
            .expect("insecure Inspection socket mode");
        assert!(matches!(
            load_bootstrap(&restored_locator),
            Err(LocalProcessError::LocalInspectionBootstrap)
        ));
        drop(listener);
    }

    #[test]
    fn snapshot_serializer_preserves_all_u64_boundaries_as_decimal_strings() {
        let below = 9_007_199_254_740_991_u64;
        let boundary = 9_007_199_254_740_992_u64;
        let maximum = u64::MAX;
        assert_eq!(decimal_string(below), "9007199254740991");
        assert_eq!(decimal_string(boundary), "9007199254740992");
        assert_eq!(decimal_string(maximum), "18446744073709551615");
        assert_eq!(optional_decimal_string(None), None);

        let authority = serde_json::to_value(coordinate_json(
            InspectionSourceCoordinateV1::AuthorityTenure {
                tenure_epoch: below,
                fact_sequence: boundary,
            },
        ))
        .expect("Authority coordinate JSON");
        assert_eq!(authority["kind"], "authority_tenure");
        assert_eq!(authority["tenure_epoch"], "9007199254740991");
        assert_eq!(authority["fact_sequence"], "9007199254740992");
        assert!(authority["tenure_epoch"].is_string());

        let runtime = serde_json::to_value(coordinate_json(
            InspectionSourceCoordinateV1::RuntimeHostEpoch {
                runtime_host_epoch: maximum,
                snapshot_sequence: boundary,
            },
        ))
        .expect("Runtime coordinate JSON");
        assert_eq!(runtime["runtime_host_epoch"], "18446744073709551615");
        assert_eq!(runtime["snapshot_sequence"], "9007199254740992");
        assert!(runtime["runtime_host_epoch"].is_string());

        for (coordinate, kind, first_field, second_field) in [
            (
                InspectionSourceCoordinateV1::DeploymentRevision {
                    revision: below,
                    fact_sequence: maximum,
                },
                "deployment_revision",
                "revision",
                "fact_sequence",
            ),
            (
                InspectionSourceCoordinateV1::FabricServiceGeneration {
                    service_generation: boundary,
                    observation_sequence: maximum,
                },
                "fabric_service_generation",
                "service_generation",
                "observation_sequence",
            ),
            (
                InspectionSourceCoordinateV1::AgentServiceGeneration {
                    service_generation: maximum,
                    observation_sequence: below,
                },
                "agent_service_generation",
                "service_generation",
                "observation_sequence",
            ),
        ] {
            let value =
                serde_json::to_value(coordinate_json(coordinate)).expect("typed coordinate JSON");
            assert_eq!(value["kind"], kind);
            assert!(value[first_field].is_string());
            assert!(value[second_field].is_string());
            assert_eq!(value.as_object().map(|object| object.len()), Some(3));
        }

        assert_eq!(
            serde_json::to_string(&coordinate_json(
                InspectionSourceCoordinateV1::AuthorityTenure {
                    tenure_epoch: below,
                    fact_sequence: boundary,
                },
            ))
            .expect("ordered coordinate JSON"),
            concat!(
                "{\"kind\":\"authority_tenure\",",
                "\"tenure_epoch\":\"9007199254740991\",",
                "\"fact_sequence\":\"9007199254740992\"}"
            )
        );
    }

    #[test]
    fn snapshot_serializer_has_exact_full_shape_order_nulls_hex_and_owner_order() {
        let below = 9_007_199_254_740_991_u64;
        let boundary = 9_007_199_254_740_992_u64;
        let maximum = u64::MAX;
        let clock =
            InspectionObservationClockRefV1::try_from_bytes([0x21; 16]).expect("Inspection clock");
        let base = LocalInspectionProjectionInputV1::try_new(
            clock,
            [
                observed_source_slot(
                    InspectionSourceOwnerV1::Authority,
                    1,
                    InspectionSourceCoordinateV1::AuthorityTenure {
                        tenure_epoch: below,
                        fact_sequence: boundary,
                    },
                    clock,
                    below,
                    maximum,
                ),
                observed_source_slot(
                    InspectionSourceOwnerV1::DeploymentController,
                    2,
                    InspectionSourceCoordinateV1::DeploymentRevision {
                        revision: boundary,
                        fact_sequence: maximum,
                    },
                    clock,
                    below,
                    maximum,
                ),
                observed_source_slot(
                    InspectionSourceOwnerV1::RuntimeHost,
                    3,
                    InspectionSourceCoordinateV1::RuntimeHostEpoch {
                        runtime_host_epoch: maximum,
                        snapshot_sequence: boundary,
                    },
                    clock,
                    below,
                    maximum,
                ),
                observed_source_slot(
                    InspectionSourceOwnerV1::FabricService,
                    4,
                    InspectionSourceCoordinateV1::FabricServiceGeneration {
                        service_generation: boundary,
                        observation_sequence: maximum,
                    },
                    clock,
                    below,
                    maximum,
                ),
                observed_source_slot(
                    InspectionSourceOwnerV1::AgentService,
                    5,
                    InspectionSourceCoordinateV1::AgentServiceGeneration {
                        service_generation: maximum,
                        observation_sequence: below,
                    },
                    clock,
                    below,
                    maximum,
                ),
            ],
        )
        .expect("five-owner projection input");
        let node_ref = [0x31; 16];
        let node_incarnation_ref = [0x32; 16];
        let node_fact = NodeInspectionFactV2::try_new(NodeInspectionFactFieldsV2 {
            node_ref,
            node_incarnation_ref,
            registration_epoch: boundary,
            status_sequence: maximum,
            observation_clock_ref: clock,
            observed_at_nanos: below,
            valid_until_nanos: maximum,
            availability: InspectionSourceAvailabilityV1::Observed,
            liveness: InspectionLivenessV1::Live,
            readiness: InspectionReadinessV1::Ready,
            health: InspectionHealthV1::Healthy,
            feature_support: InspectionFeatureSupportV1::AllRequiredSupported,
            reason: InspectionReasonV1::None,
            node_status_digest: Digest32::from_bytes([0x33; 32]),
        })
        .expect("valid Node Inspection fact");
        let node =
            NodeInspectionSourceSlotV2::try_new(node_ref, node_incarnation_ref, Some(node_fact))
                .expect("bound Node Inspection slot");
        let input =
            LocalInspectionProjectionInputV2::try_new(base, node).expect("complete PXIS-v2 input");
        let mut service =
            LocalInspectionServiceV2::try_new([0x41; 16], clock).expect("PXIS-v2 service");
        let snapshot = service.project(maximum, &input).expect("PXIS-v2 snapshot");
        let serialized = snapshot_json(snapshot);
        let wire = serde_json::to_string(&serialized).expect("ordered PXIS-v2 JSON");
        let value = serde_json::to_value(&serialized).expect("PXIS-v2 JSON value");

        let snapshot_object = value.as_object().expect("snapshot object");
        assert_eq!(snapshot_object.len(), 9);
        assert_eq!(value["snapshot_version"], 2);
        assert!(value["snapshot_version"].is_number());
        assert_eq!(value["projection_id"], "41".repeat(16));
        assert_eq!(value["observation_clock_ref"], "21".repeat(16));
        assert_eq!(value["projection_revision"], "1");
        assert!(value["projection_revision"].is_string());
        assert_eq!(value["projected_at_nanos"], maximum.to_string());
        assert!(value["projected_at_nanos"].is_string());
        assert_eq!(value["projection_digest"].as_str().map(str::len), Some(64));

        let sources = value["sources"].as_array().expect("five source records");
        assert_eq!(sources.len(), 5);
        assert_eq!(
            sources
                .iter()
                .map(|source| source["owner"].as_str().expect("owner"))
                .collect::<Vec<_>>(),
            [
                "authority",
                "deployment_controller",
                "runtime_host",
                "fabric_service",
                "agent_service",
            ]
        );
        for source in sources {
            assert_eq!(source.as_object().map(|object| object.len()), Some(12));
            assert!(source["coordinate"].is_object());
            assert!(source["observed_at_nanos"].is_string());
            assert!(source["valid_until_nanos"].is_string());
            assert_eq!(source["subject_ref"].as_str().map(str::len), Some(32));
            assert_eq!(source["owner_fact_digest"].as_str().map(str::len), Some(64));
        }
        assert_eq!(sources[0]["coordinate"]["tenure_epoch"], below.to_string());
        assert_eq!(
            sources[1]["coordinate"]["fact_sequence"],
            maximum.to_string()
        );
        assert_eq!(
            sources[2]["coordinate"]["runtime_host_epoch"],
            maximum.to_string()
        );

        let node = value["node"].as_object().expect("Node record");
        assert_eq!(node.len(), 13);
        assert_eq!(value["node"]["registration_epoch"], boundary.to_string());
        assert_eq!(value["node"]["status_sequence"], maximum.to_string());
        assert_eq!(value["node"]["node_ref"], "31".repeat(16));
        assert_eq!(value["node"]["node_incarnation_ref"], "32".repeat(16));
        assert_eq!(value["node"]["node_status_digest"], "33".repeat(32));

        let expected_field_order = [
            "\"snapshot_version\"",
            "\"projection_id\"",
            "\"observation_clock_ref\"",
            "\"projection_revision\"",
            "\"projected_at_nanos\"",
            "\"overall\"",
            "\"projection_digest\"",
            "\"sources\"",
            "\"node\"",
        ];
        let positions = expected_field_order.map(|field| wire.find(field).expect("snapshot field"));
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));

        let missing_base = LocalInspectionProjectionInputV1::try_new(
            clock,
            [
                (InspectionSourceOwnerV1::Authority, 1),
                (InspectionSourceOwnerV1::DeploymentController, 2),
                (InspectionSourceOwnerV1::RuntimeHost, 3),
                (InspectionSourceOwnerV1::FabricService, 4),
                (InspectionSourceOwnerV1::AgentService, 5),
            ]
            .map(|(owner, subject_byte)| {
                InspectionSourceSlotV1::try_new(owner, [subject_byte; 16], None)
                    .expect("missing owner slot")
            }),
        )
        .expect("missing five-owner input");
        let missing_node = NodeInspectionSourceSlotV2::try_new([0x51; 16], [0x52; 16], None)
            .expect("missing Node slot");
        let missing_input = LocalInspectionProjectionInputV2::try_new(missing_base, missing_node)
            .expect("missing PXIS-v2 input");
        let missing = service
            .project(maximum, &missing_input)
            .expect("missing PXIS-v2 snapshot");
        let missing = serde_json::to_value(snapshot_json(missing)).expect("missing snapshot JSON");
        for source in missing["sources"].as_array().expect("missing sources") {
            assert!(source["coordinate"].is_null());
            assert!(source["observed_at_nanos"].is_null());
            assert!(source["valid_until_nanos"].is_null());
            assert!(source["owner_fact_digest"].is_null());
        }
        for field in [
            "registration_epoch",
            "status_sequence",
            "observed_at_nanos",
            "valid_until_nanos",
            "node_status_digest",
        ] {
            assert!(missing["node"][field].is_null());
        }
    }

    #[test]
    fn serializer_names_are_exact_lower_case_snake_case() {
        for (value, expected) in [
            (InspectionSourceOwnerV1::Authority, "authority"),
            (
                InspectionSourceOwnerV1::DeploymentController,
                "deployment_controller",
            ),
            (InspectionSourceOwnerV1::RuntimeHost, "runtime_host"),
            (InspectionSourceOwnerV1::FabricService, "fabric_service"),
            (InspectionSourceOwnerV1::AgentService, "agent_service"),
        ] {
            assert_eq!(owner_name(value), expected);
        }
        for (value, expected) in [
            (InspectionFreshnessV1::Fresh, "fresh"),
            (InspectionFreshnessV1::Stale, "stale"),
            (InspectionFreshnessV1::Partitioned, "partitioned"),
            (InspectionFreshnessV1::Missing, "missing"),
        ] {
            assert_eq!(freshness_name(value), expected);
        }
        for (value, expected) in [
            (InspectionLivenessV1::Unknown, "unknown"),
            (InspectionLivenessV1::Bootstrapping, "bootstrapping"),
            (InspectionLivenessV1::Live, "live"),
            (InspectionLivenessV1::Unresponsive, "unresponsive"),
            (InspectionLivenessV1::Exited, "exited"),
            (InspectionLivenessV1::Quarantined, "quarantined"),
        ] {
            assert_eq!(liveness_name(value), expected);
        }
        for (value, expected) in [
            (InspectionReadinessV1::Unknown, "unknown"),
            (InspectionReadinessV1::Ready, "ready"),
            (InspectionReadinessV1::NotReady, "not_ready"),
            (InspectionReadinessV1::Degraded, "degraded"),
            (InspectionReadinessV1::Blocked, "blocked"),
        ] {
            assert_eq!(readiness_name(value), expected);
        }
        for (value, expected) in [
            (InspectionHealthV1::Unknown, "unknown"),
            (InspectionHealthV1::Healthy, "healthy"),
            (InspectionHealthV1::Degraded, "degraded"),
            (InspectionHealthV1::Faulted, "faulted"),
        ] {
            assert_eq!(health_name(value), expected);
        }
        for (value, expected) in [
            (InspectionFeatureSupportV1::Unknown, "unknown"),
            (
                InspectionFeatureSupportV1::AllRequiredSupported,
                "all_required_supported",
            ),
            (
                InspectionFeatureSupportV1::RequiredUnsupported,
                "required_unsupported",
            ),
        ] {
            assert_eq!(feature_support_name(value), expected);
        }
        for (value, expected) in [
            (InspectionReasonV1::None, "none"),
            (InspectionReasonV1::Bootstrapping, "bootstrapping"),
            (
                InspectionReasonV1::DependencyUnavailable,
                "dependency_unavailable",
            ),
            (
                InspectionReasonV1::OwnerReportedDegraded,
                "owner_reported_degraded",
            ),
            (
                InspectionReasonV1::OwnerReportedFailure,
                "owner_reported_failure",
            ),
            (
                InspectionReasonV1::FeatureUnsupported,
                "feature_unsupported",
            ),
            (InspectionReasonV1::Quarantined, "quarantined"),
            (InspectionReasonV1::OutcomeUncertain, "outcome_uncertain"),
            (InspectionReasonV1::SourceUnknown, "source_unknown"),
            (InspectionReasonV1::SourceMissing, "source_missing"),
            (InspectionReasonV1::SourceStale, "source_stale"),
            (InspectionReasonV1::SourcePartitioned, "source_partitioned"),
        ] {
            assert_eq!(reason_name(value), expected);
        }
        for (value, expected) in [
            (LocalInspectionOverallV1::Ready, "ready"),
            (LocalInspectionOverallV1::Degraded, "degraded"),
            (LocalInspectionOverallV1::Unavailable, "unavailable"),
            (LocalInspectionOverallV1::Unknown, "unknown"),
        ] {
            assert_eq!(overall_name(value), expected);
        }
    }
}
