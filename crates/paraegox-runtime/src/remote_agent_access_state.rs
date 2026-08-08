#![cfg(unix)]

//! Runtime-private durable state for one authenticated PXRA-v1 Apply operation.
//!
//! PXRS is a bounded latest-slot snapshot, not a filesystem owner or an effect
//! executor.  It retains the byte-exact outer PXRA, the sole embedded PXAR-v10,
//! the exact active predecessor PXAS, the live-verified bootstrap PXDE required
//! by `RemoteAccessActive`, and an authenticated PXAU only in a terminal phase.
//! Recovery performs strict structural re-decoding; only the authority-bearing
//! constructors below may create new live state.

use core::fmt;

use paraegox_kernel::{
    digest::Digest32,
    identity::RuntimeHostId,
    time::ClockGeneration,
};
use paraegox_runtime_contracts::{
    managed_fabric_plan::ManagedFabricApplyTerminalOutcomeV1,
    managed_service::ManagedServiceGeneration,
    remote_agent_access::{
        ControllerAuthenticatedRemoteAgentAccessRequestV1, MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES,
        RemoteAgentAccessKindV1, RemoteAgentAccessRequestV1,
    },
    remote_agent_data_plane_plan::{
        MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES, RemoteAgentDataPlaneApplyRequestV1,
        RemoteAgentDataPlanePlanError, RemoteAgentDataPlaneTargetModeV1,
        RemoteAgentDataPlaneTerminalOutcomeV1, RemoteAgentDataPlaneTerminalReceiptV1,
        RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1,
    },
};
use sha2::{Digest as ShaDigest, Sha256};

use crate::{
    admission::VerifiedRemoteAgentDataPlaneApplyIngressV1,
    managed_fabric_state::{
        MAX_MANAGED_FABRIC_SNAPSHOT_BYTES, ManagedFabricDurablePhase, ManagedFabricSnapshot,
        ManagedFabricStateError,
    },
    managed_agent_stack_state::{
        MAX_MANAGED_AGENT_STACK_SNAPSHOT_BYTES, ManagedAgentStackDurablePhase,
        ManagedAgentStackSnapshot, ManagedAgentStackStateError,
    },
    remote_agent_descriptor_evidence::{
        MAX_REMOTE_AGENT_DESCRIPTOR_EVIDENCE_BYTES, RemoteAgentDescriptorEvidenceError,
        RemoteAgentDescriptorEvidenceV1, RemoteAgentVerifiedDescriptorEvidenceV1,
    },
};

const SNAPSHOT_MAGIC: &[u8; 4] = b"PXRS";
const SNAPSHOT_VERSION: u16 = 1;
const SNAPSHOT_HEADER_BYTES: usize = 492;
const SNAPSHOT_DIGEST_BYTES: usize = 32;
const SNAPSHOT_HAS_PREVIOUS: u16 = 1;
const SNAPSHOT_HAS_DESCRIPTOR_EVIDENCE: u16 = 1 << 1;
const SNAPSHOT_HAS_TERMINAL: u16 = 1 << 2;
const SNAPSHOT_KNOWN_FLAGS: u16 =
    SNAPSHOT_HAS_PREVIOUS | SNAPSHOT_HAS_DESCRIPTOR_EVIDENCE | SNAPSHOT_HAS_TERMINAL;
const SNAPSHOT_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-access-snapshot.sha256.v1";

pub(crate) const MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_BYTES: usize = SNAPSHOT_HEADER_BYTES
    + MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES
    + MAX_MANAGED_FABRIC_SNAPSHOT_BYTES
    + MAX_MANAGED_AGENT_STACK_SNAPSHOT_BYTES
    + MAX_REMOTE_AGENT_DESCRIPTOR_EVIDENCE_BYTES
    + MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES
    + SNAPSHOT_DIGEST_BYTES;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub(crate) enum RemoteAgentAccessDurablePhaseV1 {
    PreparedNoEffects = 1,
    AgentStopIntent = 2,
    FabricStopIntent = 3,
    FabricStartIntent = 4,
    AgentStartIntent = 5,
    ReadyObservation = 6,
    NoEffectTerminal = 7,
    ActiveReady = 8,
    LocalOnlyReady = 9,
    Uncertain = 10,
    QuarantineIntent = 11,
    Quarantined = 12,
}

impl RemoteAgentAccessDurablePhaseV1 {
    fn decode(value: u8) -> Result<Self, RemoteAgentAccessStateError> {
        match value {
            1 => Ok(Self::PreparedNoEffects),
            2 => Ok(Self::AgentStopIntent),
            3 => Ok(Self::FabricStopIntent),
            4 => Ok(Self::FabricStartIntent),
            5 => Ok(Self::AgentStartIntent),
            6 => Ok(Self::ReadyObservation),
            7 => Ok(Self::NoEffectTerminal),
            8 => Ok(Self::ActiveReady),
            9 => Ok(Self::LocalOnlyReady),
            10 => Ok(Self::Uncertain),
            11 => Ok(Self::QuarantineIntent),
            12 => Ok(Self::Quarantined),
            _ => Err(RemoteAgentAccessStateError::UnknownPhase),
        }
    }

    const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::NoEffectTerminal
                | Self::ActiveReady
                | Self::LocalOnlyReady
                | Self::Uncertain
                | Self::Quarantined
        )
    }
}

