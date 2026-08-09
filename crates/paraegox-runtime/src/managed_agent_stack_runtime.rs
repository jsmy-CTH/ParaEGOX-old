#![cfg(unix)]

//! RuntimeHost-owned PXAR-v7 Fabric→Agent durable apply owner.
//!
//! PXAS is an independent successor journal. The first intent is embedded in
//! the immutable PXSC cutover record, and every physical effect follows a
//! durable intent. The predecessor PXMS journal remains byte-compatible and is
//! used only as the lifecycle substrate for the already-admitted Fabric.

use core::fmt;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use ed25519_dalek::{Signature, Signer, SigningKey};
use paraegox_kernel::digest::{Digest32, Digest32Builder, DigestBuildError};
use paraegox_kernel::time::ClockReading;
use paraegox_runtime_contracts::apply::ExpectedActive;
use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    DistributedAgentStackTerminalOutcomeV1, DistributedAgentStackTerminalReceiptV1,
    DistributedAgentStackTerminalReceiptV2, DistributedFabricSessionEpochV1,
};
use paraegox_runtime_contracts::managed_agent_stack_plan::{
    ManagedAgentStackApplyRequestV1, ManagedAgentStackPlanError, ManagedAgentStackProjectionV1,
    ManagedAgentStackTargetModeV1, ManagedAgentStackTerminalAuthClaimV1,
    ManagedAgentStackTerminalEvidenceFieldsV1, ManagedAgentStackTerminalEvidenceV1,
    ManagedAgentStackTerminalFactsV1, ManagedAgentStackTerminalHeadV1,
    ManagedAgentStackTerminalLifecycleEffectV1, ManagedAgentStackTerminalOutcomeV1,
    ManagedAgentStackTerminalReceiptDraftV1, ManagedAgentStackTerminalReceiptV1,
    ManagedAgentStackTerminalStateV1,
};
use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
    ManagedModelAgentStackTerminalOutcomeV1, ManagedModelAgentStackTerminalReceiptV1,
};
use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
use paraegox_runtime_contracts::managed_serving_bootstrap::RuntimeVerifiedHistoricalManagedAgentStackReceiptV1;
use paraegox_runtime_contracts::reference_control::ReferenceChannelBindingV1;
use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyAuthKeyRef};

use crate::admission::{
    ED25519_ALGORITHM, ED25519_ALGORITHM_VERSION, VerifiedManagedAgentStackApplyIngressV1,
};
#[cfg(test)]
use crate::managed_agent_runtime::LiveConversationPortExportTestInterlockV1;
use crate::managed_agent_runtime::{
    ManagedAgentAssembly, ManagedAgentAssemblyConfig, ManagedAgentAssemblyError,
    RuntimeAgentConversationHandle,
};
use crate::managed_agent_stack_state::{
    ManagedAgentStackDurableActive, ManagedAgentStackDurablePending, ManagedAgentStackDurablePhase,
    ManagedAgentStackPendingKind, ManagedAgentStackReplayRecord,
    ManagedAgentStackRevisionHighWater, ManagedAgentStackSnapshot,
    ManagedAgentStackSnapshotTransition, ManagedAgentStackStateError,
    ManagedAgentStackTerminalRecord, ManagedAgentStackWriterFence,
};
use crate::managed_fabric_runtime::{
    ManagedFabricControlHandle, ManagedFabricRuntimeCore, ManagedFabricRuntimeError,
    ManagedFabricStackCutoverObservation,
};
use crate::runtime_agent_provider::{
    RuntimeAgentProviderResolverV1, RuntimeResolvedAgentProviderV1,
};
use crate::runtime_clock::{RuntimeClock, RuntimeClockError};

const STACK_PROJECTION_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-agent-stack-transition-projection.sha256.v1";
const STACK_RESOURCE_CENSUS_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-agent-stack-resource-census.sha256.v1";
const STACK_RAW_OUTCOME_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-agent-stack-raw-outcome.sha256.v1";
const STACK_QUARANTINE_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.managed-agent-stack-quarantine.sha256.v1";
const MAX_STACK_REPLAY_RECORDS: usize = 256;

pub(crate) struct ManagedAgentStackOwnerConfig {
    pub(crate) state_directory: PathBuf,
    pub(crate) projection: ManagedAgentStackProjectionV1,
    pub(crate) runtime_host_epoch: u64,
    pub(crate) clock: RuntimeClock,
    pub(crate) response_key_ref: ApplyAuthKeyRef,
    pub(crate) response_signer: SigningKey,
    pub(crate) handle_broker: RuntimeAgentHandleBroker,
    pub(crate) provider_resolver: Arc<dyn RuntimeAgentProviderResolverV1>,
}

/// Process-local issuance point for opaque Agent conversation capabilities.
/// A handle appears only after its protocol-specific ActiveReady receipt is
/// durably published. A trusted restricted endpoint may attach one exact
/// PXDS-v2 alias to the currently published PXDS-v1 receipt; retirement or any
/// successor publication clears that alias.
#[derive(Clone, Default)]
pub(crate) struct RuntimeAgentHandleBroker {
    inner: Arc<RwLock<Option<PublishedRuntimeAgentHandle>>>,
}

struct PublishedRuntimeAgentHandle {
    handle: RuntimeAgentConversationHandle,
    publication_identity: Arc<RuntimeAgentHandlePublicationIdentity>,
    committed_receipt_wire: Box<[u8]>,
    restricted_distributed_alias_wire: Option<Box<[u8]>>,
}

struct RuntimeAgentHandlePublicationIdentity;

/// Owned proof of one exact broker publication. It deliberately is not
/// Clone: a raw cloned conversation handle cannot preserve publication
/// authority across an asynchronous owner observation.
struct RuntimeAgentHandlePublicationClaim {
    handle: RuntimeAgentConversationHandle,
    publication_identity: Arc<RuntimeAgentHandlePublicationIdentity>,
}

impl PublishedRuntimeAgentHandle {
    fn new(handle: RuntimeAgentConversationHandle, committed_receipt_wire: &[u8]) -> Self {
        Self {
            handle,
            publication_identity: Arc::new(RuntimeAgentHandlePublicationIdentity),
            committed_receipt_wire: committed_receipt_wire.into(),
            restricted_distributed_alias_wire: None,
        }
    }
}

