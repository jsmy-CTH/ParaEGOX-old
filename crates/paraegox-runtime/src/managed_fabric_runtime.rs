#![cfg(unix)]

//! RuntimeHost-owned lifecycle adapter and durable successor owner for PXAR v6.
//!
//! This module deliberately does not reinterpret the payload-v5 Runtime journal
//! or its `OneSourceLoop` desired head.  The one-way cutover and successor
//! journal live beside that frozen journal; the legacy store supplies only
//! independently verified installation facts and must be fresh before cutover.

use core::{fmt, future::Future, pin::Pin, time::Duration};
use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use ed25519_dalek::{Signature, Signer, SigningKey};
use paraegox_fabric::{
    ExperimentalRemoteMtlsLinkSnapshotV1, ExperimentalRemoteMtlsObservationErrorV1, FabricService,
    FabricServiceConfig, SessionEndpoint,
};
use paraegox_kernel::digest::{Digest32, Digest32Builder, DigestBuildError};
use paraegox_kernel::identity::RuntimeHostId;
use paraegox_kernel::time::{ClockGeneration, ClockReading, MonotonicDeadline};
use paraegox_runtime_contracts::apply::ExpectedActive;
use paraegox_runtime_contracts::managed_fabric_plan::{
    ManagedFabricApplyRequestV1, ManagedFabricApplyTerminalEvidenceV1,
    ManagedFabricApplyTerminalFactsV1, ManagedFabricApplyTerminalHeadV1,
    ManagedFabricApplyTerminalLifecycleEffectV1, ManagedFabricApplyTerminalOutcomeV1,
    ManagedFabricApplyTerminalReceiptAuthClaimV1, ManagedFabricApplyTerminalReceiptDraftV1,
    ManagedFabricApplyTerminalReceiptV1, ManagedFabricApplyTerminalStateV1,
    ManagedFabricListenEndpointV1, ManagedFabricManifestProjectionV1,
    ManagedFabricTargetExecutionV1, ManagedFabricTargetModeV1,
};
use paraegox_runtime_contracts::managed_service::{
    ManagedServiceGeneration, ManagedServiceLifecycleStage,
};
use paraegox_runtime_contracts::reference_control::ReferenceChannelBindingV1;
use paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentActiveS1CasV2;
use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyAuthKeyRef};
use tokio::sync::RwLock;
use tokio::time::{Instant, timeout_at};

use crate::admission::VerifiedManagedFabricApplyIngressV1;
use crate::managed_fabric_state::{
    ManagedFabricDurableActive, ManagedFabricDurablePending, ManagedFabricDurablePhase,
    ManagedFabricPendingKind, ManagedFabricReplayRecord, ManagedFabricRevisionHighWater,
    ManagedFabricSnapshot, ManagedFabricSnapshotTransition, ManagedFabricStateError,
    ManagedFabricTerminalRecord, ManagedFabricWriterFence,
};
use crate::managed_service_assembly::{
    ManagedServiceAssembly, ManagedServiceAttempt, ManagedServiceCompletion, ManagedServiceContext,
    ManagedServiceFuture, ManagedServiceImplementation, ManagedServiceReadiness,
    ManagedServiceStartupOutcome,
};
use crate::remote_agent_access_state::{
    RemoteAgentAccessDurablePhaseV2, RemoteAgentAccessGenesisCandidateV2,
    RemoteAgentAccessSnapshotV2, RemoteAgentAccessStateErrorV2,
    RemoteAgentAccessStaticIdentityPinsV2,
};
use crate::remote_agent_descriptor_evidence::{
    RemoteAgentDescriptorEvidenceError, RemoteAgentDescriptorEvidenceV1,
};
use crate::runtime_clock::RuntimeClock;
use crate::runtime_control_endpoint::{
    RemoteAgentLiveLowerFactsV2, RemoteAgentLiveLowerProjectionV2,
};
#[cfg(test)]
use crate::runtime_store::RemoteAgentAccessInitializeCommitErrorV2;
use crate::runtime_store::{
    ManagedFabricStore, ManagedFabricStoreError, RemoteAgentAccessAbsentLeaseV2,
    RemoteAgentAccessCommitErrorV2, RemoteAgentAccessGenesisInitializeCommitErrorV2,
    RemoteAgentAccessSameEpochLeaseV2, RemoteAgentAccessStartupSlotV2, RuntimeStore,
};
use crate::task_registry::CancellationSource;

/// Exact Fabric implementation owned by one managed-service assembly.
///
/// The adapter retains no discovery/default path and exposes no raw Zenoh
/// object. `prepare` either translates the predecessor's canonical loopback
/// endpoint or consumes one already validated distributed configuration;
/// `start` opens the sole session, and `stop` consumes and closes it.
pub(crate) struct RuntimeManagedFabricService {
    requested: Option<RuntimeManagedFabricPrepareRequest>,
    prepared: Option<FabricServiceConfig>,
    shared: Arc<RwLock<ManagedFabricSlot>>,
}

enum RuntimeManagedFabricPrepareRequest {
    LoopbackEndpoint(ManagedFabricListenEndpointV1),
    ExactConfig(FabricServiceConfig),
}

impl RuntimeManagedFabricService {
    fn try_from_execution(
        execution: &ManagedFabricTargetExecutionV1,
        generation: ManagedServiceGeneration,
    ) -> Result<(Self, ManagedFabricControlHandle), ManagedFabricRuntimeError> {
        if execution.mode() != ManagedFabricTargetModeV1::OneManagedFabricService {
            return Err(ManagedFabricRuntimeError::ExpectedActiveExecution);
        }
        let endpoint = execution
            .listen_endpoint()
            .ok_or(ManagedFabricRuntimeError::MissingListenEndpoint)?
            .clone();
        if execution.service().is_none() {
            return Err(ManagedFabricRuntimeError::MissingServiceSpec);
        }
        Ok(Self::from_prepare_request(
            RuntimeManagedFabricPrepareRequest::LoopbackEndpoint(endpoint),
            generation,
        ))
    }

    /// Adapts one already validated exact transport configuration to the same
    /// lifecycle owner and generation-fenced slot used by the predecessor.
    /// The distributed mapper remains responsible for producing this config;
    /// this constructor cannot add endpoints, discovery, or another session.
    pub(crate) fn from_exact_config(
        config: FabricServiceConfig,
        generation: ManagedServiceGeneration,
    ) -> (Self, ManagedFabricControlHandle) {
        Self::from_prepare_request(
            RuntimeManagedFabricPrepareRequest::ExactConfig(config),
            generation,
        )
    }

    fn from_prepare_request(
        requested: RuntimeManagedFabricPrepareRequest,
        generation: ManagedServiceGeneration,
    ) -> (Self, ManagedFabricControlHandle) {
        let shared = Arc::new(RwLock::new(ManagedFabricSlot {
            generation,
            state: ManagedFabricSlotState::NotStarted,
            owned_binding_count: 0,
            binding_census_known: true,
        }));
        let handle = ManagedFabricControlHandle {
            generation,
            shared: Arc::downgrade(&shared),
        };
        (
            Self {
                requested: Some(requested),
                prepared: None,
                shared,
            },
            handle,
        )
    }
}

enum ManagedFabricSlotState {
    NotStarted,
    Live(FabricService),
    Stopping,
    Stopped,
}

struct ManagedFabricSlot {
    generation: ManagedServiceGeneration,
    state: ManagedFabricSlotState,
    owned_binding_count: u32,
    binding_census_known: bool,
}

/// Crate-private, generation-fenced access to the one lifecycle-owned Fabric
/// session. The handle cannot construct, replace, or restart a session; it can
/// only run one operation while the exact generation is live. This remains the
/// sole path for a later typed Agent-port installer to share the same session.
#[derive(Clone)]
pub(crate) struct ManagedFabricControlHandle {
    generation: ManagedServiceGeneration,
    shared: Weak<RwLock<ManagedFabricSlot>>,
}

impl ManagedFabricControlHandle {
    #[must_use]
    pub(crate) const fn generation(&self) -> ManagedServiceGeneration {
        self.generation
    }

    pub(crate) async fn binding_census(&self) -> Result<u32, ManagedFabricControlError> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(ManagedFabricControlError::OwnerRetired)?;
        let slot = shared.read().await;
        if slot.generation != self.generation {
            return Err(ManagedFabricControlError::GenerationFenced);
        }
        if !slot.binding_census_known {
            return Err(ManagedFabricControlError::BindingCensusUnknown);
        }
        match slot.state {
            ManagedFabricSlotState::Live(_) => Ok(slot.owned_binding_count),
            ManagedFabricSlotState::NotStarted => Err(ManagedFabricControlError::NotReady),
            ManagedFabricSlotState::Stopping | ManagedFabricSlotState::Stopped => {
                Err(ManagedFabricControlError::OwnerRetired)
            }
        }
    }

    /// Performs one synchronous observation while holding the read fence for
    /// this exact live generation and its exact owner-observed binding census.
    /// The observation receives no ownership or mutation token, performs no
    /// retry, and cannot outlive the slot guard.
    pub(crate) async fn observe_live_fabric_exact_census_once<T>(
        &self,
        expected_binding_census: u32,
        observation: impl FnOnce(&FabricService) -> T,
    ) -> Result<T, ManagedFabricControlError> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(ManagedFabricControlError::OwnerRetired)?;
        let slot = shared.read().await;
        if slot.generation != self.generation {
            return Err(ManagedFabricControlError::GenerationFenced);
        }
        if !slot.binding_census_known {
            return Err(ManagedFabricControlError::BindingCensusUnknown);
        }
        let service = match &slot.state {
            ManagedFabricSlotState::Live(service) => service,
            ManagedFabricSlotState::NotStarted => {
                return Err(ManagedFabricControlError::NotReady);
            }
            ManagedFabricSlotState::Stopping | ManagedFabricSlotState::Stopped => {
                return Err(ManagedFabricControlError::OwnerRetired);
            }
        };
        if slot.owned_binding_count != expected_binding_census {
            return Err(ManagedFabricControlError::BindingCensusMismatch);
        }
        Ok(observation(service))
    }

    pub(crate) async fn with_live_fabric<T>(
        &self,
        operation: impl for<'fabric> FnOnce(
            &'fabric FabricService,
        )
            -> Pin<Box<dyn Future<Output = T> + Send + 'fabric>>,
    ) -> Result<T, ManagedFabricControlError> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(ManagedFabricControlError::OwnerRetired)?;
        let slot = shared.read().await;
        if slot.generation != self.generation {
            return Err(ManagedFabricControlError::GenerationFenced);
        }
        match &slot.state {
            ManagedFabricSlotState::Live(service) => Ok(operation(service).await),
            ManagedFabricSlotState::NotStarted => Err(ManagedFabricControlError::NotReady),
            ManagedFabricSlotState::Stopping | ManagedFabricSlotState::Stopped => {
                Err(ManagedFabricControlError::OwnerRetired)
            }
        }
    }

    /// Captures one experimental remote-mTLS link snapshot from the exact live
    /// generation. The caller supplies one absolute reactor deadline shared by
    /// write-fence acquisition and the Session observation; this method never
    /// retries or reopens a session.
    pub(crate) async fn observe_experimental_remote_mtls_links_once(
        &self,
        deadline: Instant,
    ) -> Result<ExperimentalRemoteMtlsLinkSnapshotV1, ManagedFabricExperimentalSnapshotError> {
        let shared =
            self.shared
                .upgrade()
                .ok_or(ManagedFabricExperimentalSnapshotError::Control(
                    ManagedFabricControlError::OwnerRetired,
                ))?;
        if Instant::now() >= deadline {
            return Err(ManagedFabricExperimentalSnapshotError::DeadlineExpired);
        }
        let mut slot = timeout_at(deadline, shared.write())
            .await
            .map_err(|_| ManagedFabricExperimentalSnapshotError::DeadlineExpired)?;
        if slot.generation != self.generation {
            return Err(ManagedFabricExperimentalSnapshotError::Control(
                ManagedFabricControlError::GenerationFenced,
            ));
        }
        let service = match &mut slot.state {
            ManagedFabricSlotState::Live(service) => service,
            ManagedFabricSlotState::NotStarted => {
                return Err(ManagedFabricExperimentalSnapshotError::Control(
                    ManagedFabricControlError::NotReady,
                ));
            }
            ManagedFabricSlotState::Stopping | ManagedFabricSlotState::Stopped => {
                return Err(ManagedFabricExperimentalSnapshotError::Control(
                    ManagedFabricControlError::OwnerRetired,
                ));
            }
        };
        if Instant::now() >= deadline {
            return Err(ManagedFabricExperimentalSnapshotError::DeadlineExpired);
        }
        timeout_at(deadline, service.observe_experimental_remote_mtls_links())
            .await
            .map_err(|_| ManagedFabricExperimentalSnapshotError::DeadlineExpired)?
            .map_err(ManagedFabricExperimentalSnapshotError::Observation)
    }

    /// Performs one binding mutation while holding the exact live generation
    /// fence and within one end-to-end lifecycle budget. Timing out before the
    /// write fence is acquired proves no effect; timing out after the mutation
    /// future is admitted is conservatively outcome-uncertain. A successful
    /// operation advances the owner-observed census.
    pub(crate) async fn mutate_live_fabric<T, E>(
        &self,
        mutation: ManagedFabricBindingMutation,
        budget: Duration,
        deadline_error: E,
        operation: impl for<'fabric> FnOnce(
            &'fabric mut FabricService,
        ) -> Pin<
            Box<dyn Future<Output = ManagedFabricMutationDisposition<T, E>> + Send + 'fabric>,
        >,
    ) -> Result<ManagedFabricMutationDisposition<T, E>, ManagedFabricControlError> {
        let shared = self
            .shared
            .upgrade()
            .ok_or(ManagedFabricControlError::OwnerRetired)?;
        let deadline = Instant::now() + budget;
        let mut deadline_error = Some(deadline_error);
        let mut slot = match timeout_at(deadline, shared.write()).await {
            Ok(slot) => slot,
            Err(_) => {
                return Ok(ManagedFabricMutationDisposition::RejectedNoEffect(
                    deadline_error
                        .take()
                        .expect("deadline error is consumed exactly once"),
                ));
            }
        };
        if slot.generation != self.generation {
            return Err(ManagedFabricControlError::GenerationFenced);
        }
        if !slot.binding_census_known {
            return Err(ManagedFabricControlError::BindingCensusUnknown);
        }
        let (retired_count, installed_count) = mutation.counts()?;
        let next_count = slot
            .owned_binding_count
            .checked_sub(retired_count)
            .ok_or(ManagedFabricControlError::BindingCensusUnderflow)?
            .checked_add(installed_count)
            .ok_or(ManagedFabricControlError::BindingCensusOverflow)?;
        let outcome = match &mut slot.state {
            ManagedFabricSlotState::Live(service) => {
                if Instant::now() >= deadline {
                    ManagedFabricMutationDisposition::RejectedNoEffect(
                        deadline_error
                            .take()
                            .expect("deadline error is consumed exactly once"),
                    )
                } else {
                    match timeout_at(deadline, operation(service)).await {
                        Ok(outcome) => outcome,
                        Err(_) => ManagedFabricMutationDisposition::Uncertain(
                            deadline_error
                                .take()
                                .expect("deadline error is consumed exactly once"),
                        ),
                    }
                }
            }
            ManagedFabricSlotState::NotStarted => {
                return Err(ManagedFabricControlError::NotReady);
            }
            ManagedFabricSlotState::Stopping | ManagedFabricSlotState::Stopped => {
                return Err(ManagedFabricControlError::OwnerRetired);
            }
        };
        match &outcome {
            ManagedFabricMutationDisposition::Committed(_) => {
                slot.owned_binding_count = next_count;
            }
            ManagedFabricMutationDisposition::RejectedNoEffect(_)
            | ManagedFabricMutationDisposition::RolledBackExact(_) => {}
            ManagedFabricMutationDisposition::Uncertain(_) => {
                slot.binding_census_known = false;
            }
        }
        Ok(outcome)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedFabricBindingMutation {
    InstallNew {
        physical_bindings: u32,
    },
    ReplaceExisting {
        retired_physical_bindings: u32,
        installed_physical_bindings: u32,
    },
    RetireExisting {
        physical_bindings: u32,
    },
}

impl ManagedFabricBindingMutation {
    fn counts(self) -> Result<(u32, u32), ManagedFabricControlError> {
        match self {
            Self::InstallNew { physical_bindings } if physical_bindings != 0 => {
                Ok((0, physical_bindings))
            }
            Self::ReplaceExisting {
                retired_physical_bindings,
                installed_physical_bindings,
            } if retired_physical_bindings != 0 && installed_physical_bindings != 0 => {
                Ok((retired_physical_bindings, installed_physical_bindings))
            }
            Self::RetireExisting { physical_bindings } if physical_bindings != 0 => {
                Ok((physical_bindings, 0))
            }
            _ => Err(ManagedFabricControlError::InvalidBindingMutation),
        }
    }
}

/// Caller-observed physical mutation boundary. Only `Committed` changes the
/// physical binding census. Proven no-effect and exact rollback preserve it;
/// an uncertain result permanently marks the live generation census unknown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ManagedFabricMutationDisposition<T, E> {
    Committed(T),
    RejectedNoEffect(E),
    RolledBackExact(E),
    Uncertain(E),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedFabricControlError {
    NotReady,
    GenerationFenced,
    OwnerRetired,
    BindingCensusUnknown,
    BindingCensusMismatch,
    BindingCensusOverflow,
    BindingCensusUnderflow,
    InvalidBindingMutation,
}

/// Exact failures from the bounded, generation-fenced experimental snapshot
/// path. Transport observations stay distinct from binding mutation outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedFabricExperimentalSnapshotError {
    Control(ManagedFabricControlError),
    DeadlineExpired,
    Observation(ExperimentalRemoteMtlsObservationErrorV1),
}

impl fmt::Display for ManagedFabricExperimentalSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(error) => write!(formatter, "managed Fabric control failed: {error:?}"),
            Self::DeadlineExpired => {
                formatter.write_str("managed Fabric experimental snapshot deadline expired")
            }
            Self::Observation(error) => {
                write!(
                    formatter,
                    "managed Fabric experimental snapshot failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for ManagedFabricExperimentalSnapshotError {}

const TRANSITION_PROJECTION_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-fabric-transition-projection.sha256.v1";
const RESOURCE_CENSUS_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-fabric-resource-census.sha256.v1";
const RAW_OUTCOME_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.managed-fabric-raw-outcome.sha256.v1";
const RECOVERY_QUARANTINE_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-fabric-recovery-quarantine.sha256.v1";
const MAX_SUCCESSOR_REPLAY_RECORDS: usize = 256;

pub(crate) struct ManagedFabricOwnerConfig {
    pub(crate) state_directory: PathBuf,
    pub(crate) store_instance_id: [u8; 32],
    pub(crate) owner_target_fingerprint: Digest32,
    pub(crate) projection: ManagedFabricManifestProjectionV1,
    pub(crate) runtime_host_epoch: u64,
    pub(crate) clock: RuntimeClock,
    pub(crate) response_key_ref: ApplyAuthKeyRef,
    pub(crate) response_signer: SigningKey,
}

pub(crate) struct ManagedFabricRuntimeCore {
    store: ManagedFabricStore,
    snapshot: ManagedFabricSnapshot,
    projection: ManagedFabricManifestProjectionV1,
    runtime_host_epoch: u64,
    clock: RuntimeClock,
    response_key_ref: ApplyAuthKeyRef,
    response_signer: SigningKey,
    cancellation: CancellationSource,
    assembly: Option<ManagedServiceAssembly>,
    fabric_control: Option<ManagedFabricControlHandle>,
    remote_agent_descriptor_evidence: Option<RemoteAgentDescriptorEvidenceV1>,
    remote_agent_access_startup_v2: Option<RemoteAgentAccessStartupSlotV2>,
    remote_agent_access_s0_mutation_frozen_v2: bool,
    #[cfg(test)]
    fail_next_remote_agent_descriptor_post_commit_reverify: bool,
    cleanup_exact_zero: bool,
    recovery_completed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ManagedFabricApplyOutcome {
    Committed(ManagedFabricApplyTerminalReceiptV1),
    Replayed(ManagedFabricApplyTerminalReceiptV1),
}

/// Runtime-observed successor serving facts exposed only after async recovery
/// reaches a stable ready phase. This is not a wire contract; the endpoint
/// uses it as the narrow source of truth when constructing a signed managed
/// serving response for the request-time channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedFabricRecoveredObservation {
    pub(crate) target: RuntimeHostId,
    pub(crate) store_instance_id: [u8; 32],
    pub(crate) projection: ManagedFabricManifestProjectionV1,
    pub(crate) transition_projection_digest: Digest32,
    pub(crate) runtime_host_epoch: u64,
    pub(crate) clock: ClockReading,
    pub(crate) successor_snapshot_sequence: u64,
}

/// Exact predecessor authority that may be transferred to the PXAR-v7 stack
/// owner without replacing or reopening the live Fabric generation.
#[derive(Clone)]
pub(crate) struct ManagedFabricStackCutoverObservation {
    pub(crate) execution: ManagedFabricTargetExecutionV1,
    pub(crate) target_slice_digest: paraegox_runtime_contracts::provenance::TargetSliceDigest,
    pub(crate) generation: ManagedServiceGeneration,
    pub(crate) control: ManagedFabricControlHandle,
}

/// Exact durable PXFT root correlated with the currently live Fabric
/// generation. Historical terminal generation remains intentionally absent:
/// restart recovery can retain the exact receipt while rebuilding the live
/// generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ManagedFabricRetainedRootExportV1 {
    pub(crate) active_pxft_digest: Digest32,
    pub(crate) fabric_generation: ManagedServiceGeneration,
}

/// Exact named-final readback retained inside the live genesis authority.
/// This internal component never escapes without both live observations and
/// the endpoint Pin that fenced their complete interval.
struct RemoteAgentAccessInitializedAbsentReadbackV2 {
    same_epoch: RemoteAgentAccessSameEpochLeaseV2,
    target: RuntimeHostId,
    store_instance_id: [u8; 32],
    runtime_host_epoch: u64,
    initial_absent_s1_cas: RemoteAgentActiveS1CasV2,
}

impl RemoteAgentAccessInitializedAbsentReadbackV2 {
    #[must_use]
    const fn target(&self) -> RuntimeHostId {
        self.target
    }

    #[must_use]
    const fn store_instance_id(&self) -> [u8; 32] {
        self.store_instance_id
    }

    #[must_use]
    const fn runtime_host_epoch(&self) -> u64 {
        self.runtime_host_epoch
    }

    #[must_use]
    const fn initial_absent_s1_cas(&self) -> RemoteAgentActiveS1CasV2 {
        self.initial_absent_s1_cas
    }

    #[must_use]
    const fn committed_snapshot(&self) -> &RemoteAgentAccessSnapshotV2 {
        self.same_epoch.snapshot()
    }

    #[must_use]
    fn committed_canonical_wire(&self) -> &[u8] {
        self.same_epoch.canonical_wire()
    }
}

#[derive(Debug)]
pub(crate) enum RemoteAgentAccessGenesisInitializeErrorV2 {
    Core(ManagedFabricRuntimeError),
    State(RemoteAgentAccessStateErrorV2),
    Commit(RemoteAgentAccessGenesisInitializeCommitErrorV2),
}

/// Exact initialization readback paired with the still-live precommit Pin.
/// Only the endpoint owner can add the required full post-readback observation.
pub(crate) struct RemoteAgentAccessInitializedGenesisBundleV2<'running> {
    readback: RemoteAgentAccessInitializedAbsentReadbackV2,
    precommit_live_lower: RemoteAgentLiveLowerProjectionV2<'running>,
}