/// Generation allocation facts owned by the later PXRA effect owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentAccessGenerationStateV1 {
    pub(crate) access_generation_high_water: u64,
    pub(crate) fabric_generation_high_water: u64,
    pub(crate) agent_generation_high_water: u64,
    pub(crate) access_generation_candidate: Option<ManagedServiceGeneration>,
    pub(crate) fabric_generation_candidate: Option<ManagedServiceGeneration>,
    pub(crate) agent_generation_candidate: Option<ManagedServiceGeneration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RemoteAgentAccessAdmissionFactsV1 {
    request_digest: Digest32,
    proof_envelope_digest: Digest32,
    tenure_nonce_identity: Digest32,
    request_nonce_identity: Digest32,
    temporal_lineage_identity: Digest32,
    clock_generation: ClockGeneration,
    admitted_at_nanos: u64,
    deadline_nanos: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentAccessSnapshotV1 {
    store_instance_id: [u8; 32],
    owner_target_fingerprint: Digest32,
    transition_projection_digest: Digest32,
    fabric_owner_target_fingerprint: Digest32,
    fabric_transition_projection_digest: Digest32,
    sequence: u64,
    previous_snapshot_digest: Option<Digest32>,
    runtime_host_epoch: u64,
    target: RuntimeHostId,
    mode: RemoteAgentDataPlaneTargetModeV1,
    phase: RemoteAgentAccessDurablePhaseV1,
    generations: RemoteAgentAccessGenerationStateV1,
    admission: RemoteAgentAccessAdmissionFactsV1,
    request: RemoteAgentAccessRequestV1,
    fabric: ManagedFabricSnapshot,
    predecessor: ManagedAgentStackSnapshot,
    descriptor_evidence: Option<RemoteAgentDescriptorEvidenceV1>,
    terminal: Option<RemoteAgentDataPlaneTerminalReceiptV1>,
    canonical_wire: Box<[u8]>,
    snapshot_digest: Digest32,
}

impl RemoteAgentAccessSnapshotV1 {
    /// Creates `PreparedNoEffects` only from both authentication markers and a
    /// strictly re-decoded ActiveReady PXAS.  Active mode additionally consumes
    /// the non-cloneable live-verification marker for its exact bootstrap PXDE.
    pub(crate) fn try_prepared(
        previous: Option<&Self>,
        expected_owner_target_fingerprint: Digest32,
        expected_transition_projection_digest: Digest32,
        expected_fabric_owner_target_fingerprint: Digest32,
        expected_fabric_transition_projection_digest: Digest32,
        authenticated_request: ControllerAuthenticatedRemoteAgentAccessRequestV1<'_>,
        verified_ingress: VerifiedRemoteAgentDataPlaneApplyIngressV1,
        fabric: ManagedFabricSnapshot,
        predecessor: ManagedAgentStackSnapshot,
        verified_descriptor: Option<RemoteAgentVerifiedDescriptorEvidenceV1<'_>>,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let outer = authenticated_request.request();
        if authenticated_request.kind() != RemoteAgentAccessKindV1::ApplyRemoteAccess {
            return Err(RemoteAgentAccessStateError::NotApplyRequest);
        }
        let inner = outer
            .apply_request()
            .ok_or(RemoteAgentAccessStateError::NotApplyRequest)?;
        let authenticated_inner = verified_ingress.authenticated();
        if authenticated_inner.request_digest() != inner.request_digest() {
            return Err(RemoteAgentAccessStateError::AuthenticationMismatch);
        }
        let strict_predecessor = ManagedAgentStackSnapshot::decode(
            predecessor.canonical_wire(),
            outer.expected_runtime_store_instance_id(),
            expected_owner_target_fingerprint,
            expected_transition_projection_digest,
            inner.target_execution().predecessor().projection(),
        )
        .map_err(RemoteAgentAccessStateError::Predecessor)?;
        validate_prepared_predecessor(outer, inner, &strict_predecessor)?;
        let strict_fabric = ManagedFabricSnapshot::decode(
            fabric.canonical_wire(),
            outer.expected_runtime_store_instance_id(),
            expected_fabric_owner_target_fingerprint,
            expected_fabric_transition_projection_digest,
            inner
                .target_execution()
                .predecessor()
                .projection()
                .managed_fabric_projection(),
        )
        .map_err(RemoteAgentAccessStateError::Fabric)?;
        validate_fabric_shape(inner, &strict_predecessor, &strict_fabric)?;

        let descriptor_evidence = verified_descriptor
            .as_ref()
            .map(|verified| verified.evidence().clone());
        validate_descriptor_shape(
            outer,
            inner,
            &strict_fabric,
            &strict_predecessor,
            descriptor_evidence.as_ref(),
        )?;

        let active = strict_predecessor
            .active
            .as_ref()
            .ok_or(RemoteAgentAccessStateError::InvalidPredecessor)?;
        let (sequence, previous_snapshot_digest, access_generation_high_water) = match previous {
            Some(previous) => {
                if !previous.phase.is_terminal()
                    || previous.store_instance_id != outer.expected_runtime_store_instance_id()
                    || previous.owner_target_fingerprint != expected_owner_target_fingerprint
                    || previous.transition_projection_digest
                        != expected_transition_projection_digest
                    || previous.fabric_owner_target_fingerprint
                        != expected_fabric_owner_target_fingerprint
                    || previous.fabric_transition_projection_digest
                        != expected_fabric_transition_projection_digest
                    || previous.target != outer.target()
                    || previous
                        .request
                        .apply_request()
                        .is_some_and(|prior| prior.request_digest() == inner.request_digest())
                {
                    return Err(RemoteAgentAccessStateError::InvalidOperationReplacement);
                }
                (
                    previous
                        .sequence
                        .checked_add(1)
                        .ok_or(RemoteAgentAccessStateError::SequenceExhausted)?,
                    Some(previous.snapshot_digest),
                    previous.generations.access_generation_high_water,
                )
            }
            None => (1, None, 0),
        };
        let inherited_fabric_high_water = previous.map_or(
            strict_predecessor.fabric_generation_high_water,
            |prior| {
                prior
                    .generations
                    .fabric_generation_high_water
                    .max(strict_predecessor.fabric_generation_high_water)
            },
        );
        let inherited_agent_high_water = previous.map_or(
            strict_predecessor.agent_generation_high_water,
            |prior| {
                prior
                    .generations
                    .agent_generation_high_water
                    .max(strict_predecessor.agent_generation_high_water)
            },
        );
        if active.fabric_generation.value() > inherited_fabric_high_water
            || active.agent_generation.value() > inherited_agent_high_water
        {
            return Err(RemoteAgentAccessStateError::GenerationRegression);
        }
        let admission = RemoteAgentAccessAdmissionFactsV1 {
            request_digest: authenticated_inner.request_digest(),
            proof_envelope_digest: authenticated_inner.proof_envelope_digest(),
            tenure_nonce_identity: authenticated_inner.tenure_nonce_identity(),
            request_nonce_identity: authenticated_inner.request_nonce_identity(),
            temporal_lineage_identity: authenticated_inner.temporal_lineage_identity(),
            clock_generation: verified_ingress.clock_generation(),
            admitted_at_nanos: verified_ingress.admitted_at_nanos(),
            deadline_nanos: verified_ingress.deadline_nanos(),
        };
        Self::try_build(Self {
            store_instance_id: outer.expected_runtime_store_instance_id(),
            owner_target_fingerprint: expected_owner_target_fingerprint,
            transition_projection_digest: expected_transition_projection_digest,
            fabric_owner_target_fingerprint: expected_fabric_owner_target_fingerprint,
            fabric_transition_projection_digest: expected_fabric_transition_projection_digest,
            sequence,
            previous_snapshot_digest,
            runtime_host_epoch: outer.expected_runtime_host_epoch(),
            target: outer.target(),
            mode: inner.target_execution().mode(),
            phase: RemoteAgentAccessDurablePhaseV1::PreparedNoEffects,
            generations: RemoteAgentAccessGenerationStateV1 {
                access_generation_high_water,
                fabric_generation_high_water: inherited_fabric_high_water,
                agent_generation_high_water: inherited_agent_high_water,
                access_generation_candidate: None,
                fabric_generation_candidate: None,
                agent_generation_candidate: None,
            },
            admission,
            request: outer.clone(),
            fabric: strict_fabric,
            predecessor: strict_predecessor,
            descriptor_evidence,
            terminal: None,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })
    }

    /// Advances only a nonterminal effect or observation phase.  It cannot
    /// create a PXAU and therefore cannot enter a terminal phase.
    pub(crate) fn try_effect_successor(
        &self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        if phase.is_terminal() || !valid_phase_successor(self.phase, phase, self.mode) {
            return Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor);
        }
        validate_generation_successor(self, phase, generations)?;
        self.try_successor(phase, generations, None)
    }

    /// Enters a terminal phase only by consuming an already authenticated PXAU
    /// marker. The exact receipt is correlated with the sole PXAR-v10 embedded
    /// inside the retained outer PXRA before any successor exists.
    pub(crate) fn try_terminal_successor(
        &self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
        authenticated_terminal: RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1<'_>,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        if !phase.is_terminal() || !valid_phase_successor(self.phase, phase, self.mode) {
            return Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor);
        }
        validate_generation_successor(self, phase, generations)?;
        let terminal = authenticated_terminal.receipt().clone();
        let inner = inner_request(&self.request)?;
        terminal
            .validate_against_request(inner)
            .map_err(RemoteAgentAccessStateError::TerminalContract)?;
        self.try_successor(phase, generations, Some(terminal))
    }

    fn try_successor(
        &self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
        terminal: Option<RemoteAgentDataPlaneTerminalReceiptV1>,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(RemoteAgentAccessStateError::SequenceExhausted)?;
        Self::try_build(Self {
            store_instance_id: self.store_instance_id,
            owner_target_fingerprint: self.owner_target_fingerprint,
            transition_projection_digest: self.transition_projection_digest,
            fabric_owner_target_fingerprint: self.fabric_owner_target_fingerprint,
            fabric_transition_projection_digest: self.fabric_transition_projection_digest,
            sequence,
            previous_snapshot_digest: Some(self.snapshot_digest),
            runtime_host_epoch: self.runtime_host_epoch,
            target: self.target,
            mode: self.mode,
            phase,
            generations,
            admission: self.admission,
            request: self.request.clone(),
            fabric: self.fabric.clone(),
            predecessor: self.predecessor.clone(),
            descriptor_evidence: self.descriptor_evidence.clone(),
            terminal,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })
    }

    /// Strict recovery decode. This restores structural state only and cannot
    /// manufacture either live PXDE verification or authenticated PXAU markers.
    pub(crate) fn decode(
        frame: &[u8],
        expected_store_instance_id: [u8; 32],
        expected_owner_target_fingerprint: Digest32,
        expected_transition_projection_digest: Digest32,
        expected_fabric_owner_target_fingerprint: Digest32,
        expected_fabric_transition_projection_digest: Digest32,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        if frame.len() < SNAPSHOT_HEADER_BYTES + SNAPSHOT_DIGEST_BYTES {
            return Err(RemoteAgentAccessStateError::Truncated);
        }
        if frame.len() > MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_BYTES {
            return Err(RemoteAgentAccessStateError::FrameTooLarge);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *SNAPSHOT_MAGIC
            || cursor.u16()? != SNAPSHOT_VERSION
            || usize::from(cursor.u16()?) != SNAPSHOT_HEADER_BYTES
        {
            return Err(RemoteAgentAccessStateError::UnsupportedWire);
        }
        let total_length = cursor.usize_u32()?;
        let sequence = cursor.u64()?;
        let runtime_host_epoch = cursor.u64()?;
        let phase = RemoteAgentAccessDurablePhaseV1::decode(cursor.u8()?)?;
        let mode = decode_mode(cursor.u8()?)?;
        let flags = cursor.u16()?;
        if flags & !SNAPSHOT_KNOWN_FLAGS != 0 {
            return Err(RemoteAgentAccessStateError::InvalidFlags);
        }
        let request_length = cursor.usize_u32()?;
        let fabric_length = cursor.usize_u32()?;
        let predecessor_length = cursor.usize_u32()?;
        let descriptor_length = cursor.usize_u32()?;
        let terminal_length = cursor.usize_u32()?;
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let store_instance_id = cursor.array()?;
        let owner_target_fingerprint = Digest32::from_bytes(cursor.array()?);
        let transition_projection_digest = Digest32::from_bytes(cursor.array()?);
        let fabric_owner_target_fingerprint = Digest32::from_bytes(cursor.array()?);
        let fabric_transition_projection_digest = Digest32::from_bytes(cursor.array()?);
        let encoded_previous = Digest32::from_bytes(cursor.array()?);
        let generations = RemoteAgentAccessGenerationStateV1 {
            access_generation_high_water: cursor.u64()?,
            fabric_generation_high_water: cursor.u64()?,
            agent_generation_high_water: cursor.u64()?,
            access_generation_candidate: decode_optional_generation(cursor.u64()?)?,
            fabric_generation_candidate: decode_optional_generation(cursor.u64()?)?,
            agent_generation_candidate: decode_optional_generation(cursor.u64()?)?,
        };
        let admission = RemoteAgentAccessAdmissionFactsV1 {
            clock_generation: ClockGeneration::try_new(cursor.u64()?)
                .map_err(|_| RemoteAgentAccessStateError::InvalidAdmission)?,
            admitted_at_nanos: cursor.u64()?,
            deadline_nanos: cursor.u64()?,
            request_digest: Digest32::from_bytes(cursor.array()?),
            proof_envelope_digest: Digest32::from_bytes(cursor.array()?),
            tenure_nonce_identity: Digest32::from_bytes(cursor.array()?),
            request_nonce_identity: Digest32::from_bytes(cursor.array()?),
            temporal_lineage_identity: Digest32::from_bytes(cursor.array()?),
        };
        if cursor.offset != SNAPSHOT_HEADER_BYTES
            || total_length != frame.len()
            || request_length == 0
            || request_length > MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES
            || fabric_length == 0
            || fabric_length > MAX_MANAGED_FABRIC_SNAPSHOT_BYTES
            || predecessor_length == 0
            || predecessor_length > MAX_MANAGED_AGENT_STACK_SNAPSHOT_BYTES
            || descriptor_length > MAX_REMOTE_AGENT_DESCRIPTOR_EVIDENCE_BYTES
            || terminal_length > MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES
            || SNAPSHOT_HEADER_BYTES
                .checked_add(request_length)
                .and_then(|length| length.checked_add(fabric_length))
                .and_then(|length| length.checked_add(predecessor_length))
                .and_then(|length| length.checked_add(descriptor_length))
                .and_then(|length| length.checked_add(terminal_length))
                .and_then(|length| length.checked_add(SNAPSHOT_DIGEST_BYTES))
                != Some(frame.len())
        {
            return Err(RemoteAgentAccessStateError::InvalidLength);
        }
        validate_presence_flag(flags, SNAPSHOT_HAS_DESCRIPTOR_EVIDENCE, descriptor_length != 0)?;
        validate_presence_flag(flags, SNAPSHOT_HAS_TERMINAL, terminal_length != 0)?;
        let previous_snapshot_digest = match (
            flags & SNAPSHOT_HAS_PREVIOUS != 0,
            sequence,
            digest_is_zero(encoded_previous),
        ) {
            (false, 1, true) => None,
            (true, 2.., false) => Some(encoded_previous),
            _ => return Err(RemoteAgentAccessStateError::InvalidSequence),
        };
        let request = RemoteAgentAccessRequestV1::decode(cursor.take(request_length)?)
            .map_err(|_| RemoteAgentAccessStateError::InvalidNestedRequest)?;
        let inner = inner_request(&request)?;
        let fabric = ManagedFabricSnapshot::decode(
            cursor.take(fabric_length)?,
            expected_store_instance_id,
            expected_fabric_owner_target_fingerprint,
            expected_fabric_transition_projection_digest,
            inner
                .target_execution()
                .predecessor()
                .projection()
                .managed_fabric_projection(),
        )
        .map_err(RemoteAgentAccessStateError::Fabric)?;
        let predecessor = ManagedAgentStackSnapshot::decode(
            cursor.take(predecessor_length)?,
            expected_store_instance_id,
            expected_owner_target_fingerprint,
            expected_transition_projection_digest,
            inner.target_execution().predecessor().projection(),
        )
        .map_err(RemoteAgentAccessStateError::Predecessor)?;
        let descriptor_evidence = if descriptor_length == 0 {
            None
        } else {
            Some(
                RemoteAgentDescriptorEvidenceV1::decode(cursor.take(descriptor_length)?)
                    .map_err(RemoteAgentAccessStateError::DescriptorEvidence)?,
            )
        };
        let terminal = if terminal_length == 0 {
            None
        } else {
            Some(
                RemoteAgentDataPlaneTerminalReceiptV1::decode(cursor.take(terminal_length)?)
                    .map_err(RemoteAgentAccessStateError::TerminalContract)?,
            )
        };
        let encoded_snapshot_digest = Digest32::from_bytes(cursor.array()?);
        cursor.finish()?;
        if store_instance_id != expected_store_instance_id
            || owner_target_fingerprint != expected_owner_target_fingerprint
            || transition_projection_digest != expected_transition_projection_digest
            || fabric_owner_target_fingerprint != expected_fabric_owner_target_fingerprint
            || fabric_transition_projection_digest
                != expected_fabric_transition_projection_digest
        {
            return Err(RemoteAgentAccessStateError::IdentityMismatch);
        }
        let expected_snapshot_digest = snapshot_digest(&frame[..frame.len() - SNAPSHOT_DIGEST_BYTES]);
        if expected_snapshot_digest != encoded_snapshot_digest {
            return Err(RemoteAgentAccessStateError::ChecksumMismatch);
        }
        let snapshot = Self::try_build(Self {
            store_instance_id,
            owner_target_fingerprint,
            transition_projection_digest,
            fabric_owner_target_fingerprint,
            fabric_transition_projection_digest,
            sequence,
            previous_snapshot_digest,
            runtime_host_epoch,
            target,
            mode,
            phase,
            generations,
            admission,
            request,
            fabric,
            predecessor,
            descriptor_evidence,
            terminal,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })?;
        if snapshot.snapshot_digest != encoded_snapshot_digest || snapshot.canonical_wire() != frame
        {
            return Err(RemoteAgentAccessStateError::NonCanonical);
        }
        Ok(snapshot)
    }

    fn try_build(mut snapshot: Self) -> Result<Self, RemoteAgentAccessStateError> {
        snapshot.validate_shape()?;
        let (canonical_wire, snapshot_digest) = snapshot.encode()?;
        snapshot.canonical_wire = canonical_wire;
        snapshot.snapshot_digest = snapshot_digest;
        Ok(snapshot)
    }

    fn validate_shape(&self) -> Result<(), RemoteAgentAccessStateError> {
        if self.store_instance_id.iter().all(|byte| *byte == 0)
            || digest_is_zero(self.owner_target_fingerprint)
            || digest_is_zero(self.transition_projection_digest)
            || digest_is_zero(self.fabric_owner_target_fingerprint)
            || digest_is_zero(self.fabric_transition_projection_digest)
            || self.sequence == 0
            || self.runtime_host_epoch == 0
            || self.target.as_bytes().iter().all(|byte| *byte == 0)
            || self.request.kind() != RemoteAgentAccessKindV1::ApplyRemoteAccess
            || self.request.target() != self.target
            || self.request.expected_runtime_store_instance_id() != self.store_instance_id
            || self.request.expected_runtime_host_epoch() != self.runtime_host_epoch
            || self.admission.admitted_at_nanos == 0
            || self.admission.deadline_nanos < self.admission.admitted_at_nanos
            || [
                self.admission.request_digest,
                self.admission.proof_envelope_digest,
                self.admission.tenure_nonce_identity,
                self.admission.request_nonce_identity,
                self.admission.temporal_lineage_identity,
            ]
            .into_iter()
            .any(digest_is_zero)
        {
            return Err(RemoteAgentAccessStateError::InvalidState);
        }
        match (self.sequence, self.previous_snapshot_digest) {
            (1, None) => {}
            (2.., Some(previous)) if !digest_is_zero(previous) => {}
            _ => return Err(RemoteAgentAccessStateError::InvalidSequence),
        }
        let inner = inner_request(&self.request)?;
        if inner.request_digest() != self.admission.request_digest
            || inner.target_execution().mode() != self.mode
            || inner.temporal().target_clock_generation() != self.admission.clock_generation
        {
            return Err(RemoteAgentAccessStateError::InvalidAdmission);
        }
        validate_prepared_predecessor(&self.request, inner, &self.predecessor)?;
        validate_fabric_shape(inner, &self.predecessor, &self.fabric)?;
        validate_descriptor_shape(
            &self.request,
            inner,
            &self.fabric,
            &self.predecessor,
            self.descriptor_evidence.as_ref(),
        )?;
        validate_generation_shape(self)?;
        match (self.phase.is_terminal(), self.terminal.as_ref()) {
            (false, None) => {}
            (true, Some(terminal)) => validate_terminal_shape(self, terminal)?,
            _ => return Err(RemoteAgentAccessStateError::InvalidTerminalShape),
        }
        Ok(())
    }

    fn encode(&self) -> Result<(Box<[u8]>, Digest32), RemoteAgentAccessStateError> {
        let request_length = checked_length(self.request.canonical_wire().len())?;
        let fabric_length = checked_length(self.fabric.canonical_wire().len())?;
        let predecessor_length = checked_length(self.predecessor.canonical_wire().len())?;
        let descriptor_length = checked_length(
            self.descriptor_evidence
                .as_ref()
                .map_or(0, |evidence| evidence.canonical_wire().len()),
        )?;
        let terminal_length = checked_length(
            self.terminal
                .as_ref()
                .map_or(0, |terminal| terminal.canonical_wire().len()),
        )?;
        let total_length = SNAPSHOT_HEADER_BYTES
            .checked_add(request_length as usize)
            .and_then(|length| length.checked_add(fabric_length as usize))
            .and_then(|length| length.checked_add(predecessor_length as usize))
            .and_then(|length| length.checked_add(descriptor_length as usize))
            .and_then(|length| length.checked_add(terminal_length as usize))
            .and_then(|length| length.checked_add(SNAPSHOT_DIGEST_BYTES))
            .ok_or(RemoteAgentAccessStateError::FrameTooLarge)?;
        if total_length > MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_BYTES {
            return Err(RemoteAgentAccessStateError::FrameTooLarge);
        }
        let total_length = u32::try_from(total_length)
            .map_err(|_| RemoteAgentAccessStateError::FrameTooLarge)?;
        let mut flags = 0_u16;
        if self.previous_snapshot_digest.is_some() {
            flags |= SNAPSHOT_HAS_PREVIOUS;
        }
        if self.descriptor_evidence.is_some() {
            flags |= SNAPSHOT_HAS_DESCRIPTOR_EVIDENCE;
        }
        if self.terminal.is_some() {
            flags |= SNAPSHOT_HAS_TERMINAL;
        }
        let mut wire = Vec::with_capacity(total_length as usize);
        wire.extend_from_slice(SNAPSHOT_MAGIC);
        wire.extend_from_slice(&SNAPSHOT_VERSION.to_be_bytes());
        wire.extend_from_slice(&(SNAPSHOT_HEADER_BYTES as u16).to_be_bytes());
        wire.extend_from_slice(&total_length.to_be_bytes());
        wire.extend_from_slice(&self.sequence.to_be_bytes());
        wire.extend_from_slice(&self.runtime_host_epoch.to_be_bytes());
        wire.push(self.phase as u8);
        wire.push(self.mode as u8);
        wire.extend_from_slice(&flags.to_be_bytes());
        wire.extend_from_slice(&request_length.to_be_bytes());
        wire.extend_from_slice(&fabric_length.to_be_bytes());
        wire.extend_from_slice(&predecessor_length.to_be_bytes());
        wire.extend_from_slice(&descriptor_length.to_be_bytes());
        wire.extend_from_slice(&terminal_length.to_be_bytes());
        wire.extend_from_slice(self.target.as_bytes());
        wire.extend_from_slice(&self.store_instance_id);
        wire.extend_from_slice(self.owner_target_fingerprint.as_bytes());
        wire.extend_from_slice(self.transition_projection_digest.as_bytes());
        wire.extend_from_slice(self.fabric_owner_target_fingerprint.as_bytes());
        wire.extend_from_slice(self.fabric_transition_projection_digest.as_bytes());
        wire.extend_from_slice(
            self.previous_snapshot_digest
                .unwrap_or_else(zero_digest)
                .as_bytes(),
        );
        wire.extend_from_slice(&self.generations.access_generation_high_water.to_be_bytes());
        wire.extend_from_slice(&self.generations.fabric_generation_high_water.to_be_bytes());
        wire.extend_from_slice(&self.generations.agent_generation_high_water.to_be_bytes());
        wire.extend_from_slice(&encode_optional_generation(
            self.generations.access_generation_candidate,
        ));
        wire.extend_from_slice(&encode_optional_generation(
            self.generations.fabric_generation_candidate,
        ));
        wire.extend_from_slice(&encode_optional_generation(
            self.generations.agent_generation_candidate,
        ));
        wire.extend_from_slice(&self.admission.clock_generation.value().to_be_bytes());
        wire.extend_from_slice(&self.admission.admitted_at_nanos.to_be_bytes());
        wire.extend_from_slice(&self.admission.deadline_nanos.to_be_bytes());
        wire.extend_from_slice(self.admission.request_digest.as_bytes());
        wire.extend_from_slice(self.admission.proof_envelope_digest.as_bytes());
        wire.extend_from_slice(self.admission.tenure_nonce_identity.as_bytes());
        wire.extend_from_slice(self.admission.request_nonce_identity.as_bytes());
        wire.extend_from_slice(self.admission.temporal_lineage_identity.as_bytes());
        if wire.len() != SNAPSHOT_HEADER_BYTES {
            return Err(RemoteAgentAccessStateError::NonCanonical);
        }
        wire.extend_from_slice(self.request.canonical_wire());
        wire.extend_from_slice(self.fabric.canonical_wire());
        wire.extend_from_slice(self.predecessor.canonical_wire());
        if let Some(evidence) = &self.descriptor_evidence {
            wire.extend_from_slice(evidence.canonical_wire());
        }
        if let Some(terminal) = &self.terminal {
            wire.extend_from_slice(terminal.canonical_wire());
        }
        let snapshot_digest = snapshot_digest(&wire);
        wire.extend_from_slice(snapshot_digest.as_bytes());
        Ok((wire.into_boxed_slice(), snapshot_digest))
    }

    #[must_use]
    pub(crate) const fn phase(&self) -> RemoteAgentAccessDurablePhaseV1 {
        self.phase
    }

    #[must_use]
    pub(crate) const fn mode(&self) -> RemoteAgentDataPlaneTargetModeV1 {
        self.mode
    }

    #[must_use]
    pub(crate) const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub(crate) const fn previous_snapshot_digest(&self) -> Option<Digest32> {
        self.previous_snapshot_digest
    }

    #[must_use]
    pub(crate) const fn snapshot_digest(&self) -> Digest32 {
        self.snapshot_digest
    }

    #[must_use]
    pub(crate) const fn generations(&self) -> RemoteAgentAccessGenerationStateV1 {
        self.generations
    }

    #[must_use]
    pub(crate) fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }
}