impl RuntimeAgentHandleBroker {
    pub(crate) fn try_acquire(&self) -> Option<RuntimeAgentConversationHandle> {
        self.inner
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|published| published.handle.clone()))
    }

    pub(crate) fn try_claim(
        &self,
        committed_receipt_wire: &[u8],
    ) -> Result<Option<RuntimeAgentConversationHandle>, ManagedAgentStackRuntimeError> {
        let receipt = ManagedAgentStackTerminalReceiptV1::decode(committed_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        if receipt.facts().state().outcome() != ManagedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        let guard = self
            .inner
            .read()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        Ok(guard.as_ref().and_then(|published| {
            (published.committed_receipt_wire.as_ref() == committed_receipt_wire)
                .then(|| published.handle.clone())
        }))
    }

    fn try_claim_publication(
        &self,
        committed_receipt_wire: &[u8],
    ) -> Result<Option<RuntimeAgentHandlePublicationClaim>, ManagedAgentStackRuntimeError> {
        let receipt = ManagedAgentStackTerminalReceiptV1::decode(committed_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        if receipt.facts().state().outcome() != ManagedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        let guard = self
            .inner
            .read()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        Ok(guard.as_ref().and_then(|published| {
            (published.committed_receipt_wire.as_ref() == committed_receipt_wire).then(|| {
                RuntimeAgentHandlePublicationClaim {
                    handle: published.handle.clone(),
                    publication_identity: Arc::clone(&published.publication_identity),
                }
            })
        }))
    }

    fn retains_publication_claim(
        &self,
        claim: &RuntimeAgentHandlePublicationClaim,
        committed_receipt_wire: &[u8],
    ) -> Result<bool, ManagedAgentStackRuntimeError> {
        let guard = self
            .inner
            .read()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        Ok(guard.as_ref().is_some_and(|published| {
            Arc::ptr_eq(&published.publication_identity, &claim.publication_identity)
                && published.committed_receipt_wire.as_ref() == committed_receipt_wire
        }))
    }

    pub(crate) fn try_claim_distributed(
        &self,
        committed_receipt_wire: &[u8],
    ) -> Result<Option<RuntimeAgentConversationHandle>, ManagedAgentStackRuntimeError> {
        let receipt = DistributedAgentStackTerminalReceiptV1::decode(committed_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        if receipt.facts().outcome() != DistributedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        let guard = self
            .inner
            .read()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        Ok(guard.as_ref().and_then(|published| {
            (published.committed_receipt_wire.as_ref() == committed_receipt_wire)
                .then(|| published.handle.clone())
        }))
    }

    pub(crate) fn try_claim_restricted_distributed(
        &self,
        committed_receipt_wire: &[u8],
    ) -> Result<Option<RuntimeAgentConversationHandle>, ManagedAgentStackRuntimeError> {
        let receipt = DistributedAgentStackTerminalReceiptV2::decode(committed_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        if receipt.facts().outcome() != DistributedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        let guard = self
            .inner
            .read()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        Ok(guard.as_ref().and_then(|published| {
            published
                .restricted_distributed_alias_wire
                .as_deref()
                .is_some_and(|alias| alias == committed_receipt_wire)
                .then(|| published.handle.clone())
        }))
    }

    pub(crate) fn try_claim_model_agent(
        &self,
        committed_receipt_wire: &[u8],
    ) -> Result<Option<RuntimeAgentConversationHandle>, ManagedAgentStackRuntimeError> {
        let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(committed_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        if receipt.facts().state().outcome() != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
        {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        let guard = self
            .inner
            .read()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        Ok(guard.as_ref().and_then(|published| {
            (published.committed_receipt_wire.as_ref() == committed_receipt_wire)
                .then(|| published.handle.clone())
        }))
    }

    fn publish(
        &self,
        handle: RuntimeAgentConversationHandle,
        receipt: &ManagedAgentStackTerminalReceiptV1,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        if receipt.facts().state().outcome() != ManagedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(ManagedAgentStackRuntimeError::InvalidDurableState);
        }
        *self
            .inner
            .write()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)? = Some(
            PublishedRuntimeAgentHandle::new(handle, receipt.canonical_wire()),
        );
        Ok(())
    }

    pub(crate) fn publish_distributed(
        &self,
        handle: RuntimeAgentConversationHandle,
        receipt: &DistributedAgentStackTerminalReceiptV1,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        if receipt.facts().outcome() != DistributedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(ManagedAgentStackRuntimeError::InvalidDurableState);
        }
        *self
            .inner
            .write()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)? = Some(
            PublishedRuntimeAgentHandle::new(handle, receipt.canonical_wire()),
        );
        Ok(())
    }

    pub(crate) fn register_restricted_distributed_alias(
        &self,
        committed_inner_receipt_wire: &[u8],
        committed_outer_receipt_wire: &[u8],
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        let inner = DistributedAgentStackTerminalReceiptV1::decode(committed_inner_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        let outer = DistributedAgentStackTerminalReceiptV2::decode(committed_outer_receipt_wire)
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        if inner.facts().outcome() != DistributedAgentStackTerminalOutcomeV1::ActiveReady
            || outer.facts().outcome() != DistributedAgentStackTerminalOutcomeV1::ActiveReady
            || inner.facts() != outer.facts()
        {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        // Both strict decoders reconstruct their canonical frame and compare
        // it byte-for-byte with the input. The shared typed facts equality
        // therefore also proves equality of their canonical facts bytes; no
        // PXDS2-to-PXDS1 reconstruction is involved here.
        let mut guard = self
            .inner
            .write()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)?;
        let published = guard
            .as_mut()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        if published.committed_receipt_wire.as_ref() != committed_inner_receipt_wire {
            return Err(ManagedAgentStackRuntimeError::InvalidDurableState);
        }
        match published.restricted_distributed_alias_wire.as_deref() {
            None => {
                published.restricted_distributed_alias_wire =
                    Some(committed_outer_receipt_wire.into());
                Ok(())
            }
            Some(alias) if alias == committed_outer_receipt_wire => Ok(()),
            Some(_) => Err(ManagedAgentStackRuntimeError::InvalidDurableState),
        }
    }

    pub(crate) fn publish_model_agent(
        &self,
        handle: RuntimeAgentConversationHandle,
        receipt: &ManagedModelAgentStackTerminalReceiptV1,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        if receipt.facts().state().outcome() != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
        {
            return Err(ManagedAgentStackRuntimeError::InvalidDurableState);
        }
        *self
            .inner
            .write()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)? = Some(
            PublishedRuntimeAgentHandle::new(handle, receipt.canonical_wire()),
        );
        Ok(())
    }

    pub(crate) fn revoke(&self) -> Result<(), ManagedAgentStackRuntimeError> {
        *self
            .inner
            .write()
            .map_err(|_| ManagedAgentStackRuntimeError::HandleBrokerUnavailable)? = None;
        Ok(())
    }
}

pub(crate) struct ManagedAgentStackRuntimeCore {
    snapshot: ManagedAgentStackSnapshot,
    projection: ManagedAgentStackProjectionV1,
    state_directory: PathBuf,
    runtime_host_epoch: u64,
    clock: RuntimeClock,
    response_key_ref: ApplyAuthKeyRef,
    response_signer: SigningKey,
    assembly: Option<ManagedAgentAssembly>,
    handle: Option<RuntimeAgentConversationHandle>,
    handle_broker: RuntimeAgentHandleBroker,
    provider_resolver: Arc<dyn RuntimeAgentProviderResolverV1>,
    recovery_completed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedAgentStackDistributedCutoverObservation {
    pub(crate) execution:
        paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentStackTargetExecutionV1,
    pub(crate) target_slice_digest: paraegox_runtime_contracts::provenance::TargetSliceDigest,
    pub(crate) fabric_generation: ManagedServiceGeneration,
    pub(crate) agent_generation: ManagedServiceGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ManagedAgentStackApplyOutcome {
    Committed(ManagedAgentStackTerminalReceiptV1),
    Replayed(ManagedAgentStackTerminalReceiptV1),
    HistoricalReplayed(RuntimeVerifiedHistoricalManagedAgentStackReceiptV1),
}

/// One bootstrap-only snapshot of the exact currently published Agent port.
pub(crate) struct RuntimeAgentConversationPortExportV1 {
    pub(crate) active_pxst_digest: Digest32,
    pub(crate) descriptor_wire: Box<[u8]>,
    pub(crate) fabric_generation: ManagedServiceGeneration,
    pub(crate) agent_generation: ManagedServiceGeneration,
    pub(crate) fabric_execution_digest: Digest32,
    pub(crate) fabric_session_epoch: DistributedFabricSessionEpochV1,
    pub(crate) descriptor_digest: Digest32,
    pub(crate) request_binding_descriptor_digest: Digest32,
    pub(crate) event_binding_descriptor_digest: Digest32,
    pub(crate) submit_binding_epoch: u64,
    pub(crate) control_binding_epoch: u64,
    pub(crate) physical_binding_census: u16,
}

/// Exact current Agent root selected by the owner itself. This value is owned
/// and intentionally non-Clone; it contains no conversation handle, route, or
/// Fabric session capability.
pub(crate) struct RuntimeAgentCurrentConversationPortExportV2 {
    active_request: ManagedAgentStackApplyRequestV1,
    active_terminal_receipt: ManagedAgentStackTerminalReceiptV1,
    live_port: RuntimeAgentConversationPortExportV1,
}

impl RuntimeAgentCurrentConversationPortExportV2 {
    pub(crate) fn active_request(&self) -> &ManagedAgentStackApplyRequestV1 {
        &self.active_request
    }

    pub(crate) fn active_terminal_receipt(&self) -> &ManagedAgentStackTerminalReceiptV1 {
        &self.active_terminal_receipt
    }

    pub(crate) const fn live_port(&self) -> &RuntimeAgentConversationPortExportV1 {
        &self.live_port
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeAgentConversationPortExportErrorV1 {
    ExpectedActiveReceiptMismatch,
    OwnerUnavailable,
    InternalInvariant,
}

#[derive(Clone, Copy)]
struct TerminalSelection {
    outcome: ManagedAgentStackTerminalOutcomeV1,
    lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1,
    head: ManagedAgentStackTerminalHeadV1,
    fabric_generation: Option<ManagedServiceGeneration>,
    agent_generation: Option<ManagedServiceGeneration>,
    physical_binding_census: u16,
    census_complete: bool,
    fabric_ready: bool,
    agent_ready: bool,
    dependency_satisfied: bool,
    exact_zero: bool,
    quarantined: bool,
    raw_code: u16,
    raw_context: Option<Digest32>,
}

impl ManagedAgentStackRuntimeCore {
    pub(crate) fn open(
        fabric: &ManagedFabricRuntimeCore,
        config: ManagedAgentStackOwnerConfig,
    ) -> Result<Option<Self>, ManagedAgentStackRuntimeError> {
        let projection_digest = stack_projection_digest(&config.projection)?;
        let Some(stored_projection_digest) = fabric.managed_agent_stack_projection_digest() else {
            if fabric.managed_agent_stack_snapshot_bytes()?.is_some() {
                return Err(ManagedAgentStackRuntimeError::InvalidDurableState);
            }
            return Ok(None);
        };
        if stored_projection_digest != projection_digest {
            return Err(ManagedAgentStackRuntimeError::ProjectionMismatch);
        }
        let frame = fabric
            .managed_agent_stack_snapshot_bytes()?
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        let snapshot = ManagedAgentStackSnapshot::decode(
            frame,
            fabric.store_instance_id(),
            fabric.owner_target_fingerprint(),
            projection_digest,
            &config.projection,
        )?;
        if config.runtime_host_epoch == 0
            || config.runtime_host_epoch <= snapshot.runtime_host_epoch()
            || config.clock.generation().value() == 0
        {
            return Err(ManagedAgentStackRuntimeError::RuntimeEpochRegressed);
        }
        Ok(Some(Self {
            snapshot,
            projection: config.projection,
            state_directory: config.state_directory,
            runtime_host_epoch: config.runtime_host_epoch,
            clock: config.clock,
            response_key_ref: config.response_key_ref,
            response_signer: config.response_signer,
            assembly: None,
            handle: None,
            handle_broker: config.handle_broker,
            provider_resolver: config.provider_resolver,
            recovery_completed: false,
        }))
    }

    pub(crate) fn requires_predecessor_recovery(&self) -> bool {
        matches!(
            self.snapshot.phase,
            ManagedAgentStackDurablePhase::AgentStartIntent
                | ManagedAgentStackDurablePhase::ActiveReady
                | ManagedAgentStackDurablePhase::RecoveryIntent
        )
    }

    pub(crate) fn distributed_cutover_observation(
        &self,
    ) -> Result<ManagedAgentStackDistributedCutoverObservation, ManagedAgentStackRuntimeError> {
        if !self.recovery_completed
            || self.snapshot.phase != ManagedAgentStackDurablePhase::ActiveReady
            || self.assembly.is_none()
            || self.handle.is_none()
        {
            return Err(ManagedAgentStackRuntimeError::RecoveryNotCompleted);
        }
        let active = self
            .snapshot
            .active
            .as_ref()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        Ok(ManagedAgentStackDistributedCutoverObservation {
            execution: active.request.target_execution().clone(),
            target_slice_digest: active.request.target_slice_digest(),
            fabric_generation: active.fabric_generation,
            agent_generation: active.agent_generation,
        })
    }

    /// Exports a PXAP only from the intersection of durable ActiveReady,
    /// complete two-binding census, the exact currently brokered PXST root,
    /// and this core's live assembly/handle. The terminal receipt generation
    /// is deliberately not compared with the current physical generations:
    /// restart recovery may retain exact PXST bytes while rebuilding both live
    /// generations.
    pub(crate) async fn export_active_conversation_port_v1(
        &self,
        expected_active_pxst_digest: Digest32,
    ) -> Result<RuntimeAgentConversationPortExportV1, RuntimeAgentConversationPortExportErrorV1>
    {
        let current = self
            .export_current_conversation_port_inner_v2(
                Some(expected_active_pxst_digest),
                #[cfg(test)]
                None,
            )
            .await?;
        Ok(current.live_port)
    }

    /// Exports the exact current ActiveReady PXAR-v7 request, PXST root, and
    /// live PXAP facts without accepting a caller-selected PXST or PXRS value.
    pub(crate) async fn export_current_conversation_port_v2(
        &self,
    ) -> Result<
        RuntimeAgentCurrentConversationPortExportV2,
        RuntimeAgentConversationPortExportErrorV1,
    > {
        self.export_current_conversation_port_inner_v2(
            None,
            #[cfg(test)]
            None,
        )
        .await
    }

    #[cfg(test)]
    async fn export_current_conversation_port_with_interlock_v2(
        &self,
        interlock: &LiveConversationPortExportTestInterlockV1,
    ) -> Result<
        RuntimeAgentCurrentConversationPortExportV2,
        RuntimeAgentConversationPortExportErrorV1,
    > {
        self.export_current_conversation_port_inner_v2(None, Some(interlock))
            .await
    }

    async fn export_current_conversation_port_inner_v2(
        &self,
        expected_active_pxst_digest: Option<Digest32>,
        #[cfg(test)] interlock: Option<&LiveConversationPortExportTestInterlockV1>,
    ) -> Result<
        RuntimeAgentCurrentConversationPortExportV2,
        RuntimeAgentConversationPortExportErrorV1,
    > {
        if !self.recovery_completed
            || self.snapshot.phase != ManagedAgentStackDurablePhase::ActiveReady
        {
            return Err(RuntimeAgentConversationPortExportErrorV1::OwnerUnavailable);
        }
        if self.snapshot.physical_binding_census != 2
            || !self.snapshot.census_complete
            || !self.snapshot.fabric_ready
            || !self.snapshot.agent_ready
            || !self.snapshot.dependency_satisfied
        {
            return Err(RuntimeAgentConversationPortExportErrorV1::InternalInvariant);
        }
        let active = self
            .snapshot
            .active
            .as_ref()
            .ok_or(RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?;
        let receipt = self
            .active_terminal_receipt()
            .map_err(|_| RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?;
        if receipt.facts().state().outcome() != ManagedAgentStackTerminalOutcomeV1::ActiveReady {
            return Err(RuntimeAgentConversationPortExportErrorV1::InternalInvariant);
        }
        if expected_active_pxst_digest.is_some_and(|expected| receipt.receipt_digest() != expected)
        {
            return Err(RuntimeAgentConversationPortExportErrorV1::ExpectedActiveReceiptMismatch);
        }
        let broker_claim = self
            .handle_broker
            .try_claim_publication(receipt.canonical_wire())
            .map_err(|_| RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?
            .ok_or(RuntimeAgentConversationPortExportErrorV1::OwnerUnavailable)?;
        let owner_handle = self
            .handle
            .as_ref()
            .ok_or(RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?;
        let assembly = self
            .assembly
            .as_ref()
            .ok_or(RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?;
        #[cfg(test)]
        let live_port = match interlock {
            Some(interlock) => {
                assembly
                    .export_live_conversation_port_descriptor_with_interlock_v1(
                        owner_handle,
                        &broker_claim.handle,
                        active.fabric_generation,
                        interlock,
                    )
                    .await
            }
            None => {
                assembly
                    .export_live_conversation_port_descriptor_v1(
                        owner_handle,
                        &broker_claim.handle,
                        active.fabric_generation,
                    )
                    .await
            }
        }
        .map_err(|_| RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?;
        #[cfg(not(test))]
        let live_port = assembly
            .export_live_conversation_port_descriptor_v1(
                owner_handle,
                &broker_claim.handle,
                active.fabric_generation,
            )
            .await
            .map_err(|_| RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?;
        if !self
            .handle_broker
            .retains_publication_claim(&broker_claim, receipt.canonical_wire())
            .map_err(|_| RuntimeAgentConversationPortExportErrorV1::InternalInvariant)?
        {
            return Err(RuntimeAgentConversationPortExportErrorV1::OwnerUnavailable);
        }
        if live_port.physical_binding_census != 2 {
            return Err(RuntimeAgentConversationPortExportErrorV1::InternalInvariant);
        }
        let fabric_execution_digest = active
            .request
            .target_execution()
            .fabric()
            .execution_digest();
        Ok(RuntimeAgentCurrentConversationPortExportV2 {
            active_request: active.request.clone(),
            active_terminal_receipt: receipt.clone(),
            live_port: RuntimeAgentConversationPortExportV1 {
                active_pxst_digest: receipt.receipt_digest(),
                descriptor_wire: live_port.descriptor_wire,
                fabric_generation: active.fabric_generation,
                agent_generation: active.agent_generation,
                fabric_execution_digest,
                fabric_session_epoch: live_port.fabric_session_epoch,
                descriptor_digest: live_port.descriptor_digest,
                request_binding_descriptor_digest: live_port.request_binding_descriptor_digest,
                event_binding_descriptor_digest: live_port.event_binding_descriptor_digest,
                submit_binding_epoch: live_port.submit_binding_epoch,
                control_binding_epoch: live_port.control_binding_epoch,
                physical_binding_census: live_port.physical_binding_census,
            },
        })
    }

    pub(crate) async fn cutover(
        fabric: &mut ManagedFabricRuntimeCore,
        config: ManagedAgentStackOwnerConfig,
        request: ManagedAgentStackApplyRequestV1,
        verified: VerifiedManagedAgentStackApplyIngressV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<(Self, ManagedAgentStackApplyOutcome), ManagedAgentStackRuntimeError> {
        if fabric.managed_agent_stack_projection_digest().is_some()
            || request.target_execution().mode() != ManagedAgentStackTargetModeV1::FabricAndAgent
            || request.target_execution().projection() != &config.projection
            || request.target() != config.projection.target()
            || request.expected_runtime_store_instance_id() != fabric.store_instance_id()
            || response_channel.target() != request.target()
        {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        fabric.require_remote_agent_access_s0_mutation_unfrozen_v2()?;
        let provider = request
            .target_execution()
            .agent()
            .ok_or(ManagedAgentStackRuntimeError::RequestRejected)?
            .provider();
        observe_deadline(config.clock, verified)?;
        let predecessor = fabric.stack_cutover_observation().await?;
        validate_cutover_cas_and_fabric(&request, &predecessor)?;
        let agent_generation = ManagedServiceGeneration::try_new(1)
            .map_err(|_| ManagedAgentStackRuntimeError::GenerationExhausted)?;
        let transition = initial_intent_transition(
            &request,
            verified,
            response_channel,
            predecessor.generation,
            agent_generation,
        )?;
        let projection_digest = stack_projection_digest(&config.projection)?;
        let snapshot = ManagedAgentStackSnapshot::try_initial(
            fabric.store_instance_id(),
            fabric.owner_target_fingerprint(),
            projection_digest,
            config.runtime_host_epoch,
            transition,
            &config.projection,
        )?;
        // The store returns only after exact marker readback. Any non-Absent
        // PXRS v2 result then exits through `?`, retaining that marker while
        // keeping provider resolution and every Agent binding at zero.
        fabric.initialize_managed_agent_stack(projection_digest, snapshot.canonical_wire())?;
        fabric.adjudicate_remote_agent_access_after_first_stack_marker_v2()?;
        let prepared_provider =
            prepare_agent_provider(provider, config.provider_resolver.as_ref())?;
        let mut core = Self {
            snapshot,
            projection: config.projection,
            state_directory: config.state_directory,
            runtime_host_epoch: config.runtime_host_epoch,
            clock: config.clock,
            response_key_ref: config.response_key_ref,
            response_signer: config.response_signer,
            assembly: None,
            handle: None,
            handle_broker: config.handle_broker,
            provider_resolver: config.provider_resolver,
            recovery_completed: true,
        };
        let started = core
            .start_agent(
                predecessor.control,
                request.target_execution(),
                prepared_provider,
            )
            .await;
        if let Err(error) = started {
            let receipt = core
                .quarantine_activation(fabric, &request, response_channel, 30, &error)
                .await?;
            return Ok((core, ManagedAgentStackApplyOutcome::Committed(receipt)));
        }
        let mut ready = core.snapshot.transition();
        ready.phase = ManagedAgentStackDurablePhase::ActiveReady;
        ready.active = Some(ManagedAgentStackDurableActive {
            fabric_generation: predecessor.generation,
            agent_generation,
            response_channel,
            request: request.clone(),
        });
        ready.pending = None;
        ready.physical_binding_census = 2;
        ready.census_complete = true;
        ready.fabric_ready = true;
        ready.agent_ready = true;
        ready.dependency_satisfied = true;
        ready.quarantine_reason = None;
        let receipt = core.build_terminal(
            &request,
            response_channel,
            TerminalSelection {
                outcome: ManagedAgentStackTerminalOutcomeV1::ActiveReady,
                lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                head: ManagedAgentStackTerminalHeadV1::CommittedIncoming,
                fabric_generation: Some(predecessor.generation),
                agent_generation: Some(agent_generation),
                physical_binding_census: 2,
                census_complete: true,
                fabric_ready: true,
                agent_ready: true,
                dependency_satisfied: true,
                exact_zero: false,
                quarantined: false,
                raw_code: 1,
                raw_context: None,
            },
        )?;
        insert_terminal(&mut ready.terminals, &request, receipt.clone())?;
        if let Err(error) = core.commit_transition(fabric, ready) {
            let _ = core.shutdown_agent().await;
            return Err(error);
        }
        core.publish_handle(&receipt)?;
        Ok((core, ManagedAgentStackApplyOutcome::Committed(receipt)))
    }

    pub(crate) async fn recover(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        if self.recovery_completed || self.assembly.is_some() || self.handle.is_some() {
            return if self.recovery_completed {
                Ok(())
            } else {
                Err(ManagedAgentStackRuntimeError::RecoveryWhileLive)
            };
        }
        self.handle_broker.revoke()?;
        match self.snapshot.phase {
            ManagedAgentStackDurablePhase::ExactZero => {
                if self.snapshot.runtime_host_epoch() != self.runtime_host_epoch {
                    self.commit_transition(fabric, self.snapshot.transition())?;
                }
                self.recovery_completed = true;
                return Ok(());
            }
            ManagedAgentStackDurablePhase::AgentRetireIntent
            | ManagedAgentStackDurablePhase::FabricStopIntent => {
                return self.recover_deactivation(fabric).await;
            }
            ManagedAgentStackDurablePhase::Quarantined
            | ManagedAgentStackDurablePhase::Uncertain => {
                return Err(ManagedAgentStackRuntimeError::RecoveryQuarantined);
            }
            ManagedAgentStackDurablePhase::AgentStartIntent
            | ManagedAgentStackDurablePhase::ActiveReady
            | ManagedAgentStackDurablePhase::RecoveryIntent => {}
        }
        let (request, response_channel) = match self.snapshot.phase {
            ManagedAgentStackDurablePhase::ActiveReady => {
                let active = self
                    .snapshot
                    .active
                    .as_ref()
                    .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
                (active.request.clone(), active.response_channel)
            }
            _ => {
                let pending = self
                    .snapshot
                    .pending
                    .as_ref()
                    .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
                (pending.request.clone(), pending.response_channel)
            }
        };
        let provider = request
            .target_execution()
            .agent()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?
            .provider();
        let prepared_provider = prepare_agent_provider(provider, self.provider_resolver.as_ref())?;
        let predecessor = fabric.stack_cutover_observation().await?;
        if predecessor.execution != *request.target_execution().fabric() {
            return Err(ManagedAgentStackRuntimeError::FabricChangeRequiresEmpty);
        }
        let agent_generation = next_generation(self.snapshot.agent_generation_high_water)?;
        let reading = self.clock.reading()?;
        let deadline_nanos = recovery_deadline(&request, reading)?;
        let mut intent = self.snapshot.transition();
        intent.fabric_generation_high_water = intent
            .fabric_generation_high_water
            .max(predecessor.generation.value());
        intent.agent_generation_high_water = agent_generation.value();
        intent.phase = ManagedAgentStackDurablePhase::RecoveryIntent;
        intent.pending = Some(ManagedAgentStackDurablePending {
            kind: ManagedAgentStackPendingKind::RecoverActive,
            fabric_generation: Some(predecessor.generation),
            agent_generation: Some(agent_generation),
            admitted_clock_generation: reading.generation(),
            admitted_at_nanos: reading.now().value(),
            deadline_nanos,
            response_channel,
            request: request.clone(),
        });
        intent.physical_binding_census = 0;
        intent.census_complete = true;
        intent.fabric_ready = true;
        intent.agent_ready = false;
        intent.dependency_satisfied = true;
        intent.quarantine_reason = None;
        self.commit_transition(fabric, intent)?;
        if let Err(error) = self
            .start_agent(
                predecessor.control,
                request.target_execution(),
                prepared_provider,
            )
            .await
        {
            let _ = self
                .quarantine_activation(fabric, &request, response_channel, 40, &error)
                .await?;
            return Err(ManagedAgentStackRuntimeError::RecoveryQuarantined);
        }
        let mut ready = self.snapshot.transition();
        ready.phase = ManagedAgentStackDurablePhase::ActiveReady;
        ready.active = Some(ManagedAgentStackDurableActive {
            fabric_generation: predecessor.generation,
            agent_generation,
            response_channel,
            request: request.clone(),
        });
        ready.pending = None;
        ready.physical_binding_census = 2;
        ready.census_complete = true;
        ready.fabric_ready = true;
        ready.agent_ready = true;
        ready.dependency_satisfied = true;
        ready.quarantine_reason = None;
        if self.lookup_terminal(&request, response_channel)?.is_none() {
            let receipt = self.build_terminal(
                &request,
                response_channel,
                TerminalSelection {
                    outcome: ManagedAgentStackTerminalOutcomeV1::ActiveReady,
                    lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                    head: ManagedAgentStackTerminalHeadV1::CommittedIncoming,
                    fabric_generation: Some(predecessor.generation),
                    agent_generation: Some(agent_generation),
                    physical_binding_census: 2,
                    census_complete: true,
                    fabric_ready: true,
                    agent_ready: true,
                    dependency_satisfied: true,
                    exact_zero: false,
                    quarantined: false,
                    raw_code: 41,
                    raw_context: None,
                },
            )?;
            insert_terminal(&mut ready.terminals, &request, receipt)?;
        }
        if let Err(error) = self.commit_transition(fabric, ready) {
            let _ = self.shutdown_agent().await;
            return Err(error);
        }
        let receipt = self.active_terminal_receipt()?;
        self.publish_handle(&receipt)?;
        self.recovery_completed = true;
        Ok(())
    }

    async fn recover_deactivation(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        let pending = self
            .snapshot
            .pending
            .clone()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        if pending.kind != ManagedAgentStackPendingKind::DeactivateStack
            || pending.request.target_execution().mode()
                != ManagedAgentStackTargetModeV1::EmptyDeactivate
        {
            return Err(ManagedAgentStackRuntimeError::InvalidDurableState);
        }
        let mut exact_zero = self.snapshot.transition();
        exact_zero.phase = ManagedAgentStackDurablePhase::ExactZero;
        exact_zero.active = None;
        exact_zero.pending = None;
        exact_zero.physical_binding_census = 0;
        exact_zero.census_complete = true;
        exact_zero.fabric_ready = false;
        exact_zero.agent_ready = false;
        exact_zero.dependency_satisfied = false;
        exact_zero.quarantine_reason = None;
        if self
            .lookup_terminal(&pending.request, pending.response_channel)?
            .is_none()
        {
            let receipt = self.build_terminal(
                &pending.request,
                pending.response_channel,
                TerminalSelection {
                    outcome: ManagedAgentStackTerminalOutcomeV1::EmptyExactZero,
                    lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                    head: ManagedAgentStackTerminalHeadV1::CommittedIncoming,
                    fabric_generation: None,
                    agent_generation: None,
                    physical_binding_census: 0,
                    census_complete: true,
                    fabric_ready: false,
                    agent_ready: false,
                    dependency_satisfied: false,
                    exact_zero: true,
                    quarantined: false,
                    raw_code: 42,
                    raw_context: None,
                },
            )?;
            insert_terminal(&mut exact_zero.terminals, &pending.request, receipt)?;
        }
        self.commit_transition(fabric, exact_zero)?;
        self.recovery_completed = true;
        Ok(())
    }

    pub(crate) fn authenticated_terminal_replay(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<Option<ManagedAgentStackApplyOutcome>, ManagedAgentStackRuntimeError> {
        self.validate_request(request, response_channel)?;
        let Some(record) = self.terminal_record(request)? else {
            return Ok(None);
        };
        let receipt = record.receipt.clone();
        let completion_runtime_host_epoch = receipt
            .facts()
            .evidence()
            .fields()
            .completion_runtime_host_epoch;
        if completion_runtime_host_epoch == self.runtime_host_epoch {
            let receipt = self
                .lookup_terminal(request, response_channel)?
                .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
            return Ok(Some(ManagedAgentStackApplyOutcome::Replayed(receipt)));
        }
        let verified = RuntimeVerifiedHistoricalManagedAgentStackReceiptV1::try_verify(
            request,
            self.runtime_host_epoch,
            receipt,
            |key, algorithm, version, transcript, signature| {
                if key != self.response_key_ref
                    || algorithm.value() != ED25519_ALGORITHM
                    || version != ED25519_ALGORITHM_VERSION
                    || signature.len() != 64
                {
                    return false;
                }
                let Ok(signature) = Signature::from_slice(signature) else {
                    return false;
                };
                self.response_signer
                    .verifying_key()
                    .verify_strict(transcript, &signature)
                    .is_ok()
            },
        )
        .map_err(|_| ManagedAgentStackRuntimeError::TerminalCorrelation)?;
        Ok(Some(ManagedAgentStackApplyOutcome::HistoricalReplayed(
            verified,
        )))
    }

    pub(crate) async fn apply(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
        request: ManagedAgentStackApplyRequestV1,
        verified: VerifiedManagedAgentStackApplyIngressV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<ManagedAgentStackApplyOutcome, ManagedAgentStackRuntimeError> {
        if !self.recovery_completed {
            return Err(ManagedAgentStackRuntimeError::RecoveryNotCompleted);
        }
        self.validate_request(&request, response_channel)?;
        if let Some(receipt) = self.lookup_terminal(&request, response_channel)? {
            return Ok(ManagedAgentStackApplyOutcome::Replayed(receipt));
        }
        fabric.require_remote_agent_access_s0_mutation_unfrozen_v2()?;
        if !matches!(
            self.snapshot.phase,
            ManagedAgentStackDurablePhase::ActiveReady | ManagedAgentStackDurablePhase::ExactZero
        ) {
            return Err(ManagedAgentStackRuntimeError::RecoveryRequired);
        }
        if observe_deadline(self.clock, verified).is_err() {
            return self
                .terminalize_no_effect(fabric, request, response_channel, 10)
                .map(ManagedAgentStackApplyOutcome::Committed);
        }
        let mut transition = match self.admit_transition(&request, verified) {
            Ok(transition) => transition,
            Err(
                ManagedAgentStackRuntimeError::ExpectedActiveMismatch
                | ManagedAgentStackRuntimeError::StaleWriter
                | ManagedAgentStackRuntimeError::StaleRevision,
            ) => {
                return self
                    .terminalize_no_effect(fabric, request, response_channel, 11)
                    .map(ManagedAgentStackApplyOutcome::Committed);
            }
            Err(error) => return Err(error),
        };
        match request.target_execution().mode() {
            ManagedAgentStackTargetModeV1::FabricAndAgent => self
                .terminalize_no_effect(fabric, request, response_channel, 12)
                .map(ManagedAgentStackApplyOutcome::Committed),
            ManagedAgentStackTargetModeV1::EmptyDeactivate => {
                self.apply_empty(fabric, request, verified, response_channel, &mut transition)
                    .await
            }
        }
    }

    async fn apply_empty(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
        request: ManagedAgentStackApplyRequestV1,
        verified: VerifiedManagedAgentStackApplyIngressV1,
        response_channel: ReferenceChannelBindingV1,
        transition: &mut ManagedAgentStackSnapshotTransition,
    ) -> Result<ManagedAgentStackApplyOutcome, ManagedAgentStackRuntimeError> {
        if self.snapshot.active.is_none() {
            return self
                .terminalize_no_effect(fabric, request, response_channel, 20)
                .map(ManagedAgentStackApplyOutcome::Committed);
        }
        transition.phase = ManagedAgentStackDurablePhase::AgentRetireIntent;
        transition.pending = Some(ManagedAgentStackDurablePending {
            kind: ManagedAgentStackPendingKind::DeactivateStack,
            fabric_generation: self
                .snapshot
                .active
                .as_ref()
                .map(|active| active.fabric_generation),
            agent_generation: self
                .snapshot
                .active
                .as_ref()
                .map(|active| active.agent_generation),
            admitted_clock_generation: verified.clock_generation(),
            admitted_at_nanos: verified.admitted_at_nanos(),
            deadline_nanos: verified.deadline_nanos(),
            response_channel,
            request: request.clone(),
        });
        transition.quarantine_reason = None;
        self.commit_transition(fabric, transition.clone())?;
        self.handle_broker.revoke()?;
        self.handle = None;
        if let Err(error) = self.shutdown_agent().await {
            let reason = quarantine_reason_digest(50, &request, Some(&error))?;
            let mut uncertain = self.snapshot.transition();
            uncertain.phase = ManagedAgentStackDurablePhase::Uncertain;
            uncertain.census_complete = false;
            uncertain.agent_ready = false;
            uncertain.dependency_satisfied = false;
            uncertain.quarantine_reason = Some(reason);
            self.commit_transition(fabric, uncertain)?;
            return Err(ManagedAgentStackRuntimeError::Agent(error));
        }
        let mut stop_intent = self.snapshot.transition();
        stop_intent.phase = ManagedAgentStackDurablePhase::FabricStopIntent;
        stop_intent.physical_binding_census = 0;
        stop_intent.census_complete = true;
        stop_intent.agent_ready = false;
        stop_intent.dependency_satisfied = false;
        self.commit_transition(fabric, stop_intent)?;
        if !fabric.stop_live_for_stack().await? {
            let reason = quarantine_reason_digest(51, &request, None)?;
            let mut uncertain = self.snapshot.transition();
            uncertain.phase = ManagedAgentStackDurablePhase::Uncertain;
            uncertain.census_complete = false;
            uncertain.fabric_ready = false;
            uncertain.quarantine_reason = Some(reason);
            self.commit_transition(fabric, uncertain)?;
            return Err(ManagedAgentStackRuntimeError::ShutdownUncertain);
        }
        let mut exact_zero = self.snapshot.transition();
        exact_zero.phase = ManagedAgentStackDurablePhase::ExactZero;
        exact_zero.active = None;
        exact_zero.pending = None;
        exact_zero.physical_binding_census = 0;
        exact_zero.census_complete = true;
        exact_zero.fabric_ready = false;
        exact_zero.agent_ready = false;
        exact_zero.dependency_satisfied = false;
        exact_zero.quarantine_reason = None;
        let receipt = self.build_terminal(
            &request,
            response_channel,
            TerminalSelection {
                outcome: ManagedAgentStackTerminalOutcomeV1::EmptyExactZero,
                lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                head: ManagedAgentStackTerminalHeadV1::CommittedIncoming,
                fabric_generation: None,
                agent_generation: None,
                physical_binding_census: 0,
                census_complete: true,
                fabric_ready: false,
                agent_ready: false,
                dependency_satisfied: false,
                exact_zero: true,
                quarantined: false,
                raw_code: 2,
                raw_context: None,
            },
        )?;
        insert_terminal(&mut exact_zero.terminals, &request, receipt.clone())?;
        self.commit_transition(fabric, exact_zero)?;
        Ok(ManagedAgentStackApplyOutcome::Committed(receipt))
    }

    fn terminalize_no_effect(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
        request: ManagedAgentStackApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        raw_code: u16,
    ) -> Result<ManagedAgentStackTerminalReceiptV1, ManagedAgentStackRuntimeError> {
        let active = self.snapshot.active.as_ref();
        let (
            head,
            fabric_generation,
            agent_generation,
            physical_binding_census,
            fabric_ready,
            agent_ready,
            dependency_satisfied,
            exact_zero,
        ) = active.map_or(
            (
                ManagedAgentStackTerminalHeadV1::PreservedNone,
                None,
                None,
                0,
                false,
                false,
                false,
                true,
            ),
            |active| {
                (
                    ManagedAgentStackTerminalHeadV1::PreservedExisting(
                        active.request.target_slice_digest(),
                    ),
                    Some(active.fabric_generation),
                    Some(active.agent_generation),
                    2,
                    true,
                    true,
                    true,
                    false,
                )
            },
        );
        let receipt = self.build_terminal(
            &request,
            response_channel,
            TerminalSelection {
                outcome: ManagedAgentStackTerminalOutcomeV1::NoEffectRejected,
                lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1::ProvenNotStarted,
                head,
                fabric_generation,
                agent_generation,
                physical_binding_census,
                census_complete: true,
                fabric_ready,
                agent_ready,
                dependency_satisfied,
                exact_zero,
                quarantined: false,
                raw_code,
                raw_context: None,
            },
        )?;
        let mut transition = self.snapshot.transition();
        insert_terminal(&mut transition.terminals, &request, receipt.clone())?;
        self.commit_transition(fabric, transition)?;
        Ok(receipt)
    }

    async fn quarantine_activation(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
        request: &ManagedAgentStackApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        raw_code: u16,
        error: &ManagedAgentAssemblyError,
    ) -> Result<ManagedAgentStackTerminalReceiptV1, ManagedAgentStackRuntimeError> {
        self.handle_broker.revoke()?;
        self.handle = None;
        let reason = quarantine_reason_digest(raw_code, request, Some(error))?;
        let pending = self
            .snapshot
            .pending
            .as_ref()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        let fabric_generation = pending
            .fabric_generation
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        let agent_generation = pending
            .agent_generation
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        let receipt = self.build_terminal(
            request,
            response_channel,
            TerminalSelection {
                outcome: ManagedAgentStackTerminalOutcomeV1::Quarantined,
                lifecycle_effect: ManagedAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                head: ManagedAgentStackTerminalHeadV1::CommittedIncoming,
                fabric_generation: Some(fabric_generation),
                agent_generation: Some(agent_generation),
                physical_binding_census: 0,
                census_complete: false,
                fabric_ready: true,
                agent_ready: false,
                dependency_satisfied: false,
                exact_zero: false,
                quarantined: true,
                raw_code,
                raw_context: Some(reason),
            },
        )?;
        let mut quarantined = self.snapshot.transition();
        quarantined.phase = ManagedAgentStackDurablePhase::Quarantined;
        quarantined.active = Some(ManagedAgentStackDurableActive {
            fabric_generation,
            agent_generation,
            response_channel,
            request: request.clone(),
        });
        quarantined.pending = None;
        quarantined.physical_binding_census = 0;
        quarantined.census_complete = false;
        quarantined.fabric_ready = true;
        quarantined.agent_ready = false;
        quarantined.dependency_satisfied = false;
        quarantined.quarantine_reason = Some(reason);
        insert_terminal(&mut quarantined.terminals, request, receipt.clone())?;
        self.commit_transition(fabric, quarantined)?;
        Ok(receipt)
    }

    async fn start_agent(
        &mut self,
        fabric: ManagedFabricControlHandle,
        execution: &paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentStackTargetExecutionV1,
        provider: RuntimeResolvedAgentProviderV1,
    ) -> Result<(), ManagedAgentAssemblyError> {
        let config = ManagedAgentAssemblyConfig::try_from_execution(
            execution,
            self.state_directory.clone(),
        )?;
        let (assembly, handle) =
            ManagedAgentAssembly::start_resolved_provider(fabric, config, provider).await?;
        self.assembly = Some(assembly);
        self.handle = Some(handle);
        Ok(())
    }

    async fn shutdown_agent(&mut self) -> Result<(), ManagedAgentAssemblyError> {
        let Some(mut assembly) = self.assembly.take() else {
            return Ok(());
        };
        if let Err(error) = assembly.shutdown().await {
            self.assembly = Some(assembly);
            return Err(error);
        }
        Ok(())
    }

    fn publish_handle(
        &self,
        receipt: &ManagedAgentStackTerminalReceiptV1,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        let handle = self
            .handle
            .as_ref()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        self.handle_broker.publish(handle.clone(), receipt)
    }

    fn active_terminal_receipt(
        &self,
    ) -> Result<ManagedAgentStackTerminalReceiptV1, ManagedAgentStackRuntimeError> {
        let active = self
            .snapshot
            .active
            .as_ref()
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)?;
        self.lookup_terminal(&active.request, active.response_channel)?
            .ok_or(ManagedAgentStackRuntimeError::InvalidDurableState)
    }

    fn validate_request(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        request
            .validate_expected_store(self.snapshot.store_instance_id())
            .map_err(|_| ManagedAgentStackRuntimeError::RequestRejected)?;
        request
            .validate_projection(&self.projection)
            .map_err(|_| ManagedAgentStackRuntimeError::ProjectionMismatch)?;
        if request.target() != self.projection.target()
            || response_channel.target() != request.target()
        {
            return Err(ManagedAgentStackRuntimeError::RequestRejected);
        }
        Ok(())
    }

    fn lookup_terminal(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
    ) -> Result<Option<ManagedAgentStackTerminalReceiptV1>, ManagedAgentStackRuntimeError> {
        let Some(record) = self.terminal_record(request)? else {
            return Ok(None);
        };
        record
            .receipt
            .validate_against_request(request, response_channel)
            .map_err(|_| ManagedAgentStackRuntimeError::TerminalCorrelation)?;
        let signature_bytes = record.receipt.authentication_signature();
        if record.receipt.authentication_key() != self.response_key_ref
            || record.receipt.authentication_algorithm().value() != ED25519_ALGORITHM
            || record.receipt.authentication_algorithm_version() != ED25519_ALGORITHM_VERSION
            || signature_bytes.len() != 64
        {
            return Err(ManagedAgentStackRuntimeError::TerminalCorrelation);
        }
        let signature = Signature::from_slice(signature_bytes)
            .map_err(|_| ManagedAgentStackRuntimeError::TerminalCorrelation)?;
        let transcript = record
            .receipt
            .signing_transcript()
            .map_err(|_| ManagedAgentStackRuntimeError::TerminalCorrelation)?;
        self.response_signer
            .verifying_key()
            .verify_strict(transcript.as_bytes(), &signature)
            .map_err(|_| ManagedAgentStackRuntimeError::TerminalCorrelation)?;
        Ok(Some(record.receipt.clone()))
    }

    fn terminal_record(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
    ) -> Result<Option<&ManagedAgentStackTerminalRecord>, ManagedAgentStackRuntimeError> {
        let source_scope = request.provenance().source_scope();
        let operation_id = request.operation_id();
        let Some(record) = self.snapshot.terminals.iter().find(|record| {
            record.source_scope == source_scope && record.operation_id == operation_id
        }) else {
            return Ok(None);
        };
        if record.request_digest != request.envelope_request_digest() {
            return Err(ManagedAgentStackRuntimeError::OperationConflict);
        }
        Ok(Some(record))
    }

    fn admit_transition(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
        verified: VerifiedManagedAgentStackApplyIngressV1,
    ) -> Result<ManagedAgentStackSnapshotTransition, ManagedAgentStackRuntimeError> {
        self.validate_cas(request)?;
        let control = request.control_commitment().control();
        let writer = control.writer_context();
        let claim = writer.proof().claim();
        let proof_digest = verified.authenticated().proof_envelope_digest();
        let writer_fence = match self.snapshot.writer_fence {
            None => writer_fence(request, proof_digest),
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
                writer_fence(request, proof_digest)
            }
            Some(_) => return Err(ManagedAgentStackRuntimeError::StaleWriter),
        };
        let provenance = request.provenance();
        let revision_high_water = match self.snapshot.revision_high_water {
            None => revision_high_water(request),
            Some(current)
                if current.source_scope == provenance.source_scope()
                    && (provenance.source_revision().value() > current.revision
                        || provenance.source_revision().value() == current.revision
                            && provenance.source_plan_digest() == current.source_plan_digest) =>
            {
                revision_high_water(request)
            }
            Some(_) => return Err(ManagedAgentStackRuntimeError::StaleRevision),
        };
        let mut transition = self.snapshot.transition();
        transition.writer_fence = Some(writer_fence);
        transition.revision_high_water = Some(revision_high_water);
        insert_replay(
            &mut transition.tenure_nonces,
            ManagedAgentStackReplayRecord {
                identity: verified.authenticated().tenure_nonce_identity(),
                value_digest: proof_digest,
            },
        )?;
        insert_replay(
            &mut transition.request_nonces,
            ManagedAgentStackReplayRecord {
                identity: verified.authenticated().request_nonce_identity(),
                value_digest: request.envelope_request_digest(),
            },
        )?;
        insert_replay(
            &mut transition.temporal_lineages,
            ManagedAgentStackReplayRecord {
                identity: verified.authenticated().temporal_lineage_identity(),
                value_digest: request.envelope_request_digest(),
            },
        )?;
        Ok(transition)
    }

    fn validate_cas(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
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
            _ => Err(ManagedAgentStackRuntimeError::ExpectedActiveMismatch),
        }
    }

    fn build_terminal(
        &self,
        request: &ManagedAgentStackApplyRequestV1,
        response_channel: ReferenceChannelBindingV1,
        selection: TerminalSelection,
    ) -> Result<ManagedAgentStackTerminalReceiptV1, ManagedAgentStackRuntimeError> {
        let reading = self.clock.reading()?;
        let completion_sequence = self
            .snapshot
            .sequence()
            .checked_add(1)
            .ok_or(ManagedAgentStackRuntimeError::SequenceOverflow)?;
        let state = ManagedAgentStackTerminalStateV1::try_new(
            selection.outcome,
            selection.lifecycle_effect,
            selection.head,
            selection.fabric_generation,
            selection.agent_generation,
        )?;
        let evidence = ManagedAgentStackTerminalEvidenceV1::try_new(
            ManagedAgentStackTerminalEvidenceFieldsV1 {
                physical_binding_census: selection.physical_binding_census,
                census_complete: selection.census_complete,
                fabric_ready: selection.fabric_ready,
                agent_ready: selection.agent_ready,
                dependency_satisfied: selection.dependency_satisfied,
                exact_zero: selection.exact_zero,
                quarantined: selection.quarantined,
                resource_census_digest: resource_census_digest(selection)?,
                raw_outcome_digest: raw_outcome_digest(selection, request)?,
                completion_runtime_host_epoch: self.runtime_host_epoch,
                completion_snapshot_sequence: completion_sequence,
                selection_clock_generation: reading.generation(),
                selection_observed_at_nanos: reading.now().value(),
            },
        )?;
        let facts = ManagedAgentStackTerminalFactsV1::try_new(request, state, evidence)?;
        let algorithm = ApplyAuthAlgorithm::try_new(1)
            .map_err(|_| ManagedAgentStackRuntimeError::SignerConfiguration)?;
        let auth_claim = ManagedAgentStackTerminalAuthClaimV1::try_new(
            response_channel,
            self.response_key_ref,
            algorithm,
            1,
        )?;
        let draft = ManagedAgentStackTerminalReceiptDraftV1::try_new(
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
        fabric: &mut ManagedFabricRuntimeCore,
        transition: ManagedAgentStackSnapshotTransition,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        let next = self.snapshot.try_successor_at_epoch(
            self.runtime_host_epoch,
            transition,
            &self.projection,
        )?;
        fabric.commit_managed_agent_stack(next.canonical_wire())?;
        self.snapshot = next;
        Ok(())
    }

    pub(crate) async fn shutdown(
        &mut self,
        fabric: &mut ManagedFabricRuntimeCore,
    ) -> Result<(), ManagedAgentStackRuntimeError> {
        self.recovery_completed = false;
        self.handle_broker.revoke()?;
        self.handle = None;
        self.shutdown_agent().await?;
        if self.snapshot.phase == ManagedAgentStackDurablePhase::ExactZero {
            return Ok(());
        }
        if !fabric.stop_live_for_stack().await? {
            return Err(ManagedAgentStackRuntimeError::ShutdownUncertain);
        }
        Ok(())
    }
}

fn prepare_agent_provider(
    selection: paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentProviderSelectionV1,
    resolver: &dyn RuntimeAgentProviderResolverV1,
) -> Result<RuntimeResolvedAgentProviderV1, ManagedAgentStackRuntimeError> {
    let provider = resolver
        .resolve(selection)
        .map_err(|_| ManagedAgentStackRuntimeError::ProviderResolverUnavailable)?;
    if provider.selection() != selection {
        return Err(ManagedAgentStackRuntimeError::ProviderResolverUnavailable);
    }
    Ok(provider)
}

fn validate_cutover_cas_and_fabric(
    request: &ManagedAgentStackApplyRequestV1,
    predecessor: &ManagedFabricStackCutoverObservation,
) -> Result<(), ManagedAgentStackRuntimeError> {
    if request.target_execution().fabric() != &predecessor.execution
        || request.control_commitment().control().expected_active()
            != ExpectedActive::Exact(predecessor.target_slice_digest)
    {
        return Err(ManagedAgentStackRuntimeError::FabricChangeRequiresEmpty);
    }
    Ok(())
}

fn initial_intent_transition(
    request: &ManagedAgentStackApplyRequestV1,
    verified: VerifiedManagedAgentStackApplyIngressV1,
    response_channel: ReferenceChannelBindingV1,
    fabric_generation: ManagedServiceGeneration,
    agent_generation: ManagedServiceGeneration,
) -> Result<ManagedAgentStackSnapshotTransition, ManagedAgentStackRuntimeError> {
    let proof_digest = verified.authenticated().proof_envelope_digest();
    let mut tenure_nonces = Vec::new();
    let mut request_nonces = Vec::new();
    let mut temporal_lineages = Vec::new();
    insert_replay(
        &mut tenure_nonces,
        ManagedAgentStackReplayRecord {
            identity: verified.authenticated().tenure_nonce_identity(),
            value_digest: proof_digest,
        },
    )?;
    insert_replay(
        &mut request_nonces,
        ManagedAgentStackReplayRecord {
            identity: verified.authenticated().request_nonce_identity(),
            value_digest: request.envelope_request_digest(),
        },
    )?;
    insert_replay(
        &mut temporal_lineages,
        ManagedAgentStackReplayRecord {
            identity: verified.authenticated().temporal_lineage_identity(),
            value_digest: request.envelope_request_digest(),
        },
    )?;
    Ok(ManagedAgentStackSnapshotTransition {
        fabric_generation_high_water: fabric_generation.value(),
        agent_generation_high_water: agent_generation.value(),
        phase: ManagedAgentStackDurablePhase::AgentStartIntent,
        writer_fence: Some(writer_fence(request, proof_digest)),
        revision_high_water: Some(revision_high_water(request)),
        active: None,
        pending: Some(ManagedAgentStackDurablePending {
            kind: ManagedAgentStackPendingKind::ActivateAgent,
            fabric_generation: Some(fabric_generation),
            agent_generation: Some(agent_generation),
            admitted_clock_generation: verified.clock_generation(),
            admitted_at_nanos: verified.admitted_at_nanos(),
            deadline_nanos: verified.deadline_nanos(),
            response_channel,
            request: request.clone(),
        }),
        tenure_nonces,
        request_nonces,
        temporal_lineages,
        terminals: Vec::new(),
        physical_binding_census: 0,
        census_complete: true,
        fabric_ready: true,
        agent_ready: false,
        dependency_satisfied: true,
        quarantine_reason: None,
    })
}

fn writer_fence(
    request: &ManagedAgentStackApplyRequestV1,
    proof_envelope_digest: Digest32,
) -> ManagedAgentStackWriterFence {
    let writer = request.control_commitment().control().writer_context();
    let claim = writer.proof().claim();
    ManagedAgentStackWriterFence {
        source_scope: claim.source_scope(),
        writer: claim.writer(),
        principal: request.authentication().claim().principal(),
        epoch: claim.epoch().value(),
        proof_envelope_digest,
    }
}

fn revision_high_water(
    request: &ManagedAgentStackApplyRequestV1,
) -> ManagedAgentStackRevisionHighWater {
    let provenance = request.provenance();
    ManagedAgentStackRevisionHighWater {
        source_scope: provenance.source_scope(),
        revision: provenance.source_revision().value(),
        source_plan_digest: provenance.source_plan_digest(),
    }
}

fn observe_deadline(
    clock: RuntimeClock,
    verified: VerifiedManagedAgentStackApplyIngressV1,
) -> Result<(), ManagedAgentStackRuntimeError> {
    let reading = clock.reading()?;
    if reading.generation() != verified.clock_generation()
        || reading.now().value() >= verified.deadline_nanos()
    {
        return Err(ManagedAgentStackRuntimeError::DeadlineExpired);
    }
    Ok(())
}

fn recovery_deadline(
    request: &ManagedAgentStackApplyRequestV1,
    reading: ClockReading,
) -> Result<u64, ManagedAgentStackRuntimeError> {
    let remaining = request.temporal().original_budget().value();
    reading
        .now()
        .value()
        .checked_add(remaining)
        .ok_or(ManagedAgentStackRuntimeError::DeadlineOverflow)
}

fn stack_projection_digest(
    projection: &ManagedAgentStackProjectionV1,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(STACK_PROJECTION_DIGEST_DOMAIN)?;
    builder.field_bytes(projection.canonical_wire())?;
    Ok(builder.finish())
}

fn resource_census_digest(selection: TerminalSelection) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(STACK_RESOURCE_CENSUS_DIGEST_DOMAIN)?;
    builder.field_bytes(&selection.physical_binding_census.to_be_bytes())?;
    builder.field_u16(u16::from(selection.census_complete))?;
    builder.field_u16(u16::from(selection.fabric_ready))?;
    builder.field_u16(u16::from(selection.agent_ready))?;
    builder.field_u16(u16::from(selection.dependency_satisfied))?;
    builder.field_u64(
        selection
            .fabric_generation
            .map_or(0, ManagedServiceGeneration::value),
    )?;
    builder.field_u64(
        selection
            .agent_generation
            .map_or(0, ManagedServiceGeneration::value),
    )?;
    Ok(builder.finish())
}

fn raw_outcome_digest(
    selection: TerminalSelection,
    request: &ManagedAgentStackApplyRequestV1,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(STACK_RAW_OUTCOME_DIGEST_DOMAIN)?;
    builder.field_u16(selection.raw_code)?;
    builder.field_u16(u16::from(selection.raw_context.is_some()))?;
    if let Some(context) = selection.raw_context {
        builder.field_digest(&context)?;
    }
    builder.field_digest(&request.envelope_request_digest())?;
    Ok(builder.finish())
}

fn quarantine_reason_digest(
    code: u16,
    request: &ManagedAgentStackApplyRequestV1,
    error: Option<&ManagedAgentAssemblyError>,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(STACK_QUARANTINE_DIGEST_DOMAIN)?;
    builder.field_u16(code)?;
    builder.field_digest(&request.envelope_request_digest())?;
    if let Some(error) = error {
        builder.field_bytes(format!("{error:?}").as_bytes())?;
    }
    Ok(builder.finish())
}

fn next_generation(
    high_water: u64,
) -> Result<ManagedServiceGeneration, ManagedAgentStackRuntimeError> {
    high_water
        .checked_add(1)
        .ok_or(ManagedAgentStackRuntimeError::GenerationExhausted)
        .and_then(|value| {
            ManagedServiceGeneration::try_new(value)
                .map_err(|_| ManagedAgentStackRuntimeError::GenerationExhausted)
        })
}

fn insert_replay(
    records: &mut Vec<ManagedAgentStackReplayRecord>,
    incoming: ManagedAgentStackReplayRecord,
) -> Result<(), ManagedAgentStackRuntimeError> {
    match records.binary_search_by_key(&incoming.identity, |record| record.identity) {
        Ok(index) if records[index].value_digest == incoming.value_digest => Ok(()),
        Ok(_) => Err(ManagedAgentStackRuntimeError::ReplayConflict),
        Err(index) if records.len() < MAX_STACK_REPLAY_RECORDS => {
            records.insert(index, incoming);
            Ok(())
        }
        Err(_) => Err(ManagedAgentStackRuntimeError::ReplayCapacityReached),
    }
}

fn insert_terminal(
    records: &mut Vec<ManagedAgentStackTerminalRecord>,
    request: &ManagedAgentStackApplyRequestV1,
    receipt: ManagedAgentStackTerminalReceiptV1,
) -> Result<(), ManagedAgentStackRuntimeError> {
    let key = (
        *request.provenance().source_scope().as_bytes(),
        *request.operation_id().as_bytes(),
    );
    match records.binary_search_by_key(&key, |record| {
        (
            *record.source_scope.as_bytes(),
            *record.operation_id.as_bytes(),
        )
    }) {
        Ok(index) if records[index].request_digest == request.envelope_request_digest() => Ok(()),
        Ok(_) => Err(ManagedAgentStackRuntimeError::OperationConflict),
        Err(index) if records.len() < MAX_STACK_REPLAY_RECORDS => {
            records.insert(
                index,
                ManagedAgentStackTerminalRecord {
                    source_scope: request.provenance().source_scope(),
                    operation_id: request.operation_id(),
                    request_digest: request.envelope_request_digest(),
                    receipt,
                },
            );
            Ok(())
        }
        Err(_) => Err(ManagedAgentStackRuntimeError::ReplayCapacityReached),
    }
}

#[derive(Debug)]
pub(crate) enum ManagedAgentStackRuntimeError {
    RequestRejected,
    ProjectionMismatch,
    FabricChangeRequiresEmpty,
    ProviderResolverUnavailable,
    RuntimeEpochRegressed,
    RecoveryRequired,
    RecoveryNotCompleted,
    RecoveryWhileLive,
    RecoveryQuarantined,
    DeadlineExpired,
    DeadlineOverflow,
    ExpectedActiveMismatch,
    StaleWriter,
    StaleRevision,
    ReplayConflict,
    ReplayCapacityReached,
    OperationConflict,
    TerminalCorrelation,
    GenerationExhausted,
    SequenceOverflow,
    SignerConfiguration,
    InvalidDurableState,
    HandleBrokerUnavailable,
    ShutdownUncertain,
    Digest(DigestBuildError),
    Contract(ManagedAgentStackPlanError),
    State(ManagedAgentStackStateError),
    Fabric(ManagedFabricRuntimeError),
    Agent(ManagedAgentAssemblyError),
    Clock(RuntimeClockError),
}

impl ManagedAgentStackRuntimeError {
    pub(crate) const fn is_request_unavailable(&self) -> bool {
        matches!(
            self,
            Self::Fabric(ManagedFabricRuntimeError::RemoteAgentAccessSameEpochFrozen)
        )
    }

    pub(crate) const fn is_request_rejection(&self) -> bool {
        matches!(
            self,
            Self::RequestRejected
                | Self::ProjectionMismatch
                | Self::FabricChangeRequiresEmpty
                | Self::ProviderResolverUnavailable
                | Self::DeadlineExpired
                | Self::ExpectedActiveMismatch
                | Self::StaleWriter
                | Self::StaleRevision
                | Self::ReplayConflict
                | Self::OperationConflict
                | Self::TerminalCorrelation
        )
    }
}

impl fmt::Display for ManagedAgentStackRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "managed Agent-stack Runtime failed: {self:?}")
    }
}

impl std::error::Error for ManagedAgentStackRuntimeError {}

impl From<DigestBuildError> for ManagedAgentStackRuntimeError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}

impl From<ManagedAgentStackPlanError> for ManagedAgentStackRuntimeError {
    fn from(value: ManagedAgentStackPlanError) -> Self {
        Self::Contract(value)
    }
}

impl From<ManagedAgentStackStateError> for ManagedAgentStackRuntimeError {
    fn from(value: ManagedAgentStackStateError) -> Self {
        Self::State(value)
    }
}

impl From<ManagedFabricRuntimeError> for ManagedAgentStackRuntimeError {
    fn from(value: ManagedFabricRuntimeError) -> Self {
        Self::Fabric(value)
    }
}

impl From<ManagedAgentAssemblyError> for ManagedAgentStackRuntimeError {
    fn from(value: ManagedAgentAssemblyError) -> Self {
        Self::Agent(value)
    }
}

impl From<RuntimeClockError> for ManagedAgentStackRuntimeError {
    fn from(value: RuntimeClockError) -> Self {
        Self::Clock(value)
    }
}

#[cfg(test)]
mod provider_resolver_tests {
    use super::*;

    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

    use paraegox_agent_contracts::AgentConversationRequestV1;
    use paraegox_agent_service::{
        AgentConversationModelCancellation, AgentConversationModelFuture,
        AgentConversationModelOutcomeV1, AgentConversationModelProvider,
    };
    use paraegox_kernel::identity::{PrincipalRef, RuntimeHostId};
    use paraegox_kernel::time::BoundedDuration;
    use paraegox_runtime_contracts::apply::{
        PlanWriterRef, RuntimeApplyControl, TenureAuthorityRef, TenureKeyRef, TenureProofAlgorithm,
    };
    use paraegox_runtime_contracts::assignment::BindingId;
    use paraegox_runtime_contracts::managed_agent_stack_plan::{
        ManagedAgentIngressLimitsV1, ManagedAgentPortPlanV1, ManagedAgentProviderRefV1,
        ManagedAgentProviderSelectionV1, ManagedAgentSecretRefV1, ManagedAgentSemanticLimitsV1,
        ManagedAgentServicePlanV1, ManagedAgentStackApplyRequestDraftV1,
        ManagedAgentStackTargetExecutionV1,
    };
    use paraegox_runtime_contracts::managed_fabric_plan::{
        ManagedFabricApplyRequestDraftV1, ManagedFabricApplyRequestV1,
        ManagedFabricApplyTerminalOutcomeV1, ManagedFabricListenEndpointV1,
        ManagedFabricTargetExecutionV1,
    };
    use paraegox_runtime_contracts::managed_service::{
        ManagedServiceId, ManagedServiceLifecycleBudgetsV1, ManagedServiceSpecV1,
    };
    use paraegox_runtime_contracts::provenance::SourceScopeRef;
    use paraegox_runtime_contracts::temporal::ApplyTemporalConstraint;
    use tokio::sync::Barrier;

    use crate::admission::{
        AdmissionStateLimits, ApplyAdmissionPolicy, TrustedApplyIdentity, TrustedApplyKey,
        TrustedTenureIdentity, TrustedTenureKey,
    };
    use crate::managed_fabric_runtime::{
        ManagedFabricApplyOutcome, ManagedFabricOwnerConfig, transition_projection_digest,
    };
    use crate::runtime_store::tests::{TestDirectory, managed_fabric_store_fixture};

    const FABRIC_FIXTURE: &str =
        include_str!("../../../tests/fixtures/wire/s7_managed_fabric_successor_v1.json");
    const STORE_BYTE: u8 = 0x44;
    const TARGET_FINGERPRINT_BYTE: u8 = 0x55;
    const REQUEST_SIGNING_SEED: [u8; 32] = [0x22; 32];

    struct ReturnedSelectionResolver(ManagedAgentProviderSelectionV1);

    impl RuntimeAgentProviderResolverV1 for ReturnedSelectionResolver {
        fn resolve(
            &self,
            _selection: ManagedAgentProviderSelectionV1,
        ) -> Result<RuntimeResolvedAgentProviderV1, crate::RuntimeAgentProviderResolveError>
        {
            Ok(RuntimeResolvedAgentProviderV1::new(self.0, TestProvider))
        }
    }

    struct TestProvider;

    impl AgentConversationModelProvider for TestProvider {
        fn complete(
            &mut self,
            _request: AgentConversationRequestV1,
            _cancellation: AgentConversationModelCancellation,
        ) -> AgentConversationModelFuture {
            Box::pin(async { AgentConversationModelOutcomeV1::Failed })
        }
    }

    fn provisioned(byte: u8) -> ManagedAgentProviderSelectionV1 {
        ManagedAgentProviderSelectionV1::try_provisioned(
            ManagedAgentProviderRefV1::try_from_bytes([byte; 16]).expect("test provider reference"),
            Digest32::from_bytes([byte.wrapping_add(1); 32]),
            ManagedAgentSecretRefV1::try_from_bytes([byte.wrapping_add(2); 16])
                .expect("test Secret reference"),
        )
        .expect("test Provisioned selection")
    }

    fn deterministic(byte: u8) -> ManagedAgentProviderSelectionV1 {
        ManagedAgentProviderSelectionV1::try_deterministic_fixture(
            ManagedAgentProviderRefV1::try_from_bytes([byte; 16]).expect("test provider reference"),
            Digest32::from_bytes([byte.wrapping_add(1); 32]),
        )
        .expect("test deterministic selection")
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

    fn fabric_basis_request() -> ManagedFabricApplyRequestV1 {
        let section = "\"one_managed_fabric_service\"";
        let section_start = FABRIC_FIXTURE
            .find(section)
            .unwrap_or_else(|| panic!("missing managed Fabric fixture section"));
        let field = "\"outer_v6_hex\": \"";
        let field_start = FABRIC_FIXTURE[section_start..]
            .find(field)
            .map(|offset| section_start + offset + field.len())
            .unwrap_or_else(|| panic!("missing managed Fabric request fixture"));
        let field_end = FABRIC_FIXTURE[field_start..]
            .find('"')
            .map(|offset| field_start + offset)
            .unwrap_or_else(|| panic!("unterminated managed Fabric request fixture"));
        ManagedFabricApplyRequestV1::decode(&decode_hex(&FABRIC_FIXTURE[field_start..field_end]))
            .unwrap_or_else(|error| panic!("managed Fabric request fixture must decode: {error}"))
    }

    fn available_port() -> u16 {
        TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .unwrap_or_else(|error| panic!("ephemeral loopback bind failed: {error}"))
            .local_addr()
            .unwrap_or_else(|error| panic!("ephemeral loopback address failed: {error}"))
            .port()
    }

    fn long_temporal(basis: &ManagedFabricApplyRequestV1) -> ApplyTemporalConstraint {
        let budget = BoundedDuration::from_nanos(60_000_000_000);
        ApplyTemporalConstraint::try_new(
            basis.temporal().constraint_id(),
            basis.temporal().target_clock_domain(),
            basis.temporal().target_clock_generation(),
            budget,
            budget,
        )
        .unwrap_or_else(|error| panic!("long test temporal constraint rejected: {error}"))
    }

    fn signed_fabric_request(
        basis: &ManagedFabricApplyRequestV1,
        execution: ManagedFabricTargetExecutionV1,
    ) -> ManagedFabricApplyRequestV1 {
        let control = RuntimeApplyControl::new(
            basis
                .control_commitment()
                .control()
                .writer_context()
                .clone(),
            ExpectedActive::None,
            basis.operation_id(),
        );
        let draft = ManagedFabricApplyRequestDraftV1::try_new(
            execution,
            basis.provenance(),
            control,
            long_temporal(basis),
            [STORE_BYTE; 32],
            basis.authentication().claim().clone(),
        )
        .unwrap_or_else(|error| panic!("managed Fabric request draft rejected: {error}"));
        let signature = SigningKey::from_bytes(&REQUEST_SIGNING_SEED)
            .sign(
                draft
                    .signing_transcript()
                    .unwrap_or_else(|error| panic!("Fabric transcript rejected: {error}"))
                    .as_bytes(),
            )
            .to_bytes();
        draft
            .finalize(&signature)
            .unwrap_or_else(|error| panic!("managed Fabric request rejected: {error}"))
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
        .unwrap_or_else(|error| panic!("Agent lifecycle budgets rejected: {error}"));
        let service =
            ManagedServiceSpecV1::new(ManagedServiceId::from_bytes([0xa1; 16]), lifecycle_budgets);
        let semantic_limits = ManagedAgentSemanticLimitsV1::try_new(8, 16, 16, 32)
            .unwrap_or_else(|error| panic!("Agent semantic limits rejected: {error}"));
        let ingress_limits = ManagedAgentIngressLimitsV1::try_new(
            8,
            512 * 1024,
            64 * 1024,
            64 * 1024,
            2_000_000_000,
        )
        .unwrap_or_else(|error| panic!("Agent ingress limits rejected: {error}"));
        let port = ManagedAgentPortPlanV1::try_new(
            BindingId::from_bytes([0xa2; 16]),
            BindingId::from_bytes([0xa3; 16]),
            "paraegox/runtime/current-agent-export/submit",
            "paraegox/runtime/current-agent-export/control",
            ingress_limits,
        )
        .unwrap_or_else(|error| panic!("Agent port plan rejected: {error}"));
        let provider = deterministic(0xa4);
        let agent = ManagedAgentServicePlanV1::try_new(service, semantic_limits, port, provider)
            .unwrap_or_else(|error| panic!("Agent service plan rejected: {error}"));
        let projection = ManagedAgentStackProjectionV1::try_from_managed_fabric_projection(
            fabric_execution.projection().clone(),
        )
        .unwrap_or_else(|error| panic!("Agent stack projection rejected: {error}"));
        ManagedAgentStackTargetExecutionV1::try_fabric_and_agent(
            projection,
            fabric_execution,
            agent,
        )
        .unwrap_or_else(|error| panic!("Agent stack execution rejected: {error}"))
    }

    fn signed_stack_request(
        basis: &ManagedFabricApplyRequestV1,
        execution: ManagedAgentStackTargetExecutionV1,
        active_fabric_digest: paraegox_runtime_contracts::provenance::TargetSliceDigest,
    ) -> ManagedAgentStackApplyRequestV1 {
        let control = RuntimeApplyControl::new(
            basis
                .control_commitment()
                .control()
                .writer_context()
                .clone(),
            ExpectedActive::Exact(active_fabric_digest),
            basis.operation_id(),
        );
        let draft = ManagedAgentStackApplyRequestDraftV1::try_new(
            execution,
            basis.provenance(),
            control,
            long_temporal(basis),
            [STORE_BYTE; 32],
            basis.authentication().claim().clone(),
        )
        .unwrap_or_else(|error| panic!("Agent stack request draft rejected: {error}"));
        let signature = SigningKey::from_bytes(&REQUEST_SIGNING_SEED)
            .sign(
                draft
                    .signing_transcript()
                    .unwrap_or_else(|error| panic!("Agent transcript rejected: {error}"))
                    .as_bytes(),
            )
            .to_bytes();
        draft
            .finalize(&signature)
            .unwrap_or_else(|error| panic!("Agent stack request rejected: {error}"))
    }

    fn admission_policy() -> ApplyAdmissionPolicy {
        let tenure_algorithm = TenureProofAlgorithm::try_new(ED25519_ALGORITHM)
            .unwrap_or_else(|error| panic!("tenure algorithm rejected: {error}"));
        let apply_algorithm = ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)
            .unwrap_or_else(|error| panic!("apply algorithm rejected: {error}"));
        let tenure = TrustedTenureKey::try_new(
            TrustedTenureIdentity::new(
                SourceScopeRef::from_bytes([0x01; 16]),
                PrincipalRef::from_bytes([0x06; 16]),
                1_001,
                1_002,
                TenureAuthorityRef::from_bytes([0x07; 16]),
            ),
            TenureKeyRef::from_bytes([0x08; 16]),
            tenure_algorithm,
            ED25519_ALGORITHM_VERSION,
            SigningKey::from_bytes(&[0x11; 32])
                .verifying_key()
                .to_bytes(),
        )
        .unwrap_or_else(|error| panic!("tenure trust rejected: {error}"));
        let apply = TrustedApplyKey::try_new(
            TrustedApplyIdentity::new(
                SourceScopeRef::from_bytes([0x01; 16]),
                RuntimeHostId::from_bytes([0x05; 16]),
                PrincipalRef::from_bytes([0x09; 16]),
                PlanWriterRef::from_bytes([0x09; 16]),
            ),
            ApplyAuthKeyRef::from_bytes([0x0c; 16]),
            apply_algorithm,
            ED25519_ALGORITHM_VERSION,
            SigningKey::from_bytes(&REQUEST_SIGNING_SEED)
                .verifying_key()
                .to_bytes(),
        )
        .unwrap_or_else(|error| panic!("apply trust rejected: {error}"));
        ApplyAdmissionPolicy::try_new(
            BoundedDuration::from_nanos(60_000_000_000),
            AdmissionStateLimits::try_new(4, 4, 4)
                .unwrap_or_else(|error| panic!("admission limits rejected: {error}")),
            [tenure],
            [apply],
        )
        .unwrap_or_else(|error| panic!("admission policy rejected: {error}"))
    }

    fn response_channel(target: RuntimeHostId) -> ReferenceChannelBindingV1 {
        ReferenceChannelBindingV1::try_new(
            target,
            PrincipalRef::from_bytes([0xe1; 16]),
            Digest32::from_bytes([0xe3; 32]),
            Digest32::from_bytes([0xe4; 32]),
        )
        .unwrap_or_else(|error| panic!("response channel rejected: {error}"))
    }

    struct LiveCurrentAgentFixture {
        _directory: TestDirectory,
        fabric: ManagedFabricRuntimeCore,
        stack: ManagedAgentStackRuntimeCore,
        broker: RuntimeAgentHandleBroker,
        request: ManagedAgentStackApplyRequestV1,
        receipt: ManagedAgentStackTerminalReceiptV1,
    }

    impl LiveCurrentAgentFixture {
        async fn shutdown(mut self) {
            self.stack
                .shutdown(&mut self.fabric)
                .await
                .unwrap_or_else(|error| panic!("Agent stack shutdown failed: {error}"));
            self.fabric
                .shutdown()
                .await
                .unwrap_or_else(|error| panic!("managed Fabric shutdown failed: {error}"));
        }
    }

    async fn live_current_agent_fixture() -> LiveCurrentAgentFixture {
        let basis = fabric_basis_request();
        let endpoint =
            ManagedFabricListenEndpointV1::try_new(&format!("tcp/127.0.0.1:{}", available_port()))
                .unwrap_or_else(|error| panic!("ephemeral Fabric endpoint rejected: {error}"));
        let fabric_execution = ManagedFabricTargetExecutionV1::try_one_managed_fabric_service(
            basis.target_execution().projection().clone(),
            basis
                .target_execution()
                .service()
                .unwrap_or_else(|| panic!("fixture Fabric service disappeared")),
            endpoint,
        )
        .unwrap_or_else(|error| panic!("managed Fabric execution rejected: {error}"));
        let fabric_request = signed_fabric_request(&basis, fabric_execution.clone());
        let stack_request = signed_stack_request(
            &basis,
            agent_stack_execution(fabric_execution),
            fabric_request.target_slice_digest(),
        );
        let projection = basis.target_execution().projection().clone();
        let projection_digest = transition_projection_digest(&projection)
            .unwrap_or_else(|error| panic!("Fabric projection digest failed: {error}"));
        let (directory, store) =
            managed_fabric_store_fixture(STORE_BYTE, TARGET_FINGERPRINT_BYTE, projection_digest);
        let clock = RuntimeClock::new(
            fabric_request.temporal().target_clock_domain(),
            fabric_request.temporal().target_clock_generation(),
            1,
        );
        let mut fabric = ManagedFabricRuntimeCore::from_preopened_store(
            store,
            ManagedFabricOwnerConfig {
                state_directory: directory.path().to_path_buf(),
                store_instance_id: [STORE_BYTE; 32],
                owner_target_fingerprint: Digest32::from_bytes([TARGET_FINGERPRINT_BYTE; 32]),
                projection,
                runtime_host_epoch: 1,
                clock,
                response_key_ref: ApplyAuthKeyRef::from_bytes([0xe2; 16]),
                response_signer: SigningKey::from_bytes(&[0x71; 32]),
            },
        )
        .unwrap_or_else(|error| panic!("managed Fabric core open failed: {error}"));
        fabric
            .recover()
            .await
            .unwrap_or_else(|error| panic!("managed Fabric recovery failed: {error}"));
        let policy = admission_policy();
        let fabric_ingress = policy
            .verify_managed_fabric_apply_request(
                &fabric_request,
                fabric
                    .clock_reading()
                    .unwrap_or_else(|error| panic!("Fabric clock read failed: {error}")),
            )
            .unwrap_or_else(|error| panic!("managed Fabric admission failed: {error:?}"));
        let channel = response_channel(fabric_request.target());
        let ManagedFabricApplyOutcome::Committed(fabric_receipt) = fabric
            .apply(fabric_request, fabric_ingress, channel)
            .await
            .unwrap_or_else(|error| panic!("managed Fabric apply failed: {error}"))
        else {
            panic!("first managed Fabric apply must commit")
        };
        assert_eq!(
            fabric_receipt.facts().outcome(),
            ManagedFabricApplyTerminalOutcomeV1::ActiveReady
        );

        let stack_ingress = policy
            .verify_managed_agent_stack_apply_request(
                &stack_request,
                fabric
                    .clock_reading()
                    .unwrap_or_else(|error| panic!("Agent stack clock read failed: {error}")),
            )
            .unwrap_or_else(|error| panic!("Agent stack admission failed: {error:?}"));
        let selection = stack_request
            .target_execution()
            .agent()
            .unwrap_or_else(|| panic!("active Agent plan disappeared"))
            .provider();
        let broker = RuntimeAgentHandleBroker::default();
        let (stack, outcome) = ManagedAgentStackRuntimeCore::cutover(
            &mut fabric,
            ManagedAgentStackOwnerConfig {
                state_directory: directory.path().to_path_buf(),
                projection: stack_request.target_execution().projection().clone(),
                runtime_host_epoch: 1,
                clock,
                response_key_ref: ApplyAuthKeyRef::from_bytes([0xe2; 16]),
                response_signer: SigningKey::from_bytes(&[0x71; 32]),
                handle_broker: broker.clone(),
                provider_resolver: Arc::new(ReturnedSelectionResolver(selection)),
            },
            stack_request.clone(),
            stack_ingress,
            channel,
        )
        .await
        .unwrap_or_else(|error| panic!("Agent stack cutover failed: {error}"));
        let ManagedAgentStackApplyOutcome::Committed(receipt) = outcome else {
            panic!("first Agent stack cutover must commit")
        };
        assert_eq!(
            receipt.facts().state().outcome(),
            ManagedAgentStackTerminalOutcomeV1::ActiveReady
        );
        LiveCurrentAgentFixture {
            _directory: directory,
            fabric,
            stack,
            broker,
            request: stack_request,
            receipt,
        }
    }

    #[test]
    fn deterministic_fixture_resolves_through_the_same_exact_selection_path() {
        let selection = deterministic(0x11);

        let provider = prepare_agent_provider(selection, &ReturnedSelectionResolver(selection))
            .expect("deterministic fixture must resolve");
        assert_eq!(provider.selection(), selection);
    }

    #[test]
    fn deterministic_fixture_fails_closed_when_the_resolver_is_unavailable() {
        let error = prepare_agent_provider(
            deterministic(0x11),
            &crate::runtime_agent_provider::UnavailableRuntimeAgentProviderResolver,
        )
        .expect_err("unavailable resolver must reject the deterministic fixture");

        assert!(matches!(
            error,
            ManagedAgentStackRuntimeError::ProviderResolverUnavailable
        ));
    }

    #[test]
    fn resolver_must_return_the_exact_requested_selection_for_every_profile() {
        for (requested, different) in [
            (deterministic(0x21), deterministic(0x31)),
            (provisioned(0x41), provisioned(0x51)),
        ] {
            let error = prepare_agent_provider(requested, &ReturnedSelectionResolver(different))
                .expect_err("mismatched resolver output must fail");

            assert!(matches!(
                error,
                ManagedAgentStackRuntimeError::ProviderResolverUnavailable
            ));
        }
    }

    #[test]
    fn first_cutover_compile_boundary_static_gate_precedes_effects() {
        // This compile-boundary guard fixes source order around the synchronous
        // `?` gate. A later CurrentFinal tranche still needs an adversarial
        // Runtime/store test for the SameEpoch branch and physical bind census.
        let source = include_str!("managed_agent_stack_runtime.rs");
        let start = source
            .find("    pub(crate) async fn cutover(")
            .expect("missing first-cutover entrypoint");
        let tail = &source[start..];
        let end = tail
            .find("    pub(crate) async fn recover(")
            .expect("missing first-cutover boundary");
        let cutover = &tail[..end];
        let freeze = cutover
            .find("fabric.require_remote_agent_access_s0_mutation_unfrozen_v2()?")
            .expect("missing owner-level S0 freeze gate");
        let marker = cutover
            .find("fabric.initialize_managed_agent_stack(")
            .expect("missing exact marker publication");
        let adjudication = cutover
            .find("fabric.adjudicate_remote_agent_access_after_first_stack_marker_v2()?")
            .expect("missing post-marker PXRS v2 adjudication");
        let resolver = cutover
            .find("prepare_agent_provider(provider, config.provider_resolver.as_ref())?")
            .expect("missing provider resolution");
        let start_agent = cutover.find(".start_agent(").expect("missing Agent start");

        assert!(
            freeze < marker
                && marker < adjudication
                && adjudication < resolver
                && resolver < start_agent
        );
        assert_eq!(
            cutover[..adjudication]
                .match_indices("prepare_agent_provider(")
                .count(),
            0
        );
        assert_eq!(
            cutover[..adjudication]
                .match_indices(".start_agent(")
                .count(),
            0
        );
        assert_eq!(
            cutover[..adjudication]
                .match_indices("publish_handle(")
                .count(),
            0
        );

        let apply = source
            .split_once("    pub(crate) async fn apply(")
            .and_then(|(_, tail)| tail.split_once("    async fn apply_empty("))
            .map(|(apply, _)| apply)
            .expect("missing Agent-stack apply boundary");
        let replay = apply
            .find("self.lookup_terminal(")
            .expect("missing exact replay lookup");
        let gate = apply
            .find("fabric.require_remote_agent_access_s0_mutation_unfrozen_v2()?")
            .expect("missing Agent-stack S0 freeze gate");
        let phase = apply
            .find("self.snapshot.phase")
            .expect("missing phase check");
        let deadline = apply
            .find("observe_deadline(")
            .expect("missing deadline check");
        let admission = apply
            .find("self.admit_transition(")
            .expect("missing admission");
        assert!(replay < gate && gate < phase && phase < deadline && deadline < admission);

        let lookup = source
            .split_once("    fn lookup_terminal(")
            .and_then(|(_, tail)| tail.split_once("    fn terminal_record("))
            .map(|(lookup, _)| lookup)
            .expect("missing Agent-stack terminal lookup boundary");
        for required in [
            ".validate_against_request(request, response_channel)",
            "authentication_key() != self.response_key_ref",
            "authentication_algorithm().value() != ED25519_ALGORITHM",
            "authentication_algorithm_version() != ED25519_ALGORITHM_VERSION",
            "signature_bytes.len() != 64",
            "Signature::from_slice(signature_bytes)",
            ".signing_transcript()",
            ".verify_strict(transcript.as_bytes(), &signature)",
        ] {
            assert!(
                lookup.contains(required),
                "missing Agent terminal check: {required}"
            );
        }
        let authenticated_replay = source
            .split_once("    pub(crate) fn authenticated_terminal_replay(")
            .and_then(|(_, tail)| tail.split_once("    pub(crate) async fn apply("))
            .map(|(replay, _)| replay)
            .expect("missing authenticated replay boundary");
        let retained = authenticated_replay
            .find("self.terminal_record(request)?")
            .expect("missing retained terminal selection");
        let epoch = authenticated_replay
            .find("completion_runtime_host_epoch")
            .expect("missing completion-epoch classification");
        let current = authenticated_replay
            .find("if completion_runtime_host_epoch == self.runtime_host_epoch")
            .expect("missing current-epoch branch");
        let current_lookup = authenticated_replay
            .find("self.lookup_terminal(request, response_channel)?")
            .expect("current replay bypasses strict terminal lookup");
        let historical = authenticated_replay
            .find("RuntimeVerifiedHistoricalManagedAgentStackReceiptV1::try_verify(")
            .expect("missing historical receipt verification");
        assert!(
            retained < epoch
                && epoch < current
                && current < current_lookup
                && current_lookup < historical
        );
        assert!(authenticated_replay.contains("self.lookup_terminal(request, response_channel)?"));
        assert!(authenticated_replay.contains("|| signature.len() != 64"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn same_epoch_agent_replay_requires_owner_signature_and_survives_s0_freeze() {
        let mut fixture = live_current_agent_fixture().await;
        let channel = response_channel(fixture.request.target());
        let operation_id = fixture.request.operation_id();
        fixture
            .fabric
            .latch_remote_agent_access_s0_mutation_freeze_v2();
        assert!(matches!(
            fixture
                .stack
                .authenticated_terminal_replay(&fixture.request, channel)
                .expect("verified exact replay must remain available"),
            Some(ManagedAgentStackApplyOutcome::Replayed(receipt))
                if receipt.canonical_wire() == fixture.receipt.canonical_wire()
        ));

        let record = fixture
            .stack
            .snapshot
            .terminals
            .iter_mut()
            .find(|record| record.operation_id == operation_id)
            .expect("committed Agent terminal disappeared");
        let mut tampered = record.receipt.canonical_wire().to_vec();
        *tampered
            .last_mut()
            .expect("Agent terminal signature disappeared") ^= 1;
        record.receipt = ManagedAgentStackTerminalReceiptV1::decode(&tampered)
            .expect("opaque bad Agent signature must remain canonical");
        assert!(matches!(
            fixture
                .stack
                .authenticated_terminal_replay(&fixture.request, channel),
            Err(ManagedAgentStackRuntimeError::TerminalCorrelation)
        ));
        fixture.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn broker_revoke_during_live_observation_invalidates_preclaim() {
        let fixture = live_current_agent_fixture().await;
        let observation_completed = Arc::new(Barrier::new(2));
        let broker_mutation_completed = Arc::new(Barrier::new(2));
        let interlock = LiveConversationPortExportTestInterlockV1::new(
            Arc::clone(&observation_completed),
            Arc::clone(&broker_mutation_completed),
        );
        let export = fixture
            .stack
            .export_current_conversation_port_with_interlock_v2(&interlock);
        let revoke = async {
            observation_completed.wait().await;
            fixture
                .broker
                .revoke()
                .unwrap_or_else(|error| panic!("broker revoke failed: {error}"));
            broker_mutation_completed.wait().await;
        };
        let (result, ()) = tokio::join!(export, revoke);
        assert!(matches!(
            result,
            Err(RuntimeAgentConversationPortExportErrorV1::OwnerUnavailable)
        ));
        fixture.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_selector_current_export_is_exact_and_rejects_same_facts_republication() {
        let fixture = live_current_agent_fixture().await;
        let current = fixture
            .stack
            .export_current_conversation_port_v2()
            .await
            .unwrap_or_else(|error| panic!("current Agent export failed: {error:?}"));
        assert_eq!(current.active_request(), &fixture.request);
        assert_eq!(current.active_terminal_receipt(), &fixture.receipt);
        assert_eq!(
            current.live_port().active_pxst_digest,
            fixture.receipt.receipt_digest()
        );
        assert_eq!(current.live_port().physical_binding_census, 2);
        assert!(
            current
                .live_port()
                .descriptor_wire
                .starts_with(b"PXAP\0\x01")
        );

        let observation_completed = Arc::new(Barrier::new(2));
        let broker_mutation_completed = Arc::new(Barrier::new(2));
        let interlock = LiveConversationPortExportTestInterlockV1::new(
            Arc::clone(&observation_completed),
            Arc::clone(&broker_mutation_completed),
        );
        let handle = fixture
            .stack
            .handle
            .as_ref()
            .unwrap_or_else(|| panic!("current Agent handle disappeared"))
            .clone();
        let export = fixture
            .stack
            .export_current_conversation_port_with_interlock_v2(&interlock);
        let replace = async {
            observation_completed.wait().await;
            fixture
                .broker
                .publish(handle, &fixture.receipt)
                .unwrap_or_else(|error| panic!("same-facts broker replacement failed: {error}"));
            broker_mutation_completed.wait().await;
        };
        let (result, ()) = tokio::join!(export, replace);
        assert!(matches!(
            result,
            Err(RuntimeAgentConversationPortExportErrorV1::OwnerUnavailable)
        ));
        fixture.shutdown().await;
    }
}
