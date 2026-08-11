//! Owner-private one-shot access to the current Runtime PXMT receipt.
//!
//! This module deliberately owns no durable receipt history and no lifecycle
//! discovery. The headless composition supplies one already completed Runtime
//! activation, the lifecycle supervisor publishes only a generation-bound
//! bootstrap locator, and the public client performs one authenticated Latest
//! exchange with no retry or fallback.

use core::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ed25519_dalek::{Signature, VerifyingKey};
use nix::unistd::{Gid, Uid};
use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
    MANAGED_MODEL_AGENT_STACK_TERMINAL_RECEIPT_VERSION, ManagedModelAgentStackTargetModeV1,
    ManagedModelAgentStackTerminalHeadV1, ManagedModelAgentStackTerminalLifecycleEffectV1,
    ManagedModelAgentStackTerminalOutcomeV1, ManagedModelAgentStackTerminalReceiptV1,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};
use zeroize::Zeroizing;

use crate::error::LocalProcessError;

pub(crate) const RECEIPT_BOOTSTRAP_HEADER_BYTES: usize = 320;
pub(crate) const MAX_RECEIPT_BOOTSTRAP_BYTES: usize = 832;
pub(crate) const MIN_RECEIPT_BOOTSTRAP_BYTES: usize = 321;

const RECEIPT_BOOTSTRAP_MAGIC: [u8; 4] = *b"PXRB";
const RECEIPT_BOOTSTRAP_VERSION: u16 = 1;
const RECEIPT_BOOTSTRAP_DIGEST_DOMAIN: &[u8] = b"paraegox.local.receipt-bootstrap.v1";
const RECEIPT_REQUEST_MAGIC: [u8; 4] = *b"PXRQ";
const RECEIPT_REQUEST_VERSION: u16 = 1;
const RECEIPT_REQUEST_ACTION: u8 = b'L';
const RECEIPT_REQUEST_BYTES: usize = 176;
const RECEIPT_REQUEST_TRANSPORT_BYTES: usize = 208;
const RECEIPT_REQUEST_DIGEST_DOMAIN: &[u8] = b"paraegox.local.receipt-latest-request.v1";
const RECEIPT_REQUEST_ID_DOMAIN: &[u8] = b"paraegox.local.receipt-request-id.v1";
const RECEIPT_RESPONSE_MAGIC: [u8; 4] = *b"PXRO";
const RECEIPT_RESPONSE_VERSION: u16 = 1;
const RECEIPT_RESPONSE_HEADER_BYTES: usize = 224;
const MAX_RECEIPT_PAYLOAD_BYTES: usize = 2_048;
const MAX_RECEIPT_RESPONSE_FRAME_BYTES: usize =
    RECEIPT_RESPONSE_HEADER_BYTES + MAX_RECEIPT_PAYLOAD_BYTES;
const RECEIPT_RESPONSE_DIGEST_DOMAIN: &[u8] = b"paraegox.local.receipt-latest-response.v1";
const RECEIPT_READY_OUTCOME: u8 = b'R';
const RECEIPT_NOT_FOUND_OUTCOME: u8 = b'N';
const RECEIPT_OPERATION_TIMEOUT_NANOS: u64 = 5_000_000_000;
const RECEIPT_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const RECEIPT_MAX_IN_FLIGHT: usize = 8;
const RECEIPT_PRIVATE_FILE_MODE: u32 = 0o600;
const RECEIPT_SOCKET_DIRECTORY_MODE: u32 = 0o2750;
const ED25519_ALGORITHM: u16 = 1;
const ED25519_ALGORITHM_VERSION: u16 = 1;
const ED25519_SIGNATURE_BYTES: usize = 64;

#[derive(Clone, Copy)]
struct ReceiptCorrelationV1 {
    runtime_target: [u8; 16],
    runtime_store_instance: [u8; 32],
    runtime_response_key_ref: [u8; 16],
    runtime_response_public_key: [u8; 32],
    expected_request_digest: [u8; 32],
    expected_receipt_digest: [u8; 32],
}

/// Single-use verified activation supplied by the existing composition.
///
/// It intentionally implements neither `Clone` nor `Debug`: the canonical
/// receipt is owner-private even though the eventual JSON projection is safe.
pub(crate) struct LocalReceiptActivationInputV1 {
    canonical_receipt: Box<[u8]>,
    correlation: ReceiptCorrelationV1,
}

impl LocalReceiptActivationInputV1 {
    pub(crate) fn try_new(
        canonical_receipt: Box<[u8]>,
        expected_request_digest: [u8; 32],
        expected_receipt_digest: [u8; 32],
        runtime_target: [u8; 16],
        runtime_store_instance: [u8; 32],
        runtime_response_key_ref: [u8; 16],
        runtime_response_public_key: [u8; 32],
    ) -> Result<Self, LocalProcessError> {
        let correlation = ReceiptCorrelationV1 {
            runtime_target,
            runtime_store_instance,
            runtime_response_key_ref,
            runtime_response_public_key,
            expected_request_digest,
            expected_receipt_digest,
        };
        verify_activation_receipt(&canonical_receipt, correlation)?;
        Ok(Self {
            canonical_receipt,
            correlation,
        })
    }
}

/// Exact lifecycle generation and config authority supplied by the supervisor.
///
/// This value is intentionally non-cloneable and has no debug projection.
pub(crate) struct LocalReceiptOwnerBindingV1 {
    generation: [u8; 16],
    config_commitment: [u8; 32],
}

impl LocalReceiptOwnerBindingV1 {
    pub(crate) fn try_new(
        generation: [u8; 16],
        config_commitment: [u8; 32],
    ) -> Result<Self, LocalProcessError> {
        if bytes_are_zero(&generation) || bytes_are_zero(&config_commitment) {
            return Err(LocalProcessError::LifecycleStartup);
        }
        Ok(Self {
            generation,
            config_commitment,
        })
    }
}

/// Token-free pin returned by the lifecycle PXRL decoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalReceiptBootstrapLocatorV1 {
    generation: [u8; 16],
    config_commitment: [u8; 32],
    path: PathBuf,
    content_length: u32,
    content_sha256: [u8; 32],
    device: u64,
    inode: u64,
}

impl LocalReceiptBootstrapLocatorV1 {
    pub(crate) fn try_from_pinned_parts(
        generation: [u8; 16],
        config_commitment: [u8; 32],
        path: PathBuf,
        content_length: u32,
        content_sha256: [u8; 32],
        device: u64,
        inode: u64,
    ) -> Result<Self, ()> {
        let content_length = usize::try_from(content_length).map_err(|_| ())?;
        if bytes_are_zero(&generation)
            || bytes_are_zero(&config_commitment)
            || bytes_are_zero(&content_sha256)
            || !(MIN_RECEIPT_BOOTSTRAP_BYTES..=MAX_RECEIPT_BOOTSTRAP_BYTES)
                .contains(&content_length)
            || !is_lexically_absolute_file(&path)
        {
            return Err(());
        }
        Ok(Self {
            generation,
            config_commitment,
            path,
            content_length: u32::try_from(content_length).map_err(|_| ())?,
            content_sha256,
            device,
            inode,
        })
    }

    pub(crate) const fn generation(&self) -> [u8; 16] {
        self.generation
    }

    pub(crate) const fn config_commitment(&self) -> [u8; 32] {
        self.config_commitment
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) const fn content_length(&self) -> u32 {
        self.content_length
    }

    pub(crate) const fn content_sha256(&self) -> [u8; 32] {
        self.content_sha256
    }

    pub(crate) const fn device(&self) -> u64 {
        self.device
    }

    pub(crate) const fn inode(&self) -> u64 {
        self.inode
    }
}

/// Public-safe point-in-time projection of one verified ActiveReady PXMT.
#[derive(Serialize)]
pub(crate) struct LocalReceiptSnapshotJsonV1 {
    snapshot_version: u16,
    query_scope: &'static str,
    source_owner: &'static str,
    record_kind: &'static str,
    receipt_version: u16,
    request_digest: String,
    receipt_digest: String,
    request_mode: &'static str,
    terminal_outcome: &'static str,
    lifecycle_effect: &'static str,
    desired_head: &'static str,
    desired_head_digest: String,
    fabric_generation: String,
    model_generation: String,
    agent_generation: String,
    physical_binding_census: u16,
    census_complete: bool,
    fabric_ready: bool,
    model_ready: bool,
    agent_ready: bool,
    fabric_to_agent_dependency_ready: bool,
    model_to_agent_dependency_ready: bool,
    exact_zero: bool,
    quarantined: bool,
    resource_census_digest: String,
    raw_outcome_digest: String,
    completion_runtime_host_epoch: String,
    completion_snapshot_sequence: String,
    selection_clock_generation: String,
    selection_observed_at_nanos: String,
    current_health_checked: bool,
}

pub(crate) struct LocalReceiptSnapshotReadV1 {
    generation: [u8; 16],
    snapshot: LocalReceiptSnapshotJsonV1,
}

impl LocalReceiptSnapshotReadV1 {
    pub(crate) const fn generation(&self) -> [u8; 16] {
        self.generation
    }

    pub(crate) const fn snapshot(&self) -> &LocalReceiptSnapshotJsonV1 {
        &self.snapshot
    }
}

struct LocalReceiptBootstrapV1 {
    generation: [u8; 16],
    config_commitment: [u8; 32],
    generation_token: Zeroizing<[u8; 32]>,
    server_uid: u32,
    server_gid: u32,
    request_id_seed: Zeroizing<[u8; 16]>,
    correlation: ReceiptCorrelationV1,
    socket_path: PathBuf,
}