fn validate_prepared_predecessor(
    outer: &RemoteAgentAccessRequestV1,
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    predecessor: &ManagedAgentStackSnapshot,
) -> Result<(), RemoteAgentAccessStateError> {
    let Some(active) = predecessor.active.as_ref() else {
        return Err(RemoteAgentAccessStateError::InvalidPredecessor);
    };
    if predecessor.phase != ManagedAgentStackDurablePhase::ActiveReady
        || predecessor.runtime_host_epoch() != outer.expected_runtime_host_epoch()
        || predecessor.store_instance_id() != outer.expected_runtime_store_instance_id()
        || active.request.target_execution() != inner.target_execution().predecessor()
        || active.request.target() != outer.target()
        || active.fabric_generation.value() > predecessor.fabric_generation_high_water
        || active.agent_generation.value() > predecessor.agent_generation_high_water
    {
        return Err(RemoteAgentAccessStateError::InvalidPredecessor);
    }
    Ok(())
}

fn validate_fabric_shape(
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    predecessor: &ManagedAgentStackSnapshot,
    fabric: &ManagedFabricSnapshot,
) -> Result<(), RemoteAgentAccessStateError> {
    let Some(stack_active) = predecessor.active.as_ref() else {
        return Err(RemoteAgentAccessStateError::InvalidPredecessor);
    };
    let Some(fabric_active) = fabric.active.as_ref() else {
        return Err(RemoteAgentAccessStateError::InvalidFabric);
    };
    if fabric.phase != ManagedFabricDurablePhase::ActiveReady
        || fabric.store_instance_id() != predecessor.store_instance_id()
        || fabric.runtime_host_epoch() != predecessor.runtime_host_epoch()
        || fabric_active.request.target_execution()
            != inner.target_execution().predecessor().fabric()
        || fabric_active.request.target() != inner.target()
        || fabric_active.generation != stack_active.fabric_generation
        || fabric_active.generation.value() > fabric.generation_high_water()
    {
        return Err(RemoteAgentAccessStateError::InvalidFabric);
    }
    Ok(())
}

