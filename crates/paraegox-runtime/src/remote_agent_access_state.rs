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
    time::{ClockDomainRef, ClockGeneration, ClockReading},
};
use paraegox_runtime_contracts::{
    managed_agent_stack_plan::ManagedAgentStackTerminalOutcomeV1,
    managed_fabric_plan::ManagedFabricApplyTerminalOutcomeV1,
    managed_service::ManagedServiceGeneration,
    remote_agent_access::{
        ControllerAuthenticatedRemoteAgentAccessRequestV1,
        MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES, MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES,
        RemoteAgentAccessKindV1, RemoteAgentAccessKindV2, RemoteAgentAccessRequestV1,
        RemoteAgentAccessRequestV2,
    },
    remote_agent_data_plane_plan::{
        MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES,
        MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES, RemoteAgentActiveS1CasV2,
        RemoteAgentDataPlaneApplyRequestV1, RemoteAgentDataPlaneApplyRequestV2,
        RemoteAgentDataPlaneDrainOutcomeV2, RemoteAgentDataPlanePlanError,
        RemoteAgentDataPlaneRemoteObservationV2, RemoteAgentDataPlaneTargetModeV1,
        RemoteAgentDataPlaneTargetModeV2, RemoteAgentDataPlaneTerminalLifecycleEffectV2,
        RemoteAgentDataPlaneTerminalOutcomeV1, RemoteAgentDataPlaneTerminalOutcomeV2,
        RemoteAgentDataPlaneTerminalPhaseV2, RemoteAgentDataPlaneTerminalReceiptV1,
        RemoteAgentDataPlaneTerminalReceiptV2, RemoteAgentRetainedS0CasV2,
        RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1,
        RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV2,
    },
};
use sha2::{Digest as ShaDigest, Sha256};

use crate::{
    admission::{
        VerifiedRemoteAgentAccessApplyIngressV2, VerifiedRemoteAgentDataPlaneApplyIngressV1,
    },
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
                        || previous.runtime_host_epoch != outer.expected_runtime_host_epoch()
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

// -----------------------------------------------------------------------------
// Additive Runtime-private PXRS v2 state for PXRA v2 / PXAR v11 / PXAU v2.
// -----------------------------------------------------------------------------

const SNAPSHOT_V2_VERSION: u16 = 2;
const SNAPSHOT_V2_HEADER_BYTES: usize = 1_314;
const SNAPSHOT_V2_DIGEST_BYTES: usize = 32;
const SNAPSHOT_V2_HAS_PREVIOUS: u16 = 1;
const SNAPSHOT_V2_HAS_ACTIVE_REQUEST: u16 = 1 << 1;
const SNAPSHOT_V2_HAS_ACTIVE_TERMINAL: u16 = 1 << 2;
const SNAPSHOT_V2_HAS_OPERATION_REQUEST: u16 = 1 << 3;
const SNAPSHOT_V2_HAS_OPERATION_TERMINAL: u16 = 1 << 4;
const SNAPSHOT_V2_SELF_HEAD: u16 = 1 << 5;
const SNAPSHOT_V2_KNOWN_FLAGS: u16 = SNAPSHOT_V2_HAS_PREVIOUS
    | SNAPSHOT_V2_HAS_ACTIVE_REQUEST
    | SNAPSHOT_V2_HAS_ACTIVE_TERMINAL
    | SNAPSHOT_V2_HAS_OPERATION_REQUEST
    | SNAPSHOT_V2_HAS_OPERATION_TERMINAL
    | SNAPSHOT_V2_SELF_HEAD;
const SNAPSHOT_V2_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-access-snapshot.sha256.v2";
const SNAPSHOT_V2_OUTER_AUTH_TRANSCRIPT_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-access-outer-auth-transcript.sha256.v2";
const SNAPSHOT_V2_TENURE_NONCE_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-tenure-nonce.sha256.v2";
const SNAPSHOT_V2_REQUEST_NONCE_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-request-nonce.sha256.v2";
const SNAPSHOT_V2_TEMPORAL_LINEAGE_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-temporal-lineage.sha256.v2";
const SNAPSHOT_V2_EXACT_ROUTE_BITMAP: u8 = 0b11;
const SNAPSHOT_V2_RETAINED_BINDING_CENSUS: u16 = 2;
const SNAPSHOT_V2_ED25519_ALGORITHM: u16 = 1;
const SNAPSHOT_V2_ED25519_ALGORITHM_VERSION: u16 = 1;
const SNAPSHOT_V2_ED25519_SIGNATURE_BYTES: usize = 64;

/// Exact canonical PXRS v2 ceiling. The two 7,855-byte PXRA v2 values and two
/// 1,195-byte canonical PXAU v2 values are independently bounded nested wires.
pub(crate) const MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES: usize = SNAPSHOT_V2_HEADER_BYTES
    + (2 * MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES)
    + (2 * MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES)
    + SNAPSHOT_V2_DIGEST_BYTES;

const _: [(); 19_446] = [(); MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub(crate) enum RemoteAgentAccessDurablePhaseV2 {
    InitializedAbsent = 1,
    PreparedNoEffects = 2,
    S1OpenIntent = 3,
    S1Opened = 4,
    SubmitDeclareIntent = 5,
    SubmitDeclared = 6,
    ControlDeclareIntent = 7,
    QueryablesDeclared = 8,
    ReadyObserved = 9,
    ActiveReady = 10,
    SubmitFenceIntent = 11,
    SubmitFenced = 12,
    ControlFenceIntent = 13,
    IngressFenced = 14,
    DrainIntent = 15,
    Drained = 16,
    SubmitJoinIntent = 17,
    SubmitJoined = 18,
    ControlJoinIntent = 19,
    WorkersJoined = 20,
    S1CloseIntent = 21,
    S1Closed = 22,
    LocalOnlyObserved = 23,
    LocalOnlyReady = 24,
    NoEffectTerminal = 25,
    Uncertain = 26,
    QuarantineIntent = 27,
    Quarantined = 28,
}

impl RemoteAgentAccessDurablePhaseV2 {
    fn decode(value: u8) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        match value {
            1 => Ok(Self::InitializedAbsent),
            2 => Ok(Self::PreparedNoEffects),
            3 => Ok(Self::S1OpenIntent),
            4 => Ok(Self::S1Opened),
            5 => Ok(Self::SubmitDeclareIntent),
            6 => Ok(Self::SubmitDeclared),
            7 => Ok(Self::ControlDeclareIntent),
            8 => Ok(Self::QueryablesDeclared),
            9 => Ok(Self::ReadyObserved),
            10 => Ok(Self::ActiveReady),
            11 => Ok(Self::SubmitFenceIntent),
            12 => Ok(Self::SubmitFenced),
            13 => Ok(Self::ControlFenceIntent),
            14 => Ok(Self::IngressFenced),
            15 => Ok(Self::DrainIntent),
            16 => Ok(Self::Drained),
            17 => Ok(Self::SubmitJoinIntent),
            18 => Ok(Self::SubmitJoined),
            19 => Ok(Self::ControlJoinIntent),
            20 => Ok(Self::WorkersJoined),
            21 => Ok(Self::S1CloseIntent),
            22 => Ok(Self::S1Closed),
            23 => Ok(Self::LocalOnlyObserved),
            24 => Ok(Self::LocalOnlyReady),
            25 => Ok(Self::NoEffectTerminal),
            26 => Ok(Self::Uncertain),
            27 => Ok(Self::QuarantineIntent),
            28 => Ok(Self::Quarantined),
            _ => Err(RemoteAgentAccessStateErrorV2::UnknownPhase),
        }
    }

    const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::ActiveReady
                | Self::LocalOnlyReady
                | Self::NoEffectTerminal
                | Self::Uncertain
                | Self::Quarantined
        )
    }

    const fn may_have_started(self) -> bool {
        !matches!(
            self,
            Self::InitializedAbsent | Self::PreparedNoEffects | Self::NoEffectTerminal
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum RemoteAgentAccessHeadKindV2 {
    Absent = 1,
    Active = 2,
}

impl RemoteAgentAccessHeadKindV2 {
    fn decode(value: u8) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        match value {
            1 => Ok(Self::Absent),
            2 => Ok(Self::Active),
            _ => Err(RemoteAgentAccessStateErrorV2::UnknownHead),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentAccessSnapshotIdentityPinsV2 {
    pub(crate) target: RuntimeHostId,
    pub(crate) store_instance_id: [u8; 32],
    pub(crate) owner_target_fingerprint: Digest32,
    pub(crate) transition_projection_digest: Digest32,
    pub(crate) lower_capability_projection_digest: Digest32,
}

/// Independently available identity required before startup may classify an
/// inert PXRS v2. The live lower-capability projection is deliberately absent:
/// D2 must bind that separate evidence when it mints current-final authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentAccessStaticIdentityPinsV2 {
    pub(crate) target: RuntimeHostId,
    pub(crate) store_instance_id: [u8; 32],
    pub(crate) owner_target_fingerprint: Digest32,
    pub(crate) transition_projection_digest: Digest32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RemoteAgentAccessAdmissionFactsV2 {
    clock_domain: ClockDomainRef,
    clock_generation: ClockGeneration,
    admitted_at_nanos: u64,
    absolute_deadline_nanos: u64,
    outer_request_digest: Digest32,
    outer_auth_transcript_digest: Digest32,
    inner_request_digest: Digest32,
    inner_envelope_request_digest: Digest32,
    inner_proof_envelope_digest: Digest32,
    tenure_nonce_identity: Digest32,
    request_nonce_identity: Digest32,
    temporal_lineage_identity: Digest32,
    carrier_binding_digest: Digest32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RemoteAgentAccessProgressFactsV2 {
    lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2,
    drain_outcome: RemoteAgentDataPlaneDrainOutcomeV2,
    remote_observation: RemoteAgentDataPlaneRemoteObservationV2,
    queryable_declared_bitmap: u8,
    ingress_fenced_bitmap: u8,
    worker_joined_bitmap: u8,
    public_phase: RemoteAgentDataPlaneTerminalPhaseV2,
    physical_binding_census: u16,
    selection_observed_at_nanos: u64,
    submit_admitted_count: u64,
    submit_terminalized_count: u64,
    control_admitted_count: u64,
    control_terminalized_count: u64,
    retained_s0_census_before_digest: Digest32,
    retained_s0_census_after_digest: Digest32,
    proxy_topology_compatibility_digest: Digest32,
    resource_census_digest: Digest32,
    raw_outcome_digest: Digest32,
}

/// Opaque D1 observation input. No production constructor exists until the D4
/// effect owner can mint it from a typed S1 observation. It therefore cannot
/// be used as a substitute for an effect token in this tranche.
pub(crate) struct RemoteAgentAccessObservedProgressV2 {
    next_phase: RemoteAgentAccessDurablePhaseV2,
    facts: RemoteAgentAccessProgressFactsV2,
    candidate_proxy_session_epoch: Option<[u8; 16]>,
    fresh_clock: Option<ClockReading>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteAgentAccessActiveHeadV2 {
    runtime_host_epoch: u64,
    snapshot_sequence: u64,
    snapshot_digest: Digest32,
    access_generation: ManagedServiceGeneration,
    proxy_session_epoch: [u8; 16],
    request: RemoteAgentAccessRequestV2,
    terminal: RemoteAgentDataPlaneTerminalReceiptV2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentAccessSnapshotV2 {
    identity: RemoteAgentAccessSnapshotIdentityPinsV2,
    sequence: u64,
    previous_snapshot_digest: Option<Digest32>,
    writer_runtime_host_epoch: u64,
    owner_slot_revision: u64,
    access_generation_high_water: u64,
    head_kind: RemoteAgentAccessHeadKindV2,
    active_head: Option<RemoteAgentAccessActiveHeadV2>,
    self_head: bool,
    active_access_generation: Option<ManagedServiceGeneration>,
    active_proxy_session_epoch: Option<[u8; 16]>,
    candidate_access_generation: Option<ManagedServiceGeneration>,
    candidate_proxy_session_epoch: Option<[u8; 16]>,
    mode: Option<RemoteAgentDataPlaneTargetModeV2>,
    phase: RemoteAgentAccessDurablePhaseV2,
    admission: Option<RemoteAgentAccessAdmissionFactsV2>,
    first_effect_observed_at_nanos: u64,
    progress: Option<RemoteAgentAccessProgressFactsV2>,
    submit_binding_epoch: u64,
    control_binding_epoch: u64,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    operation_request: Option<RemoteAgentAccessRequestV2>,
    operation_terminal: Option<RemoteAgentDataPlaneTerminalReceiptV2>,
    canonical_wire: Box<[u8]>,
    snapshot_digest: Digest32,
}

/// D2 will construct this non-cloneable marker only after exact named-final
/// readback and live S0/current-epoch verification. D1 deliberately exposes no
/// production constructor, so raw decode cannot acquire transition authority.
pub(crate) struct RemoteAgentCurrentFinalAccessSnapshotV2 {
    snapshot: RemoteAgentAccessSnapshotV2,
    current_identity: RemoteAgentAccessSnapshotIdentityPinsV2,
    current_runtime_host_epoch: u64,
    current_retained_s0_cas: RemoteAgentRetainedS0CasV2,
    current_retained_s0_census_digest: Digest32,
    current_submit_binding_epoch: u64,
    current_control_binding_epoch: u64,
}

/// Fresh-only authenticated request marker retained inside one authorized
/// transition. It is intentionally non-Clone and never comes from PXRS decode.
struct RemoteAgentFreshAccessRequestV2<'request> {
    request: &'request RemoteAgentAccessRequestV2,
    admission: RemoteAgentAccessAdmissionFactsV2,
}

/// Opaque, non-cloneable proof that D2's durable all-history replay registry
/// admitted this exact request against this exact current-final snapshot. D1
/// intentionally provides no production constructor: a bounded PXRS record
/// cannot honestly prove permanent A -> B -> A replay exclusion by itself.
pub(crate) struct RemoteAgentDurableReplayCheckedV2 {
    current_snapshot_digest: Digest32,
    operation_id: [u8; 16],
    tenure_nonce_identity: Digest32,
    request_nonce_identity: Digest32,
}

/// One fully-built successor awaiting D2 durable commit plus exact readback.
/// It deliberately has no transition API and is non-cloneable.
pub(crate) struct RemoteAgentPendingAccessSnapshotV2 {
    snapshot: RemoteAgentAccessSnapshotV2,
}

/// Non-cloneable D1 typestate. Its only input is a future D2 current-final
/// marker plus the complete Runtime-private PXAR11 admission marker.
pub(crate) struct RemoteAgentAuthorizedTransitionV2 {
    snapshot: RemoteAgentAccessSnapshotV2,
}

impl RemoteAgentAccessSnapshotV2 {
    /// Builds the structural sequence-one absent slot. Persistence and exact
    /// readback remain D2 responsibilities; this value alone has no authority.
    pub(crate) fn try_initialize_absent(
        identity: RemoteAgentAccessSnapshotIdentityPinsV2,
        writer_runtime_host_epoch: u64,
        retained_s0_cas: RemoteAgentRetainedS0CasV2,
        expected_s1_cas: RemoteAgentActiveS1CasV2,
        submit_binding_epoch: u64,
        control_binding_epoch: u64,
    ) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        if writer_runtime_host_epoch == 0
            || expected_s1_cas.active().is_some()
            || expected_s1_cas.access_generation_high_water() != 0
            || expected_s1_cas.owner_slot_revision() != 1
            || submit_binding_epoch == 0
            || control_binding_epoch == 0
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidInitialState);
        }
        Self::try_build(Self {
            identity,
            sequence: 1,
            previous_snapshot_digest: None,
            writer_runtime_host_epoch,
            owner_slot_revision: 1,
            access_generation_high_water: 0,
            head_kind: RemoteAgentAccessHeadKindV2::Absent,
            active_head: None,
            self_head: false,
            active_access_generation: None,
            active_proxy_session_epoch: None,
            candidate_access_generation: None,
            candidate_proxy_session_epoch: None,
            mode: None,
            phase: RemoteAgentAccessDurablePhaseV2::InitializedAbsent,
            admission: None,
            first_effect_observed_at_nanos: 0,
            progress: None,
            submit_binding_epoch,
            control_binding_epoch,
            retained_s0_cas,
            expected_s1_cas,
            operation_request: None,
            operation_terminal: None,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })
    }

    fn try_build(mut snapshot: Self) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        snapshot.validate_shape()?;
        let (canonical_wire, snapshot_digest) = snapshot.encode()?;
        snapshot.canonical_wire = canonical_wire;
        snapshot.snapshot_digest = snapshot_digest;
        Ok(snapshot)
    }

    fn encode(&self) -> Result<(Box<[u8]>, Digest32), RemoteAgentAccessStateErrorV2> {
        let active_request = self.active_head.as_ref().map(|head| &head.request);
        let active_terminal = self.active_head.as_ref().map(|head| &head.terminal);
        let active_request_length =
            checked_length_v2(active_request.map_or(0, |request| request.canonical_wire().len()))?;
        let active_terminal_length = checked_length_v2(
            active_terminal.map_or(0, |terminal| terminal.canonical_wire().len()),
        )?;
        let operation_request_length = checked_length_v2(
            self.operation_request
                .as_ref()
                .map_or(0, |request| request.canonical_wire().len()),
        )?;
        let operation_terminal_length = checked_length_v2(
            self.operation_terminal
                .as_ref()
                .map_or(0, |terminal| terminal.canonical_wire().len()),
        )?;
        let total_length = SNAPSHOT_V2_HEADER_BYTES
            .checked_add(active_request_length as usize)
            .and_then(|length| length.checked_add(active_terminal_length as usize))
            .and_then(|length| length.checked_add(operation_request_length as usize))
            .and_then(|length| length.checked_add(operation_terminal_length as usize))
            .and_then(|length| length.checked_add(SNAPSHOT_V2_DIGEST_BYTES))
            .ok_or(RemoteAgentAccessStateErrorV2::FrameTooLarge)?;
        if total_length > MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES {
            return Err(RemoteAgentAccessStateErrorV2::FrameTooLarge);
        }
        let total_length = u32::try_from(total_length)
            .map_err(|_| RemoteAgentAccessStateErrorV2::FrameTooLarge)?;
        let mut flags = 0_u16;
        if self.previous_snapshot_digest.is_some() {
            flags |= SNAPSHOT_V2_HAS_PREVIOUS;
        }
        if active_request.is_some() {
            flags |= SNAPSHOT_V2_HAS_ACTIVE_REQUEST;
        }
        if active_terminal.is_some() {
            flags |= SNAPSHOT_V2_HAS_ACTIVE_TERMINAL;
        }
        if self.operation_request.is_some() {
            flags |= SNAPSHOT_V2_HAS_OPERATION_REQUEST;
        }
        if self.operation_terminal.is_some() {
            flags |= SNAPSHOT_V2_HAS_OPERATION_TERMINAL;
        }
        if self.self_head {
            flags |= SNAPSHOT_V2_SELF_HEAD;
        }

        let progress = self.progress;
        let admission = self.admission;
        let (stable_head_runtime_host_epoch, stable_head_snapshot_sequence, stable_head_digest) =
            self.active_head
                .as_ref()
                .map_or((0, 0, zero_digest()), |head| {
                    (
                        head.runtime_host_epoch,
                        head.snapshot_sequence,
                        head.snapshot_digest,
                    )
                });
        let (active_request_digest, active_pxau_digest) = if self.self_head {
            (
                self.operation_request
                    .as_ref()
                    .map_or_else(zero_digest, RemoteAgentAccessRequestV2::request_digest),
                self.operation_terminal.as_ref().map_or_else(
                    zero_digest,
                    RemoteAgentDataPlaneTerminalReceiptV2::receipt_digest,
                ),
            )
        } else {
            self.active_head
                .as_ref()
                .map_or((zero_digest(), zero_digest()), |head| {
                    (
                        head.request.request_digest(),
                        head.terminal.receipt_digest(),
                    )
                })
        };
        let mut wire = Vec::with_capacity(total_length as usize);
        wire.extend_from_slice(SNAPSHOT_MAGIC);
        wire.extend_from_slice(&SNAPSHOT_V2_VERSION.to_be_bytes());
        wire.extend_from_slice(&(SNAPSHOT_V2_HEADER_BYTES as u16).to_be_bytes());
        wire.extend_from_slice(&total_length.to_be_bytes());
        wire.extend_from_slice(&flags.to_be_bytes());
        wire.push(self.phase as u8);
        wire.push(self.mode.map_or(0, |mode| mode as u8));
        wire.push(self.head_kind as u8);
        wire.push(progress.map_or(0, |facts| facts.lifecycle_effect as u8));
        wire.push(progress.map_or(0, |facts| facts.drain_outcome as u8));
        wire.push(progress.map_or(0, |facts| facts.remote_observation as u8));
        wire.push(progress.map_or(0, |facts| facts.queryable_declared_bitmap));
        wire.push(progress.map_or(0, |facts| facts.ingress_fenced_bitmap));
        wire.push(progress.map_or(0, |facts| facts.worker_joined_bitmap));
        wire.push(progress.map_or(0, |facts| facts.public_phase as u8));
        wire.extend_from_slice(
            &progress
                .map_or(0, |facts| facts.physical_binding_census)
                .to_be_bytes(),
        );
        for length in [
            active_request_length,
            active_terminal_length,
            operation_request_length,
            operation_terminal_length,
        ] {
            wire.extend_from_slice(&length.to_be_bytes());
        }
        for value in [
            self.sequence,
            self.writer_runtime_host_epoch,
            stable_head_runtime_host_epoch,
            self.owner_slot_revision,
            self.access_generation_high_water,
            encode_optional_generation_v2(self.active_access_generation),
            encode_optional_generation_v2(self.candidate_access_generation),
            stable_head_snapshot_sequence,
            admission.map_or(0, |facts| facts.clock_generation.value()),
            admission.map_or(0, |facts| facts.admitted_at_nanos),
            admission.map_or(0, |facts| facts.absolute_deadline_nanos),
            self.first_effect_observed_at_nanos,
            progress.map_or(0, |facts| facts.selection_observed_at_nanos),
            progress.map_or(0, |facts| facts.submit_admitted_count),
            progress.map_or(0, |facts| facts.submit_terminalized_count),
            progress.map_or(0, |facts| facts.control_admitted_count),
            progress.map_or(0, |facts| facts.control_terminalized_count),
            self.submit_binding_epoch,
            self.control_binding_epoch,
        ] {
            wire.extend_from_slice(&value.to_be_bytes());
        }
        wire.extend_from_slice(self.identity.target.as_bytes());
        wire.extend_from_slice(&self.identity.store_instance_id);
        wire.extend_from_slice(
            admission
                .map_or([0; 16], |facts| *facts.clock_domain.as_bytes())
                .as_slice(),
        );
        wire.extend_from_slice(&self.active_proxy_session_epoch.unwrap_or([0; 16]));
        wire.extend_from_slice(&self.candidate_proxy_session_epoch.unwrap_or([0; 16]));
        for digest in [
            self.identity.owner_target_fingerprint,
            self.identity.transition_projection_digest,
            self.identity.lower_capability_projection_digest,
            self.previous_snapshot_digest.unwrap_or_else(zero_digest),
            stable_head_digest,
            active_request_digest,
            active_pxau_digest,
            admission.map_or_else(zero_digest, |facts| facts.outer_request_digest),
            admission.map_or_else(zero_digest, |facts| facts.outer_auth_transcript_digest),
            admission.map_or_else(zero_digest, |facts| facts.inner_request_digest),
            admission.map_or_else(zero_digest, |facts| facts.inner_envelope_request_digest),
            admission.map_or_else(zero_digest, |facts| facts.inner_proof_envelope_digest),
            admission.map_or_else(zero_digest, |facts| facts.tenure_nonce_identity),
            admission.map_or_else(zero_digest, |facts| facts.request_nonce_identity),
            admission.map_or_else(zero_digest, |facts| facts.temporal_lineage_identity),
            admission.map_or_else(zero_digest, |facts| facts.carrier_binding_digest),
            progress.map_or_else(zero_digest, |facts| facts.retained_s0_census_before_digest),
            progress.map_or_else(zero_digest, |facts| facts.retained_s0_census_after_digest),
            progress.map_or_else(zero_digest, |facts| {
                facts.proxy_topology_compatibility_digest
            }),
            progress.map_or_else(zero_digest, |facts| facts.resource_census_digest),
            progress.map_or_else(zero_digest, |facts| facts.raw_outcome_digest),
        ] {
            wire.extend_from_slice(digest.as_bytes());
        }
        wire.extend_from_slice(self.retained_s0_cas.canonical_wire());
        wire.extend_from_slice(self.expected_s1_cas.canonical_wire());
        if wire.len() != SNAPSHOT_V2_HEADER_BYTES {
            return Err(RemoteAgentAccessStateErrorV2::NonCanonical);
        }
        if let Some(request) = active_request {
            wire.extend_from_slice(request.canonical_wire());
        }
        if let Some(terminal) = active_terminal {
            wire.extend_from_slice(terminal.canonical_wire());
        }
        if let Some(request) = &self.operation_request {
            wire.extend_from_slice(request.canonical_wire());
        }
        if let Some(terminal) = &self.operation_terminal {
            wire.extend_from_slice(terminal.canonical_wire());
        }
        let snapshot_digest = snapshot_digest_v2(&wire);
        wire.extend_from_slice(snapshot_digest.as_bytes());
        Ok((wire.into_boxed_slice(), snapshot_digest))
    }

    #[must_use]
    pub(crate) const fn phase(&self) -> RemoteAgentAccessDurablePhaseV2 {
        self.phase
    }

    #[must_use]
    pub(crate) const fn mode(&self) -> Option<RemoteAgentDataPlaneTargetModeV2> {
        self.mode
    }

    #[must_use]
    pub(crate) const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub(crate) const fn writer_runtime_host_epoch(&self) -> u64 {
        self.writer_runtime_host_epoch
    }

    #[must_use]
    pub(crate) const fn previous_snapshot_digest(&self) -> Option<Digest32> {
        self.previous_snapshot_digest
    }

    #[must_use]
    pub(crate) const fn owner_slot_revision(&self) -> u64 {
        self.owner_slot_revision
    }

    #[must_use]
    pub(crate) const fn access_generation_high_water(&self) -> u64 {
        self.access_generation_high_water
    }

    #[must_use]
    pub(crate) const fn candidate_access_generation(&self) -> Option<ManagedServiceGeneration> {
        self.candidate_access_generation
    }

    #[must_use]
    pub(crate) const fn candidate_proxy_session_epoch(&self) -> Option<[u8; 16]> {
        self.candidate_proxy_session_epoch
    }

    #[must_use]
    pub(crate) const fn snapshot_digest(&self) -> Digest32 {
        self.snapshot_digest
    }

    #[must_use]
    pub(crate) fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    fn validate_static_identity(
        &self,
        expected: RemoteAgentAccessStaticIdentityPinsV2,
    ) -> Result<(), RemoteAgentAccessStateErrorV2> {
        if self.identity.target != expected.target
            || self.identity.store_instance_id != expected.store_instance_id
            || self.identity.owner_target_fingerprint != expected.owner_target_fingerprint
            || self.identity.transition_projection_digest != expected.transition_projection_digest
        {
            return Err(RemoteAgentAccessStateErrorV2::IdentityMismatch);
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), RemoteAgentAccessStateErrorV2> {
        if self
            .identity
            .target
            .as_bytes()
            .iter()
            .all(|byte| *byte == 0)
            || self
                .identity
                .store_instance_id
                .iter()
                .all(|byte| *byte == 0)
            || digest_is_zero(self.identity.owner_target_fingerprint)
            || digest_is_zero(self.identity.transition_projection_digest)
            || digest_is_zero(self.identity.lower_capability_projection_digest)
            || self.sequence == 0
            || self.writer_runtime_host_epoch == 0
            || self.owner_slot_revision == 0
            || self.submit_binding_epoch == 0
            || self.control_binding_epoch == 0
            || self.expected_s1_cas.owner_slot_revision() == 0
            || self
                .retained_s0_cas
                .fields()
                .expected_fabric_generation
                .value()
                == 0
            || self
                .retained_s0_cas
                .fields()
                .expected_agent_generation
                .value()
                == 0
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidState);
        }
        match (self.sequence, self.previous_snapshot_digest) {
            (1, None) => {}
            (2.., Some(previous)) if !digest_is_zero(previous) => {}
            _ => return Err(RemoteAgentAccessStateErrorV2::InvalidSequence),
        }
        if self.active_access_generation.is_some() != self.active_proxy_session_epoch.is_some()
            || self.candidate_access_generation.is_some()
                != self.candidate_proxy_session_epoch.is_some()
            || self
                .active_proxy_session_epoch
                .is_some_and(|epoch| bytes_are_zero_v2(&epoch))
            || self
                .candidate_proxy_session_epoch
                .is_some_and(|epoch| bytes_are_zero_v2(&epoch))
            || self
                .active_access_generation
                .is_some_and(|generation| generation.value() > self.access_generation_high_water)
            || self
                .candidate_access_generation
                .is_some_and(|generation| generation.value() != self.access_generation_high_water)
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
        }
        if let Some(head) = &self.active_head {
            validate_active_head_v2(self, head)?;
            if self.active_access_generation != Some(head.access_generation)
                || self.active_proxy_session_epoch != Some(head.proxy_session_epoch)
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
            }
        } else if !self.self_head
            && (self.active_access_generation.is_some()
                || self.active_proxy_session_epoch.is_some())
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
        }
        if self.self_head
            && (self.phase != RemoteAgentAccessDurablePhaseV2::ActiveReady
                || self.head_kind != RemoteAgentAccessHeadKindV2::Active
                || self.active_head.is_some()
                || self.operation_request.is_none()
                || self.operation_terminal.is_none()
                || self.active_access_generation != self.candidate_access_generation
                || self.active_proxy_session_epoch != self.candidate_proxy_session_epoch)
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
        }
        if self.phase == RemoteAgentAccessDurablePhaseV2::InitializedAbsent {
            if self.sequence != 1
                || self.previous_snapshot_digest.is_some()
                || self.owner_slot_revision != 1
                || self.access_generation_high_water != 0
                || self.head_kind != RemoteAgentAccessHeadKindV2::Absent
                || self.active_head.is_some()
                || self.self_head
                || self.active_access_generation.is_some()
                || self.candidate_access_generation.is_some()
                || self.mode.is_some()
                || self.admission.is_some()
                || self.first_effect_observed_at_nanos != 0
                || self.progress.is_some()
                || self.operation_request.is_some()
                || self.operation_terminal.is_some()
                || self.expected_s1_cas.active().is_some()
                || self.expected_s1_cas.access_generation_high_water() != 0
                || self.expected_s1_cas.owner_slot_revision() != 1
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidInitialState);
            }
            return Ok(());
        }

        let mode = self
            .mode
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidState)?;
        let request = self
            .operation_request
            .as_ref()
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidOperationRequest)?;
        let inner = inner_request_v2(request)?;
        let admission = self
            .admission
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?;
        let progress = self
            .progress
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidProgress)?;
        if request.target() != self.identity.target
            || request.expected_runtime_store_instance_id() != self.identity.store_instance_id
            || request.expected_runtime_host_epoch() != self.writer_runtime_host_epoch
            || request.retained_s0_cas() != self.retained_s0_cas
            || request.expected_s1_cas() != self.expected_s1_cas
            || inner.target() != self.identity.target
            || inner.expected_runtime_store_instance_id() != self.identity.store_instance_id
            || inner.target_execution().mode() != mode
            || inner.target_execution().retained_s0_cas() != self.retained_s0_cas
            || inner.target_execution().expected_s1_cas() != self.expected_s1_cas
            || derive_admission_facts_v2(request, admission.admitted_at_nanos)? != admission
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidOperationRequest);
        }
        let first_effect_expected = self.phase.may_have_started();
        if first_effect_expected != (self.first_effect_observed_at_nanos != 0)
            || (first_effect_expected
                && (self.first_effect_observed_at_nanos < admission.admitted_at_nanos
                    || self.first_effect_observed_at_nanos >= admission.absolute_deadline_nanos))
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker);
        }
        validate_progress_shape_v2(self, progress)?;
        validate_mode_generation_shape_v2(self, mode)?;
        match (self.phase.is_terminal(), self.operation_terminal.as_ref()) {
            (false, None) => {}
            (true, Some(terminal)) => validate_terminal_shape_v2(self, terminal)?,
            _ => return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape),
        }
        Ok(())
    }

    fn resolved_current_s1_cas(
        &self,
    ) -> Result<RemoteAgentActiveS1CasV2, RemoteAgentAccessStateErrorV2> {
        match self.phase {
            RemoteAgentAccessDurablePhaseV2::InitializedAbsent
            | RemoteAgentAccessDurablePhaseV2::LocalOnlyReady => {
                RemoteAgentActiveS1CasV2::try_expect_absent(
                    self.access_generation_high_water,
                    self.owner_slot_revision,
                )
                .map_err(RemoteAgentAccessStateErrorV2::Contract)
            }
            RemoteAgentAccessDurablePhaseV2::NoEffectTerminal
                if self.head_kind == RemoteAgentAccessHeadKindV2::Absent =>
            {
                RemoteAgentActiveS1CasV2::try_expect_absent(
                    self.access_generation_high_water,
                    self.owner_slot_revision,
                )
                .map_err(RemoteAgentAccessStateErrorV2::Contract)
            }
            RemoteAgentAccessDurablePhaseV2::ActiveReady => {
                let request = self
                    .operation_request
                    .as_ref()
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                let terminal = self
                    .operation_terminal
                    .as_ref()
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                let access_generation = self
                    .active_access_generation
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                let proxy_session_epoch = self
                    .active_proxy_session_epoch
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                RemoteAgentActiveS1CasV2::try_expect_active(
                    self.access_generation_high_water,
                    self.owner_slot_revision,
                    paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentActiveS1FieldsV2 {
                        active_pxau_digest: terminal.receipt_digest(),
                        active_request_digest: request.request_digest(),
                        active_snapshot_digest: self.snapshot_digest,
                        active_snapshot_sequence: self.sequence,
                        active_access_generation: access_generation,
                        active_proxy_session_epoch: proxy_session_epoch,
                    },
                )
                .map_err(RemoteAgentAccessStateErrorV2::Contract)
            }
            RemoteAgentAccessDurablePhaseV2::NoEffectTerminal
                if self.head_kind == RemoteAgentAccessHeadKindV2::Active =>
            {
                let head = self
                    .active_head
                    .as_ref()
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                RemoteAgentActiveS1CasV2::try_expect_active(
                    self.access_generation_high_water,
                    self.owner_slot_revision,
                    paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentActiveS1FieldsV2 {
                        active_pxau_digest: head.terminal.receipt_digest(),
                        active_request_digest: head.request.request_digest(),
                        active_snapshot_digest: head.snapshot_digest,
                        active_snapshot_sequence: head.snapshot_sequence,
                        active_access_generation: head.access_generation,
                        active_proxy_session_epoch: head.proxy_session_epoch,
                    },
                )
                .map_err(RemoteAgentAccessStateErrorV2::Contract)
            }
            RemoteAgentAccessDurablePhaseV2::Uncertain
            | RemoteAgentAccessDurablePhaseV2::Quarantined
            | RemoteAgentAccessDurablePhaseV2::QuarantineIntent => {
                Err(RemoteAgentAccessStateErrorV2::ReconcileRequired)
            }
            _ => Err(RemoteAgentAccessStateErrorV2::OperationInProgress),
        }
    }
}