impl fmt::Debug for LocalReceiptBootstrapV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalReceiptBootstrapV1")
            .field("generation", &"[BOUND]")
            .field("config_commitment", &"[BOUND]")
            .field("generation_token", &"[REDACTED]")
            .field("request_id_seed", &"[REDACTED]")
            .field("socket_path", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl LocalReceiptBootstrapV1 {
    fn encode(&self) -> Result<Vec<u8>, ReceiptWireErrorV1> {
        let path = path_bytes(&self.socket_path)?;
        if bytes_are_zero(&self.generation)
            || bytes_are_zero(&self.config_commitment)
            || bytes_are_zero(self.generation_token.as_ref())
            || self.server_uid == 0
            || self.server_gid == 0
            || bytes_are_zero(self.request_id_seed.as_ref())
            || !valid_correlation(self.correlation)
            || !(1..=512).contains(&path.len())
        {
            return Err(ReceiptWireErrorV1);
        }
        let frame_length = RECEIPT_BOOTSTRAP_HEADER_BYTES
            .checked_add(path.len())
            .ok_or(ReceiptWireErrorV1)?;
        if frame_length > MAX_RECEIPT_BOOTSTRAP_BYTES {
            return Err(ReceiptWireErrorV1);
        }
        let mut frame = vec![0_u8; frame_length];
        frame[..4].copy_from_slice(&RECEIPT_BOOTSTRAP_MAGIC);
        frame[4..6].copy_from_slice(&RECEIPT_BOOTSTRAP_VERSION.to_be_bytes());
        frame[6..8].copy_from_slice(
            &u16::try_from(RECEIPT_BOOTSTRAP_HEADER_BYTES)
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[8..12].copy_from_slice(
            &u32::try_from(frame_length)
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[12..16].copy_from_slice(
            &u32::try_from(path.len())
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[16..32].copy_from_slice(&self.generation);
        frame[32..64].copy_from_slice(&self.config_commitment);
        frame[64..96].copy_from_slice(self.generation_token.as_ref());
        frame[96..100].copy_from_slice(&self.server_uid.to_be_bytes());
        frame[100..104].copy_from_slice(&self.server_gid.to_be_bytes());
        frame[104..112].copy_from_slice(&RECEIPT_OPERATION_TIMEOUT_NANOS.to_be_bytes());
        frame[112..128].copy_from_slice(self.request_id_seed.as_ref());
        frame[128..144].copy_from_slice(&self.correlation.runtime_target);
        frame[144..176].copy_from_slice(&self.correlation.runtime_store_instance);
        frame[176..192].copy_from_slice(&self.correlation.runtime_response_key_ref);
        frame[192..224].copy_from_slice(&self.correlation.runtime_response_public_key);
        frame[224..256].copy_from_slice(&self.correlation.expected_request_digest);
        frame[256..288].copy_from_slice(&self.correlation.expected_receipt_digest);
        frame[RECEIPT_BOOTSTRAP_HEADER_BYTES..].copy_from_slice(path);
        let digest = digest_parts(
            RECEIPT_BOOTSTRAP_DIGEST_DOMAIN,
            &frame[..288],
            &frame[RECEIPT_BOOTSTRAP_HEADER_BYTES..],
        );
        frame[288..RECEIPT_BOOTSTRAP_HEADER_BYTES].copy_from_slice(&digest);
        Ok(frame)
    }

    fn decode(frame: &[u8]) -> Result<Self, ReceiptWireErrorV1> {
        if frame.len() < MIN_RECEIPT_BOOTSTRAP_BYTES
            || frame.len() > MAX_RECEIPT_BOOTSTRAP_BYTES
            || frame[..4] != RECEIPT_BOOTSTRAP_MAGIC
            || read_u16(frame, 4)? != RECEIPT_BOOTSTRAP_VERSION
            || usize::from(read_u16(frame, 6)?) != RECEIPT_BOOTSTRAP_HEADER_BYTES
            || usize::try_from(read_u32(frame, 8)?).map_err(|_| ReceiptWireErrorV1)? != frame.len()
        {
            return Err(ReceiptWireErrorV1);
        }
        let path_length = usize::try_from(read_u32(frame, 12)?).map_err(|_| ReceiptWireErrorV1)?;
        if !(1..=512).contains(&path_length)
            || RECEIPT_BOOTSTRAP_HEADER_BYTES.checked_add(path_length) != Some(frame.len())
            || read_u64(frame, 104)? != RECEIPT_OPERATION_TIMEOUT_NANOS
        {
            return Err(ReceiptWireErrorV1);
        }
        let declared_digest = array_at::<32>(frame, 288)?;
        if bytes_are_zero(&declared_digest)
            || declared_digest
                != digest_parts(
                    RECEIPT_BOOTSTRAP_DIGEST_DOMAIN,
                    &frame[..288],
                    &frame[RECEIPT_BOOTSTRAP_HEADER_BYTES..],
                )
        {
            return Err(ReceiptWireErrorV1);
        }
        let path = std::str::from_utf8(&frame[RECEIPT_BOOTSTRAP_HEADER_BYTES..])
            .map_err(|_| ReceiptWireErrorV1)?;
        let decoded = Self {
            generation: array_at::<16>(frame, 16)?,
            config_commitment: array_at::<32>(frame, 32)?,
            generation_token: Zeroizing::new(array_at::<32>(frame, 64)?),
            server_uid: read_u32(frame, 96)?,
            server_gid: read_u32(frame, 100)?,
            request_id_seed: Zeroizing::new(array_at::<16>(frame, 112)?),
            correlation: ReceiptCorrelationV1 {
                runtime_target: array_at::<16>(frame, 128)?,
                runtime_store_instance: array_at::<32>(frame, 144)?,
                runtime_response_key_ref: array_at::<16>(frame, 176)?,
                runtime_response_public_key: array_at::<32>(frame, 192)?,
                expected_request_digest: array_at::<32>(frame, 224)?,
                expected_receipt_digest: array_at::<32>(frame, 256)?,
            },
            socket_path: PathBuf::from(path),
        };
        let canonical = Zeroizing::new(decoded.encode()?);
        if bytes_are_zero(&decoded.generation)
            || bytes_are_zero(&decoded.config_commitment)
            || bytes_are_zero(decoded.generation_token.as_ref())
            || decoded.server_uid == 0
            || decoded.server_gid == 0
            || bytes_are_zero(decoded.request_id_seed.as_ref())
            || !valid_correlation(decoded.correlation)
            || !is_lexically_absolute_file(&decoded.socket_path)
            || canonical.as_slice() != frame
        {
            return Err(ReceiptWireErrorV1);
        }
        Ok(decoded)
    }

    fn request_id(&self) -> Result<[u8; 16], ReceiptWireErrorV1> {
        derive_request_id(&self.request_id_seed, 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReceiptLatestRequestV1 {
    request_id: [u8; 16],
    generation: [u8; 16],
    config_commitment: [u8; 32],
    expected_request_digest: [u8; 32],
    expected_receipt_digest: [u8; 32],
}

impl ReceiptLatestRequestV1 {
    fn encode(self) -> Result<[u8; RECEIPT_REQUEST_BYTES], ReceiptWireErrorV1> {
        if !self.valid_nonzero() {
            return Err(ReceiptWireErrorV1);
        }
        let mut frame = [0_u8; RECEIPT_REQUEST_BYTES];
        frame[..4].copy_from_slice(&RECEIPT_REQUEST_MAGIC);
        frame[4..6].copy_from_slice(&RECEIPT_REQUEST_VERSION.to_be_bytes());
        frame[6] = RECEIPT_REQUEST_ACTION;
        frame[8..10].copy_from_slice(
            &u16::try_from(RECEIPT_REQUEST_BYTES)
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[12..16].copy_from_slice(
            &u32::try_from(RECEIPT_REQUEST_BYTES)
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[16..32].copy_from_slice(&self.request_id);
        frame[32..48].copy_from_slice(&self.generation);
        frame[48..80].copy_from_slice(&self.config_commitment);
        frame[80..112].copy_from_slice(&self.expected_request_digest);
        frame[112..144].copy_from_slice(&self.expected_receipt_digest);
        let digest = digest_parts(RECEIPT_REQUEST_DIGEST_DOMAIN, &frame[..144], &[]);
        frame[144..176].copy_from_slice(&digest);
        Ok(frame)
    }

    fn decode(frame: &[u8]) -> Result<Self, ReceiptWireErrorV1> {
        if frame.len() != RECEIPT_REQUEST_BYTES
            || frame[..4] != RECEIPT_REQUEST_MAGIC
            || read_u16(frame, 4)? != RECEIPT_REQUEST_VERSION
            || frame[6] != RECEIPT_REQUEST_ACTION
            || frame[7] != 0
            || usize::from(read_u16(frame, 8)?) != RECEIPT_REQUEST_BYTES
            || frame[10..12].iter().any(|byte| *byte != 0)
            || usize::try_from(read_u32(frame, 12)?).map_err(|_| ReceiptWireErrorV1)?
                != RECEIPT_REQUEST_BYTES
        {
            return Err(ReceiptWireErrorV1);
        }
        let declared_digest = array_at::<32>(frame, 144)?;
        if bytes_are_zero(&declared_digest)
            || declared_digest != digest_parts(RECEIPT_REQUEST_DIGEST_DOMAIN, &frame[..144], &[])
        {
            return Err(ReceiptWireErrorV1);
        }
        let decoded = Self {
            request_id: array_at::<16>(frame, 16)?,
            generation: array_at::<16>(frame, 32)?,
            config_commitment: array_at::<32>(frame, 48)?,
            expected_request_digest: array_at::<32>(frame, 80)?,
            expected_receipt_digest: array_at::<32>(frame, 112)?,
        };
        if !decoded.valid_nonzero() || decoded.encode()?.as_slice() != frame {
            return Err(ReceiptWireErrorV1);
        }
        Ok(decoded)
    }

    fn valid_nonzero(self) -> bool {
        !bytes_are_zero(&self.request_id)
            && !bytes_are_zero(&self.generation)
            && !bytes_are_zero(&self.config_commitment)
            && !bytes_are_zero(&self.expected_request_digest)
            && !bytes_are_zero(&self.expected_receipt_digest)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReceiptResponseOutcomeV1 {
    Ready,
    NotFound,
}

struct ReceiptLatestResponseV1 {
    request: ReceiptLatestRequestV1,
    outcome: ReceiptResponseOutcomeV1,
    payload: Box<[u8]>,
}

impl ReceiptLatestResponseV1 {
    fn ready(
        request: ReceiptLatestRequestV1,
        payload: Arc<[u8]>,
    ) -> Result<Self, ReceiptWireErrorV1> {
        if payload.is_empty() || payload.len() > MAX_RECEIPT_PAYLOAD_BYTES {
            return Err(ReceiptWireErrorV1);
        }
        Ok(Self {
            request,
            outcome: ReceiptResponseOutcomeV1::Ready,
            payload: payload.as_ref().into(),
        })
    }

    fn not_found(request: ReceiptLatestRequestV1) -> Self {
        Self {
            request,
            outcome: ReceiptResponseOutcomeV1::NotFound,
            payload: Box::new([]),
        }
    }

    fn encode_frame(&self) -> Result<Vec<u8>, ReceiptWireErrorV1> {
        if !self.request.valid_nonzero() {
            return Err(ReceiptWireErrorV1);
        }
        match self.outcome {
            ReceiptResponseOutcomeV1::Ready
                if self.payload.is_empty() || self.payload.len() > MAX_RECEIPT_PAYLOAD_BYTES =>
            {
                return Err(ReceiptWireErrorV1);
            }
            ReceiptResponseOutcomeV1::NotFound if !self.payload.is_empty() => {
                return Err(ReceiptWireErrorV1);
            }
            ReceiptResponseOutcomeV1::Ready | ReceiptResponseOutcomeV1::NotFound => {}
        }
        let frame_length = RECEIPT_RESPONSE_HEADER_BYTES
            .checked_add(self.payload.len())
            .ok_or(ReceiptWireErrorV1)?;
        let mut frame = vec![0_u8; frame_length];
        frame[..4].copy_from_slice(&RECEIPT_RESPONSE_MAGIC);
        frame[4..6].copy_from_slice(&RECEIPT_RESPONSE_VERSION.to_be_bytes());
        frame[6] = RECEIPT_REQUEST_ACTION;
        frame[7] = match self.outcome {
            ReceiptResponseOutcomeV1::Ready => RECEIPT_READY_OUTCOME,
            ReceiptResponseOutcomeV1::NotFound => RECEIPT_NOT_FOUND_OUTCOME,
        };
        frame[8..10].copy_from_slice(
            &u16::try_from(RECEIPT_RESPONSE_HEADER_BYTES)
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[12..16].copy_from_slice(
            &u32::try_from(frame_length)
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[16..20].copy_from_slice(
            &u32::try_from(self.payload.len())
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        frame[24..40].copy_from_slice(&self.request.request_id);
        frame[40..56].copy_from_slice(&self.request.generation);
        frame[56..88].copy_from_slice(&self.request.config_commitment);
        frame[88..120].copy_from_slice(&self.request.expected_request_digest);
        frame[120..152].copy_from_slice(&self.request.expected_receipt_digest);
        if matches!(self.outcome, ReceiptResponseOutcomeV1::Ready) {
            let payload_digest: [u8; 32] = Sha256::digest(&self.payload).into();
            frame[152..184].copy_from_slice(&payload_digest);
        }
        frame[RECEIPT_RESPONSE_HEADER_BYTES..].copy_from_slice(&self.payload);
        let digest = digest_parts(
            RECEIPT_RESPONSE_DIGEST_DOMAIN,
            &frame[..192],
            &frame[RECEIPT_RESPONSE_HEADER_BYTES..],
        );
        frame[192..RECEIPT_RESPONSE_HEADER_BYTES].copy_from_slice(&digest);
        Ok(frame)
    }

    fn encode_transport(&self) -> Result<Vec<u8>, ReceiptWireErrorV1> {
        let frame = self.encode_frame()?;
        let mut transport = Vec::with_capacity(4 + frame.len());
        transport.extend_from_slice(
            &u32::try_from(frame.len())
                .map_err(|_| ReceiptWireErrorV1)?
                .to_be_bytes(),
        );
        transport.extend_from_slice(&frame);
        Ok(transport)
    }

    fn decode_frame(frame: &[u8]) -> Result<Self, ReceiptWireErrorV1> {
        if frame.len() < RECEIPT_RESPONSE_HEADER_BYTES
            || frame.len() > MAX_RECEIPT_RESPONSE_FRAME_BYTES
            || frame[..4] != RECEIPT_RESPONSE_MAGIC
            || read_u16(frame, 4)? != RECEIPT_RESPONSE_VERSION
            || frame[6] != RECEIPT_REQUEST_ACTION
            || usize::from(read_u16(frame, 8)?) != RECEIPT_RESPONSE_HEADER_BYTES
            || frame[10..12].iter().any(|byte| *byte != 0)
            || usize::try_from(read_u32(frame, 12)?).map_err(|_| ReceiptWireErrorV1)? != frame.len()
            || frame[20..24].iter().any(|byte| *byte != 0)
            || frame[184..192].iter().any(|byte| *byte != 0)
        {
            return Err(ReceiptWireErrorV1);
        }
        let payload_length =
            usize::try_from(read_u32(frame, 16)?).map_err(|_| ReceiptWireErrorV1)?;
        if RECEIPT_RESPONSE_HEADER_BYTES.checked_add(payload_length) != Some(frame.len())
            || payload_length > MAX_RECEIPT_PAYLOAD_BYTES
        {
            return Err(ReceiptWireErrorV1);
        }
        let outcome = match frame[7] {
            RECEIPT_READY_OUTCOME if payload_length > 0 => ReceiptResponseOutcomeV1::Ready,
            RECEIPT_NOT_FOUND_OUTCOME if payload_length == 0 => ReceiptResponseOutcomeV1::NotFound,
            _ => return Err(ReceiptWireErrorV1),
        };
        let payload = &frame[RECEIPT_RESPONSE_HEADER_BYTES..];
        let declared_payload_digest = array_at::<32>(frame, 152)?;
        match outcome {
            ReceiptResponseOutcomeV1::Ready
                if declared_payload_digest != <[u8; 32]>::from(Sha256::digest(payload)) =>
            {
                return Err(ReceiptWireErrorV1);
            }
            ReceiptResponseOutcomeV1::NotFound if !bytes_are_zero(&declared_payload_digest) => {
                return Err(ReceiptWireErrorV1);
            }
            ReceiptResponseOutcomeV1::Ready | ReceiptResponseOutcomeV1::NotFound => {}
        }
        let declared_frame_digest = array_at::<32>(frame, 192)?;
        if bytes_are_zero(&declared_frame_digest)
            || declared_frame_digest
                != digest_parts(RECEIPT_RESPONSE_DIGEST_DOMAIN, &frame[..192], payload)
        {
            return Err(ReceiptWireErrorV1);
        }
        let decoded = Self {
            request: ReceiptLatestRequestV1 {
                request_id: array_at::<16>(frame, 24)?,
                generation: array_at::<16>(frame, 40)?,
                config_commitment: array_at::<32>(frame, 56)?,
                expected_request_digest: array_at::<32>(frame, 88)?,
                expected_receipt_digest: array_at::<32>(frame, 120)?,
            },
            outcome,
            payload: payload.into(),
        };
        if !decoded.request.valid_nonzero() || decoded.encode_frame()?.as_slice() != frame {
            return Err(ReceiptWireErrorV1);
        }
        Ok(decoded)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReceiptWireErrorV1;

struct ReceiptSlotV1 {
    active_payload: Option<Arc<[u8]>>,
}

/// Opaque one-owner retirement lease transferred to the lifecycle supervisor.
///
/// Dropping this value is synchronous retirement. No caller can inspect or
/// recover the canonical PXMT bytes through this seam.
pub(crate) struct LocalReceiptRetirementHandleV1 {
    slot: Arc<Mutex<ReceiptSlotV1>>,
    retired: bool,
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct LocalReceiptRetirementProbeV1 {
    slot: Arc<Mutex<ReceiptSlotV1>>,
}

#[cfg(test)]
impl LocalReceiptRetirementProbeV1 {
    pub(crate) fn is_retired(&self) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active_payload
            .is_none()
    }
}

impl LocalReceiptRetirementHandleV1 {
    #[cfg(test)]
    pub(crate) fn for_test() -> (Self, LocalReceiptRetirementProbeV1) {
        let slot = Arc::new(Mutex::new(ReceiptSlotV1 {
            active_payload: Some(Arc::from(&b"test-receipt"[..])),
        }));
        (
            Self {
                slot: Arc::clone(&slot),
                retired: false,
            },
            LocalReceiptRetirementProbeV1 { slot },
        )
    }

    fn retire(&mut self) {
        if self.retired {
            return;
        }
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.active_payload = None;
        self.retired = true;
    }
}

impl Drop for LocalReceiptRetirementHandleV1 {
    fn drop(&mut self) {
        self.retire();
    }
}

/// Ready-only transfer object. It carries the private path only until the
/// lifecycle captures a token-free PXRL pin and then owns the retire handle.
pub(crate) struct LocalReceiptAdapterReadyLeaseV1 {
    bootstrap_path: PathBuf,
    retirement: LocalReceiptRetirementHandleV1,
}

impl LocalReceiptAdapterReadyLeaseV1 {
    pub(crate) fn into_parts(self) -> (PathBuf, LocalReceiptRetirementHandleV1) {
        (self.bootstrap_path, self.retirement)
    }
}

/// Joined lifecycle for the owner-private endpoint and bootstrap file.
pub(crate) struct DeveloperLocalReceiptSnapshotLifecycleV1 {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<Result<(), LocalProcessError>>>,
    slot: Arc<Mutex<ReceiptSlotV1>>,
}

impl fmt::Debug for DeveloperLocalReceiptSnapshotLifecycleV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeveloperLocalReceiptSnapshotLifecycleV1")
            .field("endpoint", &"[REDACTED]")
            .field("running", &self.thread.is_some())
            .finish()
    }
}

impl DeveloperLocalReceiptSnapshotLifecycleV1 {
    pub(crate) fn shutdown_and_join(mut self) -> Result<(), LocalProcessError> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<(), LocalProcessError> {
        {
            let mut slot = self
                .slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            slot.active_payload = None;
        }
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.thread.take().map_or(Ok(()), |thread| {
            thread
                .join()
                .map_err(|_| LocalProcessError::LifecycleShutdown)?
        })
    }
}

impl Drop for DeveloperLocalReceiptSnapshotLifecycleV1 {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

struct ReceiptServerV1 {
    bootstrap: LocalReceiptBootstrapV1,
    slot: Arc<Mutex<ReceiptSlotV1>>,
}

/// Starts the bounded endpoint after independently validating the Runtime PXMT.
pub(crate) fn start_developer_local_receipt_snapshot_v1(
    activation: LocalReceiptActivationInputV1,
    binding: LocalReceiptOwnerBindingV1,
    socket_path: PathBuf,
    bootstrap_path: PathBuf,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<
    (
        DeveloperLocalReceiptSnapshotLifecycleV1,
        LocalReceiptAdapterReadyLeaseV1,
    ),
    LocalProcessError,
> {
    if expected_uid == 0 || expected_gid == 0 {
        return Err(LocalProcessError::LifecycleStartup);
    }
    validate_endpoint_paths(
        &socket_path,
        &bootstrap_path,
        expected_uid,
        expected_gid,
        LocalProcessError::LifecycleStartup,
    )?;
    let mut entropy = Zeroizing::new([0_u8; 48]);
    getrandom::fill(entropy.as_mut()).map_err(|_| LocalProcessError::LifecycleStartup)?;
    let mut generation_token = Zeroizing::new([0_u8; 32]);
    generation_token.copy_from_slice(&entropy[..32]);
    let mut request_id_seed = Zeroizing::new([0_u8; 16]);
    request_id_seed.copy_from_slice(&entropy[32..]);
    if bytes_are_zero(generation_token.as_ref()) || bytes_are_zero(request_id_seed.as_ref()) {
        return Err(LocalProcessError::LifecycleStartup);
    }
    let bootstrap = LocalReceiptBootstrapV1 {
        generation: binding.generation,
        config_commitment: binding.config_commitment,
        generation_token,
        server_uid: expected_uid,
        server_gid: expected_gid,
        request_id_seed,
        correlation: activation.correlation,
        socket_path: socket_path.clone(),
    };
    let bootstrap_wire = Zeroizing::new(
        bootstrap
            .encode()
            .map_err(|_| LocalProcessError::LifecycleStartup)?,
    );
    let slot = Arc::new(Mutex::new(ReceiptSlotV1 {
        active_payload: Some(Arc::from(activation.canonical_receipt)),
    }));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    let mut files = ReceiptEndpointFilesV1::try_new(
        socket_path.clone(),
        bootstrap_path.clone(),
        expected_uid,
        expected_gid,
    )?;
    let listener = {
        let _entered = runtime.enter();
        bind_receipt_listener(&socket_path, &mut files, expected_uid, expected_gid)?
    };
    create_receipt_bootstrap(
        &bootstrap_path,
        bootstrap_wire.as_slice(),
        &mut files,
        expected_uid,
        expected_gid,
    )?;
    let server = Arc::new(ReceiptServerV1 {
        bootstrap,
        slot: Arc::clone(&slot),
    });
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let thread = thread::Builder::new()
        .name("paraegox-local-receipt-v1".to_owned())
        .spawn(move || {
            let serve_result =
                runtime.block_on(serve_receipt_endpoint(listener, server, shutdown_receiver));
            let cleanup_result = files.cleanup();
            serve_result.and(cleanup_result)
        })
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    Ok((
        DeveloperLocalReceiptSnapshotLifecycleV1 {
            shutdown: Some(shutdown_sender),
            thread: Some(thread),
            slot: Arc::clone(&slot),
        },
        LocalReceiptAdapterReadyLeaseV1 {
            bootstrap_path,
            retirement: LocalReceiptRetirementHandleV1 {
                slot,
                retired: false,
            },
        },
    ))
}

async fn serve_receipt_endpoint(
    listener: UnixListener,
    server: Arc<ReceiptServerV1>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), LocalProcessError> {
    let permits = Arc::new(Semaphore::new(RECEIPT_MAX_IN_FLIGHT));
    let mut tasks = JoinSet::new();
    let mut task_panicked = false;
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            completed = tasks.join_next(), if !tasks.is_empty() => {
                if completed.is_some_and(|result| result.is_err()) {
                    task_panicked = true;
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|_| LocalProcessError::LifecycleShutdown)?;
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    drop(stream);
                    continue;
                };
                if !peer_matches(
                    &stream,
                    server.bootstrap.server_uid,
                    server.bootstrap.server_gid,
                ) {
                    drop(stream);
                    continue;
                }
                let server = Arc::clone(&server);
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = serve_receipt_exchange(stream, server).await;
                });
            }
        }
    }
    let deadline = Instant::now() + RECEIPT_OPERATION_TIMEOUT;
    while !tasks.is_empty() {
        match timeout_at(deadline, tasks.join_next()).await {
            Ok(Some(Ok(()))) => {}
            Ok(Some(Err(_))) => task_panicked = true,
            Ok(None) => break,
            Err(_) => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                break;
            }
        }
    }
    if task_panicked {
        Err(LocalProcessError::LifecycleShutdown)
    } else {
        Ok(())
    }
}

async fn serve_receipt_exchange(
    mut stream: UnixStream,
    server: Arc<ReceiptServerV1>,
) -> Result<(), ReceiptWireErrorV1> {
    let deadline = Instant::now() + RECEIPT_OPERATION_TIMEOUT;
    let mut transport = Zeroizing::new([0_u8; RECEIPT_REQUEST_TRANSPORT_BYTES]);
    timeout_at(deadline, stream.read_exact(transport.as_mut()))
        .await
        .map_err(|_| ReceiptWireErrorV1)?
        .map_err(|_| ReceiptWireErrorV1)?;
    let mut trailing = [0_u8; 1];
    if timeout_at(deadline, stream.read(&mut trailing))
        .await
        .map_err(|_| ReceiptWireErrorV1)?
        .map_err(|_| ReceiptWireErrorV1)?
        != 0
    {
        return Err(ReceiptWireErrorV1);
    }
    if !constant_time_equal(&transport[..32], server.bootstrap.generation_token.as_ref()) {
        return Err(ReceiptWireErrorV1);
    }
    let request = ReceiptLatestRequestV1::decode(&transport[32..])?;
    if request.request_id != server.bootstrap.request_id()?
        || request.generation != server.bootstrap.generation
        || request.config_commitment != server.bootstrap.config_commitment
        || request.expected_request_digest != server.bootstrap.correlation.expected_request_digest
        || request.expected_receipt_digest != server.bootstrap.correlation.expected_receipt_digest
    {
        return Err(ReceiptWireErrorV1);
    }
    let payload = server
        .slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .active_payload
        .clone();
    let response = payload.map_or_else(
        || Ok(ReceiptLatestResponseV1::not_found(request)),
        |payload| ReceiptLatestResponseV1::ready(request, payload),
    )?;
    let response = response.encode_transport()?;
    timeout_at(deadline, async {
        stream.write_all(&response).await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| ReceiptWireErrorV1)?
    .map_err(|_| ReceiptWireErrorV1)
}

/// Reads exactly one receipt from the generation selected by lifecycle PXRL.
pub(crate) fn read_latest_receipt_snapshot(
    locator: &LocalReceiptBootstrapLocatorV1,
) -> Result<LocalReceiptSnapshotReadV1, LocalProcessError> {
    let (bootstrap, socket_identity) = load_receipt_bootstrap(locator)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    let snapshot = runtime.block_on(exchange_latest_receipt(&bootstrap, socket_identity))?;
    Ok(LocalReceiptSnapshotReadV1 {
        generation: locator.generation,
        snapshot,
    })
}

fn load_receipt_bootstrap(
    locator: &LocalReceiptBootstrapLocatorV1,
) -> Result<(LocalReceiptBootstrapV1, FileIdentityV1), LocalProcessError> {
    let path = locator.path();
    if !is_lexically_absolute_file(path) {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    let expected_uid = Uid::effective().as_raw();
    let expected_gid = Gid::effective().as_raw();
    if expected_uid == 0 || expected_gid == 0 {
        return Err(LocalProcessError::UnsafeExecutionIdentity);
    }
    validate_private_parent(
        path,
        expected_uid,
        expected_gid,
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    let before =
        fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    validate_bootstrap_metadata(
        &before,
        expected_uid,
        expected_gid,
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    let opened = file
        .metadata()
        .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    let after = fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    validate_bootstrap_metadata(
        &opened,
        expected_uid,
        expected_gid,
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    validate_bootstrap_metadata(
        &after,
        expected_uid,
        expected_gid,
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    let identity = FileIdentityV1::from_metadata(&opened);
    if FileIdentityV1::from_metadata(&before) != identity
        || FileIdentityV1::from_metadata(&after) != identity
        || identity.device != locator.device()
        || identity.inode != locator.inode()
        || opened.len() != u64::from(locator.content_length())
    {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    let wire = read_bounded_bootstrap(&file)?;
    if <[u8; 32]>::from(Sha256::digest(wire.as_slice())) != locator.content_sha256() {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    let bootstrap = LocalReceiptBootstrapV1::decode(wire.as_slice())
        .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    if bootstrap.generation != locator.generation()
        || bootstrap.config_commitment != locator.config_commitment()
        || bootstrap.server_uid != expected_uid
        || bootstrap.server_gid != expected_gid
        || bootstrap.socket_path.parent() != path.parent()
    {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    let socket_identity = validate_socket_identity(
        &bootstrap.socket_path,
        expected_uid,
        expected_gid,
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    let final_metadata =
        fs::symlink_metadata(path).map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    validate_bootstrap_metadata(
        &final_metadata,
        expected_uid,
        expected_gid,
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    if FileIdentityV1::from_metadata(&final_metadata) != identity
        || final_metadata.len() != u64::from(locator.content_length())
    {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    Ok((bootstrap, socket_identity))
}

async fn exchange_latest_receipt(
    bootstrap: &LocalReceiptBootstrapV1,
    socket_identity: FileIdentityV1,
) -> Result<LocalReceiptSnapshotJsonV1, LocalProcessError> {
    let request = ReceiptLatestRequestV1 {
        request_id: bootstrap
            .request_id()
            .map_err(|_| LocalProcessError::LocalReceiptProtocol)?,
        generation: bootstrap.generation,
        config_commitment: bootstrap.config_commitment,
        expected_request_digest: bootstrap.correlation.expected_request_digest,
        expected_receipt_digest: bootstrap.correlation.expected_receipt_digest,
    };
    let request_frame = request
        .encode()
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?;
    let mut transport = Zeroizing::new([0_u8; RECEIPT_REQUEST_TRANSPORT_BYTES]);
    transport[..32].copy_from_slice(bootstrap.generation_token.as_ref());
    transport[32..].copy_from_slice(&request_frame);
    let deadline = Instant::now() + RECEIPT_OPERATION_TIMEOUT;
    let mut stream = timeout_at(deadline, UnixStream::connect(&bootstrap.socket_path))
        .await
        .map_err(|_| LocalProcessError::LocalReceiptIo)?
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    let credentials = stream
        .peer_cred()
        .map_err(|_| LocalProcessError::LocalReceiptPeer)?;
    if credentials.uid() != bootstrap.server_uid || credentials.gid() != bootstrap.server_gid {
        return Err(LocalProcessError::LocalReceiptPeer);
    }
    validate_socket_path_identity(&bootstrap.socket_path, socket_identity)?;
    timeout_at(deadline, stream.write_all(transport.as_ref()))
        .await
        .map_err(|_| LocalProcessError::LocalReceiptIo)?
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    let write_shutdown = timeout_at(deadline, stream.shutdown())
        .await
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    accept_client_write_half_shutdown(write_shutdown)
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    let mut frame_length = [0_u8; 4];
    timeout_at(deadline, stream.read_exact(&mut frame_length))
        .await
        .map_err(|_| LocalProcessError::LocalReceiptIo)?
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    let frame_length = usize::try_from(u32::from_be_bytes(frame_length))
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?;
    if !(RECEIPT_RESPONSE_HEADER_BYTES..=MAX_RECEIPT_RESPONSE_FRAME_BYTES).contains(&frame_length) {
        return Err(LocalProcessError::LocalReceiptProtocol);
    }
    let mut frame = vec![0_u8; frame_length];
    timeout_at(deadline, stream.read_exact(&mut frame))
        .await
        .map_err(|_| LocalProcessError::LocalReceiptIo)?
        .map_err(|_| LocalProcessError::LocalReceiptIo)?;
    let mut trailing = [0_u8; 1];
    if timeout_at(deadline, stream.read(&mut trailing))
        .await
        .map_err(|_| LocalProcessError::LocalReceiptIo)?
        .map_err(|_| LocalProcessError::LocalReceiptIo)?
        != 0
    {
        return Err(LocalProcessError::LocalReceiptProtocol);
    }
    validate_socket_path_identity(&bootstrap.socket_path, socket_identity)?;
    if Instant::now() >= deadline {
        return Err(LocalProcessError::LocalReceiptIo);
    }
    let response = ReceiptLatestResponseV1::decode_frame(&frame)
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?;
    if response.request != request {
        return Err(LocalProcessError::LocalReceiptProtocol);
    }
    match response.outcome {
        ReceiptResponseOutcomeV1::NotFound => Err(LocalProcessError::LocalReceiptNotFound),
        ReceiptResponseOutcomeV1::Ready => {
            verify_client_receipt(&response.payload, bootstrap.correlation)
        }
    }
}

fn verify_activation_receipt(
    wire: &[u8],
    correlation: ReceiptCorrelationV1,
) -> Result<(), LocalProcessError> {
    if wire.is_empty() || wire.len() > MAX_RECEIPT_PAYLOAD_BYTES || !valid_correlation(correlation)
    {
        return Err(LocalProcessError::LifecycleStartup);
    }
    let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(wire)
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    let facts = receipt.facts();
    let state = facts.state();
    let evidence = facts.evidence().fields();
    let desired = facts
        .desired_head_digest()
        .ok_or(LocalProcessError::LifecycleStartup)?;
    if receipt.canonical_wire() != wire
        || facts.target().as_bytes() != &correlation.runtime_target
        || facts.runtime_store_instance_id() != correlation.runtime_store_instance
        || facts.request_digest().as_bytes() != &correlation.expected_request_digest
        || receipt.receipt_digest().as_bytes() != &correlation.expected_receipt_digest
        || receipt.authentication_key().as_bytes() != &correlation.runtime_response_key_ref
        || receipt.authentication_algorithm().value() != ED25519_ALGORITHM
        || receipt.authentication_algorithm_version() != ED25519_ALGORITHM_VERSION
        || receipt.authentication_signature().len() != ED25519_SIGNATURE_BYTES
        || facts.request_mode() != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
        || state.outcome() != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
        || state.lifecycle_effect()
            != ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted
        || state.head() != ManagedModelAgentStackTerminalHeadV1::CommittedIncoming
        || bytes_are_zero(desired.value().as_bytes())
        || state.fabric_generation().is_none()
        || state.model_generation().is_none()
        || state.agent_generation().is_none()
        || evidence.physical_binding_census != 2
        || !evidence.census_complete
        || !evidence.fabric_ready
        || !evidence.model_ready
        || !evidence.agent_ready
        || !evidence.fabric_to_agent_dependency_ready
        || !evidence.model_to_agent_dependency_ready
        || evidence.exact_zero
        || evidence.quarantined
    {
        return Err(LocalProcessError::LifecycleStartup);
    }
    let signature: [u8; ED25519_SIGNATURE_BYTES] = receipt
        .authentication_signature()
        .try_into()
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    VerifyingKey::from_bytes(&correlation.runtime_response_public_key)
        .map_err(|_| LocalProcessError::LifecycleStartup)?
        .verify_strict(
            receipt
                .signing_transcript()
                .map_err(|_| LocalProcessError::LifecycleStartup)?
                .as_bytes(),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| LocalProcessError::LifecycleStartup)
}

fn verify_client_receipt(
    wire: &[u8],
    correlation: ReceiptCorrelationV1,
) -> Result<LocalReceiptSnapshotJsonV1, LocalProcessError> {
    if wire.is_empty() || wire.len() > MAX_RECEIPT_PAYLOAD_BYTES || !valid_correlation(correlation)
    {
        return Err(LocalProcessError::LocalReceiptProtocol);
    }
    let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(wire)
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?;
    let facts = receipt.facts();
    let state = facts.state();
    let evidence = facts.evidence().fields();
    let desired = facts
        .desired_head_digest()
        .ok_or(LocalProcessError::LocalReceiptProtocol)?;
    let fabric_generation = state
        .fabric_generation()
        .ok_or(LocalProcessError::LocalReceiptProtocol)?;
    let model_generation = state
        .model_generation()
        .ok_or(LocalProcessError::LocalReceiptProtocol)?;
    let agent_generation = state
        .agent_generation()
        .ok_or(LocalProcessError::LocalReceiptProtocol)?;
    if receipt.canonical_wire() != wire
        || facts.target().as_bytes() != &correlation.runtime_target
        || facts.runtime_store_instance_id() != correlation.runtime_store_instance
        || facts.request_digest().as_bytes() != &correlation.expected_request_digest
        || receipt.receipt_digest().as_bytes() != &correlation.expected_receipt_digest
        || receipt.authentication_key().as_bytes() != &correlation.runtime_response_key_ref
        || receipt.authentication_algorithm().value() != ED25519_ALGORITHM
        || receipt.authentication_algorithm_version() != ED25519_ALGORITHM_VERSION
        || receipt.authentication_signature().len() != ED25519_SIGNATURE_BYTES
        || facts.request_mode() != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
        || state.outcome() != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
        || state.lifecycle_effect()
            != ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted
        || state.head() != ManagedModelAgentStackTerminalHeadV1::CommittedIncoming
        || bytes_are_zero(desired.value().as_bytes())
        || evidence.physical_binding_census != 2
        || !evidence.census_complete
        || !evidence.fabric_ready
        || !evidence.model_ready
        || !evidence.agent_ready
        || !evidence.fabric_to_agent_dependency_ready
        || !evidence.model_to_agent_dependency_ready
        || evidence.exact_zero
        || evidence.quarantined
    {
        return Err(LocalProcessError::LocalReceiptProtocol);
    }
    let signature: [u8; ED25519_SIGNATURE_BYTES] = receipt
        .authentication_signature()
        .try_into()
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?;
    VerifyingKey::from_bytes(&correlation.runtime_response_public_key)
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?
        .verify_strict(
            receipt
                .signing_transcript()
                .map_err(|_| LocalProcessError::LocalReceiptProtocol)?
                .as_bytes(),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| LocalProcessError::LocalReceiptProtocol)?;
    Ok(LocalReceiptSnapshotJsonV1 {
        snapshot_version: 1,
        query_scope: "current_running_generation",
        source_owner: "runtime_host",
        record_kind: "managed_model_agent_stack_terminal_receipt",
        receipt_version: MANAGED_MODEL_AGENT_STACK_TERMINAL_RECEIPT_VERSION,
        request_digest: lower_hex(&correlation.expected_request_digest),
        receipt_digest: lower_hex(&correlation.expected_receipt_digest),
        request_mode: "fabric_model_and_agent",
        terminal_outcome: "active_ready",
        lifecycle_effect: "may_have_started",
        desired_head: "committed_incoming",
        desired_head_digest: lower_hex(desired.value().as_bytes()),
        fabric_generation: fabric_generation.value().to_string(),
        model_generation: model_generation.value().to_string(),
        agent_generation: agent_generation.value().to_string(),
        physical_binding_census: evidence.physical_binding_census,
        census_complete: evidence.census_complete,
        fabric_ready: evidence.fabric_ready,
        model_ready: evidence.model_ready,
        agent_ready: evidence.agent_ready,
        fabric_to_agent_dependency_ready: evidence.fabric_to_agent_dependency_ready,
        model_to_agent_dependency_ready: evidence.model_to_agent_dependency_ready,
        exact_zero: evidence.exact_zero,
        quarantined: evidence.quarantined,
        resource_census_digest: lower_hex(evidence.resource_census_digest.as_bytes()),
        raw_outcome_digest: lower_hex(evidence.raw_outcome_digest.as_bytes()),
        completion_runtime_host_epoch: evidence.completion_runtime_host_epoch.to_string(),
        completion_snapshot_sequence: evidence.completion_snapshot_sequence.to_string(),
        selection_clock_generation: evidence.selection_clock_generation.value().to_string(),
        selection_observed_at_nanos: evidence.selection_observed_at_nanos.to_string(),
        current_health_checked: false,
    })
}

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

struct ReceiptEndpointFilesV1 {
    socket_path: PathBuf,
    bootstrap_path: PathBuf,
    socket_identity: Option<FileIdentityV1>,
    bootstrap_identity: Option<FileIdentityV1>,
    cleaned: bool,
}

impl ReceiptEndpointFilesV1 {
    fn try_new(
        socket_path: PathBuf,
        bootstrap_path: PathBuf,
        expected_uid: u32,
        expected_gid: u32,
    ) -> Result<Self, LocalProcessError> {
        validate_endpoint_paths(
            &socket_path,
            &bootstrap_path,
            expected_uid,
            expected_gid,
            LocalProcessError::LifecycleStartup,
        )?;
        for path in [&socket_path, &bootstrap_path] {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) | Err(_) => return Err(LocalProcessError::LifecycleStartup),
            }
        }
        Ok(Self {
            socket_path,
            bootstrap_path,
            socket_identity: None,
            bootstrap_identity: None,
            cleaned: false,
        })
    }

    fn cleanup(&mut self) -> Result<(), LocalProcessError> {
        if self.cleaned {
            return Ok(());
        }
        let mut result = Ok(());
        for (path, expected) in [
            (&self.bootstrap_path, self.bootstrap_identity),
            (&self.socket_path, self.socket_identity),
        ] {
            let Some(expected) = expected else {
                continue;
            };
            match fs::symlink_metadata(path) {
                Ok(metadata) if FileIdentityV1::from_metadata(&metadata) == expected => {
                    if fs::remove_file(path).is_err() {
                        result = Err(LocalProcessError::LifecycleShutdown);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) | Err(_) => result = Err(LocalProcessError::LifecycleShutdown),
            }
        }
        if let Some(parent) = self.socket_path.parent()
            && File::open(parent)
                .and_then(|directory| directory.sync_all())
                .is_err()
        {
            result = Err(LocalProcessError::LifecycleShutdown);
        }
        self.cleaned = true;
        result
    }
}

impl Drop for ReceiptEndpointFilesV1 {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn bind_receipt_listener(
    socket_path: &Path,
    files: &mut ReceiptEndpointFilesV1,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<UnixListener, LocalProcessError> {
    let listener =
        StdUnixListener::bind(socket_path).map_err(|_| LocalProcessError::LifecycleStartup)?;
    fs::set_permissions(
        socket_path,
        fs::Permissions::from_mode(RECEIPT_PRIVATE_FILE_MODE),
    )
    .map_err(|_| LocalProcessError::LifecycleStartup)?;
    let metadata =
        fs::symlink_metadata(socket_path).map_err(|_| LocalProcessError::LifecycleStartup)?;
    validate_socket_metadata(
        &metadata,
        expected_uid,
        expected_gid,
        LocalProcessError::LifecycleStartup,
    )?;
    files.socket_identity = Some(FileIdentityV1::from_metadata(&metadata));
    listener
        .set_nonblocking(true)
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    UnixListener::from_std(listener).map_err(|_| LocalProcessError::LifecycleStartup)
}

fn create_receipt_bootstrap(
    path: &Path,
    wire: &[u8],
    files: &mut ReceiptEndpointFilesV1,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), LocalProcessError> {
    if !(MIN_RECEIPT_BOOTSTRAP_BYTES..=MAX_RECEIPT_BOOTSTRAP_BYTES).contains(&wire.len()) {
        return Err(LocalProcessError::LifecycleStartup);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(RECEIPT_PRIVATE_FILE_MODE)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    file.write_all(wire)
        .and_then(|()| file.sync_all())
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    fs::set_permissions(path, fs::Permissions::from_mode(RECEIPT_PRIVATE_FILE_MODE))
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    let metadata = file
        .metadata()
        .map_err(|_| LocalProcessError::LifecycleStartup)?;
    validate_bootstrap_metadata(
        &metadata,
        expected_uid,
        expected_gid,
        LocalProcessError::LifecycleStartup,
    )?;
    if metadata.len()
        != u64::try_from(wire.len()).map_err(|_| LocalProcessError::LifecycleStartup)?
    {
        return Err(LocalProcessError::LifecycleStartup);
    }
    files.bootstrap_identity = Some(FileIdentityV1::from_metadata(&metadata));
    File::open(path.parent().ok_or(LocalProcessError::LifecycleStartup)?)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| LocalProcessError::LifecycleStartup)
}

fn read_bounded_bootstrap(file: &File) -> Result<Zeroizing<Vec<u8>>, LocalProcessError> {
    let length = usize::try_from(
        file.metadata()
            .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?
            .len(),
    )
    .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    if !(MIN_RECEIPT_BOOTSTRAP_BYTES..=MAX_RECEIPT_BOOTSTRAP_BYTES).contains(&length) {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    let mut wire = Zeroizing::new(Vec::with_capacity(length));
    file.take(
        u64::try_from(MAX_RECEIPT_BOOTSTRAP_BYTES + 1)
            .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?,
    )
    .read_to_end(&mut wire)
    .map_err(|_| LocalProcessError::LocalReceiptBootstrap)?;
    if wire.len() != length {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    Ok(wire)
}

fn validate_endpoint_paths(
    socket_path: &Path,
    bootstrap_path: &Path,
    expected_uid: u32,
    expected_gid: u32,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    if socket_path == bootstrap_path
        || socket_path.parent() != bootstrap_path.parent()
        || !is_lexically_absolute_file(socket_path)
        || !is_lexically_absolute_file(bootstrap_path)
    {
        return Err(error);
    }
    validate_private_parent(socket_path, expected_uid, expected_gid, error)
}

fn validate_private_parent(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    let parent = path.parent().ok_or(error)?;
    validate_canonical_path_chain(parent, error)?;
    let before = fs::symlink_metadata(parent).map_err(|_| error)?;
    validate_socket_directory_metadata(&before, expected_uid, expected_gid, error)?;
    let canonical = fs::canonicalize(parent).map_err(|_| error)?;
    let after = fs::symlink_metadata(parent).map_err(|_| error)?;
    validate_socket_directory_metadata(&after, expected_uid, expected_gid, error)?;
    if canonical != parent
        || FileIdentityV1::from_metadata(&before) != FileIdentityV1::from_metadata(&after)
        || before.nlink() != after.nlink()
    {
        return Err(error);
    }
    Ok(())
}

fn validate_canonical_path_chain(
    path: &Path,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    if !path.is_absolute() {
        return Err(error);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                if fs::symlink_metadata(&current)
                    .map_err(|_| error)?
                    .file_type()
                    .is_symlink()
                {
                    return Err(error);
                }
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(error);
            }
        }
    }
    Ok(())
}

fn validate_socket_directory_metadata(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.permissions().mode() & 0o7777 != RECEIPT_SOCKET_DIRECTORY_MODE
        || metadata.nlink() == 0
    {
        return Err(error);
    }
    Ok(())
}

fn validate_bootstrap_metadata(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    if !metadata.file_type().is_file()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.permissions().mode() & 0o7777 != RECEIPT_PRIVATE_FILE_MODE
        || metadata.nlink() != 1
    {
        return Err(error);
    }
    Ok(())
}

fn validate_socket_metadata(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
    error: LocalProcessError,
) -> Result<(), LocalProcessError> {
    if !metadata.file_type().is_socket()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.permissions().mode() & 0o7777 != RECEIPT_PRIVATE_FILE_MODE
        || metadata.nlink() != 1
    {
        return Err(error);
    }
    Ok(())
}

fn validate_socket_identity(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
    error: LocalProcessError,
) -> Result<FileIdentityV1, LocalProcessError> {
    validate_private_parent(path, expected_uid, expected_gid, error)?;
    let before = fs::symlink_metadata(path).map_err(|_| error)?;
    validate_socket_metadata(&before, expected_uid, expected_gid, error)?;
    let identity = FileIdentityV1::from_metadata(&before);
    validate_private_parent(path, expected_uid, expected_gid, error)?;
    let after = fs::symlink_metadata(path).map_err(|_| error)?;
    validate_socket_metadata(&after, expected_uid, expected_gid, error)?;
    if FileIdentityV1::from_metadata(&after) != identity {
        return Err(error);
    }
    Ok(identity)
}

fn validate_socket_path_identity(
    path: &Path,
    expected: FileIdentityV1,
) -> Result<(), LocalProcessError> {
    let observed = validate_socket_identity(
        path,
        Uid::effective().as_raw(),
        Gid::effective().as_raw(),
        LocalProcessError::LocalReceiptBootstrap,
    )?;
    if observed != expected {
        return Err(LocalProcessError::LocalReceiptBootstrap);
    }
    Ok(())
}

fn peer_matches(stream: &UnixStream, expected_uid: u32, expected_gid: u32) -> bool {
    stream.peer_cred().is_ok_and(|credentials| {
        credentials.uid() == expected_uid && credentials.gid() == expected_gid
    })
}

fn accept_client_write_half_shutdown(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotConnected => Ok(()),
        Err(error) => Err(error),
    }
}

fn valid_correlation(correlation: ReceiptCorrelationV1) -> bool {
    !bytes_are_zero(&correlation.runtime_target)
        && !bytes_are_zero(&correlation.runtime_store_instance)
        && !bytes_are_zero(&correlation.runtime_response_key_ref)
        && !bytes_are_zero(&correlation.runtime_response_public_key)
        && !bytes_are_zero(&correlation.expected_request_digest)
        && !bytes_are_zero(&correlation.expected_receipt_digest)
}

fn derive_request_id(seed: &[u8; 16], sequence: u64) -> Result<[u8; 16], ReceiptWireErrorV1> {
    if bytes_are_zero(seed) || sequence != 1 {
        return Err(ReceiptWireErrorV1);
    }
    let mut digest = Sha256::new();
    digest.update(RECEIPT_REQUEST_ID_DOMAIN);
    digest.update(seed);
    digest.update(sequence.to_be_bytes());
    let digest: [u8; 32] = digest.finalize().into();
    let mut request_id = [0_u8; 16];
    request_id.copy_from_slice(&digest[..16]);
    if bytes_are_zero(&request_id) {
        return Err(ReceiptWireErrorV1);
    }
    Ok(request_id)
}

fn digest_parts(domain: &[u8], prefix: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(prefix);
    digest.update(payload);
    digest.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn path_bytes(path: &Path) -> Result<&[u8], ReceiptWireErrorV1> {
    if !is_lexically_absolute_file(path) {
        return Err(ReceiptWireErrorV1);
    }
    let bytes = path.to_str().ok_or(ReceiptWireErrorV1)?.as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(ReceiptWireErrorV1);
    }
    Ok(bytes)
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

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ReceiptWireErrorV1> {
    Ok(u16::from_be_bytes(array_at(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ReceiptWireErrorV1> {
    Ok(u32::from_be_bytes(array_at(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ReceiptWireErrorV1> {
    Ok(u64::from_be_bytes(array_at(bytes, offset)?))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], ReceiptWireErrorV1> {
    bytes
        .get(offset..offset.checked_add(N).ok_or(ReceiptWireErrorV1)?)
        .ok_or(ReceiptWireErrorV1)?
        .try_into()
        .map_err(|_| ReceiptWireErrorV1)
}

fn bytes_are_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use core::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};
    use paraegox_kernel::digest::Digest32;
    use paraegox_kernel::identity::{PrincipalRef, RuntimeHostId};
    use paraegox_runtime_contracts::reference_control::ReferenceChannelBindingV1;

    const FIXTURE_TARGET: [u8; 16] = [0x11; 16];
    const FIXTURE_STORE: [u8; 32] = [0x12; 32];
    const FIXTURE_REQUEST_DIGEST: [u8; 32] = [0x15; 32];
    const FIXTURE_RESPONSE_KEY_REF: [u8; 16] = [0x1f; 16];
    const FIXTURE_RESPONSE_SEED: [u8; 32] = [0x21; 32];

    struct FixtureReceiptV1 {
        wire: Box<[u8]>,
        correlation: ReceiptCorrelationV1,
    }

    fn fixture_receipt() -> FixtureReceiptV1 {
        fixture_receipt_with_outcome(ManagedModelAgentStackTerminalOutcomeV1::ActiveReady as u8)
    }

    fn fixture_receipt_with_outcome(outcome: u8) -> FixtureReceiptV1 {
        let channel = ReferenceChannelBindingV1::try_new(
            RuntimeHostId::from_bytes(FIXTURE_TARGET),
            PrincipalRef::from_bytes([0x1c; 16]),
            Digest32::from_bytes([0x1d; 32]),
            Digest32::from_bytes([0x1e; 32]),
        )
        .expect("fixture channel");
        let mut fields = fixture_terminal_fields(b"PXMT", 1, channel);
        let mut transcript = fixture_terminal_fields(
            b"ParaEGOX\0managed-model-agent-stack-terminal-signing",
            1,
            channel,
        );
        fields[199] = outcome;
        transcript[246] = outcome;
        let key = SigningKey::from_bytes(&FIXTURE_RESPONSE_SEED);
        let signature = key.sign(&transcript).to_bytes();
        let mut wire = fields;
        wire.extend_from_slice(&64_u16.to_be_bytes());
        wire.extend_from_slice(&signature);
        let receipt =
            ManagedModelAgentStackTerminalReceiptV1::decode(&wire).expect("canonical fixture PXMT");
        let correlation = ReceiptCorrelationV1 {
            runtime_target: FIXTURE_TARGET,
            runtime_store_instance: FIXTURE_STORE,
            runtime_response_key_ref: FIXTURE_RESPONSE_KEY_REF,
            runtime_response_public_key: key.verifying_key().to_bytes(),
            expected_request_digest: FIXTURE_REQUEST_DIGEST,
            expected_receipt_digest: receipt.receipt_digest().into_bytes(),
        };
        FixtureReceiptV1 {
            wire: wire.into_boxed_slice(),
            correlation,
        }
    }

    fn fixture_terminal_fields(
        magic: &[u8],
        version: u16,
        channel: ReferenceChannelBindingV1,
    ) -> Vec<u8> {
        let mut wire = Vec::new();
        wire.extend_from_slice(magic);
        wire.extend_from_slice(&version.to_be_bytes());
        wire.extend_from_slice(&FIXTURE_TARGET);
        wire.extend_from_slice(&FIXTURE_STORE);
        wire.extend_from_slice(&[0x13; 16]);
        wire.extend_from_slice(&[0x14; 16]);
        wire.extend_from_slice(&FIXTURE_REQUEST_DIGEST);
        wire.extend_from_slice(&[0x16; 32]);
        wire.extend_from_slice(&[0x17; 32]);
        wire.extend_from_slice(&[0x18; 16]);
        wire.extend_from_slice(&[1, 1, 2, 3, 1]);
        wire.extend_from_slice(&[0x19; 32]);
        for generation in [7_u64, 8, 9] {
            wire.push(1);
            wire.extend_from_slice(&generation.to_be_bytes());
        }
        wire.extend_from_slice(&2_u16.to_be_bytes());
        wire.push(0x3f);
        wire.extend_from_slice(&[0x1a; 32]);
        wire.extend_from_slice(&[0x1b; 32]);
        wire.extend_from_slice(&9_u64.to_be_bytes());
        wire.extend_from_slice(&10_u64.to_be_bytes());
        wire.extend_from_slice(&11_u64.to_be_bytes());
        wire.extend_from_slice(&12_u64.to_be_bytes());
        wire.extend_from_slice(channel.target().as_bytes());
        wire.extend_from_slice(channel.runtime_peer().as_bytes());
        wire.extend_from_slice(channel.local_endpoint_identity_digest().as_bytes());
        wire.extend_from_slice(channel.peer_credentials_digest().as_bytes());
        wire.extend_from_slice(channel.runtime_peer().as_bytes());
        wire.extend_from_slice(channel.binding_digest().as_bytes());
        wire.extend_from_slice(&FIXTURE_RESPONSE_KEY_REF);
        wire.extend_from_slice(&1_u16.to_be_bytes());
        wire.extend_from_slice(&1_u16.to_be_bytes());
        wire
    }

    fn fixture_bootstrap(receipt: &FixtureReceiptV1) -> LocalReceiptBootstrapV1 {
        LocalReceiptBootstrapV1 {
            generation: [0x41; 16],
            config_commitment: [0x42; 32],
            generation_token: Zeroizing::new([0x43; 32]),
            server_uid: 501,
            server_gid: 20,
            request_id_seed: Zeroizing::new([0x44; 16]),
            correlation: receipt.correlation,
            socket_path: PathBuf::from("/private/run/receipt.sock"),
        }
    }

    fn fixture_request(bootstrap: &LocalReceiptBootstrapV1) -> ReceiptLatestRequestV1 {
        ReceiptLatestRequestV1 {
            request_id: bootstrap.request_id().expect("sequence-one request id"),
            generation: bootstrap.generation,
            config_commitment: bootstrap.config_commitment,
            expected_request_digest: bootstrap.correlation.expected_request_digest,
            expected_receipt_digest: bootstrap.correlation.expected_receipt_digest,
        }
    }

    fn fixture_request_transport(
        bootstrap: &LocalReceiptBootstrapV1,
        request: ReceiptLatestRequestV1,
    ) -> Vec<u8> {
        let mut transport = bootstrap.generation_token.as_ref().to_vec();
        transport.extend_from_slice(&request.encode().expect("canonical PXRQ"));
        assert_eq!(transport.len(), RECEIPT_REQUEST_TRANSPORT_BYTES);
        transport
    }

    fn assert_receipt_verification_rejects(wire: &[u8], correlation: ReceiptCorrelationV1) {
        assert_eq!(
            verify_activation_receipt(wire, correlation),
            Err(LocalProcessError::LifecycleStartup)
        );
        assert!(matches!(
            verify_client_receipt(wire, correlation),
            Err(LocalProcessError::LocalReceiptProtocol)
        ));
    }

    async fn exchange_once(
        server: Arc<ReceiptServerV1>,
        transport: &[u8],
    ) -> (Result<(), ReceiptWireErrorV1>, Vec<u8>) {
        let (mut client, server_stream) = UnixStream::pair().expect("receipt transport pair");
        let task = tokio::spawn(serve_receipt_exchange(server_stream, server));
        client
            .write_all(transport)
            .await
            .expect("write request transport");
        accept_client_write_half_shutdown(client.shutdown().await)
            .expect("close client write half");
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("read response to exact EOF");
        (
            task.await.expect("receipt exchange task must not panic"),
            response,
        )
    }

    fn decode_response_transport(transport: &[u8]) -> ReceiptLatestResponseV1 {
        let declared = usize::try_from(u32::from_be_bytes(
            transport[..4].try_into().expect("outer PXRO length"),
        ))
        .expect("bounded PXRO length");
        assert_eq!(declared, transport.len() - 4);
        ReceiptLatestResponseV1::decode_frame(&transport[4..]).expect("canonical PXRO")
    }

    fn decode_lower_hex_fixture(value: &str) -> Vec<u8> {
        let value = value
            .strip_suffix('\n')
            .expect("lowercase hexadecimal fixture ends with LF");
        assert!(!value.is_empty());
        assert_eq!(value.len() % 2, 0);
        assert!(!value.contains('\r'));
        assert!(!value.contains('\n'));
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let nibble = |value: u8| match value {
                    b'0'..=b'9' => value - b'0',
                    b'a'..=b'f' => value - b'a' + 10,
                    _ => panic!("fixture must be lowercase hexadecimal"),
                };
                (nibble(pair[0]) << 4) | nibble(pair[1])
            })
            .collect()
    }

    #[test]
    fn activation_and_client_independently_verify_the_same_active_ready_receipt() {
        let fixture = fixture_receipt();
        verify_activation_receipt(&fixture.wire, fixture.correlation)
            .expect("adapter PXMT verification");
        let snapshot = verify_client_receipt(&fixture.wire, fixture.correlation)
            .expect("client PXMT verification");
        assert_eq!(snapshot.receipt_version, 1);
        assert_eq!(snapshot.request_mode, "fabric_model_and_agent");
        assert_eq!(snapshot.terminal_outcome, "active_ready");
        assert_eq!(snapshot.fabric_generation, "7");
        assert!(!snapshot.current_health_checked);

        let mut wrong_key = fixture.correlation;
        wrong_key.runtime_response_public_key[0] ^= 1;
        assert_eq!(
            verify_activation_receipt(&fixture.wire, wrong_key),
            Err(LocalProcessError::LifecycleStartup)
        );
        assert!(matches!(
            verify_client_receipt(&fixture.wire, wrong_key),
            Err(LocalProcessError::LocalReceiptProtocol)
        ));
    }

    #[test]
    fn adapter_and_client_reject_every_runtime_correlation_and_signature_tamper() {
        let fixture = fixture_receipt();
        let mut correlations = Vec::new();

        let mut wrong_request = fixture.correlation;
        wrong_request.expected_request_digest[0] ^= 1;
        correlations.push(wrong_request);

        let mut wrong_receipt = fixture.correlation;
        wrong_receipt.expected_receipt_digest[0] ^= 1;
        correlations.push(wrong_receipt);

        let mut wrong_target = fixture.correlation;
        wrong_target.runtime_target[0] ^= 1;
        correlations.push(wrong_target);

        let mut wrong_store = fixture.correlation;
        wrong_store.runtime_store_instance[0] ^= 1;
        correlations.push(wrong_store);

        let mut wrong_key_ref = fixture.correlation;
        wrong_key_ref.runtime_response_key_ref[0] ^= 1;
        correlations.push(wrong_key_ref);

        let mut wrong_public_key = fixture.correlation;
        wrong_public_key.runtime_response_public_key[0] ^= 1;
        correlations.push(wrong_public_key);

        for correlation in correlations {
            assert_receipt_verification_rejects(&fixture.wire, correlation);
        }

        let mut wrong_signature = fixture.wire.to_vec();
        *wrong_signature.last_mut().expect("fixture signature") ^= 1;
        let decoded = ManagedModelAgentStackTerminalReceiptV1::decode(&wrong_signature)
            .expect("signature bytes remain canonical PXMT framing");
        let mut signature_correlation = fixture.correlation;
        signature_correlation.expected_receipt_digest = decoded.receipt_digest().into_bytes();
        assert_receipt_verification_rejects(&wrong_signature, signature_correlation);

        let mut trailing = fixture.wire.to_vec();
        trailing.push(0);
        assert_receipt_verification_rejects(&trailing, fixture.correlation);
    }

    #[test]
    fn valid_signed_non_active_ready_terminal_is_not_a_receipt_snapshot() {
        let fixture =
            fixture_receipt_with_outcome(ManagedModelAgentStackTerminalOutcomeV1::Uncertain as u8);
        let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(&fixture.wire)
            .expect("signed Uncertain PXMT is valid to the domain decoder");
        assert_eq!(
            receipt.facts().state().outcome(),
            ManagedModelAgentStackTerminalOutcomeV1::Uncertain
        );
        assert_receipt_verification_rejects(&fixture.wire, fixture.correlation);
    }

    #[test]
    fn public_snapshot_json_is_the_exact_31_field_allowlist() {
        let fixture = fixture_receipt();
        let snapshot = verify_client_receipt(&fixture.wire, fixture.correlation)
            .expect("verified public snapshot");
        let json = serde_json::to_string(&snapshot).expect("compact ordered snapshot JSON");
        assert_eq!(
            json,
            concat!(
                "{\"snapshot_version\":1,",
                "\"query_scope\":\"current_running_generation\",",
                "\"source_owner\":\"runtime_host\",",
                "\"record_kind\":\"managed_model_agent_stack_terminal_receipt\",",
                "\"receipt_version\":1,",
                "\"request_digest\":\"1515151515151515151515151515151515151515151515151515151515151515\",",
                "\"receipt_digest\":\"2206e2e2010cf3e85a4d697fd7d64acb7d9403f54e0f59bac9a55599f3ba3ae6\",",
                "\"request_mode\":\"fabric_model_and_agent\",",
                "\"terminal_outcome\":\"active_ready\",",
                "\"lifecycle_effect\":\"may_have_started\",",
                "\"desired_head\":\"committed_incoming\",",
                "\"desired_head_digest\":\"1919191919191919191919191919191919191919191919191919191919191919\",",
                "\"fabric_generation\":\"7\",",
                "\"model_generation\":\"8\",",
                "\"agent_generation\":\"9\",",
                "\"physical_binding_census\":2,",
                "\"census_complete\":true,",
                "\"fabric_ready\":true,",
                "\"model_ready\":true,",
                "\"agent_ready\":true,",
                "\"fabric_to_agent_dependency_ready\":true,",
                "\"model_to_agent_dependency_ready\":true,",
                "\"exact_zero\":false,",
                "\"quarantined\":false,",
                "\"resource_census_digest\":\"1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a\",",
                "\"raw_outcome_digest\":\"1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b\",",
                "\"completion_runtime_host_epoch\":\"9\",",
                "\"completion_snapshot_sequence\":\"10\",",
                "\"selection_clock_generation\":\"11\",",
                "\"selection_observed_at_nanos\":\"12\",",
                "\"current_health_checked\":false}"
            )
        );
    }

    #[test]
    fn request_id_is_exactly_sequence_one_and_has_no_successor_budget() {
        let seed = [0x44; 16];
        assert_eq!(
            derive_request_id(&seed, 1).expect("sequence one request id"),
            [
                0xb5, 0xbd, 0xff, 0x5d, 0xe7, 0x9a, 0xe2, 0x58, 0x7d, 0xff, 0x0c, 0xc7, 0x61, 0xb7,
                0xde, 0xb0,
            ]
        );
        assert_eq!(derive_request_id(&seed, 0), Err(ReceiptWireErrorV1));
        assert_eq!(derive_request_id(&seed, 2), Err(ReceiptWireErrorV1));
        assert_eq!(derive_request_id(&[0; 16], 1), Err(ReceiptWireErrorV1));
    }

    #[test]
    fn bootstrap_request_and_both_response_transports_match_shared_goldens() {
        let fixture = fixture_receipt();
        let bootstrap = fixture_bootstrap(&fixture);
        let bootstrap_wire = bootstrap.encode().expect("PXRB");
        assert_eq!(
            bootstrap_wire,
            decode_lower_hex_fixture(include_str!(
                "../../../tests/fixtures/wire/m4a_receipt_bootstrap_v1.hex"
            ))
        );
        let request = fixture_request(&bootstrap);
        let request_transport = fixture_request_transport(&bootstrap, request);
        assert_eq!(
            request_transport,
            decode_lower_hex_fixture(include_str!(
                "../../../tests/fixtures/wire/m4a_receipt_latest_request_transport_v1.hex"
            ))
        );
        let ready = ReceiptLatestResponseV1::ready(request, Arc::from(fixture.wire))
            .expect("PXRO Ready")
            .encode_transport()
            .expect("PXRO Ready transport");
        assert_eq!(
            ready,
            decode_lower_hex_fixture(include_str!(
                "../../../tests/fixtures/wire/m4a_receipt_latest_ready_response_transport_v1.hex"
            ))
        );
        let not_found = ReceiptLatestResponseV1::not_found(request)
            .encode_transport()
            .expect("PXRO NotFound transport");
        assert_eq!(
            not_found,
            decode_lower_hex_fixture(include_str!(
                "../../../tests/fixtures/wire/m4a_receipt_latest_not_found_response_transport_v1.hex"
            ))
        );
    }

    #[test]
    fn server_returns_ready_then_retiring_not_found_and_silently_closes_bad_requests() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("receipt exchange runtime");
        runtime.block_on(async {
            let fixture = fixture_receipt();
            let bootstrap = fixture_bootstrap(&fixture);
            let request = fixture_request(&bootstrap);
            let canonical_transport = fixture_request_transport(&bootstrap, request);
            let server = Arc::new(ReceiptServerV1 {
                bootstrap,
                slot: Arc::new(Mutex::new(ReceiptSlotV1 {
                    active_payload: Some(Arc::from(fixture.wire)),
                })),
            });

            let (ready_result, ready_transport) =
                exchange_once(Arc::clone(&server), &canonical_transport).await;
            assert_eq!(ready_result, Ok(()));
            let ready = decode_response_transport(&ready_transport);
            assert_eq!(ready.request, request);
            assert_eq!(ready.outcome, ReceiptResponseOutcomeV1::Ready);
            assert!(!ready.payload.is_empty());

            server
                .slot
                .lock()
                .expect("retire receipt slot")
                .active_payload = None;
            let (not_found_result, not_found_transport) =
                exchange_once(Arc::clone(&server), &canonical_transport).await;
            assert_eq!(not_found_result, Ok(()));
            let not_found = decode_response_transport(&not_found_transport);
            assert_eq!(not_found.request, request);
            assert_eq!(not_found.outcome, ReceiptResponseOutcomeV1::NotFound);
            assert!(not_found.payload.is_empty());

            let mut authenticated_mismatches = Vec::new();
            let mut wrong_request_id = request;
            wrong_request_id.request_id[0] ^= 1;
            authenticated_mismatches.push(wrong_request_id);
            let mut wrong_generation = request;
            wrong_generation.generation[0] ^= 1;
            authenticated_mismatches.push(wrong_generation);
            let mut wrong_config = request;
            wrong_config.config_commitment[0] ^= 1;
            authenticated_mismatches.push(wrong_config);
            let mut wrong_request_digest = request;
            wrong_request_digest.expected_request_digest[0] ^= 1;
            authenticated_mismatches.push(wrong_request_digest);
            let mut wrong_receipt_digest = request;
            wrong_receipt_digest.expected_receipt_digest[0] ^= 1;
            authenticated_mismatches.push(wrong_receipt_digest);
            for mismatch in authenticated_mismatches {
                let transport = fixture_request_transport(&server.bootstrap, mismatch);
                let (result, response) = exchange_once(Arc::clone(&server), &transport).await;
                assert!(result.is_err());
                assert!(response.is_empty(), "correlation mismatch is not an oracle");
            }

            let mut wrong_token = canonical_transport.clone();
            wrong_token[0] ^= 1;
            let (result, response) = exchange_once(Arc::clone(&server), &wrong_token).await;
            assert!(result.is_err());
            assert!(response.is_empty(), "wrong token is not an oracle");

            let mut malformed = canonical_transport.clone();
            malformed[32 + 7] = 1;
            let (result, response) = exchange_once(Arc::clone(&server), &malformed).await;
            assert!(result.is_err());
            assert!(response.is_empty(), "malformed request is not an oracle");

            let mut trailing = canonical_transport.clone();
            trailing.push(0);
            let (result, response) = exchange_once(Arc::clone(&server), &trailing).await;
            assert!(result.is_err());
            assert!(response.is_empty(), "trailing request is not an oracle");

            let (result, response) = exchange_once(
                Arc::clone(&server),
                &canonical_transport[..RECEIPT_REQUEST_TRANSPORT_BYTES - 1],
            )
            .await;
            assert!(result.is_err());
            assert!(response.is_empty(), "partial request is not an oracle");
        });
    }

    #[test]
    fn endpoint_and_client_source_lock_single_deadline_capacity_and_zero_retry() {
        assert_eq!(RECEIPT_OPERATION_TIMEOUT, Duration::from_secs(5));
        assert_eq!(RECEIPT_OPERATION_TIMEOUT_NANOS, 5_000_000_000);
        assert_eq!(RECEIPT_MAX_IN_FLIGHT, 8);
        assert_eq!(RECEIPT_REQUEST_TRANSPORT_BYTES, 208);

        let source = include_str!("receipt_snapshot.rs");
        let tests_start = source
            .rfind("\n#[cfg(test)]\nmod tests {")
            .expect("receipt test module");
        let production = &source[..tests_start];
        let endpoint_start = production
            .find("async fn serve_receipt_endpoint(")
            .expect("bounded endpoint");
        let exchange_start = production
            .find("async fn serve_receipt_exchange(")
            .expect("one-shot server exchange");
        let client_start = production
            .find("async fn exchange_latest_receipt(")
            .expect("one-shot typed client");
        let activation_start = production
            .find("fn verify_activation_receipt(")
            .expect("client function boundary");
        let endpoint = &production[endpoint_start..exchange_start];
        let server = &production[exchange_start..client_start];
        let client = &production[client_start..activation_start];

        assert!(endpoint.contains("Semaphore::new(RECEIPT_MAX_IN_FLIGHT)"));
        assert!(endpoint.contains("try_acquire_owned()"));
        assert!(endpoint.contains("peer_matches("));
        assert!(endpoint.contains("drop(stream)"));
        assert_eq!(server.matches("let deadline = Instant::now()").count(), 1);
        assert_eq!(client.matches("let deadline = Instant::now()").count(), 1);
        assert_eq!(client.matches("UnixStream::connect(").count(), 1);
        assert_eq!(client.matches("stream.write_all(").count(), 1);
        assert!(!client.contains("loop {"));
        assert!(!client.contains("retry"));
        assert!(!client.contains("reconnect"));
        assert!(!client.contains("fallback"));
    }

    #[test]
    fn codecs_reject_reserved_trailing_and_cross_domain_tampering() {
        let fixture = fixture_receipt();
        let bootstrap = fixture_bootstrap(&fixture);
        let mut bootstrap_wire = bootstrap.encode().expect("PXRB");
        bootstrap_wire[288] ^= 1;
        assert!(LocalReceiptBootstrapV1::decode(&bootstrap_wire).is_err());

        let request = ReceiptLatestRequestV1 {
            request_id: bootstrap.request_id().expect("request id"),
            generation: bootstrap.generation,
            config_commitment: bootstrap.config_commitment,
            expected_request_digest: bootstrap.correlation.expected_request_digest,
            expected_receipt_digest: bootstrap.correlation.expected_receipt_digest,
        };
        let mut request_wire = request.encode().expect("PXRQ").to_vec();
        request_wire[7] = 1;
        assert!(ReceiptLatestRequestV1::decode(&request_wire).is_err());

        let mut response = ReceiptLatestResponseV1::not_found(request)
            .encode_frame()
            .expect("PXRO N");
        response.push(0);
        assert!(ReceiptLatestResponseV1::decode_frame(&response).is_err());
    }

    #[test]
    fn dropping_the_opaque_lease_retires_and_clears_owner_bytes() {
        let slot = Arc::new(Mutex::new(ReceiptSlotV1 {
            active_payload: Some(Arc::from(&b"PXMT"[..])),
        }));
        let lease = LocalReceiptRetirementHandleV1 {
            slot: Arc::clone(&slot),
            retired: false,
        };
        assert!(slot.lock().expect("active slot").active_payload.is_some());
        drop(lease);
        assert!(slot.lock().expect("retired slot").active_payload.is_none());
    }

    #[test]
    fn sensitive_bootstrap_debug_is_redacted() {
        let fixture = fixture_receipt();
        let debug = format!("{:?}", fixture_bootstrap(&fixture));
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("43434343"));
        assert!(!debug.contains("44444444"));
        assert!(!debug.contains("receipt.sock"));

        let source = include_str!("receipt_snapshot.rs");
        let production = source
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .expect("Receipt production source");
        assert!(production.contains("let canonical = Zeroizing::new(decoded.encode()?);"));
    }
}