fn validate_descriptor_shape(
    outer: &RemoteAgentAccessRequestV1,
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    fabric: &ManagedFabricSnapshot,
    predecessor: &ManagedAgentStackSnapshot,
    evidence: Option<&RemoteAgentDescriptorEvidenceV1>,
) -> Result<(), RemoteAgentAccessStateError> {
    let active = predecessor
        .active
        .as_ref()
        .ok_or(RemoteAgentAccessStateError::InvalidPredecessor)?;
    match (
        inner.target_execution().mode(),
        inner.target_execution().bootstrap_cas(),
        evidence,
    ) {
        (RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive, Some(cas), Some(evidence)) => {
            let fabric_active = fabric
                .active
                .as_ref()
                .ok_or(RemoteAgentAccessStateError::InvalidFabric)?;
            let mut matching_terminals = fabric.terminals.iter().filter(|record| {
                record.receipt.receipt_digest() == cas.expected_active_pxft_digest()
            });
            let fabric_terminal = matching_terminals
                .next()
                .ok_or(RemoteAgentAccessStateError::InvalidFabricTerminal)?;
            if matching_terminals.next().is_some() {
                return Err(RemoteAgentAccessStateError::InvalidFabricTerminal);
            }
            let fabric_terminal_facts = fabric_terminal
                .receipt
                .validate_against_request(&fabric_active.request, fabric_active.response_channel)
                .map_err(|_| RemoteAgentAccessStateError::InvalidFabricTerminal)?;
            if fabric_terminal_facts.outcome()
                != ManagedFabricApplyTerminalOutcomeV1::ActiveReady
                || fabric_terminal_facts.generation() != Some(fabric_active.generation)
                || fabric_terminal.receipt.receipt_digest()
                    != cas.expected_active_pxft_digest()
                || outer.expected_active_pxst_digest() != cas.expected_active_pxst_digest()
                || cas.expected_fabric_generation() != fabric_active.generation
                || cas.expected_agent_generation() != active.agent_generation
            {
                return Err(RemoteAgentAccessStateError::InvalidDescriptorShape);
            }
            if evidence.target() != outer.target()
                || evidence.runtime_store_instance_id()
                    != outer.expected_runtime_store_instance_id()
                || evidence.runtime_host_epoch() != outer.expected_runtime_host_epoch()
                || evidence.active_pxst_digest() != cas.expected_active_pxst_digest()
                || evidence.receipt_digest()
                    != cas.expected_bootstrap_descriptor_receipt_digest()
                || evidence.descriptor_payload_digest()
                    != cas.expected_bootstrap_descriptor_payload_digest()
                || evidence.fabric_generation() != cas.expected_fabric_generation()
                || evidence.agent_generation() != cas.expected_agent_generation()
                || evidence.fabric_generation() != active.fabric_generation
                || evidence.agent_generation() != active.agent_generation
                || evidence.fabric_generation() != fabric_active.generation
            {
                return Err(RemoteAgentAccessStateError::InvalidDescriptorShape);
            }
            Ok(())
        }
        (RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate, None, None) => Ok(()),
        _ => Err(RemoteAgentAccessStateError::InvalidDescriptorShape),
    }
}

