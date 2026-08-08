#![cfg(unix)]

//! Runtime-owned durable evidence for the latest authenticated PXAG Describe.
//!
//! This is a single-slot owner-private ledger, not an access grant or a PXRA
//! dispatcher.  Each record retains the exact Controller-signed PXAG and the
//! exact Runtime-signed PXAH.  Its sequence and previous-record digest make a
//! replacement explicit, while the trailing record digest is the canonical
//! checksum for the complete slot.  A consumer must still reverify both
//! signatures and compare the record with independently observed live PXST,
//! PXAP, generation, store, target, and RuntimeHost-epoch facts.

use core::fmt;

use paraegox_kernel::{
    digest::{Digest32, Digest32Builder, DigestBuildError},
    identity::{PrincipalRef, RuntimeHostId},
};
use paraegox_runtime_contracts::{
    distributed_agent_stack_plan::RestrictedRuntimeApplyCarrierBindingV1,
    managed_service::ManagedServiceGeneration,
    managed_serving_bootstrap::{
        ControllerAuthenticatedRuntimeAgentControlRequestV1,
        MAX_RUNTIME_AGENT_CONTROL_RECEIPT_BYTES, MAX_RUNTIME_AGENT_CONTROL_REQUEST_BYTES,
        ManagedServingBootstrapError, RuntimeAgentControlKindV1, RuntimeAgentControlReceiptV1,
        RuntimeAgentControlRequestV1, RuntimeAuthenticatedAgentControlReceiptV1,
        runtime_agent_control_descriptor_payload_digest_v1,
    },
    wire::ApplyAuthKeyRef,
};

const DESCRIPTOR_EVIDENCE_MAGIC: &[u8; 4] = b"PXDE";
const DESCRIPTOR_EVIDENCE_VERSION: u16 = 1;
const DESCRIPTOR_EVIDENCE_HAS_PREVIOUS: u16 = 1;
const DESCRIPTOR_EVIDENCE_HEADER_BYTES: usize = 256;
const DESCRIPTOR_EVIDENCE_CHECKSUM_BYTES: usize = 32;
const DESCRIPTOR_EVIDENCE_RECORD_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-descriptor-evidence-record.sha256.v1";

pub(crate) const MAX_REMOTE_AGENT_DESCRIPTOR_EVIDENCE_BYTES: usize =
    DESCRIPTOR_EVIDENCE_HEADER_BYTES
        + MAX_RUNTIME_AGENT_CONTROL_REQUEST_BYTES
        + MAX_RUNTIME_AGENT_CONTROL_RECEIPT_BYTES
        + DESCRIPTOR_EVIDENCE_CHECKSUM_BYTES;

/// Strict latest-slot record for one authenticated Describe exchange.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentDescriptorEvidenceV1 {
    record_sequence: u64,
    previous_record_digest: Option<Digest32>,
    target: RuntimeHostId,
    runtime_store_instance_id: [u8; 32],
    runtime_host_epoch: u64,
    fabric_generation: ManagedServiceGeneration,
    agent_generation: ManagedServiceGeneration,
    active_pxst_digest: Digest32,
    descriptor_payload_digest: Digest32,
    receipt_digest: Digest32,
    request_digest: Digest32,
    request: RuntimeAgentControlRequestV1,
    receipt: RuntimeAgentControlReceiptV1,
    canonical_wire: Box<[u8]>,
    record_digest: Digest32,
}

impl RemoteAgentDescriptorEvidenceV1 {
    /// Builds the next slot only from already authenticated PXAG/PXAH markers.
    pub(crate) fn try_next(
        previous: Option<&Self>,
        authenticated_request: ControllerAuthenticatedRuntimeAgentControlRequestV1<'_>,
        authenticated_receipt: RuntimeAuthenticatedAgentControlReceiptV1<'_>,
    ) -> Result<Self, RemoteAgentDescriptorEvidenceError> {
        let request = authenticated_request.request();
        let receipt = authenticated_receipt.receipt();
        receipt
            .validate_descriptor_against_request(request)
            .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
        if request.kind() != RuntimeAgentControlKindV1::DescribeConversationPort {
            return Err(RemoteAgentDescriptorEvidenceError::NotDescribe);
        }
        let descriptor = receipt
            .conversation_port_descriptor()
            .ok_or(RemoteAgentDescriptorEvidenceError::NotDescribe)?;
        let descriptor_payload_digest =
            runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
        if descriptor_payload_digest != receipt.payload_wire_digest() {
            return Err(RemoteAgentDescriptorEvidenceError::CorrelationMismatch);
        }
        let fabric_generation = receipt
            .fabric_generation()
            .ok_or(RemoteAgentDescriptorEvidenceError::CorrelationMismatch)?;
        let agent_generation = receipt
            .agent_generation()
            .ok_or(RemoteAgentDescriptorEvidenceError::CorrelationMismatch)?;
        let (record_sequence, previous_record_digest) = match previous {
            Some(previous) => (
                previous
                    .record_sequence
                    .checked_add(1)
                    .ok_or(RemoteAgentDescriptorEvidenceError::SequenceExhausted)?,
                Some(previous.record_digest),
            ),
            None => (1, None),
        };
        let mut record = Self {
            record_sequence,
            previous_record_digest,
            target: request.target(),
            runtime_store_instance_id: request.expected_runtime_store_instance_id(),
            runtime_host_epoch: request.expected_runtime_host_epoch(),
            fabric_generation,
            agent_generation,
            active_pxst_digest: request.expected_active_pxst_digest(),
            descriptor_payload_digest,
            receipt_digest: receipt.receipt_digest(),
            request_digest: request.request_digest(),
            request: request.clone(),
            receipt: receipt.clone(),
            canonical_wire: Vec::new().into_boxed_slice(),
            record_digest: Digest32::from_bytes([0; 32]),
        };
        let (canonical_wire, record_digest) = build_record_wire(&record)?;
        record.canonical_wire = canonical_wire;
        record.record_digest = record_digest;
        Ok(record)
    }