impl RemoteAgentCurrentFinalAccessSnapshotV2 {
    /// Test-only stand-in for the exact D2 store/readback marker. Keeping this
    /// constructor out of production is the D1 proof that raw decode is inert.
    #[cfg(test)]
    fn from_exact_readback_for_test(
        snapshot: RemoteAgentAccessSnapshotV2,
        current_identity: RemoteAgentAccessSnapshotIdentityPinsV2,
        current_runtime_host_epoch: u64,
        current_retained_s0_cas: RemoteAgentRetainedS0CasV2,
        current_retained_s0_census_digest: Digest32,
        current_submit_binding_epoch: u64,
        current_control_binding_epoch: u64,
    ) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        if current_runtime_host_epoch == 0
            || current_identity
                .target
                .as_bytes()
                .iter()
                .all(|byte| *byte == 0)
            || current_identity
                .store_instance_id
                .iter()
                .all(|byte| *byte == 0)
            || digest_is_zero(current_identity.owner_target_fingerprint)
            || digest_is_zero(current_identity.transition_projection_digest)
            || digest_is_zero(current_identity.lower_capability_projection_digest)
            || digest_is_zero(current_retained_s0_census_digest)
            || current_submit_binding_epoch == 0
            || current_control_binding_epoch == 0
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker);
        }
        Ok(Self {
            snapshot,
            current_identity,
            current_runtime_host_epoch,
            current_retained_s0_cas,
            current_retained_s0_census_digest,
            current_submit_binding_epoch,
            current_control_binding_epoch,
        })
    }

    fn validate_marker(&self) -> Result<(), RemoteAgentAccessStateErrorV2> {
        if self.snapshot.identity != self.current_identity
            || self.snapshot.writer_runtime_host_epoch != self.current_runtime_host_epoch
            || self.snapshot.retained_s0_cas != self.current_retained_s0_cas
            || self.snapshot.submit_binding_epoch != self.current_submit_binding_epoch
            || self.snapshot.control_binding_epoch != self.current_control_binding_epoch
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker);
        }
        Ok(())
    }

    /// Consumes current-final authority and creates a fresh Prepared snapshot.
    /// CAS mismatches return before a successor value exists; consequently D2
    /// has no state write and PXAU v2 has no producer input on that path.
    pub(crate) fn try_authorize_fresh(
        self,
        verified_ingress: VerifiedRemoteAgentAccessApplyIngressV2<'_>,
        durable_replay_checked: RemoteAgentDurableReplayCheckedV2,
    ) -> Result<RemoteAgentPendingAccessSnapshotV2, RemoteAgentAccessStateErrorV2> {
        self.validate_marker()?;
        let current = self.snapshot;
        let fresh = bind_fresh_request_v2(verified_ingress, self.current_runtime_host_epoch)?;
        let request = fresh.request;
        if request.kind() != RemoteAgentAccessKindV2::ApplyRemoteAccess
            || request.target() != current.identity.target
            || request.expected_runtime_store_instance_id() != current.identity.store_instance_id
            || request.expected_runtime_host_epoch() != self.current_runtime_host_epoch
            || request.retained_s0_cas() != self.current_retained_s0_cas
            || request.expected_s1_cas() != current.resolved_current_s1_cas()?
        {
            return Err(RemoteAgentAccessStateErrorV2::CasMismatch);
        }
        durable_replay_checked.validate(&current, &fresh)?;
        validate_fresh_replay_fence_v2(&current, &fresh)?;
        let inner = inner_request_v2(request)?;
        let mode = inner.target_execution().mode();
        let current_head = current.resolved_active_head_for_replacement()?;
        match (mode, current_head.as_ref()) {
            (RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive, None)
            | (RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate, Some(_)) => {}
            _ => return Err(RemoteAgentAccessStateErrorV2::CasMismatch),
        }
        let sequence = current
            .sequence
            .checked_add(1)
            .ok_or(RemoteAgentAccessStateErrorV2::SequenceExhausted)?;
        let (head_kind, active_access_generation, active_proxy_session_epoch) = current_head
            .as_ref()
            .map_or((RemoteAgentAccessHeadKindV2::Absent, None, None), |head| {
                (
                    RemoteAgentAccessHeadKindV2::Active,
                    Some(head.access_generation),
                    Some(head.proxy_session_epoch),
                )
            });
        let progress = RemoteAgentAccessProgressFactsV2 {
            lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::ProvenNotStarted,
            drain_outcome: RemoteAgentDataPlaneDrainOutcomeV2::NotStarted,
            remote_observation: match head_kind {
                RemoteAgentAccessHeadKindV2::Absent => {
                    RemoteAgentDataPlaneRemoteObservationV2::S1Absent
                }
                RemoteAgentAccessHeadKindV2::Active => {
                    RemoteAgentDataPlaneRemoteObservationV2::S1TlsExactRoutesReady
                }
            },
            queryable_declared_bitmap: if head_kind == RemoteAgentAccessHeadKindV2::Active {
                SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            } else {
                0
            },
            ingress_fenced_bitmap: 0,
            worker_joined_bitmap: 0,
            public_phase: RemoteAgentDataPlaneTerminalPhaseV2::PreparedNoEffects,
            physical_binding_census: SNAPSHOT_V2_RETAINED_BINDING_CENSUS,
            selection_observed_at_nanos: fresh.admission.admitted_at_nanos,
            submit_admitted_count: 0,
            submit_terminalized_count: 0,
            control_admitted_count: 0,
            control_terminalized_count: 0,
            retained_s0_census_before_digest: self.current_retained_s0_census_digest,
            retained_s0_census_after_digest: self.current_retained_s0_census_digest,
            proxy_topology_compatibility_digest: inner
                .target_execution()
                .proxy_topology_compatibility_digest(),
            resource_census_digest: zero_digest(),
            raw_outcome_digest: zero_digest(),
        };
        let snapshot = RemoteAgentAccessSnapshotV2::try_build(RemoteAgentAccessSnapshotV2 {
            identity: current.identity,
            sequence,
            previous_snapshot_digest: Some(current.snapshot_digest),
            writer_runtime_host_epoch: self.current_runtime_host_epoch,
            owner_slot_revision: current.owner_slot_revision,
            access_generation_high_water: current.access_generation_high_water,
            head_kind,
            active_head: current_head,
            self_head: false,
            active_access_generation,
            active_proxy_session_epoch,
            candidate_access_generation: None,
            candidate_proxy_session_epoch: None,
            mode: Some(mode),
            phase: RemoteAgentAccessDurablePhaseV2::PreparedNoEffects,
            admission: Some(fresh.admission),
            first_effect_observed_at_nanos: 0,
            progress: Some(progress),
            submit_binding_epoch: current.submit_binding_epoch,
            control_binding_epoch: current.control_binding_epoch,
            retained_s0_cas: request.retained_s0_cas(),
            expected_s1_cas: request.expected_s1_cas(),
            operation_request: Some((*fresh.request).clone()),
            operation_terminal: None,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })?;
        Ok(RemoteAgentPendingAccessSnapshotV2 { snapshot })
    }

    /// Re-mints one-edge authority only after D2 has committed and exactly read
    /// back a nonterminal Pending snapshot. No raw decode can call this seam.
    pub(crate) fn try_authorize_existing(
        self,
    ) -> Result<RemoteAgentAuthorizedTransitionV2, RemoteAgentAccessStateErrorV2> {
        self.validate_marker()?;
        if self.snapshot.phase == RemoteAgentAccessDurablePhaseV2::InitializedAbsent
            || self.snapshot.phase.is_terminal()
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidPhaseSuccessor);
        }
        Ok(RemoteAgentAuthorizedTransitionV2 {
            snapshot: self.snapshot,
        })
    }
}

impl RemoteAgentPendingAccessSnapshotV2 {
    #[must_use]
    pub(crate) const fn snapshot(&self) -> &RemoteAgentAccessSnapshotV2 {
        &self.snapshot
    }

    #[must_use]
    pub(crate) fn canonical_wire(&self) -> &[u8] {
        self.snapshot.canonical_wire()
    }
}

impl RemoteAgentDurableReplayCheckedV2 {
    fn validate(
        self,
        current: &RemoteAgentAccessSnapshotV2,
        fresh: &RemoteAgentFreshAccessRequestV2<'_>,
    ) -> Result<(), RemoteAgentAccessStateErrorV2> {
        let inner = inner_request_v2(fresh.request)?;
        if self.current_snapshot_digest != current.snapshot_digest
            || self.operation_id != *inner.operation_id().as_bytes()
            || self.tenure_nonce_identity != fresh.admission.tenure_nonce_identity
            || self.request_nonce_identity != fresh.admission.request_nonce_identity
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidReplayAuthority);
        }
        Ok(())
    }

    #[cfg(test)]
    fn from_durable_replay_ledger_for_test(
        current: &RemoteAgentAccessSnapshotV2,
        request: &RemoteAgentAccessRequestV2,
        admitted_at_nanos: u64,
        seen_operation_ids: &[[u8; 16]],
        seen_tenure_nonce_identities: &[Digest32],
        seen_request_nonce_identities: &[Digest32],
    ) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        let inner = inner_request_v2(request)?;
        let admission = derive_admission_facts_v2(request, admitted_at_nanos)?;
        let operation_id = *inner.operation_id().as_bytes();
        if seen_operation_ids.contains(&operation_id)
            || seen_tenure_nonce_identities.contains(&admission.tenure_nonce_identity)
            || seen_request_nonce_identities.contains(&admission.request_nonce_identity)
        {
            return Err(RemoteAgentAccessStateErrorV2::ReplayDetected);
        }
        Ok(Self {
            current_snapshot_digest: current.snapshot_digest,
            operation_id,
            tenure_nonce_identity: admission.tenure_nonce_identity,
            request_nonce_identity: admission.request_nonce_identity,
        })
    }
}

impl RemoteAgentAuthorizedTransitionV2 {
    #[must_use]
    pub(crate) const fn snapshot(&self) -> &RemoteAgentAccessSnapshotV2 {
        &self.snapshot
    }

    /// Commits one already-observed structural phase. D1 cannot construct the
    /// observation token in production; D4 will connect this seam to effects.
    pub(crate) fn try_observed_successor(
        self,
        observed: RemoteAgentAccessObservedProgressV2,
    ) -> Result<RemoteAgentPendingAccessSnapshotV2, RemoteAgentAccessStateErrorV2> {
        let current = &self.snapshot;
        if observed.next_phase.is_terminal()
            || !valid_phase_successor_v2(
                current.phase,
                observed.next_phase,
                current
                    .mode
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidState)?,
            )
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidPhaseSuccessor);
        }
        validate_progress_successor_v2(current, &observed)?;
        let mut owner_slot_revision = current.owner_slot_revision;
        let mut access_generation_high_water = current.access_generation_high_water;
        let mut candidate_access_generation = current.candidate_access_generation;
        let mut candidate_proxy_session_epoch = current.candidate_proxy_session_epoch;
        let mut first_effect_observed_at_nanos = current.first_effect_observed_at_nanos;
        match (current.mode, current.phase, observed.next_phase) {
            (
                Some(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive),
                RemoteAgentAccessDurablePhaseV2::PreparedNoEffects,
                RemoteAgentAccessDurablePhaseV2::S1OpenIntent,
            ) => {
                owner_slot_revision = owner_slot_revision
                    .checked_add(1)
                    .ok_or(RemoteAgentAccessStateErrorV2::RevisionExhausted)?;
                access_generation_high_water = access_generation_high_water
                    .checked_add(1)
                    .ok_or(RemoteAgentAccessStateErrorV2::GenerationExhausted)?;
                let epoch = observed
                    .candidate_proxy_session_epoch
                    .filter(|epoch| !bytes_are_zero_v2(epoch))
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidGenerationSuccessor)?;
                candidate_access_generation = Some(
                    ManagedServiceGeneration::try_new(access_generation_high_water)
                        .map_err(|_| RemoteAgentAccessStateErrorV2::GenerationExhausted)?,
                );
                candidate_proxy_session_epoch = Some(epoch);
                first_effect_observed_at_nanos = observed
                    .fresh_clock
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker)?
                    .now()
                    .value();
            }
            (
                Some(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate),
                RemoteAgentAccessDurablePhaseV2::PreparedNoEffects,
                RemoteAgentAccessDurablePhaseV2::SubmitFenceIntent,
            ) => {
                owner_slot_revision = owner_slot_revision
                    .checked_add(1)
                    .ok_or(RemoteAgentAccessStateErrorV2::RevisionExhausted)?;
                if observed.candidate_proxy_session_epoch.is_some() {
                    return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationSuccessor);
                }
                candidate_access_generation = None;
                candidate_proxy_session_epoch = None;
                first_effect_observed_at_nanos = observed
                    .fresh_clock
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker)?
                    .now()
                    .value();
            }
            _ if observed.candidate_proxy_session_epoch.is_some() => {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationSuccessor);
            }
            _ => {}
        }
        let sequence = current
            .sequence
            .checked_add(1)
            .ok_or(RemoteAgentAccessStateErrorV2::SequenceExhausted)?;
        let snapshot = RemoteAgentAccessSnapshotV2::try_build(RemoteAgentAccessSnapshotV2 {
            identity: current.identity,
            sequence,
            previous_snapshot_digest: Some(current.snapshot_digest),
            writer_runtime_host_epoch: current.writer_runtime_host_epoch,
            owner_slot_revision,
            access_generation_high_water,
            head_kind: current.head_kind,
            active_head: current.active_head.clone(),
            self_head: false,
            active_access_generation: current.active_access_generation,
            active_proxy_session_epoch: current.active_proxy_session_epoch,
            candidate_access_generation,
            candidate_proxy_session_epoch,
            mode: current.mode,
            phase: observed.next_phase,
            admission: current.admission,
            first_effect_observed_at_nanos,
            progress: Some(observed.facts),
            submit_binding_epoch: current.submit_binding_epoch,
            control_binding_epoch: current.control_binding_epoch,
            retained_s0_cas: current.retained_s0_cas,
            expected_s1_cas: current.expected_s1_cas,
            operation_request: current.operation_request.clone(),
            operation_terminal: None,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })?;
        Ok(RemoteAgentPendingAccessSnapshotV2 { snapshot })
    }

    /// Correlates an authenticated PXAU v2 and commits the one exact terminal
    /// phase. A raw or merely decoded receipt cannot enter this seam.
    pub(crate) fn try_terminal_successor(
        self,
        authenticated_terminal: RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV2<'_>,
    ) -> Result<RemoteAgentPendingAccessSnapshotV2, RemoteAgentAccessStateErrorV2> {
        let current = &self.snapshot;
        let terminal = authenticated_terminal.receipt().clone();
        let outer = current
            .operation_request
            .as_ref()
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidOperationRequest)?;
        if !terminal_auth_matches_outer_carrier_v2(&terminal, outer) {
            return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalAuthentication);
        }
        let request = inner_request_v2(outer)?;
        let facts = terminal
            .validate_against_request(request)
            .map_err(RemoteAgentAccessStateErrorV2::Contract)?;
        let next_phase = match facts.state().outcome() {
            RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady => {
                RemoteAgentAccessDurablePhaseV2::ActiveReady
            }
            RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady => {
                RemoteAgentAccessDurablePhaseV2::LocalOnlyReady
            }
            RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected => {
                RemoteAgentAccessDurablePhaseV2::NoEffectTerminal
            }
            RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain => {
                RemoteAgentAccessDurablePhaseV2::Uncertain
            }
            RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined => {
                RemoteAgentAccessDurablePhaseV2::Quarantined
            }
        };
        let mode = current
            .mode
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidState)?;
        if !valid_phase_successor_v2(current.phase, next_phase, mode) {
            return Err(RemoteAgentAccessStateErrorV2::InvalidPhaseSuccessor);
        }
        let sequence = current
            .sequence
            .checked_add(1)
            .ok_or(RemoteAgentAccessStateErrorV2::SequenceExhausted)?;
        let evidence = facts.evidence().fields();
        let progress = RemoteAgentAccessProgressFactsV2 {
            lifecycle_effect: facts.state().lifecycle_effect(),
            drain_outcome: evidence.drain_outcome,
            remote_observation: evidence.remote_observation,
            queryable_declared_bitmap: evidence.queryable_declared_bitmap,
            ingress_fenced_bitmap: evidence.ingress_fenced_bitmap,
            worker_joined_bitmap: evidence.worker_joined_bitmap,
            public_phase: facts.state().phase(),
            physical_binding_census: evidence.physical_binding_census,
            selection_observed_at_nanos: evidence.selection_observed_at_nanos,
            submit_admitted_count: evidence.submit_admitted_count,
            submit_terminalized_count: evidence.submit_terminalized_count,
            control_admitted_count: evidence.control_admitted_count,
            control_terminalized_count: evidence.control_terminalized_count,
            retained_s0_census_before_digest: evidence.retained_s0_census_before_digest,
            retained_s0_census_after_digest: evidence.retained_s0_census_after_digest,
            proxy_topology_compatibility_digest: evidence.proxy_topology_compatibility_digest,
            resource_census_digest: evidence.resource_census_digest,
            raw_outcome_digest: evidence.raw_outcome_digest,
        };
        validate_terminal_progress_successor_v2(current, next_phase, progress)?;
        let (head_kind, self_head, active_access_generation, active_proxy_session_epoch) =
            match next_phase {
                RemoteAgentAccessDurablePhaseV2::ActiveReady => (
                    RemoteAgentAccessHeadKindV2::Active,
                    true,
                    current.candidate_access_generation,
                    current.candidate_proxy_session_epoch,
                ),
                RemoteAgentAccessDurablePhaseV2::LocalOnlyReady => (
                    RemoteAgentAccessHeadKindV2::Absent,
                    false,
                    current.active_access_generation,
                    current.active_proxy_session_epoch,
                ),
                _ => (
                    current.head_kind,
                    false,
                    current.active_access_generation,
                    current.active_proxy_session_epoch,
                ),
            };
        let snapshot = RemoteAgentAccessSnapshotV2::try_build(RemoteAgentAccessSnapshotV2 {
            identity: current.identity,
            sequence,
            previous_snapshot_digest: Some(current.snapshot_digest),
            writer_runtime_host_epoch: current.writer_runtime_host_epoch,
            owner_slot_revision: current.owner_slot_revision,
            access_generation_high_water: current.access_generation_high_water,
            head_kind,
            active_head: current.active_head.clone(),
            self_head,
            active_access_generation,
            active_proxy_session_epoch,
            candidate_access_generation: current.candidate_access_generation,
            candidate_proxy_session_epoch: current.candidate_proxy_session_epoch,
            mode: current.mode,
            phase: next_phase,
            admission: current.admission,
            first_effect_observed_at_nanos: current.first_effect_observed_at_nanos,
            progress: Some(progress),
            submit_binding_epoch: current.submit_binding_epoch,
            control_binding_epoch: current.control_binding_epoch,
            retained_s0_cas: current.retained_s0_cas,
            expected_s1_cas: current.expected_s1_cas,
            operation_request: current.operation_request.clone(),
            operation_terminal: Some(terminal),
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })?;
        Ok(RemoteAgentPendingAccessSnapshotV2 { snapshot })
    }
}

