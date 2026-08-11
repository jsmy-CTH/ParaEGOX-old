//! Owner-private Deployment producer for the fixed PXTE v8/PXAR v9
//! Fabric/Model/Agent successor.
//!
//! PXAR v6 remains the predecessor authority. The embedded PXTE v6 value is
//! desired structure assembled here; its presence never claims that PXAR v7
//! executed. Provider, Agent, Model, and adapter selections are all explicit.

use core::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use paraegox_kernel::digest::{Digest32, Digest32Builder, DigestBuildError};
use paraegox_kernel::time::BoundedDuration;
use paraegox_runtime_contracts::apply::{ApplyOperationId, ExpectedActive, RuntimeApplyControl};
use paraegox_runtime_contracts::managed_agent_stack_plan::{
    ManagedAgentServicePlanV1, ManagedAgentStackProjectionV1, ManagedAgentStackTargetExecutionV1,
};
use paraegox_runtime_contracts::managed_fabric_plan::{
    ManagedFabricTargetExecutionV1, ManagedFabricTargetModeV1,
};
use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
    ARTIFACT_EXECUTION_BINDING_V1_BYTES, ArtifactBoundManagedModelAgentStackApplyRequestDraftV1,
    ArtifactBoundManagedModelAgentStackApplyRequestV1,
    ArtifactBoundManagedModelAgentStackTargetExecutionV1, ArtifactExecutionBindingV1,
    MAX_ARTIFACT_BOUND_MANAGED_MODEL_AGENT_STACK_TARGET_EXECUTION_BYTES,
    ManagedModelAgentStackApplyRequestDraftV1, ManagedModelAgentStackApplyRequestV1,
    ManagedModelAgentStackPlanError, ManagedModelAgentStackProjectionV1,
    ManagedModelAgentStackTargetExecutionV1, ManagedModelAgentStackTargetModeV1,
    ManagedModelServicePlanV1,
};
use paraegox_runtime_contracts::managed_service::ManagedServiceLifecycleStage;
use paraegox_runtime_contracts::provenance::{
    PlanProvenance, SourcePlanDigest, SourcePlanRevision, TargetSliceDigest,
};
use paraegox_runtime_contracts::temporal::{
    ApplyTemporalConstraint, TemporalConstraintId, TemporalContractError,
};
use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyAuthError, ApplyRequestAuthClaim};

use crate::managed_fabric_producer::{
    ManagedFabricProducerError, VerifiedManagedFabricProducerContextV1,
};

const ED25519_ALGORITHM: u16 = 1;
const ED25519_ALGORITHM_VERSION: u16 = 1;
const ED25519_SIGNATURE_BYTES: usize = 64;
const STACK_DESIRED_DIGEST_DOMAIN: &[u8] =
    b"paraegox.deployment.managed-model-agent-stack-desired.sha256.v1";
const ARTIFACT_PLAN_CONTENT_MAGIC: &[u8; 32] = b"ParaEGOX\0deployment-plan-content";
const ARTIFACT_PLAN_CONTENT_DIGEST_DOMAIN: &[u8] = b"paraegox.deployment.plan-content.sha256.v2";
const ARTIFACT_STACK_DESIRED_DIGEST_DOMAIN: &[u8] =
    b"paraegox.deployment.artifact-bound-managed-model-agent-stack-desired.sha256.v1";
const ARTIFACT_PLAN_CONTENT_PREFIX_BYTES: usize = 252;

/// Exact Artifact-bound PlanContent successor version.
pub(crate) const ARTIFACT_PLAN_CONTENT_VERSION: u16 = 2;
/// Exact Artifact-bound managed Model/Agent shape discriminator.
pub(crate) const ARTIFACT_PLAN_CONTENT_SHAPE: u8 = 3;
/// Maximum canonical Artifact-bound PlanContent bytes.
pub(crate) const MAX_ARTIFACT_PLAN_CONTENT_BYTES: usize = 2_758;

/// Explicit requested sibling stack assembled over active PXAR v6 authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedModelAgentStackActivationV1 {
    expected_fabric: ManagedFabricTargetExecutionV1,
    agent: ManagedAgentServicePlanV1,
    model: ManagedModelServicePlanV1,
}

impl ManagedModelAgentStackActivationV1 {
    pub(crate) fn try_new(
        expected_fabric: ManagedFabricTargetExecutionV1,
        agent: ManagedAgentServicePlanV1,
        model: ManagedModelServicePlanV1,
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if expected_fabric.mode() != ManagedFabricTargetModeV1::OneManagedFabricService {
            return Err(ManagedModelAgentStackProducerError::FabricChangeRequiresEmpty);
        }
        Ok(Self {
            expected_fabric,
            agent,
            model,
        })
    }

    #[must_use]
    pub(crate) const fn expected_fabric(&self) -> &ManagedFabricTargetExecutionV1 {
        &self.expected_fabric
    }

    #[must_use]
    pub(crate) const fn agent(&self) -> &ManagedAgentServicePlanV1 {
        &self.agent
    }

    #[must_use]
    pub(crate) const fn model(&self) -> &ManagedModelServicePlanV1 {
        &self.model
    }
}

/// Fresh request identities consumed only if no PXAR v9 is durable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FreshManagedModelAgentStackApplyV1 {
    operation_id: [u8; 16],
    temporal_constraint_id: [u8; 16],
    authentication_nonce: [u8; 32],
}

impl FreshManagedModelAgentStackApplyV1 {
    pub(crate) fn try_new(
        operation_id: [u8; 16],
        temporal_constraint_id: [u8; 16],
        authentication_nonce: [u8; 32],
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if bytes_are_zero(&operation_id)
            || bytes_are_zero(&temporal_constraint_id)
            || bytes_are_zero(&authentication_nonce)
            || operation_id == temporal_constraint_id
        {
            return Err(ManagedModelAgentStackProducerError::InvalidFreshIdentity);
        }
        Ok(Self {
            operation_id,
            temporal_constraint_id,
            authentication_nonce,
        })
    }
}