    /// Decodes only the one canonical bounded PXDE v1 representation.
    pub(crate) fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDescriptorEvidenceError> {
        if frame.len() > MAX_REMOTE_AGENT_DESCRIPTOR_EVIDENCE_BYTES {
            return Err(RemoteAgentDescriptorEvidenceError::FrameTooLarge);
        }
        if frame.len() < DESCRIPTOR_EVIDENCE_HEADER_BYTES + DESCRIPTOR_EVIDENCE_CHECKSUM_BYTES {
            return Err(RemoteAgentDescriptorEvidenceError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *DESCRIPTOR_EVIDENCE_MAGIC
            || cursor.u16()? != DESCRIPTOR_EVIDENCE_VERSION
        {
            return Err(RemoteAgentDescriptorEvidenceError::UnsupportedWire);
        }
        let flags = cursor.u16()?;
        if flags & !DESCRIPTOR_EVIDENCE_HAS_PREVIOUS != 0 {
            return Err(RemoteAgentDescriptorEvidenceError::NonCanonical);
        }
        let record_sequence = cursor.u64()?;
        let request_length = cursor.usize_u32()?;
        let receipt_length = cursor.usize_u32()?;
        if request_length == 0
            || request_length > MAX_RUNTIME_AGENT_CONTROL_REQUEST_BYTES
            || receipt_length == 0
            || receipt_length > MAX_RUNTIME_AGENT_CONTROL_RECEIPT_BYTES
            || DESCRIPTOR_EVIDENCE_HEADER_BYTES
                .checked_add(request_length)
                .and_then(|length| length.checked_add(receipt_length))
                .and_then(|length| length.checked_add(DESCRIPTOR_EVIDENCE_CHECKSUM_BYTES))
                != Some(frame.len())
        {
            return Err(RemoteAgentDescriptorEvidenceError::InvalidLength);
        }
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let runtime_store_instance_id = cursor.array()?;
        let runtime_host_epoch = cursor.u64()?;
        let fabric_generation = ManagedServiceGeneration::try_new(cursor.u64()?)
            .map_err(|_| RemoteAgentDescriptorEvidenceError::InvalidGeneration)?;
        let agent_generation = ManagedServiceGeneration::try_new(cursor.u64()?)
            .map_err(|_| RemoteAgentDescriptorEvidenceError::InvalidGeneration)?;
        let active_pxst_digest = Digest32::from_bytes(cursor.array()?);
        let descriptor_payload_digest = Digest32::from_bytes(cursor.array()?);
        let receipt_digest = Digest32::from_bytes(cursor.array()?);
        let request_digest = Digest32::from_bytes(cursor.array()?);
        let encoded_previous_digest = Digest32::from_bytes(cursor.array()?);
        let previous_record_digest = match flags {
            0 if record_sequence == 1 && digest_is_zero(encoded_previous_digest) => None,
            DESCRIPTOR_EVIDENCE_HAS_PREVIOUS
                if record_sequence > 1 && !digest_is_zero(encoded_previous_digest) =>
            {
                Some(encoded_previous_digest)
            }
            _ => return Err(RemoteAgentDescriptorEvidenceError::InvalidSequence),
        };
        let request = RuntimeAgentControlRequestV1::decode(cursor.take(request_length)?)
            .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
        let receipt = RuntimeAgentControlReceiptV1::decode(cursor.take(receipt_length)?)
            .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
        let encoded_record_digest = Digest32::from_bytes(cursor.array()?);
        cursor.finish()?;
        receipt
            .validate_descriptor_against_request(&request)
            .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
        let descriptor = receipt
            .conversation_port_descriptor()
            .ok_or(RemoteAgentDescriptorEvidenceError::NotDescribe)?;
        let derived_descriptor_digest =
            runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
        if request.kind() != RuntimeAgentControlKindV1::DescribeConversationPort
            || target != request.target()
            || runtime_store_instance_id != request.expected_runtime_store_instance_id()
            || runtime_host_epoch != request.expected_runtime_host_epoch()
            || fabric_generation
                != receipt
                    .fabric_generation()
                    .ok_or(RemoteAgentDescriptorEvidenceError::CorrelationMismatch)?
            || agent_generation
                != receipt
                    .agent_generation()
                    .ok_or(RemoteAgentDescriptorEvidenceError::CorrelationMismatch)?
            || active_pxst_digest != request.expected_active_pxst_digest()
            || descriptor_payload_digest != derived_descriptor_digest
            || descriptor_payload_digest != receipt.payload_wire_digest()
            || receipt_digest != receipt.receipt_digest()
            || request_digest != request.request_digest()
        {
            return Err(RemoteAgentDescriptorEvidenceError::CorrelationMismatch);
        }
        let mut record = Self {
            record_sequence,
            previous_record_digest,
            target,
            runtime_store_instance_id,
            runtime_host_epoch,
            fabric_generation,
            agent_generation,
            active_pxst_digest,
            descriptor_payload_digest,
            receipt_digest,
            request_digest,
            request,
            receipt,
            canonical_wire: frame.into(),
            record_digest: encoded_record_digest,
        };
        let (canonical_wire, record_digest) = build_record_wire(&record)?;
        if canonical_wire.as_ref() != frame || record_digest != encoded_record_digest {
            return Err(RemoteAgentDescriptorEvidenceError::ChecksumMismatch);
        }
        record.canonical_wire = canonical_wire;
        record.record_digest = record_digest;
        Ok(record)
    }

    #[must_use]
    pub(crate) const fn record_sequence(&self) -> u64 {
        self.record_sequence
    }

    #[must_use]
    pub(crate) const fn previous_record_digest(&self) -> Option<Digest32> {
        self.previous_record_digest
    }

    #[must_use]
    pub(crate) const fn target(&self) -> RuntimeHostId {
        self.target
    }

    #[must_use]
    pub(crate) const fn runtime_store_instance_id(&self) -> [u8; 32] {
        self.runtime_store_instance_id
    }

    #[must_use]
    pub(crate) const fn runtime_host_epoch(&self) -> u64 {
        self.runtime_host_epoch
    }

    #[must_use]
    pub(crate) const fn active_pxst_digest(&self) -> Digest32 {
        self.active_pxst_digest
    }

    #[must_use]
    pub(crate) const fn receipt_digest(&self) -> Digest32 {
        self.receipt_digest
    }

    #[must_use]
    pub(crate) const fn request(&self) -> &RuntimeAgentControlRequestV1 {
        &self.request
    }

    #[must_use]
    pub(crate) fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub(crate) const fn record_digest(&self) -> Digest32 {
        self.record_digest
    }
}

/// Non-cloneable marker for one slot reverified against current live facts.
pub(crate) struct RemoteAgentVerifiedDescriptorEvidenceV1<'a> {
    evidence: &'a RemoteAgentDescriptorEvidenceV1,
}

/// Independently observed current owner facts required for revalidation.
pub(crate) struct RemoteAgentDescriptorLiveFactsV1<'a> {
    pub(crate) carrier: &'a RestrictedRuntimeApplyCarrierBindingV1,
    pub(crate) target: RuntimeHostId,
    pub(crate) store_instance_id: [u8; 32],
    pub(crate) runtime_host_epoch: u64,
    pub(crate) active_pxst_digest: Digest32,
    pub(crate) descriptor: &'a [u8],
    pub(crate) fabric_generation: ManagedServiceGeneration,
    pub(crate) agent_generation: ManagedServiceGeneration,
}

