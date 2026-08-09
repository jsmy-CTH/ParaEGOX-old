#![cfg(unix)]

//! Runtime-private durable state for one authenticated PXRA-v1 Apply operation.
//!
//! PXRS is a bounded latest-slot snapshot, not a filesystem owner or an effect
//! executor.  It retains the byte-exact outer PXRA, the sole embedded PXAR-v10,
//! the exact active predecessor PXAS, the live-verified bootstrap PXDE required
//! by `RemoteAccessActive`, and an authenticated PXAU only in a terminal phase.
//! Recovery returns inert structural state only.  No API can promote a decoded
//! PXRS back into transition authority: every restart, including a terminal
//! restart, is `ReconcileRequired` until a future store owner supplies a
//! non-cloneable marker binding the exact snapshot digest, store and Runtime
//! epoch, slot revision, and current-latest observation.  Local-only in-process
//! authority permits removal of a distinct remote overlay only.  That overlay
//! owner is not implemented yet, and the current same-session Fabric shutdown
//! path must not be used to simulate hot removal because it would also disturb
//! the retained local bindings.

use core::fmt;

use paraegox_kernel::{
    digest::Digest32,
    identity::RuntimeHostId,
    time::{ClockGeneration, ClockReading},
};
use paraegox_runtime_contracts::{
    managed_agent_stack_plan::ManagedAgentStackTerminalOutcomeV1,
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
    managed_agent_stack_state::{
        MAX_MANAGED_AGENT_STACK_SNAPSHOT_BYTES, ManagedAgentStackDurablePhase,
        ManagedAgentStackSnapshot, ManagedAgentStackStateError,
    },
    managed_fabric_state::{
        MAX_MANAGED_FABRIC_SNAPSHOT_BYTES, ManagedFabricDurablePhase, ManagedFabricSnapshot,
        ManagedFabricStateError,
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
const SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-access-snapshot.sha256.v1";

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
    RemoteAccessStopIntent = 13,
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
            13 => Ok(Self::RemoteAccessStopIntent),
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

/// Exact store and projection identities required to recover a PXRS slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentAccessSnapshotIdentityPinsV1 {
    pub(crate) store_instance_id: [u8; 32],
    pub(crate) owner_target_fingerprint: Digest32,
    pub(crate) transition_projection_digest: Digest32,
    pub(crate) fabric_owner_target_fingerprint: Digest32,
    pub(crate) fabric_transition_projection_digest: Digest32,
}

/// Strict current owner state and clock sample consumed by fresh preparation.
pub(crate) struct RemoteAgentAccessPreparedInputsV1 {
    pub(crate) fresh_clock: ClockReading,
    pub(crate) fabric: ManagedFabricSnapshot,
    pub(crate) predecessor: ManagedAgentStackSnapshot,
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

/// Non-cloneable in-process authority to advance one freshly prepared PXRS value.
pub(crate) struct RemoteAgentAuthorizedAccessSnapshotV1 {
    snapshot: RemoteAgentAccessSnapshotV1,
}

impl RemoteAgentAuthorizedAccessSnapshotV1 {
    /// Creates `PreparedNoEffects` only from both authentication markers and a
    /// strictly re-decoded ActiveReady PXAS.  Active mode additionally consumes
    /// the non-cloneable live-verification marker for its exact bootstrap PXDE.
    pub(crate) fn try_prepared(
        previous: Option<Self>,
        identity: RemoteAgentAccessSnapshotIdentityPinsV1,
        authenticated_request: ControllerAuthenticatedRemoteAgentAccessRequestV1<'_>,
        verified_ingress: VerifiedRemoteAgentDataPlaneApplyIngressV1,
        inputs: RemoteAgentAccessPreparedInputsV1,
        verified_descriptor: Option<RemoteAgentVerifiedDescriptorEvidenceV1<'_>>,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let RemoteAgentAccessPreparedInputsV1 {
            fresh_clock,
            fabric,
            predecessor,
        } = inputs;
        let previous = previous.map(|authorized| authorized.snapshot);
        let outer = authenticated_request.request();
        if authenticated_request.kind() != RemoteAgentAccessKindV1::ApplyRemoteAccess {
            return Err(RemoteAgentAccessStateError::NotApplyRequest);
        }
        if identity.store_instance_id != outer.expected_runtime_store_instance_id() {
            return Err(RemoteAgentAccessStateError::IdentityMismatch);
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
            identity.store_instance_id,
            identity.owner_target_fingerprint,
            identity.transition_projection_digest,
            inner.target_execution().predecessor().projection(),
        )
        .map_err(RemoteAgentAccessStateError::Predecessor)?;
        validate_prepared_predecessor(outer, inner, &strict_predecessor)?;
        let strict_fabric = ManagedFabricSnapshot::decode(
            fabric.canonical_wire(),
            identity.store_instance_id,
            identity.fabric_owner_target_fingerprint,
            identity.fabric_transition_projection_digest,
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
        let (sequence, previous_snapshot_digest, access_generation_high_water) =
            match previous.as_ref() {
                Some(previous) => {
                    let prior_inner = inner_request(&previous.request)?;
                    if !matches!(
                        previous.phase,
                        RemoteAgentAccessDurablePhaseV1::NoEffectTerminal
                            | RemoteAgentAccessDurablePhaseV1::ActiveReady
                            | RemoteAgentAccessDurablePhaseV1::LocalOnlyReady
                    ) || previous.store_instance_id != identity.store_instance_id
                        || previous.owner_target_fingerprint != identity.owner_target_fingerprint
                        || previous.transition_projection_digest
                            != identity.transition_projection_digest
                        || previous.fabric_owner_target_fingerprint
                            != identity.fabric_owner_target_fingerprint
                        || previous.fabric_transition_projection_digest
                            != identity.fabric_transition_projection_digest
                        || previous.runtime_host_epoch
                            != outer.expected_runtime_host_epoch()
                        || previous.target != outer.target()
                        || prior_inner.operation_id() == inner.operation_id()
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
        let observed_fabric_high_water = strict_predecessor
            .fabric_generation_high_water
            .max(strict_fabric.generation_high_water());
        let inherited_fabric_high_water =
            previous
                .as_ref()
                .map_or(observed_fabric_high_water, |prior| {
                    prior
                        .generations
                        .fabric_generation_high_water
                        .max(observed_fabric_high_water)
                });
        let inherited_agent_high_water =
            previous
                .as_ref()
                .map_or(strict_predecessor.agent_generation_high_water, |prior| {
                    prior
                        .generations
                        .agent_generation_high_water
                        .max(strict_predecessor.agent_generation_high_water)
                });
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
        validate_clock_window(inner, admission, fresh_clock)?;
        let snapshot = RemoteAgentAccessSnapshotV1::try_build(RemoteAgentAccessSnapshotV1 {
            store_instance_id: identity.store_instance_id,
            owner_target_fingerprint: identity.owner_target_fingerprint,
            transition_projection_digest: identity.transition_projection_digest,
            fabric_owner_target_fingerprint: identity.fabric_owner_target_fingerprint,
            fabric_transition_projection_digest: identity.fabric_transition_projection_digest,
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
        })?;
        Ok(Self { snapshot })
    }

    /// Begins the first effect only while the admitted temporal window remains fresh.
    pub(crate) fn try_begin_effect_successor(
        self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
        fresh_clock: ClockReading,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let snapshot = &self.snapshot;
        if snapshot.phase != RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
            || phase.is_terminal()
            || !valid_phase_successor(snapshot.phase, phase, snapshot.mode)
        {
            return Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor);
        }
        validate_clock_window(
            inner_request(&snapshot.request)?,
            snapshot.admission,
            fresh_clock,
        )?;
        validate_generation_successor(snapshot, phase, generations)?;
        self.try_successor(phase, generations, None)
    }

    /// Advances cleanup or observation after an effect has already begun.
    pub(crate) fn try_effect_successor(
        self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let snapshot = &self.snapshot;
        if snapshot.phase == RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
            || phase.is_terminal()
            || !valid_phase_successor(snapshot.phase, phase, snapshot.mode)
        {
            return Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor);
        }
        validate_generation_successor(snapshot, phase, generations)?;
        self.try_successor(phase, generations, None)
    }

    /// Enters a terminal phase only by consuming an already authenticated PXAU
    /// marker. The exact receipt is correlated with the sole PXAR-v10 embedded
    /// inside the retained outer PXRA before any successor exists.
    pub(crate) fn try_terminal_successor(
        self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
        authenticated_terminal: RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1<'_>,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let snapshot = &self.snapshot;
        if !phase.is_terminal() || !valid_phase_successor(snapshot.phase, phase, snapshot.mode) {
            return Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor);
        }
        validate_generation_successor(snapshot, phase, generations)?;
        let terminal = authenticated_terminal.receipt().clone();
        let inner = inner_request(&snapshot.request)?;
        terminal
            .validate_against_request(inner)
            .map_err(RemoteAgentAccessStateError::TerminalContract)?;
        self.try_successor(phase, generations, Some(terminal))
    }

    fn try_successor(
        self,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
        terminal: Option<RemoteAgentDataPlaneTerminalReceiptV1>,
    ) -> Result<Self, RemoteAgentAccessStateError> {
        let current = &self.snapshot;
        let sequence = current
            .sequence
            .checked_add(1)
            .ok_or(RemoteAgentAccessStateError::SequenceExhausted)?;
        let snapshot = RemoteAgentAccessSnapshotV1::try_build(RemoteAgentAccessSnapshotV1 {
            store_instance_id: current.store_instance_id,
            owner_target_fingerprint: current.owner_target_fingerprint,
            transition_projection_digest: current.transition_projection_digest,
            fabric_owner_target_fingerprint: current.fabric_owner_target_fingerprint,
            fabric_transition_projection_digest: current.fabric_transition_projection_digest,
            sequence,
            previous_snapshot_digest: Some(current.snapshot_digest),
            runtime_host_epoch: current.runtime_host_epoch,
            target: current.target,
            mode: current.mode,
            phase,
            generations,
            admission: current.admission,
            request: current.request.clone(),
            fabric: current.fabric.clone(),
            predecessor: current.predecessor.clone(),
            descriptor_evidence: current.descriptor_evidence.clone(),
            terminal,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })?;
        Ok(Self { snapshot })
    }

    #[must_use]
    pub(crate) const fn snapshot(&self) -> &RemoteAgentAccessSnapshotV1 {
        &self.snapshot
    }
}

impl RemoteAgentAccessSnapshotV1 {
    /// Strict recovery decode. This permanently restores structural state only;
    /// the returned value has no path back to transition authority.
    pub(crate) fn decode(
        frame: &[u8],
        identity: RemoteAgentAccessSnapshotIdentityPinsV1,
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
        validate_presence_flag(
            flags,
            SNAPSHOT_HAS_DESCRIPTOR_EVIDENCE,
            descriptor_length != 0,
        )?;
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
            identity.store_instance_id,
            identity.fabric_owner_target_fingerprint,
            identity.fabric_transition_projection_digest,
            inner
                .target_execution()
                .predecessor()
                .projection()
                .managed_fabric_projection(),
        )
        .map_err(RemoteAgentAccessStateError::Fabric)?;
        let predecessor = ManagedAgentStackSnapshot::decode(
            cursor.take(predecessor_length)?,
            identity.store_instance_id,
            identity.owner_target_fingerprint,
            identity.transition_projection_digest,
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
        if store_instance_id != identity.store_instance_id
            || owner_target_fingerprint != identity.owner_target_fingerprint
            || transition_projection_digest != identity.transition_projection_digest
            || fabric_owner_target_fingerprint != identity.fabric_owner_target_fingerprint
            || fabric_transition_projection_digest != identity.fabric_transition_projection_digest
        {
            return Err(RemoteAgentAccessStateError::IdentityMismatch);
        }
        let expected_snapshot_digest =
            snapshot_digest(&frame[..frame.len() - SNAPSHOT_DIGEST_BYTES]);
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
        let expected_deadline = self
            .admission
            .admitted_at_nanos
            .checked_add(inner.temporal().remaining_budget().value())
            .ok_or(RemoteAgentAccessStateError::InvalidAdmission)?;
        if inner.request_digest() != self.admission.request_digest
            || inner.target_execution().mode() != self.mode
            || inner.temporal().target_clock_generation() != self.admission.clock_generation
            || self.admission.deadline_nanos != expected_deadline
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
        let total_length =
            u32::try_from(total_length).map_err(|_| RemoteAgentAccessStateError::FrameTooLarge)?;
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
            validate_descriptor_authority_scope(outer, inner, evidence)?;
            let mut matching_stack_terminals = predecessor.terminals.iter().filter(|record| {
                record.receipt.receipt_digest() == cas.expected_active_pxst_digest()
            });
            let stack_terminal = matching_stack_terminals
                .next()
                .ok_or(RemoteAgentAccessStateError::InvalidAgentTerminal)?;
            if matching_stack_terminals.next().is_some() {
                return Err(RemoteAgentAccessStateError::InvalidAgentTerminal);
            }
            let stack_terminal_facts = stack_terminal
                .receipt
                .validate_against_request(&active.request, active.response_channel)
                .map_err(|_| RemoteAgentAccessStateError::InvalidAgentTerminal)?;
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
            if stack_terminal_facts.state().outcome()
                != ManagedAgentStackTerminalOutcomeV1::ActiveReady
                || stack_terminal_facts.state().fabric_generation()
                    != Some(active.fabric_generation)
                || stack_terminal_facts.state().agent_generation() != Some(active.agent_generation)
                || stack_terminal.receipt.receipt_digest() != cas.expected_active_pxst_digest()
                || fabric_terminal_facts.outcome()
                    != ManagedFabricApplyTerminalOutcomeV1::ActiveReady
                || fabric_terminal_facts.generation() != Some(fabric_active.generation)
                || fabric_terminal.receipt.receipt_digest() != cas.expected_active_pxft_digest()
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
                || evidence.receipt_digest() != cas.expected_bootstrap_descriptor_receipt_digest()
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

fn validate_descriptor_authority_scope(
    outer: &RemoteAgentAccessRequestV1,
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    evidence: &RemoteAgentDescriptorEvidenceV1,
) -> Result<(), RemoteAgentAccessStateError> {
    if evidence.request().carrier() != outer.carrier()
        || evidence.intended_client()
            != inner
                .target_execution()
                .profile()
                .mac_agent_client_principal()
    {
        return Err(RemoteAgentAccessStateError::InvalidDescriptorShape);
    }
    Ok(())
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
        || snapshot.fabric.generation_high_water() > generations.fabric_generation_high_water
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
    let no_candidates = generations.access_generation_candidate.is_none()
        && generations.fabric_generation_candidate.is_none()
        && generations.agent_generation_candidate.is_none();
    let fabric_only = generations.fabric_generation_candidate.is_some()
        && generations.agent_generation_candidate.is_none();
    let ready_candidates = generations.fabric_generation_candidate.is_some()
        && generations.agent_generation_candidate.is_some();
    if snapshot.mode == RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive
        && generations.access_generation_candidate.is_some()
            != generations.fabric_generation_candidate.is_some()
    {
        return Err(RemoteAgentAccessStateError::InvalidGenerationShape);
    }
    let valid = match snapshot.mode {
        RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive => match snapshot.phase {
            RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
            | RemoteAgentAccessDurablePhaseV1::AgentStopIntent
            | RemoteAgentAccessDurablePhaseV1::FabricStopIntent
            | RemoteAgentAccessDurablePhaseV1::NoEffectTerminal => no_candidates,
            RemoteAgentAccessDurablePhaseV1::FabricStartIntent => fabric_only,
            RemoteAgentAccessDurablePhaseV1::AgentStartIntent
            | RemoteAgentAccessDurablePhaseV1::ReadyObservation
            | RemoteAgentAccessDurablePhaseV1::ActiveReady => ready_candidates,
            RemoteAgentAccessDurablePhaseV1::Uncertain
            | RemoteAgentAccessDurablePhaseV1::QuarantineIntent
            | RemoteAgentAccessDurablePhaseV1::Quarantined => {
                no_candidates || fabric_only || ready_candidates
            }
            RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent
            | RemoteAgentAccessDurablePhaseV1::LocalOnlyReady => false,
        },
        RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate => {
            no_candidates
                && matches!(
                    snapshot.phase,
                    RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
                        | RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent
                        | RemoteAgentAccessDurablePhaseV1::ReadyObservation
                        | RemoteAgentAccessDurablePhaseV1::NoEffectTerminal
                        | RemoteAgentAccessDurablePhaseV1::LocalOnlyReady
                        | RemoteAgentAccessDurablePhaseV1::Uncertain
                        | RemoteAgentAccessDurablePhaseV1::QuarantineIntent
                        | RemoteAgentAccessDurablePhaseV1::Quarantined
                )
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
    if current.mode == RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate
        && next != current.generations
    {
        return Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor);
    }
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
        ReadyObservation, RemoteAccessStopIntent, Uncertain,
    };
    matches!(
        (mode, current, next),
        (_, PreparedNoEffects, NoEffectTerminal)
            | (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                PreparedNoEffects,
                AgentStopIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                AgentStopIntent,
                FabricStopIntent | Uncertain | QuarantineIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                FabricStopIntent,
                FabricStartIntent | Uncertain | QuarantineIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                FabricStartIntent,
                AgentStartIntent | Uncertain | QuarantineIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                AgentStartIntent,
                ReadyObservation | Uncertain | QuarantineIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                ReadyObservation,
                ActiveReady | Uncertain | QuarantineIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
                PreparedNoEffects,
                RemoteAccessStopIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
                RemoteAccessStopIntent,
                ReadyObservation | Uncertain | QuarantineIntent,
            )
            | (
                RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
                ReadyObservation,
                LocalOnlyReady | Uncertain | QuarantineIntent,
            )
            | (_, QuarantineIntent, Quarantined)
    )
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
            let active = snapshot
                .predecessor
                .active
                .as_ref()
                .ok_or(RemoteAgentAccessStateError::InvalidPredecessor)?;
            if state.fabric_generation() != Some(active.fabric_generation)
                || state.agent_generation() != Some(active.agent_generation)
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
        actual.value() <= high_water && (candidate == Some(actual) || predecessor == Some(actual))
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

fn validate_clock_window(
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    admission: RemoteAgentAccessAdmissionFactsV1,
    reading: ClockReading,
) -> Result<(), RemoteAgentAccessStateError> {
    let now = reading.now().value();
    if reading.domain() != inner.temporal().target_clock_domain()
        || reading.generation() != inner.temporal().target_clock_generation()
        || reading.generation() != admission.clock_generation
        || now < admission.admitted_at_nanos
        || now >= admission.deadline_nanos
    {
        return Err(RemoteAgentAccessStateError::InvalidTemporalWindow);
    }
    Ok(())
}

fn decode_mode(value: u8) -> Result<RemoteAgentDataPlaneTargetModeV1, RemoteAgentAccessStateError> {
    match value {
        1 => Ok(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive),
        2 => Ok(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate),
        _ => Err(RemoteAgentAccessStateError::UnknownMode),
    }
}

fn encode_optional_generation(value: Option<ManagedServiceGeneration>) -> [u8; 8] {
    value
        .map_or(0, ManagedServiceGeneration::value)
        .to_be_bytes()
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
    InvalidTemporalWindow,
    InvalidPredecessor,
    InvalidAgentTerminal,
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
        formatter.write_str("remote Agent access state failed: ")?;
        match self {
            Self::Fabric(error) => write!(formatter, "managed Fabric snapshot: {error}"),
            Self::Predecessor(error) => {
                write!(formatter, "managed Agent-stack predecessor: {error}")
            }
            Self::DescriptorEvidence(error) => {
                write!(formatter, "remote descriptor evidence: {error}")
            }
            Self::TerminalContract(error) => write!(formatter, "PXAU contract: {error}"),
            error => write!(formatter, "{error:?}"),
        }
    }
}

impl std::error::Error for RemoteAgentAccessStateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Fabric(error) => Some(error),
            Self::Predecessor(error) => Some(error),
            Self::DescriptorEvidence(error) => Some(error),
            Self::TerminalContract(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use paraegox_kernel::{
        digest::Digest32,
        identity::{PrincipalRef, RuntimeHostId},
        time::{BoundedDuration, ClockDomainRef, ClockGeneration, ClockReading, MonotonicInstant},
    };
    use paraegox_runtime_contracts::{
        apply::{
            ApplyOperationId, PlanWriterRef, RuntimeApplyControl, TenureAuthorityRef, TenureKeyRef,
            TenureProofAlgorithm,
        },
        distributed_agent_stack_plan::{
            RestrictedRuntimeApplyCarrierBindingFieldsV1, RestrictedRuntimeApplyCarrierBindingV1,
        },
        managed_agent_stack_plan::{
            ManagedAgentStackApplyRequestV1, ManagedAgentStackTerminalReceiptV1,
        },
        managed_fabric_plan::{ManagedFabricApplyRequestV1, ManagedFabricApplyTerminalReceiptV1},
        managed_serving_bootstrap::{
            RuntimeAgentControlReceiptDraftV1, RuntimeAgentControlRequestDraftV1,
            RuntimeAgentControlRequestFieldsV1, RuntimeAgentControlRequestIdV1,
            RuntimeAgentControlResponseAuthClaimV1,
        },
        provenance::SourceScopeRef,
        reference_control::ReferenceChannelBindingV1,
        remote_agent_access::{
            RemoteAgentAccessRequestDraftV1, RemoteAgentAccessRequestFieldsV1,
            RemoteAgentAccessRequestIdV1, RemoteAgentAccessRequestV1,
        },
        remote_agent_data_plane_plan::{
            RemoteAgentBootstrapCasV1, RemoteAgentDataPlaneApplyRequestDraftV1,
            RemoteAgentDataPlaneApplyRequestV1, RemoteAgentDataPlaneProjectionV1,
            RemoteAgentDataPlaneRemoteObservationV1, RemoteAgentDataPlaneTargetExecutionV1,
            RemoteAgentDataPlaneTerminalAuthClaimV1, RemoteAgentDataPlaneTerminalEvidenceFieldsV1,
            RemoteAgentDataPlaneTerminalEvidenceV1, RemoteAgentDataPlaneTerminalHeadV1,
            RemoteAgentDataPlaneTerminalLifecycleEffectV1, RemoteAgentDataPlaneTerminalOutcomeV1,
            RemoteAgentDataPlaneTerminalReceiptDraftV1, RemoteAgentDataPlaneTerminalStateV1,
        },
        wire::{ApplyAuthAlgorithm, ApplyAuthKeyRef, ApplyRequestAuthClaim},
    };

    use crate::{
        admission::{
            AdmissionStateLimits, ApplyAdmissionPolicy, ED25519_ALGORITHM,
            ED25519_ALGORITHM_VERSION, TrustedApplyIdentity, TrustedApplyKey,
            TrustedTenureIdentity, TrustedTenureKey,
        },
        managed_agent_stack_state::{
            ManagedAgentStackDurableActive, ManagedAgentStackSnapshot,
            ManagedAgentStackSnapshotTransition, ManagedAgentStackTerminalRecord,
        },
        managed_fabric_state::{
            ManagedFabricDurableActive, ManagedFabricSnapshot, ManagedFabricSnapshotTransition,
            ManagedFabricTerminalRecord,
        },
        remote_agent_descriptor_evidence::{
            RemoteAgentDescriptorEvidenceV1, RemoteAgentDescriptorLiveFactsV1,
            verify_remote_agent_descriptor_evidence_v1,
        },
    };

    use super::*;

    const FABRIC_FIXTURE: &str =
        include_str!("../../../tests/fixtures/wire/s7_managed_fabric_successor_v1.json");
    const STACK_FIXTURE: &str =
        include_str!("../../../tests/fixtures/wire/s7_managed_agent_stack_successor_v1.json");
    const DATA_PLANE_FIXTURE: &str =
        include_str!("../../../tests/fixtures/wire/t2_remote_agent_data_plane_v1.json");
    const ACCESS_FIXTURE: &str =
        include_str!("../../../tests/fixtures/wire/t2_remote_agent_access_v1.json");
    const STORE: [u8; 32] = [0x44; 32];
    const STACK_OWNER: Digest32 = Digest32::from_bytes([0x55; 32]);
    const STACK_PROJECTION: Digest32 = Digest32::from_bytes([0x66; 32]);
    const FABRIC_OWNER: Digest32 = Digest32::from_bytes([0x57; 32]);
    const FABRIC_PROJECTION: Digest32 = Digest32::from_bytes([0x68; 32]);
    const RUNTIME_EPOCH: u64 = 23;
    const FABRIC_GENERATION: u64 = 7;
    const AGENT_GENERATION: u64 = 8;
    const INNER_SIGNING_SEED: [u8; 32] = [0x22; 32];
    const TENURE_SIGNING_SEED: [u8; 32] = [0x11; 32];
    const OUTER_SIGNATURE: [u8; 64] = [0xe2; 64];
    const DESCRIPTOR_REQUEST_SIGNATURE: [u8; 64] = [0xd5; 64];
    const DESCRIPTOR_RECEIPT_SIGNATURE: [u8; 64] = [0xd6; 64];
    const DESCRIPTOR: &[u8] = b"PXAP\0\x01pxrs-bootstrap-descriptor";

    #[derive(Clone, Copy)]
    enum DescriptorScope {
        Exact,
        CrossCarrier,
        CrossClient,
    }

    #[derive(Clone, Copy)]
    struct PreparedOptions {
        mode: RemoteAgentDataPlaneTargetModeV1,
        include_descriptor: bool,
        include_stack_terminal: bool,
        fabric_generation_high_water: u64,
        stack_fabric_generation_high_water: u64,
        stack_agent_generation_high_water: u64,
        descriptor_scope: DescriptorScope,
        operation_id: ApplyOperationId,
        admitted_operation_id: Option<ApplyOperationId>,
        identity: RemoteAgentAccessSnapshotIdentityPinsV1,
    }

    impl PreparedOptions {
        const fn valid(mode: RemoteAgentDataPlaneTargetModeV1) -> Self {
            Self {
                mode,
                include_descriptor: matches!(
                    mode,
                    RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive
                ),
                include_stack_terminal: true,
                fabric_generation_high_water: FABRIC_GENERATION,
                stack_fabric_generation_high_water: FABRIC_GENERATION,
                stack_agent_generation_high_water: AGENT_GENERATION,
                descriptor_scope: DescriptorScope::Exact,
                operation_id: ApplyOperationId::from_bytes([0xa1; 16]),
                admitted_operation_id: None,
                identity: identity(),
            }
        }
    }

    const fn identity() -> RemoteAgentAccessSnapshotIdentityPinsV1 {
        RemoteAgentAccessSnapshotIdentityPinsV1 {
            store_instance_id: STORE,
            owner_target_fingerprint: STACK_OWNER,
            transition_projection_digest: STACK_PROJECTION,
            fabric_owner_target_fingerprint: FABRIC_OWNER,
            fabric_transition_projection_digest: FABRIC_PROJECTION,
        }
    }

    fn generation(value: u64) -> ManagedServiceGeneration {
        ManagedServiceGeneration::try_new(value)
            .unwrap_or_else(|error| panic!("fixture generation rejected: {error}"))
    }

    fn fixture_hex_after(fixture: &str, section: &str, key: &str) -> Vec<u8> {
        fn nibble(byte: u8) -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("fixture contains non-hex byte"),
            }
        }

        let section_start = fixture
            .find(section)
            .unwrap_or_else(|| panic!("missing fixture section {section}"));
        let key_start = fixture[section_start..]
            .find(key)
            .map(|offset| section_start + offset + key.len())
            .unwrap_or_else(|| panic!("missing fixture key {section}.{key}"));
        let quote_start = fixture[key_start..]
            .find('"')
            .map(|offset| key_start + offset + 1)
            .unwrap_or_else(|| panic!("missing fixture opening quote {section}.{key}"));
        let quote_end = fixture[quote_start..]
            .find('"')
            .map(|offset| quote_start + offset)
            .unwrap_or_else(|| panic!("missing fixture closing quote {section}.{key}"));
        fixture.as_bytes()[quote_start..quote_end]
            .chunks_exact(2)
            .map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1]))
            .collect()
    }

    fn stack_request() -> ManagedAgentStackApplyRequestV1 {
        ManagedAgentStackApplyRequestV1::decode(&fixture_hex_after(
            STACK_FIXTURE,
            "\"fabric_and_agent\"",
            "\"outer_v7_hex\"",
        ))
        .unwrap_or_else(|error| panic!("managed Agent-stack fixture rejected: {error}"))
    }

    fn stack_terminal() -> ManagedAgentStackTerminalReceiptV1 {
        ManagedAgentStackTerminalReceiptV1::decode(&fixture_hex_after(
            STACK_FIXTURE,
            "\"fabric_and_agent\"",
            "\"wire_hex\"",
        ))
        .unwrap_or_else(|error| panic!("managed Agent-stack terminal rejected: {error}"))
    }

    fn fabric_request() -> ManagedFabricApplyRequestV1 {
        ManagedFabricApplyRequestV1::decode(&fixture_hex_after(
            FABRIC_FIXTURE,
            "\"one_managed_fabric_service\"",
            "\"outer_v6_hex\"",
        ))
        .unwrap_or_else(|error| panic!("managed Fabric fixture rejected: {error}"))
    }

    fn fabric_terminal() -> ManagedFabricApplyTerminalReceiptV1 {
        ManagedFabricApplyTerminalReceiptV1::decode(&fixture_hex_after(
            FABRIC_FIXTURE,
            "\"active_ready\"",
            "\"wire_hex\"",
        ))
        .unwrap_or_else(|error| panic!("managed Fabric terminal rejected: {error}"))
    }

    fn data_plane_template() -> RemoteAgentDataPlaneApplyRequestV1 {
        RemoteAgentDataPlaneApplyRequestV1::decode(&fixture_hex_after(
            DATA_PLANE_FIXTURE,
            "\"data_plane\"",
            "\"pxar_v10_hex\"",
        ))
        .unwrap_or_else(|error| panic!("remote Agent data-plane fixture rejected: {error}"))
    }

    fn access_template() -> RemoteAgentAccessRequestV1 {
        RemoteAgentAccessRequestV1::decode(&fixture_hex_after(
            ACCESS_FIXTURE,
            "\"pxra_apply\"",
            "\"wire_hex\"",
        ))
        .unwrap_or_else(|error| panic!("remote Agent access fixture rejected: {error}"))
    }

    fn stack_channel(target: RuntimeHostId) -> ReferenceChannelBindingV1 {
        ReferenceChannelBindingV1::try_new(
            target,
            PrincipalRef::from_bytes([0x71; 16]),
            Digest32::from_bytes([0x72; 32]),
            Digest32::from_bytes([0x73; 32]),
        )
        .unwrap_or_else(|error| panic!("stack channel rejected: {error}"))
    }

    fn fabric_channel(target: RuntimeHostId) -> ReferenceChannelBindingV1 {
        ReferenceChannelBindingV1::try_new(
            target,
            PrincipalRef::from_bytes([0xe1; 16]),
            Digest32::from_bytes([0xe3; 32]),
            Digest32::from_bytes([0xe4; 32]),
        )
        .unwrap_or_else(|error| panic!("Fabric channel rejected: {error}"))
    }

    fn alternate_carrier(
        carrier: &RestrictedRuntimeApplyCarrierBindingV1,
    ) -> RestrictedRuntimeApplyCarrierBindingV1 {
        RestrictedRuntimeApplyCarrierBindingV1::try_new(
            RestrictedRuntimeApplyCarrierBindingFieldsV1 {
                target: carrier.target(),
                runtime_principal: carrier.runtime_principal(),
                controller_principal: carrier.controller_principal(),
                endpoint_ref: carrier.endpoint_ref(),
                endpoint_generation: carrier.endpoint_generation() + 1,
                route: carrier.route(),
                controller_request_key: carrier.controller_request_key(),
                controller_request_key_fingerprint: carrier.controller_request_key_fingerprint(),
                runtime_response_key: carrier.runtime_response_key(),
                runtime_response_key_fingerprint: carrier.runtime_response_key_fingerprint(),
                control_transport_profile_ref: carrier.control_transport_profile_ref(),
                control_transport_profile_digest: carrier.control_transport_profile_digest(),
            },
        )
        .unwrap_or_else(|error| panic!("alternate carrier rejected: {error}"))
    }

    fn active_fabric_snapshot(generation_high_water: u64) -> ManagedFabricSnapshot {
        let stack = stack_request();
        let projection = stack
            .target_execution()
            .projection()
            .managed_fabric_projection();
        let initial = ManagedFabricSnapshot::try_initial(
            STORE,
            FABRIC_OWNER,
            FABRIC_PROJECTION,
            RUNTIME_EPOCH,
            projection,
        )
        .unwrap_or_else(|error| panic!("initial PXMS rejected: {error}"));
        let request = fabric_request();
        let receipt = fabric_terminal();
        initial
            .try_successor(
                ManagedFabricSnapshotTransition {
                    generation_high_water,
                    phase: ManagedFabricDurablePhase::ActiveReady,
                    writer_fence: None,
                    revision_high_water: None,
                    active: Some(ManagedFabricDurableActive {
                        generation: generation(FABRIC_GENERATION),
                        response_channel: fabric_channel(request.target()),
                        request: request.clone(),
                    }),
                    pending: None,
                    tenure_nonces: Vec::new(),
                    request_nonces: Vec::new(),
                    temporal_lineages: Vec::new(),
                    terminals: vec![ManagedFabricTerminalRecord {
                        source_scope: request.provenance().source_scope(),
                        operation_id: request.operation_id(),
                        request_digest: request.envelope_request_digest(),
                        receipt,
                    }],
                    quarantine_reason: None,
                },
                projection,
            )
            .unwrap_or_else(|error| panic!("active PXMS rejected: {error}"))
    }

    fn active_stack_snapshot(options: PreparedOptions) -> ManagedAgentStackSnapshot {
        let request = stack_request();
        let receipt = stack_terminal();
        let projection = request.target_execution().projection().clone();
        let terminals = options
            .include_stack_terminal
            .then(|| ManagedAgentStackTerminalRecord {
                source_scope: request.provenance().source_scope(),
                operation_id: request.operation_id(),
                request_digest: request.envelope_request_digest(),
                receipt,
            });
        let mut snapshot = ManagedAgentStackSnapshot::try_initial(
            STORE,
            STACK_OWNER,
            STACK_PROJECTION,
            RUNTIME_EPOCH,
            ManagedAgentStackSnapshotTransition {
                fabric_generation_high_water: options.stack_fabric_generation_high_water,
                agent_generation_high_water: options.stack_agent_generation_high_water,
                phase: ManagedAgentStackDurablePhase::ActiveReady,
                writer_fence: None,
                revision_high_water: None,
                active: Some(ManagedAgentStackDurableActive {
                    fabric_generation: generation(FABRIC_GENERATION),
                    agent_generation: generation(AGENT_GENERATION),
                    response_channel: stack_channel(request.target()),
                    request,
                }),
                pending: None,
                tenure_nonces: Vec::new(),
                request_nonces: Vec::new(),
                temporal_lineages: Vec::new(),
                terminals: Vec::new(),
                physical_binding_census: 2,
                census_complete: true,
                fabric_ready: true,
                agent_ready: true,
                dependency_satisfied: true,
                quarantine_reason: None,
            },
            &projection,
        )
        .unwrap_or_else(|error| panic!("active PXAS rejected: {error}"));
        let Some(terminal) = terminals else {
            return snapshot;
        };
        while snapshot.sequence() < 10 {
            snapshot = snapshot
                .try_successor(snapshot.transition(), &projection)
                .unwrap_or_else(|error| panic!("PXAS sequence advance rejected: {error}"));
        }
        let mut transition = snapshot.transition();
        transition.terminals.push(terminal);
        snapshot
            .try_successor(transition, &projection)
            .unwrap_or_else(|error| panic!("PXAS terminal retention rejected: {error}"))
    }

    fn descriptor_evidence(
        carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        active_pxst_digest: Digest32,
        intended_client: PrincipalRef,
    ) -> RemoteAgentDescriptorEvidenceV1 {
        let request = RuntimeAgentControlRequestDraftV1::try_describe_conversation_port(
            RuntimeAgentControlRequestFieldsV1 {
                request_id: RuntimeAgentControlRequestIdV1::try_from_bytes([0xd1; 16])
                    .unwrap_or_else(|error| panic!("descriptor request id rejected: {error}")),
                carrier: carrier.clone(),
                target: carrier.target(),
                expected_runtime_store_instance_id: STORE,
                expected_runtime_host_epoch: RUNTIME_EPOCH,
                auth_claim: ApplyRequestAuthClaim::try_new(
                    carrier.controller_principal(),
                    carrier.controller_request_key(),
                    ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)
                        .unwrap_or_else(|error| panic!("descriptor algorithm rejected: {error}")),
                    ED25519_ALGORITHM_VERSION,
                    b"pxrs-descriptor-request-nonce",
                )
                .unwrap_or_else(|error| panic!("descriptor auth claim rejected: {error}")),
            },
            active_pxst_digest,
            intended_client,
        )
        .unwrap_or_else(|error| panic!("descriptor request draft rejected: {error}"))
        .finalize(&DESCRIPTOR_REQUEST_SIGNATURE)
        .unwrap_or_else(|error| panic!("descriptor request rejected: {error}"));
        let authenticated_for_receipt = request
            .verify_controller_request(carrier, |_, _, _, _, signature| {
                signature == DESCRIPTOR_REQUEST_SIGNATURE
            })
            .unwrap_or_else(|error| panic!("descriptor request auth rejected: {error}"));
        let response_auth = RuntimeAgentControlResponseAuthClaimV1::try_new(
            carrier,
            carrier.runtime_response_key(),
            ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)
                .unwrap_or_else(|error| panic!("descriptor response algorithm rejected: {error}")),
            ED25519_ALGORITHM_VERSION,
        )
        .unwrap_or_else(|error| panic!("descriptor response auth rejected: {error}"));
        let receipt = RuntimeAgentControlReceiptDraftV1::try_conversation_port_descriptor(
            authenticated_for_receipt,
            DESCRIPTOR,
            generation(FABRIC_GENERATION),
            generation(AGENT_GENERATION),
            response_auth,
        )
        .unwrap_or_else(|error| panic!("descriptor receipt draft rejected: {error}"))
        .finalize(&DESCRIPTOR_RECEIPT_SIGNATURE)
        .unwrap_or_else(|error| panic!("descriptor receipt rejected: {error}"));
        let authenticated_request = request
            .verify_controller_request(carrier, |_, _, _, _, signature| {
                signature == DESCRIPTOR_REQUEST_SIGNATURE
            })
            .unwrap_or_else(|error| panic!("descriptor request reauth rejected: {error}"));
        let authenticated_receipt = receipt
            .verify_runtime_descriptor_receipt(&request, carrier, |_, _, _, _, signature| {
                signature == DESCRIPTOR_RECEIPT_SIGNATURE
            })
            .unwrap_or_else(|error| panic!("descriptor receipt auth rejected: {error}"));
        RemoteAgentDescriptorEvidenceV1::try_next(
            None,
            authenticated_request,
            authenticated_receipt,
        )
        .unwrap_or_else(|error| panic!("PXDE fixture rejected: {error}"))
    }

    fn rebuilt_inner_request(
        mode: RemoteAgentDataPlaneTargetModeV1,
        cas: RemoteAgentBootstrapCasV1,
        operation_id: ApplyOperationId,
    ) -> RemoteAgentDataPlaneApplyRequestV1 {
        let template = data_plane_template();
        let predecessor = stack_request();
        let projection = RemoteAgentDataPlaneProjectionV1::try_from_managed_agent_stack_projection(
            predecessor.target_execution().projection().clone(),
        )
        .unwrap_or_else(|error| panic!("data-plane projection rejected: {error}"));
        let execution = match mode {
            RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive => {
                RemoteAgentDataPlaneTargetExecutionV1::try_remote_access_active(
                    projection,
                    predecessor.target_execution().clone(),
                    cas,
                    template.target_execution().profile().clone(),
                )
            }
            RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate => {
                RemoteAgentDataPlaneTargetExecutionV1::try_local_agent_only_deactivate(
                    projection,
                    predecessor.target_execution().clone(),
                    template.target_execution().profile().clone(),
                )
            }
        }
        .unwrap_or_else(|error| panic!("data-plane execution rejected: {error}"));
        let draft = RemoteAgentDataPlaneApplyRequestDraftV1::try_new(
            execution,
            template.provenance(),
            RuntimeApplyControl::new(
                template
                    .control_commitment()
                    .control()
                    .writer_context()
                    .clone(),
                template.control_commitment().control().expected_active(),
                operation_id,
            ),
            template.temporal(),
            template.expected_runtime_store_instance_id(),
            template.authentication().claim().clone(),
        )
        .unwrap_or_else(|error| panic!("data-plane request draft rejected: {error}"));
        let transcript = draft
            .signing_transcript()
            .unwrap_or_else(|error| panic!("data-plane transcript rejected: {error}"));
        let signature = SigningKey::from_bytes(&INNER_SIGNING_SEED)
            .sign(transcript.as_bytes())
            .to_bytes();
        draft
            .finalize(&signature)
            .unwrap_or_else(|error| panic!("data-plane request rejected: {error}"))
    }

    fn admission_policy(inner: &RemoteAgentDataPlaneApplyRequestV1) -> ApplyAdmissionPolicy {
        let tenure_key = TrustedTenureKey::try_new(
            TrustedTenureIdentity::new(
                SourceScopeRef::from_bytes([0x01; 16]),
                PrincipalRef::from_bytes([0x06; 16]),
                1_001,
                1_002,
                TenureAuthorityRef::from_bytes([0x07; 16]),
            ),
            TenureKeyRef::from_bytes([0x08; 16]),
            TenureProofAlgorithm::try_new(ED25519_ALGORITHM)
                .unwrap_or_else(|error| panic!("tenure algorithm rejected: {error}")),
            ED25519_ALGORITHM_VERSION,
            SigningKey::from_bytes(&TENURE_SIGNING_SEED)
                .verifying_key()
                .to_bytes(),
        )
        .unwrap_or_else(|error| panic!("tenure trust rejected: {error}"));
        let apply_key = TrustedApplyKey::try_new(
            TrustedApplyIdentity::new(
                SourceScopeRef::from_bytes([0x01; 16]),
                inner.target(),
                PrincipalRef::from_bytes([0x09; 16]),
                PlanWriterRef::from_bytes([0x09; 16]),
            ),
            ApplyAuthKeyRef::from_bytes([0x0c; 16]),
            ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)
                .unwrap_or_else(|error| panic!("apply algorithm rejected: {error}")),
            ED25519_ALGORITHM_VERSION,
            SigningKey::from_bytes(&INNER_SIGNING_SEED)
                .verifying_key()
                .to_bytes(),
        )
        .unwrap_or_else(|error| panic!("apply trust rejected: {error}"));
        ApplyAdmissionPolicy::try_new(
            BoundedDuration::from_nanos(inner.temporal().original_budget().value()),
            AdmissionStateLimits::try_new(4, 4, 4)
                .unwrap_or_else(|error| panic!("admission limits rejected: {error}")),
            [tenure_key],
            [apply_key],
        )
        .unwrap_or_else(|error| panic!("admission policy rejected: {error}"))
    }

    fn outer_request(
        inner: &RemoteAgentDataPlaneApplyRequestV1,
        active_pxst_digest: Digest32,
    ) -> RemoteAgentAccessRequestV1 {
        let template = access_template();
        RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
            RemoteAgentAccessRequestFieldsV1 {
                request_id: RemoteAgentAccessRequestIdV1::try_from_bytes(
                    *inner.operation_id().as_bytes(),
                )
                .unwrap_or_else(|error| panic!("access request id rejected: {error}")),
                carrier: template.carrier().clone(),
                target: inner.target(),
                expected_runtime_store_instance_id: inner.expected_runtime_store_instance_id(),
                expected_runtime_host_epoch: RUNTIME_EPOCH,
                auth_claim: template.authentication().claim().clone(),
            },
            active_pxst_digest,
            inner.clone(),
        )
        .unwrap_or_else(|error| panic!("access request draft rejected: {error}"))
        .finalize(&OUTER_SIGNATURE)
        .unwrap_or_else(|error| panic!("access request rejected: {error}"))
    }

    fn prepared_result_after(
        options: PreparedOptions,
        previous: Option<RemoteAgentAuthorizedAccessSnapshotV1>,
        fresh_clock: Option<ClockReading>,
    ) -> Result<RemoteAgentAuthorizedAccessSnapshotV1, RemoteAgentAccessStateError> {
        let stack_terminal = stack_terminal();
        let fabric_terminal = fabric_terminal();
        let carrier = access_template().carrier().clone();
        let descriptor_carrier = match options.descriptor_scope {
            DescriptorScope::CrossCarrier => alternate_carrier(&carrier),
            DescriptorScope::Exact | DescriptorScope::CrossClient => carrier.clone(),
        };
        let intended_client = match options.descriptor_scope {
            DescriptorScope::CrossClient => PrincipalRef::from_bytes([0xd3; 16]),
            DescriptorScope::Exact | DescriptorScope::CrossCarrier => data_plane_template()
                .target_execution()
                .profile()
                .mac_agent_client_principal(),
        };
        let evidence = descriptor_evidence(
            &descriptor_carrier,
            stack_terminal.receipt_digest(),
            intended_client,
        );
        let cas = RemoteAgentBootstrapCasV1::try_new(
            fabric_terminal.receipt_digest(),
            stack_terminal.receipt_digest(),
            evidence.receipt_digest(),
            evidence.descriptor_payload_digest(),
            generation(FABRIC_GENERATION),
            generation(AGENT_GENERATION),
        )
        .unwrap_or_else(|error| panic!("bootstrap CAS rejected: {error}"));
        let inner = rebuilt_inner_request(options.mode, cas, options.operation_id);
        let admitted_inner = options.admitted_operation_id.map_or_else(
            || inner.clone(),
            |operation_id| rebuilt_inner_request(options.mode, cas, operation_id),
        );
        let outer = outer_request(&inner, stack_terminal.receipt_digest());
        let authenticated_outer = outer
            .verify_controller_request(outer.carrier(), |_, _, _, _, signature| {
                signature == OUTER_SIGNATURE
            })
            .unwrap_or_else(|error| panic!("outer authentication rejected: {error}"));
        let admission_reading = ClockReading::new(
            inner.temporal().target_clock_domain(),
            inner.temporal().target_clock_generation(),
            MonotonicInstant::from_ticks(1),
        );
        let verified_ingress = admission_policy(&admitted_inner)
            .verify_remote_agent_data_plane_apply_request(&admitted_inner, admission_reading)
            .unwrap_or_else(|error| panic!("inner admission rejected: {error:?}"));
        let verified_descriptor = verify_remote_agent_descriptor_evidence_v1(
            &evidence,
            RemoteAgentDescriptorLiveFactsV1 {
                carrier: &descriptor_carrier,
                target: inner.target(),
                store_instance_id: STORE,
                runtime_host_epoch: RUNTIME_EPOCH,
                active_pxst_digest: stack_terminal.receipt_digest(),
                descriptor: DESCRIPTOR,
                fabric_generation: generation(FABRIC_GENERATION),
                agent_generation: generation(AGENT_GENERATION),
            },
            |_, _, _, _, signature| signature == DESCRIPTOR_REQUEST_SIGNATURE,
            |_, _, _, _, signature| signature == DESCRIPTOR_RECEIPT_SIGNATURE,
        )
        .unwrap_or_else(|error| panic!("descriptor live verification rejected: {error}"));
        RemoteAgentAuthorizedAccessSnapshotV1::try_prepared(
            previous,
            options.identity,
            authenticated_outer,
            verified_ingress,
            RemoteAgentAccessPreparedInputsV1 {
                fresh_clock: fresh_clock.unwrap_or(admission_reading),
                fabric: active_fabric_snapshot(options.fabric_generation_high_water),
                predecessor: active_stack_snapshot(options),
            },
            options.include_descriptor.then_some(verified_descriptor),
        )
    }

    fn prepared_result(
        options: PreparedOptions,
    ) -> Result<RemoteAgentAuthorizedAccessSnapshotV1, RemoteAgentAccessStateError> {
        prepared_result_after(options, None, None)
    }

    fn prepared_authorized(
        mode: RemoteAgentDataPlaneTargetModeV1,
    ) -> RemoteAgentAuthorizedAccessSnapshotV1 {
        prepared_result(PreparedOptions::valid(mode))
            .unwrap_or_else(|error| panic!("valid Prepared snapshot rejected: {error}"))
    }

    fn prepared(mode: RemoteAgentDataPlaneTargetModeV1) -> RemoteAgentAccessSnapshotV1 {
        prepared_authorized(mode).snapshot().clone()
    }

    fn decode_roundtrip(snapshot: &RemoteAgentAccessSnapshotV1) {
        let decoded = RemoteAgentAccessSnapshotV1::decode(snapshot.canonical_wire(), identity())
            .unwrap_or_else(|error| panic!("snapshot restart decode rejected: {error}"));
        assert_eq!(&decoded, snapshot);
        assert_eq!(decoded.canonical_wire(), snapshot.canonical_wire());
    }

    fn next_generation(high_water: u64) -> ManagedServiceGeneration {
        generation(
            high_water
                .checked_add(1)
                .unwrap_or_else(|| panic!("fixture generation high-water exhausted")),
        )
    }

    fn fabric_start_generations(
        snapshot: &RemoteAgentAccessSnapshotV1,
    ) -> RemoteAgentAccessGenerationStateV1 {
        let current = snapshot.generations();
        let fabric = next_generation(current.fabric_generation_high_water);
        let access = (snapshot.mode() == RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive)
            .then(|| next_generation(current.access_generation_high_water));
        RemoteAgentAccessGenerationStateV1 {
            access_generation_high_water: access.map_or(
                current.access_generation_high_water,
                ManagedServiceGeneration::value,
            ),
            fabric_generation_high_water: fabric.value(),
            agent_generation_high_water: current.agent_generation_high_water,
            access_generation_candidate: access,
            fabric_generation_candidate: Some(fabric),
            agent_generation_candidate: None,
        }
    }

    fn agent_start_generations(
        snapshot: &RemoteAgentAccessSnapshotV1,
    ) -> RemoteAgentAccessGenerationStateV1 {
        let current = snapshot.generations();
        let agent = next_generation(current.agent_generation_high_water);
        RemoteAgentAccessGenerationStateV1 {
            agent_generation_high_water: agent.value(),
            agent_generation_candidate: Some(agent),
            ..current
        }
    }

    fn clock_at(snapshot: &RemoteAgentAccessSnapshotV1, ticks: u64) -> ClockReading {
        let inner = inner_request(&snapshot.request)
            .unwrap_or_else(|error| panic!("clock fixture request rejected: {error}"));
        ClockReading::new(
            inner.temporal().target_clock_domain(),
            inner.temporal().target_clock_generation(),
            MonotonicInstant::from_ticks(ticks),
        )
    }

    fn effect_successor(
        authorized: RemoteAgentAuthorizedAccessSnapshotV1,
        phase: RemoteAgentAccessDurablePhaseV1,
        generations: RemoteAgentAccessGenerationStateV1,
    ) -> RemoteAgentAuthorizedAccessSnapshotV1 {
        let current = authorized.snapshot();
        let sequence = current.sequence();
        let previous_digest = current.snapshot_digest();
        let prepared = current.phase() == RemoteAgentAccessDurablePhaseV1::PreparedNoEffects;
        let fresh_clock = clock_at(current, 2);
        let successor = if prepared {
            authorized.try_begin_effect_successor(phase, generations, fresh_clock)
        } else {
            authorized.try_effect_successor(phase, generations)
        }
        .unwrap_or_else(|error| panic!("valid {phase:?} successor rejected: {error}"));
        assert_eq!(successor.snapshot().sequence(), sequence + 1);
        assert_eq!(
            successor.snapshot().previous_snapshot_digest(),
            Some(previous_digest)
        );
        decode_roundtrip(successor.snapshot());
        successor
    }

    fn ready_observation(
        mode: RemoteAgentDataPlaneTargetModeV1,
    ) -> RemoteAgentAuthorizedAccessSnapshotV1 {
        let prepared = prepared_authorized(mode);
        match mode {
            RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive => {
                let generations = prepared.snapshot().generations();
                let agent_stop = effect_successor(
                    prepared,
                    RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
                    generations,
                );
                let generations = agent_stop.snapshot().generations();
                let fabric_stop = effect_successor(
                    agent_stop,
                    RemoteAgentAccessDurablePhaseV1::FabricStopIntent,
                    generations,
                );
                let generations = fabric_start_generations(fabric_stop.snapshot());
                let fabric_start = effect_successor(
                    fabric_stop,
                    RemoteAgentAccessDurablePhaseV1::FabricStartIntent,
                    generations,
                );
                let generations = agent_start_generations(fabric_start.snapshot());
                let agent_start = effect_successor(
                    fabric_start,
                    RemoteAgentAccessDurablePhaseV1::AgentStartIntent,
                    generations,
                );
                let generations = agent_start.snapshot().generations();
                effect_successor(
                    agent_start,
                    RemoteAgentAccessDurablePhaseV1::ReadyObservation,
                    generations,
                )
            }
            RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate => {
                let generations = prepared.snapshot().generations();
                let remote_stop = effect_successor(
                    prepared,
                    RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                    generations,
                );
                let generations = remote_stop.snapshot().generations();
                effect_successor(
                    remote_stop,
                    RemoteAgentAccessDurablePhaseV1::ReadyObservation,
                    generations,
                )
            }
        }
    }

    fn terminal_auth_claim(
        snapshot: &RemoteAgentAccessSnapshotV1,
    ) -> RemoteAgentDataPlaneTerminalAuthClaimV1 {
        RemoteAgentDataPlaneTerminalAuthClaimV1::try_new(
            snapshot.request.carrier().runtime_principal(),
            snapshot.request.carrier().runtime_response_key(),
            ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)
                .unwrap_or_else(|error| panic!("terminal algorithm rejected: {error}")),
            ED25519_ALGORITHM_VERSION,
        )
        .unwrap_or_else(|error| panic!("terminal auth claim rejected: {error}"))
    }

    fn terminal_receipt(
        snapshot: &RemoteAgentAccessSnapshotV1,
        outcome: RemoteAgentDataPlaneTerminalOutcomeV1,
    ) -> RemoteAgentDataPlaneTerminalReceiptV1 {
        terminal_receipt_with_local_generations(snapshot, outcome, None)
    }

    fn terminal_receipt_with_local_generations(
        snapshot: &RemoteAgentAccessSnapshotV1,
        outcome: RemoteAgentDataPlaneTerminalOutcomeV1,
        local_generations: Option<(ManagedServiceGeneration, ManagedServiceGeneration)>,
    ) -> RemoteAgentDataPlaneTerminalReceiptV1 {
        let inner = inner_request(&snapshot.request)
            .unwrap_or_else(|error| panic!("terminal fixture request rejected: {error}"));
        let zero = zero_digest();
        let (echoed_receipt, echoed_payload) =
            inner
                .target_execution()
                .bootstrap_cas()
                .map_or((zero, zero), |cas| {
                    (
                        cas.expected_bootstrap_descriptor_receipt_digest(),
                        cas.expected_bootstrap_descriptor_payload_digest(),
                    )
                });
        let (
            lifecycle,
            head,
            fabric,
            agent,
            access,
            census,
            base_ready,
            remote,
            quarantined,
            fresh,
        ) = match outcome {
            RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady => (
                RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
                RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
                snapshot.generations().fabric_generation_candidate,
                snapshot.generations().agent_generation_candidate,
                snapshot.generations().access_generation_candidate,
                2,
                true,
                RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
                false,
                Digest32::from_bytes([0xf1; 32]),
            ),
            RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady => {
                let (fabric, agent) = local_generations.map_or_else(
                    || {
                        let active = snapshot
                            .predecessor
                            .active
                            .as_ref()
                            .unwrap_or_else(|| panic!("LocalOnly predecessor must remain active"));
                        (active.fabric_generation, active.agent_generation)
                    },
                    |generations| generations,
                );
                (
                    RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
                    RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
                    Some(fabric),
                    Some(agent),
                    None,
                    2,
                    true,
                    RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent,
                    false,
                    zero,
                )
            }
            RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected => (
                RemoteAgentDataPlaneTerminalLifecycleEffectV1::ProvenNotStarted,
                RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
                None,
                None,
                None,
                0,
                false,
                RemoteAgentDataPlaneRemoteObservationV1::Unknown,
                false,
                zero,
            ),
            RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain => (
                RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
                RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
                None,
                None,
                None,
                0,
                false,
                RemoteAgentDataPlaneRemoteObservationV1::Unknown,
                false,
                zero,
            ),
            RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined => (
                RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
                RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
                None,
                None,
                None,
                0,
                false,
                RemoteAgentDataPlaneRemoteObservationV1::Unknown,
                true,
                zero,
            ),
        };
        let state = RemoteAgentDataPlaneTerminalStateV1::try_new(
            outcome, lifecycle, head, fabric, agent, access,
        )
        .unwrap_or_else(|error| panic!("terminal state rejected: {error}"));
        let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(
            RemoteAgentDataPlaneTerminalEvidenceFieldsV1 {
                physical_binding_census: census,
                census_complete: base_ready,
                base_fabric_ready: base_ready,
                base_agent_ready: base_ready,
                remote_observation: remote,
                quarantined,
                echoed_bootstrap_descriptor_receipt_digest: echoed_receipt,
                echoed_bootstrap_descriptor_payload_digest: echoed_payload,
                fresh_current_descriptor_payload_digest: fresh,
                resource_census_digest: Digest32::from_bytes([0xf2; 32]),
                raw_outcome_digest: Digest32::from_bytes([0xf3; 32]),
                completion_runtime_host_epoch: snapshot.runtime_host_epoch,
                completion_snapshot_sequence: snapshot.sequence() + 1,
                selection_clock_domain: inner.temporal().target_clock_domain(),
                selection_clock_generation: inner.temporal().target_clock_generation(),
                selection_observed_at_nanos: 2,
            },
        )
        .unwrap_or_else(|error| panic!("terminal evidence rejected: {error}"));
        RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
            inner,
            state,
            evidence,
            terminal_auth_claim(snapshot),
        )
        .unwrap_or_else(|error| panic!("terminal receipt draft rejected: {error}"))
        .finalize(&[0xf4; 64])
        .unwrap_or_else(|error| panic!("terminal receipt rejected: {error}"))
    }

    fn terminal_successor(
        authorized: RemoteAgentAuthorizedAccessSnapshotV1,
        phase: RemoteAgentAccessDurablePhaseV1,
        outcome: RemoteAgentDataPlaneTerminalOutcomeV1,
    ) -> Result<RemoteAgentAuthorizedAccessSnapshotV1, RemoteAgentAccessStateError> {
        let snapshot = authorized.snapshot();
        let generations = snapshot.generations();
        let receipt = terminal_receipt(snapshot, outcome);
        let auth_claim = terminal_auth_claim(snapshot);
        let inner = inner_request(&snapshot.request)
            .unwrap_or_else(|error| panic!("terminal fixture request rejected: {error}"));
        let authenticated = receipt
            .verify_runtime_terminal(inner, auth_claim, |_, _, _, _, _, signature| {
                signature == [0xf4; 64]
            })
            .unwrap_or_else(|error| panic!("terminal authentication rejected: {error}"));
        authorized.try_terminal_successor(phase, generations, authenticated)
    }

    fn no_effect_terminal(options: PreparedOptions) -> RemoteAgentAuthorizedAccessSnapshotV1 {
        let prepared = prepared_result(options)
            .unwrap_or_else(|error| panic!("NoEffect Prepared fixture rejected: {error}"));
        terminal_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::NoEffectTerminal,
            RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        )
        .unwrap_or_else(|error| panic!("NoEffect terminal fixture rejected: {error}"))
    }

    #[derive(Clone, Debug)]
    struct PayloadRanges {
        request: core::ops::Range<usize>,
        fabric: core::ops::Range<usize>,
        predecessor: core::ops::Range<usize>,
        descriptor: core::ops::Range<usize>,
        terminal: core::ops::Range<usize>,
    }

    fn read_u32_at(frame: &[u8], offset: usize) -> usize {
        u32::from_be_bytes(
            frame[offset..offset + 4]
                .try_into()
                .unwrap_or_else(|_| panic!("fixture length field must be four bytes")),
        ) as usize
    }

    fn payload_ranges(frame: &[u8]) -> PayloadRanges {
        let lengths = [32, 36, 40, 44, 48].map(|offset| read_u32_at(frame, offset));
        let request_start = SNAPSHOT_HEADER_BYTES;
        let fabric_start = request_start + lengths[0];
        let predecessor_start = fabric_start + lengths[1];
        let descriptor_start = predecessor_start + lengths[2];
        let terminal_start = descriptor_start + lengths[3];
        let digest_start = terminal_start + lengths[4];
        assert_eq!(digest_start + SNAPSHOT_DIGEST_BYTES, frame.len());
        PayloadRanges {
            request: request_start..fabric_start,
            fabric: fabric_start..predecessor_start,
            predecessor: predecessor_start..descriptor_start,
            descriptor: descriptor_start..terminal_start,
            terminal: terminal_start..digest_start,
        }
    }

    fn reseal(frame: &mut [u8]) {
        let digest_start = frame
            .len()
            .checked_sub(SNAPSHOT_DIGEST_BYTES)
            .unwrap_or_else(|| panic!("snapshot fixture must include its digest"));
        let digest = snapshot_digest(&frame[..digest_start]);
        frame[digest_start..].copy_from_slice(digest.as_bytes());
    }

    fn unique_subslice_offset(
        frame: &[u8],
        range: core::ops::Range<usize>,
        needle: &[u8],
    ) -> usize {
        let offsets = frame[range.clone()]
            .windows(needle.len())
            .enumerate()
            .filter_map(|(offset, candidate)| (candidate == needle).then_some(range.start + offset))
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 1, "fixture needle must occur exactly once");
        offsets[0]
    }

    #[test]
    fn prepared_roundtrips_for_both_modes_and_retains_exact_payload_order() {
        for mode in [
            RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
            RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
        ] {
            let snapshot = prepared(mode);
            let decoded =
                RemoteAgentAccessSnapshotV1::decode(snapshot.canonical_wire(), identity())
                    .unwrap_or_else(|error| panic!("Prepared restart decode rejected: {error}"));
            assert_eq!(decoded, snapshot);
            assert_eq!(
                decoded.phase(),
                RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
            );
            assert_eq!(decoded.sequence(), 1);
            assert_eq!(decoded.previous_snapshot_digest(), None);
            assert_eq!(decoded.mode(), mode);
            assert_eq!(
                decoded.request.canonical_wire(),
                snapshot.request.canonical_wire()
            );
            assert_eq!(
                decoded.fabric.canonical_wire(),
                snapshot.fabric.canonical_wire()
            );
            assert_eq!(
                decoded.predecessor.canonical_wire(),
                snapshot.predecessor.canonical_wire()
            );
            assert_eq!(
                decoded.descriptor_evidence.is_some(),
                mode == RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive
            );
            assert!(decoded.terminal.is_none());
            assert_eq!(decoded.snapshot_digest(), snapshot.snapshot_digest());
            assert!(decoded.canonical_wire().len() <= MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_BYTES);
        }
    }

    #[test]
    fn prepared_shape_requires_exact_mode_specific_descriptor_authority() {
        let mut active =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        active.include_descriptor = false;
        assert!(matches!(
            prepared_result(active),
            Err(RemoteAgentAccessStateError::InvalidDescriptorShape)
        ));

        let mut local =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        local.include_descriptor = true;
        assert!(matches!(
            prepared_result(local),
            Err(RemoteAgentAccessStateError::InvalidDescriptorShape)
        ));

        let mut missing_pxst =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        missing_pxst.include_stack_terminal = false;
        assert!(matches!(
            prepared_result(missing_pxst),
            Err(RemoteAgentAccessStateError::InvalidAgentTerminal)
        ));
    }

    #[test]
    fn prepared_rejects_independently_valid_cross_carrier_and_cross_client_pxde() {
        for descriptor_scope in [DescriptorScope::CrossCarrier, DescriptorScope::CrossClient] {
            let mut options =
                PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
            options.descriptor_scope = descriptor_scope;
            assert!(matches!(
                prepared_result(options),
                Err(RemoteAgentAccessStateError::InvalidDescriptorShape)
            ));
        }
    }

    #[test]
    fn fresh_preparation_rejects_a_different_authenticated_inner_request() {
        let mut options =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        options.admitted_operation_id = Some(ApplyOperationId::from_bytes([0xa2; 16]));
        assert!(matches!(
            prepared_result(options),
            Err(RemoteAgentAccessStateError::AuthenticationMismatch)
        ));
    }

    #[test]
    fn prepared_pins_store_and_inherits_both_fabric_high_waters() {
        let mut options =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        options.fabric_generation_high_water = 19;
        options.stack_fabric_generation_high_water = 17;
        options.stack_agent_generation_high_water = 21;
        let snapshot = prepared_result(options)
            .unwrap_or_else(|error| panic!("high-water Prepared rejected: {error}"));
        assert_eq!(
            snapshot
                .snapshot()
                .generations()
                .fabric_generation_high_water,
            19
        );
        assert_eq!(
            snapshot
                .snapshot()
                .generations()
                .agent_generation_high_water,
            21
        );

        options.identity.store_instance_id = [0x45; 32];
        assert!(matches!(
            prepared_result(options),
            Err(RemoteAgentAccessStateError::IdentityMismatch)
        ));
    }

    #[test]
    fn phase_successor_matrix_is_exact_for_both_modes() {
        use RemoteAgentAccessDurablePhaseV1::{
            ActiveReady, AgentStartIntent, AgentStopIntent, FabricStartIntent, FabricStopIntent,
            LocalOnlyReady, NoEffectTerminal, PreparedNoEffects, QuarantineIntent, Quarantined,
            ReadyObservation, RemoteAccessStopIntent, Uncertain,
        };

        let phases = [
            PreparedNoEffects,
            AgentStopIntent,
            FabricStopIntent,
            FabricStartIntent,
            AgentStartIntent,
            ReadyObservation,
            NoEffectTerminal,
            ActiveReady,
            LocalOnlyReady,
            Uncertain,
            QuarantineIntent,
            Quarantined,
            RemoteAccessStopIntent,
        ];
        let active = [
            (PreparedNoEffects, AgentStopIntent),
            (PreparedNoEffects, NoEffectTerminal),
            (AgentStopIntent, FabricStopIntent),
            (AgentStopIntent, Uncertain),
            (AgentStopIntent, QuarantineIntent),
            (FabricStopIntent, FabricStartIntent),
            (FabricStopIntent, Uncertain),
            (FabricStopIntent, QuarantineIntent),
            (FabricStartIntent, AgentStartIntent),
            (FabricStartIntent, Uncertain),
            (FabricStartIntent, QuarantineIntent),
            (AgentStartIntent, ReadyObservation),
            (AgentStartIntent, Uncertain),
            (AgentStartIntent, QuarantineIntent),
            (ReadyObservation, Uncertain),
            (ReadyObservation, QuarantineIntent),
            (QuarantineIntent, Quarantined),
            (ReadyObservation, ActiveReady),
        ];
        let local = [
            (PreparedNoEffects, NoEffectTerminal),
            (PreparedNoEffects, RemoteAccessStopIntent),
            (RemoteAccessStopIntent, ReadyObservation),
            (RemoteAccessStopIntent, Uncertain),
            (RemoteAccessStopIntent, QuarantineIntent),
            (ReadyObservation, LocalOnlyReady),
            (ReadyObservation, Uncertain),
            (ReadyObservation, QuarantineIntent),
            (QuarantineIntent, Quarantined),
        ];
        for mode in [
            RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
            RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
        ] {
            for current in phases {
                for next in phases {
                    let expected = match mode {
                        RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive => {
                            active.contains(&(current, next))
                        }
                        RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate => {
                            local.contains(&(current, next))
                        }
                    };
                    assert_eq!(
                        valid_phase_successor(current, next, mode),
                        expected,
                        "unexpected matrix cell {mode:?}: {current:?} -> {next:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn authorized_methods_reject_cross_mode_and_skipped_successors() {
        let active = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = active.snapshot().generations();
        let reading = clock_at(active.snapshot(), 2);
        assert!(matches!(
            active.try_begin_effect_successor(
                RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                generations,
                reading,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));

        let local =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let generations = local.snapshot().generations();
        let reading = clock_at(local.snapshot(), 2);
        assert!(matches!(
            local.try_begin_effect_successor(
                RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
                generations,
                reading,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));

        let active = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = active.snapshot().generations();
        let agent_stop = effect_successor(
            active,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let generations = agent_stop.snapshot().generations();
        assert!(matches!(
            agent_stop.try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::ReadyObservation,
                generations,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));

        let local =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let generations = local.snapshot().generations();
        let remote_stop = effect_successor(
            local,
            RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
            generations,
        );
        assert!(matches!(
            remote_stop.try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::FabricStopIntent,
                generations,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));

        let active = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = active.snapshot().generations();
        let agent_stop = effect_successor(
            active,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        assert!(matches!(
            terminal_successor(
                agent_stop,
                RemoteAgentAccessDurablePhaseV1::NoEffectTerminal,
                RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));
    }

    #[test]
    fn complete_effect_chains_roundtrip_to_authenticated_ready_terminals() {
        for (mode, phase, outcome) in [
            (
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
                RemoteAgentAccessDurablePhaseV1::ActiveReady,
                RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
            ),
            (
                RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
                RemoteAgentAccessDurablePhaseV1::LocalOnlyReady,
                RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
            ),
        ] {
            let ready = ready_observation(mode);
            assert_eq!(
                ready.snapshot().phase(),
                RemoteAgentAccessDurablePhaseV1::ReadyObservation
            );
            let expected_ready_sequence = match mode {
                RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive => 6,
                RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate => 3,
            };
            assert_eq!(ready.snapshot().sequence(), expected_ready_sequence);
            let ready_digest = ready.snapshot().snapshot_digest();
            let terminal = terminal_successor(ready, phase, outcome)
                .unwrap_or_else(|error| panic!("ready terminal rejected: {error}"));
            assert_eq!(terminal.snapshot().phase(), phase);
            assert_eq!(terminal.snapshot().sequence(), expected_ready_sequence + 1);
            assert!(terminal.snapshot().terminal.is_some());
            assert_eq!(
                terminal.snapshot().previous_snapshot_digest(),
                Some(ready_digest)
            );
            decode_roundtrip(terminal.snapshot());
        }
    }

    #[test]
    fn local_only_path_preserves_base_generations_and_never_allocates_candidates() {
        let prepared =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let initial = prepared.snapshot().generations();
        assert!(initial.access_generation_candidate.is_none());
        assert!(initial.fabric_generation_candidate.is_none());
        assert!(initial.agent_generation_candidate.is_none());

        let remote_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
            initial,
        );
        assert_eq!(remote_stop.snapshot().generations(), initial);
        let ready = effect_successor(
            remote_stop,
            RemoteAgentAccessDurablePhaseV1::ReadyObservation,
            initial,
        );
        assert_eq!(ready.snapshot().generations(), initial);
        let terminal = terminal_successor(
            ready,
            RemoteAgentAccessDurablePhaseV1::LocalOnlyReady,
            RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
        )
        .unwrap_or_else(|error| panic!("LocalOnly terminal rejected: {error}"));
        assert_eq!(terminal.snapshot().generations(), initial);
        let active = terminal
            .snapshot()
            .predecessor
            .active
            .as_ref()
            .unwrap_or_else(|| panic!("LocalOnly predecessor must remain active"));
        let inner = inner_request(&terminal.snapshot().request)
            .unwrap_or_else(|error| panic!("LocalOnly terminal request rejected: {error}"));
        let state = terminal
            .snapshot()
            .terminal
            .as_ref()
            .unwrap_or_else(|| panic!("LocalOnly terminal must retain PXAU"))
            .validate_against_request(inner)
            .unwrap_or_else(|error| panic!("LocalOnly PXAU validation rejected: {error}"))
            .state();
        assert_eq!(state.fabric_generation(), Some(active.fabric_generation));
        assert_eq!(state.agent_generation(), Some(active.agent_generation));
        assert_eq!(state.access_generation(), None);

        let invalid =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let generations = invalid.snapshot().generations();
        let reading = clock_at(invalid.snapshot(), 2);
        assert!(matches!(
            invalid.try_begin_effect_successor(
                RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
                generations,
                reading,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));
    }

    #[test]
    fn local_only_authority_rejects_every_candidate_and_high_water_change() {
        let initial = prepared(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate)
            .generations();
        let access = next_generation(initial.access_generation_high_water);
        let fabric = next_generation(initial.fabric_generation_high_water);
        let agent = next_generation(initial.agent_generation_high_water);
        let mutations = [
            RemoteAgentAccessGenerationStateV1 {
                access_generation_high_water: access.value(),
                access_generation_candidate: Some(access),
                ..initial
            },
            RemoteAgentAccessGenerationStateV1 {
                fabric_generation_high_water: fabric.value(),
                fabric_generation_candidate: Some(fabric),
                ..initial
            },
            RemoteAgentAccessGenerationStateV1 {
                agent_generation_high_water: agent.value(),
                agent_generation_candidate: Some(agent),
                ..initial
            },
            RemoteAgentAccessGenerationStateV1 {
                access_generation_high_water: access.value(),
                ..initial
            },
            RemoteAgentAccessGenerationStateV1 {
                fabric_generation_high_water: fabric.value(),
                ..initial
            },
            RemoteAgentAccessGenerationStateV1 {
                agent_generation_high_water: agent.value(),
                ..initial
            },
        ];
        for generations in mutations {
            let prepared =
                prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
            let reading = clock_at(prepared.snapshot(), 2);
            assert!(matches!(
                prepared.try_begin_effect_successor(
                    RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                    generations,
                    reading,
                ),
                Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor)
            ));
        }
    }

    #[test]
    fn local_only_terminal_rejects_a_different_valid_base_generation() {
        for change_fabric in [true, false] {
            let ready =
                ready_observation(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
            let snapshot = ready.snapshot();
            let generations = snapshot.generations();
            let active = snapshot
                .predecessor
                .active
                .as_ref()
                .unwrap_or_else(|| panic!("LocalOnly predecessor must remain active"));
            let receipt = terminal_receipt_with_local_generations(
                snapshot,
                RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
                Some(if change_fabric {
                    (
                        next_generation(active.fabric_generation.value()),
                        active.agent_generation,
                    )
                } else {
                    (
                        active.fabric_generation,
                        next_generation(active.agent_generation.value()),
                    )
                }),
            );
            let authenticated = receipt
                .verify_runtime_terminal(
                    inner_request(&snapshot.request)
                        .unwrap_or_else(|error| panic!("LocalOnly request rejected: {error}")),
                    terminal_auth_claim(snapshot),
                    |_, _, _, _, _, signature| signature == [0xf4; 64],
                )
                .unwrap_or_else(|error| panic!("wrong-generation PXAU auth rejected: {error}"));
            assert!(matches!(
                ready.try_terminal_successor(
                    RemoteAgentAccessDurablePhaseV1::LocalOnlyReady,
                    generations,
                    authenticated,
                ),
                Err(RemoteAgentAccessStateError::InvalidTerminalShape)
            ));
        }
    }

    #[test]
    fn authenticated_no_effect_uncertain_and_quarantine_terminals_are_exact() {
        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let no_effect = terminal_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::NoEffectTerminal,
            RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        )
        .unwrap_or_else(|error| panic!("NoEffect terminal rejected: {error}"));
        assert_eq!(
            no_effect.snapshot().phase(),
            RemoteAgentAccessDurablePhaseV1::NoEffectTerminal
        );
        decode_roundtrip(no_effect.snapshot());

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let uncertain = terminal_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::Uncertain,
            RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain,
        )
        .unwrap_or_else(|error| panic!("Uncertain terminal rejected: {error}"));
        assert_eq!(
            uncertain.snapshot().phase(),
            RemoteAgentAccessDurablePhaseV1::Uncertain
        );
        decode_roundtrip(uncertain.snapshot());

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let generations = agent_stop.snapshot().generations();
        let quarantine_intent = effect_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::QuarantineIntent,
            generations,
        );
        let quarantined = terminal_successor(
            quarantine_intent,
            RemoteAgentAccessDurablePhaseV1::Quarantined,
            RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined,
        )
        .unwrap_or_else(|error| panic!("Quarantined terminal rejected: {error}"));
        assert_eq!(
            quarantined.snapshot().phase(),
            RemoteAgentAccessDurablePhaseV1::Quarantined
        );
        decode_roundtrip(quarantined.snapshot());

        assert!(matches!(
            terminal_successor(
                ready_observation(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive),
                RemoteAgentAccessDurablePhaseV1::Uncertain,
                RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
            ),
            Err(RemoteAgentAccessStateError::InvalidTerminalShape)
        ));
        let no_effect = terminal_successor(
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive),
            RemoteAgentAccessDurablePhaseV1::NoEffectTerminal,
            RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        )
        .unwrap_or_else(|error| panic!("NoEffect terminal rejected: {error}"));
        let generations = no_effect.snapshot().generations();
        assert!(matches!(
            no_effect.try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
                generations,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));
    }

    #[test]
    fn generation_candidates_allocate_once_at_their_exact_intent() {
        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let mut early = prepared.snapshot().generations();
        early.fabric_generation_candidate =
            Some(next_generation(early.fabric_generation_high_water));
        early.fabric_generation_high_water += 1;
        early.access_generation_candidate =
            Some(next_generation(early.access_generation_high_water));
        early.access_generation_high_water += 1;
        let fresh_clock = clock_at(prepared.snapshot(), 2);
        assert!(matches!(
            prepared.try_begin_effect_successor(
                RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
                early,
                fresh_clock,
            ),
            Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor)
        ));

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let generations = agent_stop.snapshot().generations();
        let fabric_stop = effect_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::FabricStopIntent,
            generations,
        );
        let mut skipped_fabric = fabric_start_generations(fabric_stop.snapshot());
        skipped_fabric.fabric_generation_high_water += 1;
        skipped_fabric.fabric_generation_candidate =
            Some(generation(skipped_fabric.fabric_generation_high_water));
        assert!(matches!(
            fabric_stop.try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::FabricStartIntent,
                skipped_fabric,
            ),
            Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor)
        ));

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let generations = agent_stop.snapshot().generations();
        let fabric_stop = effect_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::FabricStopIntent,
            generations,
        );
        let generations = fabric_start_generations(fabric_stop.snapshot());
        let fabric_start = effect_successor(
            fabric_stop,
            RemoteAgentAccessDurablePhaseV1::FabricStartIntent,
            generations,
        );
        let mut skipped_agent = agent_start_generations(fabric_start.snapshot());
        skipped_agent.agent_generation_high_water += 1;
        skipped_agent.agent_generation_candidate =
            Some(generation(skipped_agent.agent_generation_high_water));
        assert!(matches!(
            fabric_start.try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::AgentStartIntent,
                skipped_agent,
            ),
            Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor)
        ));

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let generations = agent_stop.snapshot().generations();
        let fabric_stop = effect_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::FabricStopIntent,
            generations,
        );
        let generations = fabric_start_generations(fabric_stop.snapshot());
        let fabric_start = effect_successor(
            fabric_stop,
            RemoteAgentAccessDurablePhaseV1::FabricStartIntent,
            generations,
        );
        let generations = agent_start_generations(fabric_start.snapshot());
        let agent_start = effect_successor(
            fabric_start,
            RemoteAgentAccessDurablePhaseV1::AgentStartIntent,
            generations,
        );
        let mut swapped = agent_start.snapshot().generations();
        swapped.fabric_generation_candidate =
            Some(next_generation(swapped.fabric_generation_high_water));
        swapped.fabric_generation_high_water += 1;
        assert!(matches!(
            agent_start
                .try_effect_successor(RemoteAgentAccessDurablePhaseV1::ReadyObservation, swapped,),
            Err(RemoteAgentAccessStateError::InvalidGenerationSuccessor)
        ));
    }

    #[test]
    fn preparation_and_first_effect_enforce_the_fresh_clock_window_only() {
        let template = data_plane_template();
        let temporal = template.temporal();
        let deadline = 1_u64
            .checked_add(temporal.remaining_budget().value())
            .unwrap_or_else(|| panic!("fixture deadline overflowed"));
        let drift_generation = ClockGeneration::try_new(
            temporal
                .target_clock_generation()
                .value()
                .checked_add(1)
                .unwrap_or_else(|| panic!("fixture clock generation exhausted")),
        )
        .unwrap_or_else(|error| panic!("drift clock generation rejected: {error}"));
        let invalid_readings = [
            ClockReading::new(
                temporal.target_clock_domain(),
                temporal.target_clock_generation(),
                MonotonicInstant::from_ticks(0),
            ),
            ClockReading::new(
                temporal.target_clock_domain(),
                temporal.target_clock_generation(),
                MonotonicInstant::from_ticks(deadline),
            ),
            ClockReading::new(
                temporal.target_clock_domain(),
                temporal.target_clock_generation(),
                MonotonicInstant::from_ticks(deadline + 1),
            ),
            ClockReading::new(
                ClockDomainRef::from_bytes([0xfe; 16]),
                temporal.target_clock_generation(),
                MonotonicInstant::from_ticks(2),
            ),
            ClockReading::new(
                temporal.target_clock_domain(),
                drift_generation,
                MonotonicInstant::from_ticks(2),
            ),
        ];
        let options =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        for reading in invalid_readings {
            assert!(matches!(
                prepared_result_after(options, None, Some(reading)),
                Err(RemoteAgentAccessStateError::InvalidTemporalWindow)
            ));
        }
        for reading in invalid_readings {
            let prepared =
                prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
            let generations = prepared.snapshot().generations();
            assert!(matches!(
                prepared.try_begin_effect_successor(
                    RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                    generations,
                    reading,
                ),
                Err(RemoteAgentAccessStateError::InvalidTemporalWindow)
            ));
        }

        let prepared =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let generations = prepared.snapshot().generations();
        assert!(matches!(
            prepared.try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                generations,
            ),
            Err(RemoteAgentAccessStateError::InvalidPhaseSuccessor)
        ));

        let prepared =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let generations = prepared.snapshot().generations();
        let reading = clock_at(prepared.snapshot(), 2);
        let remote_stop = prepared
            .try_begin_effect_successor(
                RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                generations,
                reading,
            )
            .unwrap_or_else(|error| panic!("fresh first effect rejected: {error}"));
        remote_stop
            .try_effect_successor(
                RemoteAgentAccessDurablePhaseV1::ReadyObservation,
                generations,
            )
            .unwrap_or_else(|error| panic!("post-effect cleanup was deadline-blocked: {error}"));
    }

    #[test]
    fn preparation_and_first_effect_accept_now_equal_to_admitted() {
        let prepared =
            prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let generations = prepared.snapshot().generations();
        let admitted_at = prepared.snapshot().admission.admitted_at_nanos;
        let reading = clock_at(prepared.snapshot(), admitted_at);
        prepared
            .try_begin_effect_successor(
                RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent,
                generations,
                reading,
            )
            .unwrap_or_else(|error| panic!("effect at the admission instant rejected: {error}"));
    }

    #[test]
    fn replacement_requires_an_authorized_safe_terminal_and_a_fresh_operation_id() {
        let default = PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        assert!(matches!(
            prepared_result_after(default, Some(no_effect_terminal(default)), None),
            Err(RemoteAgentAccessStateError::InvalidOperationReplacement)
        ));

        let mut same_id_different =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        same_id_different.operation_id = default.operation_id;
        assert!(matches!(
            prepared_result_after(same_id_different, Some(no_effect_terminal(default)), None,),
            Err(RemoteAgentAccessStateError::InvalidOperationReplacement)
        ));

        let previous = no_effect_terminal(default);
        let previous_sequence = previous.snapshot().sequence();
        let previous_digest = previous.snapshot().snapshot_digest();
        let mut fresh = default;
        fresh.operation_id = ApplyOperationId::from_bytes([0xa2; 16]);
        let replacement = prepared_result_after(fresh, Some(previous), None)
            .unwrap_or_else(|error| panic!("fresh-id safe replacement rejected: {error}"));
        assert_eq!(replacement.snapshot().sequence(), previous_sequence + 1);
        assert_eq!(
            replacement.snapshot().previous_snapshot_digest(),
            Some(previous_digest)
        );

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let uncertain = terminal_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::Uncertain,
            RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain,
        )
        .unwrap_or_else(|error| panic!("Uncertain replacement fixture rejected: {error}"));
        assert!(matches!(
            prepared_result_after(fresh, Some(uncertain), None),
            Err(RemoteAgentAccessStateError::InvalidOperationReplacement)
        ));

        let prepared = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let generations = prepared.snapshot().generations();
        let agent_stop = effect_successor(
            prepared,
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent,
            generations,
        );
        let generations = agent_stop.snapshot().generations();
        let quarantine = effect_successor(
            agent_stop,
            RemoteAgentAccessDurablePhaseV1::QuarantineIntent,
            generations,
        );
        let quarantined = terminal_successor(
            quarantine,
            RemoteAgentAccessDurablePhaseV1::Quarantined,
            RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined,
        )
        .unwrap_or_else(|error| panic!("Quarantined replacement fixture rejected: {error}"));
        assert!(matches!(
            prepared_result_after(fresh, Some(quarantined), None),
            Err(RemoteAgentAccessStateError::InvalidOperationReplacement)
        ));

        let local_ready =
            ready_observation(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let local_terminal = terminal_successor(
            local_ready,
            RemoteAgentAccessDurablePhaseV1::LocalOnlyReady,
            RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
        )
        .unwrap_or_else(|error| panic!("LocalOnly replacement fixture rejected: {error}"));
        let mut fresh_local =
            PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        fresh_local.operation_id = ApplyOperationId::from_bytes([0xa2; 16]);
        prepared_result_after(fresh_local, Some(local_terminal), None)
            .unwrap_or_else(|error| panic!("LocalOnly safe replacement rejected: {error}"));
    }

    #[test]
    fn replacement_rejects_each_previous_identity_pin_and_runtime_epoch() {
        fn assert_rejected(mutate: impl FnOnce(&mut RemoteAgentAccessSnapshotV1)) {
            let default =
                PreparedOptions::valid(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
            let mut previous = no_effect_terminal(default);
            mutate(&mut previous.snapshot);
            let mut fresh = default;
            fresh.operation_id = ApplyOperationId::from_bytes([0xa2; 16]);
            assert!(matches!(
                prepared_result_after(fresh, Some(previous), None),
                Err(RemoteAgentAccessStateError::InvalidOperationReplacement)
            ));
        }

        assert_rejected(|previous| previous.store_instance_id = [0x45; 32]);
        assert_rejected(|previous| {
            previous.target = RuntimeHostId::from_bytes([0xee; 16]);
        });
        assert_rejected(|previous| {
            previous.owner_target_fingerprint = Digest32::from_bytes([0x58; 32]);
        });
        assert_rejected(|previous| {
            previous.transition_projection_digest = Digest32::from_bytes([0x69; 32]);
        });
        assert_rejected(|previous| {
            previous.fabric_owner_target_fingerprint = Digest32::from_bytes([0x59; 32]);
        });
        assert_rejected(|previous| {
            previous.fabric_transition_projection_digest = Digest32::from_bytes([0x6a; 32]);
        });
        assert_rejected(|previous| previous.runtime_host_epoch = RUNTIME_EPOCH + 1);
    }

    #[test]
    fn outer_wire_bounds_flags_lengths_magic_and_checksum_fail_closed() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let wire = snapshot.canonical_wire();

        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&wire[..SNAPSHOT_HEADER_BYTES - 1], identity(),),
            Err(RemoteAgentAccessStateError::Truncated)
        ));
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(
                &vec![0; MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_BYTES + 1],
                identity(),
            ),
            Err(RemoteAgentAccessStateError::FrameTooLarge)
        ));

        let mut trailing = wire.to_vec();
        trailing.push(0);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&trailing, identity()),
            Err(RemoteAgentAccessStateError::InvalidLength)
        ));

        let mut checksum = wire.to_vec();
        *checksum
            .last_mut()
            .unwrap_or_else(|| panic!("snapshot fixture must be nonempty")) ^= 1;
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&checksum, identity()),
            Err(RemoteAgentAccessStateError::ChecksumMismatch)
        ));

        let mut flags = wire.to_vec();
        flags[30..32].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&flags, identity()),
            Err(RemoteAgentAccessStateError::InvalidFlags)
        ));

        let mut false_descriptor_flag = wire.to_vec();
        false_descriptor_flag[30..32]
            .copy_from_slice(&SNAPSHOT_HAS_DESCRIPTOR_EVIDENCE.to_be_bytes());
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&false_descriptor_flag, identity()),
            Err(RemoteAgentAccessStateError::InvalidFlags)
        ));

        let mut invalid_sequence = wire.to_vec();
        invalid_sequence[30..32].copy_from_slice(&SNAPSHOT_HAS_PREVIOUS.to_be_bytes());
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&invalid_sequence, identity()),
            Err(RemoteAgentAccessStateError::InvalidSequence)
        ));

        let mut length = wire.to_vec();
        length[8..12].copy_from_slice(
            &u32::try_from(wire.len() + 1)
                .unwrap_or_else(|_| panic!("fixture length must fit u32"))
                .to_be_bytes(),
        );
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&length, identity()),
            Err(RemoteAgentAccessStateError::InvalidLength)
        ));

        let mut magic = wire.to_vec();
        magic[0] = b'Q';
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&magic, identity()),
            Err(RemoteAgentAccessStateError::UnsupportedWire)
        ));
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(snapshot.fabric.canonical_wire(), identity()),
            Err(RemoteAgentAccessStateError::UnsupportedWire)
        ));

        let ranges = payload_ranges(wire);
        let mut nested_magic = wire.to_vec();
        nested_magic[ranges.request.start] = b'Q';
        reseal(&mut nested_magic);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&nested_magic, identity()),
            Err(RemoteAgentAccessStateError::InvalidNestedRequest)
        ));
    }

    #[test]
    fn checksum_resealed_active_agent_stop_decodes_only_raw_inert_with_no_authority() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let mut wire = snapshot.canonical_wire().to_vec();
        wire[28] = RemoteAgentAccessDurablePhaseV1::AgentStopIntent as u8;
        reseal(&mut wire);

        let decoded: RemoteAgentAccessSnapshotV1 =
            RemoteAgentAccessSnapshotV1::decode(&wire, identity())
                .unwrap_or_else(|error| panic!("raw Active intent decode rejected: {error}"));
        assert_eq!(
            decoded.phase(),
            RemoteAgentAccessDurablePhaseV1::AgentStopIntent
        );
        assert_eq!(decoded.canonical_wire(), wire);
    }

    #[test]
    fn checksum_resealed_local_remote_stop_decodes_only_raw_inert_with_no_authority() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
        let mut wire = snapshot.canonical_wire().to_vec();
        wire[28] = RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent as u8;
        reseal(&mut wire);

        let decoded: RemoteAgentAccessSnapshotV1 =
            RemoteAgentAccessSnapshotV1::decode(&wire, identity())
                .unwrap_or_else(|error| panic!("raw Local intent decode rejected: {error}"));
        assert_eq!(
            decoded.phase(),
            RemoteAgentAccessDurablePhaseV1::RemoteAccessStopIntent
        );
        assert_eq!(decoded.canonical_wire(), wire);
    }

    #[test]
    fn checksum_resealed_admission_shift_decodes_only_raw_inert_with_no_authority() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let mut wire = snapshot.canonical_wire().to_vec();
        let admitted = u64::from_be_bytes(
            wire[316..324]
                .try_into()
                .unwrap_or_else(|_| panic!("admitted-at field must be eight bytes")),
        );
        let deadline = u64::from_be_bytes(
            wire[324..332]
                .try_into()
                .unwrap_or_else(|_| panic!("deadline field must be eight bytes")),
        );
        let shifted_admitted = admitted + 7;
        let shifted_deadline = deadline + 7;
        wire[316..324].copy_from_slice(&shifted_admitted.to_be_bytes());
        wire[324..332].copy_from_slice(&shifted_deadline.to_be_bytes());
        reseal(&mut wire);

        let decoded: RemoteAgentAccessSnapshotV1 =
            RemoteAgentAccessSnapshotV1::decode(&wire, identity())
                .unwrap_or_else(|error| panic!("raw admission-shift decode rejected: {error}"));
        assert_eq!(decoded.admission.admitted_at_nanos, shifted_admitted);
        assert_eq!(decoded.admission.deadline_nanos, shifted_deadline);
        assert_eq!(shifted_deadline - shifted_admitted, deadline - admitted);
    }

    #[test]
    fn checksum_resealed_high_waters_decode_only_raw_inert_with_no_authority() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let initial = snapshot.generations();
        let mut wire = snapshot.canonical_wire().to_vec();
        let access = initial.access_generation_high_water + 1;
        let fabric = initial.fabric_generation_high_water + 1;
        let agent = initial.agent_generation_high_water + 1;
        wire[260..268].copy_from_slice(&access.to_be_bytes());
        wire[268..276].copy_from_slice(&fabric.to_be_bytes());
        wire[276..284].copy_from_slice(&agent.to_be_bytes());
        reseal(&mut wire);

        let decoded: RemoteAgentAccessSnapshotV1 =
            RemoteAgentAccessSnapshotV1::decode(&wire, identity())
                .unwrap_or_else(|error| panic!("raw high-water decode rejected: {error}"));
        assert_eq!(decoded.generations().access_generation_high_water, access);
        assert_eq!(decoded.generations().fabric_generation_high_water, fabric);
        assert_eq!(decoded.generations().agent_generation_high_water, agent);
    }

    #[test]
    fn nested_pxms_pxas_pxst_pxde_and_cas_tamper_fail_closed() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let wire = snapshot.canonical_wire();
        let ranges = payload_ranges(wire);

        let mut pxms = wire.to_vec();
        pxms[ranges.fabric.start + 136] ^= 1;
        reseal(&mut pxms);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&pxms, identity()),
            Err(RemoteAgentAccessStateError::Fabric(
                ManagedFabricStateError::ChecksumMismatch
            ))
        ));

        let mut pxas = wire.to_vec();
        pxas[ranges.predecessor.start + 160] ^= 1;
        reseal(&mut pxas);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&pxas, identity()),
            Err(RemoteAgentAccessStateError::Predecessor(
                ManagedAgentStackStateError::ChecksumMismatch
            ))
        ));

        let pxst_offset = unique_subslice_offset(wire, ranges.predecessor.clone(), b"PXST");
        let mut pxst = wire.to_vec();
        pxst[pxst_offset + 8] ^= 1;
        reseal(&mut pxst);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&pxst, identity()),
            Err(RemoteAgentAccessStateError::Predecessor(
                ManagedAgentStackStateError::ChecksumMismatch
            ))
        ));

        assert!(!ranges.descriptor.is_empty());
        let mut pxde = wire.to_vec();
        pxde[ranges.descriptor.end - 1] ^= 1;
        reseal(&mut pxde);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&pxde, identity()),
            Err(RemoteAgentAccessStateError::DescriptorEvidence(
                RemoteAgentDescriptorEvidenceError::ChecksumMismatch
            ))
        ));

        let cas = inner_request(&snapshot.request)
            .unwrap_or_else(|error| panic!("snapshot inner request rejected: {error}"))
            .target_execution()
            .bootstrap_cas()
            .unwrap_or_else(|| panic!("Active fixture must retain bootstrap CAS"));
        let cas_offset = unique_subslice_offset(
            wire,
            ranges.request.clone(),
            cas.expected_active_pxft_digest().as_bytes(),
        );
        let mut cas_tamper = wire.to_vec();
        cas_tamper[cas_offset] ^= 1;
        reseal(&mut cas_tamper);
        assert!(RemoteAgentAccessSnapshotV1::decode(&cas_tamper, identity()).is_err());
    }

    #[test]
    fn derived_admission_generation_identity_and_terminal_shape_tamper_fail_closed() {
        let prepared = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let wire = prepared.canonical_wire();

        let mut deadline = wire.to_vec();
        let encoded_deadline = u64::from_be_bytes(
            deadline[324..332]
                .try_into()
                .unwrap_or_else(|_| panic!("deadline field must be eight bytes")),
        );
        deadline[324..332].copy_from_slice(&(encoded_deadline + 1).to_be_bytes());
        reseal(&mut deadline);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&deadline, identity()),
            Err(RemoteAgentAccessStateError::InvalidAdmission)
        ));

        let mut request_digest = wire.to_vec();
        request_digest[332] ^= 1;
        reseal(&mut request_digest);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&request_digest, identity()),
            Err(RemoteAgentAccessStateError::InvalidAdmission)
        ));

        let mut generation = wire.to_vec();
        generation[268..276].copy_from_slice(&(FABRIC_GENERATION - 1).to_be_bytes());
        reseal(&mut generation);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&generation, identity()),
            Err(RemoteAgentAccessStateError::InvalidGenerationShape)
        ));

        let mut wrong_identity = identity();
        wrong_identity.owner_target_fingerprint = Digest32::from_bytes([0x58; 32]);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(wire, wrong_identity),
            Err(RemoteAgentAccessStateError::Predecessor(
                ManagedAgentStackStateError::IdentityMismatch
            ))
        ));

        let ready = ready_observation(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let terminal = terminal_successor(
            ready,
            RemoteAgentAccessDurablePhaseV1::ActiveReady,
            RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
        )
        .unwrap_or_else(|error| panic!("terminal fixture rejected: {error}"));
        let mut nonterminal_with_pxau = terminal.snapshot().canonical_wire().to_vec();
        nonterminal_with_pxau[28] = RemoteAgentAccessDurablePhaseV1::ReadyObservation as u8;
        reseal(&mut nonterminal_with_pxau);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&nonterminal_with_pxau, identity()),
            Err(RemoteAgentAccessStateError::InvalidTerminalShape)
        ));

        let terminal_ranges = payload_ranges(terminal.snapshot().canonical_wire());
        assert!(!terminal_ranges.terminal.is_empty());
        let mut cross_terminal = terminal.snapshot().canonical_wire().to_vec();
        cross_terminal[terminal_ranges.terminal.start] = b'Q';
        reseal(&mut cross_terminal);
        assert!(matches!(
            RemoteAgentAccessSnapshotV1::decode(&cross_terminal, identity()),
            Err(RemoteAgentAccessStateError::TerminalContract(_))
        ));
    }

    #[test]
    fn checksum_resealed_outer_signature_decodes_raw_inert_with_no_authority() {
        let snapshot = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
        let ranges = payload_ranges(snapshot.canonical_wire());
        let mut structurally_resigned = snapshot.canonical_wire().to_vec();
        structurally_resigned[ranges.request.end - 1] ^= 1;
        reseal(&mut structurally_resigned);
        let decoded = RemoteAgentAccessSnapshotV1::decode(&structurally_resigned, identity())
            .unwrap_or_else(|error| panic!("structural recovery rejected: {error}"));
        assert_ne!(
            decoded.request.canonical_wire(),
            snapshot.request.canonical_wire()
        );
        assert_eq!(decoded.admission, snapshot.admission);
        assert_eq!(
            decoded.phase(),
            RemoteAgentAccessDurablePhaseV1::PreparedNoEffects
        );
        assert!(decoded
            .request
            .verify_controller_request(decoded.request.carrier(), |_, _, _, _, signature| {
                signature == OUTER_SIGNATURE
            })
            .is_err());

        assert!(!terminal_generation_is_known(
            Some(generation(1)),
            None,
            None,
            1,
        ));
    }
}