impl RemoteAgentAccessSnapshotV2 {
    fn resolved_active_head_for_replacement(
        &self,
    ) -> Result<Option<RemoteAgentAccessActiveHeadV2>, RemoteAgentAccessStateErrorV2> {
        match self.phase {
            RemoteAgentAccessDurablePhaseV2::InitializedAbsent
            | RemoteAgentAccessDurablePhaseV2::LocalOnlyReady => Ok(None),
            RemoteAgentAccessDurablePhaseV2::NoEffectTerminal => match self.head_kind {
                RemoteAgentAccessHeadKindV2::Absent => Ok(None),
                RemoteAgentAccessHeadKindV2::Active => self
                    .active_head
                    .clone()
                    .map(Some)
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead),
            },
            RemoteAgentAccessDurablePhaseV2::ActiveReady => {
                let request = self
                    .operation_request
                    .clone()
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                let terminal = self
                    .operation_terminal
                    .clone()
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
                Ok(Some(RemoteAgentAccessActiveHeadV2 {
                    runtime_host_epoch: self.writer_runtime_host_epoch,
                    snapshot_sequence: self.sequence,
                    snapshot_digest: self.snapshot_digest,
                    access_generation: self
                        .active_access_generation
                        .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?,
                    proxy_session_epoch: self
                        .active_proxy_session_epoch
                        .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?,
                    request,
                    terminal,
                }))
            }
            RemoteAgentAccessDurablePhaseV2::Uncertain
            | RemoteAgentAccessDurablePhaseV2::QuarantineIntent
            | RemoteAgentAccessDurablePhaseV2::Quarantined => {
                Err(RemoteAgentAccessStateErrorV2::ReconcileRequired)
            }
            _ => Err(RemoteAgentAccessStateErrorV2::OperationInProgress),
        }
    }
}

fn validate_fresh_replay_fence_v2(
    current: &RemoteAgentAccessSnapshotV2,
    fresh: &RemoteAgentFreshAccessRequestV2<'_>,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let fresh_inner = inner_request_v2(fresh.request)?;
    let validate_prior = |prior_request: &RemoteAgentAccessRequestV2,
                          prior_admission: RemoteAgentAccessAdmissionFactsV2|
     -> Result<(), RemoteAgentAccessStateErrorV2> {
        let prior_inner = inner_request_v2(prior_request)?;
        if prior_inner.operation_id() == fresh_inner.operation_id() {
            return Err(RemoteAgentAccessStateErrorV2::InvalidOperationReplacement);
        }
        if prior_admission.tenure_nonce_identity == fresh.admission.tenure_nonce_identity
            || prior_admission.request_nonce_identity == fresh.admission.request_nonce_identity
        {
            return Err(RemoteAgentAccessStateErrorV2::ReplayDetected);
        }
        Ok(())
    };

    if let Some(prior_request) = current.operation_request.as_ref() {
        validate_prior(
            prior_request,
            current
                .admission
                .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?,
        )?;
    }
    if let Some(head) = current.active_head.as_ref()
        && current
            .operation_request
            .as_ref()
            .is_none_or(|prior| prior.request_digest() != head.request.request_digest())
    {
        let admitted_at_nanos = head.terminal.facts().evidence().fields().admitted_at_nanos;
        validate_prior(
            &head.request,
            derive_admission_facts_v2(&head.request, admitted_at_nanos)?,
        )?;
    }
    Ok(())
}

fn bind_fresh_request_v2<'request>(
    verified_ingress: VerifiedRemoteAgentAccessApplyIngressV2<'request>,
    current_runtime_host_epoch: u64,
) -> Result<RemoteAgentFreshAccessRequestV2<'request>, RemoteAgentAccessStateErrorV2> {
    let request = verified_ingress.request();
    let inner = inner_request_v2(request)?;
    let temporal = inner.temporal();
    let admitted_at_nanos = verified_ingress.admitted_at_nanos();
    let operation_timeout = inner.target_execution().profile().operation_timeout_nanos();
    if current_runtime_host_epoch == 0
        || request.expected_runtime_host_epoch() != current_runtime_host_epoch
        || request.target() != inner.target()
        || request.expected_runtime_store_instance_id()
            != inner.expected_runtime_store_instance_id()
        || request.retained_s0_cas() != inner.target_execution().retained_s0_cas()
        || request.expected_s1_cas() != inner.target_execution().expected_s1_cas()
        || admitted_at_nanos == 0
        || temporal.remaining_budget().value() < operation_timeout
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest);
    }
    let admission = derive_admission_facts_v2(request, admitted_at_nanos)?;
    let verified_admission = RemoteAgentAccessAdmissionFactsV2 {
        clock_domain: verified_ingress.clock_domain(),
        clock_generation: verified_ingress.clock_generation(),
        admitted_at_nanos,
        absolute_deadline_nanos: verified_ingress.deadline_nanos(),
        outer_request_digest: verified_ingress.outer_request_digest(),
        outer_auth_transcript_digest: verified_ingress.outer_auth_transcript_digest(),
        inner_request_digest: verified_ingress.inner_request_digest(),
        inner_envelope_request_digest: verified_ingress.inner_envelope_request_digest(),
        inner_proof_envelope_digest: verified_ingress.proof_envelope_digest(),
        tenure_nonce_identity: verified_ingress.tenure_nonce_identity(),
        request_nonce_identity: verified_ingress.request_nonce_identity(),
        temporal_lineage_identity: verified_ingress.temporal_lineage_identity(),
        carrier_binding_digest: verified_ingress.carrier_binding_digest(),
    };
    if admission != verified_admission {
        return Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest);
    }
    Ok(RemoteAgentFreshAccessRequestV2 { request, admission })
}

fn derive_admission_facts_v2(
    request: &RemoteAgentAccessRequestV2,
    admitted_at_nanos: u64,
) -> Result<RemoteAgentAccessAdmissionFactsV2, RemoteAgentAccessStateErrorV2> {
    let inner = inner_request_v2(request)?;
    let temporal = inner.temporal();
    let absolute_deadline_nanos = admitted_at_nanos
        .checked_add(inner.target_execution().profile().operation_timeout_nanos())
        .ok_or(RemoteAgentAccessStateErrorV2::DeadlineOverflow)?;
    if admitted_at_nanos == 0 || absolute_deadline_nanos <= admitted_at_nanos {
        return Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest);
    }
    let control = inner.control_commitment().control();
    let writer_context = control.writer_context();
    let proof = writer_context.proof();
    let proof_authority = proof.authority();
    let auth_claim = inner.authentication().claim();
    let source_scope = inner.provenance().source_scope();
    let outer_transcript = request
        .signing_transcript()
        .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?;
    Ok(RemoteAgentAccessAdmissionFactsV2 {
        clock_domain: temporal.target_clock_domain(),
        clock_generation: temporal.target_clock_generation(),
        admitted_at_nanos,
        absolute_deadline_nanos,
        outer_request_digest: request.request_digest(),
        outer_auth_transcript_digest: framed_digest_v2(
            SNAPSHOT_V2_OUTER_AUTH_TRANSCRIPT_DOMAIN,
            &[outer_transcript.as_bytes()],
        ),
        inner_request_digest: inner.request_digest(),
        inner_envelope_request_digest: inner.envelope_request_digest(),
        inner_proof_envelope_digest: proof
            .envelope_digest()
            .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?,
        tenure_nonce_identity: framed_digest_v2(
            SNAPSHOT_V2_TENURE_NONCE_DOMAIN,
            &[
                source_scope.as_bytes(),
                proof_authority.authority().as_bytes(),
                proof_authority.key().as_bytes(),
                proof.nonce(),
            ],
        ),
        request_nonce_identity: framed_digest_v2(
            SNAPSHOT_V2_REQUEST_NONCE_DOMAIN,
            &[
                source_scope.as_bytes(),
                inner.target().as_bytes(),
                auth_claim.principal().as_bytes(),
                writer_context.writer().as_bytes(),
                auth_claim.key().as_bytes(),
                auth_claim.nonce(),
            ],
        ),
        temporal_lineage_identity: framed_digest_v2(
            SNAPSHOT_V2_TEMPORAL_LINEAGE_DOMAIN,
            &[
                source_scope.as_bytes(),
                inner.target().as_bytes(),
                temporal.constraint_id().as_bytes(),
            ],
        ),
        carrier_binding_digest: request.carrier().binding_digest(),
    })
}

fn inner_request_v2(
    request: &RemoteAgentAccessRequestV2,
) -> Result<&RemoteAgentDataPlaneApplyRequestV2, RemoteAgentAccessStateErrorV2> {
    if request.kind() != RemoteAgentAccessKindV2::ApplyRemoteAccess {
        return Err(RemoteAgentAccessStateErrorV2::NotApplyRequest);
    }
    request
        .apply_request()
        .ok_or(RemoteAgentAccessStateErrorV2::NotApplyRequest)
}

fn validate_clock_window_v2(
    admission: RemoteAgentAccessAdmissionFactsV2,
    reading: ClockReading,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let now = reading.now().value();
    if reading.domain() != admission.clock_domain
        || reading.generation() != admission.clock_generation
        || now < admission.admitted_at_nanos
        || now >= admission.absolute_deadline_nanos
    {
        return Err(RemoteAgentAccessStateErrorV2::DeadlineExpired);
    }
    Ok(())
}

fn valid_phase_successor_v2(
    current: RemoteAgentAccessDurablePhaseV2,
    next: RemoteAgentAccessDurablePhaseV2,
    mode: RemoteAgentDataPlaneTargetModeV2,
) -> bool {
    use RemoteAgentAccessDurablePhaseV2::{
        ActiveReady, ControlDeclareIntent, ControlFenceIntent, ControlJoinIntent, DrainIntent,
        Drained, IngressFenced, LocalOnlyObserved, LocalOnlyReady, NoEffectTerminal,
        PreparedNoEffects, QuarantineIntent, Quarantined, QueryablesDeclared, ReadyObserved,
        S1CloseIntent, S1Closed, S1OpenIntent, S1Opened, SubmitDeclareIntent, SubmitDeclared,
        SubmitFenceIntent, SubmitFenced, SubmitJoinIntent, SubmitJoined, Uncertain, WorkersJoined,
    };
    let exact = match mode {
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => matches!(
            (current, next),
            (PreparedNoEffects, NoEffectTerminal | S1OpenIntent)
                | (S1OpenIntent, S1Opened)
                | (S1Opened, SubmitDeclareIntent)
                | (SubmitDeclareIntent, SubmitDeclared)
                | (SubmitDeclared, ControlDeclareIntent)
                | (ControlDeclareIntent, QueryablesDeclared)
                | (QueryablesDeclared, ReadyObserved)
                | (ReadyObserved, ActiveReady)
        ),
        RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => matches!(
            (current, next),
            (PreparedNoEffects, NoEffectTerminal | SubmitFenceIntent)
                | (SubmitFenceIntent, SubmitFenced)
                | (SubmitFenced, ControlFenceIntent)
                | (ControlFenceIntent, IngressFenced)
                | (IngressFenced, DrainIntent)
                | (DrainIntent, Drained)
                | (Drained, SubmitJoinIntent)
                | (SubmitJoinIntent, SubmitJoined)
                | (SubmitJoined, ControlJoinIntent)
                | (ControlJoinIntent, WorkersJoined)
                | (WorkersJoined, S1CloseIntent)
                | (S1CloseIntent, S1Closed)
                | (S1Closed, LocalOnlyObserved)
                | (LocalOnlyObserved, LocalOnlyReady)
        ),
    };
    let failure_source = match mode {
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => matches!(
            current,
            S1OpenIntent
                | S1Opened
                | SubmitDeclareIntent
                | SubmitDeclared
                | ControlDeclareIntent
                | QueryablesDeclared
                | ReadyObserved
        ),
        RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => matches!(
            current,
            SubmitFenceIntent
                | SubmitFenced
                | ControlFenceIntent
                | IngressFenced
                | DrainIntent
                | Drained
                | SubmitJoinIntent
                | SubmitJoined
                | ControlJoinIntent
                | WorkersJoined
                | S1CloseIntent
                | S1Closed
                | LocalOnlyObserved
        ),
    };
    exact
        || (failure_source && matches!(next, Uncertain | QuarantineIntent))
        || matches!((current, next), (QuarantineIntent, Quarantined))
}

fn expected_public_phase_v2(
    phase: RemoteAgentAccessDurablePhaseV2,
) -> Option<RemoteAgentDataPlaneTerminalPhaseV2> {
    use RemoteAgentAccessDurablePhaseV2::{
        ActiveReady, ControlDeclareIntent, ControlFenceIntent, ControlJoinIntent, DrainIntent,
        Drained, IngressFenced, LocalOnlyObserved, LocalOnlyReady, NoEffectTerminal,
        PreparedNoEffects, QuarantineIntent, Quarantined, QueryablesDeclared, ReadyObserved,
        S1CloseIntent, S1Closed, S1OpenIntent, S1Opened, SubmitDeclareIntent, SubmitDeclared,
        SubmitFenceIntent, SubmitFenced, SubmitJoinIntent, SubmitJoined, WorkersJoined,
    };
    match phase {
        PreparedNoEffects | NoEffectTerminal => {
            Some(RemoteAgentDataPlaneTerminalPhaseV2::PreparedNoEffects)
        }
        S1OpenIntent | S1Opened => Some(RemoteAgentDataPlaneTerminalPhaseV2::S1OpenIntent),
        SubmitDeclareIntent | SubmitDeclared | ControlDeclareIntent | QueryablesDeclared => {
            Some(RemoteAgentDataPlaneTerminalPhaseV2::QueryablesDeclareIntent)
        }
        ReadyObserved | ActiveReady => Some(RemoteAgentDataPlaneTerminalPhaseV2::ReadyObservation),
        SubmitFenceIntent | SubmitFenced | ControlFenceIntent | IngressFenced => {
            Some(RemoteAgentDataPlaneTerminalPhaseV2::IngressFenceIntent)
        }
        DrainIntent | Drained | SubmitJoinIntent | SubmitJoined | ControlJoinIntent
        | WorkersJoined => Some(RemoteAgentDataPlaneTerminalPhaseV2::DrainIntent),
        S1CloseIntent | S1Closed => Some(RemoteAgentDataPlaneTerminalPhaseV2::S1CloseIntent),
        LocalOnlyObserved | LocalOnlyReady => {
            Some(RemoteAgentDataPlaneTerminalPhaseV2::LocalOnlyObservation)
        }
        QuarantineIntent | Quarantined => {
            Some(RemoteAgentDataPlaneTerminalPhaseV2::QuarantineIntent)
        }
        RemoteAgentAccessDurablePhaseV2::InitializedAbsent
        | RemoteAgentAccessDurablePhaseV2::Uncertain => None,
    }
}

fn validate_mode_generation_shape_v2(
    snapshot: &RemoteAgentAccessSnapshotV2,
    mode: RemoteAgentDataPlaneTargetModeV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let prior_high_water = snapshot.expected_s1_cas.access_generation_high_water();
    let prior_revision = snapshot.expected_s1_cas.owner_slot_revision();
    match mode {
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => {
            if snapshot.expected_s1_cas.active().is_some()
                || snapshot.active_head.is_some()
                || (!snapshot.self_head
                    && (snapshot.active_access_generation.is_some()
                        || snapshot.active_proxy_session_epoch.is_some()))
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
            }
            let effect_started = snapshot.phase.may_have_started();
            if effect_started {
                let expected_high_water = prior_high_water
                    .checked_add(1)
                    .ok_or(RemoteAgentAccessStateErrorV2::GenerationExhausted)?;
                if snapshot.access_generation_high_water != expected_high_water
                    || snapshot.owner_slot_revision
                        != prior_revision
                            .checked_add(1)
                            .ok_or(RemoteAgentAccessStateErrorV2::RevisionExhausted)?
                    || snapshot
                        .candidate_access_generation
                        .map(ManagedServiceGeneration::value)
                        != Some(expected_high_water)
                    || snapshot.candidate_proxy_session_epoch.is_none()
                {
                    return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
                }
            } else if snapshot.access_generation_high_water != prior_high_water
                || snapshot.owner_slot_revision != prior_revision
                || snapshot.candidate_access_generation.is_some()
                || snapshot.candidate_proxy_session_epoch.is_some()
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
            }
            if snapshot.phase == RemoteAgentAccessDurablePhaseV2::ActiveReady {
                if !snapshot.self_head
                    || snapshot.head_kind != RemoteAgentAccessHeadKindV2::Active
                    || snapshot.active_access_generation != snapshot.candidate_access_generation
                    || snapshot.active_proxy_session_epoch != snapshot.candidate_proxy_session_epoch
                {
                    return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
                }
            } else if snapshot.head_kind != RemoteAgentAccessHeadKindV2::Absent {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
            }
        }
        RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => {
            let expected_active = snapshot
                .expected_s1_cas
                .active()
                .ok_or(RemoteAgentAccessStateErrorV2::InvalidGenerationShape)?;
            let head = snapshot
                .active_head
                .as_ref()
                .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
            if snapshot.self_head
                || snapshot.access_generation_high_water != prior_high_water
                || snapshot.candidate_access_generation.is_some()
                || snapshot.candidate_proxy_session_epoch.is_some()
                || head.access_generation != expected_active.active_access_generation
                || head.proxy_session_epoch != expected_active.active_proxy_session_epoch
                || head.snapshot_digest != expected_active.active_snapshot_digest
                || head.snapshot_sequence != expected_active.active_snapshot_sequence
                || head.request.request_digest() != expected_active.active_request_digest
                || head.terminal.receipt_digest() != expected_active.active_pxau_digest
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
            }
            let effect_started = snapshot.phase.may_have_started();
            let expected_revision = if effect_started {
                prior_revision
                    .checked_add(1)
                    .ok_or(RemoteAgentAccessStateErrorV2::RevisionExhausted)?
            } else {
                prior_revision
            };
            if snapshot.owner_slot_revision != expected_revision {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
            }
            let expected_head = if snapshot.phase == RemoteAgentAccessDurablePhaseV2::LocalOnlyReady
            {
                RemoteAgentAccessHeadKindV2::Absent
            } else {
                RemoteAgentAccessHeadKindV2::Active
            };
            if snapshot.head_kind != expected_head {
                return Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape);
            }
        }
    }
    Ok(())
}

fn validate_progress_shape_v2(
    snapshot: &RemoteAgentAccessSnapshotV2,
    progress: RemoteAgentAccessProgressFactsV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let terminal_outcome = snapshot
        .operation_terminal
        .as_ref()
        .map(|terminal| terminal.facts().state().outcome());
    let s0_currentness_may_be_unknown = (snapshot.phase
        == RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
        && progress.remote_observation == RemoteAgentDataPlaneRemoteObservationV2::Unknown)
        || matches!(
            terminal_outcome,
            Some(
                RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected
                    | RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain
                    | RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined
            )
        );
    if (!s0_currentness_may_be_unknown
        && progress.physical_binding_census != SNAPSHOT_V2_RETAINED_BINDING_CENSUS)
        || progress.queryable_declared_bitmap & !SNAPSHOT_V2_EXACT_ROUTE_BITMAP != 0
        || progress.ingress_fenced_bitmap & !SNAPSHOT_V2_EXACT_ROUTE_BITMAP != 0
        || progress.worker_joined_bitmap & !SNAPSHOT_V2_EXACT_ROUTE_BITMAP != 0
        || progress.submit_terminalized_count > progress.submit_admitted_count
        || progress.control_terminalized_count > progress.control_admitted_count
        || progress.selection_observed_at_nanos == 0
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgress);
    }
    let admission = snapshot
        .admission
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?;
    if progress.selection_observed_at_nanos < admission.admitted_at_nanos {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgress);
    }
    let request = inner_request_v2(
        snapshot
            .operation_request
            .as_ref()
            .ok_or(RemoteAgentAccessStateErrorV2::InvalidOperationRequest)?,
    )?;
    if progress.proxy_topology_compatibility_digest
        != request
            .target_execution()
            .proxy_topology_compatibility_digest()
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgress);
    }
    let exact_retained_census_required = !s0_currentness_may_be_unknown;
    if exact_retained_census_required
        && (digest_is_zero(progress.retained_s0_census_before_digest)
            || progress.retained_s0_census_before_digest
                != progress.retained_s0_census_after_digest)
    {
        return Err(RemoteAgentAccessStateErrorV2::RetainedS0Changed);
    }
    let expected_effect = if snapshot.phase == RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
        || snapshot.phase == RemoteAgentAccessDurablePhaseV2::NoEffectTerminal
    {
        RemoteAgentDataPlaneTerminalLifecycleEffectV2::ProvenNotStarted
    } else {
        RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted
    };
    if progress.lifecycle_effect != expected_effect {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgress);
    }
    if snapshot.phase != RemoteAgentAccessDurablePhaseV2::Uncertain
        && expected_public_phase_v2(snapshot.phase) != Some(progress.public_phase)
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgress);
    }
    validate_phase_progress_milestone_v2(snapshot, progress)?;
    Ok(())
}

fn validate_phase_progress_milestone_v2(
    snapshot: &RemoteAgentAccessSnapshotV2,
    progress: RemoteAgentAccessProgressFactsV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    use RemoteAgentAccessDurablePhaseV2::{
        ActiveReady, ControlDeclareIntent, ControlFenceIntent, ControlJoinIntent, DrainIntent,
        Drained, IngressFenced, LocalOnlyObserved, LocalOnlyReady, QueryablesDeclared,
        ReadyObserved, S1CloseIntent, S1Closed, SubmitDeclareIntent, SubmitDeclared,
        SubmitFenceIntent, SubmitFenced, SubmitJoinIntent, SubmitJoined, WorkersJoined,
    };
    let valid = match snapshot.mode {
        Some(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive) => match snapshot.phase {
            SubmitDeclareIntent => progress.queryable_declared_bitmap == 0,
            SubmitDeclared | ControlDeclareIntent => progress.queryable_declared_bitmap == 0b01,
            QueryablesDeclared | ReadyObserved | ActiveReady => {
                progress.queryable_declared_bitmap == SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            }
            _ => true,
        },
        Some(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate) => match snapshot.phase {
            SubmitFenceIntent => {
                progress.queryable_declared_bitmap == SNAPSHOT_V2_EXACT_ROUTE_BITMAP
                    && progress.ingress_fenced_bitmap == 0
            }
            SubmitFenced | ControlFenceIntent => progress.ingress_fenced_bitmap == 0b01,
            IngressFenced | DrainIntent => {
                progress.ingress_fenced_bitmap == SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            }
            Drained | SubmitJoinIntent => {
                progress.ingress_fenced_bitmap == SNAPSHOT_V2_EXACT_ROUTE_BITMAP
                    && progress.drain_outcome == RemoteAgentDataPlaneDrainOutcomeV2::Drained
                    && progress.worker_joined_bitmap == 0
            }
            SubmitJoined | ControlJoinIntent => {
                progress.drain_outcome == RemoteAgentDataPlaneDrainOutcomeV2::Drained
                    && progress.worker_joined_bitmap == 0b01
            }
            WorkersJoined | S1CloseIntent | S1Closed | LocalOnlyObserved | LocalOnlyReady => {
                progress.drain_outcome == RemoteAgentDataPlaneDrainOutcomeV2::Drained
                    && progress.worker_joined_bitmap == SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            }
            _ => true,
        },
        None => false,
    };
    if valid {
        Ok(())
    } else {
        Err(RemoteAgentAccessStateErrorV2::InvalidProgress)
    }
}

fn validate_progress_successor_v2(
    current: &RemoteAgentAccessSnapshotV2,
    observed: &RemoteAgentAccessObservedProgressV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let current_progress = current
        .progress
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidProgress)?;
    if observed.facts.lifecycle_effect
        != RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted
        || expected_public_phase_v2(observed.next_phase) != Some(observed.facts.public_phase)
        || observed.facts.physical_binding_census != current_progress.physical_binding_census
        || observed.facts.retained_s0_census_before_digest
            != current_progress.retained_s0_census_before_digest
        || observed.facts.retained_s0_census_after_digest
            != current_progress.retained_s0_census_after_digest
        || observed.facts.proxy_topology_compatibility_digest
            != current_progress.proxy_topology_compatibility_digest
        || observed.facts.submit_admitted_count < current_progress.submit_admitted_count
        || observed.facts.submit_terminalized_count < current_progress.submit_terminalized_count
        || observed.facts.control_admitted_count < current_progress.control_admitted_count
        || observed.facts.control_terminalized_count < current_progress.control_terminalized_count
        || observed.facts.queryable_declared_bitmap | current_progress.queryable_declared_bitmap
            != observed.facts.queryable_declared_bitmap
        || observed.facts.ingress_fenced_bitmap | current_progress.ingress_fenced_bitmap
            != observed.facts.ingress_fenced_bitmap
        || observed.facts.worker_joined_bitmap | current_progress.worker_joined_bitmap
            != observed.facts.worker_joined_bitmap
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor);
    }
    let mode = current
        .mode
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidState)?;
    let requires_fresh_clock = matches!(
        (mode, observed.next_phase),
        (
            RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
            RemoteAgentAccessDurablePhaseV2::S1OpenIntent
                | RemoteAgentAccessDurablePhaseV2::SubmitDeclareIntent
                | RemoteAgentAccessDurablePhaseV2::ControlDeclareIntent
                | RemoteAgentAccessDurablePhaseV2::ReadyObserved
        ) | (
            RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
            RemoteAgentAccessDurablePhaseV2::SubmitFenceIntent
        )
    );
    match (requires_fresh_clock, observed.fresh_clock) {
        (true, Some(reading)) => {
            validate_clock_window_v2(
                current
                    .admission
                    .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?,
                reading,
            )?;
            if observed.facts.selection_observed_at_nanos != reading.now().value() {
                return Err(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker);
            }
        }
        (true, None) | (false, Some(_)) => {
            return Err(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker);
        }
        (false, None) => {}
    }
    Ok(())
}