impl<'a> RemoteAgentVerifiedDescriptorEvidenceV1<'a> {
    #[must_use]
    pub(crate) const fn evidence(&self) -> &'a RemoteAgentDescriptorEvidenceV1 {
        self.evidence
    }
}

/// Rechecks both retained signatures and every caller-supplied live fence.
///
/// The caller must derive `live_*` from the current Runtime owners. Passing
/// values read back from this record would be self-pinning and is not valid
/// evidence for a later PXRA access decision.
pub(crate) fn verify_remote_agent_descriptor_evidence_v1<'a, VerifyController, VerifyRuntime>(
    evidence: &'a RemoteAgentDescriptorEvidenceV1,
    live: RemoteAgentDescriptorLiveFactsV1<'_>,
    verify_controller: VerifyController,
    verify_runtime: VerifyRuntime,
) -> Result<RemoteAgentVerifiedDescriptorEvidenceV1<'a>, RemoteAgentDescriptorEvidenceError>
where
    VerifyController: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    VerifyRuntime: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
{
    let authenticated_request = evidence
        .request
        .verify_controller_request(live.carrier, verify_controller)
        .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
    evidence
        .receipt
        .verify_runtime_descriptor_receipt(
            authenticated_request.request(),
            live.carrier,
            verify_runtime,
        )
        .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
    let live_descriptor_payload_digest =
        runtime_agent_control_descriptor_payload_digest_v1(live.descriptor)
            .map_err(RemoteAgentDescriptorEvidenceError::Contract)?;
    if live.runtime_host_epoch == 0
        || evidence.target != live.target
        || evidence.runtime_store_instance_id != live.store_instance_id
        || evidence.runtime_host_epoch != live.runtime_host_epoch
        || evidence.active_pxst_digest != live.active_pxst_digest
        || evidence.descriptor_payload_digest != live_descriptor_payload_digest
        || evidence.receipt.conversation_port_descriptor() != Some(live.descriptor)
        || evidence.fabric_generation != live.fabric_generation
        || evidence.agent_generation != live.agent_generation
    {
        return Err(RemoteAgentDescriptorEvidenceError::LiveStateMismatch);
    }
    Ok(RemoteAgentVerifiedDescriptorEvidenceV1 { evidence })
}