fn validate_generation_shape(
    snapshot: &RemoteAgentAccessSnapshotV1,
) -> Result<(), RemoteAgentAccessStateError> {
    let generations = snapshot.generations;
    let active = snapshot
        .predecessor
        .active
        .as_ref()
        .ok_or(RemoteAgentAccessStateError::InvalidPredecessor)?;
    if generations.fabric_generation_high_water == 0
        || generations.agent_generation_high_water == 0
        || active.fabric_generation.value() > generations.fabric_generation_high_water
        || active.agent_generation.value() > generations.agent_generation_high_water
        || !candidate_matches_high_water(
            generations.access_generation_candidate,
            generations.access_generation_high_water,
        )
        || !candidate_matches_high_water(
            generations.fabric_generation_candidate,
            generations.fabric_generation_high_water,
        )
        || !candidate_matches_high_water(
            generations.agent_generation_candidate,
            generations.agent_generation_high_water,
        )
        || generations.agent_generation_candidate.is_some()
            && generations.fabric_generation_candidate.is_none()
    {
        return Err(RemoteAgentAccessStateError::InvalidGenerationShape);
    }
    let access_matches_mode = match snapshot.mode {
        RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive => {
            generations.access_generation_candidate.is_some()
                == generations.fabric_generation_candidate.is_some()
        }
        RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate => {
            generations.access_generation_candidate.is_none()
        }
    };
    if !access_matches_mode {
        return Err(RemoteAgentAccessStateError::InvalidGenerationShape);
    }
    let no_candidates = generations.access_generation_candidate.is_none()
        && generations.fabric_generation_candidate.is_none()
        && generations.agent_generation_candidate.is_none();
    let fabric_only = generations.fabric_generation_candidate.is_some()
        && generations.agent_generation_candidate.is_none();
    let ready_candidates = generations.fabric_generation_candidate.is_some()
        && generations.agent_generation_candidate.is_some();
    let valid = match snapshot.phase {
        RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
        | RemoteAgentAccessDurablePhaseV1::AgentStopIntent
        | RemoteAgentAccessDurablePhaseV1::FabricStopIntent
        | RemoteAgentAccessDurablePhaseV1::NoEffectTerminal => no_candidates,
        RemoteAgentAccessDurablePhaseV1::FabricStartIntent => fabric_only,
        RemoteAgentAccessDurablePhaseV1::AgentStartIntent
        | RemoteAgentAccessDurablePhaseV1::ReadyObservation => ready_candidates,
        RemoteAgentAccessDurablePhaseV1::ActiveReady => {
            snapshot.mode == RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive
                && ready_candidates
        }
        RemoteAgentAccessDurablePhaseV1::LocalOnlyReady => {
            snapshot.mode == RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate
                && ready_candidates
        }
        RemoteAgentAccessDurablePhaseV1::Uncertain
        | RemoteAgentAccessDurablePhaseV1::QuarantineIntent
        | RemoteAgentAccessDurablePhaseV1::Quarantined => {
            no_candidates || fabric_only || ready_candidates
        }
    };
    if !valid {
        return Err(RemoteAgentAccessStateError::InvalidGenerationShape);
    }
    if let Some(fabric) = generations.fabric_generation_candidate
        && fabric.value() <= active.fabric_generation.value()
    {
        return Err(RemoteAgentAccessStateError::InvalidGenerationShape);
    }
    if let Some(agent) = generations.agent_generation_candidate
        && agent.value() <= active.agent_generation.value()
    {
        return Err(RemoteAgentAccessStateError::InvalidGenerationShape);
    }
    Ok(())
}