impl<'running> RemoteAgentAccessInitializedGenesisBundleV2<'running> {
    #[must_use]
    pub(crate) const fn precommit_live_lower(&self) -> &RemoteAgentLiveLowerProjectionV2<'running> {
        &self.precommit_live_lower
    }

    pub(crate) fn try_verify_post_readback_v2(
        self,
        post_readback_live_lower: RemoteAgentLiveLowerFactsV2,
    ) -> Result<RemoteAgentAccessPostReadbackVerifiedGenesisBundleV2<'running>, Box<Self>> {
        if self.precommit_live_lower.exact_facts() != &post_readback_live_lower {
            return Err(Box::new(self));
        }
        Ok(RemoteAgentAccessPostReadbackVerifiedGenesisBundleV2 {
            readback: self.readback,
            precommit_live_lower: self.precommit_live_lower,
            post_readback_live_lower,
        })
    }
}

/// Move-only post-readback genesis authority. It retains the same restricted
/// endpoint Pin, the exact pre/post live facts, and the sole SameEpoch lease.
/// A later CurrentFinal binder must consume this complete value; this tranche
/// intentionally exposes no binder and no active/S1 transition authority.
pub(crate) struct RemoteAgentAccessPostReadbackVerifiedGenesisBundleV2<'running> {
    readback: RemoteAgentAccessInitializedAbsentReadbackV2,
    precommit_live_lower: RemoteAgentLiveLowerProjectionV2<'running>,
    post_readback_live_lower: RemoteAgentLiveLowerFactsV2,
}