fn build_record_wire(
    record: &RemoteAgentDescriptorEvidenceV1,
) -> Result<(Box<[u8]>, Digest32), RemoteAgentDescriptorEvidenceError> {
    let request_length = u32::try_from(record.request.canonical_wire().len())
        .map_err(|_| RemoteAgentDescriptorEvidenceError::FrameTooLarge)?;
    let receipt_length = u32::try_from(record.receipt.canonical_wire().len())
        .map_err(|_| RemoteAgentDescriptorEvidenceError::FrameTooLarge)?;
    let (flags, previous_digest) = match (record.record_sequence, record.previous_record_digest) {
        (1, None) => (0_u16, Digest32::from_bytes([0; 32])),
        (sequence, Some(previous)) if sequence > 1 && !digest_is_zero(previous) => {
            (DESCRIPTOR_EVIDENCE_HAS_PREVIOUS, previous)
        }
        _ => return Err(RemoteAgentDescriptorEvidenceError::InvalidSequence),
    };
    let capacity = DESCRIPTOR_EVIDENCE_HEADER_BYTES
        .checked_add(request_length as usize)
        .and_then(|length| length.checked_add(receipt_length as usize))
        .and_then(|length| length.checked_add(DESCRIPTOR_EVIDENCE_CHECKSUM_BYTES))
        .ok_or(RemoteAgentDescriptorEvidenceError::FrameTooLarge)?;
    if capacity > MAX_REMOTE_AGENT_DESCRIPTOR_EVIDENCE_BYTES {
        return Err(RemoteAgentDescriptorEvidenceError::FrameTooLarge);
    }
    let mut prefix = Vec::with_capacity(capacity - DESCRIPTOR_EVIDENCE_CHECKSUM_BYTES);
    prefix.extend_from_slice(DESCRIPTOR_EVIDENCE_MAGIC);
    prefix.extend_from_slice(&DESCRIPTOR_EVIDENCE_VERSION.to_be_bytes());
    prefix.extend_from_slice(&flags.to_be_bytes());
    prefix.extend_from_slice(&record.record_sequence.to_be_bytes());
    prefix.extend_from_slice(&request_length.to_be_bytes());
    prefix.extend_from_slice(&receipt_length.to_be_bytes());
    prefix.extend_from_slice(record.target.as_bytes());
    prefix.extend_from_slice(&record.runtime_store_instance_id);
    prefix.extend_from_slice(&record.runtime_host_epoch.to_be_bytes());
    prefix.extend_from_slice(&record.fabric_generation.value().to_be_bytes());
    prefix.extend_from_slice(&record.agent_generation.value().to_be_bytes());
    prefix.extend_from_slice(record.active_pxst_digest.as_bytes());
    prefix.extend_from_slice(record.descriptor_payload_digest.as_bytes());
    prefix.extend_from_slice(record.receipt_digest.as_bytes());
    prefix.extend_from_slice(record.request_digest.as_bytes());
    prefix.extend_from_slice(previous_digest.as_bytes());
    if prefix.len() != DESCRIPTOR_EVIDENCE_HEADER_BYTES {
        return Err(RemoteAgentDescriptorEvidenceError::NonCanonical);
    }
    prefix.extend_from_slice(record.request.canonical_wire());
    prefix.extend_from_slice(record.receipt.canonical_wire());
    let record_digest = record_digest(&prefix)?;
    prefix.extend_from_slice(record_digest.as_bytes());
    Ok((prefix.into_boxed_slice(), record_digest))
}