fn validate_generation_successor(
    current: &RemoteAgentAccessSnapshotV1,
    next_phase: RemoteAgentAccessDurablePhaseV1,
    next: RemoteAgentAccessGenerationStateV1,
) -> Result<(), RemoteAgentAccessStateError> {
    validate_one_generation_successor(
        current.generations.access_generation_high_water,
        current.generations.access_generation_candidate,
        next.access_generation_high_water,
        next.access_generation_candidate,
    )?;
    validate_one_generation_successor(
        current.generations.fabric_generation_high_water,
        current.generations.fabric_generation_candidate,
        next.fabric_generation_high_water,
        next.fabric_generation_candidate,
    )?;
    validate_one_generation_successor(
        current.generations.agent_generation_high_water,
        current.generations.agent_generation_candidate,
        next.agent_generation_high_water,
        next.agent_generation_candidate,
    )?;
    let access_introduced = current.generations.access_generation_candidate.is_none()
        && next.access_generation_candidate.is_some();
    let fabric_introduced = current.generations.fabric_generation_candidate.is_none()
        && next.fabric_generation_candidate.is_some();
    let agent_introduced = current.generations.agent_generation_candidate.is_none()
        && next.agent_generation_candidate.is_some();
    if access_introduced
        && (next_phase != RemoteAgentAccessDurablePhaseV1::FabricStartIntent
            || current.mode != RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive)
        || fabric_introduced && next_phase != RemoteAgentAccessDurablePhaseV1::FabricStartIntent
        || agent_introduced && next_phase != RemoteAgentAccessDurablePhaseV1::AgentStartIntent
    {
        return Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor);
    }
    Ok(())
}