/// Digest of exact Artifact-bound PlanContent v2 bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactPlanContentDigestV2(Digest32);

impl ArtifactPlanContentDigestV2 {
    #[must_use]
    pub(crate) const fn value(self) -> Digest32 {
        self.0
    }
}

/// Owner-private PlanContent v2 carrying one exact binding and PXTE v11.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactBoundManagedModelAgentStackPlanContentV2 {
    target: paraegox_kernel::identity::RuntimeHostId,
    binding: ArtifactExecutionBindingV1,
    execution: ArtifactBoundManagedModelAgentStackTargetExecutionV1,
    canonical_bytes: Box<[u8]>,
    digest: ArtifactPlanContentDigestV2,
}

impl ArtifactBoundManagedModelAgentStackPlanContentV2 {
    pub(crate) fn try_new(
        target: paraegox_kernel::identity::RuntimeHostId,
        binding: ArtifactExecutionBindingV1,
        execution: ArtifactBoundManagedModelAgentStackTargetExecutionV1,
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if execution.projection().target() != target || execution.binding() != binding {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let execution_length = u32::try_from(execution.canonical_wire().len())
            .map_err(|_| ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
        let frame_length = ARTIFACT_PLAN_CONTENT_PREFIX_BYTES
            .checked_add(execution.canonical_wire().len())
            .ok_or(ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
        if frame_length > MAX_ARTIFACT_PLAN_CONTENT_BYTES {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let frame_length = u32::try_from(frame_length)
            .map_err(|_| ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
        let mut canonical_bytes = Vec::with_capacity(frame_length as usize);
        canonical_bytes.extend_from_slice(ARTIFACT_PLAN_CONTENT_MAGIC);
        canonical_bytes.extend_from_slice(&ARTIFACT_PLAN_CONTENT_VERSION.to_be_bytes());
        canonical_bytes.push(ARTIFACT_PLAN_CONTENT_SHAPE);
        canonical_bytes.push(0);
        canonical_bytes.extend_from_slice(&frame_length.to_be_bytes());
        canonical_bytes.extend_from_slice(target.as_bytes());
        canonical_bytes.extend_from_slice(binding.canonical_wire());
        canonical_bytes.extend_from_slice(&execution_length.to_be_bytes());
        canonical_bytes.extend_from_slice(execution.canonical_wire());
        let mut digest = Digest32Builder::try_new(ARTIFACT_PLAN_CONTENT_DIGEST_DOMAIN)?;
        digest.field_bytes(&canonical_bytes)?;
        Ok(Self {
            target,
            binding,
            execution,
            canonical_bytes: canonical_bytes.into_boxed_slice(),
            digest: ArtifactPlanContentDigestV2(digest.finish()),
        })
    }

    pub(crate) fn decode(
        expected_target: paraegox_kernel::identity::RuntimeHostId,
        frame: &[u8],
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if frame.len() > MAX_ARTIFACT_PLAN_CONTENT_BYTES {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        if frame.len() < ARTIFACT_PLAN_CONTENT_PREFIX_BYTES
            || frame.get(..32) != Some(ARTIFACT_PLAN_CONTENT_MAGIC.as_slice())
            || read_u16(&frame[32..34]) != ARTIFACT_PLAN_CONTENT_VERSION
            || frame[34] != ARTIFACT_PLAN_CONTENT_SHAPE
            || frame[35] != 0
            || read_u32(&frame[36..40]) as usize != frame.len()
        {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let target = paraegox_kernel::identity::RuntimeHostId::from_bytes(
            frame[40..56]
                .try_into()
                .map_err(|_| ManagedModelAgentStackProducerError::InvalidDesiredPlan)?,
        );
        if target != expected_target {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let binding = ArtifactExecutionBindingV1::decode(
            &frame[56..56 + ARTIFACT_EXECUTION_BINDING_V1_BYTES],
        )?;
        let execution_length = read_u32(&frame[248..252]) as usize;
        if execution_length == 0
            || execution_length
                > MAX_ARTIFACT_BOUND_MANAGED_MODEL_AGENT_STACK_TARGET_EXECUTION_BYTES
            || ARTIFACT_PLAN_CONTENT_PREFIX_BYTES.checked_add(execution_length) != Some(frame.len())
        {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let execution = ArtifactBoundManagedModelAgentStackTargetExecutionV1::decode(
            &frame[ARTIFACT_PLAN_CONTENT_PREFIX_BYTES..],
        )?;
        let decoded = Self::try_new(target, binding, execution)?;
        if decoded.canonical_bytes() != frame {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        Ok(decoded)
    }

    #[must_use]
    pub(crate) const fn target(&self) -> paraegox_kernel::identity::RuntimeHostId {
        self.target
    }

    #[must_use]
    pub(crate) const fn binding(&self) -> ArtifactExecutionBindingV1 {
        self.binding
    }

    #[must_use]
    pub(crate) const fn execution(&self) -> &ArtifactBoundManagedModelAgentStackTargetExecutionV1 {
        &self.execution
    }

    #[must_use]
    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    #[must_use]
    pub(crate) const fn digest(&self) -> ArtifactPlanContentDigestV2 {
        self.digest
    }
}

/// Exact Artifact-bound desired state committed by the external Controller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactBoundManagedModelAgentStackDesiredPlanV1 {
    cutover_marker_digest: Digest32,
    predecessor_slice_digest: TargetSliceDigest,
    deployment_request_digest: Digest32,
    deployment_admission_digest: Digest32,
    revision: SourcePlanRevision,
    provenance: PlanProvenance,
    plan_content: ArtifactBoundManagedModelAgentStackPlanContentV2,
    execution: ArtifactBoundManagedModelAgentStackTargetExecutionV1,
}

/// All immutable authorities needed to derive one Artifact-bound desired state.
pub(crate) struct ArtifactBoundManagedModelAgentStackDesiredInputV1<'a> {
    pub(crate) context: &'a VerifiedManagedFabricProducerContextV1,
    pub(crate) cutover_marker_digest: Digest32,
    pub(crate) predecessor_revision: SourcePlanRevision,
    pub(crate) predecessor_execution: &'a ManagedFabricTargetExecutionV1,
    pub(crate) predecessor_slice_digest: TargetSliceDigest,
    pub(crate) deployment_request_digest: Digest32,
    pub(crate) deployment_admission_digest: Digest32,
    pub(crate) binding: ArtifactExecutionBindingV1,
    pub(crate) activation: &'a ManagedModelAgentStackActivationV1,
}

impl ArtifactBoundManagedModelAgentStackDesiredPlanV1 {
    pub(crate) fn try_activate(
        input: ArtifactBoundManagedModelAgentStackDesiredInputV1<'_>,
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        let ArtifactBoundManagedModelAgentStackDesiredInputV1 {
            context,
            cutover_marker_digest,
            predecessor_revision,
            predecessor_execution,
            predecessor_slice_digest,
            deployment_request_digest,
            deployment_admission_digest,
            binding,
            activation,
        } = input;
        if digest_is_zero(deployment_request_digest) || digest_is_zero(deployment_admission_digest)
        {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let embedded = ManagedModelAgentStackDesiredPlanV1::try_activate(
            context,
            cutover_marker_digest,
            predecessor_revision,
            predecessor_execution,
            predecessor_slice_digest,
            activation,
        )?;
        let revision = embedded.revision();
        let execution = ArtifactBoundManagedModelAgentStackTargetExecutionV1::try_new(
            embedded.execution().projection().clone(),
            binding,
            embedded.execution().clone(),
        )?;
        let plan_content = ArtifactBoundManagedModelAgentStackPlanContentV2::try_new(
            context.target(),
            binding,
            execution.clone(),
        )?;
        let mut digest = Digest32Builder::try_new(ARTIFACT_STACK_DESIRED_DIGEST_DOMAIN)?;
        digest.field_digest(&cutover_marker_digest)?;
        digest.field_bytes(context.target().as_bytes())?;
        digest.field_bytes(context.source_scope().as_bytes())?;
        digest.field_bytes(context.source_plan().as_bytes())?;
        digest.field_u64(revision.value())?;
        digest.field_bytes(predecessor_slice_digest.value().as_bytes())?;
        digest.field_digest(&deployment_request_digest)?;
        digest.field_digest(&deployment_admission_digest)?;
        digest.field_digest(&plan_content.digest().value())?;
        digest.field_bytes(execution.canonical_wire())?;
        let provenance = PlanProvenance::new(
            context.source_scope(),
            context.source_plan(),
            revision,
            SourcePlanDigest::new(digest.finish()),
        );
        Ok(Self {
            cutover_marker_digest,
            predecessor_slice_digest,
            deployment_request_digest,
            deployment_admission_digest,
            revision,
            provenance,
            plan_content,
            execution,
        })
    }

    #[must_use]
    pub(crate) const fn cutover_marker_digest(&self) -> Digest32 {
        self.cutover_marker_digest
    }

    #[must_use]
    pub(crate) const fn predecessor_slice_digest(&self) -> TargetSliceDigest {
        self.predecessor_slice_digest
    }

    #[must_use]
    pub(crate) const fn deployment_request_digest(&self) -> Digest32 {
        self.deployment_request_digest
    }

    #[must_use]
    pub(crate) const fn deployment_admission_digest(&self) -> Digest32 {
        self.deployment_admission_digest
    }

    #[must_use]
    pub(crate) const fn revision(&self) -> SourcePlanRevision {
        self.revision
    }

    #[must_use]
    pub(crate) const fn provenance(&self) -> PlanProvenance {
        self.provenance
    }

    #[must_use]
    pub(crate) const fn plan_content(&self) -> &ArtifactBoundManagedModelAgentStackPlanContentV2 {
        &self.plan_content
    }

    #[must_use]
    pub(crate) const fn execution(&self) -> &ArtifactBoundManagedModelAgentStackTargetExecutionV1 {
        &self.execution
    }
}

/// Exact desired A2 stack derived from the current pin and active PXAR v6.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedModelAgentStackDesiredPlanV1 {
    cutover_marker_digest: Digest32,
    predecessor_slice_digest: TargetSliceDigest,
    revision: SourcePlanRevision,
    provenance: PlanProvenance,
    execution: ManagedModelAgentStackTargetExecutionV1,
}

impl ManagedModelAgentStackDesiredPlanV1 {
    pub(crate) fn try_activate(
        context: &VerifiedManagedFabricProducerContextV1,
        cutover_marker_digest: Digest32,
        predecessor_revision: SourcePlanRevision,
        predecessor_execution: &ManagedFabricTargetExecutionV1,
        predecessor_slice_digest: TargetSliceDigest,
        activation: &ManagedModelAgentStackActivationV1,
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if activation.expected_fabric != *predecessor_execution
            || predecessor_execution.mode() != ManagedFabricTargetModeV1::OneManagedFabricService
            || predecessor_execution.projection() != context.projection()
        {
            return Err(ManagedModelAgentStackProducerError::FabricChangeRequiresEmpty);
        }
        let agent_projection = ManagedAgentStackProjectionV1::try_from_managed_fabric_projection(
            context.projection().clone(),
        )?;
        let embedded = ManagedAgentStackTargetExecutionV1::try_fabric_and_agent(
            agent_projection.clone(),
            predecessor_execution.clone(),
            activation.agent.clone(),
        )?;
        let projection =
            ManagedModelAgentStackProjectionV1::try_from_managed_agent_stack_projection(
                agent_projection,
            )?;
        let execution = ManagedModelAgentStackTargetExecutionV1::try_fabric_model_and_agent(
            projection,
            embedded,
            activation.model,
        )?;
        let revision = predecessor_revision
            .value()
            .checked_add(1)
            .ok_or(ManagedModelAgentStackProducerError::RevisionExhausted)?;
        Self::try_restore(
            context,
            cutover_marker_digest,
            predecessor_slice_digest,
            revision,
            execution.canonical_wire(),
        )
    }

    pub(crate) fn try_restore(
        context: &VerifiedManagedFabricProducerContextV1,
        cutover_marker_digest: Digest32,
        predecessor_slice_digest: TargetSliceDigest,
        revision: u64,
        execution_wire: &[u8],
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if digest_is_zero(cutover_marker_digest)
            || digest_is_zero(*predecessor_slice_digest.value())
            || revision == 0
        {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let execution = ManagedModelAgentStackTargetExecutionV1::decode(execution_wire)?;
        let agent_projection = ManagedAgentStackProjectionV1::try_from_managed_fabric_projection(
            context.projection().clone(),
        )?;
        let projection =
            ManagedModelAgentStackProjectionV1::try_from_managed_agent_stack_projection(
                agent_projection,
            )?;
        if execution.projection() != &projection {
            return Err(ManagedModelAgentStackProducerError::ProjectionMismatch);
        }
        let revision = SourcePlanRevision::new(revision);
        let mut digest = Digest32Builder::try_new(STACK_DESIRED_DIGEST_DOMAIN)?;
        digest.field_digest(&cutover_marker_digest)?;
        digest.field_bytes(context.target().as_bytes())?;
        digest.field_bytes(context.source_scope().as_bytes())?;
        digest.field_bytes(context.source_plan().as_bytes())?;
        digest.field_u64(revision.value())?;
        digest.field_bytes(predecessor_slice_digest.value().as_bytes())?;
        digest.field_bytes(execution.canonical_wire())?;
        let provenance = PlanProvenance::new(
            context.source_scope(),
            context.source_plan(),
            revision,
            SourcePlanDigest::new(digest.finish()),
        );
        Ok(Self {
            cutover_marker_digest,
            predecessor_slice_digest,
            revision,
            provenance,
            execution,
        })
    }

    pub(crate) fn try_empty_deactivate(
        context: &VerifiedManagedFabricProducerContextV1,
        cutover_marker_digest: Digest32,
        active_desired: &Self,
        active_request: &ManagedModelAgentStackApplyRequestV1,
    ) -> Result<Self, ManagedModelAgentStackProducerError> {
        if active_desired.execution().mode()
            != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
            || active_request.target_execution() != active_desired.execution()
            || active_request.provenance() != active_desired.provenance()
        {
            return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
        }
        let agent_projection = ManagedAgentStackProjectionV1::try_from_managed_fabric_projection(
            context.projection().clone(),
        )?;
        let projection =
            ManagedModelAgentStackProjectionV1::try_from_managed_agent_stack_projection(
                agent_projection,
            )?;
        let execution = ManagedModelAgentStackTargetExecutionV1::try_empty_deactivate(projection)?;
        let revision = active_desired
            .revision()
            .value()
            .checked_add(1)
            .ok_or(ManagedModelAgentStackProducerError::RevisionExhausted)?;
        Self::try_restore(
            context,
            cutover_marker_digest,
            active_request.target_slice_digest(),
            revision,
            execution.canonical_wire(),
        )
    }

    #[must_use]
    pub(crate) const fn cutover_marker_digest(&self) -> Digest32 {
        self.cutover_marker_digest
    }

    #[must_use]
    pub(crate) const fn predecessor_slice_digest(&self) -> TargetSliceDigest {
        self.predecessor_slice_digest
    }

    #[must_use]
    pub(crate) const fn revision(&self) -> SourcePlanRevision {
        self.revision
    }

    #[must_use]
    pub(crate) const fn provenance(&self) -> PlanProvenance {
        self.provenance
    }

    #[must_use]
    pub(crate) const fn execution(&self) -> &ManagedModelAgentStackTargetExecutionV1 {
        &self.execution
    }
}

pub(crate) fn produce_managed_model_agent_stack_request_v1(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ManagedModelAgentStackDesiredPlanV1,
    fresh: FreshManagedModelAgentStackApplyV1,
    controller_signer: &SigningKey,
) -> Result<ManagedModelAgentStackApplyRequestV1, ManagedModelAgentStackProducerError> {
    let budget = active_stack_lifecycle_budget(desired)?;
    produce_request_with_budget(context, desired, fresh, controller_signer, budget)
}

pub(crate) fn produce_managed_model_agent_stack_empty_request_v1(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ManagedModelAgentStackDesiredPlanV1,
    archived_active_execution: &ManagedModelAgentStackTargetExecutionV1,
    fresh: FreshManagedModelAgentStackApplyV1,
    controller_signer: &SigningKey,
) -> Result<ManagedModelAgentStackApplyRequestV1, ManagedModelAgentStackProducerError> {
    if desired.execution().mode() != ManagedModelAgentStackTargetModeV1::EmptyDeactivate {
        return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
    }
    let budget = empty_stack_lifecycle_budget(archived_active_execution)?;
    produce_request_with_budget(context, desired, fresh, controller_signer, budget)
}

pub(crate) fn produce_artifact_bound_managed_model_agent_stack_request_v1(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ArtifactBoundManagedModelAgentStackDesiredPlanV1,
    fresh: FreshManagedModelAgentStackApplyV1,
    controller_signer: &SigningKey,
) -> Result<ArtifactBoundManagedModelAgentStackApplyRequestV1, ManagedModelAgentStackProducerError>
{
    let budget = active_execution_lifecycle_budget(desired.execution().embedded())?;
    if controller_signer.verifying_key().to_bytes() != context.controller_verifying_key()
        || desired
            .execution()
            .projection()
            .managed_agent_stack_projection()
            .managed_fabric_projection()
            != context.projection()
        || desired.provenance().source_scope() != context.source_scope()
        || desired.provenance().source_plan() != context.source_plan()
        || desired.plan_content().execution() != desired.execution()
        || desired.plan_content().binding() != desired.execution().binding()
    {
        return Err(ManagedModelAgentStackProducerError::ControllerOrDesiredMismatch);
    }
    let control = RuntimeApplyControl::new(
        context.writer_context().clone(),
        ExpectedActive::Exact(desired.predecessor_slice_digest()),
        ApplyOperationId::from_bytes(fresh.operation_id),
    );
    let temporal = ApplyTemporalConstraint::try_new(
        TemporalConstraintId::from_bytes(fresh.temporal_constraint_id),
        context.clock_domain(),
        context.clock_generation(),
        budget,
        budget,
    )?;
    let claim = ApplyRequestAuthClaim::try_new(
        context.controller_principal(),
        context.request_key(),
        ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)?,
        ED25519_ALGORITHM_VERSION,
        &fresh.authentication_nonce,
    )?;
    let draft = ArtifactBoundManagedModelAgentStackApplyRequestDraftV1::try_new(
        desired.execution().clone(),
        desired.provenance(),
        control,
        temporal,
        context.runtime_store_instance_id(),
        claim,
    )?;
    let signature = controller_signer.sign(draft.signing_transcript()?.as_bytes());
    let request = draft.finalize(&signature.to_bytes())?;
    validate_artifact_bound_managed_model_agent_stack_request_v1(context, desired, &request)?;
    Ok(request)
}

pub(crate) fn validate_artifact_bound_managed_model_agent_stack_request_v1(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ArtifactBoundManagedModelAgentStackDesiredPlanV1,
    request: &ArtifactBoundManagedModelAgentStackApplyRequestV1,
) -> Result<(), ManagedModelAgentStackProducerError> {
    let lifecycle_budget = active_execution_lifecycle_budget(desired.execution().embedded())?;
    let control = request.control_commitment().control();
    let temporal = request.temporal();
    let authentication = request.authentication();
    let claim = authentication.claim();
    if request.target() != context.target()
        || request.target_execution() != desired.execution()
        || request.provenance() != desired.provenance()
        || request.expected_runtime_store_instance_id() != context.runtime_store_instance_id()
        || control.expected_active() != ExpectedActive::Exact(desired.predecessor_slice_digest())
        || control.writer_context() != context.writer_context()
        || temporal.target_clock_domain() != context.clock_domain()
        || temporal.target_clock_generation() != context.clock_generation()
        || temporal.original_budget() != lifecycle_budget
        || temporal.remaining_budget() != lifecycle_budget
        || claim.principal() != context.controller_principal()
        || claim.key() != context.request_key()
        || claim.algorithm().value() != ED25519_ALGORITHM
        || claim.algorithm_version() != ED25519_ALGORITHM_VERSION
        || authentication.signature().len() != ED25519_SIGNATURE_BYTES
    {
        return Err(ManagedModelAgentStackProducerError::RequestMismatch);
    }
    let signature: [u8; ED25519_SIGNATURE_BYTES] = authentication
        .signature()
        .try_into()
        .map_err(|_| ManagedModelAgentStackProducerError::RequestMismatch)?;
    VerifyingKey::from_bytes(&context.controller_verifying_key())
        .map_err(|_| ManagedModelAgentStackProducerError::RequestMismatch)?
        .verify_strict(
            request.signing_transcript()?.as_bytes(),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| ManagedModelAgentStackProducerError::RequestMismatch)
}

fn produce_request_with_budget(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ManagedModelAgentStackDesiredPlanV1,
    fresh: FreshManagedModelAgentStackApplyV1,
    controller_signer: &SigningKey,
    budget: BoundedDuration,
) -> Result<ManagedModelAgentStackApplyRequestV1, ManagedModelAgentStackProducerError> {
    if controller_signer.verifying_key().to_bytes() != context.controller_verifying_key()
        || desired
            .execution()
            .projection()
            .managed_agent_stack_projection()
            .managed_fabric_projection()
            != context.projection()
        || desired.provenance().source_scope() != context.source_scope()
        || desired.provenance().source_plan() != context.source_plan()
    {
        return Err(ManagedModelAgentStackProducerError::ControllerOrDesiredMismatch);
    }
    let control = RuntimeApplyControl::new(
        context.writer_context().clone(),
        ExpectedActive::Exact(desired.predecessor_slice_digest()),
        ApplyOperationId::from_bytes(fresh.operation_id),
    );
    let temporal = ApplyTemporalConstraint::try_new(
        TemporalConstraintId::from_bytes(fresh.temporal_constraint_id),
        context.clock_domain(),
        context.clock_generation(),
        budget,
        budget,
    )?;
    let claim = ApplyRequestAuthClaim::try_new(
        context.controller_principal(),
        context.request_key(),
        ApplyAuthAlgorithm::try_new(ED25519_ALGORITHM)?,
        ED25519_ALGORITHM_VERSION,
        &fresh.authentication_nonce,
    )?;
    let draft = ManagedModelAgentStackApplyRequestDraftV1::try_new(
        desired.execution().clone(),
        desired.provenance(),
        control,
        temporal,
        context.runtime_store_instance_id(),
        claim,
    )?;
    let signature = controller_signer.sign(draft.signing_transcript()?.as_bytes());
    let request = draft.finalize(&signature.to_bytes())?;
    validate_request_with_budget(context, desired, &request, budget)?;
    Ok(request)
}

pub(crate) fn validate_managed_model_agent_stack_request_v1(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ManagedModelAgentStackDesiredPlanV1,
    request: &ManagedModelAgentStackApplyRequestV1,
) -> Result<(), ManagedModelAgentStackProducerError> {
    validate_request_with_budget(
        context,
        desired,
        request,
        active_stack_lifecycle_budget(desired)?,
    )
}

pub(crate) fn validate_managed_model_agent_stack_empty_request_v1(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ManagedModelAgentStackDesiredPlanV1,
    archived_active_execution: &ManagedModelAgentStackTargetExecutionV1,
    request: &ManagedModelAgentStackApplyRequestV1,
) -> Result<(), ManagedModelAgentStackProducerError> {
    if desired.execution().mode() != ManagedModelAgentStackTargetModeV1::EmptyDeactivate {
        return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
    }
    validate_request_with_budget(
        context,
        desired,
        request,
        empty_stack_lifecycle_budget(archived_active_execution)?,
    )
}

fn validate_request_with_budget(
    context: &VerifiedManagedFabricProducerContextV1,
    desired: &ManagedModelAgentStackDesiredPlanV1,
    request: &ManagedModelAgentStackApplyRequestV1,
    lifecycle_budget: BoundedDuration,
) -> Result<(), ManagedModelAgentStackProducerError> {
    let control = request.control_commitment().control();
    let temporal = request.temporal();
    let authentication = request.authentication();
    let claim = authentication.claim();
    if request.target() != context.target()
        || request.target_execution() != desired.execution()
        || request.provenance() != desired.provenance()
        || request.expected_runtime_store_instance_id() != context.runtime_store_instance_id()
        || control.expected_active() != ExpectedActive::Exact(desired.predecessor_slice_digest())
        || control.writer_context() != context.writer_context()
        || temporal.target_clock_domain() != context.clock_domain()
        || temporal.target_clock_generation() != context.clock_generation()
        || temporal.original_budget() != lifecycle_budget
        || temporal.remaining_budget() != lifecycle_budget
        || claim.principal() != context.controller_principal()
        || claim.key() != context.request_key()
        || claim.algorithm().value() != ED25519_ALGORITHM
        || claim.algorithm_version() != ED25519_ALGORITHM_VERSION
        || authentication.signature().len() != ED25519_SIGNATURE_BYTES
    {
        return Err(ManagedModelAgentStackProducerError::RequestMismatch);
    }
    let agent_projection = ManagedAgentStackProjectionV1::try_from_managed_fabric_projection(
        context.projection().clone(),
    )?;
    let projection = ManagedModelAgentStackProjectionV1::try_from_managed_agent_stack_projection(
        agent_projection,
    )?;
    request.validate_projection(&projection)?;
    let signature: [u8; ED25519_SIGNATURE_BYTES] = authentication
        .signature()
        .try_into()
        .map_err(|_| ManagedModelAgentStackProducerError::RequestMismatch)?;
    VerifyingKey::from_bytes(&context.controller_verifying_key())
        .map_err(|_| ManagedModelAgentStackProducerError::RequestMismatch)?
        .verify_strict(
            request.signing_transcript()?.as_bytes(),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| ManagedModelAgentStackProducerError::RequestMismatch)
}

fn active_stack_lifecycle_budget(
    desired: &ManagedModelAgentStackDesiredPlanV1,
) -> Result<BoundedDuration, ManagedModelAgentStackProducerError> {
    active_execution_lifecycle_budget(desired.execution())
}

fn active_execution_lifecycle_budget(
    execution: &ManagedModelAgentStackTargetExecutionV1,
) -> Result<BoundedDuration, ManagedModelAgentStackProducerError> {
    if execution.mode() != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent {
        return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
    }
    let agent = execution
        .managed_agent_stack()
        .agent()
        .ok_or(ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
    let model = execution
        .model()
        .ok_or(ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
    let mut total = 0_u64;
    for service in [model.service(), agent.service()] {
        for stage in [
            ManagedServiceLifecycleStage::Prepare,
            ManagedServiceLifecycleStage::Start,
            ManagedServiceLifecycleStage::Readiness,
        ] {
            total = total
                .checked_add(service.lifecycle_budgets().for_stage(stage).value())
                .ok_or(ManagedModelAgentStackProducerError::LifecycleBudgetOverflow)?;
        }
    }
    bounded_nonzero(total)
}

fn empty_stack_lifecycle_budget(
    archived: &ManagedModelAgentStackTargetExecutionV1,
) -> Result<BoundedDuration, ManagedModelAgentStackProducerError> {
    if archived.mode() != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent {
        return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
    }
    let agent = archived
        .managed_agent_stack()
        .agent()
        .ok_or(ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
    let fabric = archived
        .managed_agent_stack()
        .fabric()
        .service()
        .ok_or(ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
    let model = archived
        .model()
        .ok_or(ManagedModelAgentStackProducerError::InvalidDesiredPlan)?;
    let mut total = 0_u64;
    for service in [agent.service(), model.service(), fabric] {
        for stage in [
            ManagedServiceLifecycleStage::Drain,
            ManagedServiceLifecycleStage::Stop,
        ] {
            total = total
                .checked_add(service.lifecycle_budgets().for_stage(stage).value())
                .ok_or(ManagedModelAgentStackProducerError::LifecycleBudgetOverflow)?;
        }
    }
    bounded_nonzero(total)
}

fn bounded_nonzero(value: u64) -> Result<BoundedDuration, ManagedModelAgentStackProducerError> {
    if value == 0 {
        return Err(ManagedModelAgentStackProducerError::InvalidDesiredPlan);
    }
    Ok(BoundedDuration::from_nanos(value))
}

fn bytes_are_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

fn digest_is_zero(value: Digest32) -> bool {
    bytes_are_zero(value.as_bytes())
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().unwrap_or([0; 2]))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap_or([0; 4]))
}

#[derive(Debug)]
pub(crate) enum ManagedModelAgentStackProducerError {
    Contract,
    Fabric(ManagedFabricProducerError),
    Digest(DigestBuildError),
    Authentication(ApplyAuthError),
    Temporal(TemporalContractError),
    InvalidFreshIdentity,
    FabricChangeRequiresEmpty,
    InvalidDesiredPlan,
    RevisionExhausted,
    LifecycleBudgetOverflow,
    ProjectionMismatch,
    ControllerOrDesiredMismatch,
    RequestMismatch,
}

impl From<ManagedModelAgentStackPlanError> for ManagedModelAgentStackProducerError {
    fn from(_value: ManagedModelAgentStackPlanError) -> Self {
        Self::Contract
    }
}

impl From<paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentStackPlanError>
    for ManagedModelAgentStackProducerError
{
    fn from(
        _value: paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentStackPlanError,
    ) -> Self {
        Self::Contract
    }
}

impl From<ManagedFabricProducerError> for ManagedModelAgentStackProducerError {
    fn from(value: ManagedFabricProducerError) -> Self {
        Self::Fabric(value)
    }
}

impl From<DigestBuildError> for ManagedModelAgentStackProducerError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}

impl From<ApplyAuthError> for ManagedModelAgentStackProducerError {
    fn from(value: ApplyAuthError) -> Self {
        Self::Authentication(value)
    }
}

impl From<TemporalContractError> for ManagedModelAgentStackProducerError {
    fn from(value: TemporalContractError) -> Self {
        Self::Temporal(value)
    }
}

impl fmt::Display for ManagedModelAgentStackProducerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "managed Model+Agent stack producer rejected: {self:?}"
        )
    }
}

impl std::error::Error for ManagedModelAgentStackProducerError {}

#[cfg(test)]
mod tests {
    use paraegox_artifact::{MaterializationReceiptRefV1, VerifiedArtifactPairV1};
    use paraegox_kernel::digest::Digest32;
    use paraegox_kernel::time::BoundedDuration;
    use paraegox_runtime_contracts::assignment::BindingId;
    use paraegox_runtime_contracts::managed_agent_stack_plan::{
        ManagedAgentIngressLimitsV1, ManagedAgentPortPlanV1, ManagedAgentProviderRefV1,
        ManagedAgentProviderSelectionV1, ManagedAgentSemanticLimitsV1, ManagedAgentServicePlanV1,
    };
    use paraegox_runtime_contracts::managed_fabric_plan::{
        ManagedFabricListenEndpointV1, ManagedFabricTargetExecutionV1,
    };
    use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
        ArtifactBoundManagedModelAgentStackApplyRequestV1,
        ArtifactBoundManagedModelAgentStackTargetExecutionV1,
        ManagedModelAdapterBindingV1, ManagedModelAdapterVersionV1,
        ManagedModelAgentStackApplyRequestV1, ManagedModelAgentStackTargetExecutionV1,
        ManagedModelCapabilityIdV1, ManagedModelServicePlanV1,
        artifact_execution_profile_commitment_v1,
    };
    use paraegox_runtime_contracts::managed_service::{
        ManagedServiceId, ManagedServiceLifecycleBudgetsV1, ManagedServiceSpecV1,
    };
    use paraegox_runtime_contracts::provenance::{SourcePlanRevision, TargetSliceDigest};

    use super::{
        ArtifactBoundManagedModelAgentStackDesiredInputV1,
        ArtifactBoundManagedModelAgentStackDesiredPlanV1,
        ArtifactBoundManagedModelAgentStackPlanContentV2, ArtifactExecutionBindingV1,
        FreshManagedModelAgentStackApplyV1, ManagedModelAgentStackActivationV1,
        produce_artifact_bound_managed_model_agent_stack_request_v1,
        validate_artifact_bound_managed_model_agent_stack_request_v1,
    };
    use crate::managed_fabric_apply::tests::{
        controller_signer, ready_snapshot, remote_provisioning_and_ingress, service,
    };
    use crate::managed_fabric_producer::VerifiedManagedFabricProducerContextV1;

    fn lifecycle_budgets(values: [u64; 5]) -> ManagedServiceLifecycleBudgetsV1 {
        ManagedServiceLifecycleBudgetsV1::try_new(
            BoundedDuration::from_nanos(values[0]),
            BoundedDuration::from_nanos(values[1]),
            BoundedDuration::from_nanos(values[2]),
            BoundedDuration::from_nanos(values[3]),
            BoundedDuration::from_nanos(values[4]),
        )
        .expect("lifecycle budgets")
    }

    fn provider(marker: u8) -> ManagedAgentProviderSelectionV1 {
        ManagedAgentProviderSelectionV1::try_deterministic_fixture(
            ManagedAgentProviderRefV1::try_from_bytes([marker; 16]).expect("provider ref"),
            Digest32::from_bytes([marker.wrapping_add(1); 32]),
        )
        .expect("provider selection")
    }

    fn agent_plan() -> ManagedAgentServicePlanV1 {
        let ingress = ManagedAgentIngressLimitsV1::try_new(
            64,
            512 * 1024,
            128 * 1024,
            128 * 1024,
            5_000_000_000,
        )
        .expect("Agent ingress");
        let port = ManagedAgentPortPlanV1::try_new(
            BindingId::from_bytes([0x81; 16]),
            BindingId::from_bytes([0x82; 16]),
            "paraegox/agent/v1/submit",
            "paraegox/agent/v1/control",
            ingress,
        )
        .expect("Agent port");
        ManagedAgentServicePlanV1::try_new(
            ManagedServiceSpecV1::new(
                ManagedServiceId::from_bytes([0x88; 16]),
                lifecycle_budgets([7, 11, 13, 17, 19]),
            ),
            ManagedAgentSemanticLimitsV1::try_new(16, 64, 64, 64).expect("Agent limits"),
            port,
            provider(0x83),
        )
        .expect("Agent plan")
    }

    fn model_plan() -> ManagedModelServicePlanV1 {
        ManagedModelServicePlanV1::try_new(
            ManagedServiceSpecV1::new(
                ManagedServiceId::from_bytes([0x89; 16]),
                lifecycle_budgets([23, 29, 31, 37, 41]),
            ),
            8,
            provider(0x83),
            ManagedModelAdapterBindingV1::try_new(
                *b"px-art-prefix-v1",
                ManagedModelAdapterVersionV1::try_new(1).expect("adapter version"),
                ManagedModelCapabilityIdV1::bounded_text_v1(),
            )
            .expect("Artifact Model adapter"),
        )
        .expect("Model plan")
    }

    fn artifact_binding() -> ArtifactExecutionBindingV1 {
        let pair = VerifiedArtifactPairV1::from_payload(b"literal-prefix-v1 ")
            .expect("Artifact pair");
        let receipt = format!(
            "pxamr1:{}:7:{}:{}",
            "a1".repeat(32),
            "a2".repeat(16),
            "a3".repeat(32),
        )
        .parse::<MaterializationReceiptRefV1>()
        .expect("materialization Receipt ref");
        ArtifactExecutionBindingV1::try_new(
            pair.object_ref(),
            receipt,
            artifact_execution_profile_commitment_v1(),
        )
        .expect("Artifact execution binding")
    }

    #[test]
    fn artifact_bound_plan_content_and_request_are_exact_successors() {
        let snapshot = ready_snapshot();
        let controller = controller_signer();
        let (remote, ingress) = remote_provisioning_and_ingress();
        let context = VerifiedManagedFabricProducerContextV1::try_from_remote_describe(
            snapshot.state(),
            &controller,
            &remote,
            &ingress,
        )
        .expect("verified Fabric context");
        let predecessor = ManagedFabricTargetExecutionV1::try_one_managed_fabric_service(
            context.projection().clone(),
            service(),
            ManagedFabricListenEndpointV1::try_new("tcp/127.0.0.1:7447")
                .expect("Fabric endpoint"),
        )
        .expect("Fabric predecessor");
        let predecessor_slice = TargetSliceDigest::new(Digest32::from_bytes([0xb1; 32]));
        let activation = ManagedModelAgentStackActivationV1::try_new(
            predecessor.clone(),
            agent_plan(),
            model_plan(),
        )
        .expect("Artifact activation");
        let binding = artifact_binding();
        let desired = ArtifactBoundManagedModelAgentStackDesiredPlanV1::try_activate(
            ArtifactBoundManagedModelAgentStackDesiredInputV1 {
                context: &context,
                cutover_marker_digest: Digest32::from_bytes([0xb2; 32]),
                predecessor_revision: SourcePlanRevision::new(context.legacy_revision()),
                predecessor_execution: &predecessor,
                predecessor_slice_digest: predecessor_slice,
                deployment_request_digest: Digest32::from_bytes([0xb3; 32]),
                deployment_admission_digest: Digest32::from_bytes([0xb4; 32]),
                binding,
                activation: &activation,
            },
        )
        .expect("Artifact desired");
        let request = produce_artifact_bound_managed_model_agent_stack_request_v1(
            &context,
            &desired,
            FreshManagedModelAgentStackApplyV1::try_new([0xc1; 16], [0xc2; 16], [0xc3; 32])
                .expect("fresh Runtime identities"),
            &controller,
        )
        .expect("PXAR12");

        assert_eq!(desired.plan_content().binding(), binding);
        assert_eq!(desired.plan_content().execution(), desired.execution());
        assert_eq!(request.target_execution(), desired.execution());
        assert_eq!(request.provenance(), desired.provenance());
        validate_artifact_bound_managed_model_agent_stack_request_v1(
            &context, &desired, &request,
        )
        .expect("validated PXAR12");
        assert_eq!(
            ArtifactBoundManagedModelAgentStackPlanContentV2::decode(
                context.target(),
                desired.plan_content().canonical_bytes(),
            ),
            Ok(desired.plan_content().clone()),
        );
        assert_eq!(
            ArtifactBoundManagedModelAgentStackTargetExecutionV1::decode(
                desired.execution().canonical_wire(),
            ),
            Ok(desired.execution().clone()),
        );
        assert_eq!(
            ArtifactBoundManagedModelAgentStackApplyRequestV1::decode(
                request.canonical_wire(),
            ),
            Ok(request.clone()),
        );
        assert!(
            ManagedModelAgentStackTargetExecutionV1::decode(desired.execution().canonical_wire())
                .is_err()
        );
        assert!(
            ArtifactBoundManagedModelAgentStackTargetExecutionV1::decode(
                desired.execution().embedded().canonical_wire(),
            )
            .is_err()
        );
        assert!(ManagedModelAgentStackApplyRequestV1::decode(request.canonical_wire()).is_err());
        assert!(
            crate::planner::PlanContent::try_from_persisted(
                context.target(),
                desired.plan_content().canonical_bytes(),
            )
            .is_err()
        );

        let mut reserved = desired.plan_content().canonical_bytes().to_vec();
        reserved[35] = 1;
        assert!(
            ArtifactBoundManagedModelAgentStackPlanContentV2::decode(context.target(), &reserved)
                .is_err()
        );
        let swapped = ArtifactBoundManagedModelAgentStackDesiredPlanV1::try_activate(
            ArtifactBoundManagedModelAgentStackDesiredInputV1 {
                context: &context,
                cutover_marker_digest: Digest32::from_bytes([0xb2; 32]),
                predecessor_revision: SourcePlanRevision::new(context.legacy_revision()),
                predecessor_execution: &predecessor,
                predecessor_slice_digest: predecessor_slice,
                deployment_request_digest: Digest32::from_bytes([0xb4; 32]),
                deployment_admission_digest: Digest32::from_bytes([0xb3; 32]),
                binding,
                activation: &activation,
            },
        )
        .expect("swapped commitment desired");
        assert_ne!(
            desired.provenance().source_plan_digest(),
            swapped.provenance().source_plan_digest(),
        );
    }
}