fn record_digest(prefix: &[u8]) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(DESCRIPTOR_EVIDENCE_RECORD_DIGEST_DOMAIN)?;
    builder.field_bytes(prefix)?;
    Ok(builder.finish())
}

fn digest_is_zero(digest: Digest32) -> bool {
    digest.as_bytes().iter().all(|byte| *byte == 0)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RemoteAgentDescriptorEvidenceError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(RemoteAgentDescriptorEvidenceError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(RemoteAgentDescriptorEvidenceError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RemoteAgentDescriptorEvidenceError> {
        self.take(N)?
            .try_into()
            .map_err(|_| RemoteAgentDescriptorEvidenceError::Truncated)
    }

    fn u16(&mut self) -> Result<u16, RemoteAgentDescriptorEvidenceError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, RemoteAgentDescriptorEvidenceError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn usize_u32(&mut self) -> Result<usize, RemoteAgentDescriptorEvidenceError> {
        usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| RemoteAgentDescriptorEvidenceError::InvalidLength)
    }

    fn finish(self) -> Result<(), RemoteAgentDescriptorEvidenceError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(RemoteAgentDescriptorEvidenceError::NonCanonical)
        }
    }
}

#[derive(Debug)]
pub(crate) enum RemoteAgentDescriptorEvidenceError {
    FrameTooLarge,
    Truncated,
    UnsupportedWire,
    InvalidLength,
    InvalidSequence,
    SequenceExhausted,
    InvalidGeneration,
    NotDescribe,
    CorrelationMismatch,
    ChecksumMismatch,
    NonCanonical,
    LiveStateMismatch,
    Digest(DigestBuildError),
    Contract(ManagedServingBootstrapError),
}

impl fmt::Display for RemoteAgentDescriptorEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge => formatter.write_str("descriptor evidence frame is too large"),
            Self::Truncated => formatter.write_str("descriptor evidence frame is truncated"),
            Self::UnsupportedWire => formatter.write_str("unsupported descriptor evidence wire"),
            Self::InvalidLength => formatter.write_str("invalid descriptor evidence length"),
            Self::InvalidSequence => formatter.write_str("invalid descriptor evidence sequence"),
            Self::SequenceExhausted => {
                formatter.write_str("descriptor evidence sequence is exhausted")
            }
            Self::InvalidGeneration => {
                formatter.write_str("invalid descriptor evidence generation")
            }
            Self::NotDescribe => formatter.write_str("descriptor evidence is not PXAG Describe"),
            Self::CorrelationMismatch => {
                formatter.write_str("descriptor evidence request/receipt correlation mismatch")
            }
            Self::ChecksumMismatch => formatter.write_str("descriptor evidence checksum mismatch"),
            Self::NonCanonical => formatter.write_str("non-canonical descriptor evidence frame"),
            Self::LiveStateMismatch => {
                formatter.write_str("descriptor evidence does not match current live state")
            }
            Self::Digest(error) => write!(formatter, "descriptor evidence digest failed: {error}"),
            Self::Contract(error) => {
                write!(formatter, "descriptor evidence contract failed: {error}")
            }
        }
    }
}

impl std::error::Error for RemoteAgentDescriptorEvidenceError {}