fn validate_one_generation_successor(
    current_high_water: u64,
    current_candidate: Option<ManagedServiceGeneration>,
    next_high_water: u64,
    next_candidate: Option<ManagedServiceGeneration>,
) -> Result<(), RemoteAgentAccessStateError> {
    match (current_candidate, next_candidate) {
        (None, None) if next_high_water == current_high_water => Ok(()),
        (None, Some(next_candidate))
            if current_high_water
                .checked_add(1)
                .is_some_and(|expected| expected == next_candidate.value())
                && next_high_water == next_candidate.value() =>
        {
            Ok(())
        }
        (Some(current_candidate), Some(next_candidate))
            if current_candidate == next_candidate && next_high_water == current_high_water =>
        {
            Ok(())
        }
        _ => Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor),
    }
}

fn valid_phase_successor(
    current: RemoteAgentAccessDurablePhaseV1,
    next: RemoteAgentAccessDurablePhaseV1,
    mode: RemoteAgentDataPlaneTargetModeV1,
) -> bool {
    use RemoteAgentAccessDurablePhaseV1::{
        ActiveReady, AgentStartIntent, AgentStopIntent, FabricStartIntent, FabricStopIntent,
        LocalOnlyReady, NoEffectTerminal, PreparedNoEffects, QuarantineIntent, Quarantined,
        ReadyObservation, Uncertain,
    };
    match (current, next) {
        (PreparedNoEffects, AgentStopIntent | NoEffectTerminal) => true,
        (AgentStopIntent, FabricStopIntent | Uncertain | QuarantineIntent)
        | (FabricStopIntent, FabricStartIntent | Uncertain | QuarantineIntent)
        | (FabricStartIntent, AgentStartIntent | Uncertain | QuarantineIntent)
        | (AgentStartIntent, ReadyObservation | Uncertain | QuarantineIntent) => true,
        (ReadyObservation, ActiveReady)
            if mode == RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive =>
        {
            true
        }
        (ReadyObservation, LocalOnlyReady)
            if mode == RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate =>
        {
            true
        }
        (ReadyObservation, Uncertain | QuarantineIntent) => true,
        (QuarantineIntent, Quarantined) => true,
        _ => false,
    }
}