fn validate_terminal_progress_successor_v2(
    current: &RemoteAgentAccessSnapshotV2,
    next_phase: RemoteAgentAccessDurablePhaseV2,
    terminal: RemoteAgentAccessProgressFactsV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let prior = current
        .progress
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidProgress)?;
    let bitmap_is_monotonic = |before: u8, after: u8| before | after == after;
    let digest_is_monotonic = |before: Digest32, after: Digest32| {
        digest_is_zero(before) || (!digest_is_zero(after) && before == after)
    };
    let currentness_may_become_unknown = matches!(
        next_phase,
        RemoteAgentAccessDurablePhaseV2::NoEffectTerminal
            | RemoteAgentAccessDurablePhaseV2::Uncertain
            | RemoteAgentAccessDurablePhaseV2::Quarantined
    );
    let physical_census_is_valid = terminal.physical_binding_census
        == prior.physical_binding_census
        || (currentness_may_become_unknown && terminal.physical_binding_census == 0);
    let after_census_is_valid = digest_is_monotonic(
        prior.retained_s0_census_after_digest,
        terminal.retained_s0_census_after_digest,
    ) || (currentness_may_become_unknown
        && digest_is_zero(terminal.retained_s0_census_after_digest));
    let drain_is_monotonic = match prior.drain_outcome {
        RemoteAgentDataPlaneDrainOutcomeV2::NotStarted => true,
        RemoteAgentDataPlaneDrainOutcomeV2::Drained => {
            terminal.drain_outcome == RemoteAgentDataPlaneDrainOutcomeV2::Drained
        }
        RemoteAgentDataPlaneDrainOutcomeV2::OutcomeUncertain => {
            terminal.drain_outcome == RemoteAgentDataPlaneDrainOutcomeV2::OutcomeUncertain
        }
    };
    if !physical_census_is_valid
        || terminal.proxy_topology_compatibility_digest != prior.proxy_topology_compatibility_digest
        || !digest_is_monotonic(
            prior.retained_s0_census_before_digest,
            terminal.retained_s0_census_before_digest,
        )
        || !after_census_is_valid
        || !digest_is_monotonic(
            prior.resource_census_digest,
            terminal.resource_census_digest,
        )
        || !digest_is_monotonic(prior.raw_outcome_digest, terminal.raw_outcome_digest)
        || !bitmap_is_monotonic(
            prior.queryable_declared_bitmap,
            terminal.queryable_declared_bitmap,
        )
        || !bitmap_is_monotonic(prior.ingress_fenced_bitmap, terminal.ingress_fenced_bitmap)
        || !bitmap_is_monotonic(prior.worker_joined_bitmap, terminal.worker_joined_bitmap)
        || terminal.submit_admitted_count < prior.submit_admitted_count
        || terminal.submit_terminalized_count < prior.submit_terminalized_count
        || terminal.control_admitted_count < prior.control_admitted_count
        || terminal.control_terminalized_count < prior.control_terminalized_count
        || terminal.submit_terminalized_count > terminal.submit_admitted_count
        || terminal.control_terminalized_count > terminal.control_admitted_count
        || terminal.selection_observed_at_nanos < prior.selection_observed_at_nanos
        || !drain_is_monotonic
        || (!currentness_may_become_unknown
            && prior.remote_observation != RemoteAgentDataPlaneRemoteObservationV2::Unknown
            && terminal.remote_observation == RemoteAgentDataPlaneRemoteObservationV2::Unknown)
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor);
    }
    if matches!(
        next_phase,
        RemoteAgentAccessDurablePhaseV2::ActiveReady
            | RemoteAgentAccessDurablePhaseV2::LocalOnlyReady
    ) && terminal != prior
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor);
    }
    Ok(())
}

fn terminal_auth_matches_outer_carrier_v2(
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    outer: &RemoteAgentAccessRequestV2,
) -> bool {
    let claim = terminal.authentication();
    let carrier = outer.carrier();
    claim.runtime_principal() == carrier.runtime_principal()
        && claim.key() == carrier.runtime_response_key()
        && claim.algorithm().value() == SNAPSHOT_V2_ED25519_ALGORITHM
        && claim.algorithm_version() == SNAPSHOT_V2_ED25519_ALGORITHM_VERSION
        && terminal.authentication_signature().len() == SNAPSHOT_V2_ED25519_SIGNATURE_BYTES
}

fn validate_active_head_v2(
    snapshot: &RemoteAgentAccessSnapshotV2,
    head: &RemoteAgentAccessActiveHeadV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let request = inner_request_v2(&head.request)?;
    let facts = head
        .terminal
        .validate_against_request(request)
        .map_err(RemoteAgentAccessStateErrorV2::Contract)?;
    let evidence = facts.evidence().fields();
    if head.runtime_host_epoch == 0
        || head.snapshot_sequence == 0
        || digest_is_zero(head.snapshot_digest)
        || head.request.target() != snapshot.identity.target
        || head.request.expected_runtime_store_instance_id() != snapshot.identity.store_instance_id
        || head.request.expected_runtime_host_epoch() != head.runtime_host_epoch
        || head.request.retained_s0_cas() != snapshot.retained_s0_cas
        || facts.state().outcome() != RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady
        || facts.state().access_generation() != Some(head.access_generation)
        || facts.state().proxy_session_epoch() != Some(head.proxy_session_epoch)
        || evidence.completion_runtime_host_epoch != head.runtime_host_epoch
        || evidence.completion_snapshot_sequence != head.snapshot_sequence
        || evidence.access_generation_high_water != head.access_generation.value()
        || !terminal_auth_matches_outer_carrier_v2(&head.terminal, &head.request)
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
    }
    Ok(())
}

fn validate_terminal_shape_v2(
    snapshot: &RemoteAgentAccessSnapshotV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    let outer = snapshot
        .operation_request
        .as_ref()
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidOperationRequest)?;
    let request = inner_request_v2(outer)?;
    let facts = terminal
        .validate_against_request(request)
        .map_err(RemoteAgentAccessStateErrorV2::Contract)?;
    let state = facts.state();
    let evidence = facts.evidence().fields();
    let expected_outcome = match snapshot.phase {
        RemoteAgentAccessDurablePhaseV2::ActiveReady => {
            RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady
        }
        RemoteAgentAccessDurablePhaseV2::LocalOnlyReady => {
            RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady
        }
        RemoteAgentAccessDurablePhaseV2::NoEffectTerminal => {
            RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected
        }
        RemoteAgentAccessDurablePhaseV2::Uncertain => {
            RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain
        }
        RemoteAgentAccessDurablePhaseV2::Quarantined => {
            RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined
        }
        _ => return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape),
    };
    let admission = snapshot
        .admission
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?;
    let progress = snapshot
        .progress
        .ok_or(RemoteAgentAccessStateErrorV2::InvalidProgress)?;
    let retained_s0_currentness_valid = match state.outcome() {
        RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady
        | RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady => {
            evidence.retained_s0_current_cas_digest == snapshot.retained_s0_cas.cas_digest()
                && !digest_is_zero(evidence.retained_s0_census_before_digest)
                && evidence.retained_s0_census_before_digest
                    == evidence.retained_s0_census_after_digest
        }
        RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected => {
            digest_is_zero(evidence.retained_s0_current_cas_digest)
                || evidence.retained_s0_current_cas_digest == snapshot.retained_s0_cas.cas_digest()
        }
        RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain
        | RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined => true,
    };
    if state.outcome() != expected_outcome
        || !terminal_auth_matches_outer_carrier_v2(terminal, outer)
        || !retained_s0_currentness_valid
        || evidence.completion_runtime_host_epoch != snapshot.writer_runtime_host_epoch
        || evidence.completion_snapshot_sequence != snapshot.sequence
        || evidence.completion_owner_slot_revision != snapshot.owner_slot_revision
        || evidence.access_generation_high_water != snapshot.access_generation_high_water
        || evidence.selection_clock_domain != admission.clock_domain
        || evidence.selection_clock_generation != admission.clock_generation
        || evidence.admitted_at_nanos != admission.admitted_at_nanos
        || evidence.absolute_deadline_nanos != admission.absolute_deadline_nanos
        || evidence.proxy_topology_compatibility_digest
            != progress.proxy_topology_compatibility_digest
        || evidence.selection_observed_at_nanos != progress.selection_observed_at_nanos
        || evidence.submit_admitted_count != progress.submit_admitted_count
        || evidence.submit_terminalized_count != progress.submit_terminalized_count
        || evidence.control_admitted_count != progress.control_admitted_count
        || evidence.control_terminalized_count != progress.control_terminalized_count
        || evidence.physical_binding_census != progress.physical_binding_census
        || evidence.queryable_declared_bitmap != progress.queryable_declared_bitmap
        || evidence.ingress_fenced_bitmap != progress.ingress_fenced_bitmap
        || evidence.worker_joined_bitmap != progress.worker_joined_bitmap
        || evidence.drain_outcome != progress.drain_outcome
        || evidence.remote_observation != progress.remote_observation
        || evidence.retained_s0_census_before_digest != progress.retained_s0_census_before_digest
        || evidence.retained_s0_census_after_digest != progress.retained_s0_census_after_digest
        || evidence.resource_census_digest != progress.resource_census_digest
        || evidence.raw_outcome_digest != progress.raw_outcome_digest
        || state.lifecycle_effect() != progress.lifecycle_effect
        || state.phase() != progress.public_phase
    {
        return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape);
    }
    match snapshot.phase {
        RemoteAgentAccessDurablePhaseV2::ActiveReady => {
            if state.access_generation() != snapshot.candidate_access_generation
                || state.proxy_session_epoch() != snapshot.candidate_proxy_session_epoch
                || snapshot.active_access_generation != snapshot.candidate_access_generation
                || snapshot.active_proxy_session_epoch != snapshot.candidate_proxy_session_epoch
                || evidence.selection_observed_at_nanos >= admission.absolute_deadline_nanos
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape);
            }
        }
        RemoteAgentAccessDurablePhaseV2::LocalOnlyReady => {
            if state.access_generation().is_some()
                || state.proxy_session_epoch().is_some()
                || snapshot.candidate_access_generation.is_some()
                || snapshot.candidate_proxy_session_epoch.is_some()
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape);
            }
        }
        RemoteAgentAccessDurablePhaseV2::NoEffectTerminal => {
            if snapshot.owner_slot_revision != snapshot.expected_s1_cas.owner_slot_revision()
                || snapshot.access_generation_high_water
                    != snapshot.expected_s1_cas.access_generation_high_water()
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape);
            }
        }
        RemoteAgentAccessDurablePhaseV2::Uncertain
        | RemoteAgentAccessDurablePhaseV2::Quarantined => {}
        _ => return Err(RemoteAgentAccessStateErrorV2::InvalidTerminalShape),
    }
    Ok(())
}

fn decode_mode_v2(
    value: u8,
) -> Result<RemoteAgentDataPlaneTargetModeV2, RemoteAgentAccessStateErrorV2> {
    match value {
        1 => Ok(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive),
        2 => Ok(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate),
        _ => Err(RemoteAgentAccessStateErrorV2::UnknownMode),
    }
}

fn decode_lifecycle_effect_v2(
    value: u8,
) -> Result<RemoteAgentDataPlaneTerminalLifecycleEffectV2, RemoteAgentAccessStateErrorV2> {
    match value {
        1 => Ok(RemoteAgentDataPlaneTerminalLifecycleEffectV2::ProvenNotStarted),
        2 => Ok(RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted),
        _ => Err(RemoteAgentAccessStateErrorV2::InvalidProgress),
    }
}

fn decode_drain_outcome_v2(
    value: u8,
) -> Result<RemoteAgentDataPlaneDrainOutcomeV2, RemoteAgentAccessStateErrorV2> {
    match value {
        1 => Ok(RemoteAgentDataPlaneDrainOutcomeV2::NotStarted),
        2 => Ok(RemoteAgentDataPlaneDrainOutcomeV2::Drained),
        3 => Ok(RemoteAgentDataPlaneDrainOutcomeV2::OutcomeUncertain),
        _ => Err(RemoteAgentAccessStateErrorV2::InvalidProgress),
    }
}

fn decode_remote_observation_v2(
    value: u8,
) -> Result<RemoteAgentDataPlaneRemoteObservationV2, RemoteAgentAccessStateErrorV2> {
    match value {
        1 => Ok(RemoteAgentDataPlaneRemoteObservationV2::Unknown),
        2 => Ok(RemoteAgentDataPlaneRemoteObservationV2::S1Absent),
        3 => Ok(RemoteAgentDataPlaneRemoteObservationV2::S1TlsExactRoutesReady),
        4 => Ok(RemoteAgentDataPlaneRemoteObservationV2::PartialOrConflicting),
        _ => Err(RemoteAgentAccessStateErrorV2::InvalidProgress),
    }
}

fn decode_public_phase_v2(
    value: u8,
) -> Result<RemoteAgentDataPlaneTerminalPhaseV2, RemoteAgentAccessStateErrorV2> {
    match value {
        1 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::PreparedNoEffects),
        2 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::S1OpenIntent),
        3 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::QueryablesDeclareIntent),
        4 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::ReadyObservation),
        5 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::IngressFenceIntent),
        6 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::DrainIntent),
        7 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::S1CloseIntent),
        8 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::LocalOnlyObservation),
        9 => Ok(RemoteAgentDataPlaneTerminalPhaseV2::QuarantineIntent),
        _ => Err(RemoteAgentAccessStateErrorV2::InvalidProgress),
    }
}

const fn encode_optional_generation_v2(value: Option<ManagedServiceGeneration>) -> u64 {
    match value {
        Some(value) => value.value(),
        None => 0,
    }
}

fn decode_optional_generation_v2(
    value: u64,
) -> Result<Option<ManagedServiceGeneration>, RemoteAgentAccessStateErrorV2> {
    if value == 0 {
        Ok(None)
    } else {
        ManagedServiceGeneration::try_new(value)
            .map(Some)
            .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidGenerationShape)
    }
}

fn decode_optional_epoch_v2(
    generation: Option<ManagedServiceGeneration>,
    epoch: [u8; 16],
) -> Result<Option<[u8; 16]>, RemoteAgentAccessStateErrorV2> {
    match (generation.is_some(), bytes_are_zero_v2(&epoch)) {
        (false, true) => Ok(None),
        (true, false) => Ok(Some(epoch)),
        _ => Err(RemoteAgentAccessStateErrorV2::InvalidGenerationShape),
    }
}

fn checked_length_v2(length: usize) -> Result<u32, RemoteAgentAccessStateErrorV2> {
    u32::try_from(length).map_err(|_| RemoteAgentAccessStateErrorV2::FrameTooLarge)
}

fn validate_presence_flag_v2(
    flags: u16,
    flag: u16,
    present: bool,
) -> Result<(), RemoteAgentAccessStateErrorV2> {
    if (flags & flag != 0) == present {
        Ok(())
    } else {
        Err(RemoteAgentAccessStateErrorV2::InvalidFlags)
    }
}

fn framed_digest_v2(domain: &[u8], fields: &[&[u8]]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    Digest32::from_bytes(hasher.finalize().into())
}

fn snapshot_digest_v2(prefix: &[u8]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(SNAPSHOT_V2_DIGEST_DOMAIN);
    hasher.update((prefix.len() as u64).to_be_bytes());
    hasher.update(prefix);
    Digest32::from_bytes(hasher.finalize().into())
}

fn bytes_are_zero_v2<const N: usize>(bytes: &[u8; N]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

struct CursorV2<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> CursorV2<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RemoteAgentAccessStateErrorV2> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(RemoteAgentAccessStateErrorV2::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(RemoteAgentAccessStateErrorV2::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RemoteAgentAccessStateErrorV2> {
        self.take(N)?
            .try_into()
            .map_err(|_| RemoteAgentAccessStateErrorV2::Truncated)
    }

    fn u8(&mut self) -> Result<u8, RemoteAgentAccessStateErrorV2> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, RemoteAgentAccessStateErrorV2> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, RemoteAgentAccessStateErrorV2> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn usize_u32(&mut self) -> Result<usize, RemoteAgentAccessStateErrorV2> {
        usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidLength)
    }

    fn finish(self) -> Result<(), RemoteAgentAccessStateErrorV2> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(RemoteAgentAccessStateErrorV2::TrailingBytes)
        }
    }
}

#[derive(Debug)]
pub(crate) enum RemoteAgentAccessStateErrorV2 {
    FrameTooLarge,
    Truncated,
    UnsupportedWire,
    InvalidLength,
    InvalidFlags,
    InvalidSequence,
    SequenceExhausted,
    RevisionExhausted,
    GenerationExhausted,
    UnknownMode,
    UnknownPhase,
    UnknownHead,
    IdentityMismatch,
    ChecksumMismatch,
    NonCanonical,
    TrailingBytes,
    NotApplyRequest,
    InvalidInitialState,
    InvalidState,
    InvalidNestedRequest,
    InvalidOperationRequest,
    InvalidOperationReplacement,
    ReplayDetected,
    InvalidReplayAuthority,
    InvalidFreshRequest,
    InvalidFreshClockMarker,
    InvalidCurrentFinalMarker,
    DeadlineOverflow,
    DeadlineExpired,
    CasMismatch,
    InvalidActiveHead,
    InvalidGenerationShape,
    InvalidGenerationSuccessor,
    InvalidProgress,
    InvalidProgressSuccessor,
    RetainedS0Changed,
    InvalidPhaseSuccessor,
    InvalidTerminalAuthentication,
    InvalidTerminalShape,
    OperationInProgress,
    ReconcileRequired,
    Contract(RemoteAgentDataPlanePlanError),
}

impl fmt::Display for RemoteAgentAccessStateErrorV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("remote Agent access state v2 failed: ")?;
        match self {
            Self::Contract(error) => write!(formatter, "PXAR/PXAU v2 contract: {error}"),
            error => write!(formatter, "{error:?}"),
        }
    }
}

impl std::error::Error for RemoteAgentAccessStateErrorV2 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(error) => Some(error),
            _ => None,
        }
    }
}