impl From<DigestBuildError> for RemoteAgentDescriptorEvidenceError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use paraegox_kernel::{
        digest::Digest32,
        identity::{PrincipalRef, RuntimeHostId},
    };
    use paraegox_runtime_contracts::{
        distributed_agent_stack_plan::{
            RestrictedRuntimeApplyCarrierBindingFieldsV1, RestrictedRuntimeApplyCarrierBindingV1,
        },
        managed_service::ManagedServiceGeneration,
        managed_serving_bootstrap::{
            RuntimeAgentControlReceiptDraftV1, RuntimeAgentControlRequestDraftV1,
            RuntimeAgentControlRequestFieldsV1, RuntimeAgentControlRequestIdV1,
            RuntimeAgentControlResponseAuthClaimV1,
        },
        wire::{ApplyAuthAlgorithm, ApplyAuthKeyRef, ApplyRequestAuthClaim},
    };

    use super::{
        DESCRIPTOR_EVIDENCE_HEADER_BYTES, RemoteAgentDescriptorEvidenceError,
        RemoteAgentDescriptorEvidenceV1, RemoteAgentDescriptorLiveFactsV1,
        verify_remote_agent_descriptor_evidence_v1,
    };

    const TARGET: RuntimeHostId = RuntimeHostId::from_bytes([0x71; 16]);
    const STORE: [u8; 32] = [0x72; 32];
    const CONTROLLER_PRINCIPAL: PrincipalRef = PrincipalRef::from_bytes([0x73; 16]);
    const CONTROLLER_KEY: ApplyAuthKeyRef = ApplyAuthKeyRef::from_bytes([0x74; 16]);
    const RUNTIME_PRINCIPAL: PrincipalRef = PrincipalRef::from_bytes([0x75; 16]);
    const RUNTIME_KEY: ApplyAuthKeyRef = ApplyAuthKeyRef::from_bytes([0x76; 16]);
    const CONTROLLER_SIGNATURE_BYTE: u8 = 0x77;
    const RUNTIME_SIGNATURE_BYTE: u8 = 0x78;
    const ACTIVE_PXST_BYTE: u8 = 0x79;
    const FABRIC_GENERATION: u64 = 11;
    const AGENT_GENERATION: u64 = 13;
    const RUNTIME_HOST_EPOCH: u64 = 17;
    const DESCRIPTOR: &[u8] = b"PXAP\0\x01descriptor-evidence-fixture";

    fn digest(byte: u8) -> Digest32 {
        Digest32::from_bytes([byte; 32])
    }

    fn carrier() -> RestrictedRuntimeApplyCarrierBindingV1 {
        RestrictedRuntimeApplyCarrierBindingV1::try_new(
            RestrictedRuntimeApplyCarrierBindingFieldsV1 {
                target: TARGET,
                runtime_principal: RUNTIME_PRINCIPAL,
                controller_principal: CONTROLLER_PRINCIPAL,
                endpoint_ref: [0x7a; 16],
                endpoint_generation: 19,
                route: "paraegox/runtime/descriptor-evidence/apply",
                controller_request_key: CONTROLLER_KEY,
                controller_request_key_fingerprint: digest(0x7b),
                runtime_response_key: RUNTIME_KEY,
                runtime_response_key_fingerprint: digest(0x7c),
                control_transport_profile_ref: [0x7d; 16],
                control_transport_profile_digest: digest(0x7e),
            },
        )
        .unwrap_or_else(|error| panic!("descriptor-evidence carrier rejected: {error}"))
    }

    #[derive(Clone, Copy)]
    struct DescriptorEvidenceFixtureV1<'a> {
        request_id_byte: u8,
        runtime_host_epoch: u64,
        active_pxst_byte: u8,
        descriptor: &'a [u8],
        fabric_generation: u64,
        agent_generation: u64,
        controller_signature_byte: u8,
        runtime_signature_byte: u8,
    }

    impl DescriptorEvidenceFixtureV1<'static> {
        const fn valid(request_id_byte: u8) -> Self {
            Self {
                request_id_byte,
                runtime_host_epoch: RUNTIME_HOST_EPOCH,
                active_pxst_byte: ACTIVE_PXST_BYTE,
                descriptor: DESCRIPTOR,
                fabric_generation: FABRIC_GENERATION,
                agent_generation: AGENT_GENERATION,
                controller_signature_byte: CONTROLLER_SIGNATURE_BYTE,
                runtime_signature_byte: RUNTIME_SIGNATURE_BYTE,
            }
        }
    }

    fn descriptor_evidence_fixture_with(
        previous: Option<&RemoteAgentDescriptorEvidenceV1>,
        fixture: DescriptorEvidenceFixtureV1<'_>,
    ) -> (
        RemoteAgentDescriptorEvidenceV1,
        RestrictedRuntimeApplyCarrierBindingV1,
    ) {
        let carrier = carrier();
        let fields = RuntimeAgentControlRequestFieldsV1 {
            request_id: RuntimeAgentControlRequestIdV1::try_from_bytes(
                [fixture.request_id_byte; 16],
            )
            .unwrap_or_else(|error| panic!("descriptor-evidence request id rejected: {error}")),
            carrier: carrier.clone(),
            target: TARGET,
            expected_runtime_store_instance_id: STORE,
            expected_runtime_host_epoch: fixture.runtime_host_epoch,
            auth_claim: ApplyRequestAuthClaim::try_new(
                CONTROLLER_PRINCIPAL,
                CONTROLLER_KEY,
                ApplyAuthAlgorithm::try_new(1)
                    .unwrap_or_else(|error| panic!("authentication algorithm rejected: {error}")),
                1,
                b"descriptor-evidence-controller-nonce",
            )
            .unwrap_or_else(|error| panic!("Controller claim rejected: {error}")),
        };
        let request = RuntimeAgentControlRequestDraftV1::try_describe_conversation_port(
            fields,
            digest(fixture.active_pxst_byte),
            PrincipalRef::from_bytes([0x7f; 16]),
        )
        .unwrap_or_else(|error| panic!("descriptor-evidence PXAG draft rejected: {error}"))
        .finalize(&[fixture.controller_signature_byte; 64])
        .unwrap_or_else(|error| panic!("descriptor-evidence PXAG rejected: {error}"));
        let authenticated_for_receipt = request
            .verify_controller_request(&carrier, |_, _, _, _, signature| {
                signature == [fixture.controller_signature_byte; 64]
            })
            .unwrap_or_else(|error| panic!("descriptor-evidence PXAG auth failed: {error}"));
        let response_auth = RuntimeAgentControlResponseAuthClaimV1::try_new(
            &carrier,
            RUNTIME_KEY,
            ApplyAuthAlgorithm::try_new(1)
                .unwrap_or_else(|error| panic!("response algorithm rejected: {error}")),
            1,
        )
        .unwrap_or_else(|error| panic!("Runtime response claim rejected: {error}"));
        let receipt = RuntimeAgentControlReceiptDraftV1::try_conversation_port_descriptor(
            authenticated_for_receipt,
            fixture.descriptor,
            ManagedServiceGeneration::try_new(fixture.fabric_generation)
                .unwrap_or_else(|error| panic!("Fabric generation rejected: {error}")),
            ManagedServiceGeneration::try_new(fixture.agent_generation)
                .unwrap_or_else(|error| panic!("Agent generation rejected: {error}")),
            response_auth,
        )
        .unwrap_or_else(|error| panic!("descriptor-evidence PXAH draft rejected: {error}"))
        .finalize(&[fixture.runtime_signature_byte; 64])
        .unwrap_or_else(|error| panic!("descriptor-evidence PXAH rejected: {error}"));
        let authenticated_request = request
            .verify_controller_request(&carrier, |_, _, _, _, signature| {
                signature == [fixture.controller_signature_byte; 64]
            })
            .unwrap_or_else(|error| panic!("descriptor-evidence PXAG reauth failed: {error}"));
        let authenticated_receipt = receipt
            .verify_runtime_descriptor_receipt(&request, &carrier, |_, _, _, _, signature| {
                signature == [fixture.runtime_signature_byte; 64]
            })
            .unwrap_or_else(|error| panic!("descriptor-evidence PXAH auth failed: {error}"));
        let evidence = RemoteAgentDescriptorEvidenceV1::try_next(
            previous,
            authenticated_request,
            authenticated_receipt,
        )
        .unwrap_or_else(|error| panic!("descriptor evidence rejected: {error}"));
        (evidence, carrier)
    }

    pub(crate) fn descriptor_evidence_fixture(
        previous: Option<&RemoteAgentDescriptorEvidenceV1>,
        request_id_byte: u8,
    ) -> (
        RemoteAgentDescriptorEvidenceV1,
        RestrictedRuntimeApplyCarrierBindingV1,
    ) {
        descriptor_evidence_fixture_with(
            previous,
            DescriptorEvidenceFixtureV1::valid(request_id_byte),
        )
    }

    fn valid_fixture(
        previous: Option<&RemoteAgentDescriptorEvidenceV1>,
        request_id_byte: u8,
    ) -> (
        RemoteAgentDescriptorEvidenceV1,
        RestrictedRuntimeApplyCarrierBindingV1,
    ) {
        descriptor_evidence_fixture(previous, request_id_byte)
    }

    fn live_facts<'a>(
        carrier: &'a RestrictedRuntimeApplyCarrierBindingV1,
        descriptor: &'a [u8],
    ) -> RemoteAgentDescriptorLiveFactsV1<'a> {
        RemoteAgentDescriptorLiveFactsV1 {
            carrier,
            target: TARGET,
            store_instance_id: STORE,
            runtime_host_epoch: RUNTIME_HOST_EPOCH,
            active_pxst_digest: digest(ACTIVE_PXST_BYTE),
            descriptor,
            fabric_generation: ManagedServiceGeneration::try_new(FABRIC_GENERATION)
                .unwrap_or_else(|error| panic!("Fabric generation rejected: {error}")),
            agent_generation: ManagedServiceGeneration::try_new(AGENT_GENERATION)
                .unwrap_or_else(|error| panic!("Agent generation rejected: {error}")),
        }
    }

    fn verify_live<'a>(
        evidence: &'a RemoteAgentDescriptorEvidenceV1,
        live: RemoteAgentDescriptorLiveFactsV1<'_>,
    ) -> Result<
        super::RemoteAgentVerifiedDescriptorEvidenceV1<'a>,
        RemoteAgentDescriptorEvidenceError,
    > {
        verify_remote_agent_descriptor_evidence_v1(
            evidence,
            live,
            |_, _, _, _, signature| signature == [CONTROLLER_SIGNATURE_BYTE; 64],
            |_, _, _, _, signature| signature == [RUNTIME_SIGNATURE_BYTE; 64],
        )
    }

    #[test]
    fn remote_agent_descriptor_evidence_round_trips_first_and_next_and_rejects_wire_tamper() {
        let (first, _) = valid_fixture(None, 0x81);
        assert_eq!(first.record_sequence(), 1);
        assert_eq!(first.previous_record_digest(), None);
        let decoded_first = RemoteAgentDescriptorEvidenceV1::decode(first.canonical_wire())
            .unwrap_or_else(|error| panic!("first PXDE failed strict decode: {error}"));
        assert_eq!(decoded_first, first);

        let (next, _) = valid_fixture(Some(&first), 0x82);
        assert_eq!(next.record_sequence(), 2);
        assert_eq!(next.previous_record_digest(), Some(first.record_digest()));
        let decoded_next = RemoteAgentDescriptorEvidenceV1::decode(next.canonical_wire())
            .unwrap_or_else(|error| panic!("next PXDE failed strict decode: {error}"));
        assert_eq!(decoded_next, next);

        for (offset, value) in [(0, b'Q'), (4, 1), (5, 2)] {
            let mut unsupported = first.canonical_wire().to_vec();
            unsupported[offset] = value;
            assert!(matches!(
                RemoteAgentDescriptorEvidenceV1::decode(&unsupported),
                Err(RemoteAgentDescriptorEvidenceError::UnsupportedWire)
            ));
        }
        for offset in [
            72,
            80,
            88,
            96,
            128,
            160,
            192,
            DESCRIPTOR_EVIDENCE_HEADER_BYTES,
            first.canonical_wire().len() - 33,
        ] {
            let mut tampered = first.canonical_wire().to_vec();
            tampered[offset] ^= 1;
            assert!(
                RemoteAgentDescriptorEvidenceV1::decode(&tampered).is_err(),
                "PXDE tamper at offset {offset} was accepted",
            );
        }
    }

    #[test]
    fn remote_agent_descriptor_evidence_reverification_rejects_signatures_and_live_swaps() {
        let (valid, carrier) = valid_fixture(None, 0x83);
        let verified = verify_live(&valid, live_facts(&carrier, DESCRIPTOR))
            .unwrap_or_else(|error| panic!("valid PXDE live verification failed: {error}"));
        assert_eq!(verified.evidence().record_digest(), valid.record_digest());

        let (wrong_controller, wrong_controller_carrier) = descriptor_evidence_fixture_with(
            None,
            DescriptorEvidenceFixtureV1 {
                controller_signature_byte: CONTROLLER_SIGNATURE_BYTE ^ 1,
                ..DescriptorEvidenceFixtureV1::valid(0x84)
            },
        );
        assert!(matches!(
            verify_live(
                &wrong_controller,
                live_facts(&wrong_controller_carrier, DESCRIPTOR),
            ),
            Err(RemoteAgentDescriptorEvidenceError::Contract(_))
        ));
        let (wrong_runtime, wrong_runtime_carrier) = descriptor_evidence_fixture_with(
            None,
            DescriptorEvidenceFixtureV1 {
                runtime_signature_byte: RUNTIME_SIGNATURE_BYTE ^ 1,
                ..DescriptorEvidenceFixtureV1::valid(0x85)
            },
        );
        assert!(matches!(
            verify_live(
                &wrong_runtime,
                live_facts(&wrong_runtime_carrier, DESCRIPTOR),
            ),
            Err(RemoteAgentDescriptorEvidenceError::Contract(_))
        ));

        let mut swapped = live_facts(&carrier, b"PXAP\0\x01swapped");
        assert!(matches!(
            verify_live(&valid, swapped),
            Err(RemoteAgentDescriptorEvidenceError::LiveStateMismatch)
        ));
        swapped = live_facts(&carrier, DESCRIPTOR);
        swapped.active_pxst_digest = digest(ACTIVE_PXST_BYTE ^ 1);
        assert!(matches!(
            verify_live(&valid, swapped),
            Err(RemoteAgentDescriptorEvidenceError::LiveStateMismatch)
        ));
        swapped = live_facts(&carrier, DESCRIPTOR);
        swapped.runtime_host_epoch += 1;
        assert!(matches!(
            verify_live(&valid, swapped),
            Err(RemoteAgentDescriptorEvidenceError::LiveStateMismatch)
        ));
        swapped = live_facts(&carrier, DESCRIPTOR);
        swapped.fabric_generation = ManagedServiceGeneration::try_new(FABRIC_GENERATION + 1)
            .unwrap_or_else(|error| panic!("swapped Fabric generation rejected: {error}"));
        assert!(matches!(
            verify_live(&valid, swapped),
            Err(RemoteAgentDescriptorEvidenceError::LiveStateMismatch)
        ));
        swapped = live_facts(&carrier, DESCRIPTOR);
        swapped.agent_generation = ManagedServiceGeneration::try_new(AGENT_GENERATION + 1)
            .unwrap_or_else(|error| panic!("swapped Agent generation rejected: {error}"));
        assert!(matches!(
            verify_live(&valid, swapped),
            Err(RemoteAgentDescriptorEvidenceError::LiveStateMismatch)
        ));
    }
}