fn validate_terminal_shape(
    snapshot: &RemoteAgentAccessSnapshotV1,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV1,
) -> Result<(), RemoteAgentAccessStateError> {
    let inner = inner_request(&snapshot.request)?;
    let facts = terminal
        .validate_against_request(inner)
        .map_err(RemoteAgentAccessStateError::TerminalContract)?;
    let state = facts.state();
    let evidence = facts.evidence().fields();
    let expected_outcome = match snapshot.phase {
        RemoteAgentAccessDurablePhaseV1::NoEffectTerminal => {
            RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected
        }
        RemoteAgentAccessDurablePhaseV1::ActiveReady => {
            RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady
        }
        RemoteAgentAccessDurablePhaseV1::LocalOnlyReady => {
            RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady
        }
        RemoteAgentAccessDurablePhaseV1::Uncertain => {
            RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain
        }
        RemoteAgentAccessDurablePhaseV1::Quarantined => {
            RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined
        }
        _ => return Err(RemoteAgentAccessStateError::InvalidTerminalShape),
    };
    if state.outcome() != expected_outcome
        || terminal.authentication().runtime_principal()
            != snapshot.request.carrier().runtime_principal()
        || evidence.completion_runtime_host_epoch != snapshot.runtime_host_epoch
        || evidence.completion_snapshot_sequence != snapshot.sequence
    {
        return Err(RemoteAgentAccessStateError::InvalidTerminalShape);
    }
    match snapshot.phase {
        RemoteAgentAccessDurablePhaseV1::ActiveReady => {
            if state.fabric_generation() != snapshot.generations.fabric_generation_candidate
                || state.agent_generation() != snapshot.generations.agent_generation_candidate
                || state.access_generation() != snapshot.generations.access_generation_candidate
            {
                return Err(RemoteAgentAccessStateError::InvalidTerminalShape);
            }
        }
        RemoteAgentAccessDurablePhaseV1::LocalOnlyReady => {
            if state.fabric_generation() != snapshot.generations.fabric_generation_candidate
                || state.agent_generation() != snapshot.generations.agent_generation_candidate
                || state.access_generation().is_some()
            {
                return Err(RemoteAgentAccessStateError::InvalidTerminalShape);
            }
        }
        RemoteAgentAccessDurablePhaseV1::NoEffectTerminal
        | RemoteAgentAccessDurablePhaseV1::Uncertain
        | RemoteAgentAccessDurablePhaseV1::Quarantined => {
            if !terminal_generation_is_known(
                state.fabric_generation(),
                snapshot.generations.fabric_generation_candidate,
                snapshot
                    .predecessor
                    .active
                    .as_ref()
                    .map(|active| active.fabric_generation),
                snapshot.generations.fabric_generation_high_water,
            ) || !terminal_generation_is_known(
                state.agent_generation(),
                snapshot.generations.agent_generation_candidate,
                snapshot
                    .predecessor
                    .active
                    .as_ref()
                    .map(|active| active.agent_generation),
                snapshot.generations.agent_generation_high_water,
            ) || !terminal_generation_is_known(
                state.access_generation(),
                snapshot.generations.access_generation_candidate,
                None,
                snapshot.generations.access_generation_high_water,
            ) {
                return Err(RemoteAgentAccessStateError::InvalidTerminalShape);
            }
        }
        _ => return Err(RemoteAgentAccessStateError::InvalidTerminalShape),
    }
    Ok(())
}

fn terminal_generation_is_known(
    actual: Option<ManagedServiceGeneration>,
    candidate: Option<ManagedServiceGeneration>,
    predecessor: Option<ManagedServiceGeneration>,
    high_water: u64,
) -> bool {
    actual.is_none_or(|actual| {
        actual.value() <= high_water
            && (candidate == Some(actual)
                || predecessor == Some(actual)
                || candidate.is_none() && predecessor.is_none())
    })
}

fn candidate_matches_high_water(
    candidate: Option<ManagedServiceGeneration>,
    high_water: u64,
) -> bool {
    candidate.is_none_or(|candidate| candidate.value() == high_water)
}

fn inner_request(
    request: &RemoteAgentAccessRequestV1,
) -> Result<&RemoteAgentDataPlaneApplyRequestV1, RemoteAgentAccessStateError> {
    if request.kind() != RemoteAgentAccessKindV1::ApplyRemoteAccess {
        return Err(RemoteAgentAccessStateError::NotApplyRequest);
    }
    request
        .apply_request()
        .ok_or(RemoteAgentAccessStateError::NotApplyRequest)
}

fn decode_mode(value: u8) -> Result<RemoteAgentDataPlaneTargetModeV1, RemoteAgentAccessStateError> {
    match value {
        1 => Ok(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive),
        2 => Ok(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate),
        _ => Err(RemoteAgentAccessStateError::UnknownMode),
    }
}

fn encode_optional_generation(value: Option<ManagedServiceGeneration>) -> [u8; 8] {
    value.map_or(0, ManagedServiceGeneration::value).to_be_bytes()
}

fn decode_optional_generation(
    value: u64,
) -> Result<Option<ManagedServiceGeneration>, RemoteAgentAccessStateError> {
    if value == 0 {
        Ok(None)
    } else {
        ManagedServiceGeneration::try_new(value)
            .map(Some)
            .map_err(|_| RemoteAgentAccessStateError::InvalidGenerationShape)
    }
}

fn checked_length(length: usize) -> Result<u32, RemoteAgentAccessStateError> {
    u32::try_from(length).map_err(|_| RemoteAgentAccessStateError::FrameTooLarge)
}

fn validate_presence_flag(
    flags: u16,
    flag: u16,
    present: bool,
) -> Result<(), RemoteAgentAccessStateError> {
    if (flags & flag != 0) == present {
        Ok(())
    } else {
        Err(RemoteAgentAccessStateError::InvalidFlags)
    }
}

fn snapshot_digest(prefix: &[u8]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(SNAPSHOT_DIGEST_DOMAIN);
    hasher.update((prefix.len() as u64).to_be_bytes());
    hasher.update(prefix);
    Digest32::from_bytes(hasher.finalize().into())
}

const fn zero_digest() -> Digest32 {
    Digest32::from_bytes([0; 32])
}

fn digest_is_zero(value: Digest32) -> bool {
    value.as_bytes().iter().all(|byte| *byte == 0)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RemoteAgentAccessStateError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(RemoteAgentAccessStateError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(RemoteAgentAccessStateError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RemoteAgentAccessStateError> {
        self.take(N)?
            .try_into()
            .map_err(|_| RemoteAgentAccessStateError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, RemoteAgentAccessStateError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, RemoteAgentAccessStateError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, RemoteAgentAccessStateError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn usize_u32(&mut self) -> Result<usize, RemoteAgentAccessStateError> {
        usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| RemoteAgentAccessStateError::InvalidLength)
    }

    fn finish(self) -> Result<(), RemoteAgentAccessStateError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(RemoteAgentAccessStateError::TrailingBytes)
        }
    }
}

#[derive(Debug)]
pub(crate) enum RemoteAgentAccessStateError {
    FrameTooLarge,
    Truncated,
    UnsupportedWire,
    InvalidLength,
    InvalidFlags,
    InvalidSequence,
    SequenceExhausted,
    UnknownMode,
    UnknownPhase,
    IdentityMismatch,
    ChecksumMismatch,
    NonCanonical,
    TrailingBytes,
    NotApplyRequest,
    AuthenticationMismatch,
    InvalidAdmission,
    InvalidPredecessor,
    InvalidFabric,
    InvalidFabricTerminal,
    InvalidOperationReplacement,
    InvalidDescriptorShape,
    InvalidGenerationShape,
    GenerationRegression,
    InvalidGenerationSuccessor,
    InvalidPhaseSuccessor,
    InvalidTerminalShape,
    InvalidNestedRequest,
    InvalidState,
    Fabric(ManagedFabricStateError),
    Predecessor(ManagedAgentStackStateError),
    DescriptorEvidence(RemoteAgentDescriptorEvidenceError),
    TerminalContract(RemoteAgentDataPlanePlanError),
}

impl fmt::Display for RemoteAgentAccessStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "remote Agent access state failed: {self:?}")
    }
}

impl std::error::Error for RemoteAgentAccessStateError {}