impl RemoteAgentAccessSnapshotV2 {
    /// Strict structural recovery decode for PXRS v2 only. The static pins are
    /// independently available before any S0/effect recovery. The stored lower
    /// capability projection remains inert until D2 supplies separate live
    /// evidence and mints a non-cloneable current-final marker.
    pub(crate) fn decode(
        frame: &[u8],
        expected_static_identity: RemoteAgentAccessStaticIdentityPinsV2,
    ) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        let snapshot = Self::decode_raw(frame)?;
        snapshot.validate_static_identity(expected_static_identity)?;
        Ok(snapshot)
    }

    /// Self-contained wire recovery. This private seam performs no external
    /// identity/currentness check and therefore returns structural state only.
    fn decode_raw(frame: &[u8]) -> Result<Self, RemoteAgentAccessStateErrorV2> {
        if frame.len() > MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES {
            return Err(RemoteAgentAccessStateErrorV2::FrameTooLarge);
        }
        if frame.len() < SNAPSHOT_V2_HEADER_BYTES + SNAPSHOT_V2_DIGEST_BYTES {
            return Err(RemoteAgentAccessStateErrorV2::Truncated);
        }
        let mut cursor = CursorV2::new(frame);
        if cursor.array::<4>()? != *SNAPSHOT_MAGIC
            || cursor.u16()? != SNAPSHOT_V2_VERSION
            || usize::from(cursor.u16()?) != SNAPSHOT_V2_HEADER_BYTES
        {
            return Err(RemoteAgentAccessStateErrorV2::UnsupportedWire);
        }
        let total_length = cursor.usize_u32()?;
        let flags = cursor.u16()?;
        if flags & !SNAPSHOT_V2_KNOWN_FLAGS != 0 {
            return Err(RemoteAgentAccessStateErrorV2::InvalidFlags);
        }
        let phase = RemoteAgentAccessDurablePhaseV2::decode(cursor.u8()?)?;
        let mode_byte = cursor.u8()?;
        let head_kind = RemoteAgentAccessHeadKindV2::decode(cursor.u8()?)?;
        let lifecycle_effect_byte = cursor.u8()?;
        let drain_outcome_byte = cursor.u8()?;
        let remote_observation_byte = cursor.u8()?;
        let queryable_declared_bitmap = cursor.u8()?;
        let ingress_fenced_bitmap = cursor.u8()?;
        let worker_joined_bitmap = cursor.u8()?;
        let public_phase_byte = cursor.u8()?;
        let physical_binding_census = cursor.u16()?;
        let active_request_length = cursor.usize_u32()?;
        let active_terminal_length = cursor.usize_u32()?;
        let operation_request_length = cursor.usize_u32()?;
        let operation_terminal_length = cursor.usize_u32()?;
        if active_request_length > MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES
            || operation_request_length > MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES
            || active_terminal_length
                > MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES
            || operation_terminal_length
                > MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES
            || SNAPSHOT_V2_HEADER_BYTES
                .checked_add(active_request_length)
                .and_then(|length| length.checked_add(active_terminal_length))
                .and_then(|length| length.checked_add(operation_request_length))
                .and_then(|length| length.checked_add(operation_terminal_length))
                .and_then(|length| length.checked_add(SNAPSHOT_V2_DIGEST_BYTES))
                != Some(frame.len())
            || total_length != frame.len()
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidLength);
        }
        validate_presence_flag_v2(
            flags,
            SNAPSHOT_V2_HAS_ACTIVE_REQUEST,
            active_request_length != 0,
        )?;
        validate_presence_flag_v2(
            flags,
            SNAPSHOT_V2_HAS_ACTIVE_TERMINAL,
            active_terminal_length != 0,
        )?;
        validate_presence_flag_v2(
            flags,
            SNAPSHOT_V2_HAS_OPERATION_REQUEST,
            operation_request_length != 0,
        )?;
        validate_presence_flag_v2(
            flags,
            SNAPSHOT_V2_HAS_OPERATION_TERMINAL,
            operation_terminal_length != 0,
        )?;
        if (active_request_length == 0) != (active_terminal_length == 0) {
            return Err(RemoteAgentAccessStateErrorV2::InvalidFlags);
        }
        let sequence = cursor.u64()?;
        let writer_runtime_host_epoch = cursor.u64()?;
        let stable_head_runtime_host_epoch = cursor.u64()?;
        let owner_slot_revision = cursor.u64()?;
        let access_generation_high_water = cursor.u64()?;
        let active_access_generation = decode_optional_generation_v2(cursor.u64()?)?;
        let candidate_access_generation = decode_optional_generation_v2(cursor.u64()?)?;
        let stable_head_snapshot_sequence = cursor.u64()?;
        let clock_generation_value = cursor.u64()?;
        let admitted_at_nanos = cursor.u64()?;
        let absolute_deadline_nanos = cursor.u64()?;
        let first_effect_observed_at_nanos = cursor.u64()?;
        let selection_observed_at_nanos = cursor.u64()?;
        let submit_admitted_count = cursor.u64()?;
        let submit_terminalized_count = cursor.u64()?;
        let control_admitted_count = cursor.u64()?;
        let control_terminalized_count = cursor.u64()?;
        let submit_binding_epoch = cursor.u64()?;
        let control_binding_epoch = cursor.u64()?;
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let store_instance_id = cursor.array()?;
        let clock_domain_bytes: [u8; 16] = cursor.array()?;
        let active_proxy_session_epoch_bytes: [u8; 16] = cursor.array()?;
        let candidate_proxy_session_epoch_bytes: [u8; 16] = cursor.array()?;
        let owner_target_fingerprint = Digest32::from_bytes(cursor.array()?);
        let transition_projection_digest = Digest32::from_bytes(cursor.array()?);
        let lower_capability_projection_digest = Digest32::from_bytes(cursor.array()?);
        let encoded_previous = Digest32::from_bytes(cursor.array()?);
        let stable_head_snapshot_digest = Digest32::from_bytes(cursor.array()?);
        let active_request_digest = Digest32::from_bytes(cursor.array()?);
        let active_pxau_digest = Digest32::from_bytes(cursor.array()?);
        let outer_request_digest = Digest32::from_bytes(cursor.array()?);
        let outer_auth_transcript_digest = Digest32::from_bytes(cursor.array()?);
        let inner_request_digest = Digest32::from_bytes(cursor.array()?);
        let inner_envelope_request_digest = Digest32::from_bytes(cursor.array()?);
        let inner_proof_envelope_digest = Digest32::from_bytes(cursor.array()?);
        let tenure_nonce_identity = Digest32::from_bytes(cursor.array()?);
        let request_nonce_identity = Digest32::from_bytes(cursor.array()?);
        let temporal_lineage_identity = Digest32::from_bytes(cursor.array()?);
        let carrier_binding_digest = Digest32::from_bytes(cursor.array()?);
        let retained_s0_census_before_digest = Digest32::from_bytes(cursor.array()?);
        let retained_s0_census_after_digest = Digest32::from_bytes(cursor.array()?);
        let proxy_topology_compatibility_digest = Digest32::from_bytes(cursor.array()?);
        let resource_census_digest = Digest32::from_bytes(cursor.array()?);
        let raw_outcome_digest = Digest32::from_bytes(cursor.array()?);
        let retained_s0_cas = RemoteAgentRetainedS0CasV2::decode(
            cursor.take(
                paraegox_runtime_contracts::remote_agent_data_plane_plan::REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES,
            )?,
        )
        .map_err(RemoteAgentAccessStateErrorV2::Contract)?;
        let expected_s1_cas = RemoteAgentActiveS1CasV2::decode(
            cursor.take(
                paraegox_runtime_contracts::remote_agent_data_plane_plan::REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES,
            )?,
        )
        .map_err(RemoteAgentAccessStateErrorV2::Contract)?;
        if cursor.offset != SNAPSHOT_V2_HEADER_BYTES {
            return Err(RemoteAgentAccessStateErrorV2::NonCanonical);
        }
        let identity = RemoteAgentAccessSnapshotIdentityPinsV2 {
            target,
            store_instance_id,
            owner_target_fingerprint,
            transition_projection_digest,
            lower_capability_projection_digest,
        };
        let previous_snapshot_digest = match (
            flags & SNAPSHOT_V2_HAS_PREVIOUS != 0,
            sequence,
            digest_is_zero(encoded_previous),
        ) {
            (false, 1, true) => None,
            (true, 2.., false) => Some(encoded_previous),
            _ => return Err(RemoteAgentAccessStateErrorV2::InvalidSequence),
        };
        let active_request = if active_request_length == 0 {
            None
        } else {
            Some(
                RemoteAgentAccessRequestV2::decode(cursor.take(active_request_length)?)
                    .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidNestedRequest)?,
            )
        };
        let active_terminal = if active_terminal_length == 0 {
            None
        } else {
            Some(
                RemoteAgentDataPlaneTerminalReceiptV2::decode(cursor.take(active_terminal_length)?)
                    .map_err(RemoteAgentAccessStateErrorV2::Contract)?,
            )
        };
        let operation_request = if operation_request_length == 0 {
            None
        } else {
            Some(
                RemoteAgentAccessRequestV2::decode(cursor.take(operation_request_length)?)
                    .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidNestedRequest)?,
            )
        };
        let operation_terminal = if operation_terminal_length == 0 {
            None
        } else {
            Some(
                RemoteAgentDataPlaneTerminalReceiptV2::decode(
                    cursor.take(operation_terminal_length)?,
                )
                .map_err(RemoteAgentAccessStateErrorV2::Contract)?,
            )
        };
        let encoded_snapshot_digest = Digest32::from_bytes(cursor.array()?);
        cursor.finish()?;
        if snapshot_digest_v2(&frame[..frame.len() - SNAPSHOT_V2_DIGEST_BYTES])
            != encoded_snapshot_digest
        {
            return Err(RemoteAgentAccessStateErrorV2::ChecksumMismatch);
        }
        let has_operation = operation_request.is_some();
        let mode = if has_operation {
            Some(decode_mode_v2(mode_byte)?)
        } else if mode_byte == 0 {
            None
        } else {
            return Err(RemoteAgentAccessStateErrorV2::UnknownMode);
        };
        let admission = if has_operation {
            if bytes_are_zero_v2(&clock_domain_bytes) {
                return Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest);
            }
            Some(RemoteAgentAccessAdmissionFactsV2 {
                clock_domain: ClockDomainRef::from_bytes(clock_domain_bytes),
                clock_generation: ClockGeneration::try_new(clock_generation_value)
                    .map_err(|_| RemoteAgentAccessStateErrorV2::InvalidFreshRequest)?,
                admitted_at_nanos,
                absolute_deadline_nanos,
                outer_request_digest,
                outer_auth_transcript_digest,
                inner_request_digest,
                inner_envelope_request_digest,
                inner_proof_envelope_digest,
                tenure_nonce_identity,
                request_nonce_identity,
                temporal_lineage_identity,
                carrier_binding_digest,
            })
        } else {
            None
        };
        let progress = if has_operation {
            Some(RemoteAgentAccessProgressFactsV2 {
                lifecycle_effect: decode_lifecycle_effect_v2(lifecycle_effect_byte)?,
                drain_outcome: decode_drain_outcome_v2(drain_outcome_byte)?,
                remote_observation: decode_remote_observation_v2(remote_observation_byte)?,
                queryable_declared_bitmap,
                ingress_fenced_bitmap,
                worker_joined_bitmap,
                public_phase: decode_public_phase_v2(public_phase_byte)?,
                physical_binding_census,
                selection_observed_at_nanos,
                submit_admitted_count,
                submit_terminalized_count,
                control_admitted_count,
                control_terminalized_count,
                retained_s0_census_before_digest,
                retained_s0_census_after_digest,
                proxy_topology_compatibility_digest,
                resource_census_digest,
                raw_outcome_digest,
            })
        } else {
            None
        };
        let active_proxy_session_epoch =
            decode_optional_epoch_v2(active_access_generation, active_proxy_session_epoch_bytes)?;
        let candidate_proxy_session_epoch = decode_optional_epoch_v2(
            candidate_access_generation,
            candidate_proxy_session_epoch_bytes,
        )?;
        let active_head = match (active_request, active_terminal) {
            (Some(request), Some(terminal)) => {
                if request.request_digest() != active_request_digest
                    || terminal.receipt_digest() != active_pxau_digest
                    || stable_head_runtime_host_epoch == 0
                    || stable_head_snapshot_sequence == 0
                    || digest_is_zero(stable_head_snapshot_digest)
                {
                    return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
                }
                Some(RemoteAgentAccessActiveHeadV2 {
                    runtime_host_epoch: stable_head_runtime_host_epoch,
                    snapshot_sequence: stable_head_snapshot_sequence,
                    snapshot_digest: stable_head_snapshot_digest,
                    access_generation: active_access_generation
                        .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?,
                    proxy_session_epoch: active_proxy_session_epoch
                        .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?,
                    request,
                    terminal,
                })
            }
            (None, None) => None,
            _ => return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead),
        };
        let self_head = flags & SNAPSHOT_V2_SELF_HEAD != 0;
        if self_head {
            let request = operation_request
                .as_ref()
                .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
            let terminal = operation_terminal
                .as_ref()
                .ok_or(RemoteAgentAccessStateErrorV2::InvalidActiveHead)?;
            if stable_head_runtime_host_epoch != 0
                || stable_head_snapshot_sequence != 0
                || !digest_is_zero(stable_head_snapshot_digest)
                || request.request_digest() != active_request_digest
                || terminal.receipt_digest() != active_pxau_digest
            {
                return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
            }
        } else if active_head.is_none()
            && (!digest_is_zero(active_request_digest)
                || !digest_is_zero(active_pxau_digest)
                || stable_head_runtime_host_epoch != 0
                || stable_head_snapshot_sequence != 0
                || !digest_is_zero(stable_head_snapshot_digest))
        {
            return Err(RemoteAgentAccessStateErrorV2::InvalidActiveHead);
        }
        let snapshot = Self::try_build(Self {
            identity,
            sequence,
            previous_snapshot_digest,
            writer_runtime_host_epoch,
            owner_slot_revision,
            access_generation_high_water,
            head_kind,
            active_head,
            self_head,
            active_access_generation,
            active_proxy_session_epoch,
            candidate_access_generation,
            candidate_proxy_session_epoch,
            mode,
            phase,
            admission,
            first_effect_observed_at_nanos,
            progress,
            submit_binding_epoch,
            control_binding_epoch,
            retained_s0_cas,
            expected_s1_cas,
            operation_request,
            operation_terminal,
            canonical_wire: Box::new([]),
            snapshot_digest: zero_digest(),
        })?;
        if snapshot.snapshot_digest != encoded_snapshot_digest || snapshot.canonical_wire() != frame
        {
            return Err(RemoteAgentAccessStateErrorV2::NonCanonical);
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
pub(crate) use tests::v2::remote_agent_access_prepared_fixture_v2;

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
            ED25519_ALGORITHM_VERSION, ManagedFabricApplyAdmissionError, TrustedApplyIdentity,
            TrustedApplyKey, TrustedTenureIdentity, TrustedTenureKey,
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
                let (fabric, agent) = local_generations.unwrap_or_else(|| {
                    let active = snapshot
                        .predecessor
                        .active
                        .as_ref()
                        .unwrap_or_else(|| panic!("LocalOnly predecessor must remain active"));
                    (active.fabric_generation, active.agent_generation)
                });
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

        let local = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
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

        let local = prepared_authorized(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate);
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
        let initial =
            prepared(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate).generations();
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
        assert!(
            decoded
                .request
                .verify_controller_request(decoded.request.carrier(), |_, _, _, _, signature| {
                    signature == OUTER_SIGNATURE
                })
                .is_err()
        );

        assert!(!terminal_generation_is_known(
            Some(generation(1)),
            None,
            None,
            1,
        ));
    }

    pub(crate) mod v2 {
        use super::*;
        use paraegox_runtime_contracts::{
            apply::{PlanWriterContext, WriterTenureProof},
            reference_control::ed25519_control_key_fingerprint,
            remote_agent_access::{
                ControllerAuthenticatedRemoteAgentAccessRequestV2,
                RemoteAgentAccessRequestDraftV2, RemoteAgentAccessRequestFieldsV2,
                RemoteAgentAccessRequestIdV2,
            },
            remote_agent_data_plane_plan::{
                RemoteAgentDataPlaneApplyRequestDraftV2, RemoteAgentDataPlaneTargetExecutionV2,
                RemoteAgentDataPlaneTerminalAuthClaimV2,
                RemoteAgentDataPlaneTerminalEvidenceFieldsV2,
                RemoteAgentDataPlaneTerminalEvidenceV2, RemoteAgentDataPlaneTerminalHeadV2,
                RemoteAgentDataPlaneTerminalReceiptDraftV2,
                RemoteAgentDataPlaneTerminalStateFieldsV2, RemoteAgentDataPlaneTerminalStateV2,
            },
            temporal::ApplyTemporalConstraint,
        };

        const PROXY_DATA_PLANE_V2_FIXTURE: &str =
            include_str!("../../../tests/fixtures/wire/t2_remote_agent_proxy_data_plane_v2.json");
        const ACCESS_V2_FIXTURE: &str =
            include_str!("../../../tests/fixtures/wire/t2_remote_agent_access_v2.json");
        const TERMINAL_SIGNATURE_V2: [u8; 64] = [0xa6; 64];
        const ACTIVE_PROXY_SESSION_EPOCH_V2: [u8; 16] = [0xa7; 16];
        const OBSERVED_RESOURCE_CENSUS_V2: Digest32 = Digest32::from_bytes([0xa8; 32]);
        const OBSERVED_RAW_OUTCOME_V2: Digest32 = Digest32::from_bytes([0xa9; 32]);

        const ALL_PHASES: [RemoteAgentAccessDurablePhaseV2; 28] = [
            RemoteAgentAccessDurablePhaseV2::InitializedAbsent,
            RemoteAgentAccessDurablePhaseV2::PreparedNoEffects,
            RemoteAgentAccessDurablePhaseV2::S1OpenIntent,
            RemoteAgentAccessDurablePhaseV2::S1Opened,
            RemoteAgentAccessDurablePhaseV2::SubmitDeclareIntent,
            RemoteAgentAccessDurablePhaseV2::SubmitDeclared,
            RemoteAgentAccessDurablePhaseV2::ControlDeclareIntent,
            RemoteAgentAccessDurablePhaseV2::QueryablesDeclared,
            RemoteAgentAccessDurablePhaseV2::ReadyObserved,
            RemoteAgentAccessDurablePhaseV2::ActiveReady,
            RemoteAgentAccessDurablePhaseV2::SubmitFenceIntent,
            RemoteAgentAccessDurablePhaseV2::SubmitFenced,
            RemoteAgentAccessDurablePhaseV2::ControlFenceIntent,
            RemoteAgentAccessDurablePhaseV2::IngressFenced,
            RemoteAgentAccessDurablePhaseV2::DrainIntent,
            RemoteAgentAccessDurablePhaseV2::Drained,
            RemoteAgentAccessDurablePhaseV2::SubmitJoinIntent,
            RemoteAgentAccessDurablePhaseV2::SubmitJoined,
            RemoteAgentAccessDurablePhaseV2::ControlJoinIntent,
            RemoteAgentAccessDurablePhaseV2::WorkersJoined,
            RemoteAgentAccessDurablePhaseV2::S1CloseIntent,
            RemoteAgentAccessDurablePhaseV2::S1Closed,
            RemoteAgentAccessDurablePhaseV2::LocalOnlyObserved,
            RemoteAgentAccessDurablePhaseV2::LocalOnlyReady,
            RemoteAgentAccessDurablePhaseV2::NoEffectTerminal,
            RemoteAgentAccessDurablePhaseV2::Uncertain,
            RemoteAgentAccessDurablePhaseV2::QuarantineIntent,
            RemoteAgentAccessDurablePhaseV2::Quarantined,
        ];

        fn fixture_tail_after<'fixture>(fixture: &'fixture str, marker: &str) -> &'fixture str {
            let offset = fixture
                .find(marker)
                .unwrap_or_else(|| panic!("missing fixture marker {marker}"));
            &fixture[offset..]
        }

        fn fixture_item_wire(fixture: &str, section: &str, item: &str) -> Vec<u8> {
            let section = fixture_tail_after(fixture, section);
            let item = fixture_tail_after(section, item);
            fixture_hex_after(item, "", "\"wire_hex\"")
        }

        fn controller_carrier_v2(
            template: &RestrictedRuntimeApplyCarrierBindingV1,
        ) -> RestrictedRuntimeApplyCarrierBindingV1 {
            let controller_key = SigningKey::from_bytes(&INNER_SIGNING_SEED).verifying_key();
            RestrictedRuntimeApplyCarrierBindingV1::try_new(
                RestrictedRuntimeApplyCarrierBindingFieldsV1 {
                    target: template.target(),
                    runtime_principal: template.runtime_principal(),
                    controller_principal: template.controller_principal(),
                    endpoint_ref: template.endpoint_ref(),
                    endpoint_generation: template.endpoint_generation(),
                    route: template.route(),
                    controller_request_key: template.controller_request_key(),
                    controller_request_key_fingerprint: ed25519_control_key_fingerprint(
                        controller_key.as_bytes(),
                    )
                    .unwrap_or_else(|error| {
                        panic!("Controller request key fingerprint rejected: {error}")
                    }),
                    runtime_response_key: template.runtime_response_key(),
                    runtime_response_key_fingerprint: template.runtime_response_key_fingerprint(),
                    control_transport_profile_ref: template.control_transport_profile_ref(),
                    control_transport_profile_digest: template.control_transport_profile_digest(),
                },
            )
            .unwrap_or_else(|error| panic!("Controller carrier rejected: {error}"))
        }

        fn signed_tenure_proof_v2(
            template: &WriterTenureProof,
            nonce: &[u8],
        ) -> WriterTenureProof {
            let unsigned = WriterTenureProof::try_new(
                template.authority(),
                template.claim(),
                nonce,
                &[0; 64],
            )
            .unwrap_or_else(|error| panic!("unsigned tenure proof rejected: {error}"));
            let transcript = unsigned
                .signing_transcript()
                .unwrap_or_else(|error| panic!("tenure transcript rejected: {error}"));
            let signature = SigningKey::from_bytes(&TENURE_SIGNING_SEED)
                .sign(transcript.as_bytes())
                .to_bytes();
            WriterTenureProof::try_new(
                template.authority(),
                template.claim(),
                nonce,
                &signature,
            )
            .unwrap_or_else(|error| panic!("signed tenure proof rejected: {error}"))
        }

        fn signed_control_v2(
            template: &RuntimeApplyControl,
            operation_id: ApplyOperationId,
            tenure_nonce: &[u8],
        ) -> RuntimeApplyControl {
            let writer_template = template.writer_context();
            let writer = PlanWriterContext::try_new(
                writer_template.writer(),
                writer_template.epoch(),
                signed_tenure_proof_v2(writer_template.proof(), tenure_nonce),
            )
            .unwrap_or_else(|error| panic!("signed writer context rejected: {error}"));
            RuntimeApplyControl::new(writer, template.expected_active(), operation_id)
        }

        fn finalize_inner_v2(
            draft: RemoteAgentDataPlaneApplyRequestDraftV2,
        ) -> RemoteAgentDataPlaneApplyRequestV2 {
            let transcript = draft
                .signing_transcript()
                .unwrap_or_else(|error| panic!("PXAR v11 transcript rejected: {error}"));
            let signature = SigningKey::from_bytes(&INNER_SIGNING_SEED)
                .sign(transcript.as_bytes())
                .to_bytes();
            draft
                .finalize(&signature)
                .unwrap_or_else(|error| panic!("signed PXAR v11 rejected: {error}"))
        }

        fn finalize_outer_v2(
            draft: RemoteAgentAccessRequestDraftV2,
        ) -> RemoteAgentAccessRequestV2 {
            let transcript = draft
                .signing_transcript()
                .unwrap_or_else(|error| panic!("PXRA v2 transcript rejected: {error}"));
            let signature = SigningKey::from_bytes(&INNER_SIGNING_SEED)
                .sign(transcript.as_bytes())
                .to_bytes();
            draft
                .finalize(&signature)
                .unwrap_or_else(|error| panic!("signed PXRA v2 rejected: {error}"))
        }

        fn active_inner_request_v2() -> RemoteAgentDataPlaneApplyRequestV2 {
            RemoteAgentDataPlaneApplyRequestV2::decode(&fixture_item_wire(
                PROXY_DATA_PLANE_V2_FIXTURE,
                "\"active_ready\"",
                "\"pxar_v11\"",
            ))
            .unwrap_or_else(|error| panic!("active PXAR v11 fixture rejected: {error}"))
        }

        fn local_inner_request_v2() -> RemoteAgentDataPlaneApplyRequestV2 {
            RemoteAgentDataPlaneApplyRequestV2::decode(&fixture_item_wire(
                PROXY_DATA_PLANE_V2_FIXTURE,
                "\"local_only_ready\"",
                "\"pxar_v11\"",
            ))
            .unwrap_or_else(|error| panic!("Local PXAR v11 fixture rejected: {error}"))
        }

        fn retained_s0_cas_v2() -> RemoteAgentRetainedS0CasV2 {
            RemoteAgentRetainedS0CasV2::decode(&fixture_item_wire(
                PROXY_DATA_PLANE_V2_FIXTURE,
                "\"retained_s0_cas\"",
                "\"wire_hex\"",
            ))
            .unwrap_or_else(|error| panic!("retained S0 CAS fixture rejected: {error}"))
        }

        fn outer_request_template_v2() -> RemoteAgentAccessRequestV2 {
            RemoteAgentAccessRequestV2::decode(&fixture_item_wire(
                ACCESS_V2_FIXTURE,
                "\"apply\"",
                "\"pxra_v2\"",
            ))
            .unwrap_or_else(|error| panic!("PXRA v2 fixture rejected: {error}"))
        }

        fn identity_v2() -> RemoteAgentAccessSnapshotIdentityPinsV2 {
            RemoteAgentAccessSnapshotIdentityPinsV2 {
                target: active_inner_request_v2().target(),
                store_instance_id: STORE,
                owner_target_fingerprint: Digest32::from_bytes([0xd1; 32]),
                transition_projection_digest: Digest32::from_bytes([0xd2; 32]),
                lower_capability_projection_digest: Digest32::from_bytes([0xd3; 32]),
            }
        }

        fn static_identity_v2(
            identity: RemoteAgentAccessSnapshotIdentityPinsV2,
        ) -> RemoteAgentAccessStaticIdentityPinsV2 {
            RemoteAgentAccessStaticIdentityPinsV2 {
                target: identity.target,
                store_instance_id: identity.store_instance_id,
                owner_target_fingerprint: identity.owner_target_fingerprint,
                transition_projection_digest: identity.transition_projection_digest,
            }
        }

        fn initial_snapshot_v2() -> RemoteAgentAccessSnapshotV2 {
            RemoteAgentAccessSnapshotV2::try_initialize_absent(
                identity_v2(),
                RUNTIME_EPOCH,
                retained_s0_cas_v2(),
                RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                    .unwrap_or_else(|error| panic!("absent S1 CAS rejected: {error}")),
                31,
                32,
            )
            .unwrap_or_else(|error| panic!("initial PXRS2 rejected: {error}"))
        }

        fn rebuilt_active_request_v2(
            expected_s1_cas: RemoteAgentActiveS1CasV2,
            temporal: Option<ApplyTemporalConstraint>,
            expected_runtime_host_epoch: u64,
        ) -> RemoteAgentAccessRequestV2 {
            let inner_template = active_inner_request_v2();
            let execution_template = inner_template.target_execution();
            let execution = RemoteAgentDataPlaneTargetExecutionV2::try_remote_access_active(
                execution_template.projection().clone(),
                execution_template.predecessor().clone(),
                execution_template.retained_s0_cas(),
                expected_s1_cas,
                execution_template.profile().clone(),
            )
            .unwrap_or_else(|error| panic!("rebuilt Active PXTE v10 rejected: {error}"));
            let control_template = inner_template.control_commitment().control();
            let inner = finalize_inner_v2(
                RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
                    execution,
                    inner_template.provenance(),
                    signed_control_v2(
                        control_template,
                        control_template.operation_id(),
                        control_template.writer_context().proof().nonce(),
                    ),
                    temporal.unwrap_or_else(|| inner_template.temporal()),
                    inner_template.expected_runtime_store_instance_id(),
                    inner_template.authentication().claim().clone(),
                )
                .unwrap_or_else(|error| {
                    panic!("rebuilt Active PXAR v11 draft rejected: {error}")
                }),
            );
            let outer_template = outer_request_template_v2();
            finalize_outer_v2(
                RemoteAgentAccessRequestDraftV2::try_apply_remote_access(
                    RemoteAgentAccessRequestFieldsV2 {
                        request_id: RemoteAgentAccessRequestIdV2::try_from_bytes(
                            *inner.operation_id().as_bytes(),
                        )
                        .unwrap_or_else(|error| panic!("PXRA v2 request id rejected: {error}")),
                        carrier: controller_carrier_v2(outer_template.carrier()),
                        target: inner.target(),
                        expected_runtime_store_instance_id: inner
                            .expected_runtime_store_instance_id(),
                        expected_runtime_host_epoch,
                        auth_claim: outer_template.authentication().claim().clone(),
                    },
                    inner,
                )
                .unwrap_or_else(|error| {
                    panic!("rebuilt Active PXRA v2 draft rejected: {error}")
                }),
            )
        }

        fn active_request_v2() -> RemoteAgentAccessRequestV2 {
            rebuilt_active_request_v2(
                RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                    .unwrap_or_else(|error| panic!("initial absent S1 CAS rejected: {error}")),
                None,
                RUNTIME_EPOCH,
            )
        }

        fn rebuilt_active_request_with_identities_v2(
            expected_s1_cas: RemoteAgentActiveS1CasV2,
            operation_id: ApplyOperationId,
            tenure_nonce: &[u8],
            request_nonce: &[u8],
            outer_nonce: &[u8],
        ) -> RemoteAgentAccessRequestV2 {
            let template = active_inner_request_v2();
            let execution_template = template.target_execution();
            let execution = RemoteAgentDataPlaneTargetExecutionV2::try_remote_access_active(
                execution_template.projection().clone(),
                execution_template.predecessor().clone(),
                execution_template.retained_s0_cas(),
                expected_s1_cas,
                execution_template.profile().clone(),
            )
            .unwrap_or_else(|error| panic!("fresh Active PXTE v10 rejected: {error}"));
            let control_template = template.control_commitment().control();
            let inner = finalize_inner_v2(
                RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
                    execution,
                    template.provenance(),
                    signed_control_v2(control_template, operation_id, tenure_nonce),
                    template.temporal(),
                    template.expected_runtime_store_instance_id(),
                    auth_claim_with_nonce_v2(template.authentication().claim(), request_nonce),
                )
                .unwrap_or_else(|error| {
                    panic!("fresh Active PXAR v11 draft rejected: {error}")
                }),
            );
            let outer_template = outer_request_template_v2();
            finalize_outer_v2(
                RemoteAgentAccessRequestDraftV2::try_apply_remote_access(
                    RemoteAgentAccessRequestFieldsV2 {
                        request_id: RemoteAgentAccessRequestIdV2::try_from_bytes(
                            *inner.operation_id().as_bytes(),
                        )
                        .unwrap_or_else(|error| {
                            panic!("fresh Active request id rejected: {error}")
                        }),
                        carrier: controller_carrier_v2(outer_template.carrier()),
                        target: inner.target(),
                        expected_runtime_store_instance_id: inner
                            .expected_runtime_store_instance_id(),
                        expected_runtime_host_epoch: RUNTIME_EPOCH,
                        auth_claim: auth_claim_with_nonce_v2(
                            outer_template.authentication().claim(),
                            outer_nonce,
                        ),
                    },
                    inner,
                )
                .unwrap_or_else(|error| {
                    panic!("fresh Active PXRA v2 draft rejected: {error}")
                }),
            )
        }

        fn auth_claim_with_nonce_v2(
            template: &ApplyRequestAuthClaim,
            nonce: &[u8],
        ) -> ApplyRequestAuthClaim {
            ApplyRequestAuthClaim::try_new(
                template.principal(),
                template.key(),
                template.algorithm(),
                template.algorithm_version(),
                nonce,
            )
            .unwrap_or_else(|error| panic!("fresh request auth claim rejected: {error}"))
        }

        fn rebuilt_local_request_with_identities_v2(
            expected_s1_cas: RemoteAgentActiveS1CasV2,
            operation_id: ApplyOperationId,
            tenure_nonce: &[u8],
            request_nonce: &[u8],
            outer_nonce: &[u8],
        ) -> RemoteAgentAccessRequestV2 {
            let template = local_inner_request_v2();
            let execution_template = template.target_execution();
            let execution = RemoteAgentDataPlaneTargetExecutionV2::try_local_agent_only_deactivate(
                execution_template.projection().clone(),
                execution_template.predecessor().clone(),
                execution_template.retained_s0_cas(),
                expected_s1_cas,
                execution_template.profile().clone(),
            )
            .unwrap_or_else(|error| panic!("rebuilt Local PXTE v10 rejected: {error}"));
            let control_template = template.control_commitment().control();
            let inner = finalize_inner_v2(
                RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
                    execution,
                    template.provenance(),
                    signed_control_v2(control_template, operation_id, tenure_nonce),
                    template.temporal(),
                    template.expected_runtime_store_instance_id(),
                    auth_claim_with_nonce_v2(template.authentication().claim(), request_nonce),
                )
                .unwrap_or_else(|error| {
                    panic!("rebuilt Local PXAR v11 draft rejected: {error}")
                }),
            );
            let outer_template = outer_request_template_v2();
            finalize_outer_v2(
                RemoteAgentAccessRequestDraftV2::try_apply_remote_access(
                    RemoteAgentAccessRequestFieldsV2 {
                        request_id: RemoteAgentAccessRequestIdV2::try_from_bytes(
                            *inner.operation_id().as_bytes(),
                        )
                        .unwrap_or_else(|error| {
                            panic!("Local PXRA v2 request id rejected: {error}")
                        }),
                        carrier: controller_carrier_v2(outer_template.carrier()),
                        target: inner.target(),
                        expected_runtime_store_instance_id: inner
                            .expected_runtime_store_instance_id(),
                        expected_runtime_host_epoch: RUNTIME_EPOCH,
                        auth_claim: auth_claim_with_nonce_v2(
                            outer_template.authentication().claim(),
                            outer_nonce,
                        ),
                    },
                    inner,
                )
                .unwrap_or_else(|error| {
                    panic!("rebuilt Local PXRA v2 draft rejected: {error}")
                }),
            )
        }

        fn fresh_local_request_v2(
            expected_s1_cas: RemoteAgentActiveS1CasV2,
        ) -> RemoteAgentAccessRequestV2 {
            rebuilt_local_request_with_identities_v2(
                expected_s1_cas,
                ApplyOperationId::from_bytes([0x73; 16]),
                &[0x71; 16],
                &[0x74; 16],
                &[0x76; 16],
            )
        }

        fn clock_for_request_v2(request: &RemoteAgentAccessRequestV2, ticks: u64) -> ClockReading {
            let inner = inner_request_v2(request)
                .unwrap_or_else(|error| panic!("PXRA v2 inner request rejected: {error}"));
            ClockReading::new(
                inner.temporal().target_clock_domain(),
                inner.temporal().target_clock_generation(),
                MonotonicInstant::from_ticks(ticks),
            )
        }

        fn authenticate_request_v2(
            request: &RemoteAgentAccessRequestV2,
        ) -> ControllerAuthenticatedRemoteAgentAccessRequestV2<'_> {
            let carrier = request.carrier();
            request
                .verify_controller_apply_request(
                    carrier,
                    |principal, key, algorithm, version, transcript, signature| {
                        principal == carrier.controller_principal()
                            && key == carrier.controller_request_key()
                            && algorithm.value() == ED25519_ALGORITHM
                            && version == ED25519_ALGORITHM_VERSION
                            && signature
                                == SigningKey::from_bytes(&INNER_SIGNING_SEED)
                                    .sign(transcript)
                                    .to_bytes()
                    },
                    |principal, key, fingerprint, transcript, signature| {
                        principal == carrier.controller_principal()
                            && key == carrier.controller_request_key()
                            && fingerprint == carrier.controller_request_key_fingerprint()
                            && signature
                                == SigningKey::from_bytes(&INNER_SIGNING_SEED)
                                    .sign(transcript)
                                    .to_bytes()
                    },
                )
                .unwrap_or_else(|error| panic!("real composite PXRA v2 auth rejected: {error}"))
        }

        fn admission_policy_v2(request: &RemoteAgentAccessRequestV2) -> ApplyAdmissionPolicy {
            let inner = inner_request_v2(request)
                .unwrap_or_else(|error| panic!("PXRA v2 inner request rejected: {error}"));
            let control = inner.control_commitment().control();
            let writer = control.writer_context();
            let proof = writer.proof();
            let proof_authority = proof.authority();
            let claim = inner.authentication().claim();
            let tenure_key = TrustedTenureKey::try_new(
                TrustedTenureIdentity::new(
                    inner.provenance().source_scope(),
                    PrincipalRef::from_bytes([0x06; 16]),
                    1_001,
                    1_002,
                    proof_authority.authority(),
                ),
                proof_authority.key(),
                proof_authority.algorithm(),
                proof_authority.algorithm_version(),
                SigningKey::from_bytes(&TENURE_SIGNING_SEED)
                    .verifying_key()
                    .to_bytes(),
            )
            .unwrap_or_else(|error| panic!("PXAR v11 tenure trust rejected: {error}"));
            let apply_key = TrustedApplyKey::try_new(
                TrustedApplyIdentity::new(
                    inner.provenance().source_scope(),
                    inner.target(),
                    claim.principal(),
                    writer.writer(),
                ),
                claim.key(),
                claim.algorithm(),
                claim.algorithm_version(),
                SigningKey::from_bytes(&INNER_SIGNING_SEED)
                    .verifying_key()
                    .to_bytes(),
            )
            .unwrap_or_else(|error| panic!("PXAR v11 Apply trust rejected: {error}"));
            ApplyAdmissionPolicy::try_new(
                BoundedDuration::from_nanos(inner.temporal().original_budget().value()),
                AdmissionStateLimits::try_new(4, 4, 4)
                    .unwrap_or_else(|error| panic!("PXAR v11 admission limits rejected: {error}")),
                [tenure_key],
                [apply_key],
            )
            .unwrap_or_else(|error| panic!("PXAR v11 admission policy rejected: {error}"))
        }

        fn verified_ingress_v2<'request>(
            request: &'request RemoteAgentAccessRequestV2,
            clock: ClockReading,
        ) -> Result<VerifiedRemoteAgentAccessApplyIngressV2<'request>, RemoteAgentAccessStateErrorV2>
        {
            admission_policy_v2(request)
                .verify_remote_agent_access_apply_ingress_v2(
                    authenticate_request_v2(request),
                    request.carrier(),
                    clock,
                )
                .map_err(|error| match error {
                    ManagedFabricApplyAdmissionError::DeadlineOverflow
                        if clock.now().value() != 0 =>
                    {
                        RemoteAgentAccessStateErrorV2::DeadlineOverflow
                    }
                    _ => RemoteAgentAccessStateErrorV2::InvalidFreshRequest,
                })
        }

        #[derive(Clone, Copy)]
        struct CurrentFinalFactsV2 {
            identity: RemoteAgentAccessSnapshotIdentityPinsV2,
            runtime_host_epoch: u64,
            retained_s0_cas: RemoteAgentRetainedS0CasV2,
            retained_s0_census_digest: Digest32,
            submit_binding_epoch: u64,
            control_binding_epoch: u64,
        }

        fn current_final_v2(
            snapshot: RemoteAgentAccessSnapshotV2,
            mutate: impl FnOnce(&mut CurrentFinalFactsV2),
        ) -> Result<RemoteAgentCurrentFinalAccessSnapshotV2, RemoteAgentAccessStateErrorV2>
        {
            let mut facts = CurrentFinalFactsV2 {
                identity: snapshot.identity,
                runtime_host_epoch: snapshot.writer_runtime_host_epoch,
                retained_s0_cas: snapshot.retained_s0_cas,
                retained_s0_census_digest: Digest32::from_bytes([0xd4; 32]),
                submit_binding_epoch: snapshot.submit_binding_epoch,
                control_binding_epoch: snapshot.control_binding_epoch,
            };
            mutate(&mut facts);
            RemoteAgentCurrentFinalAccessSnapshotV2::from_exact_readback_for_test(
                snapshot,
                facts.identity,
                facts.runtime_host_epoch,
                facts.retained_s0_cas,
                facts.retained_s0_census_digest,
                facts.submit_binding_epoch,
                facts.control_binding_epoch,
            )
        }

        fn authorize_on_snapshot_v2(
            snapshot: RemoteAgentAccessSnapshotV2,
            request: &RemoteAgentAccessRequestV2,
            clock: ClockReading,
            mutate: impl FnOnce(&mut CurrentFinalFactsV2),
        ) -> Result<RemoteAgentPendingAccessSnapshotV2, RemoteAgentAccessStateErrorV2> {
            authorize_on_snapshot_with_replay_ledger_v2(
                snapshot,
                request,
                clock,
                &[],
                &[],
                &[],
                mutate,
            )
        }

        fn authorize_on_snapshot_with_replay_ledger_v2(
            snapshot: RemoteAgentAccessSnapshotV2,
            request: &RemoteAgentAccessRequestV2,
            clock: ClockReading,
            seen_operation_ids: &[[u8; 16]],
            seen_tenure_nonce_identities: &[Digest32],
            seen_request_nonce_identities: &[Digest32],
            mutate: impl FnOnce(&mut CurrentFinalFactsV2),
        ) -> Result<RemoteAgentPendingAccessSnapshotV2, RemoteAgentAccessStateErrorV2> {
            let verified_ingress = verified_ingress_v2(request, clock)?;
            let durable_replay_checked =
                RemoteAgentDurableReplayCheckedV2::from_durable_replay_ledger_for_test(
                    &snapshot,
                    request,
                    verified_ingress.admitted_at_nanos(),
                    seen_operation_ids,
                    seen_tenure_nonce_identities,
                    seen_request_nonce_identities,
                )?;
            let current = current_final_v2(snapshot, mutate)?;
            current.try_authorize_fresh(verified_ingress, durable_replay_checked)
        }

        fn readback_pending_v2(
            pending: RemoteAgentPendingAccessSnapshotV2,
        ) -> RemoteAgentAccessSnapshotV2 {
            let expected = pending.snapshot().clone();
            let wire = pending.canonical_wire().to_vec();
            let decoded =
                RemoteAgentAccessSnapshotV2::decode(&wire, static_identity_v2(expected.identity))
                    .unwrap_or_else(|error| {
                        panic!("Pending PXRS2 structural readback rejected: {error}")
                    });
            assert_eq!(decoded, expected);
            decoded
        }

        fn authorize_pending_v2(
            pending: RemoteAgentPendingAccessSnapshotV2,
        ) -> RemoteAgentAuthorizedTransitionV2 {
            current_final_v2(readback_pending_v2(pending), |_| {})
                .and_then(RemoteAgentCurrentFinalAccessSnapshotV2::try_authorize_existing)
                .unwrap_or_else(|error| panic!("Pending PXRS2 readback rejected: {error}"))
        }

        fn prepared_active_v2(
            admitted_at_nanos: u64,
        ) -> (
            RemoteAgentAccessRequestV2,
            RemoteAgentAuthorizedTransitionV2,
        ) {
            let request = active_request_v2();
            let pending = authorize_on_snapshot_v2(
                initial_snapshot_v2(),
                &request,
                clock_for_request_v2(&request, admitted_at_nanos),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("Active PXRS2 authorization rejected: {error}"));
            (request, authorize_pending_v2(pending))
        }

        pub(crate) fn remote_agent_access_prepared_fixture_v2() -> (
            RemoteAgentAccessSnapshotV2,
            RemoteAgentPendingAccessSnapshotV2,
            RemoteAgentAccessStaticIdentityPinsV2,
            u64,
        ) {
            let initial = initial_snapshot_v2();
            let static_identity = static_identity_v2(initial.identity);
            let request = active_request_v2();
            let pending = authorize_on_snapshot_v2(
                initial.clone(),
                &request,
                clock_for_request_v2(&request, 100),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("Prepared PXRS2 fixture rejected: {error}"));
            (initial, pending, static_identity, RUNTIME_EPOCH)
        }

        fn active_open_observation_v2(
            snapshot: &RemoteAgentAccessSnapshotV2,
            candidate_proxy_session_epoch: Option<[u8; 16]>,
            fresh_clock: Option<ClockReading>,
        ) -> RemoteAgentAccessObservedProgressV2 {
            let mut facts = snapshot
                .progress
                .unwrap_or_else(|| panic!("Prepared PXRS2 must contain progress"));
            facts.lifecycle_effect = RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted;
            facts.public_phase = RemoteAgentDataPlaneTerminalPhaseV2::S1OpenIntent;
            if let Some(reading) = fresh_clock {
                facts.selection_observed_at_nanos = reading.now().value();
            }
            RemoteAgentAccessObservedProgressV2 {
                next_phase: RemoteAgentAccessDurablePhaseV2::S1OpenIntent,
                facts,
                candidate_proxy_session_epoch,
                fresh_clock,
            }
        }

        fn advance_observed_v2(
            authorized: RemoteAgentAuthorizedTransitionV2,
            next_phase: RemoteAgentAccessDurablePhaseV2,
            fresh_ticks: Option<u64>,
            candidate_proxy_session_epoch: Option<[u8; 16]>,
            mutate: impl FnOnce(&mut RemoteAgentAccessProgressFactsV2),
        ) -> RemoteAgentPendingAccessSnapshotV2 {
            let snapshot = authorized.snapshot();
            let mut facts = snapshot
                .progress
                .unwrap_or_else(|| panic!("authorized PXRS2 must contain progress"));
            facts.lifecycle_effect = RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted;
            facts.public_phase = expected_public_phase_v2(next_phase)
                .unwrap_or_else(|| panic!("observed phase must have a public phase"));
            let fresh_clock = fresh_ticks.map(|ticks| {
                let admission = snapshot
                    .admission
                    .unwrap_or_else(|| panic!("authorized PXRS2 must contain admission facts"));
                facts.selection_observed_at_nanos = ticks;
                ClockReading::new(
                    admission.clock_domain,
                    admission.clock_generation,
                    MonotonicInstant::from_ticks(ticks),
                )
            });
            mutate(&mut facts);
            authorized
                .try_observed_successor(RemoteAgentAccessObservedProgressV2 {
                    next_phase,
                    facts,
                    candidate_proxy_session_epoch,
                    fresh_clock,
                })
                .unwrap_or_else(|error| panic!("PXRS2 {next_phase:?} successor rejected: {error}"))
        }

        fn advance_and_remint_v2(
            authorized: RemoteAgentAuthorizedTransitionV2,
            next_phase: RemoteAgentAccessDurablePhaseV2,
            fresh_ticks: Option<u64>,
            candidate_proxy_session_epoch: Option<[u8; 16]>,
            mutate: impl FnOnce(&mut RemoteAgentAccessProgressFactsV2),
        ) -> RemoteAgentAuthorizedTransitionV2 {
            authorize_pending_v2(advance_observed_v2(
                authorized,
                next_phase,
                fresh_ticks,
                candidate_proxy_session_epoch,
                mutate,
            ))
        }

        fn active_ready_observed_v2() -> RemoteAgentAuthorizedTransitionV2 {
            let (_, mut authorized) = prepared_active_v2(100);
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::S1OpenIntent,
                Some(101),
                Some(ACTIVE_PROXY_SESSION_EPOCH_V2),
                |facts| {
                    facts.resource_census_digest = OBSERVED_RESOURCE_CENSUS_V2;
                    facts.raw_outcome_digest = OBSERVED_RAW_OUTCOME_V2;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::S1Opened,
                None,
                None,
                |_| {},
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::SubmitDeclareIntent,
                Some(102),
                None,
                |facts| facts.submit_admitted_count = 2,
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::SubmitDeclared,
                None,
                None,
                |facts| {
                    facts.queryable_declared_bitmap = 0b01;
                    facts.submit_terminalized_count = 2;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::ControlDeclareIntent,
                Some(103),
                None,
                |facts| {
                    facts.queryable_declared_bitmap = 0b01;
                    facts.control_admitted_count = 1;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::QueryablesDeclared,
                None,
                None,
                |facts| {
                    facts.queryable_declared_bitmap = SNAPSHOT_V2_EXACT_ROUTE_BITMAP;
                    facts.control_terminalized_count = 1;
                },
            );
            advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::ReadyObserved,
                Some(104),
                None,
                |facts| {
                    facts.queryable_declared_bitmap = SNAPSHOT_V2_EXACT_ROUTE_BITMAP;
                    facts.remote_observation =
                        RemoteAgentDataPlaneRemoteObservationV2::S1TlsExactRoutesReady;
                },
            )
        }

        fn terminal_auth_for_outer_v2(
            outer: &RemoteAgentAccessRequestV2,
        ) -> RemoteAgentDataPlaneTerminalAuthClaimV2 {
            RemoteAgentDataPlaneTerminalAuthClaimV2::try_new(
                outer.carrier().runtime_principal(),
                outer.carrier().runtime_response_key(),
                ApplyAuthAlgorithm::try_new(SNAPSHOT_V2_ED25519_ALGORITHM)
                    .unwrap_or_else(|error| panic!("terminal auth algorithm rejected: {error}")),
                SNAPSHOT_V2_ED25519_ALGORITHM_VERSION,
            )
            .unwrap_or_else(|error| panic!("terminal auth claim rejected: {error}"))
        }

        fn try_terminal_receipt_v2(
            snapshot: &RemoteAgentAccessSnapshotV2,
            outcome: RemoteAgentDataPlaneTerminalOutcomeV2,
            auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV2,
            signature: &[u8],
            mutate: impl FnOnce(&mut RemoteAgentDataPlaneTerminalEvidenceFieldsV2),
        ) -> Result<RemoteAgentDataPlaneTerminalReceiptV2, RemoteAgentDataPlanePlanError> {
            let outer = snapshot
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("terminal predecessor must retain PXRA v2"));
            let request = inner_request_v2(outer)
                .unwrap_or_else(|error| panic!("terminal predecessor PXAR rejected: {error}"));
            let retained = snapshot.retained_s0_cas.fields();
            let progress = snapshot
                .progress
                .unwrap_or_else(|| panic!("terminal predecessor must retain progress"));
            let admission = snapshot
                .admission
                .unwrap_or_else(|| panic!("terminal predecessor must retain admission facts"));
            let (head, access_generation, proxy_session_epoch) = match outcome {
                RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady => (
                    RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
                    snapshot.candidate_access_generation,
                    snapshot.candidate_proxy_session_epoch,
                ),
                RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady => (
                    RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
                    None,
                    None,
                ),
                RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected
                | RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain
                | RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined => {
                    let preserved = snapshot.active_head.as_ref().map_or_else(
                        || {
                            snapshot
                                .operation_request
                                .as_ref()
                                .filter(|_| {
                                    snapshot.head_kind == RemoteAgentAccessHeadKindV2::Active
                                })
                                .and_then(|_| snapshot.expected_s1_cas.active())
                                .map(|_| {
                                    RemoteAgentDataPlaneTerminalHeadV2::PreservedExisting(
                                        request.target_slice_digest(),
                                    )
                                })
                                .unwrap_or(RemoteAgentDataPlaneTerminalHeadV2::PreservedNone)
                        },
                        |active| {
                            let active_request =
                                inner_request_v2(&active.request).unwrap_or_else(|error| {
                                    panic!("historical Active PXAR rejected: {error}")
                                });
                            RemoteAgentDataPlaneTerminalHeadV2::PreservedExisting(
                                active_request.target_slice_digest(),
                            )
                        },
                    );
                    let generation = match snapshot.mode {
                        Some(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive) => {
                            snapshot.candidate_access_generation
                        }
                        Some(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate) => {
                            snapshot.active_access_generation
                        }
                        None => None,
                    };
                    let epoch = match snapshot.mode {
                        Some(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive) => {
                            snapshot.candidate_proxy_session_epoch
                        }
                        Some(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate) => {
                            snapshot.active_proxy_session_epoch
                        }
                        None => None,
                    };
                    (preserved, generation, epoch)
                }
            };
            let state = RemoteAgentDataPlaneTerminalStateV2::try_new(
                RemoteAgentDataPlaneTerminalStateFieldsV2 {
                    outcome,
                    lifecycle_effect: progress.lifecycle_effect,
                    phase: progress.public_phase,
                    head,
                    fabric_generation: Some(retained.expected_fabric_generation),
                    agent_generation: Some(retained.expected_agent_generation),
                    access_generation,
                    fabric_session_epoch: Some(retained.expected_fabric_session_epoch),
                    proxy_session_epoch,
                },
            )?;
            let retained_ready = matches!(
                outcome,
                RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady
                    | RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady
                    | RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected
            );
            let mut evidence = RemoteAgentDataPlaneTerminalEvidenceFieldsV2 {
                retained_s0_current_cas_digest: snapshot.retained_s0_cas.cas_digest(),
                retained_s0_census_before_digest: progress.retained_s0_census_before_digest,
                retained_s0_census_after_digest: progress.retained_s0_census_after_digest,
                proxy_topology_compatibility_digest: progress.proxy_topology_compatibility_digest,
                resource_census_digest: if digest_is_zero(progress.resource_census_digest) {
                    OBSERVED_RESOURCE_CENSUS_V2
                } else {
                    progress.resource_census_digest
                },
                raw_outcome_digest: if digest_is_zero(progress.raw_outcome_digest) {
                    OBSERVED_RAW_OUTCOME_V2
                } else {
                    progress.raw_outcome_digest
                },
                submit_admitted_count: progress.submit_admitted_count,
                submit_terminalized_count: progress.submit_terminalized_count,
                control_admitted_count: progress.control_admitted_count,
                control_terminalized_count: progress.control_terminalized_count,
                access_generation_high_water: snapshot.access_generation_high_water,
                completion_runtime_host_epoch: snapshot.writer_runtime_host_epoch,
                completion_snapshot_sequence: snapshot
                    .sequence
                    .checked_add(1)
                    .unwrap_or_else(|| panic!("terminal sequence exhausted")),
                completion_owner_slot_revision: snapshot.owner_slot_revision,
                selection_clock_domain: admission.clock_domain,
                selection_clock_generation: admission.clock_generation,
                admitted_at_nanos: admission.admitted_at_nanos,
                absolute_deadline_nanos: admission.absolute_deadline_nanos,
                selection_observed_at_nanos: progress.selection_observed_at_nanos,
                physical_binding_census: progress.physical_binding_census,
                queryable_declared_bitmap: progress.queryable_declared_bitmap,
                ingress_fenced_bitmap: progress.ingress_fenced_bitmap,
                worker_joined_bitmap: progress.worker_joined_bitmap,
                drain_outcome: progress.drain_outcome,
                remote_observation: progress.remote_observation,
                retained_s0_census_complete: retained_ready,
                retained_s0_ready: retained_ready,
                s1_tls_ready: matches!(
                    progress.remote_observation,
                    RemoteAgentDataPlaneRemoteObservationV2::S1TlsExactRoutesReady
                ),
                s1_acl_ready: matches!(
                    progress.remote_observation,
                    RemoteAgentDataPlaneRemoteObservationV2::S1TlsExactRoutesReady
                ),
                s1_closed: outcome == RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
                s1_listener_released: outcome
                    == RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
                quarantined: outcome == RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined,
            };
            mutate(&mut evidence);
            let evidence = RemoteAgentDataPlaneTerminalEvidenceV2::try_new(evidence)?;
            RemoteAgentDataPlaneTerminalReceiptDraftV2::try_new(
                request, state, evidence, auth_claim,
            )?
            .finalize(signature)
        }

        fn terminal_receipt_v2(
            snapshot: &RemoteAgentAccessSnapshotV2,
            outcome: RemoteAgentDataPlaneTerminalOutcomeV2,
            auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV2,
            signature: &[u8],
            mutate: impl FnOnce(&mut RemoteAgentDataPlaneTerminalEvidenceFieldsV2),
        ) -> RemoteAgentDataPlaneTerminalReceiptV2 {
            try_terminal_receipt_v2(snapshot, outcome, auth_claim, signature, mutate)
                .unwrap_or_else(|error| panic!("PXAU v2 receipt rejected: {error}"))
        }

        fn try_terminal_pending_v2(
            authorized: RemoteAgentAuthorizedTransitionV2,
            outcome: RemoteAgentDataPlaneTerminalOutcomeV2,
            auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV2,
            signature: &[u8],
            mutate: impl FnOnce(&mut RemoteAgentDataPlaneTerminalEvidenceFieldsV2),
        ) -> Result<RemoteAgentPendingAccessSnapshotV2, RemoteAgentAccessStateErrorV2> {
            let receipt = terminal_receipt_v2(
                authorized.snapshot(),
                outcome,
                auth_claim,
                signature,
                mutate,
            );
            let outer = authorized
                .snapshot()
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("terminal predecessor must retain PXRA v2"));
            let request = inner_request_v2(outer)
                .unwrap_or_else(|error| panic!("terminal predecessor PXAR rejected: {error}"));
            let authenticated = receipt
                .verify_runtime_terminal(request, receipt.authentication(), |_, _, _, _, _, _| true)
                .unwrap_or_else(|error| panic!("PXAU v2 authentication rejected: {error}"));
            authorized.try_terminal_successor(authenticated)
        }

        fn active_ready_snapshot_v2() -> RemoteAgentAccessSnapshotV2 {
            let authorized = active_ready_observed_v2();
            let outer = authorized
                .snapshot()
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("Active terminal predecessor must retain PXRA v2"));
            let auth_claim = terminal_auth_for_outer_v2(outer);
            let pending = try_terminal_pending_v2(
                authorized,
                RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
                auth_claim,
                &TERMINAL_SIGNATURE_V2,
                |_| {},
            )
            .unwrap_or_else(|error| panic!("ActiveReady PXRS2 rejected: {error}"));
            readback_pending_v2(pending)
        }

        fn local_ready_observed_v2() -> RemoteAgentAuthorizedTransitionV2 {
            let active = active_ready_snapshot_v2();
            let expected_s1_cas = active
                .resolved_current_s1_cas()
                .unwrap_or_else(|error| panic!("Active PXRS2 S1 CAS rejected: {error}"));
            let request = fresh_local_request_v2(expected_s1_cas);
            let pending = authorize_on_snapshot_v2(
                active,
                &request,
                clock_for_request_v2(&request, 1_000),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("Local PXRS2 authorization rejected: {error}"));
            let mut authorized = authorize_pending_v2(pending);
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::SubmitFenceIntent,
                Some(1_001),
                None,
                |facts| {
                    facts.submit_admitted_count = 3;
                    facts.control_admitted_count = 1;
                    facts.resource_census_digest = OBSERVED_RESOURCE_CENSUS_V2;
                    facts.raw_outcome_digest = OBSERVED_RAW_OUTCOME_V2;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::SubmitFenced,
                None,
                None,
                |facts| {
                    facts.ingress_fenced_bitmap = 0b01;
                    facts.submit_terminalized_count = 3;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::ControlFenceIntent,
                None,
                None,
                |facts| facts.ingress_fenced_bitmap = 0b01,
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::IngressFenced,
                None,
                None,
                |facts| {
                    facts.ingress_fenced_bitmap = SNAPSHOT_V2_EXACT_ROUTE_BITMAP;
                    facts.control_terminalized_count = 1;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::DrainIntent,
                None,
                None,
                |facts| facts.ingress_fenced_bitmap = SNAPSHOT_V2_EXACT_ROUTE_BITMAP,
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::Drained,
                None,
                None,
                |facts| {
                    facts.ingress_fenced_bitmap = SNAPSHOT_V2_EXACT_ROUTE_BITMAP;
                    facts.drain_outcome = RemoteAgentDataPlaneDrainOutcomeV2::Drained;
                },
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::SubmitJoinIntent,
                None,
                None,
                |_| {},
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::SubmitJoined,
                None,
                None,
                |facts| facts.worker_joined_bitmap = 0b01,
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::ControlJoinIntent,
                None,
                None,
                |facts| facts.worker_joined_bitmap = 0b01,
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::WorkersJoined,
                None,
                None,
                |facts| facts.worker_joined_bitmap = SNAPSHOT_V2_EXACT_ROUTE_BITMAP,
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::S1CloseIntent,
                None,
                None,
                |_| {},
            );
            authorized = advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::S1Closed,
                None,
                None,
                |facts| {
                    facts.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::S1Absent;
                },
            );
            let post_deadline = authorized
                .snapshot()
                .admission
                .unwrap_or_else(|| panic!("Local PXRS2 must retain admission facts"))
                .absolute_deadline_nanos
                .checked_add(1)
                .unwrap_or_else(|| panic!("Local post-deadline timestamp overflowed"));
            advance_and_remint_v2(
                authorized,
                RemoteAgentAccessDurablePhaseV2::LocalOnlyObserved,
                None,
                None,
                |facts| facts.selection_observed_at_nanos = post_deadline,
            )
        }

        fn local_ready_snapshot_v2() -> RemoteAgentAccessSnapshotV2 {
            let authorized = local_ready_observed_v2();
            let outer = authorized
                .snapshot()
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("Local terminal predecessor must retain PXRA v2"));
            let auth_claim = terminal_auth_for_outer_v2(outer);
            let pending = try_terminal_pending_v2(
                authorized,
                RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
                auth_claim,
                &TERMINAL_SIGNATURE_V2,
                |_| {},
            )
            .unwrap_or_else(|error| panic!("LocalOnlyReady PXRS2 rejected: {error}"));
            readback_pending_v2(pending)
        }

        fn authorize_existing_snapshot_v2(
            snapshot: RemoteAgentAccessSnapshotV2,
        ) -> RemoteAgentAuthorizedTransitionV2 {
            current_final_v2(snapshot, |_| {})
                .and_then(RemoteAgentCurrentFinalAccessSnapshotV2::try_authorize_existing)
                .unwrap_or_else(|error| panic!("PXRS2 existing readback remint rejected: {error}"))
        }

        fn active_no_effect_snapshot_v2() -> RemoteAgentAccessSnapshotV2 {
            let (_, authorized) = prepared_active_v2(100);
            let outer = authorized
                .snapshot()
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("Active Prepared PXRS2 must retain PXRA v2"));
            let auth_claim = terminal_auth_for_outer_v2(outer);
            let pending = try_terminal_pending_v2(
                authorized,
                RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected,
                auth_claim,
                &TERMINAL_SIGNATURE_V2,
                |_| {},
            )
            .unwrap_or_else(|error| panic!("Active NoEffect PXRS2 rejected: {error}"));
            readback_pending_v2(pending)
        }

        fn prepared_local_v2() -> (
            RemoteAgentAccessRequestV2,
            RemoteAgentAuthorizedTransitionV2,
        ) {
            let active = active_ready_snapshot_v2();
            let expected_s1_cas = active
                .resolved_current_s1_cas()
                .unwrap_or_else(|error| panic!("Active PXRS2 S1 CAS rejected: {error}"));
            let request = fresh_local_request_v2(expected_s1_cas);
            let pending = authorize_on_snapshot_v2(
                active,
                &request,
                clock_for_request_v2(&request, 1_000),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("Local PXRS2 authorization rejected: {error}"));
            (request, authorize_pending_v2(pending))
        }

        fn local_no_effect_snapshot_v2() -> RemoteAgentAccessSnapshotV2 {
            let (_, authorized) = prepared_local_v2();
            let outer = authorized
                .snapshot()
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("Local Prepared PXRS2 must retain PXRA v2"));
            let auth_claim = terminal_auth_for_outer_v2(outer);
            let pending = try_terminal_pending_v2(
                authorized,
                RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected,
                auth_claim,
                &TERMINAL_SIGNATURE_V2,
                |_| {},
            )
            .unwrap_or_else(|error| panic!("Local NoEffect PXRS2 rejected: {error}"));
            readback_pending_v2(pending)
        }

        fn local_fence_observation_v2(
            snapshot: &RemoteAgentAccessSnapshotV2,
            fresh_ticks: u64,
            fact_ticks: u64,
        ) -> RemoteAgentAccessObservedProgressV2 {
            let mut facts = snapshot
                .progress
                .unwrap_or_else(|| panic!("Local Prepared PXRS2 must retain progress"));
            facts.lifecycle_effect = RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted;
            facts.public_phase = RemoteAgentDataPlaneTerminalPhaseV2::IngressFenceIntent;
            facts.selection_observed_at_nanos = fact_ticks;
            let admission = snapshot
                .admission
                .unwrap_or_else(|| panic!("Local Prepared PXRS2 must retain admission"));
            RemoteAgentAccessObservedProgressV2 {
                next_phase: RemoteAgentAccessDurablePhaseV2::SubmitFenceIntent,
                facts,
                candidate_proxy_session_epoch: None,
                fresh_clock: Some(ClockReading::new(
                    admission.clock_domain,
                    admission.clock_generation,
                    MonotonicInstant::from_ticks(fresh_ticks),
                )),
            }
        }

        fn alternate_retained_s0_cas_v2() -> RemoteAgentRetainedS0CasV2 {
            let mut fields = retained_s0_cas_v2().fields();
            fields.expected_descriptor_payload_digest = Digest32::from_bytes([0xee; 32]);
            RemoteAgentRetainedS0CasV2::try_new(fields)
                .unwrap_or_else(|error| panic!("alternate retained S0 CAS rejected: {error}"))
        }

        fn reseal_v2(frame: &mut [u8]) {
            let digest_start = frame
                .len()
                .checked_sub(SNAPSHOT_V2_DIGEST_BYTES)
                .unwrap_or_else(|| panic!("PXRS2 fixture must contain a digest"));
            let digest = snapshot_digest_v2(&frame[..digest_start]);
            frame[digest_start..].copy_from_slice(digest.as_bytes());
        }

        fn expected_edge(
            current: RemoteAgentAccessDurablePhaseV2,
            next: RemoteAgentAccessDurablePhaseV2,
            mode: RemoteAgentDataPlaneTargetModeV2,
        ) -> bool {
            use RemoteAgentAccessDurablePhaseV2 as Phase;
            let normal = match mode {
                RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => [
                    (Phase::PreparedNoEffects, Phase::NoEffectTerminal),
                    (Phase::PreparedNoEffects, Phase::S1OpenIntent),
                    (Phase::S1OpenIntent, Phase::S1Opened),
                    (Phase::S1Opened, Phase::SubmitDeclareIntent),
                    (Phase::SubmitDeclareIntent, Phase::SubmitDeclared),
                    (Phase::SubmitDeclared, Phase::ControlDeclareIntent),
                    (Phase::ControlDeclareIntent, Phase::QueryablesDeclared),
                    (Phase::QueryablesDeclared, Phase::ReadyObserved),
                    (Phase::ReadyObserved, Phase::ActiveReady),
                ]
                .contains(&(current, next)),
                RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => [
                    (Phase::PreparedNoEffects, Phase::NoEffectTerminal),
                    (Phase::PreparedNoEffects, Phase::SubmitFenceIntent),
                    (Phase::SubmitFenceIntent, Phase::SubmitFenced),
                    (Phase::SubmitFenced, Phase::ControlFenceIntent),
                    (Phase::ControlFenceIntent, Phase::IngressFenced),
                    (Phase::IngressFenced, Phase::DrainIntent),
                    (Phase::DrainIntent, Phase::Drained),
                    (Phase::Drained, Phase::SubmitJoinIntent),
                    (Phase::SubmitJoinIntent, Phase::SubmitJoined),
                    (Phase::SubmitJoined, Phase::ControlJoinIntent),
                    (Phase::ControlJoinIntent, Phase::WorkersJoined),
                    (Phase::WorkersJoined, Phase::S1CloseIntent),
                    (Phase::S1CloseIntent, Phase::S1Closed),
                    (Phase::S1Closed, Phase::LocalOnlyObserved),
                    (Phase::LocalOnlyObserved, Phase::LocalOnlyReady),
                ]
                .contains(&(current, next)),
            };
            let failure_source = match mode {
                RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => [
                    Phase::S1OpenIntent,
                    Phase::S1Opened,
                    Phase::SubmitDeclareIntent,
                    Phase::SubmitDeclared,
                    Phase::ControlDeclareIntent,
                    Phase::QueryablesDeclared,
                    Phase::ReadyObserved,
                ]
                .contains(&current),
                RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => [
                    Phase::SubmitFenceIntent,
                    Phase::SubmitFenced,
                    Phase::ControlFenceIntent,
                    Phase::IngressFenced,
                    Phase::DrainIntent,
                    Phase::Drained,
                    Phase::SubmitJoinIntent,
                    Phase::SubmitJoined,
                    Phase::ControlJoinIntent,
                    Phase::WorkersJoined,
                    Phase::S1CloseIntent,
                    Phase::S1Closed,
                    Phase::LocalOnlyObserved,
                ]
                .contains(&current),
            };
            normal
                || (failure_source && matches!(next, Phase::Uncertain | Phase::QuarantineIntent))
                || (current, next) == (Phase::QuarantineIntent, Phase::Quarantined)
        }

        #[test]
        fn pxrs2_header_ceiling_and_absent_roundtrip_are_exact() {
            assert_eq!(SNAPSHOT_V2_HEADER_BYTES, 1_314);
            assert_eq!(MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES, 7_855);
            assert_eq!(
                MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES,
                1_195
            );
            assert_eq!(
                MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES,
                1_314 + (2 * 7_855) + (2 * 1_195) + 32
            );
            assert_eq!(MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES, 19_446);

            let snapshot = initial_snapshot_v2();
            let wire = snapshot.canonical_wire();
            assert_eq!(
                wire.len(),
                SNAPSHOT_V2_HEADER_BYTES + SNAPSHOT_V2_DIGEST_BYTES
            );
            assert_eq!(&wire[0..4], SNAPSHOT_MAGIC);
            assert_eq!(u16::from_be_bytes([wire[4], wire[5]]), SNAPSHOT_V2_VERSION);
            assert_eq!(
                u16::from_be_bytes([wire[6], wire[7]]) as usize,
                SNAPSHOT_V2_HEADER_BYTES
            );
            assert_eq!(
                u32::from_be_bytes(wire[8..12].try_into().unwrap_or([0; 4])) as usize,
                wire.len()
            );
            assert_eq!(&wire[26..42], &[0; 16]);

            let decoded =
                RemoteAgentAccessSnapshotV2::decode(wire, static_identity_v2(identity_v2()))
                    .unwrap_or_else(|error| panic!("PXRS2 roundtrip rejected: {error}"));
            assert_eq!(
                decoded.phase(),
                RemoteAgentAccessDurablePhaseV2::InitializedAbsent
            );
            assert_eq!(decoded.sequence(), 1);
            assert_eq!(decoded.writer_runtime_host_epoch(), RUNTIME_EPOCH);
            assert_eq!(decoded.previous_snapshot_digest(), None);
            assert_eq!(decoded.snapshot_digest(), snapshot.snapshot_digest());
            assert_eq!(decoded.canonical_wire(), wire);
        }

        #[test]
        fn pxrs2_strict_decode_rejects_bounds_checksum_reserved_static_identity_and_v1() {
            let snapshot = initial_snapshot_v2();
            let wire = snapshot.canonical_wire();
            let static_identity = static_identity_v2(identity_v2());

            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(
                    &vec![0; MAX_REMOTE_AGENT_ACCESS_SNAPSHOT_V2_BYTES + 1],
                    static_identity,
                ),
                Err(RemoteAgentAccessStateErrorV2::FrameTooLarge)
            ));
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(&wire[..wire.len() - 1], static_identity),
                Err(RemoteAgentAccessStateErrorV2::Truncated)
            ));

            let mut trailing = wire.to_vec();
            trailing.push(0);
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(&trailing, static_identity),
                Err(RemoteAgentAccessStateErrorV2::InvalidLength)
            ));

            let mut checksum = wire.to_vec();
            let last = checksum.len() - 1;
            checksum[last] ^= 1;
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(&checksum, static_identity),
                Err(RemoteAgentAccessStateErrorV2::ChecksumMismatch)
            ));

            let mut flags = wire.to_vec();
            flags[12..14].copy_from_slice(&u16::MAX.to_be_bytes());
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(&flags, static_identity),
                Err(RemoteAgentAccessStateErrorV2::InvalidFlags)
            ));

            let mut reserved_s1 = wire.to_vec();
            reserved_s1[1_163] = 1;
            reseal_v2(&mut reserved_s1);
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(&reserved_s1, static_identity),
                Err(RemoteAgentAccessStateErrorV2::Contract(_))
            ));

            let mut wrong_static = static_identity;
            wrong_static.owner_target_fingerprint = Digest32::from_bytes([0xe1; 32]);
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(wire, wrong_static),
                Err(RemoteAgentAccessStateErrorV2::IdentityMismatch)
            ));

            let mut stored_lower_only = wire.to_vec();
            stored_lower_only[354] ^= 1;
            reseal_v2(&mut stored_lower_only);
            let structural =
                RemoteAgentAccessSnapshotV2::decode(&stored_lower_only, static_identity)
                    .unwrap_or_else(|error| {
                        panic!("inert lower projection recovery rejected: {error}")
                    });
            assert_ne!(structural.snapshot_digest(), snapshot.snapshot_digest());

            let v1 = prepared(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive);
            assert!(matches!(
                RemoteAgentAccessSnapshotV2::decode(v1.canonical_wire(), static_identity),
                Err(RemoteAgentAccessStateErrorV2::UnsupportedWire)
            ));
            assert!(matches!(
                RemoteAgentAccessSnapshotV1::decode(wire, identity()),
                Err(RemoteAgentAccessStateError::UnsupportedWire)
            ));
        }

        #[test]
        fn pxrs2_phase_codes_and_twenty_eight_by_twenty_eight_graph_are_exact() {
            for (index, phase) in ALL_PHASES.into_iter().enumerate() {
                let encoded = u8::try_from(index + 1).unwrap_or(0);
                assert_eq!(phase as u8, encoded);
                assert_eq!(
                    RemoteAgentAccessDurablePhaseV2::decode(encoded)
                        .unwrap_or_else(|error| panic!("phase {encoded} rejected: {error}")),
                    phase
                );
            }
            assert!(RemoteAgentAccessDurablePhaseV2::decode(0).is_err());
            assert!(RemoteAgentAccessDurablePhaseV2::decode(29).is_err());

            for (mode, expected_count) in [
                (RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive, 24),
                (
                    RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
                    42,
                ),
            ] {
                let mut actual_count = 0;
                for current in ALL_PHASES {
                    for next in ALL_PHASES {
                        let actual = valid_phase_successor_v2(current, next, mode);
                        assert_eq!(
                            actual,
                            expected_edge(current, next, mode),
                            "unexpected {mode:?} edge {current:?} -> {next:?}"
                        );
                        actual_count += usize::from(actual);
                    }
                }
                assert_eq!(actual_count, expected_count);
            }
        }

        #[test]
        fn pxrs2_structural_decode_needs_full_current_identity_and_rejects_old_epoch_authority() {
            let request = active_request_v2();
            let snapshot = initial_snapshot_v2();
            let static_identity = static_identity_v2(identity_v2());
            let mut different_stored_lower = snapshot.canonical_wire().to_vec();
            different_stored_lower[354] ^= 1;
            reseal_v2(&mut different_stored_lower);
            let structural =
                RemoteAgentAccessSnapshotV2::decode(&different_stored_lower, static_identity)
                    .unwrap_or_else(|error| panic!("static-only recovery rejected: {error}"));
            assert!(matches!(
                authorize_on_snapshot_v2(
                    structural,
                    &request,
                    clock_for_request_v2(&request, 1),
                    |facts| facts.identity = identity_v2(),
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker)
            ));

            let old_epoch =
                RemoteAgentAccessSnapshotV2::decode(snapshot.canonical_wire(), static_identity)
                    .unwrap_or_else(|error| {
                        panic!("old-epoch structural recovery rejected: {error}")
                    });
            assert_eq!(old_epoch.writer_runtime_host_epoch(), RUNTIME_EPOCH);
            assert_eq!(old_epoch.sequence(), 1);
            assert!(matches!(
                authorize_on_snapshot_v2(
                    old_epoch,
                    &request,
                    clock_for_request_v2(&request, 1),
                    |facts| facts.runtime_host_epoch = RUNTIME_EPOCH + 1,
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker)
            ));
        }

        #[test]
        fn pxrs2_active_authorization_requires_exact_current_final_and_two_cas_values() {
            let request = active_request_v2();
            let initial = initial_snapshot_v2();
            let initial_digest = initial.snapshot_digest();
            let initial_wire = initial.canonical_wire().to_vec();
            let authorized = authorize_on_snapshot_v2(
                initial,
                &request,
                clock_for_request_v2(&request, 7),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("exact Active authorization rejected: {error}"));
            let prepared = authorized.snapshot();
            assert_eq!(
                prepared.phase(),
                RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
            );
            assert_eq!(
                prepared.mode(),
                Some(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive)
            );
            assert_eq!(prepared.sequence(), 2);
            assert_eq!(prepared.previous_snapshot_digest(), Some(initial_digest));
            assert_eq!(prepared.owner_slot_revision(), 1);
            assert_eq!(prepared.access_generation_high_water(), 0);
            assert_eq!(prepared.candidate_access_generation(), None);
            assert_eq!(prepared.candidate_proxy_session_epoch(), None);
            assert_eq!(prepared.retained_s0_cas, request.retained_s0_cas());
            assert_eq!(prepared.expected_s1_cas, request.expected_s1_cas());
            assert_eq!(
                initial_wire.as_slice(),
                initial_snapshot_v2().canonical_wire()
            );

            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &request,
                    clock_for_request_v2(&request, 7),
                    |facts| {
                        facts.identity.lower_capability_projection_digest =
                            Digest32::from_bytes([0xe2; 32]);
                    },
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker)
            ));
            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &request,
                    clock_for_request_v2(&request, 7),
                    |facts| facts.retained_s0_cas = alternate_retained_s0_cas_v2(),
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker)
            ));
            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &request,
                    clock_for_request_v2(&request, 7),
                    |facts| facts.submit_binding_epoch += 1,
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidCurrentFinalMarker)
            ));

            let wrong_s1_request = rebuilt_active_request_v2(
                RemoteAgentActiveS1CasV2::try_expect_absent(0, 2)
                    .unwrap_or_else(|error| panic!("alternate absent S1 CAS rejected: {error}")),
                None,
                RUNTIME_EPOCH,
            );
            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &wrong_s1_request,
                    clock_for_request_v2(&wrong_s1_request, 7),
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::CasMismatch)
            ));
        }

        #[test]
        fn pxrs2_public_outer_marker_and_raw_clock_cannot_bypass_private_apply_admission() {
            let source = include_str!("remote_agent_access_state.rs");
            let signature_start = source
                .find("pub(crate) fn try_authorize_fresh(")
                .unwrap_or_else(|| panic!("PXRS2 fresh authority entrypoint must exist"));
            let signature_tail = &source[signature_start..];
            let signature_end = signature_tail
                .find(") -> Result<RemoteAgentPendingAccessSnapshotV2")
                .unwrap_or_else(|| panic!("PXRS2 fresh authority signature must terminate"));
            let signature = &signature_tail[..signature_end];
            assert!(signature.contains("VerifiedRemoteAgentAccessApplyIngressV2<'_>"));
            assert!(signature.contains("RemoteAgentDurableReplayCheckedV2"));
            assert!(!signature.contains("ControllerAuthenticatedRemoteAgentAccessRequestV2"));
            assert!(!signature.contains("ClockReading"));

            let historical = outer_request_template_v2();
            let public_outer = historical
                .verify_controller_apply_request(
                    historical.carrier(),
                    |_, _, _, _, _, _| true,
                    |_, _, _, _, _| true,
                )
                .unwrap_or_else(|error| panic!("public outer marker rejected: {error}"));
            let reading = clock_for_request_v2(&historical, 1);
            assert!(
                admission_policy_v2(&historical)
                    .verify_remote_agent_access_apply_ingress_v2(
                        public_outer,
                        historical.carrier(),
                        reading,
                    )
                    .is_err(),
                "a public composite marker and raw caller clock must not mint Runtime admission",
            );
        }

        #[test]
        fn pxrs2_real_apply_admission_facts_equal_state_framing_before_pending() {
            let request = active_request_v2();
            let clock = clock_for_request_v2(&request, 9);
            let marker = verified_ingress_v2(&request, clock)
                .unwrap_or_else(|error| panic!("real PXAR11 admission rejected: {error}"));
            let expected = derive_admission_facts_v2(&request, clock.now().value())
                .unwrap_or_else(|error| panic!("state admission framing rejected: {error}"));
            let fresh = bind_fresh_request_v2(marker, RUNTIME_EPOCH)
                .unwrap_or_else(|error| panic!("exact admission/state pairing rejected: {error}"));
            assert_eq!(fresh.request, &request);
            assert_eq!(fresh.admission, expected);

            let other_request = rebuilt_active_request_with_identities_v2(
                RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                    .unwrap_or_else(|error| panic!("initial absent S1 CAS rejected: {error}")),
                ApplyOperationId::from_bytes([0xe3; 16]),
                &[0xe4; 16],
                &[0xe5; 16],
                &[0xe6; 16],
            );
            let other_clock = clock_for_request_v2(&other_request, 9);
            let other_marker = verified_ingress_v2(&other_request, other_clock)
                .unwrap_or_else(|error| panic!("other PXAR11 admission rejected: {error}"));
            let initial = initial_snapshot_v2();
            let mismatched_replay =
                RemoteAgentDurableReplayCheckedV2::from_durable_replay_ledger_for_test(
                    &initial,
                    &request,
                    clock.now().value(),
                    &[],
                    &[],
                    &[],
                )
                .unwrap_or_else(|error| panic!("durable replay marker rejected: {error}"));
            assert!(matches!(
                current_final_v2(initial, |_| {}).and_then(|current| {
                    current.try_authorize_fresh(other_marker, mismatched_replay)
                }),
                Err(RemoteAgentAccessStateErrorV2::InvalidReplayAuthority)
            ));
        }

        #[test]
        fn pxrs2_fresh_active_admission_pins_budget_clock_epoch_and_absolute_deadline() {
            let template = active_inner_request_v2();
            let operation_timeout = template
                .target_execution()
                .profile()
                .operation_timeout_nanos();
            let exact_temporal = template
                .temporal()
                .try_reduce_remaining(BoundedDuration::from_nanos(operation_timeout))
                .unwrap_or_else(|error| panic!("exact remaining budget rejected: {error}"));
            let exact_request = rebuilt_active_request_v2(
                RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                    .unwrap_or_else(|error| panic!("initial absent S1 CAS rejected: {error}")),
                Some(exact_temporal),
                RUNTIME_EPOCH,
            );
            let exact = authorize_on_snapshot_v2(
                initial_snapshot_v2(),
                &exact_request,
                clock_for_request_v2(&exact_request, 11),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("exact-budget authorization rejected: {error}"));
            let admission = exact
                .snapshot()
                .admission
                .unwrap_or_else(|| panic!("Prepared PXRS2 must retain admission"));
            assert_eq!(admission.admitted_at_nanos, 11);
            assert_eq!(admission.absolute_deadline_nanos, 11 + operation_timeout);

            let short_temporal = template
                .temporal()
                .try_reduce_remaining(BoundedDuration::from_nanos(operation_timeout - 1))
                .unwrap_or_else(|error| panic!("short temporal shape rejected: {error}"));
            assert!(
                RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
                    template.target_execution().clone(),
                    template.provenance(),
                    template.control_commitment().control().clone(),
                    short_temporal,
                    template.expected_runtime_store_instance_id(),
                    template.authentication().claim().clone(),
                )
                .is_err()
            );

            let request = active_request_v2();
            let inner = inner_request_v2(&request)
                .unwrap_or_else(|error| panic!("Active inner request rejected: {error}"));
            let wrong_domain = ClockReading::new(
                ClockDomainRef::from_bytes([0xfe; 16]),
                inner.temporal().target_clock_generation(),
                MonotonicInstant::from_ticks(1),
            );
            assert!(matches!(
                authorize_on_snapshot_v2(initial_snapshot_v2(), &request, wrong_domain, |_| {}),
                Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)
            ));
            let wrong_generation = ClockReading::new(
                inner.temporal().target_clock_domain(),
                ClockGeneration::try_new(inner.temporal().target_clock_generation().value() + 1)
                    .unwrap_or_else(|error| panic!("alternate clock generation rejected: {error}")),
                MonotonicInstant::from_ticks(1),
            );
            assert!(matches!(
                authorize_on_snapshot_v2(initial_snapshot_v2(), &request, wrong_generation, |_| {},),
                Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)
            ));
            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &request,
                    clock_for_request_v2(&request, 0),
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)
            ));
            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &request,
                    clock_for_request_v2(&request, u64::MAX - operation_timeout + 1),
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::DeadlineOverflow)
            ));

            let future_epoch_request = rebuilt_active_request_v2(
                RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                    .unwrap_or_else(|error| panic!("initial absent S1 CAS rejected: {error}")),
                None,
                RUNTIME_EPOCH + 1,
            );
            assert!(matches!(
                authorize_on_snapshot_v2(
                    initial_snapshot_v2(),
                    &future_epoch_request,
                    clock_for_request_v2(&future_epoch_request, 1),
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidFreshRequest)
            ));
        }

        #[test]
        fn pxrs2_active_first_intent_allocates_revision_generation_and_proxy_epoch_once() {
            let (request, prepared) = prepared_active_v2(13);
            let prepared_digest = prepared.snapshot().snapshot_digest();
            let retained_s0 = prepared.snapshot().retained_s0_cas;
            let deadline = prepared
                .snapshot()
                .admission
                .unwrap_or_else(|| panic!("Prepared PXRS2 must retain admission"))
                .absolute_deadline_nanos;
            let proxy_epoch = [0xc1; 16];
            let observation = active_open_observation_v2(
                prepared.snapshot(),
                Some(proxy_epoch),
                Some(clock_for_request_v2(&request, deadline - 1)),
            );
            let opened = prepared
                .try_observed_successor(observation)
                .unwrap_or_else(|error| panic!("Active first intent rejected: {error}"));
            let snapshot = opened.snapshot();
            assert_eq!(
                snapshot.phase(),
                RemoteAgentAccessDurablePhaseV2::S1OpenIntent
            );
            assert_eq!(snapshot.sequence(), 3);
            assert_eq!(snapshot.previous_snapshot_digest(), Some(prepared_digest));
            assert_eq!(snapshot.owner_slot_revision(), 2);
            assert_eq!(snapshot.access_generation_high_water(), 1);
            assert_eq!(snapshot.candidate_access_generation(), Some(generation(1)));
            assert_eq!(snapshot.candidate_proxy_session_epoch(), Some(proxy_epoch));
            assert_eq!(snapshot.retained_s0_cas, retained_s0);
            assert_eq!(snapshot.expected_s1_cas, request.expected_s1_cas());

            for missing_epoch in [None, Some([0; 16])] {
                let (request, prepared) = prepared_active_v2(13);
                let observation = active_open_observation_v2(
                    prepared.snapshot(),
                    missing_epoch,
                    Some(clock_for_request_v2(&request, 13)),
                );
                assert!(matches!(
                    prepared.try_observed_successor(observation),
                    Err(RemoteAgentAccessStateErrorV2::InvalidGenerationSuccessor)
                ));
            }

            let (_, prepared) = prepared_active_v2(13);
            let observation =
                active_open_observation_v2(prepared.snapshot(), Some(proxy_epoch), None);
            assert!(matches!(
                prepared.try_observed_successor(observation),
                Err(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker)
            ));

            let (request, prepared) = prepared_active_v2(13);
            let deadline = prepared
                .snapshot()
                .admission
                .unwrap_or_else(|| panic!("Prepared PXRS2 must retain admission"))
                .absolute_deadline_nanos;
            let observation = active_open_observation_v2(
                prepared.snapshot(),
                Some(proxy_epoch),
                Some(clock_for_request_v2(&request, deadline)),
            );
            assert!(matches!(
                prepared.try_observed_successor(observation),
                Err(RemoteAgentAccessStateErrorV2::DeadlineExpired)
            ));
        }

        #[test]
        fn pxrs2_active_full_chain_commits_each_edge_and_resolves_exact_self_head() {
            let snapshot = active_ready_snapshot_v2();
            assert_eq!(
                snapshot.phase(),
                RemoteAgentAccessDurablePhaseV2::ActiveReady
            );
            assert_eq!(snapshot.sequence(), 10);
            assert_eq!(snapshot.owner_slot_revision(), 2);
            assert_eq!(snapshot.access_generation_high_water(), 1);
            assert_eq!(snapshot.first_effect_observed_at_nanos, 101);
            assert_eq!(snapshot.head_kind, RemoteAgentAccessHeadKindV2::Active);
            assert!(snapshot.self_head);
            assert_eq!(snapshot.active_access_generation, Some(generation(1)));
            assert_eq!(
                snapshot.active_proxy_session_epoch,
                Some(ACTIVE_PROXY_SESSION_EPOCH_V2)
            );
            assert_eq!(
                snapshot.candidate_access_generation,
                snapshot.active_access_generation
            );
            assert_eq!(
                snapshot.candidate_proxy_session_epoch,
                snapshot.active_proxy_session_epoch
            );
            let progress = snapshot
                .progress
                .unwrap_or_else(|| panic!("ActiveReady PXRS2 must retain progress"));
            assert_eq!(progress.selection_observed_at_nanos, 104);
            assert_eq!(
                progress.queryable_declared_bitmap,
                SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            );
            assert_eq!(progress.submit_admitted_count, 2);
            assert_eq!(progress.submit_terminalized_count, 2);
            assert_eq!(progress.control_admitted_count, 1);
            assert_eq!(progress.control_terminalized_count, 1);
            let current_s1 = snapshot
                .resolved_current_s1_cas()
                .unwrap_or_else(|error| panic!("ActiveReady S1 CAS rejected: {error}"));
            assert_eq!(current_s1.access_generation_high_water(), 1);
            assert_eq!(current_s1.owner_slot_revision(), 2);
            let active = current_s1
                .active()
                .unwrap_or_else(|| panic!("ActiveReady S1 CAS must be active"));
            assert_eq!(active.active_snapshot_sequence, 10);
            assert_eq!(active.active_snapshot_digest, snapshot.snapshot_digest());
            assert_eq!(active.active_access_generation, generation(1));
            assert_eq!(
                active.active_proxy_session_epoch,
                ACTIVE_PROXY_SESSION_EPOCH_V2
            );
            assert!(matches!(
                current_final_v2(snapshot, |_| {})
                    .and_then(RemoteAgentCurrentFinalAccessSnapshotV2::try_authorize_existing),
                Err(RemoteAgentAccessStateErrorV2::InvalidPhaseSuccessor)
            ));
        }

        #[test]
        fn pxrs2_local_full_chain_preserves_first_fence_and_clears_only_current_head() {
            let snapshot = local_ready_snapshot_v2();
            assert_eq!(
                snapshot.phase(),
                RemoteAgentAccessDurablePhaseV2::LocalOnlyReady
            );
            assert_eq!(snapshot.sequence(), 25);
            assert_eq!(snapshot.owner_slot_revision(), 3);
            assert_eq!(snapshot.access_generation_high_water(), 1);
            assert_eq!(snapshot.first_effect_observed_at_nanos, 1_001);
            assert_eq!(snapshot.head_kind, RemoteAgentAccessHeadKindV2::Absent);
            assert!(!snapshot.self_head);
            assert!(snapshot.candidate_access_generation.is_none());
            assert!(snapshot.candidate_proxy_session_epoch.is_none());
            assert_eq!(snapshot.active_access_generation, Some(generation(1)));
            assert_eq!(
                snapshot.active_proxy_session_epoch,
                Some(ACTIVE_PROXY_SESSION_EPOCH_V2)
            );
            let admission = snapshot
                .admission
                .unwrap_or_else(|| panic!("LocalOnlyReady PXRS2 must retain admission"));
            let progress = snapshot
                .progress
                .unwrap_or_else(|| panic!("LocalOnlyReady PXRS2 must retain progress"));
            assert!(snapshot.first_effect_observed_at_nanos < admission.absolute_deadline_nanos);
            assert!(progress.selection_observed_at_nanos > admission.absolute_deadline_nanos);
            assert_eq!(
                progress.ingress_fenced_bitmap,
                SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            );
            assert_eq!(
                progress.worker_joined_bitmap,
                SNAPSHOT_V2_EXACT_ROUTE_BITMAP
            );
            assert_eq!(
                progress.drain_outcome,
                RemoteAgentDataPlaneDrainOutcomeV2::Drained
            );
            let historical = snapshot
                .active_head
                .as_ref()
                .unwrap_or_else(|| panic!("LocalOnlyReady must retain historical Active head"));
            assert_eq!(historical.snapshot_sequence, 10);
            assert_eq!(historical.access_generation, generation(1));
            assert_eq!(
                historical.proxy_session_epoch,
                ACTIVE_PROXY_SESSION_EPOCH_V2
            );
            assert_eq!(
                snapshot
                    .resolved_current_s1_cas()
                    .unwrap_or_else(|error| panic!("LocalOnlyReady S1 CAS rejected: {error}")),
                RemoteAgentActiveS1CasV2::try_expect_absent(1, 3)
                    .unwrap_or_else(|error| panic!("expected Local S1 CAS rejected: {error}"))
            );
        }

        #[test]
        fn pxrs2_terminal_requires_exact_outer_runtime_signer_tuple_and_signature_width() {
            let ready = active_ready_observed_v2().snapshot().clone();
            let outer = ready
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("ReadyObserved PXRS2 must retain PXRA v2"));
            let carrier = outer.carrier();
            let wrong_claims = [
                RemoteAgentDataPlaneTerminalAuthClaimV2::try_new(
                    carrier.runtime_principal(),
                    ApplyAuthKeyRef::from_bytes([0xee; 16]),
                    ApplyAuthAlgorithm::try_new(SNAPSHOT_V2_ED25519_ALGORITHM)
                        .unwrap_or_else(|error| panic!("terminal algorithm rejected: {error}")),
                    SNAPSHOT_V2_ED25519_ALGORITHM_VERSION,
                )
                .unwrap_or_else(|error| panic!("wrong-key terminal claim rejected: {error}")),
                RemoteAgentDataPlaneTerminalAuthClaimV2::try_new(
                    carrier.runtime_principal(),
                    carrier.runtime_response_key(),
                    ApplyAuthAlgorithm::try_new(SNAPSHOT_V2_ED25519_ALGORITHM + 1)
                        .unwrap_or_else(|error| panic!("alternate algorithm rejected: {error}")),
                    SNAPSHOT_V2_ED25519_ALGORITHM_VERSION,
                )
                .unwrap_or_else(|error| panic!("wrong-algorithm terminal claim rejected: {error}")),
                RemoteAgentDataPlaneTerminalAuthClaimV2::try_new(
                    carrier.runtime_principal(),
                    carrier.runtime_response_key(),
                    ApplyAuthAlgorithm::try_new(SNAPSHOT_V2_ED25519_ALGORITHM)
                        .unwrap_or_else(|error| panic!("terminal algorithm rejected: {error}")),
                    SNAPSHOT_V2_ED25519_ALGORITHM_VERSION + 1,
                )
                .unwrap_or_else(|error| panic!("wrong-version terminal claim rejected: {error}")),
            ];
            for claim in wrong_claims {
                let authorized = current_final_v2(ready.clone(), |_| {})
                    .and_then(RemoteAgentCurrentFinalAccessSnapshotV2::try_authorize_existing)
                    .unwrap_or_else(|error| panic!("ReadyObserved remint rejected: {error}"));
                assert!(matches!(
                    try_terminal_pending_v2(
                        authorized,
                        RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
                        claim,
                        &TERMINAL_SIGNATURE_V2,
                        |_| {},
                    ),
                    Err(RemoteAgentAccessStateErrorV2::InvalidTerminalAuthentication)
                ));
            }

            let authorized = current_final_v2(ready, |_| {})
                .and_then(RemoteAgentCurrentFinalAccessSnapshotV2::try_authorize_existing)
                .unwrap_or_else(|error| panic!("ReadyObserved remint rejected: {error}"));
            let auth_claim = terminal_auth_for_outer_v2(
                authorized
                    .snapshot()
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("ReadyObserved PXRS2 must retain PXRA v2")),
            );
            assert!(matches!(
                try_terminal_pending_v2(
                    authorized,
                    RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
                    auth_claim,
                    &[0xab; 63],
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidTerminalAuthentication)
            ));
        }

        #[test]
        fn pxrs2_no_effect_replay_fences_operation_tenure_and_request_nonce_identities() {
            let active_terminal = active_no_effect_snapshot_v2();
            let prior_active = active_terminal
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("Active NoEffect must retain its PXRA v2"))
                .clone();
            assert!(matches!(
                authorize_on_snapshot_v2(
                    active_terminal.clone(),
                    &prior_active,
                    clock_for_request_v2(&prior_active, 200),
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidOperationReplacement)
            ));

            let expected_s1 = active_terminal
                .resolved_current_s1_cas()
                .unwrap_or_else(|error| panic!("Active NoEffect S1 CAS rejected: {error}"));
            let prior_inner = inner_request_v2(&prior_active)
                .unwrap_or_else(|error| panic!("Active NoEffect PXAR rejected: {error}"));
            let prior_control = prior_inner.control_commitment().control();
            let prior_tenure_nonce = prior_control.writer_context().proof().nonce().to_vec();
            let prior_request_nonce = prior_inner.authentication().claim().nonce().to_vec();
            let replacements = [
                (
                    rebuilt_active_request_with_identities_v2(
                        expected_s1,
                        prior_inner.operation_id(),
                        &[0x81; 16],
                        &[0x82; 16],
                        &[0x83; 16],
                    ),
                    RemoteAgentAccessStateErrorV2::InvalidOperationReplacement,
                ),
                (
                    rebuilt_active_request_with_identities_v2(
                        expected_s1,
                        ApplyOperationId::from_bytes([0x84; 16]),
                        &prior_tenure_nonce,
                        &[0x85; 16],
                        &[0x86; 16],
                    ),
                    RemoteAgentAccessStateErrorV2::ReplayDetected,
                ),
                (
                    rebuilt_active_request_with_identities_v2(
                        expected_s1,
                        ApplyOperationId::from_bytes([0x87; 16]),
                        &[0x88; 16],
                        &prior_request_nonce,
                        &[0x89; 16],
                    ),
                    RemoteAgentAccessStateErrorV2::ReplayDetected,
                ),
            ];
            for (request, expected_error) in replacements {
                let result = authorize_on_snapshot_v2(
                    active_terminal.clone(),
                    &request,
                    clock_for_request_v2(&request, 200),
                    |_| {},
                );
                assert!(matches!(
                    (result, expected_error),
                    (
                        Err(RemoteAgentAccessStateErrorV2::InvalidOperationReplacement),
                        RemoteAgentAccessStateErrorV2::InvalidOperationReplacement
                    ) | (
                        Err(RemoteAgentAccessStateErrorV2::ReplayDetected),
                        RemoteAgentAccessStateErrorV2::ReplayDetected
                    )
                ));
            }
            let fresh_active = rebuilt_active_request_with_identities_v2(
                expected_s1,
                ApplyOperationId::from_bytes([0x8a; 16]),
                &[0x8b; 16],
                &[0x8c; 16],
                &[0x8d; 16],
            );
            let fresh_pending = authorize_on_snapshot_v2(
                active_terminal.clone(),
                &fresh_active,
                clock_for_request_v2(&fresh_active, 200),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("fully fresh Active replacement rejected: {error}"));
            assert_eq!(
                fresh_pending.snapshot().phase(),
                RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
            );
            let fresh_authorized = authorize_pending_v2(fresh_pending);
            let fresh_claim = terminal_auth_for_outer_v2(
                fresh_authorized
                    .snapshot()
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("fresh Active Prepared must retain PXRA v2")),
            );
            let fresh_terminal = try_terminal_pending_v2(
                fresh_authorized,
                RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected,
                fresh_claim,
                &TERMINAL_SIGNATURE_V2,
                |_| {},
            )
            .unwrap_or_else(|error| panic!("fresh Active NoEffect rejected: {error}"));
            let fresh_terminal = readback_pending_v2(fresh_terminal);
            let fresh_stored_request = fresh_terminal
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("fresh Active NoEffect must retain PXRA v2"));
            let fresh_inner = inner_request_v2(fresh_stored_request)
                .unwrap_or_else(|error| panic!("fresh Active NoEffect PXAR rejected: {error}"));
            let prior_admission = active_terminal
                .admission
                .unwrap_or_else(|| panic!("prior Active NoEffect must retain admission"));
            let fresh_admission = fresh_terminal
                .admission
                .unwrap_or_else(|| panic!("fresh Active NoEffect must retain admission"));
            let seen_operations = [
                *prior_inner.operation_id().as_bytes(),
                *fresh_inner.operation_id().as_bytes(),
            ];
            let seen_tenure_nonces = [
                prior_admission.tenure_nonce_identity,
                fresh_admission.tenure_nonce_identity,
            ];
            let seen_request_nonces = [
                prior_admission.request_nonce_identity,
                fresh_admission.request_nonce_identity,
            ];
            assert!(matches!(
                authorize_on_snapshot_with_replay_ledger_v2(
                    fresh_terminal,
                    &prior_active,
                    clock_for_request_v2(&prior_active, 300),
                    &seen_operations,
                    &seen_tenure_nonces,
                    &seen_request_nonces,
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::ReplayDetected)
            ));

            let local_terminal = local_no_effect_snapshot_v2();
            let prior_local = local_terminal
                .operation_request
                .as_ref()
                .unwrap_or_else(|| panic!("Local NoEffect must retain its PXRA v2"))
                .clone();
            assert!(matches!(
                authorize_on_snapshot_v2(
                    local_terminal.clone(),
                    &prior_local,
                    clock_for_request_v2(&prior_local, 2_000),
                    |_| {},
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidOperationReplacement)
            ));
            let local_s1 = local_terminal
                .resolved_current_s1_cas()
                .unwrap_or_else(|error| panic!("Local NoEffect S1 CAS rejected: {error}"));
            let fresh_local = rebuilt_local_request_with_identities_v2(
                local_s1,
                ApplyOperationId::from_bytes([0x91; 16]),
                &[0x92; 16],
                &[0x93; 16],
                &[0x94; 16],
            );
            let fresh_pending = authorize_on_snapshot_v2(
                local_terminal,
                &fresh_local,
                clock_for_request_v2(&fresh_local, 2_000),
                |_| {},
            )
            .unwrap_or_else(|error| panic!("fully fresh Local replacement rejected: {error}"));
            assert_eq!(
                fresh_pending.snapshot().phase(),
                RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
            );
        }

        #[test]
        fn pxrs2_terminal_progress_cannot_roll_back_counts_bitmaps_selection_or_digests() {
            let local = local_ready_observed_v2().snapshot().clone();
            let local_claim = terminal_auth_for_outer_v2(
                local
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("LocalOnlyObserved must retain PXRA v2")),
            );
            assert!(matches!(
                try_terminal_pending_v2(
                    authorize_existing_snapshot_v2(local),
                    RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
                    local_claim,
                    &TERMINAL_SIGNATURE_V2,
                    |evidence| {
                        evidence.submit_admitted_count = 0;
                        evidence.submit_terminalized_count = 0;
                        evidence.control_admitted_count = 0;
                        evidence.control_terminalized_count = 0;
                    },
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor)
            ));

            let ready = active_ready_observed_v2().snapshot().clone();
            let claim = terminal_auth_for_outer_v2(
                ready
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("ReadyObserved must retain PXRA v2")),
            );
            assert!(matches!(
                try_terminal_pending_v2(
                    authorize_existing_snapshot_v2(ready.clone()),
                    RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain,
                    claim,
                    &TERMINAL_SIGNATURE_V2,
                    |evidence| {
                        evidence.queryable_declared_bitmap = 0b01;
                        evidence.remote_observation =
                            RemoteAgentDataPlaneRemoteObservationV2::PartialOrConflicting;
                    },
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor)
            ));
            assert!(matches!(
                try_terminal_pending_v2(
                    authorize_existing_snapshot_v2(ready.clone()),
                    RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain,
                    claim,
                    &TERMINAL_SIGNATURE_V2,
                    |evidence| evidence.selection_observed_at_nanos = 103,
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor)
            ));
            assert!(matches!(
                try_terminal_pending_v2(
                    authorize_existing_snapshot_v2(ready),
                    RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain,
                    claim,
                    &TERMINAL_SIGNATURE_V2,
                    |evidence| {
                        evidence.resource_census_digest = Digest32::from_bytes([0xcc; 32])
                    },
                ),
                Err(RemoteAgentAccessStateErrorV2::InvalidProgressSuccessor)
            ));
        }

        #[test]
        fn pxrs2_failure_terminals_may_make_currentness_unknown_but_success_may_not() {
            let (_, prepared) = prepared_active_v2(100);
            let before = prepared
                .snapshot()
                .progress
                .unwrap_or_else(|| panic!("Prepared PXRS2 must retain progress"))
                .retained_s0_census_before_digest;
            let claim = terminal_auth_for_outer_v2(
                prepared
                    .snapshot()
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("Prepared PXRS2 must retain PXRA v2")),
            );
            let no_effect = try_terminal_pending_v2(
                prepared,
                RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected,
                claim,
                &TERMINAL_SIGNATURE_V2,
                |evidence| {
                    evidence.retained_s0_current_cas_digest = zero_digest();
                    evidence.retained_s0_census_after_digest = zero_digest();
                    evidence.physical_binding_census = 0;
                    evidence.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::Unknown;
                    evidence.retained_s0_census_complete = false;
                    evidence.retained_s0_ready = false;
                    evidence.s1_tls_ready = false;
                    evidence.s1_acl_ready = false;
                },
            )
            .unwrap_or_else(|error| panic!("unknown-currentness NoEffect rejected: {error}"));
            let no_effect = readback_pending_v2(no_effect);
            let progress = no_effect
                .progress
                .unwrap_or_else(|| panic!("NoEffect PXRS2 must retain progress"));
            assert_eq!(progress.retained_s0_census_before_digest, before);
            assert!(digest_is_zero(progress.retained_s0_census_after_digest));
            assert_eq!(progress.physical_binding_census, 0);
            assert_eq!(
                progress.remote_observation,
                RemoteAgentDataPlaneRemoteObservationV2::Unknown
            );

            let (_, prepared) = prepared_active_v2(100);
            let started = advance_and_remint_v2(
                prepared,
                RemoteAgentAccessDurablePhaseV2::S1OpenIntent,
                Some(101),
                Some(ACTIVE_PROXY_SESSION_EPOCH_V2),
                |_| {},
            );
            let started_snapshot = started.snapshot().clone();
            let claim = terminal_auth_for_outer_v2(
                started_snapshot
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("S1OpenIntent must retain PXRA v2")),
            );
            let uncertain = try_terminal_pending_v2(
                started,
                RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain,
                claim,
                &TERMINAL_SIGNATURE_V2,
                |evidence| {
                    evidence.retained_s0_current_cas_digest = zero_digest();
                    evidence.retained_s0_census_after_digest = zero_digest();
                    evidence.physical_binding_census = 0;
                    evidence.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::Unknown;
                    evidence.retained_s0_census_complete = false;
                    evidence.retained_s0_ready = false;
                    evidence.s1_tls_ready = false;
                    evidence.s1_acl_ready = false;
                },
            )
            .unwrap_or_else(|error| panic!("unknown-currentness Uncertain rejected: {error}"));
            let uncertain = readback_pending_v2(uncertain);
            assert_eq!(
                uncertain.phase(),
                RemoteAgentAccessDurablePhaseV2::Uncertain
            );
            assert_eq!(
                uncertain
                    .progress
                    .unwrap_or_else(|| panic!("Uncertain PXRS2 must retain progress"))
                    .retained_s0_census_before_digest,
                started_snapshot
                    .progress
                    .unwrap_or_else(|| panic!("S1OpenIntent must retain progress"))
                    .retained_s0_census_before_digest
            );

            let (_, prepared) = prepared_active_v2(100);
            let started = advance_and_remint_v2(
                prepared,
                RemoteAgentAccessDurablePhaseV2::S1OpenIntent,
                Some(101),
                Some(ACTIVE_PROXY_SESSION_EPOCH_V2),
                |_| {},
            );
            let quarantine = advance_and_remint_v2(
                started,
                RemoteAgentAccessDurablePhaseV2::QuarantineIntent,
                None,
                None,
                |_| {},
            );
            let claim = terminal_auth_for_outer_v2(
                quarantine
                    .snapshot()
                    .operation_request
                    .as_ref()
                    .unwrap_or_else(|| panic!("QuarantineIntent must retain PXRA v2")),
            );
            let quarantined = try_terminal_pending_v2(
                quarantine,
                RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined,
                claim,
                &TERMINAL_SIGNATURE_V2,
                |evidence| {
                    evidence.retained_s0_current_cas_digest = zero_digest();
                    evidence.retained_s0_census_after_digest = zero_digest();
                    evidence.physical_binding_census = 0;
                    evidence.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::Unknown;
                    evidence.retained_s0_census_complete = false;
                    evidence.retained_s0_ready = false;
                    evidence.s1_tls_ready = false;
                    evidence.s1_acl_ready = false;
                },
            )
            .unwrap_or_else(|error| panic!("unknown-currentness Quarantined rejected: {error}"));
            assert_eq!(
                readback_pending_v2(quarantined).phase(),
                RemoteAgentAccessDurablePhaseV2::Quarantined
            );

            for (snapshot, outcome) in [
                (
                    active_ready_observed_v2().snapshot().clone(),
                    RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
                ),
                (
                    local_ready_observed_v2().snapshot().clone(),
                    RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
                ),
            ] {
                let claim = terminal_auth_for_outer_v2(
                    snapshot
                        .operation_request
                        .as_ref()
                        .unwrap_or_else(|| panic!("success predecessor must retain PXRA v2")),
                );
                assert!(
                    try_terminal_receipt_v2(
                        &snapshot,
                        outcome,
                        claim,
                        &TERMINAL_SIGNATURE_V2,
                        |evidence| {
                            evidence.retained_s0_current_cas_digest = zero_digest();
                            evidence.retained_s0_census_after_digest = zero_digest();
                            evidence.physical_binding_census = 0;
                            evidence.remote_observation =
                                RemoteAgentDataPlaneRemoteObservationV2::Unknown;
                            evidence.retained_s0_census_complete = false;
                            evidence.retained_s0_ready = false;
                            evidence.s1_tls_ready = false;
                            evidence.s1_acl_ready = false;
                        },
                    )
                    .is_err()
                );
            }
        }

        #[test]
        fn pxrs2_local_first_fence_requires_exact_predeadline_clock_and_survives_cleanup() {
            let (_, prepared) = prepared_local_v2();
            let mismatched = local_fence_observation_v2(prepared.snapshot(), 1_001, 1_002);
            assert!(matches!(
                prepared.try_observed_successor(mismatched),
                Err(RemoteAgentAccessStateErrorV2::InvalidFreshClockMarker)
            ));

            let (_, prepared) = prepared_local_v2();
            let deadline = prepared
                .snapshot()
                .admission
                .unwrap_or_else(|| panic!("Local Prepared PXRS2 must retain admission"))
                .absolute_deadline_nanos;
            let at_deadline = local_fence_observation_v2(prepared.snapshot(), deadline, deadline);
            assert!(matches!(
                prepared.try_observed_successor(at_deadline),
                Err(RemoteAgentAccessStateErrorV2::DeadlineExpired)
            ));

            let (_, prepared) = prepared_local_v2();
            let deadline = prepared
                .snapshot()
                .admission
                .unwrap_or_else(|| panic!("Local Prepared PXRS2 must retain admission"))
                .absolute_deadline_nanos;
            let after_deadline =
                local_fence_observation_v2(prepared.snapshot(), deadline + 1, deadline + 1);
            assert!(matches!(
                prepared.try_observed_successor(after_deadline),
                Err(RemoteAgentAccessStateErrorV2::DeadlineExpired)
            ));

            let (_, prepared) = prepared_local_v2();
            let exact = local_fence_observation_v2(prepared.snapshot(), 1_001, 1_001);
            let pending = prepared
                .try_observed_successor(exact)
                .unwrap_or_else(|error| panic!("exact predeadline Local fence rejected: {error}"));
            assert_eq!(pending.snapshot().first_effect_observed_at_nanos, 1_001);
            assert_eq!(
                pending
                    .snapshot()
                    .progress
                    .unwrap_or_else(|| panic!("SubmitFenceIntent must retain progress"))
                    .selection_observed_at_nanos,
                1_001
            );

            let terminal = local_ready_snapshot_v2();
            let admission = terminal
                .admission
                .unwrap_or_else(|| panic!("LocalOnlyReady must retain admission"));
            let progress = terminal
                .progress
                .unwrap_or_else(|| panic!("LocalOnlyReady must retain progress"));
            assert_eq!(terminal.first_effect_observed_at_nanos, 1_001);
            assert!(terminal.first_effect_observed_at_nanos < admission.absolute_deadline_nanos);
            assert!(progress.selection_observed_at_nanos > admission.absolute_deadline_nanos);
        }
    }
}