impl<'running> RemoteAgentAccessPostReadbackVerifiedGenesisBundleV2<'running> {
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn target(&self) -> RuntimeHostId {
        self.readback.target()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn store_instance_id(&self) -> [u8; 32] {
        self.readback.store_instance_id()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn runtime_host_epoch(&self) -> u64 {
        self.readback.runtime_host_epoch()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn initial_absent_s1_cas(&self) -> RemoteAgentActiveS1CasV2 {
        self.readback.initial_absent_s1_cas()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn committed_snapshot(&self) -> &RemoteAgentAccessSnapshotV2 {
        self.readback.committed_snapshot()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn committed_canonical_wire(&self) -> &[u8] {
        self.readback.committed_canonical_wire()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn precommit_live_lower(&self) -> &RemoteAgentLiveLowerProjectionV2<'running> {
        &self.precommit_live_lower
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn post_readback_live_lower(&self) -> &RemoteAgentLiveLowerFactsV2 {
        &self.post_readback_live_lower
    }
}

#[derive(Clone, Copy)]
struct TerminalSelection {
    outcome: ManagedFabricApplyTerminalOutcomeV1,
    lifecycle_effect: ManagedFabricApplyTerminalLifecycleEffectV1,
    head: ManagedFabricApplyTerminalHeadV1,
    generation: Option<ManagedServiceGeneration>,
    raw_code: u16,
    raw_context: Option<Digest32>,
}

impl ManagedFabricRuntimeCore {
    fn cutover(
        mut legacy_store: RuntimeStore,
        config: ManagedFabricOwnerConfig,
    ) -> Result<Self, ManagedFabricRuntimeError> {
        let projection_digest = transition_projection_digest(&config.projection)?;
        legacy_store.publish_managed_fabric_cutover_marker(projection_digest)?;
        drop(legacy_store);
        Self::open(config)
    }

    pub(crate) fn cutover_developer_local(
        mut legacy_store: RuntimeStore,
        config: ManagedFabricOwnerConfig,
    ) -> Result<Self, ManagedFabricRuntimeError> {
        let projection_digest = transition_projection_digest(&config.projection)?;
        legacy_store.publish_managed_fabric_cutover_marker(projection_digest)?;
        drop(legacy_store);
        let store = ManagedFabricStore::open_developer_local(
            &config.state_directory,
            config.store_instance_id,
            config.owner_target_fingerprint,
            projection_digest,
        )?;
        Self::from_preopened_store(store, config)
    }

    fn open(config: ManagedFabricOwnerConfig) -> Result<Self, ManagedFabricRuntimeError> {
        let projection_digest = transition_projection_digest(&config.projection)?;
        let mut store = ManagedFabricStore::open(
            &config.state_directory,
            config.store_instance_id,
            config.owner_target_fingerprint,
            projection_digest,
        )?;
        let reopening = store.snapshot_bytes()?.is_some();
        let snapshot = match store.snapshot_bytes()? {
            Some(frame) => ManagedFabricSnapshot::decode(
                frame,
                config.store_instance_id,
                config.owner_target_fingerprint,
                projection_digest,
                &config.projection,
            )?,
            None => {
                let initial = ManagedFabricSnapshot::try_initial(
                    config.store_instance_id,
                    config.owner_target_fingerprint,
                    projection_digest,
                    config.runtime_host_epoch,
                    &config.projection,
                )?;
                store.initialize(initial.canonical_wire())?;
                initial
            }
        };
        if config.runtime_host_epoch == 0
            || config.runtime_host_epoch < snapshot.runtime_host_epoch()
            || (reopening && config.runtime_host_epoch == snapshot.runtime_host_epoch())
            || config.clock.generation().value() == 0
        {
            return Err(ManagedFabricRuntimeError::RuntimeEpochRegressed);
        }
        let cleanup_exact_zero = snapshot.phase == ManagedFabricDurablePhase::ExactZero;
        let remote_agent_descriptor_evidence = load_remote_agent_descriptor_evidence(
            &store,
            config.projection.target(),
            config.store_instance_id,
        )?;
        Ok(Self {
            store,
            snapshot,
            projection: config.projection,
            runtime_host_epoch: config.runtime_host_epoch,
            clock: config.clock,
            response_key_ref: config.response_key_ref,
            response_signer: config.response_signer,
            cancellation: CancellationSource::root(),
            assembly: None,
            fabric_control: None,
            remote_agent_descriptor_evidence,
            remote_agent_access_startup_v2: None,
            remote_agent_access_s0_mutation_frozen_v2: false,
            #[cfg(test)]
            fail_next_remote_agent_descriptor_post_commit_reverify: false,
            cleanup_exact_zero,
            recovery_completed: false,
        })
    }

    pub(crate) fn from_preopened_store(
        mut store: ManagedFabricStore,
        config: ManagedFabricOwnerConfig,
    ) -> Result<Self, ManagedFabricRuntimeError> {
        let projection_digest = transition_projection_digest(&config.projection)?;
        if store.marker().transition_projection_digest() != projection_digest {
            return Err(ManagedFabricRuntimeError::ProjectionMismatch);
        }
        let reopening = store.snapshot_bytes()?.is_some();
        let snapshot = match store.snapshot_bytes()? {
            Some(frame) => ManagedFabricSnapshot::decode(
                frame,
                config.store_instance_id,
                config.owner_target_fingerprint,
                projection_digest,
                &config.projection,
            )?,
            None => {
                let initial = ManagedFabricSnapshot::try_initial(
                    config.store_instance_id,
                    config.owner_target_fingerprint,
                    projection_digest,
                    config.runtime_host_epoch,
                    &config.projection,
                )?;
                store.initialize(initial.canonical_wire())?;
                initial
            }
        };
        if config.runtime_host_epoch == 0
            || config.runtime_host_epoch < snapshot.runtime_host_epoch()
            || (reopening && config.runtime_host_epoch == snapshot.runtime_host_epoch())
            || config.clock.generation().value() == 0
        {
            return Err(ManagedFabricRuntimeError::RuntimeEpochRegressed);
        }
        let cleanup_exact_zero = snapshot.phase == ManagedFabricDurablePhase::ExactZero;
        let remote_agent_descriptor_evidence = load_remote_agent_descriptor_evidence(
            &store,
            config.projection.target(),
            config.store_instance_id,
        )?;
        Ok(Self {
            store,
            snapshot,
            projection: config.projection,
            runtime_host_epoch: config.runtime_host_epoch,
            clock: config.clock,
            response_key_ref: config.response_key_ref,
            response_signer: config.response_signer,
            cancellation: CancellationSource::root(),
            assembly: None,
            fabric_control: None,
            remote_agent_descriptor_evidence,
            remote_agent_access_startup_v2: None,
            remote_agent_access_s0_mutation_frozen_v2: false,
            #[cfg(test)]
            fail_next_remote_agent_descriptor_post_commit_reverify: false,
            cleanup_exact_zero,
            recovery_completed: false,
        })
    }

    fn lookup_terminal(
        &self,
        request: &ManagedFabricApplyRequestV1,
        channel: ReferenceChannelBindingV1,
    ) -> Result<Option<ManagedFabricApplyTerminalReceiptV1>, ManagedFabricRuntimeError> {
        let source_scope = request.provenance().source_scope();
        let operation = request.operation_id();
        let Some(record) =
            self.snapshot.terminals.iter().find(|record| {
                record.source_scope == source_scope && record.operation_id == operation
            })
        else {
            return Ok(None);
        };
        if record.request_digest != request.envelope_request_digest() {
            return Err(ManagedFabricRuntimeError::OperationConflict);
        }
        record
            .receipt
            .validate_against_request(request, channel)
            .map_err(|_| ManagedFabricRuntimeError::TerminalCorrelation)?;
        let signature_bytes = record.receipt.authentication_signature();
        if record.receipt.authentication_key() != self.response_key_ref
            || record.receipt.authentication_algorithm().value() != 1
            || record.receipt.authentication_algorithm_version() != 1
            || signature_bytes.len() != 64
        {
            return Err(ManagedFabricRuntimeError::TerminalCorrelation);
        }
        let signature = Signature::from_slice(signature_bytes)
            .map_err(|_| ManagedFabricRuntimeError::TerminalCorrelation)?;
        let transcript = record
            .receipt
            .signing_transcript()
            .map_err(|_| ManagedFabricRuntimeError::TerminalCorrelation)?;
        self.response_signer
            .verifying_key()
            .verify_strict(transcript.as_bytes(), &signature)
            .map_err(|_| ManagedFabricRuntimeError::TerminalCorrelation)?;
        Ok(Some(record.receipt.clone()))
    }

    /// Returns an already committed terminal after the endpoint has
    /// authenticated the exact request. Temporal generation is deliberately
    /// not rechecked here: a signed terminal remains replayable after a
    /// RuntimeHost restart changes the owner clock generation.
    pub(crate) fn authenticated_terminal_replay(
        &self,
        request: &ManagedFabricApplyRequestV1,
        channel: ReferenceChannelBindingV1,
    ) -> Result<Option<ManagedFabricApplyTerminalReceiptV1>, ManagedFabricRuntimeError> {
        self.validate_request(request, channel)?;
        self.lookup_terminal(request, channel)
    }

    pub(crate) fn clock_reading(&self) -> Result<ClockReading, ManagedFabricRuntimeError> {
        self.clock.reading().map_err(Into::into)
    }

    #[must_use]
    pub(crate) const fn stack_clock(&self) -> RuntimeClock {
        self.clock
    }

    #[must_use]
    pub(crate) const fn runtime_host_epoch(&self) -> u64 {
        self.runtime_host_epoch
    }

    #[must_use]
    pub(crate) const fn store_instance_id(&self) -> [u8; 32] {
        self.snapshot.store_instance_id()
    }

    #[must_use]
    pub(crate) fn owner_target_fingerprint(&self) -> Digest32 {
        self.snapshot.owner_target_fingerprint()
    }

    /// Returns the core-owned, process-lifetime S0 mutation freeze. This bit is
    /// monotonic: historical PXRS bytes remain inert, while an exact
    /// same-epoch final or an uncertain initialization commit permanently
    /// closes same-process S0 mutation.
    #[must_use]
    pub(crate) const fn remote_agent_access_s0_mutation_frozen_v2(&self) -> bool {
        self.remote_agent_access_s0_mutation_frozen_v2
    }

    /// Monotonically closes same-process S0 mutation. Typed-error observers may
    /// call this defensively, but the Runtime core remains the only truth.
    pub(crate) fn latch_remote_agent_access_s0_mutation_freeze_v2(&mut self) {
        self.remote_agent_access_s0_mutation_frozen_v2 = true;
    }

    /// Rejects an S0 mutation after this process has learned that same-epoch
    /// PXRS authority exists or that an initialization commit is uncertain.
    pub(crate) fn require_remote_agent_access_s0_mutation_unfrozen_v2(
        &self,
    ) -> Result<(), ManagedFabricRuntimeError> {
        if self.remote_agent_access_s0_mutation_frozen_v2 {
            Err(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)
        } else {
            Ok(())
        }
    }

    /// Retains the one pre-effect PXRS v2 absent lease produced by this core's
    /// already-open store. Any existing final is rejected by the caller before
    /// core construction and therefore can never enter this owner.
    pub(crate) fn install_remote_agent_access_startup_v2(
        &mut self,
        startup: RemoteAgentAccessStartupSlotV2,
    ) -> Result<(), ManagedFabricRuntimeError> {
        if self.remote_agent_access_startup_v2.is_some()
            || !matches!(&startup, RemoteAgentAccessStartupSlotV2::Absent(_))
        {
            return Err(ManagedFabricRuntimeError::RemoteAgentAccessReconcileRequired);
        }
        self.remote_agent_access_startup_v2 = Some(startup);
        Ok(())
    }

    /// Consumes one owner-minted live-lower marker and publishes only its
    /// sealed sequence-one InitializedAbsent projection. The marker (and thus
    /// the same endpoint Pin) is returned only inside the pending genesis
    /// bundle, so the endpoint can perform the mandatory full post-readback
    /// observation before any final initialization authority escapes.
    pub(crate) fn initialize_remote_agent_access_from_live_lower_v2<'running>(
        &mut self,
        live_lower: RemoteAgentLiveLowerProjectionV2<'running>,
    ) -> Result<
        RemoteAgentAccessInitializedGenesisBundleV2<'running>,
        RemoteAgentAccessGenesisInitializeErrorV2,
    > {
        self.require_remote_agent_access_s0_mutation_unfrozen_v2()
            .map_err(RemoteAgentAccessGenesisInitializeErrorV2::Core)?;
        let facts = live_lower.exact_facts();
        let current_transition_projection_digest = transition_projection_digest(&self.projection)
            .map_err(ManagedFabricRuntimeError::from)
            .map_err(RemoteAgentAccessGenesisInitializeErrorV2::Core)?;
        if facts.target() != self.projection.target()
            || facts.store_instance_id() != self.store_instance_id()
            || facts.owner_target_fingerprint() != self.owner_target_fingerprint()
            || facts.transition_projection_digest() != current_transition_projection_digest
            || facts.runtime_host_epoch() != self.runtime_host_epoch
        {
            return Err(RemoteAgentAccessGenesisInitializeErrorV2::Core(
                ManagedFabricRuntimeError::InvalidDurableState,
            ));
        }
        let candidate = RemoteAgentAccessGenesisCandidateV2::try_from_live_lower_v2(&live_lower)
            .map_err(RemoteAgentAccessGenesisInitializeErrorV2::State)?;
        let readback = self
            .commit_remote_agent_access_initialized_absent_candidate_v2(candidate)
            .map_err(RemoteAgentAccessGenesisInitializeErrorV2::Commit)?;
        Ok(RemoteAgentAccessInitializedGenesisBundleV2 {
            readback,
            precommit_live_lower: live_lower,
        })
    }

    /// Raw structural fixture seam. Production initialization is reachable
    /// only through `initialize_remote_agent_access_from_live_lower_v2`.
    #[cfg(test)]
    fn initialize_remote_agent_access_and_latch_v2(
        &mut self,
        candidate: RemoteAgentAccessSnapshotV2,
    ) -> Result<
        RemoteAgentAccessInitializedAbsentReadbackV2,
        RemoteAgentAccessInitializeCommitErrorV2,
    > {
        if let Err(cause) = self.verify_remote_agent_access_initialized_absent_candidate_v2(
            &candidate,
            candidate.canonical_wire(),
        ) {
            return Err(RemoteAgentAccessCommitErrorV2::Rejected {
                cause,
                candidate: Box::new(candidate),
            });
        }
        let (absent, candidate) = self.take_remote_agent_access_absent_lease_v2(candidate)?;
        let result = self
            .store
            .initialize_remote_agent_access_v2(absent, candidate);
        self.finish_remote_agent_access_initialization_v2(result)
    }

    /// Private exact production writer. The opaque candidate remains intact
    /// through validation, absent-lease acquisition, and store publication.
    fn commit_remote_agent_access_initialized_absent_candidate_v2(
        &mut self,
        candidate: RemoteAgentAccessGenesisCandidateV2,
    ) -> Result<
        RemoteAgentAccessInitializedAbsentReadbackV2,
        RemoteAgentAccessGenesisInitializeCommitErrorV2,
    > {
        let snapshot = candidate.snapshot();
        if let Err(cause) = self.verify_remote_agent_access_initialized_absent_candidate_v2(
            snapshot,
            snapshot.canonical_wire(),
        ) {
            return Err(RemoteAgentAccessCommitErrorV2::Rejected {
                cause,
                candidate: Box::new(candidate),
            });
        }
        let (absent, candidate) = self.take_remote_agent_access_absent_lease_v2(candidate)?;
        let result = self
            .store
            .initialize_remote_agent_access_genesis_v2(absent, candidate);
        self.finish_remote_agent_access_initialization_v2(result)
    }

    /// Strictly proves the candidate is this core's canonical sequence-one
    /// InitializedAbsent value. This check is pure and must run before the
    /// retained Absent lease is taken or any store publication is attempted.
    /// `RemoteAgentAccessSnapshotV2::decode` also enforces the phase-specific
    /// expected-S1 shape: absent, generation high-water zero, revision one.
    fn verify_remote_agent_access_initialized_absent_candidate_v2(
        &self,
        candidate: &RemoteAgentAccessSnapshotV2,
        canonical_wire: &[u8],
    ) -> Result<RemoteAgentActiveS1CasV2, ManagedFabricStoreError> {
        let current_transition_projection_digest =
            transition_projection_digest(&self.projection)
                .map_err(|_| ManagedFabricStoreError::RemoteAgentAccessSnapshotMismatch)?;
        let current_static_identity = RemoteAgentAccessStaticIdentityPinsV2 {
            target: self.projection.target(),
            store_instance_id: self.store_instance_id(),
            owner_target_fingerprint: self.owner_target_fingerprint(),
            transition_projection_digest: current_transition_projection_digest,
        };
        let current_snapshot =
            RemoteAgentAccessSnapshotV2::decode(canonical_wire, current_static_identity)
                .map_err(|_| ManagedFabricStoreError::RemoteAgentAccessSnapshotMismatch)?;
        if &current_snapshot != candidate
            || current_snapshot.phase() != RemoteAgentAccessDurablePhaseV2::InitializedAbsent
            || current_snapshot.sequence() != 1
            || current_snapshot.previous_snapshot_digest().is_some()
            || current_snapshot.writer_runtime_host_epoch() != self.runtime_host_epoch
            || current_snapshot.access_generation_high_water() != 0
            || current_snapshot.owner_slot_revision() != 1
        {
            return Err(ManagedFabricStoreError::RemoteAgentAccessSnapshotMismatch);
        }
        RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
            .map_err(|_| ManagedFabricStoreError::RemoteAgentAccessSnapshotMismatch)
    }

    fn take_remote_agent_access_absent_lease_v2<Candidate>(
        &mut self,
        candidate: Candidate,
    ) -> Result<
        (RemoteAgentAccessAbsentLeaseV2, Candidate),
        RemoteAgentAccessCommitErrorV2<Candidate>,
    > {
        match self.remote_agent_access_startup_v2.take() {
            Some(RemoteAgentAccessStartupSlotV2::Absent(absent)) => Ok((absent, candidate)),
            Some(startup) => {
                self.remote_agent_access_startup_v2 = Some(startup);
                Err(RemoteAgentAccessCommitErrorV2::Rejected {
                    cause: ManagedFabricStoreError::RemoteAgentAccessLeaseMismatch,
                    candidate: Box::new(candidate),
                })
            }
            None => Err(RemoteAgentAccessCommitErrorV2::Rejected {
                cause: ManagedFabricStoreError::RemoteAgentAccessLeaseMismatch,
                candidate: Box::new(candidate),
            }),
        }
    }

    fn finish_remote_agent_access_initialization_v2<Candidate>(
        &mut self,
        result: Result<
            RemoteAgentAccessSameEpochLeaseV2,
            RemoteAgentAccessCommitErrorV2<Candidate>,
        >,
    ) -> Result<
        RemoteAgentAccessInitializedAbsentReadbackV2,
        RemoteAgentAccessCommitErrorV2<Candidate>,
    > {
        match result {
            Ok(same_epoch) => {
                self.latch_remote_agent_access_s0_mutation_freeze_v2();
                let initial_absent_s1_cas = self
                    .verify_remote_agent_access_initialized_absent_candidate_v2(
                        same_epoch.snapshot(),
                        same_epoch.canonical_wire(),
                    )
                    .map_err(RemoteAgentAccessCommitErrorV2::OutcomeUncertain)?;
                Ok(RemoteAgentAccessInitializedAbsentReadbackV2 {
                    same_epoch,
                    target: self.projection.target(),
                    store_instance_id: self.store_instance_id(),
                    runtime_host_epoch: self.runtime_host_epoch,
                    initial_absent_s1_cas,
                })
            }
            Err(error @ RemoteAgentAccessCommitErrorV2::OutcomeUncertain(_)) => {
                self.latch_remote_agent_access_s0_mutation_freeze_v2();
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    fn initialize_remote_agent_access_and_latch_at_failpoint_v2(
        &mut self,
        candidate: RemoteAgentAccessSnapshotV2,
        failpoint: crate::runtime_store::RemoteAgentAccessCommitFailpointV2,
    ) -> Result<
        RemoteAgentAccessInitializedAbsentReadbackV2,
        RemoteAgentAccessInitializeCommitErrorV2,
    > {
        if let Err(cause) = self.verify_remote_agent_access_initialized_absent_candidate_v2(
            &candidate,
            candidate.canonical_wire(),
        ) {
            return Err(RemoteAgentAccessCommitErrorV2::Rejected {
                cause,
                candidate: Box::new(candidate),
            });
        }
        let (absent, candidate) = self.take_remote_agent_access_absent_lease_v2(candidate)?;
        let result = self
            .store
            .initialize_remote_agent_access_v2_at_failpoint(absent, candidate, failpoint);
        self.finish_remote_agent_access_initialization_v2(result)
    }

    /// After the first PXAR-v7 marker has been durably published and exactly
    /// read back, classifies and installs PXRS v2 exactly once. Only a proven
    /// absent final grants the caller permission to resolve a provider or
    /// start an Agent. Existing finals remain frozen structural authority.
    pub(crate) fn adjudicate_remote_agent_access_after_first_stack_marker_v2(
        &mut self,
    ) -> Result<(), ManagedFabricRuntimeError> {
        if self.remote_agent_access_startup_v2.is_some()
            || self.store.managed_agent_stack_projection_digest().is_none()
            || !self.store.remote_agent_access_startup_required_v2()
        {
            return Err(ManagedFabricRuntimeError::RemoteAgentAccessReconcileRequired);
        }
        let static_identity = RemoteAgentAccessStaticIdentityPinsV2 {
            target: self.projection.target(),
            store_instance_id: self.store_instance_id(),
            owner_target_fingerprint: self.owner_target_fingerprint(),
            transition_projection_digest: transition_projection_digest(&self.projection)?,
        };
        let startup = self
            .store
            .adjudicate_remote_agent_access_startup_v2(static_identity, self.runtime_host_epoch)?;
        match startup {
            startup @ RemoteAgentAccessStartupSlotV2::Absent(_) => {
                self.remote_agent_access_startup_v2 = Some(startup);
                Ok(())
            }
            startup @ RemoteAgentAccessStartupSlotV2::SameEpoch(_) => {
                self.remote_agent_access_startup_v2 = Some(startup);
                self.latch_remote_agent_access_s0_mutation_freeze_v2();
                Err(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)
            }
            startup @ RemoteAgentAccessStartupSlotV2::RestartReconcileRequired(_) => {
                self.remote_agent_access_startup_v2 = Some(startup);
                Err(ManagedFabricRuntimeError::RemoteAgentAccessReconcileRequired)
            }
        }
    }

    /// Enforces that any retained Agent-stack authority or existing PXRS v2
    /// final was classified before successor recovery. This gate grants no S1
    /// or transition authority; only an exact absent lease permits recovery.
    pub(crate) fn require_remote_agent_access_startup_v2(
        &self,
    ) -> Result<(), ManagedFabricRuntimeError> {
        let required = self.store.remote_agent_access_startup_required_v2();
        match (&self.remote_agent_access_startup_v2, required) {
            (Some(RemoteAgentAccessStartupSlotV2::Absent(_)), true) | (None, false) => Ok(()),
            (Some(RemoteAgentAccessStartupSlotV2::SameEpoch(_)), _)
            | (Some(RemoteAgentAccessStartupSlotV2::RestartReconcileRequired(_)), _)
            | (Some(_), false)
            | (None, true) => Err(ManagedFabricRuntimeError::RemoteAgentAccessReconcileRequired),
        }
    }

    /// Returns the strictly decoded latest PXDE slot. It is historical bytes,
    /// not current access authority; callers must reverify it against live
    /// stack facts and protected provisioning before use.
    pub(crate) fn latest_remote_agent_descriptor_evidence(
        &self,
    ) -> Option<&RemoteAgentDescriptorEvidenceV1> {
        self.remote_agent_descriptor_evidence.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn fail_next_remote_agent_descriptor_post_commit_reverify_for_test(&mut self) {
        self.fail_next_remote_agent_descriptor_post_commit_reverify = true;
    }

    #[cfg(test)]
    pub(crate) fn take_remote_agent_descriptor_post_commit_reverify_failure_for_test(
        &mut self,
    ) -> bool {
        core::mem::take(&mut self.fail_next_remote_agent_descriptor_post_commit_reverify)
    }

    /// Commits exactly the next PXDE record under the same Runtime writer lock
    /// as the managed Fabric and Agent-stack state. In-memory authority moves
    /// only after the store has synced and read back byte-identical final data.
    pub(crate) fn commit_remote_agent_descriptor_evidence(
        &mut self,
        evidence: RemoteAgentDescriptorEvidenceV1,
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.require_remote_agent_access_s0_mutation_unfrozen_v2()?;
        let sequence_matches = match self.remote_agent_descriptor_evidence.as_ref() {
            Some(previous) => {
                previous
                    .record_sequence()
                    .checked_add(1)
                    .is_some_and(|next| next == evidence.record_sequence())
                    && evidence.previous_record_digest() == Some(previous.record_digest())
            }
            None => evidence.record_sequence() == 1 && evidence.previous_record_digest().is_none(),
        };
        if !sequence_matches
            || evidence.target() != self.projection.target()
            || evidence.runtime_store_instance_id() != self.store_instance_id()
            || evidence.runtime_host_epoch() != self.runtime_host_epoch
        {
            return Err(ManagedFabricRuntimeError::DescriptorEvidenceConflict);
        }
        let expected_record_digest = evidence.record_digest();
        self.store
            .commit_remote_agent_descriptor_evidence(evidence.canonical_wire())?;
        let committed_frame = self
            .store
            .remote_agent_descriptor_evidence_bytes()?
            .ok_or(ManagedFabricRuntimeError::DescriptorEvidenceConflict)?;
        let committed = RemoteAgentDescriptorEvidenceV1::decode(committed_frame)?;
        if committed.record_digest() != expected_record_digest {
            return Err(ManagedFabricRuntimeError::DescriptorEvidenceConflict);
        }
        self.remote_agent_descriptor_evidence = Some(committed);
        Ok(())
    }

    pub(crate) fn managed_agent_stack_projection_digest(&self) -> Option<Digest32> {
        self.store.managed_agent_stack_projection_digest()
    }

    pub(crate) fn managed_agent_stack_snapshot_bytes(
        &self,
    ) -> Result<Option<&[u8]>, ManagedFabricRuntimeError> {
        Ok(self.store.managed_agent_stack_snapshot_bytes()?)
    }

    pub(crate) fn initialize_managed_agent_stack(
        &mut self,
        projection_digest: Digest32,
        snapshot: &[u8],
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.store
            .initialize_managed_agent_stack(projection_digest, snapshot)?;
        Ok(())
    }

    pub(crate) fn commit_managed_agent_stack(
        &mut self,
        snapshot: &[u8],
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.store.commit_managed_agent_stack(snapshot)?;
        Ok(())
    }

    pub(crate) fn managed_model_agent_stack_projection_digest(&self) -> Option<Digest32> {
        self.store.managed_model_agent_stack_projection_digest()
    }

    pub(crate) fn managed_model_agent_stack_snapshot_bytes(
        &self,
    ) -> Result<Option<&[u8]>, ManagedFabricRuntimeError> {
        Ok(self.store.managed_model_agent_stack_snapshot_bytes()?)
    }

    pub(crate) fn initialize_managed_model_agent_stack(
        &mut self,
        projection_digest: Digest32,
        snapshot: &[u8],
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.store
            .initialize_managed_model_agent_stack(projection_digest, snapshot)?;
        Ok(())
    }

    pub(crate) fn commit_managed_model_agent_stack(
        &mut self,
        snapshot: &[u8],
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.store.commit_managed_model_agent_stack(snapshot)?;
        Ok(())
    }

    pub(crate) fn distributed_agent_stack_projection_digest(&self) -> Option<Digest32> {
        self.store.distributed_agent_stack_projection_digest()
    }

    pub(crate) fn distributed_agent_stack_snapshot_bytes(
        &self,
    ) -> Result<Option<&[u8]>, ManagedFabricRuntimeError> {
        Ok(self.store.distributed_agent_stack_snapshot_bytes()?)
    }

    pub(crate) fn initialize_distributed_agent_stack(
        &mut self,
        projection_digest: Digest32,
        snapshot: &[u8],
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.store
            .initialize_distributed_agent_stack(projection_digest, snapshot)?;
        Ok(())
    }

    pub(crate) fn commit_distributed_agent_stack(
        &mut self,
        snapshot: &[u8],
    ) -> Result<(), ManagedFabricRuntimeError> {
        self.store.commit_distributed_agent_stack(snapshot)?;
        Ok(())
    }

    /// Returns the exact already-live PXAR-v6 predecessor only when it has no
    /// installed Agent bindings. This is the sole active cutover reuse seam.
    pub(crate) async fn stack_cutover_observation(
        &self,
    ) -> Result<ManagedFabricStackCutoverObservation, ManagedFabricRuntimeError> {
        if !self.recovery_completed || self.snapshot.phase != ManagedFabricDurablePhase::ActiveReady
        {
            return Err(ManagedFabricRuntimeError::RecoveryNotCompleted);
        }
        let active = self
            .snapshot
            .active
            .as_ref()
            .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
        let control = self
            .control_handle()
            .map_err(|_| ManagedFabricRuntimeError::InvalidDurableState)?;
        if control.generation() != active.generation
            || control
                .binding_census()
                .await
                .map_err(|_| ManagedFabricRuntimeError::InvalidDurableState)?
                != 0
        {
            return Err(ManagedFabricRuntimeError::InvalidDurableState);
        }
        Ok(ManagedFabricStackCutoverObservation {
            execution: active.request.target_execution().clone(),
            target_slice_digest: active.request.target_slice_digest(),
            generation: active.generation,
            control,
        })
    }

    pub(crate) fn stack_live_control(
        &self,
        expected_generation: ManagedServiceGeneration,
    ) -> Result<ManagedFabricControlHandle, ManagedFabricRuntimeError> {
        let control = self
            .control_handle()
            .map_err(|_| ManagedFabricRuntimeError::InvalidDurableState)?;
        if control.generation() != expected_generation {
            return Err(ManagedFabricRuntimeError::InvalidDurableState);
        }
        Ok(control)
    }

    /// Exports the exact correlated ActiveReady PXFT root only for the current
    /// recovered execution and live control generation. The historical PXFT
    /// generation is not compared with the rebuilt physical generation.
    pub(crate) async fn export_active_retained_root_v1(
        &self,
        expected_fabric_execution_digest: Digest32,
        expected_fabric_generation: ManagedServiceGeneration,
    ) -> Result<ManagedFabricRetainedRootExportV1, ManagedFabricRuntimeError> {
        if !self.recovery_completed || self.snapshot.phase != ManagedFabricDurablePhase::ActiveReady
        {
            return Err(ManagedFabricRuntimeError::RecoveryNotCompleted);
        }
        let active = self
            .snapshot
            .active
            .as_ref()
            .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
        let control = self
            .control_handle()
            .map_err(|_| ManagedFabricRuntimeError::InvalidDurableState)?;
        if active.request.target_execution().execution_digest() != expected_fabric_execution_digest
            || active.generation != expected_fabric_generation
            || control.generation() != expected_fabric_generation
        {
            return Err(ManagedFabricRuntimeError::ExpectedActiveExecution);
        }
        let receipt = self
            .lookup_terminal(&active.request, active.response_channel)?
            .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
        if receipt.facts().outcome() != ManagedFabricApplyTerminalOutcomeV1::ActiveReady {
            return Err(ManagedFabricRuntimeError::InvalidDurableState);
        }
        let export = ManagedFabricRetainedRootExportV1 {
            active_pxft_digest: receipt.receipt_digest(),
            fabric_generation: expected_fabric_generation,
        };
        control
            .with_live_fabric(move |_| Box::pin(async move { export }))
            .await
            .map_err(|_| ManagedFabricRuntimeError::InvalidDurableState)
    }

    pub(crate) fn recovered_observation(
        &self,
    ) -> Result<ManagedFabricRecoveredObservation, ManagedFabricRuntimeError> {
        if !self.recovery_completed {
            return Err(ManagedFabricRuntimeError::RecoveryNotCompleted);
        }
        match self.snapshot.phase {
            ManagedFabricDurablePhase::ExactZero
                if self.assembly.is_none()
                    && self.fabric_control.is_none()
                    && self.cleanup_exact_zero => {}
            ManagedFabricDurablePhase::ActiveReady
                if self.assembly.is_some()
                    && self.fabric_control.is_some()
                    && !self.cleanup_exact_zero => {}
            _ => return Err(ManagedFabricRuntimeError::InvalidDurableState),
        }
        Ok(ManagedFabricRecoveredObservation {
            target: self.projection.target(),
            store_instance_id: self.snapshot.store_instance_id(),
            projection: self.projection.clone(),
            transition_projection_digest: transition_projection_digest(&self.projection)?,
            runtime_host_epoch: self.runtime_host_epoch,
            clock: self.clock_reading()?,
            successor_snapshot_sequence: self.snapshot.sequence(),
        })
    }

    pub(crate) async fn apply(
        &mut self,
        request: ManagedFabricApplyRequestV1,
        verified: VerifiedManagedFabricApplyIngressV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<ManagedFabricApplyOutcome, ManagedFabricRuntimeError> {
        if !self.recovery_completed {
            return Err(ManagedFabricRuntimeError::RecoveryNotCompleted);
        }
        self.validate_request(&request, response_channel)?;
        if let Some(receipt) = self.lookup_terminal(&request, response_channel)? {
            return Ok(ManagedFabricApplyOutcome::Replayed(receipt));
        }
        self.require_remote_agent_access_s0_mutation_unfrozen_v2()?;
        if matches!(
            self.snapshot.phase,
            ManagedFabricDurablePhase::Quarantined
                | ManagedFabricDurablePhase::Uncertain
                | ManagedFabricDurablePhase::StartIntent
                | ManagedFabricDurablePhase::ReplaceIntent
                | ManagedFabricDurablePhase::ReplaceOldStopped
                | ManagedFabricDurablePhase::DeactivateIntent
                | ManagedFabricDurablePhase::RecoveryIntent
        ) {
            return Err(ManagedFabricRuntimeError::RecoveryRequired);
        }
        match self.observe_deadline(verified) {
            Ok(()) => {}
            Err(ManagedFabricRuntimeError::DeadlineExpired) => {
                return self
                    .terminalize_authenticated_no_effect(request, response_channel, 10)
                    .await;
            }
            Err(error) => return Err(error),
        }
        let mut transition = match self.admit_transition(&request, verified) {
            Ok(transition) => transition,
            Err(ManagedFabricRuntimeError::ExpectedActiveMismatch) => {
                return self
                    .terminalize_authenticated_no_effect(request, response_channel, 11)
                    .await;
            }
            Err(ManagedFabricRuntimeError::StaleWriter) => {
                return self
                    .terminalize_authenticated_no_effect(request, response_channel, 12)
                    .await;
            }
            Err(ManagedFabricRuntimeError::StaleRevision) => {
                return self
                    .terminalize_authenticated_no_effect(request, response_channel, 13)
                    .await;
            }
            Err(error) => return Err(error),
        };
        match request.target_execution().mode() {
            ManagedFabricTargetModeV1::OneManagedFabricService => {
                self.apply_active(request, verified, response_channel, &mut transition)
                    .await
            }
            ManagedFabricTargetModeV1::EmptyDeactivate => {
                self.apply_empty(request, verified, response_channel, &mut transition)
                    .await
            }
        }
    }

    /// Reconciles a successor snapshot after RuntimeHost restart without ever
    /// assuming that a prior process effect completed. Every new start gets a
    /// fresh durable generation; exact canonical loopback ports are probed
    /// after the recovery intent and before the sole start effect.
    pub(crate) async fn recover(&mut self) -> Result<(), ManagedFabricRuntimeError> {
        if self.recovery_completed {
            return Ok(());
        }
        if self.assembly.is_some() || self.fabric_control.is_some() {
            return Err(ManagedFabricRuntimeError::RecoveryWhileLive);
        }
        match self.snapshot.phase {
            ManagedFabricDurablePhase::ExactZero => {
                self.cleanup_exact_zero = true;
                if self.snapshot.runtime_host_epoch() != self.runtime_host_epoch {
                    self.commit_transition(self.snapshot.transition())?;
                }
                self.recovery_completed = true;
                return Ok(());
            }
            ManagedFabricDurablePhase::Quarantined => {
                return Err(ManagedFabricRuntimeError::RecoveryQuarantined);
            }
            ManagedFabricDurablePhase::DeactivateIntent => {
                return self.recover_deactivate().await;
            }
            ManagedFabricDurablePhase::Uncertain
                if self.snapshot.pending.as_ref().is_some_and(|pending| {
                    pending.kind == ManagedFabricPendingKind::Deactivate
                }) =>
            {
                return self.recover_deactivate().await;
            }
            ManagedFabricDurablePhase::ActiveReady
            | ManagedFabricDurablePhase::StartIntent
            | ManagedFabricDurablePhase::ReplaceIntent
            | ManagedFabricDurablePhase::ReplaceOldStopped
            | ManagedFabricDurablePhase::RecoveryIntent
            | ManagedFabricDurablePhase::Uncertain => {}
        }

        let (request, response_channel) = match self.snapshot.phase {
            ManagedFabricDurablePhase::ActiveReady => {
                let active = self
                    .snapshot
                    .active
                    .as_ref()
                    .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
                (active.request.clone(), active.response_channel)
            }
            _ => {
                let pending = self
                    .snapshot
                    .pending
                    .as_ref()
                    .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
                if pending.request.target_execution().mode()
                    != ManagedFabricTargetModeV1::OneManagedFabricService
                {
                    return Err(ManagedFabricRuntimeError::InvalidDurableState);
                }
                (pending.request.clone(), pending.response_channel)
            }
        };
        let generation = next_generation(self.snapshot.generation_high_water())?;
        let (clock_generation, admitted_at_nanos, deadline_nanos) =
            self.recovery_timing(&request)?;
        let mut intent = self.snapshot.transition();
        intent.generation_high_water = generation.value();
        intent.phase = ManagedFabricDurablePhase::RecoveryIntent;
        intent.pending = Some(ManagedFabricDurablePending {
            kind: ManagedFabricPendingKind::RecoverActive,
            generation: Some(generation),
            admitted_clock_generation: clock_generation,
            admitted_at_nanos,
            deadline_nanos,
            response_channel,
            request: request.clone(),
        });
        intent.quarantine_reason = None;
        self.commit_transition(intent)?;

        if let Err(failure) = self.probe_recovery_ports() {
            return self.quarantine_recovery(failure.reason_digest()?, 40).await;
        }
        if self.pending_deadline_expired()? {
            return self
                .quarantine_recovery(recovery_reason_digest(41, 0, 0)?, 41)
                .await;
        }
        if !self.start_live(&request, generation).await? {
            return self
                .quarantine_recovery(recovery_reason_digest(42, request_port(&request)?, 0)?, 42)
                .await;
        }

        let mut ready = self.snapshot.transition();
        ready.phase = ManagedFabricDurablePhase::ActiveReady;
        ready.active = Some(ManagedFabricDurableActive {
            generation,
            response_channel,
            request: request.clone(),
        });
        ready.pending = None;
        ready.quarantine_reason = None;
        if self.lookup_terminal(&request, response_channel)?.is_none() {
            let receipt = self
                .build_terminal(
                    &request,
                    response_channel,
                    TerminalSelection {
                        outcome: ManagedFabricApplyTerminalOutcomeV1::ActiveReady,
                        lifecycle_effect:
                            ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                        head: ManagedFabricApplyTerminalHeadV1::CommittedIncoming,
                        generation: Some(generation),
                        raw_code: 43,
                        raw_context: None,
                    },
                )
                .await?;
            insert_terminal(&mut ready.terminals, &request, receipt)?;
        }
        if let Err(error) = self.commit_transition(ready) {
            let _ = self.stop_live().await;
            return Err(error);
        }
        self.recovery_completed = true;
        Ok(())
    }

    async fn recover_deactivate(&mut self) -> Result<(), ManagedFabricRuntimeError> {
        let pending = self
            .snapshot
            .pending
            .clone()
            .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
        if pending.kind != ManagedFabricPendingKind::Deactivate
            || pending.request.target_execution().mode()
                != ManagedFabricTargetModeV1::EmptyDeactivate
        {
            return Err(ManagedFabricRuntimeError::InvalidDurableState);
        }
        // A successor sequence at the new RuntimeHost epoch durably records
        // takeover before the port probe can influence recovery selection.
        self.commit_transition(self.snapshot.transition())?;
        if let Err(failure) = self.probe_recovery_ports() {
            return self.quarantine_recovery(failure.reason_digest()?, 44).await;
        }
        self.cleanup_exact_zero = true;
        let mut exact_zero = self.snapshot.transition();
        exact_zero.phase = ManagedFabricDurablePhase::ExactZero;
        exact_zero.active = None;
        exact_zero.pending = None;
        exact_zero.quarantine_reason = None;
        if self
            .lookup_terminal(&pending.request, pending.response_channel)?
            .is_none()
        {
            let receipt = self
                .build_terminal(
                    &pending.request,
                    pending.response_channel,
                    TerminalSelection {
                        outcome: ManagedFabricApplyTerminalOutcomeV1::EmptyExactZero,
                        lifecycle_effect:
                            ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                        head: ManagedFabricApplyTerminalHeadV1::CommittedIncoming,
                        generation: None,
                        raw_code: 45,
                        raw_context: None,
                    },
                )
                .await?;
            insert_terminal(&mut exact_zero.terminals, &pending.request, receipt)?;
        }
        self.commit_transition(exact_zero)?;
        self.recovery_completed = true;
        Ok(())
    }

    fn recovery_timing(
        &self,
        request: &ManagedFabricApplyRequestV1,
    ) -> Result<(ClockGeneration, u64, u64), ManagedFabricRuntimeError> {
        let service = request
            .target_execution()
            .service()
            .ok_or(ManagedFabricRuntimeError::MissingServiceSpec)?;
        let budgets = service.lifecycle_budgets();
        let total = [
            ManagedServiceLifecycleStage::Prepare,
            ManagedServiceLifecycleStage::Start,
            ManagedServiceLifecycleStage::Readiness,
        ]
        .into_iter()
        .try_fold(0_u64, |total, stage| {
            total.checked_add(budgets.for_stage(stage).value())
        })
        .ok_or(ManagedFabricRuntimeError::DeadlineOverflow)?;
        let reading = self.clock.reading()?;
        let deadline = reading
            .now()
            .value()
            .checked_add(total)
            .ok_or(ManagedFabricRuntimeError::DeadlineOverflow)?;
        Ok((reading.generation(), reading.now().value(), deadline))
    }

    fn probe_recovery_ports(&self) -> Result<(), RecoveryProbeFailure> {
        let mut ports = BTreeSet::new();
        if let Some(active) = &self.snapshot.active {
            let port = request_port(&active.request).map_err(|_| RecoveryProbeFailure {
                port: 0,
                raw_os_error: 0,
            })?;
            ports.insert(port);
        }
        if let Some(pending) = &self.snapshot.pending
            && pending.request.target_execution().mode()
                == ManagedFabricTargetModeV1::OneManagedFabricService
        {
            let port = request_port(&pending.request).map_err(|_| RecoveryProbeFailure {
                port: 0,
                raw_os_error: 0,
            })?;
            ports.insert(port);
        }
        if ports.is_empty() && self.snapshot.active.is_some() {
            return Err(RecoveryProbeFailure {
                port: 0,
                raw_os_error: 0,
            });
        }
        for port in ports {
            match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
                Ok(listener) => drop(listener),
                Err(error) => {
                    return Err(RecoveryProbeFailure {
                        port,
                        raw_os_error: error.raw_os_error().unwrap_or(0),
                    });
                }
            }
        }
        Ok(())
    }

    async fn quarantine_recovery(
        &mut self,
        reason: Digest32,
        raw_code: u16,
    ) -> Result<(), ManagedFabricRuntimeError> {
        let pending = self
            .snapshot
            .pending
            .clone()
            .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
        let mut quarantined = self.snapshot.transition();
        quarantined.phase = ManagedFabricDurablePhase::Quarantined;
        quarantined.quarantine_reason = Some(reason);
        let has_terminal = self
            .lookup_terminal(&pending.request, pending.response_channel)?
            .is_some();
        match pending.request.target_execution().mode() {
            ManagedFabricTargetModeV1::OneManagedFabricService => {
                let generation = pending
                    .generation
                    .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
                quarantined.active = Some(ManagedFabricDurableActive {
                    generation,
                    response_channel: pending.response_channel,
                    request: pending.request.clone(),
                });
                quarantined.pending = None;
                if !has_terminal {
                    let receipt = self
                        .build_terminal(
                            &pending.request,
                            pending.response_channel,
                            TerminalSelection {
                                outcome: ManagedFabricApplyTerminalOutcomeV1::Quarantined,
                                lifecycle_effect:
                                    ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                                head: ManagedFabricApplyTerminalHeadV1::CommittedIncoming,
                                generation: Some(generation),
                                raw_code,
                                raw_context: Some(reason),
                            },
                        )
                        .await?;
                    insert_terminal(&mut quarantined.terminals, &pending.request, receipt)?;
                }
            }
            ManagedFabricTargetModeV1::EmptyDeactivate => {
                let generation = quarantined
                    .active
                    .as_ref()
                    .map(|active| active.generation)
                    .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
                if !has_terminal {
                    let receipt = self
                        .build_terminal(
                            &pending.request,
                            pending.response_channel,
                            TerminalSelection {
                                outcome: ManagedFabricApplyTerminalOutcomeV1::Uncertain,
                                lifecycle_effect:
                                    ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                                head: preserved_head(quarantined.active.as_ref()),
                                generation: Some(generation),
                                raw_code,
                                raw_context: Some(reason),
                            },
                        )
                        .await?;
                    insert_terminal(&mut quarantined.terminals, &pending.request, receipt)?;
                }
            }
        }
        self.commit_transition(quarantined)?;
        Err(ManagedFabricRuntimeError::RecoveryQuarantined)
    }

    fn validate_request(
        &self,
        request: &ManagedFabricApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<(), ManagedFabricRuntimeError> {
        request
            .validate_expected_store(self.snapshot.store_instance_id())
            .map_err(|_| ManagedFabricRuntimeError::RequestRejected)?;
        request
            .validate_projection(&self.projection)
            .map_err(|_| ManagedFabricRuntimeError::ProjectionMismatch)?;
        if request.target() != self.projection.target()
            || response_channel.target() != request.target()
        {
            return Err(ManagedFabricRuntimeError::RequestRejected);
        }
        Ok(())
    }

    fn observe_deadline(
        &self,
        verified: VerifiedManagedFabricApplyIngressV1,
    ) -> Result<(), ManagedFabricRuntimeError> {
        let reading = self.clock.reading()?;
        if reading.generation() != verified.clock_generation()
            || reading.now().value() >= verified.deadline_nanos()
        {
            return Err(ManagedFabricRuntimeError::DeadlineExpired);
        }
        Ok(())
    }

    fn admit_transition(
        &self,
        request: &ManagedFabricApplyRequestV1,
        verified: VerifiedManagedFabricApplyIngressV1,
    ) -> Result<ManagedFabricSnapshotTransition, ManagedFabricRuntimeError> {
        self.validate_cas(request)?;
        let control = request.control_commitment().control();
        let writer = control.writer_context();
        let claim = writer.proof().claim();
        let proof_digest = verified.authenticated().proof_envelope_digest();
        let writer_fence = match self.snapshot.writer_fence {
            None => ManagedFabricWriterFence {
                source_scope: claim.source_scope(),
                writer: claim.writer(),
                principal: request.authentication().claim().principal(),
                epoch: claim.epoch().value(),
                proof_envelope_digest: proof_digest,
            },
            Some(current)
                if current.source_scope == claim.source_scope()
                    && current.writer == claim.writer()
                    && current.epoch == claim.epoch().value()
                    && current.proof_envelope_digest == proof_digest =>
            {
                current
            }
            Some(current)
                if current.source_scope == claim.source_scope()
                    && claim.epoch().value() > current.epoch
                    && claim.supersedes_through_epoch().value() >= current.epoch =>
            {
                ManagedFabricWriterFence {
                    source_scope: claim.source_scope(),
                    writer: claim.writer(),
                    principal: request.authentication().claim().principal(),
                    epoch: claim.epoch().value(),
                    proof_envelope_digest: proof_digest,
                }
            }
            Some(_) => return Err(ManagedFabricRuntimeError::StaleWriter),
        };
        let provenance = request.provenance();
        let revision_high_water = match self.snapshot.revision_high_water {
            None => ManagedFabricRevisionHighWater {
                source_scope: provenance.source_scope(),
                revision: provenance.source_revision().value(),
                source_plan_digest: provenance.source_plan_digest(),
            },
            Some(current)
                if current.source_scope == provenance.source_scope()
                    && (provenance.source_revision().value() > current.revision
                        || (provenance.source_revision().value() == current.revision
                            && provenance.source_plan_digest() == current.source_plan_digest)) =>
            {
                ManagedFabricRevisionHighWater {
                    source_scope: provenance.source_scope(),
                    revision: provenance.source_revision().value(),
                    source_plan_digest: provenance.source_plan_digest(),
                }
            }
            Some(_) => return Err(ManagedFabricRuntimeError::StaleRevision),
        };
        let mut transition = self.snapshot.transition();
        transition.writer_fence = Some(writer_fence);
        transition.revision_high_water = Some(revision_high_water);
        insert_replay(
            &mut transition.tenure_nonces,
            ManagedFabricReplayRecord {
                identity: verified.authenticated().tenure_nonce_identity(),
                value_digest: proof_digest,
            },
        )?;
        insert_replay(
            &mut transition.request_nonces,
            ManagedFabricReplayRecord {
                identity: verified.authenticated().request_nonce_identity(),
                value_digest: request.envelope_request_digest(),
            },
        )?;
        insert_replay(
            &mut transition.temporal_lineages,
            ManagedFabricReplayRecord {
                identity: verified.authenticated().temporal_lineage_identity(),
                value_digest: request.envelope_request_digest(),
            },
        )?;
        Ok(transition)
    }

    fn validate_cas(
        &self,
        request: &ManagedFabricApplyRequestV1,
    ) -> Result<(), ManagedFabricRuntimeError> {
        let current = self
            .snapshot
            .active
            .as_ref()
            .map(|active| active.request.target_slice_digest());
        match (
            request.control_commitment().control().expected_active(),
            current,
        ) {
            (ExpectedActive::None, None) => Ok(()),
            (ExpectedActive::Exact(expected), Some(actual)) if expected == actual => Ok(()),
            _ => Err(ManagedFabricRuntimeError::ExpectedActiveMismatch),
        }
    }

    async fn apply_active(
        &mut self,
        request: ManagedFabricApplyRequestV1,
        verified: VerifiedManagedFabricApplyIngressV1,
        response_channel: ReferenceChannelBindingV1,
        transition: &mut ManagedFabricSnapshotTransition,
    ) -> Result<ManagedFabricApplyOutcome, ManagedFabricRuntimeError> {
        let generation = next_generation(self.snapshot.generation_high_water())?;
        let replacing = self.snapshot.active.is_some();
        transition.generation_high_water = generation.value();
        transition.phase = if replacing {
            ManagedFabricDurablePhase::ReplaceIntent
        } else {
            ManagedFabricDurablePhase::StartIntent
        };
        transition.pending = Some(ManagedFabricDurablePending {
            kind: if replacing {
                ManagedFabricPendingKind::Replace
            } else {
                ManagedFabricPendingKind::Start
            },
            generation: Some(generation),
            admitted_clock_generation: verified.clock_generation(),
            admitted_at_nanos: verified.admitted_at_nanos(),
            deadline_nanos: verified.deadline_nanos(),
            response_channel,
            request: request.clone(),
        });
        transition.quarantine_reason = None;
        self.commit_transition(transition.clone())?;

        if self.pending_deadline_expired()? {
            return self
                .terminalize_no_effect_from_intent(request, response_channel, 20)
                .await;
        }

        if replacing {
            if !self.stop_live().await? {
                return self
                    .terminalize_uncertain(
                        request,
                        response_channel,
                        generation,
                        TerminalSelection {
                            outcome: ManagedFabricApplyTerminalOutcomeV1::Uncertain,
                            lifecycle_effect:
                                ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                            head: preserved_head(self.snapshot.active.as_ref()),
                            generation: Some(generation),
                            raw_code: 2,
                            raw_context: None,
                        },
                    )
                    .await;
            }
            let mut old_stopped = self.snapshot.transition();
            old_stopped.phase = ManagedFabricDurablePhase::ReplaceOldStopped;
            self.commit_transition(old_stopped)?;
        }

        if self.pending_deadline_expired()? {
            if replacing {
                return self
                    .terminalize_uncertain(
                        request,
                        response_channel,
                        generation,
                        TerminalSelection {
                            outcome: ManagedFabricApplyTerminalOutcomeV1::Uncertain,
                            lifecycle_effect:
                                ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                            head: preserved_head(self.snapshot.active.as_ref()),
                            generation: Some(generation),
                            raw_code: 22,
                            raw_context: None,
                        },
                    )
                    .await;
            }
            return self
                .terminalize_no_effect_from_intent(request, response_channel, 22)
                .await;
        }
        match self.start_live(&request, generation).await? {
            true => {
                let active = ManagedFabricDurableActive {
                    generation,
                    response_channel,
                    request: request.clone(),
                };
                let mut final_transition = self.snapshot.transition();
                final_transition.phase = ManagedFabricDurablePhase::ActiveReady;
                final_transition.active = Some(active);
                final_transition.pending = None;
                final_transition.quarantine_reason = None;
                let receipt = self
                    .build_terminal(
                        &request,
                        response_channel,
                        TerminalSelection {
                            outcome: ManagedFabricApplyTerminalOutcomeV1::ActiveReady,
                            lifecycle_effect:
                                ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                            head: ManagedFabricApplyTerminalHeadV1::CommittedIncoming,
                            generation: Some(generation),
                            raw_code: 1,
                            raw_context: None,
                        },
                    )
                    .await?;
                insert_terminal(&mut final_transition.terminals, &request, receipt.clone())?;
                if let Err(error) = self.commit_transition(final_transition) {
                    let _ = self.stop_live().await;
                    return Err(error);
                }
                Ok(ManagedFabricApplyOutcome::Committed(receipt))
            }
            false => {
                self.terminalize_uncertain(
                    request,
                    response_channel,
                    generation,
                    TerminalSelection {
                        outcome: ManagedFabricApplyTerminalOutcomeV1::Uncertain,
                        lifecycle_effect:
                            ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                        head: preserved_head(self.snapshot.active.as_ref()),
                        generation: Some(generation),
                        raw_code: 3,
                        raw_context: None,
                    },
                )
                .await
            }
        }
    }

    async fn apply_empty(
        &mut self,
        request: ManagedFabricApplyRequestV1,
        verified: VerifiedManagedFabricApplyIngressV1,
        response_channel: ReferenceChannelBindingV1,
        transition: &mut ManagedFabricSnapshotTransition,
    ) -> Result<ManagedFabricApplyOutcome, ManagedFabricRuntimeError> {
        let had_active = self.snapshot.active.is_some();
        if had_active {
            transition.phase = ManagedFabricDurablePhase::DeactivateIntent;
            transition.pending = Some(ManagedFabricDurablePending {
                kind: ManagedFabricPendingKind::Deactivate,
                generation: None,
                admitted_clock_generation: verified.clock_generation(),
                admitted_at_nanos: verified.admitted_at_nanos(),
                deadline_nanos: verified.deadline_nanos(),
                response_channel,
                request: request.clone(),
            });
            transition.quarantine_reason = None;
            self.commit_transition(transition.clone())?;
            if self.pending_deadline_expired()? {
                return self
                    .terminalize_no_effect_from_intent(request, response_channel, 23)
                    .await;
            }
            if !self.stop_live().await? {
                let generation = self
                    .snapshot
                    .active
                    .as_ref()
                    .map(|active| active.generation)
                    .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
                return self
                    .terminalize_uncertain(
                        request,
                        response_channel,
                        generation,
                        TerminalSelection {
                            outcome: ManagedFabricApplyTerminalOutcomeV1::Uncertain,
                            lifecycle_effect:
                                ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted,
                            head: preserved_head(self.snapshot.active.as_ref()),
                            generation: Some(generation),
                            raw_code: 4,
                            raw_context: None,
                        },
                    )
                    .await;
            }
        }
        let mut final_transition = self.snapshot.transition();
        final_transition.phase = ManagedFabricDurablePhase::ExactZero;
        final_transition.active = None;
        final_transition.pending = None;
        final_transition.quarantine_reason = None;
        let receipt = self
            .build_terminal(
                &request,
                response_channel,
                TerminalSelection {
                    outcome: ManagedFabricApplyTerminalOutcomeV1::EmptyExactZero,
                    lifecycle_effect: if had_active {
                        ManagedFabricApplyTerminalLifecycleEffectV1::MayHaveStarted
                    } else {
                        ManagedFabricApplyTerminalLifecycleEffectV1::ProvenNotStarted
                    },
                    head: ManagedFabricApplyTerminalHeadV1::CommittedIncoming,
                    generation: None,
                    raw_code: if had_active { 5 } else { 6 },
                    raw_context: None,
                },
            )
            .await?;
        insert_terminal(&mut final_transition.terminals, &request, receipt.clone())?;
        self.commit_transition(final_transition)?;
        Ok(ManagedFabricApplyOutcome::Committed(receipt))
    }

    fn pending_deadline_expired(&self) -> Result<bool, ManagedFabricRuntimeError> {
        let pending = self
            .snapshot
            .pending
            .as_ref()
            .ok_or(ManagedFabricRuntimeError::InvalidDurableState)?;
        let reading = self.clock.reading()?;
        Ok(reading.generation() != pending.admitted_clock_generation
            || reading.now().value() >= pending.deadline_nanos)
    }

    async fn terminalize_no_effect_from_intent(
        &mut self,
        request: ManagedFabricApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        raw_code: u16,
    ) -> Result<ManagedFabricApplyOutcome, ManagedFabricRuntimeError> {
        let mut transition = self.snapshot.transition();
        transition.phase = if transition.active.is_some() {
            ManagedFabricDurablePhase::ActiveReady
        } else {
            ManagedFabricDurablePhase::ExactZero
        };
        transition.pending = None;
        transition.quarantine_reason = None;
        let receipt = self
            .build_terminal(
                &request,
                response_channel,
                TerminalSelection {
                    outcome: ManagedFabricApplyTerminalOutcomeV1::NoEffectRejected,
                    lifecycle_effect: ManagedFabricApplyTerminalLifecycleEffectV1::ProvenNotStarted,
                    head: preserved_head(transition.active.as_ref()),
                    generation: None,
                    raw_code,
                    raw_context: None,
                },
            )
            .await?;
        insert_terminal(&mut transition.terminals, &request, receipt.clone())?;
        self.commit_transition(transition)?;
        Ok(ManagedFabricApplyOutcome::Committed(receipt))
    }

    /// Persists an authenticated, correlated rejection without advancing any
    /// writer fence, source revision, or replay high-water.  The terminal is
    /// the sole mutation and reports the Runtime-observed durable head.
    async fn terminalize_authenticated_no_effect(
        &mut self,
        request: ManagedFabricApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        raw_code: u16,
    ) -> Result<ManagedFabricApplyOutcome, ManagedFabricRuntimeError> {
        let mut transition = self.snapshot.transition();
        let receipt = self
            .build_terminal(
                &request,
                response_channel,
                TerminalSelection {
                    outcome: ManagedFabricApplyTerminalOutcomeV1::NoEffectRejected,
                    lifecycle_effect: ManagedFabricApplyTerminalLifecycleEffectV1::ProvenNotStarted,
                    head: preserved_head(transition.active.as_ref()),
                    generation: None,
                    raw_code,
                    raw_context: None,
                },
            )
            .await?;
        insert_terminal(&mut transition.terminals, &request, receipt.clone())?;
        self.commit_transition(transition)?;
        Ok(ManagedFabricApplyOutcome::Committed(receipt))
    }

    async fn terminalize_uncertain(
        &mut self,
        request: ManagedFabricApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        _generation: ManagedServiceGeneration,
        selection: TerminalSelection,
    ) -> Result<ManagedFabricApplyOutcome, ManagedFabricRuntimeError> {
        let mut transition = self.snapshot.transition();
        transition.phase = ManagedFabricDurablePhase::Uncertain;
        let receipt = self
            .build_terminal(&request, response_channel, selection)
            .await?;
        insert_terminal(&mut transition.terminals, &request, receipt.clone())?;
        self.commit_transition(transition)?;
        Ok(ManagedFabricApplyOutcome::Committed(receipt))
    }

    async fn start_live(
        &mut self,
        request: &ManagedFabricApplyRequestV1,
        generation: ManagedServiceGeneration,
    ) -> Result<bool, ManagedFabricRuntimeError> {
        self.start_live_execution(request.target_execution(), generation)
            .await
    }

    pub(crate) async fn start_live_execution(
        &mut self,
        execution: &ManagedFabricTargetExecutionV1,
        generation: ManagedServiceGeneration,
    ) -> Result<bool, ManagedFabricRuntimeError> {
        let spec = execution
            .service()
            .ok_or(ManagedFabricRuntimeError::MissingServiceSpec)?;
        let (implementation, control) =
            RuntimeManagedFabricService::try_from_execution(execution, generation)?;
        let mut assembly = ManagedServiceAssembly::new(
            spec,
            generation,
            Box::new(implementation),
            self.clock,
            &self.cancellation,
        );
        let ready = assembly.startup().await == ManagedServiceStartupOutcome::Ready;
        if ready {
            self.fabric_control = Some(control);
            self.assembly = Some(assembly);
            self.cleanup_exact_zero = false;
        } else {
            let exact_zero = assembly.shutdown().await.exact_zero();
            self.cleanup_exact_zero = exact_zero;
            if !exact_zero {
                self.fabric_control = Some(control);
                self.assembly = Some(assembly);
            }
        }
        Ok(ready)
    }

    fn control_handle(&self) -> Result<ManagedFabricControlHandle, ManagedFabricControlError> {
        self.fabric_control
            .clone()
            .ok_or(ManagedFabricControlError::NotReady)
    }

    async fn stop_live(&mut self) -> Result<bool, ManagedFabricRuntimeError> {
        let control = self.fabric_control.take();
        let Some(mut assembly) = self.assembly.take() else {
            return Ok(self.cleanup_exact_zero);
        };
        let exact_zero = assembly.shutdown().await.exact_zero();
        self.cleanup_exact_zero = exact_zero;
        if !exact_zero {
            self.fabric_control = control;
            self.assembly = Some(assembly);
        }
        Ok(exact_zero)
    }

    pub(crate) async fn stop_live_for_stack(&mut self) -> Result<bool, ManagedFabricRuntimeError> {
        self.stop_live().await
    }

    async fn resource_census_digest(&self) -> Result<Digest32, ManagedFabricRuntimeError> {
        let mut session_live = false;
        let mut generation = 0_u64;
        let mut owned_binding_count = 0_u32;
        let mut binding_census_known = true;
        if let Some(control) = &self.fabric_control
            && let Some(shared) = control.shared.upgrade()
        {
            let slot = shared.read().await;
            if slot.generation == control.generation
                && matches!(slot.state, ManagedFabricSlotState::Live(_))
            {
                session_live = true;
                generation = slot.generation.value();
                owned_binding_count = slot.owned_binding_count;
                binding_census_known = slot.binding_census_known;
            }
        }
        let mut builder = Digest32Builder::try_new(RESOURCE_CENSUS_DIGEST_DOMAIN)?;
        builder.field_u16(if session_live { 1 } else { 0 })?;
        builder.field_u64(generation)?;
        builder.field_bytes(&owned_binding_count.to_be_bytes())?;
        builder.field_u16(if binding_census_known { 1 } else { 0 })?;
        builder.field_u16(if self.cleanup_exact_zero { 1 } else { 0 })?;
        Ok(builder.finish())
    }

    async fn build_terminal(
        &self,
        request: &ManagedFabricApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        selection: TerminalSelection,
    ) -> Result<ManagedFabricApplyTerminalReceiptV1, ManagedFabricRuntimeError> {
        let reading = self.clock.reading()?;
        let completion_sequence = self
            .snapshot
            .sequence()
            .checked_add(1)
            .ok_or(ManagedFabricRuntimeError::SequenceOverflow)?;
        let terminal_state = ManagedFabricApplyTerminalStateV1::try_new(
            selection.outcome,
            selection.lifecycle_effect,
            selection.head,
            selection.generation,
        )?;
        let evidence = ManagedFabricApplyTerminalEvidenceV1::try_new(
            self.resource_census_digest().await?,
            raw_outcome_digest(selection.raw_code, selection.raw_context, request)?,
            self.runtime_host_epoch,
            completion_sequence,
            reading.generation(),
            reading.now().value(),
        )?;
        let facts = ManagedFabricApplyTerminalFactsV1::try_new(request, terminal_state, evidence)?;
        let algorithm = ApplyAuthAlgorithm::try_new(1)
            .map_err(|_| ManagedFabricRuntimeError::SignerConfiguration)?;
        let auth_claim = ManagedFabricApplyTerminalReceiptAuthClaimV1::try_new(
            response_channel,
            self.response_key_ref,
            algorithm,
            1,
        )?;
        let draft = ManagedFabricApplyTerminalReceiptDraftV1::try_new(
            request,
            facts,
            response_channel,
            auth_claim,
        )?;
        let signature = self
            .response_signer
            .sign(draft.signing_transcript()?.as_bytes());
        Ok(draft.finalize(&signature.to_bytes())?)
    }

    fn commit_transition(
        &mut self,
        transition: ManagedFabricSnapshotTransition,
    ) -> Result<(), ManagedFabricRuntimeError> {
        let next = self.snapshot.try_successor_at_epoch(
            self.runtime_host_epoch,
            transition,
            &self.projection,
        )?;
        self.store.commit(next.canonical_wire())?;
        self.snapshot = next;
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), ManagedFabricRuntimeError> {
        self.recovery_completed = false;
        if self.assembly.is_some() && !self.stop_live().await? {
            return Err(ManagedFabricRuntimeError::ShutdownUncertain);
        }
        Ok(())
    }
}

impl ManagedServiceImplementation for RuntimeManagedFabricService {
    fn prepare<'a>(
        &'a mut self,
        context: &'a ManagedServiceContext,
        attempt: ManagedServiceAttempt,
    ) -> ManagedServiceFuture<'a, ManagedServiceCompletion<()>> {
        Box::pin(async move {
            if context.cancellation().is_cancelled() {
                return ManagedServiceCompletion::failed(attempt);
            }
            let config = match self.requested.take() {
                Some(RuntimeManagedFabricPrepareRequest::LoopbackEndpoint(endpoint)) => {
                    let endpoint = match SessionEndpoint::try_new(endpoint.as_str().to_owned()) {
                        Ok(mapped) if mapped.as_str() == endpoint.as_str() => mapped,
                        _ => return ManagedServiceCompletion::failed(attempt),
                    };
                    match FabricServiceConfig::try_peer(vec![endpoint], Vec::new()) {
                        Ok(config) => config,
                        Err(_) => return ManagedServiceCompletion::failed(attempt),
                    }
                }
                Some(RuntimeManagedFabricPrepareRequest::ExactConfig(config)) => config,
                None => return ManagedServiceCompletion::failed(attempt),
            };
            self.prepared = Some(config);
            ManagedServiceCompletion::succeeded(attempt, ())
        })
    }

    fn start<'a>(
        &'a mut self,
        context: &'a ManagedServiceContext,
        attempt: ManagedServiceAttempt,
    ) -> ManagedServiceFuture<'a, ManagedServiceCompletion<()>> {
        Box::pin(async move {
            if context.cancellation().is_cancelled() {
                return ManagedServiceCompletion::failed(attempt);
            }
            let Some(config) = self.prepared.take() else {
                return ManagedServiceCompletion::failed(attempt);
            };
            match FabricService::start(config).await {
                Ok(service) => {
                    let mut slot = self.shared.write().await;
                    if !matches!(slot.state, ManagedFabricSlotState::NotStarted) {
                        drop(slot);
                        let _ = service.shutdown().await;
                        return ManagedServiceCompletion::failed(attempt);
                    }
                    slot.state = ManagedFabricSlotState::Live(service);
                    ManagedServiceCompletion::succeeded(attempt, ())
                }
                Err(_) => ManagedServiceCompletion::failed(attempt),
            }
        })
    }

    fn readiness<'a>(
        &'a mut self,
        context: &'a ManagedServiceContext,
        attempt: ManagedServiceAttempt,
    ) -> ManagedServiceFuture<'a, ManagedServiceCompletion<ManagedServiceReadiness>> {
        Box::pin(async move {
            if context.cancellation().is_cancelled() {
                return ManagedServiceCompletion::failed(attempt);
            }
            // Current evidence is deliberately local: successful
            // `FabricService::start` proves only that the owned session opened.
            // It does not assert any remote peer, route, or Agent port is ready.
            let live = {
                let slot = self.shared.read().await;
                matches!(slot.state, ManagedFabricSlotState::Live(_))
            };
            let readiness = if live {
                ManagedServiceReadiness::Ready
            } else {
                ManagedServiceReadiness::NotReady
            };
            ManagedServiceCompletion::succeeded(attempt, readiness)
        })
    }

    fn drain<'a>(
        &'a mut self,
        _context: &'a ManagedServiceContext,
        attempt: ManagedServiceAttempt,
        _deadline: MonotonicDeadline,
    ) -> ManagedServiceFuture<'a, ManagedServiceCompletion<()>> {
        // This tranche installs no PortBinding during Fabric lifecycle startup,
        // so there is no separately admitted request stream to drain.
        Box::pin(async move { ManagedServiceCompletion::succeeded(attempt, ()) })
    }

    fn stop<'a>(
        &'a mut self,
        _context: &'a ManagedServiceContext,
        attempt: ManagedServiceAttempt,
    ) -> ManagedServiceFuture<'a, ManagedServiceCompletion<()>> {
        Box::pin(async move {
            self.requested = None;
            self.prepared = None;
            let service = {
                let mut slot = self.shared.write().await;
                match core::mem::replace(&mut slot.state, ManagedFabricSlotState::Stopping) {
                    ManagedFabricSlotState::Live(service) => Some(service),
                    ManagedFabricSlotState::NotStarted | ManagedFabricSlotState::Stopped => None,
                    ManagedFabricSlotState::Stopping => {
                        return ManagedServiceCompletion::failed(attempt);
                    }
                }
            };
            let outcome = match service {
                Some(service) => service.shutdown().await,
                None => Ok(()),
            };
            let mut slot = self.shared.write().await;
            slot.state = ManagedFabricSlotState::Stopped;
            slot.owned_binding_count = 0;
            slot.binding_census_known = true;
            match outcome {
                Ok(()) => ManagedServiceCompletion::succeeded(attempt, ()),
                Err(_) => ManagedServiceCompletion::failed(attempt),
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecoveryProbeFailure {
    port: u16,
    raw_os_error: i32,
}

impl RecoveryProbeFailure {
    fn reason_digest(self) -> Result<Digest32, DigestBuildError> {
        recovery_reason_digest(40, self.port, self.raw_os_error)
    }
}

fn request_port(request: &ManagedFabricApplyRequestV1) -> Result<u16, ManagedFabricRuntimeError> {
    if request.target_execution().mode() != ManagedFabricTargetModeV1::OneManagedFabricService {
        return Err(ManagedFabricRuntimeError::ExpectedActiveExecution);
    }
    request
        .target_execution()
        .listen_endpoint()
        .map(ManagedFabricListenEndpointV1::port)
        .ok_or(ManagedFabricRuntimeError::MissingListenEndpoint)
}

fn recovery_reason_digest(
    code: u16,
    port: u16,
    raw_os_error: i32,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(RECOVERY_QUARANTINE_DIGEST_DOMAIN)?;
    builder.field_u16(code)?;
    builder.field_u16(port)?;
    builder.field_bytes(&raw_os_error.to_be_bytes())?;
    Ok(builder.finish())
}

fn next_generation(high_water: u64) -> Result<ManagedServiceGeneration, ManagedFabricRuntimeError> {
    high_water
        .checked_add(1)
        .ok_or(ManagedFabricRuntimeError::GenerationExhausted)
        .and_then(|value| {
            ManagedServiceGeneration::try_new(value)
                .map_err(|_| ManagedFabricRuntimeError::GenerationExhausted)
        })
}

fn insert_replay(
    records: &mut Vec<ManagedFabricReplayRecord>,
    incoming: ManagedFabricReplayRecord,
) -> Result<(), ManagedFabricRuntimeError> {
    match records.binary_search_by_key(&incoming.identity, |record| record.identity) {
        Ok(index) if records[index].value_digest == incoming.value_digest => Ok(()),
        Ok(_) => Err(ManagedFabricRuntimeError::ReplayConflict),
        Err(index) if records.len() < MAX_SUCCESSOR_REPLAY_RECORDS => {
            records.insert(index, incoming);
            Ok(())
        }
        Err(_) => Err(ManagedFabricRuntimeError::ReplayCapacityReached),
    }
}

fn load_remote_agent_descriptor_evidence(
    store: &ManagedFabricStore,
    expected_target: RuntimeHostId,
    expected_store_instance_id: [u8; 32],
) -> Result<Option<RemoteAgentDescriptorEvidenceV1>, ManagedFabricRuntimeError> {
    let Some(frame) = store.remote_agent_descriptor_evidence_bytes()? else {
        return Ok(None);
    };
    let evidence = RemoteAgentDescriptorEvidenceV1::decode(frame)?;
    if evidence.target() != expected_target
        || evidence.runtime_store_instance_id() != expected_store_instance_id
    {
        return Err(ManagedFabricRuntimeError::DescriptorEvidenceConflict);
    }
    Ok(Some(evidence))
}

fn insert_terminal(
    records: &mut Vec<ManagedFabricTerminalRecord>,
    request: &ManagedFabricApplyRequestV1,
    receipt: ManagedFabricApplyTerminalReceiptV1,
) -> Result<(), ManagedFabricRuntimeError> {
    let key = (
        *request.provenance().source_scope().as_bytes(),
        *request.operation_id().as_bytes(),
    );
    let position = records.binary_search_by_key(&key, |record| {
        (
            *record.source_scope.as_bytes(),
            *record.operation_id.as_bytes(),
        )
    });
    match position {
        Ok(index) if records[index].request_digest == request.envelope_request_digest() => Ok(()),
        Ok(_) => Err(ManagedFabricRuntimeError::OperationConflict),
        Err(index) if records.len() < MAX_SUCCESSOR_REPLAY_RECORDS => {
            records.insert(
                index,
                ManagedFabricTerminalRecord {
                    source_scope: request.provenance().source_scope(),
                    operation_id: request.operation_id(),
                    request_digest: request.envelope_request_digest(),
                    receipt,
                },
            );
            Ok(())
        }
        Err(_) => Err(ManagedFabricRuntimeError::ReplayCapacityReached),
    }
}

fn preserved_head(active: Option<&ManagedFabricDurableActive>) -> ManagedFabricApplyTerminalHeadV1 {
    active.map_or(ManagedFabricApplyTerminalHeadV1::PreservedNone, |active| {
        ManagedFabricApplyTerminalHeadV1::PreservedExisting(active.request.target_slice_digest())
    })
}

pub(crate) fn transition_projection_digest(
    projection: &ManagedFabricManifestProjectionV1,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(TRANSITION_PROJECTION_DIGEST_DOMAIN)?;
    builder.field_bytes(projection.canonical_wire())?;
    Ok(builder.finish())
}

fn raw_outcome_digest(
    raw_code: u16,
    raw_context: Option<Digest32>,
    request: &ManagedFabricApplyRequestV1,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(RAW_OUTCOME_DIGEST_DOMAIN)?;
    builder.field_u16(raw_code)?;
    builder.field_u16(if raw_context.is_some() { 1 } else { 0 })?;
    if let Some(raw_context) = raw_context {
        builder.field_digest(&raw_context)?;
    }
    builder.field_digest(&request.envelope_request_digest())?;
    Ok(builder.finish())
}

#[derive(Debug)]
pub(crate) enum ManagedFabricRuntimeError {
    ExpectedActiveExecution,
    MissingListenEndpoint,
    MissingServiceSpec,
    RuntimeEpochRegressed,
    RequestRejected,
    ProjectionMismatch,
    TerminalCorrelation,
    OperationConflict,
    RecoveryRequired,
    RecoveryNotCompleted,
    RecoveryWhileLive,
    RecoveryQuarantined,
    DeadlineExpired,
    DeadlineOverflow,
    StaleWriter,
    StaleRevision,
    ExpectedActiveMismatch,
    ReplayConflict,
    ReplayCapacityReached,
    RemoteAgentAccessSameEpochFrozen,
    RemoteAgentAccessReconcileRequired,
    GenerationExhausted,
    InvalidDurableState,
    SequenceOverflow,
    DescriptorEvidenceConflict,
    SignerConfiguration,
    ShutdownUncertain,
    Digest(DigestBuildError),
    Contract(paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricPlanError),
    State(ManagedFabricStateError),
    Store(ManagedFabricStoreError),
    DescriptorEvidence(RemoteAgentDescriptorEvidenceError),
    Clock(crate::runtime_clock::RuntimeClockError),
}

impl ManagedFabricRuntimeError {
    pub(crate) const fn is_request_unavailable(&self) -> bool {
        matches!(self, Self::RemoteAgentAccessSameEpochFrozen)
    }

    pub(crate) const fn is_request_rejection(&self) -> bool {
        matches!(
            self,
            Self::ExpectedActiveExecution
                | Self::MissingListenEndpoint
                | Self::MissingServiceSpec
                | Self::RequestRejected
                | Self::ProjectionMismatch
                | Self::TerminalCorrelation
                | Self::OperationConflict
                | Self::DeadlineExpired
                | Self::StaleWriter
                | Self::StaleRevision
                | Self::ExpectedActiveMismatch
                | Self::ReplayConflict
        )
    }
}

impl fmt::Display for ManagedFabricRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedActiveExecution => {
                formatter.write_str("managed Fabric lifecycle requires an active execution")
            }
            Self::MissingListenEndpoint => {
                formatter.write_str("active managed Fabric execution has no listen endpoint")
            }
            Self::MissingServiceSpec => {
                formatter.write_str("active managed Fabric execution has no service spec")
            }
            Self::RuntimeEpochRegressed => formatter.write_str("RuntimeHost epoch regressed"),
            Self::RequestRejected => formatter.write_str("managed Fabric request rejected"),
            Self::ProjectionMismatch => {
                formatter.write_str("managed Fabric installation projection mismatch")
            }
            Self::TerminalCorrelation => {
                formatter.write_str("managed Fabric terminal correlation mismatch")
            }
            Self::OperationConflict => {
                formatter.write_str("managed Fabric operation identity conflict")
            }
            Self::RecoveryRequired => formatter.write_str("managed Fabric recovery is required"),
            Self::RecoveryNotCompleted => {
                formatter.write_str("managed Fabric startup recovery has not completed")
            }
            Self::RecoveryWhileLive => {
                formatter.write_str("managed Fabric recovery attempted while a generation is live")
            }
            Self::RecoveryQuarantined => {
                formatter.write_str("managed Fabric recovery remains quarantined")
            }
            Self::DeadlineExpired => formatter.write_str("managed Fabric request expired"),
            Self::DeadlineOverflow => formatter.write_str("managed Fabric deadline overflow"),
            Self::StaleWriter => formatter.write_str("managed Fabric writer tenure is stale"),
            Self::StaleRevision => formatter.write_str("managed Fabric plan revision is stale"),
            Self::ExpectedActiveMismatch => {
                formatter.write_str("managed Fabric expected-active CAS mismatch")
            }
            Self::ReplayConflict => formatter.write_str("managed Fabric replay conflict"),
            Self::ReplayCapacityReached => {
                formatter.write_str("managed Fabric replay capacity reached")
            }
            Self::RemoteAgentAccessSameEpochFrozen => {
                formatter.write_str("remote Agent access is frozen by same-epoch authority")
            }
            Self::RemoteAgentAccessReconcileRequired => {
                formatter.write_str("remote Agent access startup requires reconciliation")
            }
            Self::GenerationExhausted => formatter.write_str("managed Fabric generation exhausted"),
            Self::InvalidDurableState => formatter.write_str("invalid managed Fabric state"),
            Self::SequenceOverflow => formatter.write_str("managed Fabric sequence overflow"),
            Self::DescriptorEvidenceConflict => {
                formatter.write_str("remote Agent descriptor-evidence authority conflict")
            }
            Self::SignerConfiguration => {
                formatter.write_str("managed Fabric response signer is invalid")
            }
            Self::ShutdownUncertain => formatter.write_str("managed Fabric shutdown is uncertain"),
            Self::Digest(error) => write!(formatter, "managed Fabric digest failed: {error}"),
            Self::Contract(error) => write!(formatter, "managed Fabric contract failed: {error}"),
            Self::State(error) => write!(formatter, "managed Fabric state failed: {error}"),
            Self::Store(error) => write!(formatter, "managed Fabric store failed: {error}"),
            Self::DescriptorEvidence(error) => write!(
                formatter,
                "remote Agent descriptor evidence failed: {error}"
            ),
            Self::Clock(error) => write!(formatter, "managed Fabric clock failed: {error}"),
        }
    }
}

impl std::error::Error for ManagedFabricRuntimeError {}

impl From<DigestBuildError> for ManagedFabricRuntimeError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}

impl From<paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricPlanError>
    for ManagedFabricRuntimeError
{
    fn from(
        value: paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricPlanError,
    ) -> Self {
        Self::Contract(value)
    }
}

impl From<ManagedFabricStateError> for ManagedFabricRuntimeError {
    fn from(value: ManagedFabricStateError) -> Self {
        Self::State(value)
    }
}

impl From<ManagedFabricStoreError> for ManagedFabricRuntimeError {
    fn from(value: ManagedFabricStoreError) -> Self {
        Self::Store(value)
    }
}

impl From<RemoteAgentDescriptorEvidenceError> for ManagedFabricRuntimeError {
    fn from(value: RemoteAgentDescriptorEvidenceError) -> Self {
        Self::DescriptorEvidence(value)
    }
}

impl From<crate::runtime_clock::RuntimeClockError> for ManagedFabricRuntimeError {
    fn from(value: crate::runtime_clock::RuntimeClockError) -> Self {
        Self::Clock(value)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::sync::Arc;
    use std::time::Duration;

    use ed25519_dalek::SigningKey;
    use paraegox_agent_contracts::control::{
        AgentConversationCancelStateV1, AgentConversationGetStateV1, AgentConversationOpenOutcomeV1,
    };
    use paraegox_agent_contracts::{
        AgentConversationDeckRunId, AgentConversationRequestId, AgentConversationRequestV1,
        AgentConversationSessionId, AgentConversationTerminalResultV1, AgentConversationTurnId,
    };
    use paraegox_agent_service::DeterministicEchoModelProvider;
    use paraegox_kernel::digest::Digest32;
    use paraegox_kernel::identity::PrincipalRef;
    use paraegox_kernel::time::{BoundedDuration, ClockGeneration, ClockReading};
    use paraegox_runtime_contracts::apply::{ExpectedActive, RuntimeApplyControl};
    use paraegox_runtime_contracts::assignment::BindingId;
    use paraegox_runtime_contracts::distributed_agent_stack_plan::DistributedFabricSessionEpochV1;
    use paraegox_runtime_contracts::managed_agent_stack_plan::{
        ManagedAgentIngressLimitsV1, ManagedAgentPortPlanV1, ManagedAgentProviderProfileV1,
        ManagedAgentProviderRefV1, ManagedAgentProviderSelectionV1, ManagedAgentSemanticLimitsV1,
        ManagedAgentServicePlanV1, ManagedAgentStackProjectionV1,
        ManagedAgentStackTargetExecutionV1,
    };
    use paraegox_runtime_contracts::managed_fabric_plan::{
        ManagedFabricApplyRequestDraftV1, ManagedFabricApplyRequestV1,
        ManagedFabricApplyTerminalOutcomeV1, ManagedFabricApplyTerminalReceiptV1,
        ManagedFabricListenEndpointV1, ManagedFabricManifestProjectionV1,
        ManagedFabricTargetExecutionV1,
    };
    use paraegox_runtime_contracts::managed_service::{
        ManagedServiceGeneration, ManagedServiceId, ManagedServiceLifecycleBudgetsV1,
        ManagedServiceSpecV1,
    };
    use paraegox_runtime_contracts::reference_control::ReferenceChannelBindingV1;
    use paraegox_runtime_contracts::remote_agent_data_plane_plan::{
        RemoteAgentActiveS1CasV2, RemoteAgentRetainedS0CasFieldsV2, RemoteAgentRetainedS0CasV2,
    };
    use paraegox_runtime_contracts::wire::ApplyAuthKeyRef;
    use sha2::{Digest as _, Sha256};
    use tokio::sync::{Barrier, RwLock};
    use tokio::time::Instant;

    use super::{
        ManagedFabricApplyOutcome, ManagedFabricControlError, ManagedFabricControlHandle,
        ManagedFabricDurablePending, ManagedFabricDurablePhase,
        ManagedFabricExperimentalSnapshotError, ManagedFabricOwnerConfig, ManagedFabricPendingKind,
        ManagedFabricRuntimeCore, ManagedFabricRuntimeError, ManagedFabricSlot,
        ManagedFabricSlotState, next_generation, transition_projection_digest,
    };
    use crate::admission::VerifiedManagedFabricApplyIngressV1;
    use crate::managed_agent_runtime::{
        ManagedAgentAssembly, ManagedAgentAssemblyError, RuntimeAgentConversationError,
    };
    use crate::managed_agent_transport::AgentConversationPortDescriptorV1;
    use crate::remote_agent_access_state::{
        RemoteAgentAccessDurablePhaseV2, RemoteAgentAccessSnapshotIdentityPinsV2,
        RemoteAgentAccessSnapshotV2, RemoteAgentAccessStaticIdentityPinsV2,
        remote_agent_access_prepared_fixture_v2,
    };
    use crate::runtime_agent_provider::{
        RuntimeAgentProviderResolveError, RuntimeAgentProviderResolverV1,
        RuntimeResolvedAgentProviderV1,
    };
    use crate::runtime_clock::RuntimeClock;
    use crate::runtime_store::{
        ManagedFabricStore, ManagedFabricStoreError, RemoteAgentAccessCommitErrorV2,
        RemoteAgentAccessCommitFailpointV2, tests::managed_fabric_store_fixture,
    };

    const FIXTURE_JSON: &str =
        include_str!("../../../tests/fixtures/wire/s7_managed_fabric_successor_v1.json");
    const STORE_BYTE: u8 = 0x44;
    const TARGET_FINGERPRINT_BYTE: u8 = 0x55;

    #[test]
    fn post_marker_pxrs2_compile_boundary_allows_only_absent() {
        let source = include_str!("managed_fabric_runtime.rs");
        let start = source
            .find("    pub(crate) fn adjudicate_remote_agent_access_after_first_stack_marker_v2(")
            .expect("missing post-marker PXRS v2 gate");
        let tail = &source[start..];
        let end = tail
            .find("    /// Enforces that any retained Agent-stack authority")
            .expect("missing post-marker PXRS v2 gate boundary");
        let gate = &tail[..end];

        assert!(gate.contains("startup @ RemoteAgentAccessStartupSlotV2::Absent(_)"));
        assert!(gate.contains("startup @ RemoteAgentAccessStartupSlotV2::SameEpoch(_)"));
        assert!(
            gate.contains("startup @ RemoteAgentAccessStartupSlotV2::RestartReconcileRequired(_)")
        );
        assert_eq!(gate.match_indices("Ok(())").count(), 1);
        assert_eq!(
            gate.match_indices("self.remote_agent_access_startup_v2 = Some(startup)")
                .count(),
            3
        );
        assert!(gate.contains("RemoteAgentAccessSameEpochFrozen"));
        assert!(gate.contains("RemoteAgentAccessReconcileRequired"));
        assert!(
            gate.find("self.latch_remote_agent_access_s0_mutation_freeze_v2();")
                .expect("same-epoch branch must latch the core")
                < gate
                    .find("Err(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)")
                    .expect("same-epoch typed error disappeared")
        );
        assert!(!gate.contains("start_agent"));
        assert!(!gate.contains("prepare_agent_provider"));
    }

    #[test]
    fn post_readback_genesis_bundle_source_retains_pin_and_lease_without_escape() {
        let source = include_str!("managed_fabric_runtime.rs");
        let start = source
            .find("pub(crate) struct RemoteAgentAccessPostReadbackVerifiedGenesisBundleV2<")
            .expect("missing post-readback genesis bundle");
        let tail = &source[start..];
        let end = tail
            .find("\n#[derive(Clone, Copy)]\nstruct TerminalSelection")
            .expect("missing post-readback genesis bundle boundary");
        let bundle = &tail[..end];

        assert!(!bundle.contains("#[derive"));
        assert!(!bundle.contains("impl Clone"));
        assert!(!bundle.contains("impl Copy"));
        assert!(!bundle.contains("fn try_new"));
        assert!(!bundle.contains("fn observe"));
        assert!(!bundle.contains("fn into_"));
        assert!(!bundle.contains("fn same_epoch_lease"));
        assert!(bundle.contains("readback: RemoteAgentAccessInitializedAbsentReadbackV2"));
        assert!(bundle.contains("precommit_live_lower: RemoteAgentLiveLowerProjectionV2"));
        assert!(bundle.contains("post_readback_live_lower: RemoteAgentLiveLowerFactsV2"));
        assert!(!include_str!("lib.rs").contains("remote_agent_s1_owner"));

        let verify_start = source
            .find("    pub(crate) fn try_verify_post_readback_v2(")
            .expect("missing post-readback exact verifier");
        let verify_tail = &source[verify_start..];
        let verify_end = verify_tail
            .find("\n}\n\n/// Move-only post-readback genesis authority")
            .expect("missing post-readback exact verifier boundary");
        let verify = &verify_tail[..verify_end];
        assert!(verify.contains("Box<Self>"));
        assert!(verify.contains("return Err(Box::new(self));"));
        assert!(verify.contains("readback: self.readback"));
        assert!(verify.contains("precommit_live_lower: self.precommit_live_lower"));
        assert!(!verify.contains("Err(self)"));

        let finish_start = source
            .find("    fn finish_remote_agent_access_initialization_v2<Candidate>(")
            .expect("missing initialized-absent success binder");
        let finish_tail = &source[finish_start..];
        let finish_end = finish_tail
            .find("\n    #[cfg(test)]\n    fn initialize_remote_agent_access_and_latch_at_failpoint_v2(")
            .expect("missing initialized-absent success binder boundary");
        let finish = &finish_tail[..finish_end];
        let latch = finish
            .find("self.latch_remote_agent_access_s0_mutation_freeze_v2();")
            .expect("exact success must latch before returning authority");
        let core_redecode = finish
            .find("verify_remote_agent_access_initialized_absent_candidate_v2(")
            .expect("exact success must rebind named-final bytes to current core pins");
        let bundle_mint = finish
            .find("Ok(RemoteAgentAccessInitializedAbsentReadbackV2 {")
            .expect("exact success must retain the one-shot named-final readback");
        assert!(latch < core_redecode && core_redecode < bundle_mint);
        assert!(!finish.contains("return Ok(same_epoch)"));
    }

    #[test]
    fn sealed_genesis_prevalidation_precedes_lease_take_store_write_and_latch() {
        let source = include_str!("managed_fabric_runtime.rs");
        let initialize_start = source
            .find("    pub(crate) fn initialize_remote_agent_access_from_live_lower_v2<")
            .expect("missing sealed PXRS2 initializer");
        let initialize_tail = &source[initialize_start..];
        let initialize_end = initialize_tail
            .find("\n    /// Raw structural fixture seam")
            .expect("missing sealed PXRS2 initializer boundary");
        let initialize = &initialize_tail[..initialize_end];
        assert!(
            initialize.contains(
                "RemoteAgentAccessGenesisCandidateV2::try_from_live_lower_v2(&live_lower)"
            )
        );
        assert!(
            initialize
                .contains("commit_remote_agent_access_initialized_absent_candidate_v2(candidate)")
        );
        assert!(!initialize.contains("RemoteAgentAccessSnapshotIdentityPinsV2 {"));
        assert!(!initialize.contains("RemoteAgentActiveS1CasV2::try_expect_absent"));

        let commit_start = source
            .find("    fn commit_remote_agent_access_initialized_absent_candidate_v2(")
            .expect("missing private exact PXRS2 writer");
        let commit_tail = &source[commit_start..];
        let commit_end = commit_tail
            .find("\n    /// Strictly proves the candidate")
            .expect("missing private exact PXRS2 writer boundary");
        let commit = &commit_tail[..commit_end];
        let prevalidation = initialize
            .find("commit_remote_agent_access_initialized_absent_candidate_v2(candidate)")
            .expect("sealed wrapper lost private writer call");
        assert!(prevalidation > 0);
        let prevalidation = commit
            .find("verify_remote_agent_access_initialized_absent_candidate_v2(")
            .expect("missing PXRS2 prevalidation");
        let lease_take = commit
            .find("take_remote_agent_access_absent_lease_v2(candidate)")
            .expect("missing PXRS2 Absent lease take");
        let store_write = commit
            .find(".initialize_remote_agent_access_genesis_v2(absent, candidate)")
            .expect("missing PXRS2 store initialization");
        let finish = commit
            .find("finish_remote_agent_access_initialization_v2(result)")
            .expect("missing PXRS2 post-readback binder");
        assert!(prevalidation < lease_take && lease_take < store_write && store_write < finish);
        assert!(!commit[..lease_take].contains("self.store"));
        assert!(!commit[..lease_take].contains("latch_remote_agent_access_s0_mutation"));

        let raw_start = source
            .find("    /// Raw structural fixture seam")
            .expect("missing raw fixture seam marker");
        let raw = &source[raw_start..commit_start];
        assert!(raw.contains("#[cfg(test)]"));
        assert!(!raw.contains("pub(crate) fn initialize_remote_agent_access_and_latch_v2"));

        let state_source = include_str!("remote_agent_access_state.rs");
        assert!(state_source.contains(
            "pub(crate) struct RemoteAgentAccessGenesisCandidateV2 {\n    snapshot: RemoteAgentAccessSnapshotV2,\n}"
        ));
        assert!(state_source.contains(
            "pub(crate) fn try_from_live_lower_v2(\n        live_lower: &RemoteAgentLiveLowerProjectionV2<'_>,"
        ));
        assert!(!state_source.contains("pub(crate) struct RemoteAgentAccessGenesisInputV2"));
        assert!(!state_source.contains("try_initialize_absent_from_genesis_v2"));
        let candidate_start = state_source
            .find("pub(crate) struct RemoteAgentAccessGenesisCandidateV2 {")
            .expect("missing opaque genesis candidate");
        let candidate_tail = &state_source[candidate_start..];
        let candidate_end = candidate_tail
            .find("\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\nstruct RemoteAgentAccessAdmissionFactsV2")
            .expect("missing opaque genesis candidate boundary");
        let candidate_source = &candidate_tail[..candidate_end];
        assert!(!candidate_source.contains("#[derive"));
        assert!(!candidate_source.contains("impl Clone"));
        assert!(!candidate_source.contains("impl Copy"));
        assert!(candidate_source.contains("live_lower: &RemoteAgentLiveLowerProjectionV2<'_>"));
        assert!(!candidate_source.contains("RemoteAgentAccessSnapshotIdentityPinsV2,"));
        assert!(!candidate_source.contains("expected_s1_cas:"));
        let raw_ctor = state_source
            .find("    pub(crate) fn try_initialize_absent(\n")
            .expect("missing raw state fixture constructor");
        assert!(state_source[..raw_ctor].ends_with("    #[cfg(test)]\n"));

        let store_source = include_str!("runtime_store.rs");
        assert!(store_source.contains(
            "pub(crate) type RemoteAgentAccessGenesisInitializeCommitErrorV2 =\n    RemoteAgentAccessCommitErrorV2<RemoteAgentAccessGenesisCandidateV2>;"
        ));
        let sealed_store = store_source
            .find("    pub(crate) fn initialize_remote_agent_access_genesis_v2(")
            .expect("missing opaque-candidate store initializer");
        let raw_store = store_source
            .find("    pub(crate) fn initialize_remote_agent_access_v2(\n")
            .expect("missing raw store fixture initializer");
        assert!(sealed_store < raw_store);
        assert!(store_source[..raw_store].ends_with("    #[cfg(test)]\n"));
        assert!(
            store_source[sealed_store..raw_store]
                .contains("candidate: RemoteAgentAccessGenesisCandidateV2")
        );
        assert!(
            !store_source[sealed_store..raw_store]
                .contains("snapshot: RemoteAgentAccessSnapshotV2")
        );

        let validator_start = source
            .find("    fn verify_remote_agent_access_initialized_absent_candidate_v2(")
            .expect("missing PXRS2 initial-only validator");
        let validator_tail = &source[validator_start..];
        let validator_end = validator_tail
            .find("\n    fn take_remote_agent_access_absent_lease_v2<Candidate>(")
            .expect("missing PXRS2 validator boundary");
        let validator = &validator_tail[..validator_end];
        for required in [
            "transition_projection_digest(&self.projection)",
            "target: self.projection.target()",
            "store_instance_id: self.store_instance_id()",
            "owner_target_fingerprint: self.owner_target_fingerprint()",
            "RemoteAgentAccessSnapshotV2::decode(canonical_wire, current_static_identity)",
            "&current_snapshot != candidate",
            "RemoteAgentAccessDurablePhaseV2::InitializedAbsent",
            "current_snapshot.sequence() != 1",
            "current_snapshot.previous_snapshot_digest().is_some()",
            "current_snapshot.writer_runtime_host_epoch() != self.runtime_host_epoch",
            "current_snapshot.access_generation_high_water() != 0",
            "current_snapshot.owner_slot_revision() != 1",
            "RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)",
        ] {
            assert!(
                validator.contains(required),
                "missing validator pin: {required}"
            );
        }
        assert!(!validator.contains("self.store."));
        assert!(!validator.contains("remote_agent_access_startup_v2.take()"));
        assert!(!validator.contains("latch_remote_agent_access_s0_mutation"));

        let failpoint_start = source
            .find("    fn initialize_remote_agent_access_and_latch_at_failpoint_v2(")
            .expect("missing PXRS2 failpoint initializer");
        let failpoint_tail = &source[failpoint_start..];
        let failpoint_end = failpoint_tail
            .find("\n    /// After the first PXAR-v7 marker")
            .expect("missing PXRS2 failpoint boundary");
        let failpoint = &failpoint_tail[..failpoint_end];
        assert!(
            failpoint
                .find("verify_remote_agent_access_initialized_absent_candidate_v2(")
                .expect("failpoint path bypasses initial-only prevalidation")
                < failpoint
                    .find("take_remote_agent_access_absent_lease_v2(candidate)")
                    .expect("failpoint path lost the Absent lease take")
        );
    }

    #[test]
    fn s0_freeze_and_retained_terminal_authority_are_ordered_at_owner_entrypoints() {
        let source = include_str!("managed_fabric_runtime.rs");
        let lookup = source
            .split_once("    fn lookup_terminal(")
            .and_then(|(_, tail)| tail.split_once("    /// Returns an already committed terminal"))
            .map(|(lookup, _)| lookup)
            .expect("missing managed Fabric terminal lookup boundary");
        for required in [
            ".validate_against_request(request, channel)",
            "authentication_key() != self.response_key_ref",
            "authentication_algorithm().value() != 1",
            "authentication_algorithm_version() != 1",
            "signature_bytes.len() != 64",
            "Signature::from_slice(signature_bytes)",
            ".signing_transcript()",
            ".verify_strict(transcript.as_bytes(), &signature)",
        ] {
            assert!(
                lookup.contains(required),
                "missing retained-terminal check: {required}"
            );
        }

        let apply = source
            .split_once("    pub(crate) async fn apply(")
            .and_then(|(_, tail)| tail.split_once("    /// Reconciles a successor snapshot"))
            .map(|(apply, _)| apply)
            .expect("missing managed Fabric apply boundary");
        let replay = apply
            .find("self.lookup_terminal(")
            .expect("missing exact replay lookup");
        let gate = apply
            .find("self.require_remote_agent_access_s0_mutation_unfrozen_v2()?")
            .expect("missing S0 freeze gate");
        let phase = apply
            .find("self.snapshot.phase")
            .expect("missing phase gate");
        let deadline = apply
            .find("self.observe_deadline(")
            .expect("missing deadline observation");
        let admission = apply
            .find("self.admit_transition(")
            .expect("missing transition admission");
        assert!(replay < gate && gate < phase && phase < deadline && deadline < admission);

        let descriptor_commit = source
            .split_once("    pub(crate) fn commit_remote_agent_descriptor_evidence(")
            .and_then(|(_, tail)| {
                tail.split_once("    pub(crate) fn managed_agent_stack_projection_digest(")
            })
            .map(|(commit, _)| commit)
            .expect("missing PXDE commit boundary");
        assert!(
            descriptor_commit
                .find("self.require_remote_agent_access_s0_mutation_unfrozen_v2()?")
                .expect("PXDE commit bypasses freeze")
                < descriptor_commit
                    .find("let sequence_matches")
                    .expect("missing PXDE sequence validation")
        );

        let shutdown = source
            .split_once("    pub(crate) async fn shutdown(&mut self)")
            .and_then(|(_, tail)| tail.split_once("\n}\n\nimpl ManagedServiceImplementation"))
            .map(|(shutdown, _)| shutdown)
            .expect("missing managed Fabric shutdown boundary");
        assert!(!shutdown.contains("require_remote_agent_access_s0_mutation_unfrozen_v2"));
    }

    struct DeterministicFixtureResolver;

    impl RuntimeAgentProviderResolverV1 for DeterministicFixtureResolver {
        fn resolve(
            &self,
            selection: ManagedAgentProviderSelectionV1,
        ) -> Result<RuntimeResolvedAgentProviderV1, RuntimeAgentProviderResolveError> {
            if selection.profile() != ManagedAgentProviderProfileV1::DeterministicFixture {
                return Err(RuntimeAgentProviderResolveError::ResolutionFailed);
            }
            Ok(RuntimeResolvedAgentProviderV1::new(
                selection,
                DeterministicEchoModelProvider::new(),
            ))
        }
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        fn nibble(byte: u8) -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("fixture contains non-hex byte"),
            }
        }
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1]))
            .collect()
    }

    fn fixture_request(name: &str) -> ManagedFabricApplyRequestV1 {
        let object_key = format!("\"{name}\"");
        let object_start = FIXTURE_JSON
            .find(&object_key)
            .unwrap_or_else(|| panic!("missing fixture object {name}"));
        let field = "\"outer_v6_hex\": \"";
        let field_start = FIXTURE_JSON[object_start..]
            .find(field)
            .map(|offset| object_start + offset + field.len())
            .unwrap_or_else(|| panic!("missing outer request for {name}"));
        let field_end = FIXTURE_JSON[field_start..]
            .find('"')
            .map(|offset| field_start + offset)
            .expect("fixture hex must terminate");
        ManagedFabricApplyRequestV1::decode(&decode_hex(&FIXTURE_JSON[field_start..field_end]))
            .unwrap_or_else(|error| panic!("fixture request must decode: {error}"))
    }

    fn projection() -> ManagedFabricManifestProjectionV1 {
        fixture_request("one_managed_fabric_service")
            .target_execution()
            .projection()
            .clone()
    }

    fn active_request(port: u16, expected_active: ExpectedActive) -> ManagedFabricApplyRequestV1 {
        let basis = fixture_request("one_managed_fabric_service");
        let endpoint = ManagedFabricListenEndpointV1::try_new(&format!("tcp/127.0.0.1:{port}"))
            .expect("ephemeral loopback endpoint must be canonical");
        let execution = ManagedFabricTargetExecutionV1::try_one_managed_fabric_service(
            projection(),
            basis
                .target_execution()
                .service()
                .expect("fixture service must exist"),
            endpoint,
        )
        .expect("active execution must build");
        rebuild_request(&basis, execution, expected_active)
    }

    fn empty_request(expected_active: ExpectedActive) -> ManagedFabricApplyRequestV1 {
        let basis = fixture_request("empty_deactivate");
        let execution = ManagedFabricTargetExecutionV1::try_empty_deactivate(projection())
            .expect("empty execution must build");
        rebuild_request(&basis, execution, expected_active)
    }

    fn rebuild_request(
        basis: &ManagedFabricApplyRequestV1,
        execution: ManagedFabricTargetExecutionV1,
        expected_active: ExpectedActive,
    ) -> ManagedFabricApplyRequestV1 {
        let control = RuntimeApplyControl::new(
            basis
                .control_commitment()
                .control()
                .writer_context()
                .clone(),
            expected_active,
            basis.operation_id(),
        );
        ManagedFabricApplyRequestDraftV1::try_new(
            execution,
            basis.provenance(),
            control,
            basis.temporal(),
            [STORE_BYTE; 32],
            basis.authentication().claim().clone(),
        )
        .expect("request draft must build")
        .finalize(basis.authentication().signature())
        .expect("opaque fixture signature must finalize")
    }

    fn channel(projection: &ManagedFabricManifestProjectionV1) -> ReferenceChannelBindingV1 {
        ReferenceChannelBindingV1::try_new(
            projection.target(),
            PrincipalRef::from_bytes([0xe1; 16]),
            Digest32::from_bytes([0xe3; 32]),
            Digest32::from_bytes([0xe4; 32]),
        )
        .expect("fixture response channel must build")
    }

    fn clock(generation: u64, origin_ticks: u64) -> RuntimeClock {
        let basis = fixture_request("one_managed_fabric_service");
        RuntimeClock::new(
            basis.temporal().target_clock_domain(),
            ClockGeneration::try_new(generation).expect("clock generation must be nonzero"),
            origin_ticks,
        )
    }

    fn verified(reading: ClockReading, seed: u8) -> VerifiedManagedFabricApplyIngressV1 {
        VerifiedManagedFabricApplyIngressV1::for_test(
            reading.now().value(),
            reading
                .now()
                .value()
                .checked_add(60_000_000_000)
                .expect("test deadline must fit"),
            reading.generation(),
            seed,
        )
    }

    fn config(
        directory: &std::path::Path,
        projection: ManagedFabricManifestProjectionV1,
        runtime_host_epoch: u64,
        clock: RuntimeClock,
    ) -> ManagedFabricOwnerConfig {
        ManagedFabricOwnerConfig {
            state_directory: directory.to_path_buf(),
            store_instance_id: [STORE_BYTE; 32],
            owner_target_fingerprint: Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
            projection,
            runtime_host_epoch,
            clock,
            response_key_ref: ApplyAuthKeyRef::from_bytes([0xe2; 16]),
            response_signer: SigningKey::from_bytes(&[0x71; 32]),
        }
    }

    fn available_port() -> u16 {
        TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("ephemeral loopback bind must work")
            .local_addr()
            .expect("ephemeral listener must have an address")
            .port()
    }

    fn fresh_core(
        runtime_host_epoch: u64,
        clock_generation: u64,
    ) -> (
        crate::runtime_store::tests::TestDirectory,
        ManagedFabricRuntimeCore,
    ) {
        let projection = projection();
        let projection_digest =
            transition_projection_digest(&projection).expect("projection digest must build");
        let (directory, store) =
            managed_fabric_store_fixture(STORE_BYTE, TARGET_FINGERPRINT_BYTE, projection_digest);
        let core = ManagedFabricRuntimeCore::from_preopened_store(
            store,
            config(
                directory.path(),
                projection,
                runtime_host_epoch,
                clock(clock_generation, 100),
            ),
        )
        .expect("managed-fabric core must initialize");
        (directory, core)
    }

    fn remote_agent_access_static_identity_v2(
        projection: &ManagedFabricManifestProjectionV1,
    ) -> RemoteAgentAccessStaticIdentityPinsV2 {
        RemoteAgentAccessStaticIdentityPinsV2 {
            target: projection.target(),
            store_instance_id: [STORE_BYTE; 32],
            owner_target_fingerprint: Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
            transition_projection_digest: transition_projection_digest(projection)
                .expect("PXRS2 transition projection digest must build"),
        }
    }

    fn remote_agent_access_initial_snapshot_v2(
        projection: &ManagedFabricManifestProjectionV1,
        runtime_host_epoch: u64,
    ) -> RemoteAgentAccessSnapshotV2 {
        let generation = ManagedServiceGeneration::try_new(5)
            .unwrap_or_else(|error| panic!("PXRS2 generation fixture rejected: {error}"));
        let retained_s0_cas =
            RemoteAgentRetainedS0CasV2::try_new(RemoteAgentRetainedS0CasFieldsV2 {
                expected_active_pxft_digest: Digest32::from_bytes([0x41; 32]),
                expected_active_pxst_digest: Digest32::from_bytes([0x42; 32]),
                expected_descriptor_evidence_record_digest: Digest32::from_bytes([0x43; 32]),
                expected_descriptor_evidence_record_sequence: 3,
                expected_descriptor_receipt_digest: Digest32::from_bytes([0x44; 32]),
                expected_descriptor_payload_digest: Digest32::from_bytes([0x45; 32]),
                expected_fabric_session_epoch: DistributedFabricSessionEpochV1::try_from_bytes(
                    [0x46; 16],
                )
                .unwrap_or_else(|error| panic!("PXRS2 Fabric epoch rejected: {error}")),
                expected_fabric_generation: generation,
                expected_agent_generation: generation,
            })
            .unwrap_or_else(|error| panic!("PXRS2 retained S0 fixture rejected: {error}"));
        let static_identity = remote_agent_access_static_identity_v2(projection);
        RemoteAgentAccessSnapshotV2::try_initialize_absent(
            RemoteAgentAccessSnapshotIdentityPinsV2 {
                target: static_identity.target,
                store_instance_id: static_identity.store_instance_id,
                owner_target_fingerprint: static_identity.owner_target_fingerprint,
                transition_projection_digest: static_identity.transition_projection_digest,
                lower_capability_projection_digest: Digest32::from_bytes([0x47; 32]),
            },
            runtime_host_epoch,
            retained_s0_cas,
            RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                .unwrap_or_else(|error| panic!("PXRS2 absent S1 CAS rejected: {error}")),
            31,
            32,
        )
        .unwrap_or_else(|error| panic!("initial PXRS2 fixture rejected: {error}"))
    }

    fn remote_agent_access_noninitial_sequence_one_candidate_v2(
        core: &ManagedFabricRuntimeCore,
    ) -> RemoteAgentAccessSnapshotV2 {
        const FLAGS_OFFSET: usize = 12;
        const SEQUENCE_OFFSET: usize = 42;
        const OWNER_TARGET_FINGERPRINT_OFFSET: usize = 290;
        const TRANSITION_PROJECTION_DIGEST_OFFSET: usize = 322;
        const PREVIOUS_SNAPSHOT_DIGEST_OFFSET: usize = 386;
        const DIGEST_BYTES: usize = 32;
        const SNAPSHOT_DIGEST_DOMAIN: &[u8] =
            b"paraegox.runtime.remote-agent-access-snapshot.sha256.v2";

        let (_, prepared, fixture_static_identity, fixture_runtime_host_epoch) =
            remote_agent_access_prepared_fixture_v2();
        assert_eq!(
            prepared.snapshot().phase(),
            RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
        );
        assert_eq!(prepared.snapshot().sequence(), 2);
        assert!(prepared.snapshot().previous_snapshot_digest().is_some());
        assert_eq!(fixture_static_identity.target, core.projection.target());
        assert_eq!(
            fixture_static_identity.store_instance_id,
            core.store_instance_id()
        );
        assert_eq!(fixture_runtime_host_epoch, core.runtime_host_epoch);

        let mut wire = prepared.canonical_wire().to_vec();
        assert_eq!(&wire[..4], b"PXRS");
        assert_eq!(u16::from_be_bytes([wire[4], wire[5]]), 2);
        assert_eq!(u16::from_be_bytes([wire[6], wire[7]]), 1_314);

        let flags = u16::from_be_bytes([wire[FLAGS_OFFSET], wire[FLAGS_OFFSET + 1]]);
        assert_ne!(flags & 1, 0);
        wire[FLAGS_OFFSET..FLAGS_OFFSET + 2].copy_from_slice(&(flags & !1).to_be_bytes());
        wire[SEQUENCE_OFFSET..SEQUENCE_OFFSET + 8].copy_from_slice(&1_u64.to_be_bytes());
        wire[OWNER_TARGET_FINGERPRINT_OFFSET..OWNER_TARGET_FINGERPRINT_OFFSET + DIGEST_BYTES]
            .copy_from_slice(core.owner_target_fingerprint().as_bytes());
        let transition_projection_digest = transition_projection_digest(&core.projection)
            .expect("PXRS2 transition projection digest must build");
        wire[TRANSITION_PROJECTION_DIGEST_OFFSET
            ..TRANSITION_PROJECTION_DIGEST_OFFSET + DIGEST_BYTES]
            .copy_from_slice(transition_projection_digest.as_bytes());
        wire[PREVIOUS_SNAPSHOT_DIGEST_OFFSET..PREVIOUS_SNAPSHOT_DIGEST_OFFSET + DIGEST_BYTES]
            .fill(0);

        let digest_offset = wire
            .len()
            .checked_sub(DIGEST_BYTES)
            .expect("PXRS2 fixture must contain its digest");
        let mut hasher = Sha256::new();
        hasher.update(SNAPSHOT_DIGEST_DOMAIN);
        hasher.update((digest_offset as u64).to_be_bytes());
        hasher.update(&wire[..digest_offset]);
        let digest: [u8; DIGEST_BYTES] = hasher.finalize().into();
        wire[digest_offset..].copy_from_slice(&digest);

        let candidate = RemoteAgentAccessSnapshotV2::decode(
            &wire,
            remote_agent_access_static_identity_v2(&core.projection),
        )
        .unwrap_or_else(|error| {
            panic!("canonical non-initial sequence-one PXRS2 rejected: {error}")
        });
        assert_eq!(
            candidate.phase(),
            RemoteAgentAccessDurablePhaseV2::PreparedNoEffects
        );
        assert_eq!(candidate.sequence(), 1);
        assert_eq!(candidate.previous_snapshot_digest(), None);
        assert_eq!(
            candidate.writer_runtime_host_epoch(),
            core.runtime_host_epoch
        );
        assert_eq!(candidate.access_generation_high_water(), 0);
        assert_eq!(candidate.owner_slot_revision(), 1);
        candidate
    }

    fn retain_remote_agent_access_absent_startup_v2(core: &mut ManagedFabricRuntimeCore) {
        core.store
            .initialize_managed_agent_stack(
                Digest32::from_bytes([0x48; 32]),
                b"core-freeze-agent-stack-initial",
            )
            .unwrap_or_else(|error| panic!("Agent-stack cutover fixture failed: {error}"));
        core.adjudicate_remote_agent_access_after_first_stack_marker_v2()
            .unwrap_or_else(|error| panic!("absent PXRS2 startup rejected: {error}"));
        assert!(!core.remote_agent_access_s0_mutation_frozen_v2());
    }

    #[test]
    fn noninitial_sequence_one_is_rejected_before_store_write_without_burning_absent_lease() {
        let (_, _, _, fixture_runtime_host_epoch) = remote_agent_access_prepared_fixture_v2();
        let (directory, mut core) = fresh_core(fixture_runtime_host_epoch, 3);
        retain_remote_agent_access_absent_startup_v2(&mut core);
        let candidate = remote_agent_access_noninitial_sequence_one_candidate_v2(&core);

        let rejected = core.initialize_remote_agent_access_and_latch_v2(candidate);
        assert!(matches!(
            rejected,
            Err(RemoteAgentAccessCommitErrorV2::Rejected {
                cause: ManagedFabricStoreError::RemoteAgentAccessSnapshotMismatch,
                ..
            })
        ));
        assert!(matches!(
            core.remote_agent_access_startup_v2.as_ref(),
            Some(crate::runtime_store::RemoteAgentAccessStartupSlotV2::Absent(_))
        ));
        assert!(!core.remote_agent_access_s0_mutation_frozen_v2());
        let final_path = directory.path().join("remote-agent-access.snapshot-v2");
        assert!(!final_path.exists());

        let valid =
            remote_agent_access_initial_snapshot_v2(&core.projection, fixture_runtime_host_epoch);
        let committed = core
            .initialize_remote_agent_access_and_latch_v2(valid)
            .expect("retained Absent lease must permit exact legal initialization");
        assert_eq!(
            committed.committed_snapshot().phase(),
            RemoteAgentAccessDurablePhaseV2::InitializedAbsent
        );
        assert!(final_path.exists());
        assert!(core.remote_agent_access_s0_mutation_frozen_v2());
    }

    #[test]
    fn exact_pxrs2_initialize_returns_one_shot_absent_bundle_after_latch() {
        let (_directory, mut core) = fresh_core(1, 3);
        retain_remote_agent_access_absent_startup_v2(&mut core);
        let candidate = remote_agent_access_initial_snapshot_v2(&core.projection, 1);
        let expected_wire = candidate.canonical_wire().to_vec();
        let expected_digest = candidate.snapshot_digest();
        let expected_target = core.projection.target();

        let committed = core
            .initialize_remote_agent_access_and_latch_v2(candidate)
            .expect("exact PXRS2 initialize/readback must pass");

        assert!(core.remote_agent_access_s0_mutation_frozen_v2());
        assert!(matches!(
            core.require_remote_agent_access_s0_mutation_unfrozen_v2(),
            Err(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)
        ));
        assert_eq!(committed.target(), expected_target);
        assert_eq!(committed.store_instance_id(), [STORE_BYTE; 32]);
        assert_eq!(committed.runtime_host_epoch(), 1);
        assert_eq!(
            committed.initial_absent_s1_cas(),
            RemoteAgentActiveS1CasV2::try_expect_absent(0, 1)
                .expect("fixed initial absent S1 CAS must be valid")
        );
        assert_eq!(
            committed.committed_canonical_wire(),
            expected_wire.as_slice()
        );
        let snapshot = committed.committed_snapshot();
        assert_eq!(snapshot.snapshot_digest(), expected_digest);
        assert_eq!(
            snapshot.phase(),
            RemoteAgentAccessDurablePhaseV2::InitializedAbsent
        );
        assert_eq!(snapshot.sequence(), 1);
        assert_eq!(snapshot.previous_snapshot_digest(), None);
        assert_eq!(snapshot.writer_runtime_host_epoch(), 1);
        assert_eq!(snapshot.access_generation_high_water(), 0);
        assert_eq!(snapshot.owner_slot_revision(), 1);

        let second_candidate = remote_agent_access_initial_snapshot_v2(&core.projection, 1);
        assert!(matches!(
            core.initialize_remote_agent_access_and_latch_v2(second_candidate),
            Err(RemoteAgentAccessCommitErrorV2::Rejected { .. })
        ));
        assert!(core.remote_agent_access_s0_mutation_frozen_v2());
    }

    #[test]
    fn same_epoch_pxrs2_adjudication_latches_core_before_typed_error() {
        let projection = projection();
        let projection_digest = transition_projection_digest(&projection)
            .expect("PXRS2 transition projection digest must build");
        let (directory, mut store) =
            managed_fabric_store_fixture(STORE_BYTE, TARGET_FINGERPRINT_BYTE, projection_digest);
        store
            .initialize_managed_agent_stack(
                Digest32::from_bytes([0x49; 32]),
                b"same-epoch-agent-stack-initial",
            )
            .unwrap_or_else(|error| panic!("Agent-stack cutover fixture failed: {error}"));
        let absent = match store
            .adjudicate_remote_agent_access_startup_v2(
                remote_agent_access_static_identity_v2(&projection),
                1,
            )
            .expect("missing PXRS2 final must adjudicate absent")
        {
            crate::runtime_store::RemoteAgentAccessStartupSlotV2::Absent(absent) => absent,
            crate::runtime_store::RemoteAgentAccessStartupSlotV2::SameEpoch(_)
            | crate::runtime_store::RemoteAgentAccessStartupSlotV2::RestartReconcileRequired(_) => {
                panic!("fresh PXRS2 fixture was not absent")
            }
        };
        store
            .initialize_remote_agent_access_v2(
                absent,
                remote_agent_access_initial_snapshot_v2(&projection, 1),
            )
            .expect("same-epoch PXRS2 fixture must commit");
        drop(store);

        let reopened = ManagedFabricStore::open_fixture(
            directory.path(),
            [STORE_BYTE; 32],
            Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
            projection_digest,
        )
        .expect("same-epoch PXRS2 store must reopen");
        let mut core = ManagedFabricRuntimeCore::from_preopened_store(
            reopened,
            config(directory.path(), projection, 1, clock(3, 100)),
        )
        .expect("same-epoch PXRS2 core must initialize");

        assert!(matches!(
            core.adjudicate_remote_agent_access_after_first_stack_marker_v2(),
            Err(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)
        ));
        assert!(core.remote_agent_access_s0_mutation_frozen_v2());
        assert!(matches!(
            core.require_remote_agent_access_s0_mutation_unfrozen_v2(),
            Err(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)
        ));
    }

    #[test]
    fn uncertain_pxrs2_initialize_latches_but_known_no_commit_does_not() {
        let (_directory, mut uncertain) = fresh_core(1, 3);
        retain_remote_agent_access_absent_startup_v2(&mut uncertain);
        let uncertain_candidate = remote_agent_access_initial_snapshot_v2(&uncertain.projection, 1);
        assert!(matches!(
            uncertain.initialize_remote_agent_access_and_latch_at_failpoint_v2(
                uncertain_candidate,
                RemoteAgentAccessCommitFailpointV2::AfterRenameBeforeDirectorySync,
            ),
            Err(RemoteAgentAccessCommitErrorV2::OutcomeUncertain(_))
        ));
        assert!(uncertain.remote_agent_access_s0_mutation_frozen_v2());

        let (_directory, mut proven_not_committed) = fresh_core(1, 3);
        retain_remote_agent_access_absent_startup_v2(&mut proven_not_committed);
        let retry_candidate =
            remote_agent_access_initial_snapshot_v2(&proven_not_committed.projection, 1);
        assert!(matches!(
            proven_not_committed.initialize_remote_agent_access_and_latch_at_failpoint_v2(
                retry_candidate,
                RemoteAgentAccessCommitFailpointV2::BeforeTempSync,
            ),
            Err(RemoteAgentAccessCommitErrorV2::ProvenNotCommitted { .. })
        ));
        assert!(!proven_not_committed.remote_agent_access_s0_mutation_frozen_v2());

        let (_directory, mut rejected) = fresh_core(1, 3);
        let rejected_candidate = remote_agent_access_initial_snapshot_v2(&rejected.projection, 1);
        assert!(matches!(
            rejected.initialize_remote_agent_access_and_latch_v2(rejected_candidate),
            Err(RemoteAgentAccessCommitErrorV2::Rejected { .. })
        ));
        assert!(!rejected.remote_agent_access_s0_mutation_frozen_v2());
    }

    fn agent_stack_execution(
        fabric_execution: ManagedFabricTargetExecutionV1,
    ) -> ManagedAgentStackTargetExecutionV1 {
        let lifecycle_budget = BoundedDuration::from_nanos(5_000_000_000);
        let lifecycle_budgets = ManagedServiceLifecycleBudgetsV1::try_new(
            lifecycle_budget,
            lifecycle_budget,
            lifecycle_budget,
            lifecycle_budget,
            lifecycle_budget,
        )
        .expect("Agent lifecycle budgets must be valid");
        let agent_service =
            ManagedServiceSpecV1::new(ManagedServiceId::from_bytes([0xa1; 16]), lifecycle_budgets);
        let semantic_limits = ManagedAgentSemanticLimitsV1::try_new(8, 16, 16, 32)
            .expect("signed Agent semantic limits must be valid");
        let ingress_limits = ManagedAgentIngressLimitsV1::try_new(
            8,
            512 * 1024,
            64 * 1024,
            64 * 1024,
            2_000_000_000,
        )
        .expect("signed Agent ingress limits must be valid");
        let port_plan = ManagedAgentPortPlanV1::try_new(
            BindingId::from_bytes([0xa2; 16]),
            BindingId::from_bytes([0xa3; 16]),
            "paraegox/runtime/managed-agent/test/submit",
            "paraegox/runtime/managed-agent/test/control",
            ingress_limits,
        )
        .expect("signed two-lane Agent port must be valid");
        let provider = ManagedAgentProviderSelectionV1::try_deterministic_fixture(
            ManagedAgentProviderRefV1::try_from_bytes([0xa4; 16])
                .expect("fixture provider ref must be valid"),
            Digest32::from_bytes([0xa5; 32]),
        )
        .expect("fixture provider must be explicitly selected");
        let agent_plan =
            ManagedAgentServicePlanV1::try_new(agent_service, semantic_limits, port_plan, provider)
                .expect("signed Agent service plan must be valid");
        let stack_projection = ManagedAgentStackProjectionV1::try_from_managed_fabric_projection(
            fabric_execution.projection().clone(),
        )
        .expect("stack projection must preserve the Fabric projection");
        ManagedAgentStackTargetExecutionV1::try_fabric_and_agent(
            stack_projection,
            fabric_execution,
            agent_plan,
        )
        .expect("signed Fabric-to-Agent execution must be valid")
    }

    #[tokio::test]
    async fn experimental_snapshot_handle_enforces_deadline_and_generation_before_session_access() {
        let owner_generation = ManagedServiceGeneration::try_new(1)
            .unwrap_or_else(|error| panic!("owner generation rejected: {error}"));
        let shared = Arc::new(RwLock::new(ManagedFabricSlot {
            generation: owner_generation,
            state: ManagedFabricSlotState::NotStarted,
            owned_binding_count: 0,
            binding_census_known: true,
        }));
        let matching = ManagedFabricControlHandle {
            generation: owner_generation,
            shared: Arc::downgrade(&shared),
        };
        assert_eq!(
            matching
                .observe_experimental_remote_mtls_links_once(Instant::now())
                .await
                .expect_err("expired absolute deadline must fail before Session access"),
            ManagedFabricExperimentalSnapshotError::DeadlineExpired
        );

        let stale_generation = ManagedServiceGeneration::try_new(2)
            .unwrap_or_else(|error| panic!("stale generation rejected: {error}"));
        let stale = ManagedFabricControlHandle {
            generation: stale_generation,
            shared: Arc::downgrade(&shared),
        };
        assert_eq!(
            stale
                .observe_experimental_remote_mtls_links_once(
                    Instant::now() + Duration::from_secs(1),
                )
                .await
                .expect_err("wrong generation must be fenced before Session access"),
            ManagedFabricExperimentalSnapshotError::Control(
                ManagedFabricControlError::GenerationFenced
            )
        );
        assert_eq!(
            matching
                .observe_experimental_remote_mtls_links_once(
                    Instant::now() + Duration::from_secs(1),
                )
                .await
                .expect_err("not-started generation must not expose a Session"),
            ManagedFabricExperimentalSnapshotError::Control(ManagedFabricControlError::NotReady)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn active_replay_empty_and_generation_fence_use_one_real_fabric_session() {
        let (_directory, mut core) = fresh_core(1, 3);
        let port = available_port();
        let active = active_request(port, ExpectedActive::None);
        let response_channel = channel(&core.projection);
        let ingress = verified(core.clock.reading().expect("clock must read"), 0xb0);

        assert!(matches!(
            core.apply(active.clone(), ingress, response_channel)
                .await
                .expect_err("apply must remain closed before startup recovery"),
            ManagedFabricRuntimeError::RecoveryNotCompleted
        ));
        core.recover()
            .await
            .expect("fresh exact-zero recovery must pass");
        let committed = core
            .apply(active.clone(), ingress, response_channel)
            .await
            .expect("active apply must complete");
        let ManagedFabricApplyOutcome::Committed(active_receipt) = committed else {
            panic!("first active apply must commit")
        };
        assert_eq!(
            active_receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::ActiveReady
        );
        assert_eq!(core.snapshot.phase, ManagedFabricDurablePhase::ActiveReady);
        assert_eq!(core.snapshot.generation_high_water(), 1);
        let generation_one =
            ManagedServiceGeneration::try_new(1).expect("first managed generation must be valid");
        let retained_root = core
            .export_active_retained_root_v1(
                active.target_execution().execution_digest(),
                generation_one,
            )
            .await
            .expect("current active execution must export its correlated PXFT root");
        assert_eq!(
            retained_root.active_pxft_digest,
            active_receipt.receipt_digest()
        );
        assert_eq!(retained_root.fabric_generation, generation_one);
        let mut wrong_execution_digest = *active.target_execution().execution_digest().as_bytes();
        wrong_execution_digest[0] ^= 1;
        assert!(matches!(
            core.export_active_retained_root_v1(
                Digest32::from_bytes(wrong_execution_digest),
                generation_one,
            )
            .await,
            Err(ManagedFabricRuntimeError::ExpectedActiveExecution)
        ));
        assert!(matches!(
            core.export_active_retained_root_v1(
                active.target_execution().execution_digest(),
                ManagedServiceGeneration::try_new(2)
                    .expect("second managed generation must be valid"),
            )
            .await,
            Err(ManagedFabricRuntimeError::ExpectedActiveExecution)
        ));
        assert!(
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_err(),
            "the one managed Fabric session must own the requested TCP port"
        );
        let control = core
            .control_handle()
            .expect("ready generation must expose its fence");

        let sequence = core.snapshot.sequence();
        assert!(matches!(
            core.apply(active.clone(), ingress, response_channel)
                .await
                .expect("exact terminal replay must succeed"),
            ManagedFabricApplyOutcome::Replayed(_)
        ));
        assert_eq!(core.snapshot.sequence(), sequence);

        let empty = empty_request(ExpectedActive::Exact(active.target_slice_digest()));
        let empty_ingress = verified(core.clock.reading().expect("clock must read"), 0xc0);
        let ManagedFabricApplyOutcome::Committed(empty_receipt) = core
            .apply(empty, empty_ingress, response_channel)
            .await
            .expect("empty apply must stop the one live session")
        else {
            panic!("first empty apply must commit")
        };
        assert_eq!(
            empty_receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::EmptyExactZero
        );
        assert_eq!(core.snapshot.phase, ManagedFabricDurablePhase::ExactZero);
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .expect("exact-zero completion must release the TCP port");
        drop(listener);
        assert_eq!(
            control
                .with_live_fabric(|_| Box::pin(async {}))
                .await
                .expect_err("retired generation must never revive"),
            ManagedFabricControlError::OwnerRetired
        );
        core.shutdown()
            .await
            .expect("exact-zero shutdown must pass");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn frozen_s0_replays_verified_terminal_rejects_fresh_mutation_and_allows_shutdown() {
        let (_directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let port = available_port();
        let request = active_request(port, ExpectedActive::None);
        let response_channel = channel(&core.projection);
        let ingress = verified(core.clock.reading().expect("clock must read"), 0xd0);
        let ManagedFabricApplyOutcome::Committed(receipt) = core
            .apply(request.clone(), ingress, response_channel)
            .await
            .expect("active Fabric apply must commit")
        else {
            panic!("first Fabric apply must commit")
        };
        core.latch_remote_agent_access_s0_mutation_freeze_v2();
        let frozen_sequence = core.snapshot.sequence();

        assert!(matches!(
            core.apply(request.clone(), ingress, response_channel)
                .await
                .expect("verified exact replay must remain available"),
            ManagedFabricApplyOutcome::Replayed(replayed)
                if replayed.canonical_wire() == receipt.canonical_wire()
        ));
        let empty = empty_request(ExpectedActive::Exact(request.target_slice_digest()));
        let empty_ingress = verified(core.clock.reading().expect("clock must read"), 0xd2);
        let error = core
            .apply(empty, empty_ingress, response_channel)
            .await
            .expect_err("fresh S0 mutation must be unavailable after freeze");
        assert!(error.is_request_unavailable());
        assert_eq!(core.snapshot.sequence(), frozen_sequence);
        assert!(TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_err());

        core.shutdown()
            .await
            .expect("ordered shutdown must ignore S0 freeze");
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .expect("ordered shutdown must release the Fabric endpoint");
        drop(listener);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retained_fabric_terminal_with_bad_owner_signature_is_not_replay_authority() {
        let (_directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let request = active_request(available_port(), ExpectedActive::None);
        let response_channel = channel(&core.projection);
        let ingress = verified(core.clock.reading().expect("clock must read"), 0xe0);
        core.apply(request.clone(), ingress, response_channel)
            .await
            .expect("active Fabric apply must commit");

        let record = core
            .snapshot
            .terminals
            .iter_mut()
            .find(|record| record.operation_id == request.operation_id())
            .expect("committed terminal disappeared");
        let mut tampered = record.receipt.canonical_wire().to_vec();
        *tampered.last_mut().expect("terminal signature disappeared") ^= 1;
        record.receipt = ManagedFabricApplyTerminalReceiptV1::decode(&tampered)
            .expect("opaque bad signature must remain structurally canonical");

        assert!(matches!(
            core.authenticated_terminal_replay(&request, response_channel),
            Err(ManagedFabricRuntimeError::TerminalCorrelation)
        ));
        core.shutdown()
            .await
            .expect("bad retained bytes must not block shutdown");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn active_retained_root_rejects_stopped_live_slot_with_durable_active_retained() {
        let (_directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let port = available_port();
        let active = active_request(port, ExpectedActive::None);
        let execution_digest = active.target_execution().execution_digest();
        let response_channel = channel(&core.projection);
        let ingress = verified(core.clock.reading().expect("clock must read"), 0xc1);
        let ManagedFabricApplyOutcome::Committed(_) = core
            .apply(active, ingress, response_channel)
            .await
            .expect("managed Fabric must become ready")
        else {
            panic!("first active Fabric apply must commit")
        };
        let control = core
            .control_handle()
            .expect("ready generation must expose its fence");
        let generation = control.generation();
        core.export_active_retained_root_v1(execution_digest, generation)
            .await
            .expect("live generation must initially export its retained root");

        let shared = control
            .shared
            .upgrade()
            .expect("durable owner must retain the test slot");
        let live_service = {
            let mut slot = shared.write().await;
            assert_eq!(slot.generation, generation);
            match std::mem::replace(&mut slot.state, ManagedFabricSlotState::Stopped) {
                ManagedFabricSlotState::Live(service) => service,
                _ => panic!("active durable state must retain one live physical service"),
            }
        };
        live_service
            .shutdown()
            .await
            .expect("test must release the physical Fabric service");
        assert_eq!(core.snapshot.phase, ManagedFabricDurablePhase::ActiveReady);
        assert!(core.recovery_completed);
        assert!(matches!(
            core.export_active_retained_root_v1(execution_digest, generation)
                .await,
            Err(ManagedFabricRuntimeError::InvalidDurableState)
        ));
        core.shutdown()
            .await
            .expect("stopped-slot owner cleanup must complete");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn authenticated_cas_rejection_persists_only_truthful_no_effect_terminal() {
        let (_directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let port = available_port();
        let request = active_request(
            port,
            ExpectedActive::Exact(
                paraegox_runtime_contracts::provenance::TargetSliceDigest::new(
                    Digest32::from_bytes([0xd9; 32]),
                ),
            ),
        );
        let response_channel = channel(&core.projection);
        let before = core.snapshot.transition();
        let ManagedFabricApplyOutcome::Committed(receipt) = core
            .apply(
                request,
                verified(core.clock.reading().expect("clock must read"), 0xb2),
                response_channel,
            )
            .await
            .expect("authenticated CAS reject must return PXFT")
        else {
            panic!("first CAS rejection must commit its terminal")
        };
        assert_eq!(
            receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::NoEffectRejected
        );
        assert_eq!(core.snapshot.phase, ManagedFabricDurablePhase::ExactZero);
        assert_eq!(core.snapshot.writer_fence, before.writer_fence);
        assert_eq!(
            core.snapshot.revision_high_water,
            before.revision_high_water
        );
        assert_eq!(core.snapshot.tenure_nonces, before.tenure_nonces);
        assert_eq!(core.snapshot.request_nonces, before.request_nonces);
        assert_eq!(core.snapshot.temporal_lineages, before.temporal_lineages);
        assert_eq!(core.snapshot.terminals.len(), 1);
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .expect("CAS no-effect rejection must not start Fabric");
        drop(listener);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_read_requests_share_generation_while_stop_waits_for_both() {
        let (_directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let port = available_port();
        let request = active_request(port, ExpectedActive::None);
        let response_channel = channel(&core.projection);
        core.apply(
            request,
            verified(core.clock.reading().expect("clock must read"), 0xb3),
            response_channel,
        )
        .await
        .expect("active apply must start the shared session");
        let control = core
            .control_handle()
            .expect("ready generation must expose its fence");
        let entered = Arc::new(Barrier::new(3));
        let release = Arc::new(Barrier::new(3));
        let mut readers = Vec::new();
        for _ in 0..2 {
            let handle = control.clone();
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            readers.push(tokio::spawn(async move {
                handle
                    .with_live_fabric(move |_| {
                        Box::pin(async move {
                            entered.wait().await;
                            release.wait().await;
                        })
                    })
                    .await
            }));
        }
        entered.wait().await;

        let shutdown = tokio::spawn(async move {
            core.shutdown().await.expect("exclusive stop must finish");
            core
        });
        tokio::task::yield_now().await;
        assert!(
            !shutdown.is_finished(),
            "stop must wait for every in-flight shared read request"
        );
        release.wait().await;
        for reader in readers {
            reader
                .await
                .expect("reader task must join")
                .expect("same-generation read must complete");
        }
        let core = shutdown.await.expect("shutdown task must join");
        assert_eq!(
            control
                .with_live_fabric(|_| Box::pin(async {}))
                .await
                .expect_err("post-stop handle must stay fenced"),
            ManagedFabricControlError::OwnerRetired
        );
        drop(core);
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .expect("exclusive stop must release the one TCP listener");
        drop(listener);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn active_restart_uses_fresh_durable_generation_before_real_rebind() {
        let (directory, mut first) = fresh_core(1, 3);
        first.recover().await.expect("fresh recovery must pass");
        let port = available_port();
        let request = active_request(port, ExpectedActive::None);
        let fabric_execution_digest = request.target_execution().execution_digest();
        let response_channel = channel(&first.projection);
        let ManagedFabricApplyOutcome::Committed(initial_receipt) = first
            .apply(
                request,
                verified(first.clock.reading().expect("clock must read"), 0xb4),
                response_channel,
            )
            .await
            .expect("initial active apply must pass")
        else {
            panic!("initial active apply must commit")
        };
        first
            .shutdown()
            .await
            .expect("first owner must stop exactly");
        drop(first);

        let stale_projection = projection();
        let stale_projection_digest =
            transition_projection_digest(&stale_projection).expect("projection digest must build");
        let stale_store = ManagedFabricStore::open_fixture(
            directory.path(),
            [STORE_BYTE; 32],
            Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
            stale_projection_digest,
        )
        .expect("successor store must reopen for stale-epoch proof");
        assert!(matches!(
            ManagedFabricRuntimeCore::from_preopened_store(
                stale_store,
                config(directory.path(), stale_projection, 1, clock(4, 200)),
            ),
            Err(ManagedFabricRuntimeError::RuntimeEpochRegressed)
        ));

        let projection = projection();
        let projection_digest =
            transition_projection_digest(&projection).expect("projection digest must build");
        let store = ManagedFabricStore::open_fixture(
            directory.path(),
            [STORE_BYTE; 32],
            Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
            projection_digest,
        )
        .expect("successor store must reopen");
        let mut restarted = ManagedFabricRuntimeCore::from_preopened_store(
            store,
            config(directory.path(), projection, 2, clock(4, 200)),
        )
        .expect("restarted core must decode durable active state");
        restarted
            .recover()
            .await
            .expect("free old port must allow managed recovery");
        assert_eq!(
            restarted.snapshot.phase,
            ManagedFabricDurablePhase::ActiveReady
        );
        assert_eq!(restarted.snapshot.generation_high_water(), 2);
        let recovered_generation = restarted
            .snapshot
            .active
            .as_ref()
            .expect("recovered service must be active")
            .generation;
        assert_eq!(recovered_generation.value(), 2);
        let retained_root = restarted
            .export_active_retained_root_v1(fabric_execution_digest, recovered_generation)
            .await
            .expect("restart must retain the correlated historical PXFT root");
        assert_eq!(
            retained_root.active_pxft_digest,
            initial_receipt.receipt_digest()
        );
        assert_eq!(retained_root.fabric_generation, recovered_generation);
        assert!(
            TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_err(),
            "recovered generation must own the exact requested port"
        );
        restarted
            .shutdown()
            .await
            .expect("recovered owner must stop exactly");
    }

    async fn quarantined_raw_evidence(port: u16) -> (Digest32, Digest32) {
        let (directory, mut first) = fresh_core(1, 3);
        first.recover().await.expect("fresh recovery must pass");
        let request = active_request(port, ExpectedActive::None);
        let response_channel = channel(&first.projection);
        let ingress = verified(first.clock.reading().expect("clock must read"), 0xb6);
        let mut intent = first
            .admit_transition(&request, ingress)
            .expect("start intent admission must pass");
        let generation = next_generation(first.snapshot.generation_high_water())
            .expect("first generation must exist");
        intent.generation_high_water = generation.value();
        intent.phase = ManagedFabricDurablePhase::StartIntent;
        intent.pending = Some(ManagedFabricDurablePending {
            kind: ManagedFabricPendingKind::Start,
            generation: Some(generation),
            admitted_clock_generation: ingress.clock_generation(),
            admitted_at_nanos: ingress.admitted_at_nanos(),
            deadline_nanos: ingress.deadline_nanos(),
            response_channel,
            request: request.clone(),
        });
        first
            .commit_transition(intent)
            .expect("crash-point intent must be durable");
        drop(first);

        let blocker = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .expect("test must exclusively occupy the recovery port");
        let projection = projection();
        let projection_digest =
            transition_projection_digest(&projection).expect("projection digest must build");
        let store = ManagedFabricStore::open_fixture(
            directory.path(),
            [STORE_BYTE; 32],
            Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
            projection_digest,
        )
        .expect("successor store must reopen");
        let mut restarted = ManagedFabricRuntimeCore::from_preopened_store(
            store,
            config(directory.path(), projection, 2, clock(4, 200)),
        )
        .expect("restart must decode the durable start intent");
        assert!(matches!(
            restarted
                .recover()
                .await
                .expect_err("busy exact port must quarantine recovery"),
            ManagedFabricRuntimeError::RecoveryQuarantined
        ));
        assert_eq!(
            restarted.snapshot.phase,
            ManagedFabricDurablePhase::Quarantined
        );
        assert_eq!(restarted.snapshot.generation_high_water(), 2);
        let receipt = restarted
            .lookup_terminal(&request, response_channel)
            .expect("terminal lookup must validate")
            .expect("quarantine must persist a terminal for the pending request");
        assert_eq!(
            receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::Quarantined
        );
        let raw = receipt.facts().raw_outcome_digest();
        let reason = restarted
            .snapshot
            .quarantine_reason
            .expect("quarantine reason must be durable");
        drop(blocker);
        (raw, reason)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn busy_exact_ports_quarantine_without_blind_bind_and_commit_distinct_evidence() {
        let first_port = available_port();
        let mut second_port = available_port();
        while second_port == first_port {
            second_port = available_port();
        }
        let first = quarantined_raw_evidence(first_port).await;
        let second = quarantined_raw_evidence(second_port).await;
        assert_ne!(
            first.1, second.1,
            "port identity must change quarantine reason"
        );
        assert_ne!(
            first.0, second.0,
            "PXFT raw evidence must commit the distinct quarantine reason"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_between_finished_probe_and_ready_cas_cannot_publish_agent_owner() {
        let (directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let fabric_port = available_port();
        let active = active_request(fabric_port, ExpectedActive::None);
        let execution_digest = active.target_execution().execution_digest();
        let stack_execution = agent_stack_execution(active.target_execution().clone());
        let response_channel = channel(&core.projection);
        let ingress = verified(core.clock.reading().expect("clock must read"), 0xcf);
        let ManagedFabricApplyOutcome::Committed(_) = core
            .apply(active, ingress, response_channel)
            .await
            .expect("managed Fabric must become ready")
        else {
            panic!("first active Fabric apply must commit")
        };
        let fabric_control = core
            .control_handle()
            .expect("ready Fabric generation must expose its fence");
        let fabric_generation = fabric_control.generation();

        let startup_error = match ManagedAgentAssembly::start_with_terminal_ready_race_for_test(
            fabric_control.clone(),
            &stack_execution,
            directory.path().to_path_buf(),
            &DeterministicFixtureResolver,
        )
        .await
        {
            Err(error) => error,
            Ok(_) => panic!("terminal owner state must prevent assembly and handle publication"),
        };
        assert!(matches!(
            startup_error,
            ManagedAgentAssemblyError::ServerStoppedBeforeReady
        ));
        assert_eq!(
            fabric_control
                .binding_census()
                .await
                .expect("failed Agent startup must retain the live Fabric generation"),
            0,
            "failed publication must retire both Agent bindings exactly"
        );
        core.export_active_retained_root_v1(execution_digest, fabric_generation)
            .await
            .expect("failed Agent publication must not retire the live Fabric owner");
        core.shutdown()
            .await
            .expect("test Fabric owner must stop exactly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn managed_agent_uses_live_fabric_for_two_turns_then_retires_to_exact_zero() {
        let (directory, mut core) = fresh_core(1, 3);
        core.recover().await.expect("fresh recovery must pass");
        let fabric_port = available_port();
        let active = active_request(fabric_port, ExpectedActive::None);
        let active_digest = active.target_slice_digest();
        let fabric_execution = active.target_execution().clone();
        let response_channel = channel(&core.projection);
        let ingress = verified(core.clock.reading().expect("clock must read"), 0xd0);
        let ManagedFabricApplyOutcome::Committed(active_receipt) = core
            .apply(active, ingress, response_channel)
            .await
            .expect("managed Fabric must become ready")
        else {
            panic!("first active Fabric apply must commit")
        };
        assert_eq!(
            active_receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::ActiveReady
        );

        let stack_execution = agent_stack_execution(fabric_execution);
        let fabric_control = core
            .control_handle()
            .expect("ready Fabric generation must expose its fence");
        let fabric_generation = fabric_control.generation();
        let (mut assembly, handle) = ManagedAgentAssembly::start_from_execution(
            fabric_control.clone(),
            &stack_execution,
            directory.path().to_path_buf(),
            &DeterministicFixtureResolver,
        )
        .await
        .expect("Agent must install on the existing Fabric generation");

        let live_port = assembly
            .export_live_conversation_port_descriptor_v1(&handle, &handle, fabric_generation)
            .await
            .expect("exact live owners and census must export PXAP facts");
        let descriptor = AgentConversationPortDescriptorV1::decode(&live_port.descriptor_wire)
            .expect("exported PXAP must strictly decode");
        assert_eq!(live_port.physical_binding_census, 2);
        assert_eq!(live_port.descriptor_digest, descriptor.descriptor_digest());
        assert_eq!(
            live_port.request_binding_descriptor_digest,
            descriptor.request_binding_descriptor_digest()
        );
        assert_eq!(
            live_port.event_binding_descriptor_digest,
            descriptor.event_binding_descriptor_digest()
        );
        assert_ne!(live_port.submit_binding_epoch, 0);
        assert_ne!(live_port.control_binding_epoch, 0);
        assert_eq!(
            live_port.fabric_session_epoch,
            fabric_control
                .observe_live_fabric_exact_census_once(2, |fabric| fabric.session_epoch())
                .await
                .expect("same read fence must expose the live Session epoch")
        );
        let wrong_generation = ManagedServiceGeneration::try_new(
            fabric_generation
                .value()
                .checked_add(1)
                .expect("test generation must not overflow"),
        )
        .expect("next test generation must be valid");
        assert!(matches!(
            assembly
                .export_live_conversation_port_descriptor_v1(&handle, &handle, wrong_generation,)
                .await,
            Err(ManagedAgentAssemblyError::InstalledPortUnavailable)
        ));

        let shared = fabric_control
            .shared
            .upgrade()
            .expect("live test control must retain its owner");
        {
            let mut slot = shared.write().await;
            assert_eq!(slot.owned_binding_count, 2);
            slot.owned_binding_count = 1;
        }
        let census_error = match assembly
            .export_live_conversation_port_descriptor_v1(&handle, &handle, fabric_generation)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("wrong exact census must fail before live observation"),
        };
        {
            let mut slot = shared.write().await;
            slot.owned_binding_count = 2;
        }
        assert!(matches!(
            census_error,
            ManagedAgentAssemblyError::FabricControl(
                ManagedFabricControlError::BindingCensusMismatch
            )
        ));

        let deck_run_id = AgentConversationDeckRunId::try_from_bytes([0xb1; 16])
            .expect("DeckRun id must be valid");
        let session_id = AgentConversationSessionId::try_from_bytes([0xb2; 16])
            .expect("Session id must be valid");
        assert_eq!(
            handle
                .open_session(deck_run_id, session_id, Duration::from_secs(2))
                .await
                .expect("explicit Session open must round trip"),
            AgentConversationOpenOutcomeV1::Opened
        );

        let first = AgentConversationRequestV1::try_new(
            deck_run_id,
            session_id,
            AgentConversationTurnId::try_from_bytes([0xb3; 16]).expect("Turn id must be valid"),
            AgentConversationRequestId::try_from_bytes([0xb4; 16])
                .expect("request id must be valid"),
            2_000_000_000,
            "first managed turn",
        )
        .expect("first request must be valid");
        let first_terminal = handle
            .submit(first.clone(), Duration::from_secs(2))
            .await
            .expect("first turn must terminate");
        assert_eq!(
            first_terminal.result(),
            &AgentConversationTerminalResultV1::Success("echo: first managed turn".into())
        );
        assert_eq!(
            handle
                .get(
                    deck_run_id,
                    session_id,
                    first.request_id(),
                    Duration::from_secs(2),
                )
                .await
                .expect("get must observe first terminal"),
            AgentConversationGetStateV1::Terminal(first_terminal)
        );

        let mut closed_lease = handle.clone();
        closed_lease.close().await.expect("lease close must pass");
        assert!(matches!(
            closed_lease
                .open_session(deck_run_id, session_id, Duration::from_secs(2))
                .await,
            Err(RuntimeAgentConversationError::Closed)
        ));

        let second = AgentConversationRequestV1::try_new(
            deck_run_id,
            session_id,
            AgentConversationTurnId::try_from_bytes([0xb5; 16]).expect("Turn id must be valid"),
            AgentConversationRequestId::try_from_bytes([0xb6; 16])
                .expect("request id must be valid"),
            2_000_000_000,
            "second managed turn",
        )
        .expect("second request must be valid");
        let second_terminal = handle
            .submit(second.clone(), Duration::from_secs(2))
            .await
            .expect("second turn must terminate");
        assert_eq!(
            second_terminal.result(),
            &AgentConversationTerminalResultV1::Success("echo: second managed turn".into())
        );
        assert_eq!(
            handle
                .cancel(
                    deck_run_id,
                    session_id,
                    second.request_id(),
                    Duration::from_secs(2),
                )
                .await
                .expect("cancel must observe the terminal truth"),
            AgentConversationCancelStateV1::Terminal(second_terminal)
        );
        let batch = handle
            .watch(deck_run_id, session_id, 0, 16, Duration::from_secs(2))
            .await
            .expect("watch must round trip")
            .expect("opened Session must retain events");
        assert_eq!(batch.events().len(), 5);
        assert_eq!(batch.next_cursor(), batch.high_watermark());
        assert!(!batch.has_more());

        assembly
            .shutdown()
            .await
            .expect("Agent must retire both physical bindings before Fabric");
        assert!(matches!(
            handle
                .open_session(deck_run_id, session_id, Duration::from_secs(2))
                .await,
            Err(RuntimeAgentConversationError::OwnerRetired)
        ));

        let empty = empty_request(ExpectedActive::Exact(active_digest));
        let empty_ingress = verified(core.clock.reading().expect("clock must read"), 0xd1);
        let ManagedFabricApplyOutcome::Committed(empty_receipt) = core
            .apply(empty, empty_ingress, response_channel)
            .await
            .expect("Fabric exact-zero must accept the retired Agent census")
        else {
            panic!("first empty apply must commit")
        };
        assert_eq!(
            empty_receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::EmptyExactZero
        );
        core.shutdown()
            .await
            .expect("exact-zero Runtime shutdown must pass");
    }
}
