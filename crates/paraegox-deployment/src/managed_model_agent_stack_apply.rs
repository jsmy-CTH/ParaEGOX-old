//! Durable Controller state for the PXAR v9 Fabric/Model/Agent sibling.
//!
//! PXMJ v1 retains the exact PXAR v6 predecessor without claiming PXAR v7
//! executed. Every valid, authenticated PXMT is durable, including uncertain,
//! quarantined, and no-effect outcomes. Only `ActiveReady` opens the explicit
//! empty transition and only `EmptyExactZero` is deactivation success.

use core::num::NonZeroU64;
use core::{fmt, str::FromStr};

use ed25519_dalek::Signature;
use paraegox_artifact::{
    ArtifactConfigCommitmentV1, ArtifactContractError, ArtifactObjectRefV1,
    MaterializationReceiptRefV1,
};
use paraegox_kernel::digest::{Digest32, Digest32Builder, DigestBuildError};
use paraegox_runtime_contracts::apply::ExpectedActive;
use paraegox_runtime_contracts::managed_fabric_plan::{
    ManagedFabricApplyTerminalOutcomeV1, ManagedFabricTargetExecutionV1, ManagedFabricTargetModeV1,
};
use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
    ArtifactBoundManagedModelAgentStackApplyRequestV1,
    ArtifactBoundManagedModelAgentStackTargetExecutionV1, ArtifactExecutionBindingV1,
    ManagedModelAgentStackApplyRequestV1, ManagedModelAgentStackPlanError,
    ManagedModelAgentStackTargetModeV1, ManagedModelAgentStackTerminalOutcomeV1,
    ManagedModelAgentStackTerminalReceiptV1, artifact_execution_profile_commitment_v1,
};
use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
use paraegox_runtime_contracts::provenance::{SourcePlanRevision, TargetSliceDigest};
use paraegox_runtime_contracts::reference_control::ReferenceChannelBindingV1;
use sha2::{Digest as _, Sha256};

use crate::managed_fabric_apply::{
    ManagedFabricApplyControllerError, ManagedFabricApplyPhaseV1, ManagedFabricControllerStateV1,
};
use crate::managed_fabric_producer::{
    ManagedFabricControllerProvisioningV1, VerifiedManagedFabricProducerContextV1,
};
use crate::managed_model_agent_stack_producer::{
    ArtifactBoundManagedModelAgentStackPlanContentV2, FreshManagedModelAgentStackApplyV1,
    ManagedModelAgentStackActivationV1, ManagedModelAgentStackDesiredPlanV1,
    ManagedModelAgentStackProducerError, produce_managed_model_agent_stack_empty_request_v1,
    produce_managed_model_agent_stack_request_v1,
    validate_managed_model_agent_stack_empty_request_v1,
    validate_managed_model_agent_stack_request_v1,
};

const STATE_MAGIC: &[u8; 4] = b"PXMJ";
const STATE_VERSION: u16 = 1;
const STATE_FIXED_BYTES: usize = 79;
const STATE_CHECKSUM_BYTES: usize = 32;
const MAX_STATE_BYTES: usize = 2 * 1024 * 1024;
const STATE_CHECKSUM_DOMAIN: &[u8] =
    b"paraegox.deployment.managed-model-agent-stack-state.sha256.v1";
const ED25519_ALGORITHM: u16 = 1;
const ED25519_ALGORITHM_VERSION: u16 = 1;
const ED25519_SIGNATURE_BYTES: usize = 64;
const EXTERNAL_REQUEST_BYTES: usize = 288;
const EXTERNAL_ADMISSION_BYTES: usize = 240;
const EXTERNAL_RECORD_BYTES: usize = 496;
const EXTERNAL_RECEIPT_BYTES: usize = 432;
const EXTERNAL_REQUEST_DIGEST_DOMAIN: &[u8] = b"paraegox.deployment.external-request.sha256.v1";
const EXTERNAL_ADMISSION_DIGEST_DOMAIN: &[u8] = b"paraegox.deployment.external-admission.sha256.v1";
const EXTERNAL_RECORD_DIGEST_DOMAIN: &[u8] =
    b"paraegox.deployment.external-operation-record.sha256.v1";
const EXTERNAL_RECEIPT_DIGEST_DOMAIN: &[u8] = b"paraegox.deployment.external-receipt.sha256.v1";
const ARTIFACT_STATE_V2_HEADER_BYTES: usize = 192;
const ARTIFACT_STATE_V2_CHECKSUM_BYTES: usize = 32;
const MAX_ARTIFACT_STATE_V2_BYTES: usize = 16_614;
const MAX_ARTIFACT_PLAN_CONTENT_V2_BYTES: usize = 2_758;
const MAX_ARTIFACT_TARGET_EXECUTION_V11_BYTES: usize = 2_506;
const MAX_ARTIFACT_APPLY_REQUEST_V12_BYTES: usize = 6_630;
const MAX_ARTIFACT_TERMINAL_RECEIPT_V1_BYTES: usize = 2_048;
const ARTIFACT_STATE_V2_CHECKSUM_DOMAIN: &[u8] =
    b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2";
const ARTIFACT_CUTOVER_MARKER_DIGEST_DOMAIN: &[u8] =
    b"paraegox.deployment.artifact-external-cutover-marker.sha256.v1";
const ARTIFACT_DESIRED_DIGEST_DOMAIN: &[u8] =
    b"paraegox.deployment.artifact-bound-managed-model-agent-stack-desired.sha256.v1";

/// DeploymentController-owned D0b operation identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactDeploymentOperationIdV1([u8; 16]);

impl ArtifactDeploymentOperationIdV1 {
    pub const fn try_from_bytes(bytes: [u8; 16]) -> Option<Self> {
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != 0 {
                return Some(Self(bytes));
            }
            index += 1;
        }
        None
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Fixed PXDQ v1 external Artifact deployment request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactExternalDeploymentRequestV1 {
    operation_id: ArtifactDeploymentOperationIdV1,
    config_commitment: ArtifactConfigCommitmentV1,
    binding: ArtifactExecutionBindingV1,
    request_digest: Digest32,
    canonical_wire: [u8; EXTERNAL_REQUEST_BYTES],
}

impl ArtifactExternalDeploymentRequestV1 {
    pub(crate) fn try_new(
        operation_id: ArtifactDeploymentOperationIdV1,
        config_commitment: ArtifactConfigCommitmentV1,
        binding: ArtifactExecutionBindingV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let mut canonical_wire = [0_u8; EXTERNAL_REQUEST_BYTES];
        canonical_wire[0..4].copy_from_slice(b"PXDQ");
        canonical_wire[4..6].copy_from_slice(&1_u16.to_be_bytes());
        canonical_wire[6] = b'D';
        canonical_wire[8..10].copy_from_slice(&(EXTERNAL_REQUEST_BYTES as u16).to_be_bytes());
        canonical_wire[12..16].copy_from_slice(&(EXTERNAL_REQUEST_BYTES as u32).to_be_bytes());
        canonical_wire[16..32].copy_from_slice(operation_id.as_bytes());
        canonical_wire[32..64].copy_from_slice(config_commitment.as_bytes());
        canonical_wire[64..256].copy_from_slice(binding.canonical_wire());
        let request_digest = raw_sha256(EXTERNAL_REQUEST_DIGEST_DOMAIN, &canonical_wire[..256]);
        require_nonzero_digest(request_digest)?;
        canonical_wire[256..288].copy_from_slice(request_digest.as_bytes());
        Ok(Self {
            operation_id,
            config_commitment,
            binding,
            request_digest,
            canonical_wire,
        })
    }

    pub(crate) fn decode(frame: &[u8]) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if frame.len() != EXTERNAL_REQUEST_BYTES
            || frame.get(0..4) != Some(b"PXDQ".as_slice())
            || read_u16_at(frame, 4) != Some(1)
            || frame[6] != b'D'
            || frame[7] != 0
            || read_u16_at(frame, 8) != Some(EXTERNAL_REQUEST_BYTES as u16)
            || frame[10..12] != [0; 2]
            || read_u32_at(frame, 12) != Some(EXTERNAL_REQUEST_BYTES as u32)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let operation_id = ArtifactDeploymentOperationIdV1::try_from_bytes(
            frame[16..32]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let config_commitment = ArtifactConfigCommitmentV1::try_from_bytes(
            frame[32..64]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )?;
        let binding = ArtifactExecutionBindingV1::decode(&frame[64..256])?;
        let decoded = Self::try_new(operation_id, config_commitment, binding)?;
        if decoded.canonical_wire() != frame {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(decoded)
    }

    #[must_use]
    pub(crate) const fn operation_id(&self) -> ArtifactDeploymentOperationIdV1 {
        self.operation_id
    }

    #[must_use]
    pub(crate) const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.config_commitment
    }

    #[must_use]
    pub(crate) const fn binding(&self) -> ArtifactExecutionBindingV1 {
        self.binding
    }

    #[must_use]
    pub(crate) const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }

    #[must_use]
    pub(crate) const fn canonical_wire(&self) -> &[u8; EXTERNAL_REQUEST_BYTES] {
        &self.canonical_wire
    }
}

/// Fixed fresh-only PXDK v1 Controller admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactExternalDeploymentAdmissionV1 {
    controller_store_instance: [u8; 32],
    admission_sequence: NonZeroU64,
    operation_id: ArtifactDeploymentOperationIdV1,
    request_digest: Digest32,
    object_ref: ArtifactObjectRefV1,
    admission_digest: Digest32,
    canonical_wire: [u8; EXTERNAL_ADMISSION_BYTES],
}

impl ArtifactExternalDeploymentAdmissionV1 {
    pub(crate) fn try_new(
        controller_store_instance: [u8; 32],
        admission_sequence: NonZeroU64,
        request: &ArtifactExternalDeploymentRequestV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if controller_store_instance.iter().all(|byte| *byte == 0) {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let mut canonical_wire = [0_u8; EXTERNAL_ADMISSION_BYTES];
        canonical_wire[0..4].copy_from_slice(b"PXDK");
        canonical_wire[4..6].copy_from_slice(&1_u16.to_be_bytes());
        canonical_wire[6] = b'D';
        canonical_wire[7] = b'A';
        canonical_wire[8..10].copy_from_slice(&(EXTERNAL_ADMISSION_BYTES as u16).to_be_bytes());
        canonical_wire[12..16].copy_from_slice(&(EXTERNAL_ADMISSION_BYTES as u32).to_be_bytes());
        canonical_wire[16..48].copy_from_slice(&controller_store_instance);
        canonical_wire[48..56].copy_from_slice(&admission_sequence.get().to_be_bytes());
        canonical_wire[56..72].copy_from_slice(request.operation_id().as_bytes());
        canonical_wire[72..104].copy_from_slice(request.request_digest().as_bytes());
        canonical_wire[104..176].copy_from_slice(&request.binding().object_ref().encode());
        let admission_digest = raw_sha256(EXTERNAL_ADMISSION_DIGEST_DOMAIN, &canonical_wire[..208]);
        require_nonzero_digest(admission_digest)?;
        canonical_wire[208..240].copy_from_slice(admission_digest.as_bytes());
        Ok(Self {
            controller_store_instance,
            admission_sequence,
            operation_id: request.operation_id(),
            request_digest: request.request_digest(),
            object_ref: request.binding().object_ref(),
            admission_digest,
            canonical_wire,
        })
    }

    pub(crate) fn decode(
        frame: &[u8],
        request: &ArtifactExternalDeploymentRequestV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if frame.len() != EXTERNAL_ADMISSION_BYTES
            || frame.get(0..4) != Some(b"PXDK".as_slice())
            || read_u16_at(frame, 4) != Some(1)
            || frame[6] != b'D'
            || frame[7] != b'A'
            || read_u16_at(frame, 8) != Some(EXTERNAL_ADMISSION_BYTES as u16)
            || frame[10..12] != [0; 2]
            || read_u32_at(frame, 12) != Some(EXTERNAL_ADMISSION_BYTES as u32)
            || frame[176..208].iter().any(|byte| *byte != 0)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let controller_store_instance: [u8; 32] = frame[16..48]
            .try_into()
            .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let admission_sequence = NonZeroU64::new(
            read_u64_at(frame, 48)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let decoded = Self::try_new(controller_store_instance, admission_sequence, request)?;
        if decoded.canonical_wire() != frame {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(decoded)
    }

    #[must_use]
    pub(crate) const fn controller_store_instance(&self) -> &[u8; 32] {
        &self.controller_store_instance
    }

    #[must_use]
    pub(crate) const fn admission_sequence(&self) -> NonZeroU64 {
        self.admission_sequence
    }

    #[must_use]
    pub(crate) const fn operation_id(&self) -> ArtifactDeploymentOperationIdV1 {
        self.operation_id
    }

    #[must_use]
    pub(crate) const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }

    #[must_use]
    pub(crate) const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }

    #[must_use]
    pub(crate) const fn admission_digest(&self) -> Digest32 {
        self.admission_digest
    }

    #[must_use]
    pub(crate) const fn canonical_wire(&self) -> &[u8; EXTERNAL_ADMISSION_BYTES] {
        &self.canonical_wire
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactExternalDeploymentRecordStateV1 {
    Committed,
    Applying,
    ActiveReady,
    Failed,
    Uncertain,
}

impl ArtifactExternalDeploymentRecordStateV1 {
    const fn state_byte(self) -> u8 {
        match self {
            Self::Committed => b'C',
            Self::Applying => b'P',
            Self::ActiveReady => b'R',
            Self::Failed => b'F',
            Self::Uncertain => b'U',
        }
    }

    const fn outcome_byte(self) -> u8 {
        match self {
            Self::Committed | Self::Applying => b'N',
            Self::ActiveReady => b'R',
            Self::Failed => b'F',
            Self::Uncertain => b'U',
        }
    }

    fn decode(state: u8, outcome: u8) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        match (state, outcome) {
            (b'C', b'N') => Ok(Self::Committed),
            (b'P', b'N') => Ok(Self::Applying),
            (b'R', b'R') => Ok(Self::ActiveReady),
            (b'F', b'F') => Ok(Self::Failed),
            (b'U', b'U') => Ok(Self::Uncertain),
            _ => Err(ManagedModelAgentStackApplyControllerError::InvalidState),
        }
    }

    const fn is_terminal(self) -> bool {
        matches!(self, Self::ActiveReady | Self::Failed | Self::Uncertain)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactExternalDeploymentProgressV1 {
    deployment_revision: u64,
    controller_snapshot_sequence: u64,
    desired_head_digest: [u8; 32],
    runtime_apply_request_digest: [u8; 32],
    runtime_terminal_receipt_digest: [u8; 32],
    lifecycle_generation: [u8; 16],
}

impl ArtifactExternalDeploymentProgressV1 {
    pub(crate) fn try_new(
        deployment_revision: Option<NonZeroU64>,
        controller_snapshot_sequence: Option<NonZeroU64>,
        desired_head_digest: Option<Digest32>,
        runtime_apply_request_digest: Option<Digest32>,
        runtime_terminal_receipt_digest: Option<Digest32>,
        lifecycle_generation: Option<[u8; 16]>,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let committed_presence = [
            deployment_revision.is_some(),
            controller_snapshot_sequence.is_some(),
            desired_head_digest.is_some(),
        ];
        if committed_presence.iter().any(|present| *present)
            && committed_presence.iter().any(|present| !*present)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        if runtime_apply_request_digest.is_some()
            && (!committed_presence[0] || lifecycle_generation.is_none())
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        if runtime_terminal_receipt_digest.is_some() && runtime_apply_request_digest.is_none() {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let desired_head_digest = optional_nonzero_digest(desired_head_digest)?;
        let runtime_apply_request_digest = optional_nonzero_digest(runtime_apply_request_digest)?;
        let runtime_terminal_receipt_digest =
            optional_nonzero_digest(runtime_terminal_receipt_digest)?;
        if lifecycle_generation.is_some_and(|generation| generation.iter().all(|byte| *byte == 0)) {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let lifecycle_generation = lifecycle_generation.unwrap_or([0; 16]);
        if lifecycle_generation.iter().all(|byte| *byte == 0)
            && runtime_apply_request_digest.iter().any(|byte| *byte != 0)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(Self {
            deployment_revision: deployment_revision.map_or(0, NonZeroU64::get),
            controller_snapshot_sequence: controller_snapshot_sequence.map_or(0, NonZeroU64::get),
            desired_head_digest,
            runtime_apply_request_digest,
            runtime_terminal_receipt_digest,
            lifecycle_generation,
        })
    }

    const fn from_wire(
        deployment_revision: u64,
        controller_snapshot_sequence: u64,
        desired_head_digest: [u8; 32],
        runtime_apply_request_digest: [u8; 32],
        runtime_terminal_receipt_digest: [u8; 32],
        lifecycle_generation: [u8; 16],
    ) -> Self {
        Self {
            deployment_revision,
            controller_snapshot_sequence,
            desired_head_digest,
            runtime_apply_request_digest,
            runtime_terminal_receipt_digest,
            lifecycle_generation,
        }
    }

    fn validate(self) -> Result<(), ManagedModelAgentStackApplyControllerError> {
        let revision = NonZeroU64::new(self.deployment_revision);
        let snapshot = NonZeroU64::new(self.controller_snapshot_sequence);
        let desired = optional_digest_from_bytes(self.desired_head_digest);
        let runtime_request = optional_digest_from_bytes(self.runtime_apply_request_digest);
        let runtime_terminal = optional_digest_from_bytes(self.runtime_terminal_receipt_digest);
        let lifecycle = if self.lifecycle_generation == [0; 16] {
            None
        } else {
            Some(self.lifecycle_generation)
        };
        Self::try_new(
            revision,
            snapshot,
            desired,
            runtime_request,
            runtime_terminal,
            lifecycle,
        )?;
        Ok(())
    }

    fn preserves(self, previous: Self) -> Result<(), ManagedModelAgentStackApplyControllerError> {
        if !preserves_optional_u64(self.deployment_revision, previous.deployment_revision)
            || !preserves_optional_u64(
                self.controller_snapshot_sequence,
                previous.controller_snapshot_sequence,
            )
            || !preserves_optional_bytes(self.desired_head_digest, previous.desired_head_digest)
            || !preserves_optional_bytes(
                self.runtime_apply_request_digest,
                previous.runtime_apply_request_digest,
            )
            || !preserves_optional_bytes(
                self.runtime_terminal_receipt_digest,
                previous.runtime_terminal_receipt_digest,
            )
            || !preserves_optional_bytes(self.lifecycle_generation, previous.lifecycle_generation)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(())
    }

    const fn is_committed(self) -> bool {
        self.deployment_revision != 0
            && self.controller_snapshot_sequence != 0
            && !all_zero_32(self.desired_head_digest)
    }

    const fn is_applying(self) -> bool {
        self.is_committed()
            && !all_zero_32(self.runtime_apply_request_digest)
            && !all_zero_16(self.lifecycle_generation)
    }

    const fn has_runtime_terminal(self) -> bool {
        !all_zero_32(self.runtime_terminal_receipt_digest)
    }

    const fn has_runtime_request(self) -> bool {
        !all_zero_32(self.runtime_apply_request_digest)
    }

    const fn has_lifecycle_generation(self) -> bool {
        !all_zero_16(self.lifecycle_generation)
    }

    const fn deployment_revision(self) -> Option<NonZeroU64> {
        NonZeroU64::new(self.deployment_revision)
    }

    const fn committed_controller_snapshot_sequence(self) -> Option<NonZeroU64> {
        NonZeroU64::new(self.controller_snapshot_sequence)
    }

    fn runtime_apply_request_digest(self) -> Option<Digest32> {
        optional_digest_from_bytes(self.runtime_apply_request_digest)
    }

    fn runtime_terminal_receipt_digest(self) -> Option<Digest32> {
        optional_digest_from_bytes(self.runtime_terminal_receipt_digest)
    }

    const fn lifecycle_generation(self) -> Option<[u8; 16]> {
        if all_zero_16(self.lifecycle_generation) {
            None
        } else {
            Some(self.lifecycle_generation)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactExternalDeploymentRecordV1 {
    state: ArtifactExternalDeploymentRecordStateV1,
    record_sequence: NonZeroU64,
    progress: ArtifactExternalDeploymentProgressV1,
    operation_record_digest: Digest32,
    canonical_wire: [u8; EXTERNAL_RECORD_BYTES],
}

impl ArtifactExternalDeploymentRecordV1 {
    pub(crate) fn try_new(
        state: ArtifactExternalDeploymentRecordStateV1,
        record_sequence: NonZeroU64,
        request: &ArtifactExternalDeploymentRequestV1,
        admission: &ArtifactExternalDeploymentAdmissionV1,
        progress: ArtifactExternalDeploymentProgressV1,
        previous: Option<&Self>,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        validate_request_admission(request, admission)?;
        if let Some(previous) = previous {
            validate_record_owner(previous, request, admission)?;
        }
        progress.validate()?;
        validate_record_transition(state, record_sequence, progress, previous)?;

        let mut canonical_wire = [0_u8; EXTERNAL_RECORD_BYTES];
        canonical_wire[0..4].copy_from_slice(b"PXDM");
        canonical_wire[4..6].copy_from_slice(&1_u16.to_be_bytes());
        canonical_wire[6] = state.state_byte();
        canonical_wire[7] = state.outcome_byte();
        canonical_wire[8..10].copy_from_slice(&(EXTERNAL_RECORD_BYTES as u16).to_be_bytes());
        canonical_wire[12..16].copy_from_slice(&(EXTERNAL_RECORD_BYTES as u32).to_be_bytes());
        canonical_wire[16..48].copy_from_slice(admission.controller_store_instance());
        canonical_wire[48..56].copy_from_slice(&record_sequence.get().to_be_bytes());
        canonical_wire[56..72].copy_from_slice(request.operation_id().as_bytes());
        canonical_wire[72..104].copy_from_slice(request.request_digest().as_bytes());
        canonical_wire[104..136].copy_from_slice(admission.admission_digest().as_bytes());
        canonical_wire[136..208].copy_from_slice(&request.binding().object_ref().encode());
        canonical_wire[208..240].copy_from_slice(
            request
                .binding()
                .materialization_receipt_ref()
                .receipt_digest()
                .as_bytes(),
        );
        canonical_wire[240..248].copy_from_slice(&progress.deployment_revision.to_be_bytes());
        canonical_wire[248..256]
            .copy_from_slice(&progress.controller_snapshot_sequence.to_be_bytes());
        canonical_wire[256..288].copy_from_slice(&progress.desired_head_digest);
        canonical_wire[288..320].copy_from_slice(&progress.runtime_apply_request_digest);
        canonical_wire[320..352].copy_from_slice(&progress.runtime_terminal_receipt_digest);
        canonical_wire[352..368].copy_from_slice(&progress.lifecycle_generation);
        canonical_wire[368..400]
            .copy_from_slice(request.binding().execution_profile_commitment().as_bytes());
        if let Some(previous) = previous {
            canonical_wire[400..432].copy_from_slice(previous.operation_record_digest().as_bytes());
        }
        let operation_record_digest =
            raw_sha256(EXTERNAL_RECORD_DIGEST_DOMAIN, &canonical_wire[..464]);
        require_nonzero_digest(operation_record_digest)?;
        canonical_wire[464..496].copy_from_slice(operation_record_digest.as_bytes());
        Ok(Self {
            state,
            record_sequence,
            progress,
            operation_record_digest,
            canonical_wire,
        })
    }

    pub(crate) fn decode(
        frame: &[u8],
        request: &ArtifactExternalDeploymentRequestV1,
        admission: &ArtifactExternalDeploymentAdmissionV1,
        previous: Option<&Self>,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if frame.len() != EXTERNAL_RECORD_BYTES
            || frame.get(0..4) != Some(b"PXDM".as_slice())
            || read_u16_at(frame, 4) != Some(1)
            || read_u16_at(frame, 8) != Some(EXTERNAL_RECORD_BYTES as u16)
            || frame[10..12] != [0; 2]
            || read_u32_at(frame, 12) != Some(EXTERNAL_RECORD_BYTES as u32)
            || frame[432..464].iter().any(|byte| *byte != 0)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let state = ArtifactExternalDeploymentRecordStateV1::decode(frame[6], frame[7])?;
        let record_sequence = NonZeroU64::new(
            read_u64_at(frame, 48)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let progress = ArtifactExternalDeploymentProgressV1::from_wire(
            read_u64_at(frame, 240)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
            read_u64_at(frame, 248)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
            frame[256..288]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
            frame[288..320]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
            frame[320..352]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
            frame[352..368]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
        );
        let decoded = Self::try_new(
            state,
            record_sequence,
            request,
            admission,
            progress,
            previous,
        )?;
        if decoded.canonical_wire() != frame {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(decoded)
    }

    #[must_use]
    pub(crate) const fn state(&self) -> ArtifactExternalDeploymentRecordStateV1 {
        self.state
    }

    #[must_use]
    pub(crate) const fn record_sequence(&self) -> NonZeroU64 {
        self.record_sequence
    }

    #[must_use]
    pub(crate) const fn progress(&self) -> ArtifactExternalDeploymentProgressV1 {
        self.progress
    }

    #[must_use]
    pub(crate) const fn operation_record_digest(&self) -> Digest32 {
        self.operation_record_digest
    }

    #[must_use]
    pub(crate) const fn canonical_wire(&self) -> &[u8; EXTERNAL_RECORD_BYTES] {
        &self.canonical_wire
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactExternalDeploymentReceiptV1 {
    receipt_sequence: NonZeroU64,
    deployment_receipt_digest: Digest32,
    canonical_wire: [u8; EXTERNAL_RECEIPT_BYTES],
}

impl ArtifactExternalDeploymentReceiptV1 {
    pub(crate) fn try_new(
        receipt_sequence: NonZeroU64,
        request: &ArtifactExternalDeploymentRequestV1,
        admission: &ArtifactExternalDeploymentAdmissionV1,
        terminal: &ArtifactExternalDeploymentRecordV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        validate_request_admission(request, admission)?;
        if !terminal.state().is_terminal() {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        validate_record_owner(terminal, request, admission)?;
        let mut canonical_wire = [0_u8; EXTERNAL_RECEIPT_BYTES];
        canonical_wire[0..4].copy_from_slice(b"PXDO");
        canonical_wire[4..6].copy_from_slice(&1_u16.to_be_bytes());
        canonical_wire[6] = b'D';
        canonical_wire[7] = terminal.state().outcome_byte();
        canonical_wire[8..10].copy_from_slice(&(EXTERNAL_RECEIPT_BYTES as u16).to_be_bytes());
        canonical_wire[12..16].copy_from_slice(&(EXTERNAL_RECEIPT_BYTES as u32).to_be_bytes());
        canonical_wire[16..48].copy_from_slice(admission.controller_store_instance());
        canonical_wire[48..56].copy_from_slice(&receipt_sequence.get().to_be_bytes());
        canonical_wire[56..72].copy_from_slice(request.operation_id().as_bytes());
        canonical_wire[72..104].copy_from_slice(request.request_digest().as_bytes());
        canonical_wire[104..136].copy_from_slice(terminal.operation_record_digest().as_bytes());
        canonical_wire[136..208].copy_from_slice(&request.binding().object_ref().encode());
        canonical_wire[208..240].copy_from_slice(
            request
                .binding()
                .materialization_receipt_ref()
                .receipt_digest()
                .as_bytes(),
        );
        let progress = terminal.progress();
        canonical_wire[240..248].copy_from_slice(&progress.deployment_revision.to_be_bytes());
        canonical_wire[248..256]
            .copy_from_slice(&progress.controller_snapshot_sequence.to_be_bytes());
        canonical_wire[256..288].copy_from_slice(&progress.desired_head_digest);
        canonical_wire[288..320].copy_from_slice(&progress.runtime_apply_request_digest);
        canonical_wire[320..352].copy_from_slice(&progress.runtime_terminal_receipt_digest);
        canonical_wire[352..368].copy_from_slice(&progress.lifecycle_generation);
        let deployment_receipt_digest =
            raw_sha256(EXTERNAL_RECEIPT_DIGEST_DOMAIN, &canonical_wire[..400]);
        require_nonzero_digest(deployment_receipt_digest)?;
        canonical_wire[400..432].copy_from_slice(deployment_receipt_digest.as_bytes());
        Ok(Self {
            receipt_sequence,
            deployment_receipt_digest,
            canonical_wire,
        })
    }

    pub(crate) fn decode(
        frame: &[u8],
        request: &ArtifactExternalDeploymentRequestV1,
        admission: &ArtifactExternalDeploymentAdmissionV1,
        terminal: &ArtifactExternalDeploymentRecordV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if frame.len() != EXTERNAL_RECEIPT_BYTES
            || frame.get(0..4) != Some(b"PXDO".as_slice())
            || read_u16_at(frame, 4) != Some(1)
            || frame[6] != b'D'
            || read_u16_at(frame, 8) != Some(EXTERNAL_RECEIPT_BYTES as u16)
            || frame[10..12] != [0; 2]
            || read_u32_at(frame, 12) != Some(EXTERNAL_RECEIPT_BYTES as u32)
            || frame[368..400].iter().any(|byte| *byte != 0)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let receipt_sequence = NonZeroU64::new(
            read_u64_at(frame, 48)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let decoded = Self::try_new(receipt_sequence, request, admission, terminal)?;
        if decoded.canonical_wire() != frame {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(decoded)
    }

    #[must_use]
    pub(crate) const fn receipt_sequence(&self) -> NonZeroU64 {
        self.receipt_sequence
    }

    #[must_use]
    pub(crate) const fn deployment_receipt_digest(&self) -> Digest32 {
        self.deployment_receipt_digest
    }

    #[must_use]
    pub(crate) const fn canonical_wire(&self) -> &[u8; EXTERNAL_RECEIPT_BYTES] {
        &self.canonical_wire
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeploymentReceiptRefV1 {
    controller_store_instance: [u8; 32],
    receipt_sequence: NonZeroU64,
    operation_id: ArtifactDeploymentOperationIdV1,
    deployment_receipt_digest: Digest32,
}

impl DeploymentReceiptRefV1 {
    pub(crate) fn from_receipt(
        request: &ArtifactExternalDeploymentRequestV1,
        admission: &ArtifactExternalDeploymentAdmissionV1,
        receipt: &ArtifactExternalDeploymentReceiptV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        validate_request_admission(request, admission)?;
        validate_receipt_owner(receipt, request, admission)?;
        if receipt
            .deployment_receipt_digest()
            .as_bytes()
            .iter()
            .all(|byte| *byte == 0)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(Self {
            controller_store_instance: *admission.controller_store_instance(),
            receipt_sequence: receipt.receipt_sequence(),
            operation_id: request.operation_id(),
            deployment_receipt_digest: receipt.deployment_receipt_digest(),
        })
    }

    pub(crate) fn encode(&self) -> String {
        format!(
            "pxdor1:{}:{}:{}:{}",
            lower_hex(&self.controller_store_instance),
            self.receipt_sequence.get(),
            lower_hex(self.operation_id.as_bytes()),
            lower_hex(self.deployment_receipt_digest.as_bytes()),
        )
    }

    #[must_use]
    pub(crate) const fn controller_store_instance(&self) -> &[u8; 32] {
        &self.controller_store_instance
    }

    #[must_use]
    pub(crate) const fn receipt_sequence(&self) -> NonZeroU64 {
        self.receipt_sequence
    }

    #[must_use]
    pub(crate) const fn operation_id(&self) -> ArtifactDeploymentOperationIdV1 {
        self.operation_id
    }

    #[must_use]
    pub(crate) const fn deployment_receipt_digest(&self) -> Digest32 {
        self.deployment_receipt_digest
    }
}

impl fmt::Display for DeploymentReceiptRefV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.encode())
    }
}

impl FromStr for DeploymentReceiptRefV1 {
    type Err = ManagedModelAgentStackApplyControllerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(':');
        if parts.next() != Some("pxdor1") {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let controller_store_instance = decode_lower_hex_exact::<32>(
            parts
                .next()
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )?;
        if controller_store_instance.iter().all(|byte| *byte == 0) {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let receipt_sequence_text = parts
            .next()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        if receipt_sequence_text.is_empty()
            || receipt_sequence_text.starts_with('0')
            || !receipt_sequence_text
                .as_bytes()
                .iter()
                .all(u8::is_ascii_digit)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let receipt_sequence = receipt_sequence_text
            .parse::<u64>()
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let operation_id =
            ArtifactDeploymentOperationIdV1::try_from_bytes(decode_lower_hex_exact::<16>(
                parts
                    .next()
                    .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
            )?)
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let deployment_receipt_digest = Digest32::from_bytes(decode_lower_hex_exact::<32>(
            parts
                .next()
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )?);
        if parts.next().is_some()
            || deployment_receipt_digest
                .as_bytes()
                .iter()
                .all(|byte| *byte == 0)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let decoded = Self {
            controller_store_instance,
            receipt_sequence,
            operation_id,
            deployment_receipt_digest,
        };
        if decoded.encode() != value {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(decoded)
    }
}

/// Durable phase byte of one Artifact-bound PXMJ v2 snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactExternalControllerPhaseV2 {
    Admitted,
    Committed,
    Applying,
    ActiveReady,
    Failed,
    Uncertain,
}

impl ArtifactExternalControllerPhaseV2 {
    const fn phase_byte(self) -> u8 {
        match self {
            Self::Admitted => b'A',
            Self::Committed => b'C',
            Self::Applying => b'P',
            Self::ActiveReady => b'R',
            Self::Failed => b'F',
            Self::Uncertain => b'U',
        }
    }

    fn decode(value: u8) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        match value {
            b'A' => Ok(Self::Admitted),
            b'C' => Ok(Self::Committed),
            b'P' => Ok(Self::Applying),
            b'R' => Ok(Self::ActiveReady),
            b'F' => Ok(Self::Failed),
            b'U' => Ok(Self::Uncertain),
            _ => Err(ManagedModelAgentStackApplyControllerError::InvalidState),
        }
    }
}

/// Fully-owned semantic input for one canonical PXMJ v2 snapshot.
pub(crate) struct ArtifactExternalControllerStateInputV2 {
    pub(crate) phase: ArtifactExternalControllerPhaseV2,
    pub(crate) controller_snapshot_sequence: NonZeroU64,
    pub(crate) request: ArtifactExternalDeploymentRequestV1,
    pub(crate) admission: ArtifactExternalDeploymentAdmissionV1,
    pub(crate) plan_content: Option<ArtifactBoundManagedModelAgentStackPlanContentV2>,
    pub(crate) execution: Option<ArtifactBoundManagedModelAgentStackTargetExecutionV1>,
    pub(crate) runtime_request: Option<ArtifactBoundManagedModelAgentStackApplyRequestV1>,
    pub(crate) runtime_terminal: Option<ManagedModelAgentStackTerminalReceiptV1>,
    pub(crate) records: Vec<ArtifactExternalDeploymentRecordV1>,
    pub(crate) receipt: Option<ArtifactExternalDeploymentReceiptV1>,
}

/// Canonical self-contained Artifact-bound Controller state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactExternalControllerStateV2 {
    phase: ArtifactExternalControllerPhaseV2,
    controller_snapshot_sequence: NonZeroU64,
    request: ArtifactExternalDeploymentRequestV1,
    admission: ArtifactExternalDeploymentAdmissionV1,
    plan_content: Option<ArtifactBoundManagedModelAgentStackPlanContentV2>,
    execution: Option<ArtifactBoundManagedModelAgentStackTargetExecutionV1>,
    runtime_request: Option<ArtifactBoundManagedModelAgentStackApplyRequestV1>,
    runtime_terminal: Option<ManagedModelAgentStackTerminalReceiptV1>,
    records: Vec<ArtifactExternalDeploymentRecordV1>,
    receipt: Option<ArtifactExternalDeploymentReceiptV1>,
}

impl ArtifactExternalControllerStateV2 {
    pub(crate) fn admit(
        request: ArtifactExternalDeploymentRequestV1,
        controller_store_instance: [u8; 32],
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let admission = ArtifactExternalDeploymentAdmissionV1::try_new(
            controller_store_instance,
            NonZeroU64::new(1).expect("one is nonzero"),
            &request,
        )?;
        Self::try_new(ArtifactExternalControllerStateInputV2 {
            phase: ArtifactExternalControllerPhaseV2::Admitted,
            controller_snapshot_sequence: NonZeroU64::new(1).expect("one is nonzero"),
            request,
            admission,
            plan_content: None,
            execution: None,
            runtime_request: None,
            runtime_terminal: None,
            records: Vec::new(),
            receipt: None,
        })
    }

    pub(crate) fn try_new(
        input: ArtifactExternalControllerStateInputV2,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let state = Self {
            phase: input.phase,
            controller_snapshot_sequence: input.controller_snapshot_sequence,
            request: input.request,
            admission: input.admission,
            plan_content: input.plan_content,
            execution: input.execution,
            runtime_request: input.runtime_request,
            runtime_terminal: input.runtime_terminal,
            records: input.records,
            receipt: input.receipt,
        };
        state.validate()?;
        Ok(state)
    }

    #[must_use]
    pub(crate) const fn phase(&self) -> ArtifactExternalControllerPhaseV2 {
        self.phase
    }

    #[must_use]
    pub(crate) const fn controller_snapshot_sequence(&self) -> NonZeroU64 {
        self.controller_snapshot_sequence
    }

    #[must_use]
    pub(crate) const fn request(&self) -> &ArtifactExternalDeploymentRequestV1 {
        &self.request
    }

    #[must_use]
    pub(crate) const fn admission(&self) -> &ArtifactExternalDeploymentAdmissionV1 {
        &self.admission
    }

    #[must_use]
    pub(crate) const fn plan_content(
        &self,
    ) -> Option<&ArtifactBoundManagedModelAgentStackPlanContentV2> {
        self.plan_content.as_ref()
    }

    #[must_use]
    pub(crate) const fn execution(
        &self,
    ) -> Option<&ArtifactBoundManagedModelAgentStackTargetExecutionV1> {
        self.execution.as_ref()
    }

    #[must_use]
    pub(crate) const fn runtime_request(
        &self,
    ) -> Option<&ArtifactBoundManagedModelAgentStackApplyRequestV1> {
        self.runtime_request.as_ref()
    }

    #[must_use]
    pub(crate) const fn runtime_terminal(
        &self,
    ) -> Option<&ManagedModelAgentStackTerminalReceiptV1> {
        self.runtime_terminal.as_ref()
    }

    #[must_use]
    pub(crate) fn records(&self) -> &[ArtifactExternalDeploymentRecordV1] {
        &self.records
    }

    #[must_use]
    pub(crate) const fn receipt(&self) -> Option<&ArtifactExternalDeploymentReceiptV1> {
        self.receipt.as_ref()
    }

    pub(crate) fn commit(
        &self,
        plan_content: ArtifactBoundManagedModelAgentStackPlanContentV2,
        execution: ArtifactBoundManagedModelAgentStackTargetExecutionV1,
        runtime_request: ArtifactBoundManagedModelAgentStackApplyRequestV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if self.phase != ArtifactExternalControllerPhaseV2::Admitted {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidPhase);
        }
        validate_artifact_runtime_prefix(
            &self.request,
            &self.admission,
            &plan_content,
            &execution,
            &runtime_request,
        )?;
        let progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(*runtime_request.target_slice_digest().value()),
            None,
            None,
            None,
        )?;
        let committed = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Committed,
            NonZeroU64::new(1).expect("one is nonzero"),
            &self.request,
            &self.admission,
            progress,
            None,
        )?;
        Self::try_new(ArtifactExternalControllerStateInputV2 {
            phase: ArtifactExternalControllerPhaseV2::Committed,
            controller_snapshot_sequence: NonZeroU64::new(2).expect("two is nonzero"),
            request: self.request.clone(),
            admission: self.admission.clone(),
            plan_content: Some(plan_content),
            execution: Some(execution),
            runtime_request: Some(runtime_request),
            runtime_terminal: None,
            records: vec![committed],
            receipt: None,
        })
    }

    pub(crate) fn begin_apply(
        &self,
        lifecycle_generation: [u8; 16],
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if self.phase != ArtifactExternalControllerPhaseV2::Committed || self.records.len() != 1 {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidPhase);
        }
        let runtime_request = self
            .runtime_request
            .as_ref()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(*runtime_request.target_slice_digest().value()),
            Some(runtime_request.envelope_request_digest()),
            None,
            Some(lifecycle_generation),
        )?;
        let applying = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Applying,
            NonZeroU64::new(2).expect("two is nonzero"),
            &self.request,
            &self.admission,
            progress,
            self.records.last(),
        )?;
        let mut records = self.records.clone();
        records.push(applying);
        Self::try_new(ArtifactExternalControllerStateInputV2 {
            phase: ArtifactExternalControllerPhaseV2::Applying,
            controller_snapshot_sequence: NonZeroU64::new(3).expect("three is nonzero"),
            request: self.request.clone(),
            admission: self.admission.clone(),
            plan_content: self.plan_content.clone(),
            execution: self.execution.clone(),
            runtime_request: self.runtime_request.clone(),
            runtime_terminal: None,
            records,
            receipt: None,
        })
    }

    pub(crate) fn finish_apply(
        &self,
        runtime_terminal: Option<ManagedModelAgentStackTerminalReceiptV1>,
        missing_terminal_phase: Option<ArtifactExternalControllerPhaseV2>,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if self.phase != ArtifactExternalControllerPhaseV2::Applying || self.records.len() != 2 {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidPhase);
        }
        let phase = match runtime_terminal
            .as_ref()
            .map(|terminal| terminal.facts().state().outcome())
        {
            Some(ManagedModelAgentStackTerminalOutcomeV1::ActiveReady) => {
                ArtifactExternalControllerPhaseV2::ActiveReady
            }
            Some(
                ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected
                | ManagedModelAgentStackTerminalOutcomeV1::Quarantined,
            ) => ArtifactExternalControllerPhaseV2::Failed,
            Some(ManagedModelAgentStackTerminalOutcomeV1::Uncertain) => {
                ArtifactExternalControllerPhaseV2::Uncertain
            }
            Some(ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero) => {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            None => match missing_terminal_phase {
                Some(
                    phase @ (ArtifactExternalControllerPhaseV2::Failed
                    | ArtifactExternalControllerPhaseV2::Uncertain),
                ) => phase,
                _ => return Err(ManagedModelAgentStackApplyControllerError::InvalidState),
            },
        };
        if runtime_terminal.is_some() && missing_terminal_phase.is_some() {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let runtime_request = self
            .runtime_request
            .as_ref()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(*runtime_request.target_slice_digest().value()),
            Some(runtime_request.envelope_request_digest()),
            runtime_terminal
                .as_ref()
                .map(ManagedModelAgentStackTerminalReceiptV1::receipt_digest),
            self.records
                .last()
                .map(|record| record.progress.lifecycle_generation),
        )?;
        let record_state = match phase {
            ArtifactExternalControllerPhaseV2::ActiveReady => {
                ArtifactExternalDeploymentRecordStateV1::ActiveReady
            }
            ArtifactExternalControllerPhaseV2::Failed => {
                ArtifactExternalDeploymentRecordStateV1::Failed
            }
            ArtifactExternalControllerPhaseV2::Uncertain => {
                ArtifactExternalDeploymentRecordStateV1::Uncertain
            }
            ArtifactExternalControllerPhaseV2::Admitted
            | ArtifactExternalControllerPhaseV2::Committed
            | ArtifactExternalControllerPhaseV2::Applying => {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        };
        let terminal = ArtifactExternalDeploymentRecordV1::try_new(
            record_state,
            NonZeroU64::new(3).expect("three is nonzero"),
            &self.request,
            &self.admission,
            progress,
            self.records.last(),
        )?;
        let receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("one is nonzero"),
            &self.request,
            &self.admission,
            &terminal,
        )?;
        let mut records = self.records.clone();
        records.push(terminal);
        Self::try_new(ArtifactExternalControllerStateInputV2 {
            phase,
            controller_snapshot_sequence: NonZeroU64::new(4).expect("four is nonzero"),
            request: self.request.clone(),
            admission: self.admission.clone(),
            plan_content: self.plan_content.clone(),
            execution: self.execution.clone(),
            runtime_request: self.runtime_request.clone(),
            runtime_terminal,
            records,
            receipt: Some(receipt),
        })
    }

    pub(crate) fn cutover_marker_digest(
        &self,
    ) -> Result<Digest32, ManagedModelAgentStackApplyControllerError> {
        artifact_external_cutover_marker_digest(&self.request, &self.admission)
    }

    pub(crate) fn encode(&self) -> Result<Box<[u8]>, ManagedModelAgentStackApplyControllerError> {
        self.validate()?;
        let plan_content = self
            .plan_content
            .as_ref()
            .map_or(&[][..], |value| value.canonical_bytes());
        let execution = self
            .execution
            .as_ref()
            .map_or(&[][..], |value| value.canonical_wire());
        let runtime_request = self
            .runtime_request
            .as_ref()
            .map_or(&[][..], |value| value.canonical_wire());
        let runtime_terminal = self
            .runtime_terminal
            .as_ref()
            .map_or(&[][..], |value| value.canonical_wire());
        let record_bytes = self
            .records
            .len()
            .checked_mul(EXTERNAL_RECORD_BYTES)
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let receipt_bytes = usize::from(self.receipt.is_some())
            .checked_mul(EXTERNAL_RECEIPT_BYTES)
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let body_len = EXTERNAL_REQUEST_BYTES
            .checked_add(EXTERNAL_ADMISSION_BYTES)
            .and_then(|value| value.checked_add(plan_content.len()))
            .and_then(|value| value.checked_add(execution.len()))
            .and_then(|value| value.checked_add(runtime_request.len()))
            .and_then(|value| value.checked_add(runtime_terminal.len()))
            .and_then(|value| value.checked_add(record_bytes))
            .and_then(|value| value.checked_add(receipt_bytes))
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let frame_len = ARTIFACT_STATE_V2_HEADER_BYTES
            .checked_add(body_len)
            .and_then(|value| value.checked_add(ARTIFACT_STATE_V2_CHECKSUM_BYTES))
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        if frame_len > MAX_ARTIFACT_STATE_V2_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::StateTooLarge);
        }
        let plan_content_len = u32::try_from(plan_content.len())
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let execution_len = u32::try_from(execution.len())
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let runtime_request_len = u32::try_from(runtime_request.len())
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let runtime_terminal_len = u32::try_from(runtime_terminal.len())
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let record_count = u16::try_from(self.records.len())
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let receipt_count = if self.receipt.is_some() { 1_u16 } else { 0 };
        let body_len_u32 = u32::try_from(body_len)
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let frame_len_u32 = u32::try_from(frame_len)
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let mut wire = Vec::with_capacity(frame_len);
        wire.extend_from_slice(b"PXMJ");
        wire.extend_from_slice(&2_u16.to_be_bytes());
        wire.extend_from_slice(&(ARTIFACT_STATE_V2_HEADER_BYTES as u16).to_be_bytes());
        wire.extend_from_slice(&frame_len_u32.to_be_bytes());
        wire.push(self.phase.phase_byte());
        wire.extend_from_slice(&[0; 3]);
        wire.extend_from_slice(&self.controller_snapshot_sequence.get().to_be_bytes());
        wire.extend_from_slice(self.admission.controller_store_instance());
        wire.extend_from_slice(&self.admission.admission_sequence().get().to_be_bytes());
        wire.extend_from_slice(
            &self
                .receipt
                .as_ref()
                .map_or(0, |value| value.receipt_sequence().get())
                .to_be_bytes(),
        );
        if let Some(runtime_request) = &self.runtime_request {
            wire.extend_from_slice(&1_u64.to_be_bytes());
            let ExpectedActive::Exact(predecessor) = runtime_request
                .control_commitment()
                .control()
                .expected_active()
            else {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            };
            wire.extend_from_slice(predecessor.value().as_bytes());
            wire.extend_from_slice(
                self.plan_content
                    .as_ref()
                    .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?
                    .digest()
                    .value()
                    .as_bytes(),
            );
        } else {
            wire.extend_from_slice(&[0; 72]);
        }
        wire.extend_from_slice(&(EXTERNAL_REQUEST_BYTES as u32).to_be_bytes());
        wire.extend_from_slice(&(EXTERNAL_ADMISSION_BYTES as u32).to_be_bytes());
        wire.extend_from_slice(&plan_content_len.to_be_bytes());
        wire.extend_from_slice(&execution_len.to_be_bytes());
        wire.extend_from_slice(&runtime_request_len.to_be_bytes());
        wire.extend_from_slice(&runtime_terminal_len.to_be_bytes());
        wire.extend_from_slice(&record_count.to_be_bytes());
        wire.extend_from_slice(&receipt_count.to_be_bytes());
        wire.extend_from_slice(&body_len_u32.to_be_bytes());
        wire.extend_from_slice(&[0; 16]);
        if wire.len() != ARTIFACT_STATE_V2_HEADER_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        wire.extend_from_slice(self.request.canonical_wire());
        wire.extend_from_slice(self.admission.canonical_wire());
        wire.extend_from_slice(plan_content);
        wire.extend_from_slice(execution);
        wire.extend_from_slice(runtime_request);
        wire.extend_from_slice(runtime_terminal);
        for record in &self.records {
            wire.extend_from_slice(record.canonical_wire());
        }
        if let Some(receipt) = &self.receipt {
            wire.extend_from_slice(receipt.canonical_wire());
        }
        let checksum = artifact_state_v2_checksum(&wire)?;
        wire.extend_from_slice(checksum.as_bytes());
        Ok(wire.into_boxed_slice())
    }

    pub(crate) fn decode(frame: &[u8]) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if frame.len() < ARTIFACT_STATE_V2_HEADER_BYTES + ARTIFACT_STATE_V2_CHECKSUM_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::StateTruncated);
        }
        if frame.len() > MAX_ARTIFACT_STATE_V2_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::StateTooLarge);
        }
        if frame.get(0..4) != Some(b"PXMJ".as_slice())
            || read_u16_at(frame, 4) != Some(2)
            || read_u16_at(frame, 6) != Some(ARTIFACT_STATE_V2_HEADER_BYTES as u16)
            || read_u32_at(frame, 8) != u32::try_from(frame.len()).ok()
            || frame[13..16] != [0; 3]
            || frame[176..192].iter().any(|byte| *byte != 0)
            || read_u32_at(frame, 144) != Some(EXTERNAL_REQUEST_BYTES as u32)
            || read_u32_at(frame, 148) != Some(EXTERNAL_ADMISSION_BYTES as u32)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let phase = ArtifactExternalControllerPhaseV2::decode(frame[12])?;
        let controller_snapshot_sequence = NonZeroU64::new(
            read_u64_at(frame, 16)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        let plan_content_len = usize::try_from(
            read_u32_at(frame, 152)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let execution_len = usize::try_from(
            read_u32_at(frame, 156)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let runtime_request_len = usize::try_from(
            read_u32_at(frame, 160)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let runtime_terminal_len = usize::try_from(
            read_u32_at(frame, 164)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
        .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let record_count = usize::from(
            read_u16_at(frame, 168)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        );
        let receipt_count = usize::from(
            read_u16_at(frame, 170)
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        );
        if plan_content_len > MAX_ARTIFACT_PLAN_CONTENT_V2_BYTES
            || execution_len > MAX_ARTIFACT_TARGET_EXECUTION_V11_BYTES
            || runtime_request_len > MAX_ARTIFACT_APPLY_REQUEST_V12_BYTES
            || runtime_terminal_len > MAX_ARTIFACT_TERMINAL_RECEIPT_V1_BYTES
            || record_count > 3
            || receipt_count > 1
        {
            return Err(ManagedModelAgentStackApplyControllerError::StateTooLarge);
        }
        let record_bytes = record_count
            .checked_mul(EXTERNAL_RECORD_BYTES)
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let receipt_bytes = receipt_count
            .checked_mul(EXTERNAL_RECEIPT_BYTES)
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let body_len = EXTERNAL_REQUEST_BYTES
            .checked_add(EXTERNAL_ADMISSION_BYTES)
            .and_then(|value| value.checked_add(plan_content_len))
            .and_then(|value| value.checked_add(execution_len))
            .and_then(|value| value.checked_add(runtime_request_len))
            .and_then(|value| value.checked_add(runtime_terminal_len))
            .and_then(|value| value.checked_add(record_bytes))
            .and_then(|value| value.checked_add(receipt_bytes))
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        if read_u32_at(frame, 172) != u32::try_from(body_len).ok()
            || ARTIFACT_STATE_V2_HEADER_BYTES
                .checked_add(body_len)
                .and_then(|value| value.checked_add(ARTIFACT_STATE_V2_CHECKSUM_BYTES))
                != Some(frame.len())
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let checksum_offset = frame.len() - ARTIFACT_STATE_V2_CHECKSUM_BYTES;
        let checksum = Digest32::from_bytes(
            frame[checksum_offset..]
                .try_into()
                .map_err(|_| ManagedModelAgentStackApplyControllerError::InvalidState)?,
        );
        if artifact_state_v2_checksum(&frame[..checksum_offset])? != checksum {
            return Err(ManagedModelAgentStackApplyControllerError::StateChecksumMismatch);
        }
        let mut cursor = Cursor {
            frame: &frame[ARTIFACT_STATE_V2_HEADER_BYTES..checksum_offset],
            position: 0,
        };
        let request =
            ArtifactExternalDeploymentRequestV1::decode(cursor.take(EXTERNAL_REQUEST_BYTES)?)?;
        let admission = ArtifactExternalDeploymentAdmissionV1::decode(
            cursor.take(EXTERNAL_ADMISSION_BYTES)?,
            &request,
        )?;
        let plan_content_wire = cursor.take(plan_content_len)?;
        let execution_wire = cursor.take(execution_len)?;
        let runtime_request_wire = cursor.take(runtime_request_len)?;
        let runtime_terminal_wire = cursor.take(runtime_terminal_len)?;
        let (plan_content, execution, runtime_request) =
            if plan_content_len == 0 && execution_len == 0 && runtime_request_len == 0 {
                (None, None, None)
            } else {
                if plan_content_len == 0 || execution_len == 0 || runtime_request_len == 0 {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
                let execution =
                    ArtifactBoundManagedModelAgentStackTargetExecutionV1::decode(execution_wire)?;
                let plan_content = ArtifactBoundManagedModelAgentStackPlanContentV2::decode(
                    execution.projection().target(),
                    plan_content_wire,
                )?;
                let runtime_request = ArtifactBoundManagedModelAgentStackApplyRequestV1::decode(
                    runtime_request_wire,
                )?;
                (Some(plan_content), Some(execution), Some(runtime_request))
            };
        let runtime_terminal = if runtime_terminal_wire.is_empty() {
            None
        } else {
            Some(ManagedModelAgentStackTerminalReceiptV1::decode(
                runtime_terminal_wire,
            )?)
        };
        let mut records = Vec::with_capacity(record_count);
        for _ in 0..record_count {
            let previous = records.last();
            records.push(ArtifactExternalDeploymentRecordV1::decode(
                cursor.take(EXTERNAL_RECORD_BYTES)?,
                &request,
                &admission,
                previous,
            )?);
        }
        let receipt = if receipt_count == 0 {
            None
        } else {
            let terminal = records
                .last()
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
            Some(ArtifactExternalDeploymentReceiptV1::decode(
                cursor.take(EXTERNAL_RECEIPT_BYTES)?,
                &request,
                &admission,
                terminal,
            )?)
        };
        cursor.finish()?;
        let decoded = Self::try_new(ArtifactExternalControllerStateInputV2 {
            phase,
            controller_snapshot_sequence,
            request,
            admission,
            plan_content,
            execution,
            runtime_request,
            runtime_terminal,
            records,
            receipt,
        })?;
        if decoded.encode()?.as_ref() != frame {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(decoded)
    }

    fn validate(&self) -> Result<(), ManagedModelAgentStackApplyControllerError> {
        validate_request_admission(&self.request, &self.admission)?;
        if self.admission.admission_sequence().get() != 1 || self.records.len() > 3 {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let plan_presence = [
            self.plan_content.is_some(),
            self.execution.is_some(),
            self.runtime_request.is_some(),
        ];
        if plan_presence.iter().any(|present| *present)
            && plan_presence.iter().any(|present| !*present)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        if let (Some(plan_content), Some(execution), Some(runtime_request)) =
            (&self.plan_content, &self.execution, &self.runtime_request)
        {
            validate_artifact_runtime_prefix(
                &self.request,
                &self.admission,
                plan_content,
                execution,
                runtime_request,
            )?;
        } else if self.runtime_terminal.is_some() {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        if let (Some(runtime_request), Some(runtime_terminal)) =
            (&self.runtime_request, &self.runtime_terminal)
        {
            runtime_terminal.validate_artifact_request_correlation(runtime_request)?;
        }
        let mut previous = None;
        for record in &self.records {
            validate_record_owner(record, &self.request, &self.admission)?;
            if ArtifactExternalDeploymentRecordV1::decode(
                record.canonical_wire(),
                &self.request,
                &self.admission,
                previous,
            )? != *record
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            validate_artifact_record_progress(
                record,
                self.runtime_request.as_ref(),
                self.runtime_terminal.as_ref(),
            )?;
            previous = Some(record);
        }
        if let Some(receipt) = &self.receipt {
            let terminal = self
                .records
                .last()
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
            if receipt.receipt_sequence().get() != 1
                || ArtifactExternalDeploymentReceiptV1::decode(
                    receipt.canonical_wire(),
                    &self.request,
                    &self.admission,
                    terminal,
                )? != *receipt
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        validate_artifact_state_shape(self)
    }
}

fn artifact_external_cutover_marker_digest(
    request: &ArtifactExternalDeploymentRequestV1,
    admission: &ArtifactExternalDeploymentAdmissionV1,
) -> Result<Digest32, ManagedModelAgentStackApplyControllerError> {
    validate_request_admission(request, admission)?;
    let mut digest = Digest32Builder::try_new(ARTIFACT_CUTOVER_MARKER_DIGEST_DOMAIN)?;
    digest.field_bytes(admission.controller_store_instance())?;
    digest.field_u64(admission.admission_sequence().get())?;
    digest.field_digest(&request.request_digest())?;
    digest.field_digest(&admission.admission_digest())?;
    let digest = digest.finish();
    require_nonzero_digest(digest)?;
    Ok(digest)
}

fn validate_artifact_runtime_prefix(
    deployment_request: &ArtifactExternalDeploymentRequestV1,
    admission: &ArtifactExternalDeploymentAdmissionV1,
    plan_content: &ArtifactBoundManagedModelAgentStackPlanContentV2,
    execution: &ArtifactBoundManagedModelAgentStackTargetExecutionV1,
    runtime_request: &ArtifactBoundManagedModelAgentStackApplyRequestV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    if plan_content.binding() != deployment_request.binding()
        || execution.binding() != deployment_request.binding()
        || plan_content.execution() != execution
        || runtime_request.target_execution() != execution
        || plan_content.target() != runtime_request.target()
    {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    let ExpectedActive::Exact(predecessor) = runtime_request
        .control_commitment()
        .control()
        .expected_active()
    else {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    };
    require_nonzero_digest(*predecessor.value())?;
    let marker = artifact_external_cutover_marker_digest(deployment_request, admission)?;
    let provenance = runtime_request.provenance();
    let mut digest = Digest32Builder::try_new(ARTIFACT_DESIRED_DIGEST_DOMAIN)?;
    digest.field_digest(&marker)?;
    digest.field_bytes(runtime_request.target().as_bytes())?;
    digest.field_bytes(provenance.source_scope().as_bytes())?;
    digest.field_bytes(provenance.source_plan().as_bytes())?;
    digest.field_u64(provenance.source_revision().value())?;
    digest.field_bytes(predecessor.value().as_bytes())?;
    digest.field_digest(&deployment_request.request_digest())?;
    digest.field_digest(&admission.admission_digest())?;
    digest.field_digest(&plan_content.digest().value())?;
    digest.field_bytes(execution.canonical_wire())?;
    if digest.finish() != *provenance.source_plan_digest().value() {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn validate_artifact_record_progress(
    record: &ArtifactExternalDeploymentRecordV1,
    runtime_request: Option<&ArtifactBoundManagedModelAgentStackApplyRequestV1>,
    runtime_terminal: Option<&ManagedModelAgentStackTerminalReceiptV1>,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    let progress = record.progress();
    let Some(runtime_request) = runtime_request else {
        if progress.deployment_revision != 0
            || progress.controller_snapshot_sequence != 0
            || !all_zero_32(progress.desired_head_digest)
            || !all_zero_32(progress.runtime_apply_request_digest)
            || !all_zero_32(progress.runtime_terminal_receipt_digest)
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        return Ok(());
    };
    if progress.deployment_revision != 1
        || progress.controller_snapshot_sequence != 2
        || progress.desired_head_digest != *runtime_request.target_slice_digest().value().as_bytes()
        || (!all_zero_32(progress.runtime_apply_request_digest)
            && progress.runtime_apply_request_digest
                != *runtime_request.envelope_request_digest().as_bytes())
    {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    match runtime_terminal {
        Some(terminal) => {
            if record.state().is_terminal()
                && progress.runtime_terminal_receipt_digest != *terminal.receipt_digest().as_bytes()
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            if !record.state().is_terminal()
                && !all_zero_32(progress.runtime_terminal_receipt_digest)
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        None if !all_zero_32(progress.runtime_terminal_receipt_digest) => {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        None => {}
    }
    Ok(())
}

fn validate_artifact_state_shape(
    state: &ArtifactExternalControllerStateV2,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    let record_states: Vec<_> = state.records.iter().map(|record| record.state()).collect();
    let plan_present = state.runtime_request.is_some();
    let sequence = state.controller_snapshot_sequence.get();
    match state.phase {
        ArtifactExternalControllerPhaseV2::Admitted => {
            if sequence != 1
                || plan_present
                || state.runtime_terminal.is_some()
                || !state.records.is_empty()
                || state.receipt.is_some()
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalControllerPhaseV2::Committed => {
            if sequence != 2
                || !plan_present
                || state.runtime_terminal.is_some()
                || record_states != [ArtifactExternalDeploymentRecordStateV1::Committed]
                || state.receipt.is_some()
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalControllerPhaseV2::Applying => {
            if sequence != 3
                || !plan_present
                || state.runtime_terminal.is_some()
                || record_states
                    != [
                        ArtifactExternalDeploymentRecordStateV1::Committed,
                        ArtifactExternalDeploymentRecordStateV1::Applying,
                    ]
                || state.receipt.is_some()
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalControllerPhaseV2::ActiveReady => {
            if sequence != 4
                || !plan_present
                || record_states
                    != [
                        ArtifactExternalDeploymentRecordStateV1::Committed,
                        ArtifactExternalDeploymentRecordStateV1::Applying,
                        ArtifactExternalDeploymentRecordStateV1::ActiveReady,
                    ]
                || state.receipt.is_none()
                || state
                    .runtime_terminal
                    .as_ref()
                    .map(|terminal| terminal.facts().state().outcome())
                    != Some(ManagedModelAgentStackTerminalOutcomeV1::ActiveReady)
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalControllerPhaseV2::Failed
        | ArtifactExternalControllerPhaseV2::Uncertain => {
            let terminal_state = if state.phase == ArtifactExternalControllerPhaseV2::Failed {
                ArtifactExternalDeploymentRecordStateV1::Failed
            } else {
                ArtifactExternalDeploymentRecordStateV1::Uncertain
            };
            let valid_prefix = match record_states.as_slice() {
                [terminal] if *terminal == terminal_state => sequence == 2 && !plan_present,
                [ArtifactExternalDeploymentRecordStateV1::Committed, terminal]
                    if *terminal == terminal_state =>
                {
                    sequence == 3 && plan_present
                }
                [
                    ArtifactExternalDeploymentRecordStateV1::Committed,
                    ArtifactExternalDeploymentRecordStateV1::Applying,
                    terminal,
                ] if *terminal == terminal_state => sequence == 4 && plan_present,
                _ => false,
            };
            if !valid_prefix || state.receipt.is_none() {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            if record_states.len() < 3 && state.runtime_terminal.is_some() {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            if let Some(runtime_terminal) = &state.runtime_terminal {
                let expected_phase = match runtime_terminal.facts().state().outcome() {
                    ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected
                    | ManagedModelAgentStackTerminalOutcomeV1::Quarantined => {
                        ArtifactExternalControllerPhaseV2::Failed
                    }
                    ManagedModelAgentStackTerminalOutcomeV1::Uncertain => {
                        ArtifactExternalControllerPhaseV2::Uncertain
                    }
                    ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
                    | ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero => {
                        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                    }
                };
                if state.phase != expected_phase {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
            }
        }
    }
    if state.receipt.is_some()
        && state
            .records
            .last()
            .is_none_or(|record| record.state().state_byte() != state.phase.phase_byte())
    {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn artifact_state_v2_checksum(
    frame_without_checksum: &[u8],
) -> Result<Digest32, ManagedModelAgentStackApplyControllerError> {
    let mut checksum = Digest32Builder::try_new(ARTIFACT_STATE_V2_CHECKSUM_DOMAIN)?;
    checksum.field_bytes(frame_without_checksum)?;
    let checksum = checksum.finish();
    require_nonzero_digest(checksum)?;
    Ok(checksum)
}

fn validate_request_admission(
    request: &ArtifactExternalDeploymentRequestV1,
    admission: &ArtifactExternalDeploymentAdmissionV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    if admission.operation_id() != request.operation_id()
        || admission.request_digest() != request.request_digest()
        || admission.object_ref() != request.binding().object_ref()
    {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn validate_record_owner(
    record: &ArtifactExternalDeploymentRecordV1,
    request: &ArtifactExternalDeploymentRequestV1,
    admission: &ArtifactExternalDeploymentAdmissionV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    let frame = record.canonical_wire();
    if frame[16..48] != admission.controller_store_instance()[..]
        || frame[56..72] != request.operation_id().as_bytes()[..]
        || frame[72..104] != request.request_digest().as_bytes()[..]
        || frame[104..136] != admission.admission_digest().as_bytes()[..]
        || frame[136..208] != request.binding().object_ref().encode()
        || frame[208..240]
            != request
                .binding()
                .materialization_receipt_ref()
                .receipt_digest()
                .as_bytes()[..]
        || frame[368..400] != request.binding().execution_profile_commitment().as_bytes()[..]
    {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn validate_receipt_owner(
    receipt: &ArtifactExternalDeploymentReceiptV1,
    request: &ArtifactExternalDeploymentRequestV1,
    admission: &ArtifactExternalDeploymentAdmissionV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    let frame = receipt.canonical_wire();
    if frame[16..48] != admission.controller_store_instance()[..]
        || frame[56..72] != request.operation_id().as_bytes()[..]
        || frame[72..104] != request.request_digest().as_bytes()[..]
        || frame[136..208] != request.binding().object_ref().encode()
        || frame[208..240]
            != request
                .binding()
                .materialization_receipt_ref()
                .receipt_digest()
                .as_bytes()[..]
    {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn validate_record_transition(
    state: ArtifactExternalDeploymentRecordStateV1,
    record_sequence: NonZeroU64,
    progress: ArtifactExternalDeploymentProgressV1,
    previous: Option<&ArtifactExternalDeploymentRecordV1>,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    match state {
        ArtifactExternalDeploymentRecordStateV1::Committed => {
            if record_sequence.get() != 1
                || previous.is_some()
                || !progress.is_committed()
                || progress.has_runtime_request()
                || progress.has_runtime_terminal()
                || progress.has_lifecycle_generation()
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalDeploymentRecordStateV1::Applying => {
            let previous = previous
                .filter(|record| {
                    record.state() == ArtifactExternalDeploymentRecordStateV1::Committed
                })
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
            validate_record_successor_sequence(record_sequence, previous)?;
            progress.preserves(previous.progress())?;
            if !progress.is_applying() || progress.has_runtime_terminal() {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalDeploymentRecordStateV1::ActiveReady => {
            let previous = previous
                .filter(|record| {
                    record.state() == ArtifactExternalDeploymentRecordStateV1::Applying
                })
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
            validate_record_successor_sequence(record_sequence, previous)?;
            progress.preserves(previous.progress())?;
            if !progress.is_applying() || !progress.has_runtime_terminal() {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
        }
        ArtifactExternalDeploymentRecordStateV1::Failed
        | ArtifactExternalDeploymentRecordStateV1::Uncertain => {
            if let Some(previous) = previous {
                if previous.state().is_terminal() {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
                validate_record_successor_sequence(record_sequence, previous)?;
                progress.preserves(previous.progress())?;
            } else {
                if record_sequence.get() != 1
                    || progress.is_committed()
                    || progress.has_runtime_request()
                    || progress.has_runtime_terminal()
                {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
            }
        }
    }
    Ok(())
}

fn validate_record_successor_sequence(
    record_sequence: NonZeroU64,
    previous: &ArtifactExternalDeploymentRecordV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    let expected = previous
        .record_sequence()
        .get()
        .checked_add(1)
        .ok_or(ManagedModelAgentStackApplyControllerError::SequenceExhausted)?;
    if record_sequence.get() != expected {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn optional_nonzero_digest(
    digest: Option<Digest32>,
) -> Result<[u8; 32], ManagedModelAgentStackApplyControllerError> {
    match digest {
        Some(digest) if digest.as_bytes().iter().all(|byte| *byte == 0) => {
            Err(ManagedModelAgentStackApplyControllerError::InvalidState)
        }
        Some(digest) => Ok(*digest.as_bytes()),
        None => Ok([0; 32]),
    }
}

fn require_nonzero_digest(
    digest: Digest32,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    if digest.as_bytes().iter().all(|byte| *byte == 0) {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    Ok(())
}

fn optional_digest_from_bytes(bytes: [u8; 32]) -> Option<Digest32> {
    (!bytes.iter().all(|byte| *byte == 0)).then(|| Digest32::from_bytes(bytes))
}

const fn all_zero_32(bytes: [u8; 32]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

const fn all_zero_16(bytes: [u8; 16]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

fn preserves_optional_u64(current: u64, previous: u64) -> bool {
    previous == 0 || current == previous
}

fn preserves_optional_bytes<const N: usize>(current: [u8; N], previous: [u8; N]) -> bool {
    previous.iter().all(|byte| *byte == 0) || current == previous
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn decode_lower_hex_exact<const N: usize>(
    value: &str,
) -> Result<[u8; N], ManagedModelAgentStackApplyControllerError> {
    if value.len() != N * 2 || !value.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
    }
    let mut output = [0_u8; N];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = decode_lower_hex_nibble(chunk[0])?
            .checked_mul(16)
            .and_then(|high| high.checked_add(decode_lower_hex_nibble(chunk[1]).ok()?))
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
    }
    Ok(output)
}

fn decode_lower_hex_nibble(value: u8) -> Result<u8, ManagedModelAgentStackApplyControllerError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(ManagedModelAgentStackApplyControllerError::InvalidState),
    }
}

fn raw_sha256(domain: &[u8], frame: &[u8]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(frame);
    Digest32::from_bytes(hasher.finalize().into())
}

fn read_u16_at(frame: &[u8], offset: usize) -> Option<u16> {
    frame
        .get(offset..offset.checked_add(2)?)?
        .try_into()
        .ok()
        .map(u16::from_be_bytes)
}

fn read_u32_at(frame: &[u8], offset: usize) -> Option<u32> {
    frame
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()
        .map(u32::from_be_bytes)
}

fn read_u64_at(frame: &[u8], offset: usize) -> Option<u64> {
    frame
        .get(offset..offset.checked_add(8)?)?
        .try_into()
        .ok()
        .map(u64::from_be_bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedModelAgentStackApplyPhaseV1 {
    RequestDurableNotSent,
    Uncertain,
    ReceiptDurable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedModelAgentStackControllerStateV1 {
    phase: ManagedModelAgentStackApplyPhaseV1,
    desired: ManagedModelAgentStackDesiredPlanV1,
    request: ManagedModelAgentStackApplyRequestV1,
    receipt: Option<ManagedModelAgentStackTerminalReceiptV1>,
    archived_active: Option<CompletedManagedModelAgentStackApplyV1>,
}

pub(crate) struct ManagedModelAgentStackDecodeContextV1<'a> {
    pub(crate) fabric: &'a VerifiedManagedFabricProducerContextV1,
    pub(crate) cutover_marker_digest: Digest32,
    pub(crate) predecessor_revision: SourcePlanRevision,
    pub(crate) predecessor_execution: &'a ManagedFabricTargetExecutionV1,
    pub(crate) predecessor_slice_digest: TargetSliceDigest,
    pub(crate) predecessor_generation: ManagedServiceGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompletedManagedModelAgentStackApplyV1 {
    desired: ManagedModelAgentStackDesiredPlanV1,
    request: ManagedModelAgentStackApplyRequestV1,
    receipt: ManagedModelAgentStackTerminalReceiptV1,
}

impl CompletedManagedModelAgentStackApplyV1 {
    #[must_use]
    pub(crate) const fn desired(&self) -> &ManagedModelAgentStackDesiredPlanV1 {
        &self.desired
    }

    #[must_use]
    pub(crate) const fn request(&self) -> &ManagedModelAgentStackApplyRequestV1 {
        &self.request
    }

    #[must_use]
    pub(crate) const fn receipt(&self) -> &ManagedModelAgentStackTerminalReceiptV1 {
        &self.receipt
    }
}

impl ManagedModelAgentStackControllerStateV1 {
    #[must_use]
    pub(crate) const fn phase(&self) -> ManagedModelAgentStackApplyPhaseV1 {
        self.phase
    }

    #[must_use]
    pub(crate) const fn desired(&self) -> &ManagedModelAgentStackDesiredPlanV1 {
        &self.desired
    }

    #[must_use]
    pub(crate) const fn request(&self) -> &ManagedModelAgentStackApplyRequestV1 {
        &self.request
    }

    #[must_use]
    pub(crate) const fn receipt(&self) -> Option<&ManagedModelAgentStackTerminalReceiptV1> {
        self.receipt.as_ref()
    }

    #[must_use]
    pub(crate) const fn archived_active(&self) -> Option<&CompletedManagedModelAgentStackApplyV1> {
        self.archived_active.as_ref()
    }

    pub(crate) fn try_prepared(
        desired: ManagedModelAgentStackDesiredPlanV1,
        request: ManagedModelAgentStackApplyRequestV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if desired.execution().mode() != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
            || request.target_execution() != desired.execution()
            || request.provenance() != desired.provenance()
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(Self {
            phase: ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent,
            desired,
            request,
            receipt: None,
            archived_active: None,
        })
    }

    pub(crate) fn try_prepare_empty(
        &self,
        desired: ManagedModelAgentStackDesiredPlanV1,
        request: ManagedModelAgentStackApplyRequestV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let active_receipt = self
            .receipt
            .as_ref()
            .ok_or(ManagedModelAgentStackApplyControllerError::ModelAgentNotActive)?;
        if self.phase != ManagedModelAgentStackApplyPhaseV1::ReceiptDurable
            || self.archived_active.is_some()
            || self.desired.execution().mode()
                != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
            || active_receipt.facts().state().outcome()
                != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
            || desired.execution().mode() != ManagedModelAgentStackTargetModeV1::EmptyDeactivate
            || desired.predecessor_slice_digest() != self.request.target_slice_digest()
            || request.target_execution() != desired.execution()
            || request.provenance() != desired.provenance()
        {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        Ok(Self {
            phase: ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent,
            desired,
            request,
            receipt: None,
            archived_active: Some(CompletedManagedModelAgentStackApplyV1 {
                desired: self.desired.clone(),
                request: self.request.clone(),
                receipt: active_receipt.clone(),
            }),
        })
    }

    pub(crate) fn try_claim(&self) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if self.phase != ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
            || self.receipt.is_some()
        {
            return Err(ManagedModelAgentStackApplyControllerError::OpaqueReplayForbidden);
        }
        let mut next = self.clone();
        next.phase = ManagedModelAgentStackApplyPhaseV1::Uncertain;
        Ok(next)
    }

    /// Retains every already-authenticated legal PXMT, not only success.
    pub(crate) fn try_terminal(
        &self,
        receipt: ManagedModelAgentStackTerminalReceiptV1,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        if self.phase != ManagedModelAgentStackApplyPhaseV1::Uncertain || self.receipt.is_some() {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidPhase);
        }
        let mut next = self.clone();
        next.phase = ManagedModelAgentStackApplyPhaseV1::ReceiptDurable;
        next.receipt = Some(receipt);
        Ok(next)
    }

    /// Pure transition predicate used by the outer PXFJ owner.
    #[must_use]
    pub(crate) fn is_valid_transition_from(&self, current: Option<&Self>) -> bool {
        match current {
            None => {
                self.phase == ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
                    && self.receipt.is_none()
                    && self.archived_active.is_none()
                    && self.desired.execution().mode()
                        == ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
            }
            Some(current) => {
                let same_request = current.desired == self.desired
                    && current.request == self.request
                    && current.archived_active == self.archived_active;
                let starts_empty = current.phase
                    == ManagedModelAgentStackApplyPhaseV1::ReceiptDurable
                    && current.archived_active.is_none()
                    && current.receipt.as_ref().is_some_and(|receipt| {
                        receipt.facts().state().outcome()
                            == ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
                    })
                    && self.phase == ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
                    && self.receipt.is_none()
                    && self.desired.execution().mode()
                        == ManagedModelAgentStackTargetModeV1::EmptyDeactivate
                    && self.desired.predecessor_slice_digest()
                        == current.request.target_slice_digest()
                    && self.desired.revision().value()
                        == current
                            .desired
                            .revision()
                            .value()
                            .checked_add(1)
                            .unwrap_or(0)
                    && self.request.target_execution() == self.desired.execution()
                    && self.request.provenance() == self.desired.provenance()
                    && self.archived_active.as_ref().is_some_and(|archived| {
                        archived.desired == current.desired
                            && archived.request == current.request
                            && current
                                .receipt
                                .as_ref()
                                .is_some_and(|receipt| archived.receipt == *receipt)
                    });
                starts_empty
                    || same_request
                        && match (current.phase, self.phase) {
                            (
                                ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent,
                                ManagedModelAgentStackApplyPhaseV1::Uncertain,
                            ) => self.receipt.is_none(),
                            (
                                ManagedModelAgentStackApplyPhaseV1::Uncertain,
                                ManagedModelAgentStackApplyPhaseV1::ReceiptDurable,
                            ) => self.receipt.is_some(),
                            _ => false,
                        }
            }
        }
    }

    #[must_use]
    pub(crate) fn deactivation_succeeded(&self) -> bool {
        self.phase == ManagedModelAgentStackApplyPhaseV1::ReceiptDurable
            && self.desired.execution().mode()
                == ManagedModelAgentStackTargetModeV1::EmptyDeactivate
            && self.receipt.as_ref().is_some_and(|receipt| {
                receipt.facts().state().outcome()
                    == ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero
            })
    }

    pub(crate) fn encode(&self) -> Result<Box<[u8]>, ManagedModelAgentStackApplyControllerError> {
        let execution = self.desired.execution().canonical_wire();
        let request = self.request.canonical_wire();
        let receipt = self
            .receipt
            .as_ref()
            .map_or(&[][..], |value| value.canonical_wire());
        let (archived_revision, archived_execution, archived_request, archived_receipt) = self
            .archived_active
            .as_ref()
            .map_or((0, &[][..], &[][..], &[][..]), |archived| {
                (
                    archived.desired.revision().value(),
                    archived.desired.execution().canonical_wire(),
                    archived.request.canonical_wire(),
                    archived.receipt.canonical_wire(),
                )
            });
        let execution_length = wire_length(execution)?;
        let request_length = wire_length(request)?;
        let receipt_length = wire_length(receipt)?;
        let archived_execution_length = wire_length(archived_execution)?;
        let archived_request_length = wire_length(archived_request)?;
        let archived_receipt_length = wire_length(archived_receipt)?;
        let total = STATE_FIXED_BYTES
            .checked_add(execution.len())
            .and_then(|value| value.checked_add(request.len()))
            .and_then(|value| value.checked_add(receipt.len()))
            .and_then(|value| value.checked_add(archived_execution.len()))
            .and_then(|value| value.checked_add(archived_request.len()))
            .and_then(|value| value.checked_add(archived_receipt.len()))
            .and_then(|value| value.checked_add(STATE_CHECKSUM_BYTES))
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        if total > MAX_STATE_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::StateTooLarge);
        }
        let mut wire = Vec::with_capacity(total);
        wire.extend_from_slice(STATE_MAGIC);
        wire.extend_from_slice(&STATE_VERSION.to_be_bytes());
        wire.push(match self.phase {
            ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent => 1,
            ManagedModelAgentStackApplyPhaseV1::Uncertain => 2,
            ManagedModelAgentStackApplyPhaseV1::ReceiptDurable => 3,
        });
        wire.extend_from_slice(&self.desired.revision().value().to_be_bytes());
        wire.extend_from_slice(self.desired.predecessor_slice_digest().value().as_bytes());
        wire.extend_from_slice(&execution_length.to_be_bytes());
        wire.extend_from_slice(&request_length.to_be_bytes());
        wire.extend_from_slice(&receipt_length.to_be_bytes());
        wire.extend_from_slice(&archived_revision.to_be_bytes());
        wire.extend_from_slice(&archived_execution_length.to_be_bytes());
        wire.extend_from_slice(&archived_request_length.to_be_bytes());
        wire.extend_from_slice(&archived_receipt_length.to_be_bytes());
        wire.extend_from_slice(execution);
        wire.extend_from_slice(request);
        wire.extend_from_slice(receipt);
        wire.extend_from_slice(archived_execution);
        wire.extend_from_slice(archived_request);
        wire.extend_from_slice(archived_receipt);
        let checksum = state_checksum(&wire)?;
        wire.extend_from_slice(checksum.as_bytes());
        Ok(wire.into_boxed_slice())
    }

    pub(crate) fn decode(
        frame: &[u8],
        decode: ManagedModelAgentStackDecodeContextV1<'_>,
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let ManagedModelAgentStackDecodeContextV1 {
            fabric: context,
            cutover_marker_digest,
            predecessor_revision,
            predecessor_execution,
            predecessor_slice_digest,
            predecessor_generation,
        } = decode;
        if frame.len() < STATE_FIXED_BYTES + STATE_CHECKSUM_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::StateTruncated);
        }
        if frame.len() > MAX_STATE_BYTES {
            return Err(ManagedModelAgentStackApplyControllerError::StateTooLarge);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *STATE_MAGIC || cursor.u16()? != STATE_VERSION {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let phase = match cursor.u8()? {
            1 => ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent,
            2 => ManagedModelAgentStackApplyPhaseV1::Uncertain,
            3 => ManagedModelAgentStackApplyPhaseV1::ReceiptDurable,
            _ => return Err(ManagedModelAgentStackApplyControllerError::InvalidState),
        };
        let revision = cursor.u64()?;
        let encoded_predecessor = TargetSliceDigest::new(Digest32::from_bytes(cursor.array()?));
        let execution_length = cursor.usize_u32()?;
        let request_length = cursor.usize_u32()?;
        let receipt_length = cursor.usize_u32()?;
        let archived_revision = cursor.u64()?;
        let archived_execution_length = cursor.usize_u32()?;
        let archived_request_length = cursor.usize_u32()?;
        let archived_receipt_length = cursor.usize_u32()?;
        let expected = STATE_FIXED_BYTES
            .checked_add(execution_length)
            .and_then(|value| value.checked_add(request_length))
            .and_then(|value| value.checked_add(receipt_length))
            .and_then(|value| value.checked_add(archived_execution_length))
            .and_then(|value| value.checked_add(archived_request_length))
            .and_then(|value| value.checked_add(archived_receipt_length))
            .and_then(|value| value.checked_add(STATE_CHECKSUM_BYTES))
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        if expected != frame.len() {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let execution_wire = cursor.take(execution_length)?;
        let request_wire = cursor.take(request_length)?;
        let receipt_wire = cursor.take(receipt_length)?;
        let archived_execution_wire = cursor.take(archived_execution_length)?;
        let archived_request_wire = cursor.take(archived_request_length)?;
        let archived_receipt_wire = cursor.take(archived_receipt_length)?;
        let checksum = Digest32::from_bytes(cursor.array()?);
        cursor.finish()?;
        if state_checksum(&frame[..frame.len() - STATE_CHECKSUM_BYTES])? != checksum {
            return Err(ManagedModelAgentStackApplyControllerError::StateChecksumMismatch);
        }

        let archived_active = if archived_revision == 0
            && archived_execution_wire.is_empty()
            && archived_request_wire.is_empty()
            && archived_receipt_wire.is_empty()
        {
            None
        } else {
            if archived_revision == 0
                || archived_execution_wire.is_empty()
                || archived_request_wire.is_empty()
                || archived_receipt_wire.is_empty()
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            let desired = ManagedModelAgentStackDesiredPlanV1::try_restore(
                context,
                cutover_marker_digest,
                predecessor_slice_digest,
                archived_revision,
                archived_execution_wire,
            )?;
            if desired.revision().value() != successor(predecessor_revision.value())?
                || desired.execution().mode()
                    != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
                || desired.execution().managed_agent_stack().fabric() != predecessor_execution
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            let request = ManagedModelAgentStackApplyRequestV1::decode(archived_request_wire)?;
            validate_managed_model_agent_stack_request_v1(context, &desired, &request)?;
            let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(archived_receipt_wire)?;
            verify_terminal(&receipt, &request, context, predecessor_generation)?;
            if receipt.facts().state().outcome()
                != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
            {
                return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
            }
            Some(CompletedManagedModelAgentStackApplyV1 {
                desired,
                request,
                receipt,
            })
        };
        let expected_predecessor = archived_active
            .as_ref()
            .map_or(predecessor_slice_digest, |archived| {
                archived.request.target_slice_digest()
            });
        if encoded_predecessor != expected_predecessor {
            return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
        }
        let desired = ManagedModelAgentStackDesiredPlanV1::try_restore(
            context,
            cutover_marker_digest,
            expected_predecessor,
            revision,
            execution_wire,
        )?;
        let request = ManagedModelAgentStackApplyRequestV1::decode(request_wire)?;
        match archived_active.as_ref() {
            None => {
                if desired.revision().value() != successor(predecessor_revision.value())?
                    || desired.execution().mode()
                        != ManagedModelAgentStackTargetModeV1::FabricModelAndAgent
                    || desired.execution().managed_agent_stack().fabric() != predecessor_execution
                {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
                validate_managed_model_agent_stack_request_v1(context, &desired, &request)?;
            }
            Some(archived) => {
                if desired.revision().value() != successor(archived.desired.revision().value())?
                    || desired.execution().mode()
                        != ManagedModelAgentStackTargetModeV1::EmptyDeactivate
                {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
                validate_managed_model_agent_stack_empty_request_v1(
                    context,
                    &desired,
                    archived.desired.execution(),
                    &request,
                )?;
            }
        }
        let receipt = match phase {
            ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
            | ManagedModelAgentStackApplyPhaseV1::Uncertain => {
                if !receipt_wire.is_empty() {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
                None
            }
            ManagedModelAgentStackApplyPhaseV1::ReceiptDurable => {
                if receipt_wire.is_empty() {
                    return Err(ManagedModelAgentStackApplyControllerError::InvalidState);
                }
                let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(receipt_wire)?;
                verify_terminal(&receipt, &request, context, predecessor_generation)?;
                Some(receipt)
            }
        };
        Ok(Self {
            phase,
            desired,
            request,
            receipt,
            archived_active,
        })
    }
}

/// Proof that PXAR v9 bytes are durable and have never reached transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreparedManagedModelAgentStackApplyV1 {
    outer_sequence: u64,
    cutover_marker_digest: Digest32,
    request_digest: Digest32,
}

impl PreparedManagedModelAgentStackApplyV1 {
    #[must_use]
    pub(crate) const fn outer_sequence(self) -> u64 {
        self.outer_sequence
    }

    #[must_use]
    pub(crate) const fn request_digest(self) -> Digest32 {
        self.request_digest
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ManagedModelAgentStackSendActionV1 {
    outer_sequence: u64,
    cutover_marker_digest: Digest32,
    request: ManagedModelAgentStackApplyRequestV1,
    channel: ReferenceChannelBindingV1,
}

impl ManagedModelAgentStackSendActionV1 {
    #[must_use]
    pub(crate) const fn request(&self) -> &ManagedModelAgentStackApplyRequestV1 {
        &self.request
    }

    #[must_use]
    pub(crate) fn canonical_request_bytes(&self) -> &[u8] {
        self.request.canonical_wire()
    }

    #[must_use]
    pub(crate) const fn channel(&self) -> ReferenceChannelBindingV1 {
        self.channel
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedModelAgentStackTerminalCommitV1 {
    outer_sequence: u64,
    receipt: ManagedModelAgentStackTerminalReceiptV1,
    replayed_from_journal: bool,
}

impl ManagedModelAgentStackTerminalCommitV1 {
    #[must_use]
    pub(crate) const fn receipt(&self) -> &ManagedModelAgentStackTerminalReceiptV1 {
        &self.receipt
    }

    #[must_use]
    pub(crate) const fn replayed_from_journal(&self) -> bool {
        self.replayed_from_journal
    }

    #[must_use]
    pub(crate) fn is_active_success(&self) -> bool {
        self.receipt.facts().state().outcome()
            == ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
    }

    #[must_use]
    pub(crate) fn is_deactivation_success(&self) -> bool {
        self.receipt.facts().state().outcome()
            == ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedModelAgentStackApplyJournalV1 {
    state: ManagedFabricControllerStateV1,
}

impl ManagedModelAgentStackApplyJournalV1 {
    #[must_use]
    pub(crate) const fn new(state: ManagedFabricControllerStateV1) -> Self {
        Self { state }
    }

    #[must_use]
    pub(crate) const fn state(&self) -> &ManagedFabricControllerStateV1 {
        &self.state
    }

    pub(crate) fn prepared(
        &self,
        controller_signer: &ed25519_dalek::SigningKey,
        provisioning: &ManagedFabricControllerProvisioningV1,
    ) -> Result<PreparedManagedModelAgentStackApplyV1, ManagedModelAgentStackApplyControllerError>
    {
        let context = self
            .state
            .verified_current_context(controller_signer, provisioning)?;
        let stack = self
            .state
            .model_agent_stack_state()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidPhase)?;
        validate_stack_request(&context, stack)?;
        prepared_token(&self.state, stack)
    }

    pub(crate) fn prepare_activate_with<Commit>(
        &mut self,
        controller_signer: &ed25519_dalek::SigningKey,
        provisioning: &ManagedFabricControllerProvisioningV1,
        activation: &ManagedModelAgentStackActivationV1,
        fresh: FreshManagedModelAgentStackApplyV1,
        commit: Commit,
    ) -> Result<PreparedManagedModelAgentStackApplyV1, ManagedModelAgentStackApplyControllerError>
    where
        Commit: FnOnce(
            &ManagedFabricControllerStateV1,
        ) -> Result<(), ManagedModelAgentStackApplyControllerError>,
    {
        let context = self
            .state
            .verified_current_context(controller_signer, provisioning)?;
        let (predecessor_desired, predecessor_request, predecessor_receipt) =
            active_predecessor(&self.state)?;
        if let Some(stack) = self.state.model_agent_stack_state() {
            if stack.phase() != ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
                || stack.archived_active().is_some()
            {
                return Err(match stack.phase() {
                    ManagedModelAgentStackApplyPhaseV1::Uncertain => {
                        ManagedModelAgentStackApplyControllerError::OpaqueReplayForbidden
                    }
                    ManagedModelAgentStackApplyPhaseV1::ReceiptDurable => {
                        ManagedModelAgentStackApplyControllerError::AlreadyTerminal
                    }
                    ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent => {
                        ManagedModelAgentStackApplyControllerError::InvalidPhase
                    }
                });
            }
            let expected = ManagedModelAgentStackDesiredPlanV1::try_activate(
                &context,
                self.state.cutover_marker_digest(),
                predecessor_desired.revision(),
                predecessor_desired.execution(),
                predecessor_request.target_slice_digest(),
                activation,
            )?;
            if stack.desired() != &expected {
                return Err(ManagedModelAgentStackApplyControllerError::DesiredConflict);
            }
            validate_managed_model_agent_stack_request_v1(
                &context,
                stack.desired(),
                stack.request(),
            )?;
            return prepared_token(&self.state, stack);
        }
        if predecessor_receipt.facts().outcome() != ManagedFabricApplyTerminalOutcomeV1::ActiveReady
        {
            return Err(ManagedModelAgentStackApplyControllerError::FabricNotActive);
        }
        let desired = ManagedModelAgentStackDesiredPlanV1::try_activate(
            &context,
            self.state.cutover_marker_digest(),
            predecessor_desired.revision(),
            predecessor_desired.execution(),
            predecessor_request.target_slice_digest(),
            activation,
        )?;
        let request = produce_managed_model_agent_stack_request_v1(
            &context,
            &desired,
            fresh,
            controller_signer,
        )?;
        let stack = ManagedModelAgentStackControllerStateV1::try_prepared(desired, request)?;
        let next = self.state.try_with_model_agent_stack_state(stack)?;
        commit(&next)?;
        self.state = next;
        prepared_token(
            &self.state,
            self.state
                .model_agent_stack_state()
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
    }

    pub(crate) fn prepare_empty_deactivate_with<Commit>(
        &mut self,
        controller_signer: &ed25519_dalek::SigningKey,
        provisioning: &ManagedFabricControllerProvisioningV1,
        fresh: FreshManagedModelAgentStackApplyV1,
        commit: Commit,
    ) -> Result<PreparedManagedModelAgentStackApplyV1, ManagedModelAgentStackApplyControllerError>
    where
        Commit: FnOnce(
            &ManagedFabricControllerStateV1,
        ) -> Result<(), ManagedModelAgentStackApplyControllerError>,
    {
        let context = self
            .state
            .verified_current_context(controller_signer, provisioning)?;
        let (_, _, predecessor_receipt) = active_predecessor(&self.state)?;
        let predecessor_generation = predecessor_receipt
            .facts()
            .generation()
            .ok_or(ManagedModelAgentStackApplyControllerError::FabricNotActive)?;
        let current = self
            .state
            .model_agent_stack_state()
            .ok_or(ManagedModelAgentStackApplyControllerError::ModelAgentNotActive)?;
        match current.phase() {
            ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent => {
                let archived = current
                    .archived_active()
                    .ok_or(ManagedModelAgentStackApplyControllerError::ModelAgentNotActive)?;
                let expected = ManagedModelAgentStackDesiredPlanV1::try_empty_deactivate(
                    &context,
                    self.state.cutover_marker_digest(),
                    archived.desired(),
                    archived.request(),
                )?;
                if current.desired() != &expected {
                    return Err(ManagedModelAgentStackApplyControllerError::DesiredConflict);
                }
                validate_managed_model_agent_stack_empty_request_v1(
                    &context,
                    current.desired(),
                    archived.desired().execution(),
                    current.request(),
                )?;
                return prepared_token(&self.state, current);
            }
            ManagedModelAgentStackApplyPhaseV1::Uncertain => {
                return Err(ManagedModelAgentStackApplyControllerError::OpaqueReplayForbidden);
            }
            ManagedModelAgentStackApplyPhaseV1::ReceiptDurable
                if current.archived_active().is_some() =>
            {
                return Err(ManagedModelAgentStackApplyControllerError::AlreadyTerminal);
            }
            ManagedModelAgentStackApplyPhaseV1::ReceiptDurable => {}
        }
        let active_receipt = current
            .receipt()
            .ok_or(ManagedModelAgentStackApplyControllerError::ModelAgentNotActive)?;
        verify_terminal(
            active_receipt,
            current.request(),
            &context,
            predecessor_generation,
        )?;
        if active_receipt.facts().state().outcome()
            != ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
        {
            return Err(ManagedModelAgentStackApplyControllerError::ModelAgentNotActive);
        }
        let desired = ManagedModelAgentStackDesiredPlanV1::try_empty_deactivate(
            &context,
            self.state.cutover_marker_digest(),
            current.desired(),
            current.request(),
        )?;
        let request = produce_managed_model_agent_stack_empty_request_v1(
            &context,
            &desired,
            current.desired().execution(),
            fresh,
            controller_signer,
        )?;
        let stack = current.try_prepare_empty(desired, request)?;
        let next = self.state.try_with_model_agent_stack_state(stack)?;
        commit(&next)?;
        self.state = next;
        prepared_token(
            &self.state,
            self.state
                .model_agent_stack_state()
                .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?,
        )
    }

    pub(crate) fn claim_send_with<Commit>(
        &mut self,
        prepared: PreparedManagedModelAgentStackApplyV1,
        controller_signer: &ed25519_dalek::SigningKey,
        provisioning: &ManagedFabricControllerProvisioningV1,
        commit: Commit,
    ) -> Result<ManagedModelAgentStackSendActionV1, ManagedModelAgentStackApplyControllerError>
    where
        Commit: FnOnce(
            &ManagedFabricControllerStateV1,
        ) -> Result<(), ManagedModelAgentStackApplyControllerError>,
    {
        let context = self
            .state
            .verified_current_context(controller_signer, provisioning)?;
        let stack = self
            .state
            .model_agent_stack_state()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidPhase)?;
        validate_prepared(&self.state, stack, prepared)?;
        validate_stack_request(&context, stack)?;
        let request = stack.request().clone();
        let next = self
            .state
            .try_with_model_agent_stack_state(stack.try_claim()?)?;
        commit(&next)?;
        self.state = next;
        Ok(ManagedModelAgentStackSendActionV1 {
            outer_sequence: self.state.sequence(),
            cutover_marker_digest: self.state.cutover_marker_digest(),
            request,
            channel: context.channel(),
        })
    }

    /// Validates and durably commits every legal PXMT classification.
    pub(crate) fn consume_pxmt_with<Commit>(
        &mut self,
        action: ManagedModelAgentStackSendActionV1,
        receipt_wire: &[u8],
        controller_signer: &ed25519_dalek::SigningKey,
        provisioning: &ManagedFabricControllerProvisioningV1,
        commit: Commit,
    ) -> Result<ManagedModelAgentStackTerminalCommitV1, ManagedModelAgentStackApplyControllerError>
    where
        Commit: FnOnce(
            &ManagedFabricControllerStateV1,
        ) -> Result<(), ManagedModelAgentStackApplyControllerError>,
    {
        let context = self
            .state
            .verified_current_context(controller_signer, provisioning)?;
        let (_, _, predecessor_receipt) = active_predecessor(&self.state)?;
        let predecessor_generation = predecessor_receipt
            .facts()
            .generation()
            .ok_or(ManagedModelAgentStackApplyControllerError::FabricNotActive)?;
        let stack = self
            .state
            .model_agent_stack_state()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidPhase)?;
        if stack.phase() != ManagedModelAgentStackApplyPhaseV1::Uncertain
            || action.outer_sequence != self.state.sequence()
            || action.cutover_marker_digest != self.state.cutover_marker_digest()
            || action.request != *stack.request()
            || action.channel != context.channel()
        {
            return Err(ManagedModelAgentStackApplyControllerError::SendActionMismatch);
        }
        let receipt = ManagedModelAgentStackTerminalReceiptV1::decode(receipt_wire)?;
        verify_terminal(&receipt, stack.request(), &context, predecessor_generation)?;
        let next = self
            .state
            .try_with_model_agent_stack_state(stack.try_terminal(receipt.clone())?)?;
        commit(&next)?;
        self.state = next;
        Ok(ManagedModelAgentStackTerminalCommitV1 {
            outer_sequence: self.state.sequence(),
            receipt,
            replayed_from_journal: false,
        })
    }

    pub(crate) fn terminal(
        &self,
        controller_signer: &ed25519_dalek::SigningKey,
        provisioning: &ManagedFabricControllerProvisioningV1,
    ) -> Result<
        Option<ManagedModelAgentStackTerminalCommitV1>,
        ManagedModelAgentStackApplyControllerError,
    > {
        let Some(stack) = self.state.model_agent_stack_state() else {
            return Ok(None);
        };
        if stack.phase() != ManagedModelAgentStackApplyPhaseV1::ReceiptDurable {
            return Ok(None);
        }
        let context = self
            .state
            .verified_current_context(controller_signer, provisioning)?;
        let (_, _, predecessor_receipt) = active_predecessor(&self.state)?;
        let predecessor_generation = predecessor_receipt
            .facts()
            .generation()
            .ok_or(ManagedModelAgentStackApplyControllerError::FabricNotActive)?;
        let receipt = stack
            .receipt()
            .ok_or(ManagedModelAgentStackApplyControllerError::InvalidState)?;
        verify_terminal(receipt, stack.request(), &context, predecessor_generation)?;
        Ok(Some(ManagedModelAgentStackTerminalCommitV1 {
            outer_sequence: self.state.sequence(),
            receipt: receipt.clone(),
            replayed_from_journal: true,
        }))
    }
}

fn validate_stack_request(
    context: &VerifiedManagedFabricProducerContextV1,
    stack: &ManagedModelAgentStackControllerStateV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    match stack.archived_active() {
        None => validate_managed_model_agent_stack_request_v1(
            context,
            stack.desired(),
            stack.request(),
        )?,
        Some(archived) => validate_managed_model_agent_stack_empty_request_v1(
            context,
            stack.desired(),
            archived.desired().execution(),
            stack.request(),
        )?,
    }
    Ok(())
}

fn active_predecessor(
    state: &ManagedFabricControllerStateV1,
) -> Result<
    (
        &crate::managed_fabric_producer::ManagedFabricDesiredPlanV1,
        &paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricApplyRequestV1,
        &paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricApplyTerminalReceiptV1,
    ),
    ManagedModelAgentStackApplyControllerError,
> {
    if state.phase() != ManagedFabricApplyPhaseV1::ReceiptDurable
        || state.archived_active().is_some()
    {
        return Err(ManagedModelAgentStackApplyControllerError::FabricNotActive);
    }
    let desired = state
        .desired()
        .ok_or(ManagedModelAgentStackApplyControllerError::FabricNotActive)?;
    let request = state
        .request()
        .ok_or(ManagedModelAgentStackApplyControllerError::FabricNotActive)?;
    let receipt = state
        .receipt()
        .ok_or(ManagedModelAgentStackApplyControllerError::FabricNotActive)?;
    if desired.execution().mode() != ManagedFabricTargetModeV1::OneManagedFabricService
        || receipt.facts().outcome() != ManagedFabricApplyTerminalOutcomeV1::ActiveReady
        || receipt.facts().generation().is_none()
    {
        return Err(ManagedModelAgentStackApplyControllerError::FabricNotActive);
    }
    Ok((desired, request, receipt))
}

fn prepared_token(
    state: &ManagedFabricControllerStateV1,
    stack: &ManagedModelAgentStackControllerStateV1,
) -> Result<PreparedManagedModelAgentStackApplyV1, ManagedModelAgentStackApplyControllerError> {
    if stack.phase() != ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent {
        return Err(ManagedModelAgentStackApplyControllerError::InvalidPhase);
    }
    Ok(PreparedManagedModelAgentStackApplyV1 {
        outer_sequence: state.sequence(),
        cutover_marker_digest: state.cutover_marker_digest(),
        request_digest: stack.request().envelope_request_digest(),
    })
}

fn validate_prepared(
    state: &ManagedFabricControllerStateV1,
    stack: &ManagedModelAgentStackControllerStateV1,
    prepared: PreparedManagedModelAgentStackApplyV1,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    if stack.phase() != ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
        || prepared.outer_sequence != state.sequence()
        || prepared.cutover_marker_digest != state.cutover_marker_digest()
        || prepared.request_digest != stack.request().envelope_request_digest()
    {
        return Err(ManagedModelAgentStackApplyControllerError::PreparedTokenMismatch);
    }
    Ok(())
}

fn verify_terminal(
    receipt: &ManagedModelAgentStackTerminalReceiptV1,
    request: &ManagedModelAgentStackApplyRequestV1,
    context: &VerifiedManagedFabricProducerContextV1,
    predecessor_generation: ManagedServiceGeneration,
) -> Result<(), ManagedModelAgentStackApplyControllerError> {
    let facts = receipt.validate_against_request(request, context.channel())?;
    let state = facts.state();
    if state
        .fabric_generation()
        .is_some_and(|generation| generation != predecessor_generation)
        || receipt.authentication_key() != context.runtime_response_key()
        || receipt.authentication_algorithm().value() != ED25519_ALGORITHM
        || receipt.authentication_algorithm_version() != ED25519_ALGORITHM_VERSION
        || receipt.authentication_signature().len() != ED25519_SIGNATURE_BYTES
    {
        return Err(ManagedModelAgentStackApplyControllerError::ReceiptMismatch);
    }
    let signature: [u8; ED25519_SIGNATURE_BYTES] = receipt
        .authentication_signature()
        .try_into()
        .map_err(|_| ManagedModelAgentStackApplyControllerError::ReceiptMismatch)?;
    context
        .runtime_response_public_key()
        .verify_strict(
            receipt.signing_transcript()?.as_bytes(),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| ManagedModelAgentStackApplyControllerError::ReceiptMismatch)
}

fn wire_length(bytes: &[u8]) -> Result<u32, ManagedModelAgentStackApplyControllerError> {
    u32::try_from(bytes.len())
        .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)
}

fn successor(value: u64) -> Result<u64, ManagedModelAgentStackApplyControllerError> {
    value
        .checked_add(1)
        .ok_or(ManagedModelAgentStackApplyControllerError::SequenceExhausted)
}

fn state_checksum(bytes: &[u8]) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(STATE_CHECKSUM_DOMAIN)?;
    builder.field_bytes(bytes)?;
    Ok(builder.finish())
}

struct Cursor<'a> {
    frame: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(frame: &'a [u8]) -> Self {
        Self { frame, position: 0 }
    }

    fn take(
        &mut self,
        length: usize,
    ) -> Result<&'a [u8], ManagedModelAgentStackApplyControllerError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTooLarge)?;
        let bytes = self
            .frame
            .get(self.position..end)
            .ok_or(ManagedModelAgentStackApplyControllerError::StateTruncated)?;
        self.position = end;
        Ok(bytes)
    }

    fn array<const N: usize>(
        &mut self,
    ) -> Result<[u8; N], ManagedModelAgentStackApplyControllerError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTruncated)
    }

    fn u8(&mut self) -> Result<u8, ManagedModelAgentStackApplyControllerError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, ManagedModelAgentStackApplyControllerError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, ManagedModelAgentStackApplyControllerError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn usize_u32(&mut self) -> Result<usize, ManagedModelAgentStackApplyControllerError> {
        usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| ManagedModelAgentStackApplyControllerError::StateTooLarge)
    }

    fn finish(self) -> Result<(), ManagedModelAgentStackApplyControllerError> {
        if self.position == self.frame.len() {
            Ok(())
        } else {
            Err(ManagedModelAgentStackApplyControllerError::InvalidState)
        }
    }
}

#[derive(Debug)]
pub(crate) enum ManagedModelAgentStackApplyControllerError {
    Contract,
    Producer(ManagedModelAgentStackProducerError),
    Fabric(ManagedFabricApplyControllerError),
    Digest(DigestBuildError),
    InvalidPhase,
    InvalidState,
    StateTruncated,
    StateTooLarge,
    StateChecksumMismatch,
    SequenceExhausted,
    FabricNotActive,
    ModelAgentNotActive,
    DesiredConflict,
    DurabilityRejected,
    PreparedTokenMismatch,
    SendActionMismatch,
    OpaqueReplayForbidden,
    ReceiptMismatch,
    AlreadyTerminal,
}

impl From<ManagedModelAgentStackPlanError> for ManagedModelAgentStackApplyControllerError {
    fn from(_value: ManagedModelAgentStackPlanError) -> Self {
        Self::Contract
    }
}

impl From<ArtifactContractError> for ManagedModelAgentStackApplyControllerError {
    fn from(_value: ArtifactContractError) -> Self {
        Self::Contract
    }
}

impl From<ManagedModelAgentStackProducerError> for ManagedModelAgentStackApplyControllerError {
    fn from(value: ManagedModelAgentStackProducerError) -> Self {
        Self::Producer(value)
    }
}

impl From<ManagedFabricApplyControllerError> for ManagedModelAgentStackApplyControllerError {
    fn from(value: ManagedFabricApplyControllerError) -> Self {
        Self::Fabric(value)
    }
}

impl From<DigestBuildError> for ManagedModelAgentStackApplyControllerError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}

mod artifact_external_store {
    use std::collections::BTreeSet;
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, Metadata, TryLockError};
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path, PathBuf};

    use nix::dir::Dir;
    use nix::fcntl::{OFlag, open, openat, renameat};
    use nix::sys::stat::{Mode, fchmod, mkdirat};
    use nix::unistd::{UnlinkatFlags, getegid, geteuid, unlinkat};
    use rustix::fs::{RenameFlags, renameat_with};

    use super::{
        ArtifactConfigCommitmentV1, ArtifactDeploymentOperationIdV1,
        ArtifactExternalControllerPhaseV2, ArtifactExternalControllerStateV2,
        ArtifactExternalDeploymentRequestV1, MAX_ARTIFACT_STATE_V2_BYTES,
    };

    const MAX_STATE_ROOT_UTF8_BYTES: usize = 3917;
    const ROOT_NAME: &str = "artifact-external-controller-v1";
    const STAGING_NAME: &str = ".artifact-external-controller-v1.initializing";
    const LOCK_NAME: &str = "artifact-external.lock";
    const SNAPSHOT_NAME: &str = "artifact-external.pxmj";
    const NEXT_NAME: &str = ".artifact-external.pxmj.next";
    const DIRECTORY_MODE_BITS: u32 = 0o700;
    const FILE_MODE_BITS: u32 = 0o600;
    const MODE_MASK: u32 = 0o7777;
    const DIRECTORY_MODE: Mode = Mode::S_IRUSR.union(Mode::S_IWUSR).union(Mode::S_IXUSR);
    const FILE_MODE: Mode = Mode::S_IRUSR.union(Mode::S_IWUSR);

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct ArtifactExternalControllerAuthorityBindingV1 {
        state_root: PathBuf,
        config_commitment: ArtifactConfigCommitmentV1,
    }

    impl ArtifactExternalControllerAuthorityBindingV1 {
        pub(crate) fn try_new(
            state_root: PathBuf,
            config_commitment: ArtifactConfigCommitmentV1,
        ) -> Result<Self, ArtifactExternalControllerStoreFailureV1> {
            validate_state_root_path(&state_root)?;
            Ok(Self {
                state_root,
                config_commitment,
            })
        }

        pub(crate) fn state_root(&self) -> &Path {
            &self.state_root
        }

        pub(crate) const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
            self.config_commitment
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum ArtifactExternalControllerAuthorityRecheckFailureV1 {
        UnsafePath,
        Configuration,
        Io,
    }

    pub(crate) trait ArtifactExternalControllerAuthorityV1 {
        fn revalidate(
            &mut self,
        ) -> Result<
            ArtifactExternalControllerAuthorityBindingV1,
            ArtifactExternalControllerAuthorityRecheckFailureV1,
        >;
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum ArtifactExternalControllerStoreChangeV1 {
        Unchanged,
        Changed,
        Unknown,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ArtifactExternalControllerStoreFailureV1 {
        UnsafePath,
        ConfigurationMismatch,
        Conflict,
        ReplaceRequired,
        NotFound,
        Contended,
        PublicationUncertain(Option<Box<ArtifactExternalControllerStateV2>>),
        Owner,
        Io,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct ArtifactExternalControllerStoreInvocationV1 {
        change: ArtifactExternalControllerStoreChangeV1,
        result: Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1>,
    }

    impl ArtifactExternalControllerStoreInvocationV1 {
        fn success(
            change: ArtifactExternalControllerStoreChangeV1,
            state: ArtifactExternalControllerStateV2,
        ) -> Self {
            Self {
                change,
                result: Ok(state),
            }
        }

        fn failure(
            change: ArtifactExternalControllerStoreChangeV1,
            failure: ArtifactExternalControllerStoreFailureV1,
        ) -> Self {
            Self {
                change,
                result: Err(failure),
            }
        }

        pub(crate) const fn change(&self) -> ArtifactExternalControllerStoreChangeV1 {
            self.change
        }

        pub(crate) fn result(
            &self,
        ) -> Result<&ArtifactExternalControllerStateV2, &ArtifactExternalControllerStoreFailureV1>
        {
            self.result.as_ref()
        }

        pub(crate) fn into_result(
            self,
        ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1>
        {
            self.result
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct FileIdentity {
        device: u64,
        inode: u64,
    }

    impl FileIdentity {
        fn from_metadata(metadata: &Metadata) -> Self {
            Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        }
    }

    struct DirectoryHandle {
        file: File,
        identity: FileIdentity,
        owner_uid: u32,
        owner_gid: u32,
    }

    struct StateRootHandle {
        parent: DirectoryHandle,
        leaf_name: OsString,
        leaf: DirectoryHandle,
    }

    #[derive(Clone, Copy)]
    enum LockMode {
        Shared,
        Exclusive,
    }

    #[derive(Clone, Copy)]
    struct ChangeTracker(ArtifactExternalControllerStoreChangeV1);

    impl ChangeTracker {
        const fn new() -> Self {
            Self(ArtifactExternalControllerStoreChangeV1::Unchanged)
        }

        fn ambiguous(&mut self) {
            self.0 = ArtifactExternalControllerStoreChangeV1::Unknown;
        }

        fn committed(&mut self) {
            self.0 = ArtifactExternalControllerStoreChangeV1::Changed;
        }

        const fn change(self) -> ArtifactExternalControllerStoreChangeV1 {
            self.0
        }
    }

    struct ValidatedNext {
        state: ArtifactExternalControllerStateV2,
        bytes: Box<[u8]>,
        identity: FileIdentity,
    }

    struct SettleSuccessorInput<'a> {
        authority: &'a mut dyn ArtifactExternalControllerAuthorityV1,
        binding: &'a ArtifactExternalControllerAuthorityBindingV1,
        next: ArtifactExternalControllerStateV2,
        next_bytes: Box<[u8]>,
        next_identity: FileIdentity,
        owned_next: bool,
    }

    struct CompleteStaging {
        staging: DirectoryHandle,
        lock: File,
        lock_identity: FileIdentity,
        state: ArtifactExternalControllerStateV2,
        bytes: Box<[u8]>,
        snapshot_identity: FileIdentity,
    }

    pub(crate) struct ArtifactExternalDeploymentControllerLockedV1 {
        state_root: StateRootHandle,
        root: DirectoryHandle,
        lock: Option<File>,
        lock_identity: FileIdentity,
        snapshot_identity: FileIdentity,
        snapshot_bytes: Box<[u8]>,
        state: ArtifactExternalControllerStateV2,
    }

    impl ArtifactExternalDeploymentControllerLockedV1 {
        pub(crate) const fn state(&self) -> &ArtifactExternalControllerStateV2 {
            &self.state
        }

        pub(crate) fn commit_successor(
            &mut self,
            authority: &mut dyn ArtifactExternalControllerAuthorityV1,
            binding: &ArtifactExternalControllerAuthorityBindingV1,
            next: ArtifactExternalControllerStateV2,
        ) -> ArtifactExternalControllerStoreInvocationV1 {
            let mut tracker = ChangeTracker::new();
            let result = commit_successor(self, authority, binding, next, &mut tracker);
            match result {
                Ok(state) => {
                    ArtifactExternalControllerStoreInvocationV1::success(tracker.change(), state)
                }
                Err(failure) => {
                    ArtifactExternalControllerStoreInvocationV1::failure(tracker.change(), failure)
                }
            }
        }

        pub(crate) fn release(mut self) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
            let lock = self
                .lock
                .take()
                .ok_or(ArtifactExternalControllerStoreFailureV1::Owner)?;
            drop(self);
            lock.unlock()
                .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)
        }
    }

    impl Drop for ArtifactExternalDeploymentControllerLockedV1 {
        fn drop(&mut self) {
            if let Some(lock) = self.lock.take() {
                let _ = lock.unlock();
            }
        }
    }

    pub(crate) struct ArtifactExternalDeploymentControllerStoreV1;

    impl ArtifactExternalDeploymentControllerStoreV1 {
        pub(crate) fn admit(
            authority: &mut dyn ArtifactExternalControllerAuthorityV1,
            request: &ArtifactExternalDeploymentRequestV1,
        ) -> ArtifactExternalControllerStoreInvocationV1 {
            let mut tracker = ChangeTracker::new();
            let result = run_admit(authority, request, &mut tracker);
            match result {
                Ok(state) => {
                    ArtifactExternalControllerStoreInvocationV1::success(tracker.change(), state)
                }
                Err(failure) => {
                    ArtifactExternalControllerStoreInvocationV1::failure(tracker.change(), failure)
                }
            }
        }

        pub(crate) fn query(
            authority: &mut dyn ArtifactExternalControllerAuthorityV1,
            operation_id: ArtifactDeploymentOperationIdV1,
        ) -> ArtifactExternalControllerStoreInvocationV1 {
            match run_query(authority, operation_id) {
                Ok(state) => ArtifactExternalControllerStoreInvocationV1::success(
                    ArtifactExternalControllerStoreChangeV1::Unchanged,
                    state,
                ),
                Err(failure) => ArtifactExternalControllerStoreInvocationV1::failure(
                    ArtifactExternalControllerStoreChangeV1::Unchanged,
                    failure,
                ),
            }
        }

        pub(crate) fn open_exclusive(
            authority: &mut dyn ArtifactExternalControllerAuthorityV1,
            operation_id: ArtifactDeploymentOperationIdV1,
        ) -> Result<
            (
                ArtifactExternalControllerAuthorityBindingV1,
                ArtifactExternalDeploymentControllerLockedV1,
            ),
            ArtifactExternalControllerStoreFailureV1,
        > {
            let binding = authority_binding(authority)?;
            let state_root = pin_state_root(&binding)?;
            revalidate_current_authority(authority, &binding, &state_root)?;
            let store = open_final_locked(state_root, &binding, LockMode::Exclusive)?;
            if store.state.request().operation_id() != operation_id {
                return release_locked(
                    store,
                    Err(ArtifactExternalControllerStoreFailureV1::NotFound),
                );
            }
            Ok((binding, store))
        }
    }

    fn validate_state_root_path(
        path: &Path,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let bytes = path.as_os_str().as_bytes();
        if !path.is_absolute()
            || path == Path::new("/")
            || bytes.len() > MAX_STATE_ROOT_UTF8_BYTES
            || bytes.contains(&0)
            || path.to_str().is_none()
            || bytes.ends_with(b"/")
            || bytes.windows(2).any(|window| window == b"//")
        {
            return Err(ArtifactExternalControllerStoreFailureV1::UnsafePath);
        }
        for segment in bytes.split(|byte| *byte == b'/').skip(1) {
            if segment.is_empty() || segment == b"." || segment == b".." {
                return Err(ArtifactExternalControllerStoreFailureV1::UnsafePath);
            }
        }
        Ok(())
    }

    fn authority_binding(
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
    ) -> Result<
        ArtifactExternalControllerAuthorityBindingV1,
        ArtifactExternalControllerStoreFailureV1,
    > {
        let binding = authority.revalidate().map_err(|failure| match failure {
            ArtifactExternalControllerAuthorityRecheckFailureV1::UnsafePath => {
                ArtifactExternalControllerStoreFailureV1::UnsafePath
            }
            ArtifactExternalControllerAuthorityRecheckFailureV1::Configuration => {
                ArtifactExternalControllerStoreFailureV1::ConfigurationMismatch
            }
            ArtifactExternalControllerAuthorityRecheckFailureV1::Io => {
                ArtifactExternalControllerStoreFailureV1::Io
            }
        })?;
        validate_state_root_path(binding.state_root())?;
        Ok(binding)
    }

    fn require_same_authority(
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
        expected: &ArtifactExternalControllerAuthorityBindingV1,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let current = authority_binding(authority)?;
        if current != *expected {
            return Err(ArtifactExternalControllerStoreFailureV1::ConfigurationMismatch);
        }
        Ok(())
    }

    fn validate_directory_metadata(
        metadata: &Metadata,
        uid: u32,
        gid: u32,
        strict: bool,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        if !metadata.file_type().is_dir() {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        if strict
            && (metadata.uid() != uid
                || metadata.gid() != gid
                || metadata.mode() & MODE_MASK != DIRECTORY_MODE_BITS)
        {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        if !strict && metadata.uid() != 0 && metadata.uid() != uid {
            return Err(ArtifactExternalControllerStoreFailureV1::UnsafePath);
        }
        if !strict && metadata.mode() & 0o022 != 0 {
            return Err(ArtifactExternalControllerStoreFailureV1::UnsafePath);
        }
        Ok(())
    }

    fn validate_regular_metadata(
        metadata: &Metadata,
        uid: u32,
        gid: u32,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        if !metadata.file_type().is_file()
            || metadata.nlink() != 1
            || metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.mode() & MODE_MASK != FILE_MODE_BITS
        {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn directory_from_owned(
        owned: OwnedFd,
        uid: u32,
        gid: u32,
        strict: bool,
    ) -> Result<DirectoryHandle, ArtifactExternalControllerStoreFailureV1> {
        let file = File::from(owned);
        let metadata = file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        validate_directory_metadata(&metadata, uid, gid, strict)?;
        Ok(DirectoryHandle {
            file,
            identity: FileIdentity::from_metadata(&metadata),
            owner_uid: uid,
            owner_gid: gid,
        })
    }

    fn open_state_parent(
        path: &Path,
    ) -> Result<(DirectoryHandle, OsString), ArtifactExternalControllerStoreFailureV1> {
        validate_state_root_path(path)?;
        let uid = geteuid().as_raw();
        let gid = getegid().as_raw();
        let leaf_name = path
            .file_name()
            .ok_or(ArtifactExternalControllerStoreFailureV1::UnsafePath)?
            .to_os_string();
        let parent_path = path
            .parent()
            .ok_or(ArtifactExternalControllerStoreFailureV1::UnsafePath)?;
        let root = open(
            Path::new("/"),
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let mut current = directory_from_owned(root, uid, gid, false)?;
        for component in parent_path.components().skip(1) {
            let Component::Normal(name) = component else {
                return Err(ArtifactExternalControllerStoreFailureV1::UnsafePath);
            };
            let owned = openat(
                &current.file,
                name,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|error| match error {
                nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => {
                    ArtifactExternalControllerStoreFailureV1::UnsafePath
                }
                nix::errno::Errno::ENOENT => ArtifactExternalControllerStoreFailureV1::NotFound,
                _ => ArtifactExternalControllerStoreFailureV1::Io,
            })?;
            let next = directory_from_owned(owned, uid, gid, false)?;
            drop(current);
            current = next;
        }
        let metadata = current
            .file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        validate_directory_metadata(&metadata, uid, gid, true)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::UnsafePath)?;
        Ok((current, leaf_name))
    }

    fn open_directory_at(
        parent: &DirectoryHandle,
        name: &OsStr,
    ) -> Result<DirectoryHandle, ArtifactExternalControllerStoreFailureV1> {
        let owned = openat(
            &parent.file,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| match error {
            nix::errno::Errno::ENOENT | nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => {
                ArtifactExternalControllerStoreFailureV1::Owner
            }
            _ => ArtifactExternalControllerStoreFailureV1::Io,
        })?;
        directory_from_owned(owned, parent.owner_uid, parent.owner_gid, true)
    }

    fn open_state_leaf(
        parent: &DirectoryHandle,
        name: &OsStr,
    ) -> Result<DirectoryHandle, ArtifactExternalControllerStoreFailureV1> {
        let owned = openat(
            &parent.file,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| match error {
            nix::errno::Errno::ENOENT => ArtifactExternalControllerStoreFailureV1::NotFound,
            nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => {
                ArtifactExternalControllerStoreFailureV1::UnsafePath
            }
            _ => ArtifactExternalControllerStoreFailureV1::Io,
        })?;
        directory_from_owned(owned, parent.owner_uid, parent.owner_gid, true).map_err(|failure| {
            match failure {
                ArtifactExternalControllerStoreFailureV1::Owner => {
                    ArtifactExternalControllerStoreFailureV1::UnsafePath
                }
                other => other,
            }
        })
    }

    fn pin_state_root(
        binding: &ArtifactExternalControllerAuthorityBindingV1,
    ) -> Result<StateRootHandle, ArtifactExternalControllerStoreFailureV1> {
        let (parent, leaf_name) = open_state_parent(binding.state_root())?;
        let leaf = open_state_leaf(&parent, &leaf_name)?;
        Ok(StateRootHandle {
            parent,
            leaf_name,
            leaf,
        })
    }

    fn revalidate_directory(
        directory: &DirectoryHandle,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let metadata = directory
            .file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        validate_directory_metadata(&metadata, directory.owner_uid, directory.owner_gid, true)?;
        if FileIdentity::from_metadata(&metadata) != directory.identity {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn reopen_named_directory(
        parent: &DirectoryHandle,
        name: &OsStr,
        expected: FileIdentity,
    ) -> Result<DirectoryHandle, ArtifactExternalControllerStoreFailureV1> {
        revalidate_directory(parent)?;
        let directory = open_directory_at(parent, name)?;
        if directory.identity != expected {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(directory)
    }

    fn revalidate_current_authority(
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
        binding: &ArtifactExternalControllerAuthorityBindingV1,
        state_root: &StateRootHandle,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        require_same_authority(authority, binding)?;
        let (parent, leaf_name) = open_state_parent(binding.state_root())?;
        if parent.identity != state_root.parent.identity || leaf_name != state_root.leaf_name {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let leaf = open_state_leaf(&parent, &leaf_name).map_err(|failure| match failure {
            ArtifactExternalControllerStoreFailureV1::UnsafePath
            | ArtifactExternalControllerStoreFailureV1::NotFound => {
                ArtifactExternalControllerStoreFailureV1::Owner
            }
            other => other,
        })?;
        if leaf.identity != state_root.leaf.identity {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn scan_names(
        directory: &DirectoryHandle,
    ) -> Result<BTreeSet<OsString>, ArtifactExternalControllerStoreFailureV1> {
        revalidate_directory(directory)?;
        let owned = openat(
            &directory.file,
            ".",
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let mut stream =
            Dir::from_fd(owned).map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let mut names = BTreeSet::new();
        for entry in stream.iter() {
            let entry = entry.map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            if bytes.contains(&0) || !names.insert(OsStr::from_bytes(bytes).to_os_string()) {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
        }
        Ok(names)
    }

    fn exact_names(
        directory: &DirectoryHandle,
        expected: &[&str],
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let actual = scan_names(directory)?;
        let expected = expected.iter().map(OsString::from).collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn root_selection(
        state_root: &DirectoryHandle,
    ) -> Result<(bool, bool), ArtifactExternalControllerStoreFailureV1> {
        let names = scan_names(state_root)?;
        Ok((
            names.contains(OsStr::new(ROOT_NAME)),
            names.contains(OsStr::new(STAGING_NAME)),
        ))
    }

    fn open_regular_at(
        parent: &DirectoryHandle,
        name: &OsStr,
        access: OFlag,
    ) -> Result<(File, FileIdentity), ArtifactExternalControllerStoreFailureV1> {
        let owned = openat(
            &parent.file,
            name,
            access | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| match error {
            nix::errno::Errno::ENOENT | nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR => {
                ArtifactExternalControllerStoreFailureV1::Owner
            }
            _ => ArtifactExternalControllerStoreFailureV1::Io,
        })?;
        let file = File::from(owned);
        let metadata = file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        validate_regular_metadata(&metadata, parent.owner_uid, parent.owner_gid)?;
        Ok((file, FileIdentity::from_metadata(&metadata)))
    }

    fn validate_named_regular(
        parent: &DirectoryHandle,
        name: &OsStr,
        expected: FileIdentity,
        expected_len: u64,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let (file, identity) = open_regular_at(parent, name, OFlag::O_RDONLY)?;
        let metadata = file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        if identity != expected || metadata.len() != expected_len {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn read_regular(
        parent: &DirectoryHandle,
        name: &OsStr,
        maximum: usize,
        allow_empty: bool,
    ) -> Result<(Box<[u8]>, FileIdentity), ArtifactExternalControllerStoreFailureV1> {
        let (mut file, identity) = open_regular_at(parent, name, OFlag::O_RDONLY)?;
        let before = file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let length = usize::try_from(before.len())
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
        if length > maximum || (!allow_empty && length == 0) {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let mut bytes = vec![0_u8; length];
        file.read_exact(&mut bytes)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let mut trailing = [0_u8; 1];
        if file
            .read(&mut trailing)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?
            != 0
        {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let after = file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        validate_regular_metadata(&after, parent.owner_uid, parent.owner_gid)?;
        if FileIdentity::from_metadata(&after) != identity || after.len() != before.len() {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        validate_named_regular(parent, name, identity, before.len())?;
        Ok((bytes.into_boxed_slice(), identity))
    }

    fn create_regular(
        parent: &DirectoryHandle,
        name: &str,
        access: OFlag,
    ) -> Result<(File, FileIdentity), ArtifactExternalControllerStoreFailureV1> {
        let owned = openat(
            &parent.file,
            name,
            access | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            FILE_MODE,
        )
        .map_err(|error| match error {
            nix::errno::Errno::EEXIST => ArtifactExternalControllerStoreFailureV1::Owner,
            _ => ArtifactExternalControllerStoreFailureV1::Io,
        })?;
        let file = File::from(owned);
        fchmod(&file, FILE_MODE).map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let metadata = file
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        validate_regular_metadata(&metadata, parent.owner_uid, parent.owner_gid)?;
        if metadata.len() != 0 {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok((file, FileIdentity::from_metadata(&metadata)))
    }

    fn write_new_exact(
        parent: &DirectoryHandle,
        name: &str,
        bytes: &[u8],
    ) -> Result<FileIdentity, ArtifactExternalControllerStoreFailureV1> {
        let (mut file, identity) = create_regular(parent, name, OFlag::O_WRONLY)?;
        file.write_all(bytes)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        file.sync_all()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        drop(file);
        let (reopened, reopened_identity) =
            read_regular(parent, OsStr::new(name), bytes.len(), bytes.is_empty())?;
        if reopened_identity != identity || reopened.as_ref() != bytes {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(identity)
    }

    fn acquire_lock(
        root: &DirectoryHandle,
        mode: LockMode,
    ) -> Result<(File, FileIdentity), ArtifactExternalControllerStoreFailureV1> {
        let (lock, identity) = open_regular_at(root, OsStr::new(LOCK_NAME), OFlag::O_RDWR)?;
        if lock
            .metadata()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?
            .len()
            != 0
        {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let result = match mode {
            LockMode::Shared => lock.try_lock_shared(),
            LockMode::Exclusive => lock.try_lock(),
        };
        result.map_err(|error| match error {
            TryLockError::WouldBlock => ArtifactExternalControllerStoreFailureV1::Contended,
            TryLockError::Error(_) => ArtifactExternalControllerStoreFailureV1::Io,
        })?;
        if let Err(failure) = validate_named_regular(root, OsStr::new(LOCK_NAME), identity, 0) {
            return release_file(lock, Err(failure));
        }
        Ok((lock, identity))
    }

    fn release_file<T>(
        lock: File,
        primary: Result<T, ArtifactExternalControllerStoreFailureV1>,
    ) -> Result<T, ArtifactExternalControllerStoreFailureV1> {
        let release = lock
            .unlock()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io);
        drop(lock);
        match primary {
            Err(failure) => Err(failure),
            Ok(value) => release.map(|()| value),
        }
    }

    fn release_locked<T>(
        mut store: ArtifactExternalDeploymentControllerLockedV1,
        primary: Result<T, ArtifactExternalControllerStoreFailureV1>,
    ) -> Result<T, ArtifactExternalControllerStoreFailureV1> {
        let lock = store
            .lock
            .take()
            .ok_or(ArtifactExternalControllerStoreFailureV1::Owner)?;
        drop(store);
        release_file(lock, primary)
    }

    fn read_state(
        root: &DirectoryHandle,
        name: &str,
    ) -> Result<
        (ArtifactExternalControllerStateV2, Box<[u8]>, FileIdentity),
        ArtifactExternalControllerStoreFailureV1,
    > {
        let (bytes, identity) =
            read_regular(root, OsStr::new(name), MAX_ARTIFACT_STATE_V2_BYTES, false)?;
        let state = ArtifactExternalControllerStateV2::decode(&bytes)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
        let canonical = state
            .encode()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
        if canonical != bytes {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok((state, bytes, identity))
    }

    fn validate_config(
        state: &ArtifactExternalControllerStateV2,
        binding: &ArtifactExternalControllerAuthorityBindingV1,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        if state.request().config_commitment() != binding.config_commitment() {
            return Err(ArtifactExternalControllerStoreFailureV1::ConfigurationMismatch);
        }
        Ok(())
    }

    fn permitted_successor(
        current: &ArtifactExternalControllerStateV2,
        next: &ArtifactExternalControllerStateV2,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let expected_sequence = current
            .controller_snapshot_sequence()
            .get()
            .checked_add(1)
            .ok_or(ArtifactExternalControllerStoreFailureV1::Owner)?;
        if next.controller_snapshot_sequence().get() != expected_sequence
            || next.request() != current.request()
            || next.admission() != current.admission()
            || !next.records().starts_with(current.records())
        {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let allowed = match current.phase() {
            ArtifactExternalControllerPhaseV2::Admitted => matches!(
                next.phase(),
                ArtifactExternalControllerPhaseV2::Committed
                    | ArtifactExternalControllerPhaseV2::Failed
                    | ArtifactExternalControllerPhaseV2::Uncertain
            ),
            ArtifactExternalControllerPhaseV2::Committed => matches!(
                next.phase(),
                ArtifactExternalControllerPhaseV2::Applying
                    | ArtifactExternalControllerPhaseV2::Failed
                    | ArtifactExternalControllerPhaseV2::Uncertain
            ),
            ArtifactExternalControllerPhaseV2::Applying => matches!(
                next.phase(),
                ArtifactExternalControllerPhaseV2::ActiveReady
                    | ArtifactExternalControllerPhaseV2::Failed
                    | ArtifactExternalControllerPhaseV2::Uncertain
            ),
            ArtifactExternalControllerPhaseV2::ActiveReady
            | ArtifactExternalControllerPhaseV2::Failed
            | ArtifactExternalControllerPhaseV2::Uncertain => false,
        };
        if !allowed {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn open_final_locked(
        state_root: StateRootHandle,
        binding: &ArtifactExternalControllerAuthorityBindingV1,
        mode: LockMode,
    ) -> Result<
        ArtifactExternalDeploymentControllerLockedV1,
        ArtifactExternalControllerStoreFailureV1,
    > {
        let root = open_directory_at(&state_root.leaf, OsStr::new(ROOT_NAME))?;
        let (lock, lock_identity) = acquire_lock(&root, mode)?;
        let result = (|| {
            let (has_final, has_staging) = root_selection(&state_root.leaf)?;
            if !has_final || has_staging {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            let public_root =
                reopen_named_directory(&state_root.leaf, OsStr::new(ROOT_NAME), root.identity)?;
            let names = scan_names(&public_root)?;
            let stable = [OsString::from(LOCK_NAME), OsString::from(SNAPSHOT_NAME)]
                .into_iter()
                .collect::<BTreeSet<_>>();
            let with_next = [
                OsString::from(LOCK_NAME),
                OsString::from(SNAPSHOT_NAME),
                OsString::from(NEXT_NAME),
            ]
            .into_iter()
            .collect::<BTreeSet<_>>();
            if names != stable && names != with_next {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            validate_named_regular(&public_root, OsStr::new(LOCK_NAME), lock_identity, 0)?;
            let (state, bytes, snapshot_identity) = read_state(&public_root, SNAPSHOT_NAME)?;
            validate_config(&state, binding)?;
            Ok((state, bytes, snapshot_identity))
        })();
        let (state, snapshot_bytes, snapshot_identity) = match result {
            Ok(value) => value,
            Err(failure) => return release_file(lock, Err(failure)),
        };
        Ok(ArtifactExternalDeploymentControllerLockedV1 {
            state_root,
            root,
            lock: Some(lock),
            lock_identity,
            snapshot_identity,
            snapshot_bytes,
            state,
        })
    }

    fn read_validated_next(
        store: &ArtifactExternalDeploymentControllerLockedV1,
    ) -> Result<Option<ValidatedNext>, ArtifactExternalControllerStoreFailureV1> {
        let names = scan_names(&store.root)?;
        let stable = [OsString::from(LOCK_NAME), OsString::from(SNAPSHOT_NAME)]
            .into_iter()
            .collect::<BTreeSet<_>>();
        if names == stable {
            return Ok(None);
        }
        let with_next = [
            OsString::from(LOCK_NAME),
            OsString::from(SNAPSHOT_NAME),
            OsString::from(NEXT_NAME),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if names != with_next {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let (next, bytes, identity) = read_state(&store.root, NEXT_NAME)?;
        permitted_successor(&store.state, &next)?;
        Ok(Some(ValidatedNext {
            state: next,
            bytes,
            identity,
        }))
    }

    fn validate_public_store(
        store: &ArtifactExternalDeploymentControllerLockedV1,
        allow_next: bool,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let (has_final, has_staging) = root_selection(&store.state_root.leaf)?;
        if !has_final || has_staging {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let root = reopen_named_directory(
            &store.state_root.leaf,
            OsStr::new(ROOT_NAME),
            store.root.identity,
        )?;
        let names = scan_names(&root)?;
        let stable = [OsString::from(LOCK_NAME), OsString::from(SNAPSHOT_NAME)]
            .into_iter()
            .collect::<BTreeSet<_>>();
        let with_next = [
            OsString::from(LOCK_NAME),
            OsString::from(SNAPSHOT_NAME),
            OsString::from(NEXT_NAME),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if names != stable && (!allow_next || names != with_next) {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        validate_named_regular(&root, OsStr::new(LOCK_NAME), store.lock_identity, 0)?;
        let (state, bytes, identity) = read_state(&root, SNAPSHOT_NAME)?;
        if identity != store.snapshot_identity
            || bytes != store.snapshot_bytes
            || state != store.state
        {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn query_locked(
        store: ArtifactExternalDeploymentControllerLockedV1,
        operation_id: ArtifactDeploymentOperationIdV1,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        let result = (|| {
            validate_public_store(&store, true)?;
            if let Some(next) = read_validated_next(&store)?
                && next.state.request().operation_id() == operation_id
            {
                return Err(
                    ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(Box::new(
                        next.state,
                    ))),
                );
            }
            if store.state.request().operation_id() != operation_id {
                return Err(ArtifactExternalControllerStoreFailureV1::NotFound);
            }
            Ok(store.state.clone())
        })();
        release_locked(store, result)
    }

    fn clone_state_root(
        state_root: &StateRootHandle,
    ) -> Result<StateRootHandle, ArtifactExternalControllerStoreFailureV1> {
        let parent = reopen_named_directory(
            &state_root.parent,
            OsStr::new("."),
            state_root.parent.identity,
        )?;
        let leaf =
            reopen_named_directory(&parent, &state_root.leaf_name, state_root.leaf.identity)?;
        Ok(StateRootHandle {
            parent,
            leaf_name: state_root.leaf_name.clone(),
            leaf,
        })
    }

    fn inspect_staging(
        state_root: &StateRootHandle,
        binding: &ArtifactExternalControllerAuthorityBindingV1,
        operation_id: ArtifactDeploymentOperationIdV1,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        let staging = match open_directory_at(&state_root.leaf, OsStr::new(STAGING_NAME)) {
            Ok(staging) => staging,
            Err(failure) => {
                let (has_final, has_staging) = root_selection(&state_root.leaf)?;
                if has_final && !has_staging {
                    let store = open_final_locked(
                        clone_state_root(state_root)?,
                        binding,
                        LockMode::Shared,
                    )?;
                    return query_locked(store, operation_id);
                }
                return Err(failure);
            }
        };
        let (lock, lock_identity) = acquire_lock(&staging, LockMode::Shared)?;
        let result = (|| {
            let (has_final, has_staging) = root_selection(&state_root.leaf)?;
            if has_final || !has_staging {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            let staging = reopen_named_directory(
                &state_root.leaf,
                OsStr::new(STAGING_NAME),
                staging.identity,
            )?;
            exact_names(&staging, &[LOCK_NAME, SNAPSHOT_NAME])?;
            validate_named_regular(&staging, OsStr::new(LOCK_NAME), lock_identity, 0)?;
            let (state, _, _) = read_state(&staging, SNAPSHOT_NAME)?;
            validate_config(&state, binding)?;
            if state.phase() != ArtifactExternalControllerPhaseV2::Admitted
                || state.controller_snapshot_sequence().get() != 1
            {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            if state.request().operation_id() != operation_id {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            Err(
                ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(Box::new(
                    state,
                ))),
            )
        })();
        release_file(lock, result)
    }

    fn run_query(
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
        operation_id: ArtifactDeploymentOperationIdV1,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        let binding = authority_binding(authority)?;
        let state_root = match pin_state_root(&binding) {
            Ok(state_root) => state_root,
            Err(ArtifactExternalControllerStoreFailureV1::NotFound) => {
                return Err(ArtifactExternalControllerStoreFailureV1::NotFound);
            }
            Err(failure) => return Err(failure),
        };
        revalidate_current_authority(authority, &binding, &state_root)?;
        match root_selection(&state_root.leaf)? {
            (false, false) => Err(ArtifactExternalControllerStoreFailureV1::NotFound),
            (true, false) => {
                let store = open_final_locked(state_root, &binding, LockMode::Shared)?;
                query_locked(store, operation_id)
            }
            (false, true) => inspect_staging(&state_root, &binding, operation_id),
            (true, true) => Err(ArtifactExternalControllerStoreFailureV1::Owner),
        }
    }

    fn draw_store_instance() -> Result<[u8; 32], ArtifactExternalControllerStoreFailureV1> {
        let mut instance = [0_u8; 32];
        getrandom::fill(&mut instance).map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        if instance.iter().all(|byte| *byte == 0) {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(instance)
    }

    fn create_directory(
        parent: &DirectoryHandle,
        name: &str,
    ) -> Result<DirectoryHandle, ArtifactExternalControllerStoreFailureV1> {
        mkdirat(&parent.file, name, DIRECTORY_MODE).map_err(|error| match error {
            nix::errno::Errno::EEXIST => ArtifactExternalControllerStoreFailureV1::Owner,
            _ => ArtifactExternalControllerStoreFailureV1::Io,
        })?;
        let directory = open_directory_at(parent, OsStr::new(name))?;
        revalidate_directory(&directory)?;
        Ok(directory)
    }

    fn seal_staging_prefix(
        state_root: &StateRootHandle,
        staging: &DirectoryHandle,
        lock: &File,
        lock_identity: FileIdentity,
    ) -> Result<DirectoryHandle, ArtifactExternalControllerStoreFailureV1> {
        lock.sync_all()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        staging
            .file
            .sync_all()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        state_root
            .leaf
            .file
            .sync_all()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        let reopened =
            reopen_named_directory(&state_root.leaf, OsStr::new(STAGING_NAME), staging.identity)?;
        exact_names(&reopened, &[LOCK_NAME])?;
        validate_named_regular(&reopened, OsStr::new(LOCK_NAME), lock_identity, 0)?;
        Ok(reopened)
    }

    fn create_and_lock_staging(
        state_root: &StateRootHandle,
    ) -> Result<(DirectoryHandle, File, FileIdentity), ArtifactExternalControllerStoreFailureV1>
    {
        let staging = create_directory(&state_root.leaf, STAGING_NAME)?;
        let (lock, lock_identity) = create_regular(&staging, LOCK_NAME, OFlag::O_RDWR)?;
        if let Err(failure) = lock.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => ArtifactExternalControllerStoreFailureV1::Contended,
            TryLockError::Error(_) => ArtifactExternalControllerStoreFailureV1::Io,
        }) {
            return release_file(lock, Err(failure));
        }
        let reopened = match seal_staging_prefix(state_root, &staging, &lock, lock_identity) {
            Ok(reopened) => reopened,
            Err(failure) => return release_file(lock, Err(failure)),
        };
        drop(staging);
        Ok((reopened, lock, lock_identity))
    }

    fn open_complete_staging(
        state_root: &StateRootHandle,
    ) -> Result<CompleteStaging, ArtifactExternalControllerStoreFailureV1> {
        let staging = open_directory_at(&state_root.leaf, OsStr::new(STAGING_NAME))?;
        let (lock, lock_identity) = acquire_lock(&staging, LockMode::Exclusive)?;
        let result = (|| {
            let (has_final, has_staging) = root_selection(&state_root.leaf)?;
            if has_final || !has_staging {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            let staging = reopen_named_directory(
                &state_root.leaf,
                OsStr::new(STAGING_NAME),
                staging.identity,
            )?;
            exact_names(&staging, &[LOCK_NAME, SNAPSHOT_NAME])?;
            validate_named_regular(&staging, OsStr::new(LOCK_NAME), lock_identity, 0)?;
            let (state, bytes, identity) = read_state(&staging, SNAPSHOT_NAME)?;
            if state.phase() != ArtifactExternalControllerPhaseV2::Admitted
                || state.controller_snapshot_sequence().get() != 1
            {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            Ok((staging, state, bytes, identity))
        })();
        match result {
            Ok((staging, state, bytes, snapshot_identity)) => Ok(CompleteStaging {
                staging,
                lock,
                lock_identity,
                state,
                bytes,
                snapshot_identity,
            }),
            Err(failure) => release_file(lock, Err(failure)),
        }
    }

    struct InitialPublication {
        state_root: StateRootHandle,
        staging: DirectoryHandle,
        lock: File,
        lock_identity: FileIdentity,
        state: ArtifactExternalControllerStateV2,
        bytes: Box<[u8]>,
        snapshot_identity: FileIdentity,
    }

    enum InitialRenameState {
        Old,
        New,
        ObservationUnavailable,
        OwnerInvalid,
    }

    struct InitialPublicationFacts<'a> {
        root_identity: FileIdentity,
        lock_identity: FileIdentity,
        state: &'a ArtifactExternalControllerStateV2,
        bytes: &'a [u8],
        snapshot_identity: FileIdentity,
    }

    fn classify_initial_rename(
        state_root: &StateRootHandle,
        facts: InitialPublicationFacts<'_>,
    ) -> InitialRenameState {
        let observed = (|| {
            let (has_final, has_staging) = root_selection(&state_root.leaf)?;
            let (name, state) = match (has_final, has_staging) {
                (false, true) => (STAGING_NAME, InitialRenameState::Old),
                (true, false) => (ROOT_NAME, InitialRenameState::New),
                _ => return Err(ArtifactExternalControllerStoreFailureV1::Owner),
            };
            let root =
                reopen_named_directory(&state_root.leaf, OsStr::new(name), facts.root_identity)?;
            exact_names(&root, &[LOCK_NAME, SNAPSHOT_NAME])?;
            validate_named_regular(&root, OsStr::new(LOCK_NAME), facts.lock_identity, 0)?;
            let (snapshot, bytes, identity) = read_state(&root, SNAPSHOT_NAME)?;
            if snapshot != *facts.state
                || bytes.as_ref() != facts.bytes
                || identity != facts.snapshot_identity
            {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            Ok(state)
        })();
        match observed {
            Ok(state) => state,
            Err(ArtifactExternalControllerStoreFailureV1::Io) => {
                InitialRenameState::ObservationUnavailable
            }
            Err(_) => InitialRenameState::OwnerInvalid,
        }
    }

    fn publish_initial(
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
        binding: &ArtifactExternalControllerAuthorityBindingV1,
        publication: InitialPublication,
        tracker: &mut ChangeTracker,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        let InitialPublication {
            state_root,
            staging,
            lock,
            lock_identity,
            state,
            bytes,
            snapshot_identity,
        } = publication;
        let staging_identity = staging.identity;
        let result = (|| {
            exact_names(&staging, &[LOCK_NAME, SNAPSHOT_NAME])?;
            validate_named_regular(&staging, OsStr::new(LOCK_NAME), lock_identity, 0)?;
            let expected_len = u64::try_from(bytes.len())
                .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
            validate_named_regular(
                &staging,
                OsStr::new(SNAPSHOT_NAME),
                snapshot_identity,
                expected_len,
            )?;
            staging
                .file
                .sync_all()
                .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
            state_root
                .leaf
                .file
                .sync_all()
                .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
            let reopened_staging = reopen_named_directory(
                &state_root.leaf,
                OsStr::new(STAGING_NAME),
                staging_identity,
            )?;
            exact_names(&reopened_staging, &[LOCK_NAME, SNAPSHOT_NAME])?;
            validate_named_regular(&reopened_staging, OsStr::new(LOCK_NAME), lock_identity, 0)?;
            validate_named_regular(
                &reopened_staging,
                OsStr::new(SNAPSHOT_NAME),
                snapshot_identity,
                expected_len,
            )?;
            revalidate_current_authority(authority, binding, &state_root)?;
            if root_selection(&state_root.leaf)? != (false, true) {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            tracker.ambiguous();
            if renameat_with(
                &state_root.leaf.file,
                STAGING_NAME,
                &state_root.leaf.file,
                ROOT_NAME,
                RenameFlags::NOREPLACE,
            )
            .is_err()
            {
                match classify_initial_rename(
                    &state_root,
                    InitialPublicationFacts {
                        root_identity: staging_identity,
                        lock_identity,
                        state: &state,
                        bytes: &bytes,
                        snapshot_identity,
                    },
                ) {
                    InitialRenameState::New => {}
                    InitialRenameState::Old | InitialRenameState::ObservationUnavailable => {
                        return Err(
                            ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(
                                Box::new(state.clone()),
                            )),
                        );
                    }
                    InitialRenameState::OwnerInvalid => {
                        return Err(ArtifactExternalControllerStoreFailureV1::Owner);
                    }
                }
            }
            drop(reopened_staging);
            state_root.leaf.file.sync_all().map_err(|_| {
                ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(Box::new(
                    state.clone(),
                )))
            })?;
            revalidate_current_authority(authority, binding, &state_root)
                .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
            if root_selection(&state_root.leaf)? != (true, false) {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            let root =
                reopen_named_directory(&state_root.leaf, OsStr::new(ROOT_NAME), staging_identity)?;
            exact_names(&root, &[LOCK_NAME, SNAPSHOT_NAME])?;
            validate_named_regular(&root, OsStr::new(LOCK_NAME), lock_identity, 0)?;
            let (reopened, reopened_bytes, reopened_identity) = read_state(&root, SNAPSHOT_NAME)?;
            if reopened != state
                || reopened_bytes != bytes
                || reopened_identity != snapshot_identity
            {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            tracker.committed();
            Ok(state)
        })();
        release_file(lock, result)
    }

    fn run_admit(
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
        request: &ArtifactExternalDeploymentRequestV1,
        tracker: &mut ChangeTracker,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        let binding = authority_binding(authority)?;
        if request.config_commitment() != binding.config_commitment() {
            return Err(ArtifactExternalControllerStoreFailureV1::ConfigurationMismatch);
        }
        let state_root = pin_state_root(&binding)?;
        revalidate_current_authority(authority, &binding, &state_root)?;
        match root_selection(&state_root.leaf)? {
            (true, true) => return Err(ArtifactExternalControllerStoreFailureV1::Owner),
            (true, false) => {
                let store = open_final_locked(state_root, &binding, LockMode::Exclusive)?;
                let result = if store.state.request().operation_id() != request.operation_id() {
                    Err(ArtifactExternalControllerStoreFailureV1::ReplaceRequired)
                } else if store.state.request() != request {
                    Err(ArtifactExternalControllerStoreFailureV1::Conflict)
                } else {
                    Ok(store.state.clone())
                };
                return release_locked(store, result);
            }
            (false, true) => {
                let CompleteStaging {
                    staging,
                    lock,
                    lock_identity,
                    state,
                    bytes,
                    snapshot_identity,
                } = open_complete_staging(&state_root)?;
                if let Err(failure) = validate_config(&state, &binding) {
                    return release_file(lock, Err(failure));
                }
                if state.request().operation_id() != request.operation_id() {
                    return release_file(
                        lock,
                        Err(ArtifactExternalControllerStoreFailureV1::Owner),
                    );
                }
                if state.request() != request {
                    return release_file(
                        lock,
                        Err(ArtifactExternalControllerStoreFailureV1::Conflict),
                    );
                }
                return publish_initial(
                    authority,
                    &binding,
                    InitialPublication {
                        state_root,
                        staging,
                        lock,
                        lock_identity,
                        state,
                        bytes,
                        snapshot_identity,
                    },
                    tracker,
                );
            }
            (false, false) => {}
        }

        tracker.ambiguous();
        let (staging, lock, lock_identity) = create_and_lock_staging(&state_root)?;
        tracker.0 = ArtifactExternalControllerStoreChangeV1::Unchanged;
        if let Err(failure) = revalidate_current_authority(authority, &binding, &state_root) {
            tracker.ambiguous();
            return release_file(lock, Err(failure));
        }
        if root_selection(&state_root.leaf)? != (false, true) {
            return release_file(lock, Err(ArtifactExternalControllerStoreFailureV1::Owner));
        }
        let instance = match draw_store_instance() {
            Ok(instance) => instance,
            Err(failure) => return release_file(lock, Err(failure)),
        };
        let state = match ArtifactExternalControllerStateV2::admit(request.clone(), instance) {
            Ok(state) => state,
            Err(_) => {
                return release_file(lock, Err(ArtifactExternalControllerStoreFailureV1::Owner));
            }
        };
        let bytes = match state.encode() {
            Ok(bytes) => bytes,
            Err(_) => {
                return release_file(lock, Err(ArtifactExternalControllerStoreFailureV1::Owner));
            }
        };
        tracker.ambiguous();
        let snapshot_identity = match write_new_exact(&staging, SNAPSHOT_NAME, &bytes) {
            Ok(identity) => identity,
            Err(failure) => return release_file(lock, Err(failure)),
        };
        publish_initial(
            authority,
            &binding,
            InitialPublication {
                state_root,
                staging,
                lock,
                lock_identity,
                state,
                bytes,
                snapshot_identity,
            },
            tracker,
        )
    }

    enum SnapshotRenameState {
        Old,
        New,
        ObservationUnavailable,
        OwnerInvalid,
    }

    fn classify_snapshot_rename(
        store: &ArtifactExternalDeploymentControllerLockedV1,
        next: &ArtifactExternalControllerStateV2,
        next_bytes: &[u8],
        next_identity: FileIdentity,
    ) -> SnapshotRenameState {
        let names = match scan_names(&store.root) {
            Ok(names) => names,
            Err(ArtifactExternalControllerStoreFailureV1::Io) => {
                return SnapshotRenameState::ObservationUnavailable;
            }
            Err(_) => return SnapshotRenameState::OwnerInvalid,
        };
        let stable = [OsString::from(LOCK_NAME), OsString::from(SNAPSHOT_NAME)]
            .into_iter()
            .collect::<BTreeSet<_>>();
        let with_next = [
            OsString::from(LOCK_NAME),
            OsString::from(SNAPSHOT_NAME),
            OsString::from(NEXT_NAME),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        if names == with_next {
            match (
                read_state(&store.root, SNAPSHOT_NAME),
                read_state(&store.root, NEXT_NAME),
            ) {
                (
                    Ok((active, active_bytes, active_identity)),
                    Ok((candidate, candidate_bytes, candidate_identity)),
                ) if active == store.state
                    && active_bytes == store.snapshot_bytes
                    && active_identity == store.snapshot_identity
                    && candidate == *next
                    && candidate_bytes.as_ref() == next_bytes
                    && candidate_identity == next_identity =>
                {
                    SnapshotRenameState::Old
                }
                (Err(ArtifactExternalControllerStoreFailureV1::Io), _)
                | (_, Err(ArtifactExternalControllerStoreFailureV1::Io)) => {
                    SnapshotRenameState::ObservationUnavailable
                }
                _ => SnapshotRenameState::OwnerInvalid,
            }
        } else if names == stable {
            match read_state(&store.root, SNAPSHOT_NAME) {
                Ok((active, active_bytes, active_identity))
                    if active == *next
                        && active_bytes.as_ref() == next_bytes
                        && active_identity == next_identity =>
                {
                    SnapshotRenameState::New
                }
                Err(ArtifactExternalControllerStoreFailureV1::Io) => {
                    SnapshotRenameState::ObservationUnavailable
                }
                _ => SnapshotRenameState::OwnerInvalid,
            }
        } else {
            SnapshotRenameState::OwnerInvalid
        }
    }

    fn cleanup_next(
        root: &DirectoryHandle,
        identity: FileIdentity,
    ) -> Result<(), ArtifactExternalControllerStoreFailureV1> {
        let (_, current) = open_regular_at(root, OsStr::new(NEXT_NAME), OFlag::O_RDONLY)?;
        if current != identity {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        unlinkat(&root.file, NEXT_NAME, UnlinkatFlags::NoRemoveDir)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        root.file
            .sync_all()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Io)?;
        if scan_names(root)?.contains(OsStr::new(NEXT_NAME)) {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        Ok(())
    }

    fn settle_successor(
        store: &mut ArtifactExternalDeploymentControllerLockedV1,
        input: SettleSuccessorInput<'_>,
        tracker: &mut ChangeTracker,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        let SettleSuccessorInput {
            authority,
            binding,
            next,
            next_bytes,
            next_identity,
            owned_next,
        } = input;
        revalidate_current_authority(authority, binding, &store.state_root)?;
        validate_public_store(store, true)?;
        tracker.ambiguous();
        if renameat(&store.root.file, NEXT_NAME, &store.root.file, SNAPSHOT_NAME).is_err() {
            match classify_snapshot_rename(store, &next, &next_bytes, next_identity) {
                SnapshotRenameState::Old => {
                    if !owned_next {
                        return Err(
                            ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(
                                Box::new(next),
                            )),
                        );
                    }
                    cleanup_next(&store.root, next_identity)?;
                    tracker.0 = ArtifactExternalControllerStoreChangeV1::Unchanged;
                    return Err(ArtifactExternalControllerStoreFailureV1::Io);
                }
                SnapshotRenameState::New => {}
                SnapshotRenameState::ObservationUnavailable => {
                    return Err(
                        ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(
                            Box::new(next),
                        )),
                    );
                }
                SnapshotRenameState::OwnerInvalid => {
                    return Err(ArtifactExternalControllerStoreFailureV1::Owner);
                }
            }
        }
        store.root.file.sync_all().map_err(|_| {
            ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(Box::new(
                next.clone(),
            )))
        })?;
        revalidate_current_authority(authority, binding, &store.state_root)
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
        if root_selection(&store.state_root.leaf)? != (true, false) {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        let root = reopen_named_directory(
            &store.state_root.leaf,
            OsStr::new(ROOT_NAME),
            store.root.identity,
        )?;
        exact_names(&root, &[LOCK_NAME, SNAPSHOT_NAME])?;
        validate_named_regular(&root, OsStr::new(LOCK_NAME), store.lock_identity, 0)?;
        let (reopened, reopened_bytes, reopened_identity) = read_state(&root, SNAPSHOT_NAME)?;
        if reopened != next || reopened_bytes != next_bytes || reopened_identity != next_identity {
            return Err(ArtifactExternalControllerStoreFailureV1::Owner);
        }
        store.snapshot_identity = reopened_identity;
        store.snapshot_bytes = reopened_bytes;
        store.state = reopened.clone();
        tracker.committed();
        Ok(reopened)
    }

    fn commit_successor(
        store: &mut ArtifactExternalDeploymentControllerLockedV1,
        authority: &mut dyn ArtifactExternalControllerAuthorityV1,
        binding: &ArtifactExternalControllerAuthorityBindingV1,
        next: ArtifactExternalControllerStateV2,
        tracker: &mut ChangeTracker,
    ) -> Result<ArtifactExternalControllerStateV2, ArtifactExternalControllerStoreFailureV1> {
        validate_config(&next, binding)?;
        permitted_successor(&store.state, &next)?;
        validate_public_store(store, true)?;
        if let Some(existing) = read_validated_next(store)? {
            if existing.state != next {
                return Err(ArtifactExternalControllerStoreFailureV1::Owner);
            }
            return settle_successor(
                store,
                SettleSuccessorInput {
                    authority,
                    binding,
                    next,
                    next_bytes: existing.bytes,
                    next_identity: existing.identity,
                    owned_next: false,
                },
                tracker,
            );
        }
        revalidate_current_authority(authority, binding, &store.state_root)?;
        let bytes = next
            .encode()
            .map_err(|_| ArtifactExternalControllerStoreFailureV1::Owner)?;
        tracker.ambiguous();
        let identity = write_new_exact(&store.root, NEXT_NAME, &bytes)?;
        settle_successor(
            store,
            SettleSuccessorInput {
                authority,
                binding,
                next,
                next_bytes: bytes,
                next_identity: identity,
                owned_next: true,
            },
            tracker,
        )
    }
}

/// Revalidation failures exposed by the narrow DeveloperLocal external
/// Controller authority callback. The callback returns values, never handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeveloperArtifactExternalControllerAuthorityRecheckFailureV1 {
    UnsafePath,
    Configuration,
    Io,
}

/// Current immutable authority binding for the external Controller store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeveloperArtifactExternalControllerAuthorityBindingV1 {
    state_root: std::path::PathBuf,
    config_commitment: ArtifactConfigCommitmentV1,
}

impl DeveloperArtifactExternalControllerAuthorityBindingV1 {
    pub fn try_new(
        state_root: std::path::PathBuf,
        config_commitment: ArtifactConfigCommitmentV1,
    ) -> Result<Self, DeveloperArtifactExternalControllerAuthorityRecheckFailureV1> {
        artifact_external_store::ArtifactExternalControllerAuthorityBindingV1::try_new(
            state_root.clone(),
            config_commitment,
        )
        .map_err(|_| DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::UnsafePath)?;
        Ok(Self {
            state_root,
            config_commitment,
        })
    }

    #[must_use]
    pub fn state_root(&self) -> &std::path::Path {
        &self.state_root
    }

    #[must_use]
    pub const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.config_commitment
    }
}

/// Revalidates the current config path before every Controller publication.
pub trait DeveloperArtifactExternalControllerAuthorityV1 {
    fn revalidate(
        &mut self,
    ) -> Result<
        DeveloperArtifactExternalControllerAuthorityBindingV1,
        DeveloperArtifactExternalControllerAuthorityRecheckFailureV1,
    >;
}

/// Fully validated fixed-profile input to the external Controller admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeveloperArtifactExternalControllerRequestV1 {
    operation_id: ArtifactDeploymentOperationIdV1,
    config_commitment: ArtifactConfigCommitmentV1,
    object_ref: ArtifactObjectRefV1,
    materialization_receipt_ref: MaterializationReceiptRefV1,
}

impl DeveloperArtifactExternalControllerRequestV1 {
    pub fn try_new(
        operation_id: ArtifactDeploymentOperationIdV1,
        config_commitment: ArtifactConfigCommitmentV1,
        object_ref: ArtifactObjectRefV1,
        materialization_receipt_ref: MaterializationReceiptRefV1,
    ) -> Option<Self> {
        let binding = ArtifactExecutionBindingV1::try_new(
            object_ref,
            materialization_receipt_ref,
            artifact_execution_profile_commitment_v1(),
        )
        .ok()?;
        ArtifactExternalDeploymentRequestV1::try_new(
            operation_id,
            config_commitment,
            binding,
        )
        .ok()?;
        Some(Self {
            operation_id,
            config_commitment,
            object_ref,
            materialization_receipt_ref,
        })
    }

    #[must_use]
    pub const fn operation_id(&self) -> ArtifactDeploymentOperationIdV1 {
        self.operation_id
    }

    #[must_use]
    pub const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.config_commitment
    }

    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }

    #[must_use]
    pub const fn materialization_receipt_ref(&self) -> MaterializationReceiptRefV1 {
        self.materialization_receipt_ref
    }
}

/// Public semantic phase. No PXMJ wire value or mutable owner handle escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeveloperArtifactExternalControllerPhaseV1 {
    Admitted,
    Committed,
    Applying,
    ActiveReady,
    Failed,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeveloperArtifactExternalControllerTerminalOutcomeV1 {
    ActiveReady,
    Failed,
    Uncertain,
}

/// Owned point-in-time projection consumed by the local JSON boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeveloperArtifactExternalControllerProjectionV1 {
    phase: DeveloperArtifactExternalControllerPhaseV1,
    operation_id: ArtifactDeploymentOperationIdV1,
    object_ref: ArtifactObjectRefV1,
    materialization_receipt_ref: MaterializationReceiptRefV1,
    lifecycle_generation: Option<[u8; 16]>,
    deployment_revision: Option<NonZeroU64>,
    committed_controller_snapshot_sequence: Option<NonZeroU64>,
    deployment_receipt_ref: Option<Box<str>>,
    runtime_apply_request_digest: Option<Digest32>,
    runtime_terminal_receipt_digest: Option<Digest32>,
    terminal_outcome: Option<DeveloperArtifactExternalControllerTerminalOutcomeV1>,
}

impl DeveloperArtifactExternalControllerProjectionV1 {
    #[must_use]
    pub const fn phase(&self) -> DeveloperArtifactExternalControllerPhaseV1 {
        self.phase
    }

    #[must_use]
    pub const fn operation_id(&self) -> ArtifactDeploymentOperationIdV1 {
        self.operation_id
    }

    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }

    #[must_use]
    pub const fn materialization_receipt_ref(&self) -> MaterializationReceiptRefV1 {
        self.materialization_receipt_ref
    }

    #[must_use]
    pub const fn lifecycle_generation(&self) -> Option<[u8; 16]> {
        self.lifecycle_generation
    }

    #[must_use]
    pub const fn deployment_revision(&self) -> Option<NonZeroU64> {
        self.deployment_revision
    }

    #[must_use]
    pub const fn committed_controller_snapshot_sequence(&self) -> Option<NonZeroU64> {
        self.committed_controller_snapshot_sequence
    }

    #[must_use]
    pub fn deployment_receipt_ref(&self) -> Option<&str> {
        self.deployment_receipt_ref.as_deref()
    }

    #[must_use]
    pub const fn runtime_apply_request_digest(&self) -> Option<Digest32> {
        self.runtime_apply_request_digest
    }

    #[must_use]
    pub const fn runtime_terminal_receipt_digest(&self) -> Option<Digest32> {
        self.runtime_terminal_receipt_digest
    }

    #[must_use]
    pub const fn terminal_outcome(
        &self,
    ) -> Option<DeveloperArtifactExternalControllerTerminalOutcomeV1> {
        self.terminal_outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeveloperArtifactExternalControllerFailureV1 {
    UnsafePath,
    ConfigurationMismatch,
    Conflict,
    ReplaceRequired,
    NotFound,
    Contended,
    PublicationUncertain(Option<Box<DeveloperArtifactExternalControllerProjectionV1>>),
    Owner,
    Io,
}

/// Fully owned result. `changed` is `None` only when owner mutation attribution
/// is no longer provable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeveloperArtifactExternalControllerInvocationV1 {
    changed: Option<bool>,
    result: Result<
        DeveloperArtifactExternalControllerProjectionV1,
        DeveloperArtifactExternalControllerFailureV1,
    >,
}

impl DeveloperArtifactExternalControllerInvocationV1 {
    #[must_use]
    pub const fn changed(&self) -> Option<bool> {
        self.changed
    }

    #[must_use]
    pub fn result(
        &self,
    ) -> Result<
        &DeveloperArtifactExternalControllerProjectionV1,
        &DeveloperArtifactExternalControllerFailureV1,
    > {
        self.result.as_ref()
    }

    pub fn into_result(
        self,
    ) -> Result<
        DeveloperArtifactExternalControllerProjectionV1,
        DeveloperArtifactExternalControllerFailureV1,
    > {
        self.result
    }
}

struct DeveloperArtifactExternalControllerAuthorityAdapter<'a> {
    authority: &'a mut dyn DeveloperArtifactExternalControllerAuthorityV1,
}

impl artifact_external_store::ArtifactExternalControllerAuthorityV1
    for DeveloperArtifactExternalControllerAuthorityAdapter<'_>
{
    fn revalidate(
        &mut self,
    ) -> Result<
        artifact_external_store::ArtifactExternalControllerAuthorityBindingV1,
        artifact_external_store::ArtifactExternalControllerAuthorityRecheckFailureV1,
    > {
        let binding = self.authority.revalidate().map_err(|failure| match failure {
            DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::UnsafePath => {
                artifact_external_store::ArtifactExternalControllerAuthorityRecheckFailureV1::UnsafePath
            }
            DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::Configuration => {
                artifact_external_store::ArtifactExternalControllerAuthorityRecheckFailureV1::Configuration
            }
            DeveloperArtifactExternalControllerAuthorityRecheckFailureV1::Io => {
                artifact_external_store::ArtifactExternalControllerAuthorityRecheckFailureV1::Io
            }
        })?;
        artifact_external_store::ArtifactExternalControllerAuthorityBindingV1::try_new(
            binding.state_root,
            binding.config_commitment,
        )
        .map_err(|_| {
            artifact_external_store::ArtifactExternalControllerAuthorityRecheckFailureV1::UnsafePath
        })
    }
}

/// Narrow one-shot facade over the owner-private PXMJ v2 store. The mutating
/// admission entrypoint is called only while the Local lifecycle owner lock is
/// already held; this facade does not create a second lifecycle authority.
pub struct DeveloperArtifactExternalControllerV1;

impl DeveloperArtifactExternalControllerV1 {
    pub fn admit_under_lifecycle_owner(
        authority: &mut dyn DeveloperArtifactExternalControllerAuthorityV1,
        request: &DeveloperArtifactExternalControllerRequestV1,
    ) -> DeveloperArtifactExternalControllerInvocationV1 {
        let Ok(request) = internal_external_request(request) else {
            return DeveloperArtifactExternalControllerInvocationV1 {
                changed: Some(false),
                result: Err(DeveloperArtifactExternalControllerFailureV1::Owner),
            };
        };
        let mut adapter = DeveloperArtifactExternalControllerAuthorityAdapter { authority };
        project_external_invocation(
            artifact_external_store::ArtifactExternalDeploymentControllerStoreV1::admit(
                &mut adapter,
                &request,
            ),
        )
    }

    pub fn query(
        authority: &mut dyn DeveloperArtifactExternalControllerAuthorityV1,
        operation_id: ArtifactDeploymentOperationIdV1,
    ) -> DeveloperArtifactExternalControllerInvocationV1 {
        let mut adapter = DeveloperArtifactExternalControllerAuthorityAdapter { authority };
        project_external_invocation(
            artifact_external_store::ArtifactExternalDeploymentControllerStoreV1::query(
                &mut adapter,
                operation_id,
            ),
        )
    }
}

fn internal_external_request(
    request: &DeveloperArtifactExternalControllerRequestV1,
) -> Result<ArtifactExternalDeploymentRequestV1, ManagedModelAgentStackApplyControllerError> {
    let binding = ArtifactExecutionBindingV1::try_new(
        request.object_ref,
        request.materialization_receipt_ref,
        artifact_execution_profile_commitment_v1(),
    )?;
    ArtifactExternalDeploymentRequestV1::try_new(
        request.operation_id,
        request.config_commitment,
        binding,
    )
}

fn project_external_invocation(
    invocation: artifact_external_store::ArtifactExternalControllerStoreInvocationV1,
) -> DeveloperArtifactExternalControllerInvocationV1 {
    let changed = match invocation.change() {
        artifact_external_store::ArtifactExternalControllerStoreChangeV1::Unchanged => Some(false),
        artifact_external_store::ArtifactExternalControllerStoreChangeV1::Changed => Some(true),
        artifact_external_store::ArtifactExternalControllerStoreChangeV1::Unknown => None,
    };
    let result = match invocation.into_result() {
        Ok(state) => project_external_state(&state),
        Err(failure) => Err(project_external_failure(failure)),
    };
    DeveloperArtifactExternalControllerInvocationV1 { changed, result }
}

fn project_external_failure(
    failure: artifact_external_store::ArtifactExternalControllerStoreFailureV1,
) -> DeveloperArtifactExternalControllerFailureV1 {
    match failure {
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::UnsafePath => {
            DeveloperArtifactExternalControllerFailureV1::UnsafePath
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::ConfigurationMismatch => {
            DeveloperArtifactExternalControllerFailureV1::ConfigurationMismatch
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::Conflict => {
            DeveloperArtifactExternalControllerFailureV1::Conflict
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::ReplaceRequired => {
            DeveloperArtifactExternalControllerFailureV1::ReplaceRequired
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::NotFound => {
            DeveloperArtifactExternalControllerFailureV1::NotFound
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::Contended => {
            DeveloperArtifactExternalControllerFailureV1::Contended
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::PublicationUncertain(
            state,
        ) => match state.map(|state| project_external_state(&state)).transpose() {
            Ok(state) => DeveloperArtifactExternalControllerFailureV1::PublicationUncertain(state),
            Err(_) => DeveloperArtifactExternalControllerFailureV1::Owner,
        },
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::Owner => {
            DeveloperArtifactExternalControllerFailureV1::Owner
        }
        artifact_external_store::ArtifactExternalControllerStoreFailureV1::Io => {
            DeveloperArtifactExternalControllerFailureV1::Io
        }
    }
}

fn project_external_state(
    state: &ArtifactExternalControllerStateV2,
) -> Result<
    DeveloperArtifactExternalControllerProjectionV1,
    DeveloperArtifactExternalControllerFailureV1,
> {
    let phase = match state.phase() {
        ArtifactExternalControllerPhaseV2::Admitted => {
            DeveloperArtifactExternalControllerPhaseV1::Admitted
        }
        ArtifactExternalControllerPhaseV2::Committed => {
            DeveloperArtifactExternalControllerPhaseV1::Committed
        }
        ArtifactExternalControllerPhaseV2::Applying => {
            DeveloperArtifactExternalControllerPhaseV1::Applying
        }
        ArtifactExternalControllerPhaseV2::ActiveReady => {
            DeveloperArtifactExternalControllerPhaseV1::ActiveReady
        }
        ArtifactExternalControllerPhaseV2::Failed => {
            DeveloperArtifactExternalControllerPhaseV1::Failed
        }
        ArtifactExternalControllerPhaseV2::Uncertain => {
            DeveloperArtifactExternalControllerPhaseV1::Uncertain
        }
    };
    let progress = state.records().last().map(|record| record.progress());
    let deployment_receipt_ref = match state.receipt() {
        Some(receipt) => Some(
            DeploymentReceiptRefV1::from_receipt(state.request(), state.admission(), receipt)
                .map_err(|_| DeveloperArtifactExternalControllerFailureV1::Owner)?
                .encode()
                .into_boxed_str(),
        ),
        None => None,
    };
    let terminal_outcome = match state.phase() {
        ArtifactExternalControllerPhaseV2::ActiveReady => {
            Some(DeveloperArtifactExternalControllerTerminalOutcomeV1::ActiveReady)
        }
        ArtifactExternalControllerPhaseV2::Failed => {
            Some(DeveloperArtifactExternalControllerTerminalOutcomeV1::Failed)
        }
        ArtifactExternalControllerPhaseV2::Uncertain => {
            Some(DeveloperArtifactExternalControllerTerminalOutcomeV1::Uncertain)
        }
        ArtifactExternalControllerPhaseV2::Admitted
        | ArtifactExternalControllerPhaseV2::Committed
        | ArtifactExternalControllerPhaseV2::Applying => None,
    };
    Ok(DeveloperArtifactExternalControllerProjectionV1 {
        phase,
        operation_id: state.request().operation_id(),
        object_ref: state.request().binding().object_ref(),
        materialization_receipt_ref: state.request().binding().materialization_receipt_ref(),
        lifecycle_generation: progress.and_then(ArtifactExternalDeploymentProgressV1::lifecycle_generation),
        deployment_revision: progress.and_then(ArtifactExternalDeploymentProgressV1::deployment_revision),
        committed_controller_snapshot_sequence: progress.and_then(
            ArtifactExternalDeploymentProgressV1::committed_controller_snapshot_sequence,
        ),
        deployment_receipt_ref,
        runtime_apply_request_digest: progress.and_then(
            ArtifactExternalDeploymentProgressV1::runtime_apply_request_digest,
        ),
        runtime_terminal_receipt_digest: progress.and_then(
            ArtifactExternalDeploymentProgressV1::runtime_terminal_receipt_digest,
        ),
        terminal_outcome,
    })
}

impl fmt::Display for ManagedModelAgentStackApplyControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "managed Model+Agent stack apply failed: {self:?}"
        )
    }
}

impl std::error::Error for ManagedModelAgentStackApplyControllerError {}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::io::Write as _;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::PathBuf;

    use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
    use paraegox_artifact::{
        ArtifactObjectRefV1, MaterializationReceiptRefV1, VerifiedArtifactPairV1,
    };
    use paraegox_kernel::digest::Digest32;
    use paraegox_kernel::time::BoundedDuration;
    use paraegox_runtime_contracts::apply::ExpectedActive;
    use paraegox_runtime_contracts::assignment::BindingId;
    use paraegox_runtime_contracts::managed_agent_stack_plan::{
        ManagedAgentIngressLimitsV1, ManagedAgentPortPlanV1, ManagedAgentProviderRefV1,
        ManagedAgentProviderSelectionV1, ManagedAgentSemanticLimitsV1, ManagedAgentServicePlanV1,
    };
    use paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricListenEndpointV1;
    use paraegox_runtime_contracts::managed_model_agent_stack_plan::{
        MANAGED_MODEL_AGENT_STACK_APPLY_REQUEST_VERSION,
        MANAGED_MODEL_AGENT_STACK_TARGET_EXECUTION_VERSION, ManagedModelAdapterBindingV1,
        ManagedModelAdapterVersionV1, ManagedModelAgentStackTerminalAuthClaimV1,
        ManagedModelAgentStackTerminalEvidenceFieldsV1, ManagedModelAgentStackTerminalEvidenceV1,
        ManagedModelAgentStackTerminalFactsV1, ManagedModelAgentStackTerminalHeadV1,
        ManagedModelAgentStackTerminalLifecycleEffectV1, ManagedModelAgentStackTerminalOutcomeV1,
        ManagedModelAgentStackTerminalReceiptDraftV1, ManagedModelAgentStackTerminalReceiptV1,
        ManagedModelAgentStackTerminalStateV1, ManagedModelCapabilityIdV1,
        ManagedModelServicePlanV1, artifact_execution_profile_commitment_v1,
    };
    use paraegox_runtime_contracts::managed_service::{
        ManagedServiceGeneration, ManagedServiceId, ManagedServiceLifecycleBudgetsV1,
        ManagedServiceSpecV1,
    };
    use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyAuthKeyRef};

    use super::artifact_external_store::{
        ArtifactExternalControllerAuthorityBindingV1,
        ArtifactExternalControllerAuthorityRecheckFailureV1, ArtifactExternalControllerAuthorityV1,
        ArtifactExternalControllerStoreChangeV1, ArtifactExternalControllerStoreFailureV1,
        ArtifactExternalDeploymentControllerStoreV1,
    };
    use super::*;
    use crate::managed_fabric_apply::{
        ManagedFabricApplyJournalV1, ManagedFabricControllerStateV1, tests as fabric_tests,
    };
    use crate::managed_model_agent_stack_producer::{
        ArtifactBoundManagedModelAgentStackDesiredInputV1,
        ArtifactBoundManagedModelAgentStackDesiredPlanV1,
        produce_artifact_bound_managed_model_agent_stack_request_v1,
    };

    const RUNTIME_KEY: ApplyAuthKeyRef = ApplyAuthKeyRef::from_bytes([0x38; 16]);

    struct ArtifactExternalStoreTestRoot {
        base: PathBuf,
        parent: PathBuf,
        device: u64,
        inode: u64,
    }

    impl ArtifactExternalStoreTestRoot {
        fn new() -> Self {
            let base = std::env::current_dir()
                .expect("current test directory")
                .canonicalize()
                .expect("canonical test directory");
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random).expect("test directory entropy");
            let suffix = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let parent = base.join(format!(".paraegox-external-store-test-{suffix}"));
            fs::create_dir(&parent).expect("create strict test parent");
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
                .expect("set strict test parent mode");
            let metadata = fs::symlink_metadata(&parent).expect("test parent metadata");
            let state_root = parent.join("state");
            fs::create_dir(&state_root).expect("create test state root");
            fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700))
                .expect("set test state-root mode");
            Self {
                base,
                parent,
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        }

        fn state_root(&self) -> PathBuf {
            self.parent.join("state")
        }
    }

    impl Drop for ArtifactExternalStoreTestRoot {
        fn drop(&mut self) {
            let exact_child = self.parent.parent() == Some(self.base.as_path())
                && self
                    .parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".paraegox-external-store-test-"));
            let same_directory = fs::symlink_metadata(&self.parent).is_ok_and(|metadata| {
                metadata.file_type().is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode
            });
            if exact_child && same_directory {
                fs::remove_dir_all(&self.parent).expect("remove exact test directory");
            }
        }
    }

    #[derive(Clone)]
    struct FixedArtifactExternalAuthority {
        binding: ArtifactExternalControllerAuthorityBindingV1,
    }

    impl ArtifactExternalControllerAuthorityV1 for FixedArtifactExternalAuthority {
        fn revalidate(
            &mut self,
        ) -> Result<
            ArtifactExternalControllerAuthorityBindingV1,
            ArtifactExternalControllerAuthorityRecheckFailureV1,
        > {
            Ok(self.binding.clone())
        }
    }

    #[derive(Clone)]
    struct FixedDeveloperArtifactExternalAuthority {
        binding: DeveloperArtifactExternalControllerAuthorityBindingV1,
    }

    impl DeveloperArtifactExternalControllerAuthorityV1
        for FixedDeveloperArtifactExternalAuthority
    {
        fn revalidate(
            &mut self,
        ) -> Result<
            DeveloperArtifactExternalControllerAuthorityBindingV1,
            DeveloperArtifactExternalControllerAuthorityRecheckFailureV1,
        > {
            Ok(self.binding.clone())
        }
    }

    fn artifact_external_request() -> ArtifactExternalDeploymentRequestV1 {
        let pair =
            VerifiedArtifactPairV1::from_payload(b"literal-prefix-v1 ").expect("Artifact pair");
        let receipt = format!(
            "pxamr1:{}:7:{}:{}",
            "a1".repeat(32),
            "a2".repeat(16),
            "a3".repeat(32),
        )
        .parse::<MaterializationReceiptRefV1>()
        .expect("materialization Receipt ref");
        let binding = ArtifactExecutionBindingV1::try_new(
            pair.object_ref(),
            receipt,
            artifact_execution_profile_commitment_v1(),
        )
        .expect("Artifact execution binding");
        ArtifactExternalDeploymentRequestV1::try_new(
            ArtifactDeploymentOperationIdV1::try_from_bytes([0x45; 16])
                .expect("deployment operation id"),
            ArtifactConfigCommitmentV1::try_from_bytes([0x44; 32]).expect("config commitment"),
            binding,
        )
        .expect("PXDQ")
    }

    fn decode_fixture_hex(value: &str) -> Vec<u8> {
        let value = value.strip_suffix('\n').expect("fixture LF");
        assert_eq!(value.len() % 2, 0);
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|chunk| {
                u8::from_str_radix(std::str::from_utf8(chunk).expect("fixture UTF-8"), 16)
                    .expect("fixture lower hex")
            })
            .collect()
    }

    #[test]
    fn artifact_external_request_and_admission_match_shared_hardcoded_goldens() {
        let object_ref = ArtifactObjectRefV1::decode(&decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxak_v1.hex"
        )))
        .expect("shared PXAK");
        let receipt = format!(
            "pxamr1:{}:1:{}:{}",
            "a0".repeat(32),
            "a2".repeat(16),
            "ef84d5cb34af9263ce2b933f3d0e4e45f9896f6805fb54e5841b5c91950110f3",
        )
        .parse::<MaterializationReceiptRefV1>()
        .expect("primary materialization Receipt ref");
        let binding = ArtifactExecutionBindingV1::try_new(
            object_ref,
            receipt,
            artifact_execution_profile_commitment_v1(),
        )
        .expect("shared binding");
        assert_eq!(
            binding.canonical_wire().as_slice(),
            decode_fixture_hex(include_str!(
                "../../../tests/fixtures/wire/artifact_f0_binding_v1.hex"
            )),
        );
        assert_eq!(
            ArtifactExecutionBindingV1::decode(&decode_fixture_hex(include_str!(
                "../../../tests/fixtures/wire/artifact_f0_binding_v1.hex"
            )))
            .expect("decoded shared Artifact execution binding"),
            binding,
        );
        assert_eq!(
            artifact_execution_profile_commitment_v1().as_bytes(),
            &[
                0x1f, 0xe2, 0x43, 0xfd, 0x90, 0x34, 0xf0, 0x4d, 0xae, 0xc0, 0xc0, 0x23, 0x66, 0x1c,
                0x6f, 0x3e, 0x8b, 0x48, 0x78, 0xf6, 0xea, 0xab, 0x6b, 0xa0, 0x59, 0x4b, 0x37, 0x16,
                0x1e, 0xc8, 0x8c, 0x1e,
            ],
        );
        let execution_wire = decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxte_v11.hex"
        ));
        let execution =
            ArtifactBoundManagedModelAgentStackTargetExecutionV1::decode(&execution_wire)
                .expect("decoded shared PXTE11");
        assert_eq!(execution.canonical_wire(), execution_wire);
        assert_eq!(execution.binding(), binding);
        let plan_content_wire = decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_plan_content_v2.hex"
        ));
        let plan_content = ArtifactBoundManagedModelAgentStackPlanContentV2::decode(
            paraegox_kernel::identity::RuntimeHostId::from_bytes([0x05; 16]),
            &plan_content_wire,
        )
        .expect("decoded shared PlanContent v2");
        assert_eq!(plan_content.canonical_bytes(), plan_content_wire);
        assert_eq!(plan_content.binding(), binding);
        assert_eq!(plan_content.execution(), &execution);
        let runtime_slice_wire = decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_runtime_slice_v11.hex"
        ));
        let runtime_request_wire = decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxar_v12.hex"
        ));
        let runtime_request =
            ArtifactBoundManagedModelAgentStackApplyRequestV1::decode(&runtime_request_wire)
                .expect("decoded shared PXAR12");
        assert_eq!(runtime_request.canonical_wire(), runtime_request_wire);
        assert_eq!(runtime_request.canonical_slice_wire(), runtime_slice_wire);
        assert_eq!(runtime_request.target_execution(), &execution);
        assert_eq!(runtime_request.target().as_bytes(), &[0x05; 16]);
        assert_eq!(runtime_request.operation_id().as_bytes(), &[0xd4; 16]);
        assert_eq!(runtime_request.provenance().source_revision().value(), 4);
        runtime_request
            .validate_expected_store([0x44; 32])
            .expect("shared Runtime store pin");
        let request = ArtifactExternalDeploymentRequestV1::try_new(
            ArtifactDeploymentOperationIdV1::try_from_bytes([0xd1; 16])
                .expect("deployment operation id"),
            ArtifactConfigCommitmentV1::try_from_bytes([0xa1; 32]).expect("config commitment"),
            binding,
        )
        .expect("shared PXDQ");
        assert_eq!(
            request.canonical_wire().as_slice(),
            decode_fixture_hex(include_str!(
                "../../../tests/fixtures/wire/artifact_f0_pxdq_v1.hex"
            )),
        );
        assert_eq!(
            ArtifactExternalDeploymentRequestV1::decode(&decode_fixture_hex(include_str!(
                "../../../tests/fixtures/wire/artifact_f0_pxdq_v1.hex"
            )))
            .expect("decoded shared PXDQ"),
            request,
        );
        let admission = ArtifactExternalDeploymentAdmissionV1::try_new(
            [0xd0; 32],
            NonZeroU64::new(1).expect("admission sequence"),
            &request,
        )
        .expect("shared PXDK");
        assert_eq!(
            admission.canonical_wire().as_slice(),
            decode_fixture_hex(include_str!(
                "../../../tests/fixtures/wire/artifact_f0_pxdk_v1.hex"
            )),
        );
        assert_eq!(
            ArtifactExternalDeploymentAdmissionV1::decode(
                &decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxdk_v1.hex"
                )),
                &request,
            )
            .expect("decoded shared PXDK"),
            admission,
        );
    }

    #[test]
    fn artifact_runtime_terminal_shared_goldens_decode_correlate_and_verify() {
        let runtime_request =
            ArtifactBoundManagedModelAgentStackApplyRequestV1::decode(&decode_fixture_hex(
                include_str!("../../../tests/fixtures/wire/artifact_f0_pxar_v12.hex"),
            ))
            .expect("decoded shared PXAR12");
        let fixtures = [
            (
                include_str!("../../../tests/fixtures/wire/artifact_f0_pxmt_artifact_v1.hex"),
                ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
                Some(7),
                Some(8),
                Some(9),
                2,
                true,
                true,
                true,
                true,
                true,
                true,
                false,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmt_artifact_no_effect_rejected_v1.hex"
                ),
                ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected,
                Some(7),
                None,
                None,
                0,
                true,
                true,
                false,
                false,
                false,
                false,
                false,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmt_artifact_uncertain_v1.hex"
                ),
                ManagedModelAgentStackTerminalOutcomeV1::Uncertain,
                Some(7),
                Some(8),
                None,
                0,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmt_artifact_quarantined_v1.hex"
                ),
                ManagedModelAgentStackTerminalOutcomeV1::Quarantined,
                Some(7),
                Some(8),
                None,
                0,
                true,
                true,
                false,
                false,
                false,
                false,
                true,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmt_artifact_quarantined_after_agent_intent_v1.hex"
                ),
                ManagedModelAgentStackTerminalOutcomeV1::Quarantined,
                Some(7),
                Some(8),
                Some(9),
                0,
                false,
                true,
                false,
                false,
                false,
                false,
                true,
            ),
        ];
        let runtime_verifier = SigningKey::from_bytes(&[0x77; 32]).verifying_key();
        let mut receipt_digests = Vec::new();
        for (
            fixture,
            expected_outcome,
            expected_fabric_generation,
            expected_model_generation,
            expected_agent_generation,
            expected_census,
            expected_census_complete,
            expected_fabric_ready,
            expected_model_ready,
            expected_agent_ready,
            expected_fabric_dependency_ready,
            expected_model_dependency_ready,
            expected_quarantined,
        ) in fixtures
        {
            let wire = decode_fixture_hex(fixture);
            assert_eq!(wire.len(), 591);
            let terminal = ManagedModelAgentStackTerminalReceiptV1::decode(&wire)
                .expect("decoded shared PXMT");
            assert_eq!(terminal.canonical_wire(), wire);
            let facts = terminal
                .validate_artifact_request_correlation(&runtime_request)
                .expect("PXMT correlates with shared PXAR12");
            let state = facts.state();
            assert_eq!(state.outcome(), expected_outcome);
            assert_eq!(
                state
                    .fabric_generation()
                    .map(ManagedServiceGeneration::value),
                expected_fabric_generation,
            );
            assert_eq!(
                state
                    .model_generation()
                    .map(ManagedServiceGeneration::value),
                expected_model_generation,
            );
            assert_eq!(
                state
                    .agent_generation()
                    .map(ManagedServiceGeneration::value),
                expected_agent_generation,
            );
            let evidence = facts.evidence().fields();
            assert_eq!(evidence.physical_binding_census, expected_census);
            assert_eq!(evidence.census_complete, expected_census_complete);
            assert_eq!(evidence.fabric_ready, expected_fabric_ready);
            assert_eq!(evidence.model_ready, expected_model_ready);
            assert_eq!(evidence.agent_ready, expected_agent_ready);
            assert_eq!(
                evidence.fabric_to_agent_dependency_ready,
                expected_fabric_dependency_ready,
            );
            assert_eq!(
                evidence.model_to_agent_dependency_ready,
                expected_model_dependency_ready,
            );
            assert_eq!(evidence.quarantined, expected_quarantined);
            assert_eq!(terminal.authentication_key().as_bytes(), &[0x76; 16]);
            assert_eq!(terminal.authentication_algorithm().value(), 1);
            assert_eq!(terminal.authentication_algorithm_version(), 1);
            let signature = Signature::from_slice(terminal.authentication_signature())
                .expect("canonical Ed25519 signature");
            runtime_verifier
                .verify(
                    terminal
                        .signing_transcript()
                        .expect("PXMT signing transcript")
                        .as_bytes(),
                    &signature,
                )
                .expect("runtime test key verifies PXMT");
            assert!(!receipt_digests.contains(&terminal.receipt_digest()));
            receipt_digests.push(terminal.receipt_digest());
        }
        assert_eq!(receipt_digests.len(), 5);
    }

    #[test]
    fn artifact_external_request_and_admission_are_exact_correlated_frames() {
        let request = artifact_external_request();
        assert_eq!(request.canonical_wire().len(), EXTERNAL_REQUEST_BYTES);
        assert_eq!(&request.canonical_wire()[0..4], b"PXDQ");
        assert_eq!(
            ArtifactExternalDeploymentRequestV1::decode(request.canonical_wire())
                .expect("decoded PXDQ"),
            request,
        );

        let admission = ArtifactExternalDeploymentAdmissionV1::try_new(
            [0x46; 32],
            NonZeroU64::new(1).expect("admission sequence"),
            &request,
        )
        .expect("PXDK");
        assert_eq!(admission.canonical_wire().len(), EXTERNAL_ADMISSION_BYTES);
        assert_eq!(&admission.canonical_wire()[0..4], b"PXDK");
        assert_eq!(
            ArtifactExternalDeploymentAdmissionV1::decode(admission.canonical_wire(), &request)
                .expect("decoded PXDK"),
            admission,
        );
        assert_eq!(
            ArtifactExternalDeploymentAdmissionV1::try_new(
                [0x46; 32],
                NonZeroU64::new(1).expect("admission sequence"),
                &request,
            )
            .expect("replayed PXDK"),
            admission,
        );

        let successor = ArtifactExternalDeploymentAdmissionV1::try_new(
            [0x46; 32],
            NonZeroU64::new(2).expect("successor sequence"),
            &request,
        )
        .expect("owner-private successor vector");
        assert_eq!(successor.admission_sequence().get(), 2);
        assert_ne!(successor.admission_digest(), admission.admission_digest());

        let mut corrupt_request = *request.canonical_wire();
        corrupt_request[7] = 1;
        assert!(ArtifactExternalDeploymentRequestV1::decode(&corrupt_request).is_err());
        corrupt_request = *request.canonical_wire();
        corrupt_request[287] ^= 1;
        assert!(ArtifactExternalDeploymentRequestV1::decode(&corrupt_request).is_err());

        let mut corrupt_admission = *admission.canonical_wire();
        corrupt_admission[176] = 1;
        assert!(
            ArtifactExternalDeploymentAdmissionV1::decode(&corrupt_admission, &request).is_err()
        );
        corrupt_admission = *admission.canonical_wire();
        corrupt_admission[239] ^= 1;
        assert!(
            ArtifactExternalDeploymentAdmissionV1::decode(&corrupt_admission, &request).is_err()
        );

        let other_request = ArtifactExternalDeploymentRequestV1::try_new(
            ArtifactDeploymentOperationIdV1::try_from_bytes([0x47; 16])
                .expect("other deployment operation id"),
            request.config_commitment(),
            request.binding(),
        )
        .expect("other PXDQ");
        assert!(
            ArtifactExternalDeploymentAdmissionV1::decode(
                admission.canonical_wire(),
                &other_request,
            )
            .is_err()
        );
        assert!(ArtifactDeploymentOperationIdV1::try_from_bytes([0; 16]).is_none());
        assert!(
            ArtifactExternalDeploymentAdmissionV1::try_new(
                [0; 32],
                NonZeroU64::new(1).expect("admission sequence"),
                &request,
            )
            .is_err()
        );
    }

    #[test]
    fn artifact_external_record_and_receipt_preserve_verified_prefixes() {
        let request = artifact_external_request();
        let admission = ArtifactExternalDeploymentAdmissionV1::try_new(
            [0x46; 32],
            NonZeroU64::new(1).expect("admission sequence"),
            &request,
        )
        .expect("PXDK");
        let committed_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(Digest32::from_bytes([0x48; 32])),
            None,
            None,
            None,
        )
        .expect("committed progress");
        let committed = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Committed,
            NonZeroU64::new(1).expect("record sequence"),
            &request,
            &admission,
            committed_progress,
            None,
        )
        .expect("PXDM-C");
        assert_eq!(committed.canonical_wire().len(), EXTERNAL_RECORD_BYTES);
        assert_eq!(
            ArtifactExternalDeploymentRecordV1::decode(
                committed.canonical_wire(),
                &request,
                &admission,
                None,
            )
            .expect("decoded PXDM-C"),
            committed,
        );

        let applying_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(Digest32::from_bytes([0x48; 32])),
            Some(Digest32::from_bytes([0x49; 32])),
            None,
            Some([0x4a; 16]),
        )
        .expect("applying progress");
        let applying = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Applying,
            NonZeroU64::new(2).expect("record sequence"),
            &request,
            &admission,
            applying_progress,
            Some(&committed),
        )
        .expect("PXDM-P");
        assert_eq!(
            ArtifactExternalDeploymentRecordV1::decode(
                applying.canonical_wire(),
                &request,
                &admission,
                Some(&committed),
            )
            .expect("decoded PXDM-P"),
            applying,
        );

        let ready_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(Digest32::from_bytes([0x48; 32])),
            Some(Digest32::from_bytes([0x49; 32])),
            Some(Digest32::from_bytes([0x4b; 32])),
            Some([0x4a; 16]),
        )
        .expect("ready progress");
        let ready = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::ActiveReady,
            NonZeroU64::new(3).expect("record sequence"),
            &request,
            &admission,
            ready_progress,
            Some(&applying),
        )
        .expect("PXDM-R");
        let ready_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &ready,
        )
        .expect("PXDO-R");
        assert_eq!(ready_receipt.canonical_wire().len(), EXTERNAL_RECEIPT_BYTES);
        assert_eq!(
            ArtifactExternalDeploymentReceiptV1::decode(
                ready_receipt.canonical_wire(),
                &request,
                &admission,
                &ready,
            )
            .expect("decoded PXDO-R"),
            ready_receipt,
        );
        let receipt_ref =
            DeploymentReceiptRefV1::from_receipt(&request, &admission, &ready_receipt)
                .expect("deployment Receipt ref");
        assert_eq!(
            receipt_ref
                .encode()
                .parse::<DeploymentReceiptRefV1>()
                .expect("reparsed deployment Receipt ref"),
            receipt_ref,
        );
        assert!(
            receipt_ref
                .encode()
                .to_ascii_uppercase()
                .parse::<DeploymentReceiptRefV1>()
                .is_err()
        );
        assert!(
            receipt_ref
                .encode()
                .replacen(":1:", ":01:", 1)
                .parse::<DeploymentReceiptRefV1>()
                .is_err()
        );

        let failed_progress = ArtifactExternalDeploymentProgressV1::try_new(
            None,
            None,
            None,
            None,
            None,
            Some([0x4c; 16]),
        )
        .expect("pre-commit failed progress");
        let failed = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Failed,
            NonZeroU64::new(1).expect("record sequence"),
            &request,
            &admission,
            failed_progress,
            None,
        )
        .expect("PXDM-F");
        ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &failed,
        )
        .expect("PXDO-F");

        let uncertain = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Uncertain,
            NonZeroU64::new(3).expect("record sequence"),
            &request,
            &admission,
            applying_progress,
            Some(&applying),
        )
        .expect("PXDM-U");
        ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &uncertain,
        )
        .expect("PXDO-U");

        assert!(
            ArtifactExternalDeploymentRecordV1::try_new(
                ArtifactExternalDeploymentRecordStateV1::Failed,
                NonZeroU64::new(1).expect("record sequence"),
                &request,
                &admission,
                committed_progress,
                None,
            )
            .is_err()
        );
        assert!(
            ArtifactExternalDeploymentRecordV1::try_new(
                ArtifactExternalDeploymentRecordStateV1::Failed,
                NonZeroU64::new(4).expect("record sequence"),
                &request,
                &admission,
                ready_progress,
                Some(&ready),
            )
            .is_err()
        );
        let mut reserved_record = *ready.canonical_wire();
        reserved_record[432] = 1;
        assert!(
            ArtifactExternalDeploymentRecordV1::decode(
                &reserved_record,
                &request,
                &admission,
                Some(&applying),
            )
            .is_err()
        );
        let mut reserved_receipt = *ready_receipt.canonical_wire();
        reserved_receipt[368] = 1;
        assert!(
            ArtifactExternalDeploymentReceiptV1::decode(
                &reserved_receipt,
                &request,
                &admission,
                &ready,
            )
            .is_err()
        );
    }

    fn budgets(values: [u64; 5]) -> ManagedServiceLifecycleBudgetsV1 {
        ManagedServiceLifecycleBudgetsV1::try_new(
            BoundedDuration::from_nanos(values[0]),
            BoundedDuration::from_nanos(values[1]),
            BoundedDuration::from_nanos(values[2]),
            BoundedDuration::from_nanos(values[3]),
            BoundedDuration::from_nanos(values[4]),
        )
        .expect("fixture lifecycle budgets")
    }

    fn provider(seed: u8) -> ManagedAgentProviderSelectionV1 {
        ManagedAgentProviderSelectionV1::try_deterministic_fixture(
            ManagedAgentProviderRefV1::try_from_bytes([seed; 16]).expect("provider ref"),
            Digest32::from_bytes([seed.wrapping_add(1); 32]),
        )
        .expect("provider selection")
    }

    fn agent_plan(selection: ManagedAgentProviderSelectionV1) -> ManagedAgentServicePlanV1 {
        let ingress = ManagedAgentIngressLimitsV1::try_new(
            64,
            512 * 1024,
            128 * 1024,
            128 * 1024,
            5_000_000_000,
        )
        .expect("bounded ingress");
        ManagedAgentServicePlanV1::try_new(
            ManagedServiceSpecV1::new(
                ManagedServiceId::from_bytes([0x88; 16]),
                budgets([7, 11, 13, 17, 19]),
            ),
            ManagedAgentSemanticLimitsV1::try_new(16, 64, 64, 64).expect("semantic limits"),
            ManagedAgentPortPlanV1::try_new(
                BindingId::from_bytes([0x81; 16]),
                BindingId::from_bytes([0x82; 16]),
                "paraegox/agent/v1/submit",
                "paraegox/agent/v1/control",
                ingress,
            )
            .expect("Agent port"),
            selection,
        )
        .expect("Agent plan")
    }

    fn model_plan(selection: ManagedAgentProviderSelectionV1) -> ManagedModelServicePlanV1 {
        ManagedModelServicePlanV1::try_new(
            ManagedServiceSpecV1::new(
                ManagedServiceId::from_bytes([0x89; 16]),
                budgets([23, 29, 31, 37, 41]),
            ),
            8,
            selection,
            ManagedModelAdapterBindingV1::try_new(
                [0x90; 16],
                ManagedModelAdapterVersionV1::try_new(7).expect("adapter version"),
                ManagedModelCapabilityIdV1::bounded_text_v1(),
            )
            .expect("adapter binding"),
        )
        .expect("Model plan")
    }

    fn artifact_model_plan(
        selection: ManagedAgentProviderSelectionV1,
    ) -> ManagedModelServicePlanV1 {
        ManagedModelServicePlanV1::try_new(
            ManagedServiceSpecV1::new(
                ManagedServiceId::from_bytes([0x89; 16]),
                budgets([23, 29, 31, 37, 41]),
            ),
            8,
            selection,
            ManagedModelAdapterBindingV1::try_new(
                *b"px-art-prefix-v1",
                ManagedModelAdapterVersionV1::try_new(1).expect("Artifact adapter version"),
                ManagedModelCapabilityIdV1::bounded_text_v1(),
            )
            .expect("Artifact adapter binding"),
        )
        .expect("Artifact Model plan")
    }

    fn activation(
        selection: ManagedAgentProviderSelectionV1,
        state: &ManagedFabricControllerStateV1,
    ) -> ManagedModelAgentStackActivationV1 {
        ManagedModelAgentStackActivationV1::try_new(
            state
                .desired()
                .expect("active Fabric desired")
                .execution()
                .clone(),
            agent_plan(selection),
            model_plan(selection),
        )
        .expect("A2 activation")
    }

    fn fresh(marker: u8) -> FreshManagedModelAgentStackApplyV1 {
        FreshManagedModelAgentStackApplyV1::try_new(
            [marker; 16],
            [marker.wrapping_add(1); 16],
            [marker.wrapping_add(2); 32],
        )
        .expect("fresh A2 identities")
    }

    fn active_fabric_state() -> ManagedFabricControllerStateV1 {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let mut journal: ManagedFabricApplyJournalV1 = fabric_tests::journal();
        let prepared = journal
            .prepare_activate_with(
                &controller,
                &provisioning,
                fabric_tests::service(),
                ManagedFabricListenEndpointV1::try_new("tcp/127.0.0.1:7447")
                    .expect("Fabric endpoint"),
                fabric_tests::fresh(0x91),
                |_| Ok(()),
            )
            .expect("prepare Fabric");
        let action = journal
            .claim_send_with(prepared, &controller, &provisioning, |_| Ok(()))
            .expect("claim Fabric send");
        let receipt = fabric_tests::active_receipt(action.request());
        journal
            .consume_pxft_with(
                action,
                receipt.canonical_wire(),
                &controller,
                &provisioning,
                |_| Ok(()),
            )
            .expect("commit active Fabric");
        journal.state().clone()
    }

    fn prepared_journal() -> (
        ManagedModelAgentStackApplyJournalV1,
        PreparedManagedModelAgentStackApplyV1,
    ) {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let state = active_fabric_state();
        let requested = activation(provider(0x83), &state);
        let mut journal = ManagedModelAgentStackApplyJournalV1::new(state);
        let prepared = journal
            .prepare_activate_with(&controller, &provisioning, &requested, fresh(0x92), |_| {
                Ok(())
            })
            .expect("prepare PXAR9");
        (journal, prepared)
    }

    fn uncertain_journal() -> (
        ManagedModelAgentStackApplyJournalV1,
        ManagedModelAgentStackSendActionV1,
    ) {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let (mut journal, prepared) = prepared_journal();
        let action = journal
            .claim_send_with(prepared, &controller, &provisioning, |_| Ok(()))
            .expect("claim PXAR9");
        (journal, action)
    }

    fn signed_receipt(
        request: &ManagedModelAgentStackApplyRequestV1,
        outcome: ManagedModelAgentStackTerminalOutcomeV1,
        fabric_generation: u64,
    ) -> ManagedModelAgentStackTerminalReceiptV1 {
        let generation =
            |value| Some(ManagedServiceGeneration::try_new(value).expect("service generation"));
        let (lifecycle, head, fabric, model, agent) = match outcome {
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                generation(fabric_generation),
                generation(2),
                generation(3),
            ),
            ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                None,
                None,
                None,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::ProvenNotStarted,
                ManagedModelAgentStackTerminalHeadV1::PreservedNone,
                None,
                None,
                None,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::Uncertain => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                generation(fabric_generation),
                None,
                None,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::Quarantined => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                generation(fabric_generation),
                generation(2),
                None,
            ),
        };
        let state = ManagedModelAgentStackTerminalStateV1::try_new(
            outcome, lifecycle, head, fabric, model, agent,
        )
        .expect("terminal state");
        let (
            physical_binding_census,
            census_complete,
            fabric_ready,
            model_ready,
            agent_ready,
            fabric_dependency,
            model_dependency,
            exact_zero,
            quarantined,
        ) = match outcome {
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady => {
                (2, true, true, true, true, true, true, false, false)
            }
            ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero
            | ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected => {
                (0, true, false, false, false, false, false, true, false)
            }
            ManagedModelAgentStackTerminalOutcomeV1::Uncertain => {
                (1, false, true, false, false, false, false, false, false)
            }
            ManagedModelAgentStackTerminalOutcomeV1::Quarantined => {
                (1, true, true, false, false, false, false, false, true)
            }
        };
        let evidence = ManagedModelAgentStackTerminalEvidenceV1::try_new(
            ManagedModelAgentStackTerminalEvidenceFieldsV1 {
                physical_binding_census,
                census_complete,
                fabric_ready,
                model_ready,
                agent_ready,
                fabric_to_agent_dependency_ready: fabric_dependency,
                model_to_agent_dependency_ready: model_dependency,
                exact_zero,
                quarantined,
                resource_census_digest: Digest32::from_bytes([0xa1; 32]),
                raw_outcome_digest: Digest32::from_bytes([0xa2; 32]),
                completion_runtime_host_epoch: 12,
                completion_snapshot_sequence: 13,
                selection_clock_generation: request.temporal().target_clock_generation(),
                selection_observed_at_nanos: 14,
            },
        )
        .expect("terminal evidence");
        let facts = ManagedModelAgentStackTerminalFactsV1::try_new(request, state, evidence)
            .expect("terminal facts");
        let channel = fabric_tests::channel();
        let auth = ManagedModelAgentStackTerminalAuthClaimV1::try_new(
            channel,
            RUNTIME_KEY,
            ApplyAuthAlgorithm::try_new(1).expect("algorithm"),
            1,
        )
        .expect("terminal auth");
        let draft =
            ManagedModelAgentStackTerminalReceiptDraftV1::try_new(request, facts, channel, auth)
                .expect("PXMT draft");
        let runtime: SigningKey = fabric_tests::runtime_signer();
        let signature = runtime.sign(
            draft
                .signing_transcript()
                .expect("PXMT transcript")
                .as_bytes(),
        );
        draft.finalize(&signature.to_bytes()).expect("signed PXMT")
    }

    fn reopen_stack(
        state: &ManagedFabricControllerStateV1,
    ) -> ManagedModelAgentStackControllerStateV1 {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let context = state
            .verified_current_context(&controller, &provisioning)
            .expect("verified context");
        let desired = state.desired().expect("Fabric desired");
        let request = state.request().expect("Fabric request");
        let generation = state
            .receipt()
            .and_then(|receipt| receipt.facts().generation())
            .expect("Fabric generation");
        let stack = state.model_agent_stack_state().expect("A2 state");
        let wire = stack.encode().expect("PXMJ encode");
        ManagedModelAgentStackControllerStateV1::decode(
            &wire,
            ManagedModelAgentStackDecodeContextV1 {
                fabric: &context,
                cutover_marker_digest: state.cutover_marker_digest(),
                predecessor_revision: desired.revision(),
                predecessor_execution: desired.execution(),
                predecessor_slice_digest: request.target_slice_digest(),
                predecessor_generation: generation,
            },
        )
        .expect("PXMJ reopen")
    }

    #[test]
    fn producer_commits_pxte8_pxar9_revision_cas_budget_provider_and_adapter() {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let state = active_fabric_state();
        let predecessor_revision = state.desired().expect("Fabric desired").revision();
        let predecessor_slice = state
            .request()
            .expect("Fabric request")
            .target_slice_digest();
        let selection = provider(0x83);
        let requested = activation(selection, &state);
        let mut journal = ManagedModelAgentStackApplyJournalV1::new(state);
        let crossed = Cell::new(false);
        let _prepared = journal
            .prepare_activate_with(
                &controller,
                &provisioning,
                &requested,
                fresh(0x92),
                |next| {
                    let stack = next.model_agent_stack_state().expect("durable A2");
                    assert_eq!(
                        stack.phase(),
                        ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
                    );
                    crossed.set(true);
                    Ok(())
                },
            )
            .expect("prepare A2");
        assert!(crossed.get());
        let stack = journal.state().model_agent_stack_state().expect("A2 state");
        assert_eq!(
            stack.desired().revision().value(),
            predecessor_revision.value() + 1
        );
        assert_eq!(
            stack.desired().predecessor_slice_digest(),
            predecessor_slice
        );
        assert_eq!(
            stack
                .request()
                .control_commitment()
                .control()
                .expected_active(),
            ExpectedActive::Exact(predecessor_slice)
        );
        assert_eq!(&stack.desired().execution().canonical_wire()[..4], b"PXTE");
        assert_eq!(
            u16::from_be_bytes(
                stack.desired().execution().canonical_wire()[4..6]
                    .try_into()
                    .expect("PXTE version")
            ),
            MANAGED_MODEL_AGENT_STACK_TARGET_EXECUTION_VERSION
        );
        assert_eq!(&stack.request().canonical_wire()[..4], b"PXAR");
        assert_eq!(
            u16::from_be_bytes(
                stack.request().canonical_wire()[4..6]
                    .try_into()
                    .expect("PXAR version")
            ),
            MANAGED_MODEL_AGENT_STACK_APPLY_REQUEST_VERSION
        );
        assert_eq!(stack.request().temporal().original_budget().value(), 114);
        let model = stack.desired().execution().model().expect("Model plan");
        assert_eq!(model.provider(), selection);
        assert_eq!(model.adapter_binding().adapter_id(), &[0x90; 16]);
        assert_eq!(model.adapter_binding().adapter_version().value(), 7);

        let mismatch = ManagedModelAgentStackActivationV1::try_new(
            requested.expected_fabric().clone(),
            requested.agent().clone(),
            model_plan(provider(0x93)),
        )
        .expect("shape-only activation");
        let mut other = ManagedModelAgentStackApplyJournalV1::new(active_fabric_state());
        assert!(
            other
                .prepare_activate_with(&controller, &provisioning, &mismatch, fresh(0x96), |_| Ok(
                    ()
                ),)
                .is_err()
        );
    }

    #[test]
    fn send_token_exists_only_after_uncertain_is_durable() {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let (mut journal, prepared) = prepared_journal();
        let failed = journal.claim_send_with(prepared, &controller, &provisioning, |_| {
            Err(ManagedModelAgentStackApplyControllerError::DurabilityRejected)
        });
        assert!(failed.is_err());
        assert_eq!(
            journal
                .state()
                .model_agent_stack_state()
                .expect("A2 state")
                .phase(),
            ManagedModelAgentStackApplyPhaseV1::RequestDurableNotSent
        );

        let crossed = Cell::new(false);
        let action = journal
            .claim_send_with(prepared, &controller, &provisioning, |next| {
                assert_eq!(
                    next.model_agent_stack_state().expect("A2 state").phase(),
                    ManagedModelAgentStackApplyPhaseV1::Uncertain
                );
                crossed.set(true);
                Ok(())
            })
            .expect("send action");
        assert!(crossed.get());
        assert_eq!(
            action.request().canonical_wire(),
            action.canonical_request_bytes()
        );
    }

    #[test]
    fn pxmj_roundtrip_checksum_lengths_and_predecessor_magic_are_strict() {
        let (journal, _) = uncertain_journal();
        let stack = journal.state().model_agent_stack_state().expect("A2 state");
        let wire = stack.encode().expect("PXMJ");
        assert_eq!(&wire[..4], b"PXMJ");
        assert_eq!(reopen_stack(journal.state()), *stack);

        for corrupted in [
            {
                let mut value = wire.to_vec();
                value[4] = 0;
                value[5] = 2;
                value
            },
            {
                let mut value = wire.to_vec();
                value[..4].copy_from_slice(b"PXAJ");
                value
            },
            {
                let mut value = wire.to_vec();
                let last = value.len() - 1;
                value[last] ^= 1;
                value
            },
            wire[..wire.len() - 1].to_vec(),
            {
                let mut value = wire.to_vec();
                value.push(0);
                value
            },
        ] {
            let controller = fabric_tests::controller_signer();
            let provisioning = fabric_tests::provisioning();
            let context = journal
                .state()
                .verified_current_context(&controller, &provisioning)
                .expect("context");
            let fabric = journal.state().desired().expect("Fabric desired");
            let request = journal.state().request().expect("Fabric request");
            let generation = journal
                .state()
                .receipt()
                .and_then(|receipt| receipt.facts().generation())
                .expect("Fabric generation");
            assert!(
                ManagedModelAgentStackControllerStateV1::decode(
                    &corrupted,
                    ManagedModelAgentStackDecodeContextV1 {
                        fabric: &context,
                        cutover_marker_digest: journal.state().cutover_marker_digest(),
                        predecessor_revision: fabric.revision(),
                        predecessor_execution: fabric.execution(),
                        predecessor_slice_digest: request.target_slice_digest(),
                        predecessor_generation: generation,
                    },
                )
                .is_err()
            );
        }
    }

    #[test]
    fn predecessor_pxmj_v1_shared_golden_is_exact_and_cross_version_strict() {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let (mut journal, action) = uncertain_journal();
        let receipt = signed_receipt(
            action.request(),
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
            1,
        );
        journal
            .consume_pxmt_with(
                action,
                receipt.canonical_wire(),
                &controller,
                &provisioning,
                |_| Ok(()),
            )
            .expect("durable predecessor ActiveReady PXMT");
        let state = journal.state();
        let stack = state.model_agent_stack_state().expect("predecessor PXMJ1");
        let fixture = decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxmj_v1.hex"
        ));
        assert_eq!(
            stack
                .encode()
                .expect("canonical predecessor PXMJ1")
                .as_ref(),
            fixture.as_slice()
        );
        assert!(ArtifactExternalControllerStateV2::decode(&fixture).is_err());

        let fabric_context = state
            .verified_current_context(&controller, &provisioning)
            .expect("verified predecessor Fabric context");
        let desired = state.desired().expect("predecessor Fabric desired");
        let fabric_request = state.request().expect("predecessor Fabric request");
        let generation = state
            .receipt()
            .and_then(|value| value.facts().generation())
            .expect("predecessor Fabric generation");
        let decode = || ManagedModelAgentStackDecodeContextV1 {
            fabric: &fabric_context,
            cutover_marker_digest: state.cutover_marker_digest(),
            predecessor_revision: desired.revision(),
            predecessor_execution: desired.execution(),
            predecessor_slice_digest: fabric_request.target_slice_digest(),
            predecessor_generation: generation,
        };
        assert_eq!(
            ManagedModelAgentStackControllerStateV1::decode(&fixture, decode())
                .expect("shared predecessor PXMJ1 must reopen"),
            *stack
        );
        let successor = decode_fixture_hex(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_active_ready.hex"
        ));
        assert!(ManagedModelAgentStackControllerStateV1::decode(&successor, decode()).is_err());
    }

    #[test]
    fn every_legal_active_pxmt_is_durable_but_only_active_ready_opens_empty() {
        for (index, outcome) in [
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
            ManagedModelAgentStackTerminalOutcomeV1::Quarantined,
            ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected,
            ManagedModelAgentStackTerminalOutcomeV1::Uncertain,
        ]
        .into_iter()
        .enumerate()
        {
            let controller = fabric_tests::controller_signer();
            let provisioning = fabric_tests::provisioning();
            let (mut journal, action) = uncertain_journal();
            let receipt = signed_receipt(action.request(), outcome, 1);
            journal
                .consume_pxmt_with(
                    action,
                    receipt.canonical_wire(),
                    &controller,
                    &provisioning,
                    |_| Ok(()),
                )
                .expect("durable legal PXMT");
            let reopened = reopen_stack(journal.state());
            assert_eq!(
                reopened
                    .receipt()
                    .expect("receipt")
                    .facts()
                    .state()
                    .outcome(),
                outcome
            );
            assert!(!reopened.deactivation_succeeded());
            let empty = journal.prepare_empty_deactivate_with(
                &controller,
                &provisioning,
                fresh(0xb0 + index as u8 * 3),
                |_| Ok(()),
            );
            assert_eq!(
                empty.is_ok(),
                outcome == ManagedModelAgentStackTerminalOutcomeV1::ActiveReady
            );
        }
    }

    #[test]
    fn empty_exact_zero_reopens_as_only_deactivation_success_and_uses_next_cas() {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let (mut journal, action) = uncertain_journal();
        let active = signed_receipt(
            action.request(),
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
            1,
        );
        journal
            .consume_pxmt_with(
                action,
                active.canonical_wire(),
                &controller,
                &provisioning,
                |_| Ok(()),
            )
            .expect("active PXMT");
        let active_stack = journal
            .state()
            .model_agent_stack_state()
            .expect("active A2")
            .clone();
        let prepared = journal
            .prepare_empty_deactivate_with(&controller, &provisioning, fresh(0xc0), |_| Ok(()))
            .expect("prepare empty");
        let empty_stack = journal.state().model_agent_stack_state().expect("empty A2");
        assert_eq!(
            empty_stack.desired().revision().value(),
            active_stack.desired().revision().value() + 1
        );
        assert_eq!(
            empty_stack
                .request()
                .control_commitment()
                .control()
                .expected_active(),
            ExpectedActive::Exact(active_stack.request().target_slice_digest())
        );
        assert_eq!(
            empty_stack.request().temporal().original_budget().value(),
            9_000_000_114
        );
        let action = journal
            .claim_send_with(prepared, &controller, &provisioning, |_| Ok(()))
            .expect("claim empty");
        let empty = signed_receipt(
            action.request(),
            ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero,
            1,
        );
        let terminal = journal
            .consume_pxmt_with(
                action,
                empty.canonical_wire(),
                &controller,
                &provisioning,
                |_| Ok(()),
            )
            .expect("empty PXMT");
        assert!(terminal.is_deactivation_success());
        assert!(
            journal
                .state()
                .model_agent_stack_state()
                .expect("empty A2")
                .deactivation_succeeded()
        );
        assert!(reopen_stack(journal.state()).deactivation_succeeded());
    }

    #[test]
    fn terminal_fabric_generation_must_match_pxar6_predecessor() {
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let (mut journal, action) = uncertain_journal();
        let mismatch = signed_receipt(
            action.request(),
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
            99,
        );
        assert!(matches!(
            journal.consume_pxmt_with(
                action,
                mismatch.canonical_wire(),
                &controller,
                &provisioning,
                |_| Ok(()),
            ),
            Err(ManagedModelAgentStackApplyControllerError::ReceiptMismatch)
        ));
        assert_eq!(
            journal
                .state()
                .model_agent_stack_state()
                .expect("A2 state")
                .phase(),
            ManagedModelAgentStackApplyPhaseV1::Uncertain
        );
    }

    fn artifact_runtime_prefix() -> (
        ArtifactExternalDeploymentRequestV1,
        ArtifactExternalDeploymentAdmissionV1,
        ArtifactBoundManagedModelAgentStackPlanContentV2,
        ArtifactBoundManagedModelAgentStackTargetExecutionV1,
        ArtifactBoundManagedModelAgentStackApplyRequestV1,
    ) {
        let deployment_request = artifact_external_request();
        let admission = ArtifactExternalDeploymentAdmissionV1::try_new(
            [0x46; 32],
            NonZeroU64::new(1).expect("admission sequence"),
            &deployment_request,
        )
        .expect("PXDK");
        let (plan_content, execution, runtime_request) =
            artifact_runtime_suffix(&deployment_request, &admission);
        (
            deployment_request,
            admission,
            plan_content,
            execution,
            runtime_request,
        )
    }

    fn artifact_runtime_suffix(
        deployment_request: &ArtifactExternalDeploymentRequestV1,
        admission: &ArtifactExternalDeploymentAdmissionV1,
    ) -> (
        ArtifactBoundManagedModelAgentStackPlanContentV2,
        ArtifactBoundManagedModelAgentStackTargetExecutionV1,
        ArtifactBoundManagedModelAgentStackApplyRequestV1,
    ) {
        let state = active_fabric_state();
        let controller = fabric_tests::controller_signer();
        let provisioning = fabric_tests::provisioning();
        let context = state
            .verified_current_context(&controller, &provisioning)
            .expect("verified Fabric context");
        let predecessor = state.desired().expect("Fabric desired");
        let predecessor_request = state.request().expect("Fabric request");
        let selection = provider(0x83);
        let requested = ManagedModelAgentStackActivationV1::try_new(
            state
                .desired()
                .expect("active Fabric desired")
                .execution()
                .clone(),
            agent_plan(selection),
            artifact_model_plan(selection),
        )
        .expect("Artifact activation");
        let desired = ArtifactBoundManagedModelAgentStackDesiredPlanV1::try_activate(
            ArtifactBoundManagedModelAgentStackDesiredInputV1 {
                context: &context,
                cutover_marker_digest: artifact_external_cutover_marker_digest(
                    deployment_request,
                    admission,
                )
                .expect("cutover marker"),
                predecessor_revision: predecessor.revision(),
                predecessor_execution: predecessor.execution(),
                predecessor_slice_digest: predecessor_request.target_slice_digest(),
                deployment_request_digest: deployment_request.request_digest(),
                deployment_admission_digest: admission.admission_digest(),
                binding: deployment_request.binding(),
                activation: &requested,
            },
        )
        .expect("Artifact-bound desired");
        let runtime_request = produce_artifact_bound_managed_model_agent_stack_request_v1(
            &context,
            &desired,
            fresh(0xa0),
            &controller,
        )
        .expect("PXAR12");
        (
            desired.plan_content().clone(),
            desired.execution().clone(),
            runtime_request,
        )
    }

    fn signed_artifact_receipt(
        request: &ArtifactBoundManagedModelAgentStackApplyRequestV1,
        outcome: ManagedModelAgentStackTerminalOutcomeV1,
    ) -> Result<ManagedModelAgentStackTerminalReceiptV1, ManagedModelAgentStackPlanError> {
        let generation =
            |value| Some(ManagedServiceGeneration::try_new(value).expect("service generation"));
        let (
            lifecycle_effect,
            head,
            fabric_generation,
            model_generation,
            agent_generation,
            physical_binding_census,
            census_complete,
            fabric_ready,
            model_ready,
            agent_ready,
            fabric_dependency,
            model_dependency,
            exact_zero,
            quarantined,
            completion_snapshot_sequence,
            selection_observed_at_nanos,
            digest_marker,
        ) = match outcome {
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                generation(7),
                generation(8),
                generation(9),
                2,
                true,
                true,
                true,
                true,
                true,
                true,
                false,
                false,
                12,
                13,
                0xa1,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                None,
                None,
                None,
                0,
                true,
                false,
                false,
                false,
                false,
                false,
                true,
                false,
                16,
                25,
                0xa3,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::ProvenNotStarted,
                ManagedModelAgentStackTerminalHeadV1::PreservedNone,
                generation(7),
                None,
                None,
                0,
                true,
                true,
                false,
                false,
                false,
                false,
                false,
                false,
                1,
                21,
                0xa5,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::Uncertain => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                generation(7),
                generation(8),
                None,
                0,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                11,
                22,
                0xa7,
            ),
            ManagedModelAgentStackTerminalOutcomeV1::Quarantined => (
                ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
                ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
                generation(7),
                generation(8),
                None,
                0,
                true,
                true,
                false,
                false,
                false,
                false,
                false,
                true,
                11,
                23,
                0xa9,
            ),
        };
        let state = ManagedModelAgentStackTerminalStateV1::try_new(
            outcome,
            lifecycle_effect,
            head,
            fabric_generation,
            model_generation,
            agent_generation,
        )?;
        let evidence = ManagedModelAgentStackTerminalEvidenceV1::try_new(
            ManagedModelAgentStackTerminalEvidenceFieldsV1 {
                physical_binding_census,
                census_complete,
                fabric_ready,
                model_ready,
                agent_ready,
                fabric_to_agent_dependency_ready: fabric_dependency,
                model_to_agent_dependency_ready: model_dependency,
                exact_zero,
                quarantined,
                resource_census_digest: Digest32::from_bytes([digest_marker; 32]),
                raw_outcome_digest: Digest32::from_bytes([digest_marker.wrapping_add(1); 32]),
                completion_runtime_host_epoch: 9,
                completion_snapshot_sequence,
                selection_clock_generation: request.temporal().target_clock_generation(),
                selection_observed_at_nanos,
            },
        )?;
        sign_artifact_terminal(request, state, evidence)
    }

    fn signed_artifact_agent_quarantined_receipt(
        request: &ArtifactBoundManagedModelAgentStackApplyRequestV1,
    ) -> Result<ManagedModelAgentStackTerminalReceiptV1, ManagedModelAgentStackPlanError> {
        let generation =
            |value| Some(ManagedServiceGeneration::try_new(value).expect("service generation"));
        let state = ManagedModelAgentStackTerminalStateV1::try_new(
            ManagedModelAgentStackTerminalOutcomeV1::Quarantined,
            ManagedModelAgentStackTerminalLifecycleEffectV1::MayHaveStarted,
            ManagedModelAgentStackTerminalHeadV1::CommittedIncoming,
            generation(7),
            generation(8),
            generation(9),
        )?;
        let evidence = ManagedModelAgentStackTerminalEvidenceV1::try_new(
            ManagedModelAgentStackTerminalEvidenceFieldsV1 {
                physical_binding_census: 0,
                census_complete: false,
                fabric_ready: true,
                model_ready: false,
                agent_ready: false,
                fabric_to_agent_dependency_ready: false,
                model_to_agent_dependency_ready: false,
                exact_zero: false,
                quarantined: true,
                resource_census_digest: Digest32::from_bytes([0xab; 32]),
                raw_outcome_digest: Digest32::from_bytes([0xac; 32]),
                completion_runtime_host_epoch: 9,
                completion_snapshot_sequence: 12,
                selection_clock_generation: request.temporal().target_clock_generation(),
                selection_observed_at_nanos: 24,
            },
        )?;
        sign_artifact_terminal(request, state, evidence)
    }

    fn sign_artifact_terminal(
        request: &ArtifactBoundManagedModelAgentStackApplyRequestV1,
        state: ManagedModelAgentStackTerminalStateV1,
        evidence: ManagedModelAgentStackTerminalEvidenceV1,
    ) -> Result<ManagedModelAgentStackTerminalReceiptV1, ManagedModelAgentStackPlanError> {
        let facts = ManagedModelAgentStackTerminalFactsV1::try_new_artifact_bound(
            request, state, evidence,
        )?;
        let channel = fabric_tests::channel();
        let auth = ManagedModelAgentStackTerminalAuthClaimV1::try_new(
            channel,
            RUNTIME_KEY,
            ApplyAuthAlgorithm::try_new(1).expect("algorithm"),
            1,
        )?;
        let draft = ManagedModelAgentStackTerminalReceiptDraftV1::try_new_artifact_bound(
            request, facts, channel, auth,
        )?;
        let runtime: SigningKey = fabric_tests::runtime_signer();
        let signature = runtime.sign(draft.signing_transcript()?.as_bytes());
        draft.finalize(&signature.to_bytes())
    }

    fn rewrite_artifact_state_checksum(frame: &mut [u8]) {
        let checksum_offset = frame.len() - ARTIFACT_STATE_V2_CHECKSUM_BYTES;
        let checksum =
            artifact_state_v2_checksum(&frame[..checksum_offset]).expect("PXMJ2 checksum");
        frame[checksum_offset..].copy_from_slice(checksum.as_bytes());
    }

    fn rewrite_external_record_digest(frame: &mut [u8], record_offset: usize) {
        let digest_offset = record_offset + 464;
        let digest = raw_sha256(
            EXTERNAL_RECORD_DIGEST_DOMAIN,
            &frame[record_offset..digest_offset],
        );
        frame[digest_offset..digest_offset + 32].copy_from_slice(digest.as_bytes());
    }

    fn rewrite_external_receipt_digest(frame: &mut [u8], receipt_offset: usize) {
        let digest_offset = receipt_offset + 400;
        let digest = raw_sha256(
            EXTERNAL_RECEIPT_DIGEST_DOMAIN,
            &frame[receipt_offset..digest_offset],
        );
        frame[digest_offset..digest_offset + 32].copy_from_slice(digest.as_bytes());
    }

    #[test]
    fn artifact_external_controller_state_v2_reopens_every_durable_prefix() {
        let (request, admission, plan_content, execution, runtime_request) =
            artifact_runtime_prefix();
        let admitted =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Admitted,
                controller_snapshot_sequence: NonZeroU64::new(1).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: None,
                execution: None,
                runtime_request: None,
                runtime_terminal: None,
                records: Vec::new(),
                receipt: None,
            })
            .expect("PXMJ2-A");
        let admitted_wire = admitted.encode().expect("PXMJ2-A wire");
        assert_eq!(admitted_wire.len(), 752);
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&admitted_wire).expect("reopen A"),
            admitted,
        );

        let desired_head = *runtime_request.target_slice_digest().value();
        let committed_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(desired_head),
            None,
            None,
            None,
        )
        .expect("committed progress");
        let committed_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Committed,
            NonZeroU64::new(1).expect("record sequence"),
            &request,
            &admission,
            committed_progress,
            None,
        )
        .expect("PXDM-C");
        let committed =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Committed,
                controller_snapshot_sequence: NonZeroU64::new(2).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: Some(plan_content.clone()),
                execution: Some(execution.clone()),
                runtime_request: Some(runtime_request.clone()),
                runtime_terminal: None,
                records: vec![committed_record.clone()],
                receipt: None,
            })
            .expect("PXMJ2-C");
        let committed_wire = committed.encode().expect("PXMJ2-C wire");
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&committed_wire).expect("reopen C"),
            committed,
        );
        let mut committed_pin_drift = committed_wire.to_vec();
        let committed_record_offset =
            committed_pin_drift.len() - ARTIFACT_STATE_V2_CHECKSUM_BYTES - EXTERNAL_RECORD_BYTES;
        committed_pin_drift[committed_record_offset + 248..committed_record_offset + 256]
            .copy_from_slice(&3_u64.to_be_bytes());
        rewrite_external_record_digest(&mut committed_pin_drift, committed_record_offset);
        rewrite_artifact_state_checksum(&mut committed_pin_drift);
        assert!(ArtifactExternalControllerStateV2::decode(&committed_pin_drift).is_err());

        let applying_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(desired_head),
            Some(runtime_request.envelope_request_digest()),
            None,
            Some([0x54; 16]),
        )
        .expect("applying progress");
        let applying_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Applying,
            NonZeroU64::new(2).expect("record sequence"),
            &request,
            &admission,
            applying_progress,
            Some(&committed_record),
        )
        .expect("PXDM-P");
        let applying =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Applying,
                controller_snapshot_sequence: NonZeroU64::new(3).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: Some(plan_content.clone()),
                execution: Some(execution.clone()),
                runtime_request: Some(runtime_request.clone()),
                runtime_terminal: None,
                records: vec![committed_record.clone(), applying_record.clone()],
                receipt: None,
            })
            .expect("PXMJ2-P");
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&applying.encode().expect("PXMJ2-P wire"))
                .expect("reopen P"),
            applying,
        );

        let runtime_terminal = signed_artifact_receipt(
            &runtime_request,
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
        )
        .expect("ActiveReady Artifact PXMT");
        let active_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(desired_head),
            Some(runtime_request.envelope_request_digest()),
            Some(runtime_terminal.receipt_digest()),
            Some([0x54; 16]),
        )
        .expect("active progress");
        let active_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::ActiveReady,
            NonZeroU64::new(3).expect("record sequence"),
            &request,
            &admission,
            active_progress,
            Some(&applying_record),
        )
        .expect("PXDM-R");
        let receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &active_record,
        )
        .expect("PXDO-R");
        let active =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::ActiveReady,
                controller_snapshot_sequence: NonZeroU64::new(4).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: Some(plan_content.clone()),
                execution: Some(execution.clone()),
                runtime_request: Some(runtime_request.clone()),
                runtime_terminal: Some(runtime_terminal.clone()),
                records: vec![
                    committed_record.clone(),
                    applying_record.clone(),
                    active_record,
                ],
                receipt: Some(receipt),
            })
            .expect("PXMJ2-R");
        let active_wire = active.encode().expect("PXMJ2-R wire");
        assert_eq!(&active_wire[..4], b"PXMJ");
        assert_eq!(u16::from_be_bytes([active_wire[4], active_wire[5]]), 2);
        assert_eq!(active_wire[12], b'R');
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&active_wire).expect("reopen R"),
            active,
        );
        let mut active_header_drift = active_wire.to_vec();
        active_header_drift[16..24].copy_from_slice(&3_u64.to_be_bytes());
        rewrite_artifact_state_checksum(&mut active_header_drift);
        assert!(ArtifactExternalControllerStateV2::decode(&active_header_drift).is_err());

        for (outcome, controller_phase, record_state) in [
            (
                ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected,
                ArtifactExternalControllerPhaseV2::Failed,
                ArtifactExternalDeploymentRecordStateV1::Failed,
            ),
            (
                ManagedModelAgentStackTerminalOutcomeV1::Quarantined,
                ArtifactExternalControllerPhaseV2::Failed,
                ArtifactExternalDeploymentRecordStateV1::Failed,
            ),
            (
                ManagedModelAgentStackTerminalOutcomeV1::Uncertain,
                ArtifactExternalControllerPhaseV2::Uncertain,
                ArtifactExternalDeploymentRecordStateV1::Uncertain,
            ),
        ] {
            let terminal = signed_artifact_receipt(&runtime_request, outcome)
                .expect("legal Artifact activation PXMT");
            let terminal_progress = ArtifactExternalDeploymentProgressV1::try_new(
                NonZeroU64::new(1),
                NonZeroU64::new(2),
                Some(desired_head),
                Some(runtime_request.envelope_request_digest()),
                Some(terminal.receipt_digest()),
                Some([0x54; 16]),
            )
            .expect("post-P terminal progress");
            let terminal_record = ArtifactExternalDeploymentRecordV1::try_new(
                record_state,
                NonZeroU64::new(3).expect("record sequence"),
                &request,
                &admission,
                terminal_progress,
                Some(&applying_record),
            )
            .expect("terminal PXDM");
            let terminal_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
                NonZeroU64::new(1).expect("receipt sequence"),
                &request,
                &admission,
                &terminal_record,
            )
            .expect("terminal PXDO");
            let controller = ArtifactExternalControllerStateV2::try_new(
                ArtifactExternalControllerStateInputV2 {
                    phase: controller_phase,
                    controller_snapshot_sequence: NonZeroU64::new(4).expect("sequence"),
                    request: request.clone(),
                    admission: admission.clone(),
                    plan_content: Some(plan_content.clone()),
                    execution: Some(execution.clone()),
                    runtime_request: Some(runtime_request.clone()),
                    runtime_terminal: Some(terminal.clone()),
                    records: vec![
                        committed_record.clone(),
                        applying_record.clone(),
                        terminal_record,
                    ],
                    receipt: Some(terminal_receipt),
                },
            )
            .expect("PXMJ2 post-P terminal");
            let wire = controller.encode().expect("PXMJ2 terminal wire");
            let reopened =
                ArtifactExternalControllerStateV2::decode(&wire).expect("reopen post-P terminal");
            assert_eq!(reopened, controller);
            assert_eq!(reopened.phase(), controller_phase);
            assert_eq!(
                reopened
                    .runtime_terminal()
                    .expect("archived exact PXMT")
                    .canonical_wire(),
                terminal.canonical_wire(),
            );
            assert_eq!(
                reopened
                    .records()
                    .last()
                    .expect("terminal record")
                    .progress()
                    .runtime_terminal_receipt_digest,
                *terminal.receipt_digest().as_bytes(),
            );

            let (wrong_phase, wrong_record_state) =
                if controller_phase == ArtifactExternalControllerPhaseV2::Failed {
                    (
                        ArtifactExternalControllerPhaseV2::Uncertain,
                        ArtifactExternalDeploymentRecordStateV1::Uncertain,
                    )
                } else {
                    (
                        ArtifactExternalControllerPhaseV2::Failed,
                        ArtifactExternalDeploymentRecordStateV1::Failed,
                    )
                };
            let wrong_record = ArtifactExternalDeploymentRecordV1::try_new(
                wrong_record_state,
                NonZeroU64::new(3).expect("record sequence"),
                &request,
                &admission,
                terminal_progress,
                Some(&applying_record),
            )
            .expect("shape-valid wrong-phase PXDM");
            let wrong_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
                NonZeroU64::new(1).expect("receipt sequence"),
                &request,
                &admission,
                &wrong_record,
            )
            .expect("shape-valid wrong-phase PXDO");
            assert!(
                ArtifactExternalControllerStateV2::try_new(
                    ArtifactExternalControllerStateInputV2 {
                        phase: wrong_phase,
                        controller_snapshot_sequence: NonZeroU64::new(4).expect("sequence"),
                        request: request.clone(),
                        admission: admission.clone(),
                        plan_content: Some(plan_content.clone()),
                        execution: Some(execution.clone()),
                        runtime_request: Some(runtime_request.clone()),
                        runtime_terminal: Some(terminal),
                        records: vec![
                            committed_record.clone(),
                            applying_record.clone(),
                            wrong_record,
                        ],
                        receipt: Some(wrong_receipt),
                    },
                )
                .is_err()
            );
        }

        let agent_quarantined = signed_artifact_agent_quarantined_receipt(&runtime_request)
            .expect("agent-intent Quarantined PXMT");
        let agent_quarantined_progress = ArtifactExternalDeploymentProgressV1::try_new(
            NonZeroU64::new(1),
            NonZeroU64::new(2),
            Some(desired_head),
            Some(runtime_request.envelope_request_digest()),
            Some(agent_quarantined.receipt_digest()),
            Some([0x54; 16]),
        )
        .expect("agent-intent Quarantined progress");
        let agent_quarantined_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Failed,
            NonZeroU64::new(3).expect("record sequence"),
            &request,
            &admission,
            agent_quarantined_progress,
            Some(&applying_record),
        )
        .expect("agent-intent Quarantined PXDM");
        let agent_quarantined_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &agent_quarantined_record,
        )
        .expect("agent-intent Quarantined PXDO");
        let expected_pxmt = agent_quarantined.canonical_wire().to_vec();
        let expected_pxdo = agent_quarantined_receipt.canonical_wire().to_vec();
        let agent_quarantined_controller =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Failed,
                controller_snapshot_sequence: NonZeroU64::new(4).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: Some(plan_content.clone()),
                execution: Some(execution.clone()),
                runtime_request: Some(runtime_request.clone()),
                runtime_terminal: Some(agent_quarantined.clone()),
                records: vec![
                    committed_record.clone(),
                    applying_record.clone(),
                    agent_quarantined_record,
                ],
                receipt: Some(agent_quarantined_receipt),
            })
            .expect("agent-intent Quarantined PXMJ2-F");
        let agent_quarantined_wire = agent_quarantined_controller
            .encode()
            .expect("agent-intent Quarantined PXMJ2 wire");
        let reopened_agent_quarantined =
            ArtifactExternalControllerStateV2::decode(&agent_quarantined_wire)
                .expect("reopen agent-intent Quarantined PXMJ2");
        assert_eq!(reopened_agent_quarantined, agent_quarantined_controller);
        assert_eq!(
            reopened_agent_quarantined
                .runtime_terminal()
                .expect("archived agent-intent PXMT")
                .canonical_wire(),
            expected_pxmt.as_slice(),
        );
        assert_eq!(
            reopened_agent_quarantined
                .runtime_terminal()
                .expect("archived agent-intent PXMT")
                .facts()
                .state()
                .agent_generation()
                .expect("Agent generation")
                .value(),
            9,
        );
        assert_eq!(
            reopened_agent_quarantined
                .records()
                .last()
                .expect("agent-intent terminal record")
                .progress()
                .runtime_terminal_receipt_digest,
            *agent_quarantined.receipt_digest().as_bytes(),
        );
        assert_eq!(
            reopened_agent_quarantined
                .receipt()
                .expect("agent-intent PXDO")
                .canonical_wire()
                .as_slice(),
            expected_pxdo.as_slice(),
        );
        assert_eq!(
            reopened_agent_quarantined
                .encode()
                .expect("canonical re-encode"),
            agent_quarantined_wire,
        );

        assert!(
            signed_artifact_receipt(
                &runtime_request,
                ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero,
            )
            .is_err()
        );

        let failed_progress = ArtifactExternalDeploymentProgressV1::try_new(
            None,
            None,
            None,
            None,
            None,
            Some([0x55; 16]),
        )
        .expect("pre-C failed progress");
        let failed_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Failed,
            NonZeroU64::new(1).expect("record sequence"),
            &request,
            &admission,
            failed_progress,
            None,
        )
        .expect("PXDM-F");
        let failed_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &failed_record,
        )
        .expect("PXDO-F");
        let failed =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Failed,
                controller_snapshot_sequence: NonZeroU64::new(2).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: None,
                execution: None,
                runtime_request: None,
                runtime_terminal: None,
                records: vec![failed_record],
                receipt: Some(failed_receipt),
            })
            .expect("PXMJ2-F pre-C");
        let failed_wire = failed.encode().expect("PXMJ2-F wire");
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&failed_wire).expect("reopen F"),
            failed,
        );
        let mut failed_pin_drift = failed_wire.to_vec();
        let failed_receipt_offset =
            failed_pin_drift.len() - ARTIFACT_STATE_V2_CHECKSUM_BYTES - EXTERNAL_RECEIPT_BYTES;
        let failed_record_offset = failed_receipt_offset - EXTERNAL_RECORD_BYTES;
        failed_pin_drift[failed_record_offset + 248..failed_record_offset + 256]
            .copy_from_slice(&2_u64.to_be_bytes());
        rewrite_external_record_digest(&mut failed_pin_drift, failed_record_offset);
        let failed_record_digest: [u8; 32] = failed_pin_drift
            [failed_record_offset + 464..failed_record_offset + 496]
            .try_into()
            .expect("PXDM digest");
        failed_pin_drift[failed_receipt_offset + 104..failed_receipt_offset + 136]
            .copy_from_slice(&failed_record_digest);
        failed_pin_drift[failed_receipt_offset + 248..failed_receipt_offset + 256]
            .copy_from_slice(&2_u64.to_be_bytes());
        rewrite_external_receipt_digest(&mut failed_pin_drift, failed_receipt_offset);
        rewrite_artifact_state_checksum(&mut failed_pin_drift);
        assert!(ArtifactExternalControllerStateV2::decode(&failed_pin_drift).is_err());

        let pre_commit_uncertain_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Uncertain,
            NonZeroU64::new(1).expect("record sequence"),
            &request,
            &admission,
            failed_progress,
            None,
        )
        .expect("PXDM-U pre-C");
        let pre_commit_uncertain_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &pre_commit_uncertain_record,
        )
        .expect("PXDO-U pre-C");
        let pre_commit_uncertain =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Uncertain,
                controller_snapshot_sequence: NonZeroU64::new(2).expect("sequence"),
                request: request.clone(),
                admission: admission.clone(),
                plan_content: None,
                execution: None,
                runtime_request: None,
                runtime_terminal: None,
                records: vec![pre_commit_uncertain_record],
                receipt: Some(pre_commit_uncertain_receipt),
            })
            .expect("PXMJ2-U pre-C");
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(
                &pre_commit_uncertain.encode().expect("PXMJ2-U pre-C wire"),
            )
            .expect("reopen U pre-C"),
            pre_commit_uncertain,
        );

        for (controller_phase, record_state) in [
            (
                ArtifactExternalControllerPhaseV2::Failed,
                ArtifactExternalDeploymentRecordStateV1::Failed,
            ),
            (
                ArtifactExternalControllerPhaseV2::Uncertain,
                ArtifactExternalDeploymentRecordStateV1::Uncertain,
            ),
        ] {
            let post_commit_progress = ArtifactExternalDeploymentProgressV1::try_new(
                NonZeroU64::new(1),
                NonZeroU64::new(2),
                Some(desired_head),
                None,
                None,
                Some([0x56; 16]),
            )
            .expect("post-C terminal progress");
            let post_commit_record = ArtifactExternalDeploymentRecordV1::try_new(
                record_state,
                NonZeroU64::new(2).expect("record sequence"),
                &request,
                &admission,
                post_commit_progress,
                Some(&committed_record),
            )
            .expect("PXDM post-C terminal");
            let post_commit_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
                NonZeroU64::new(1).expect("receipt sequence"),
                &request,
                &admission,
                &post_commit_record,
            )
            .expect("PXDO post-C terminal");
            let post_commit = ArtifactExternalControllerStateV2::try_new(
                ArtifactExternalControllerStateInputV2 {
                    phase: controller_phase,
                    controller_snapshot_sequence: NonZeroU64::new(3).expect("sequence"),
                    request: request.clone(),
                    admission: admission.clone(),
                    plan_content: Some(plan_content.clone()),
                    execution: Some(execution.clone()),
                    runtime_request: Some(runtime_request.clone()),
                    runtime_terminal: None,
                    records: vec![committed_record.clone(), post_commit_record],
                    receipt: Some(post_commit_receipt),
                },
            )
            .expect("PXMJ2 post-C terminal");
            let post_commit_wire = post_commit.encode().expect("PXMJ2 post-C wire");
            assert_eq!(
                ArtifactExternalControllerStateV2::decode(&post_commit_wire)
                    .expect("reopen post-C terminal"),
                post_commit,
            );
            let mut receipt_pin_drift = post_commit_wire.to_vec();
            let receipt_offset =
                receipt_pin_drift.len() - ARTIFACT_STATE_V2_CHECKSUM_BYTES - EXTERNAL_RECEIPT_BYTES;
            receipt_pin_drift[receipt_offset + 248..receipt_offset + 256]
                .copy_from_slice(&3_u64.to_be_bytes());
            rewrite_external_receipt_digest(&mut receipt_pin_drift, receipt_offset);
            rewrite_artifact_state_checksum(&mut receipt_pin_drift);
            assert!(ArtifactExternalControllerStateV2::decode(&receipt_pin_drift).is_err());
        }

        let uncertain_record = ArtifactExternalDeploymentRecordV1::try_new(
            ArtifactExternalDeploymentRecordStateV1::Uncertain,
            NonZeroU64::new(3).expect("record sequence"),
            &request,
            &admission,
            applying_progress,
            Some(&applying_record),
        )
        .expect("PXDM-U");
        let uncertain_receipt = ArtifactExternalDeploymentReceiptV1::try_new(
            NonZeroU64::new(1).expect("receipt sequence"),
            &request,
            &admission,
            &uncertain_record,
        )
        .expect("PXDO-U");
        let uncertain =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Uncertain,
                controller_snapshot_sequence: NonZeroU64::new(4).expect("sequence"),
                request,
                admission,
                plan_content: Some(plan_content),
                execution: Some(execution),
                runtime_request: Some(runtime_request),
                runtime_terminal: None,
                records: vec![committed_record, applying_record, uncertain_record],
                receipt: Some(uncertain_receipt),
            })
            .expect("PXMJ2-U post-P without PXMT");
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&uncertain.encode().expect("PXMJ2-U wire"))
                .expect("reopen U"),
            uncertain,
        );
    }

    #[test]
    fn artifact_external_controller_owner_reducer_matches_primary_goldens() {
        let admitted_wire = decode_fixture_hex(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/wire/artifact_f0_pxmj_v2_admitted.hex"
        )));
        let committed_wire = decode_fixture_hex(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/wire/artifact_f0_pxmj_v2_committed.hex"
        )));
        let applying_wire = decode_fixture_hex(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/wire/artifact_f0_pxmj_v2_applying.hex"
        )));
        let active_wire = decode_fixture_hex(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/wire/artifact_f0_pxmj_v2_active_ready.hex"
        )));
        let admitted_fixture =
            ArtifactExternalControllerStateV2::decode(&admitted_wire).expect("A fixture");
        let committed_fixture =
            ArtifactExternalControllerStateV2::decode(&committed_wire).expect("C fixture");
        let applying_fixture =
            ArtifactExternalControllerStateV2::decode(&applying_wire).expect("P fixture");
        let active_fixture =
            ArtifactExternalControllerStateV2::decode(&active_wire).expect("R fixture");

        let admitted = ArtifactExternalControllerStateV2::admit(
            admitted_fixture.request().clone(),
            *admitted_fixture.admission().controller_store_instance(),
        )
        .expect("owner admits PXMJ2-A");
        assert_eq!(admitted.encode().expect("A wire").as_ref(), admitted_wire);

        let committed = admitted
            .commit(
                committed_fixture
                    .plan_content()
                    .expect("C plan content")
                    .clone(),
                committed_fixture.execution().expect("C execution").clone(),
                committed_fixture
                    .runtime_request()
                    .expect("C PXAR12")
                    .clone(),
            )
            .expect("owner commits PXMJ2-C");
        assert_eq!(committed.encode().expect("C wire").as_ref(), committed_wire);

        let applying = committed
            .begin_apply(applying_fixture.records()[1].progress.lifecycle_generation)
            .expect("owner commits PXMJ2-P");
        assert_eq!(applying.encode().expect("P wire").as_ref(), applying_wire);

        let active = applying
            .finish_apply(
                Some(active_fixture.runtime_terminal().expect("R PXMT").clone()),
                None,
            )
            .expect("owner commits PXMJ2-R");
        assert_eq!(active.encode().expect("R wire").as_ref(), active_wire);
        assert!(matches!(
            active.finish_apply(None, Some(ArtifactExternalControllerPhaseV2::Failed)),
            Err(ManagedModelAgentStackApplyControllerError::InvalidPhase)
        ));
    }

    #[test]
    fn artifact_external_controller_store_publishes_replays_and_advances_exact_chain() {
        let admitted_fixture =
            ArtifactExternalControllerStateV2::decode(&decode_fixture_hex(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/wire/artifact_f0_pxmj_v2_admitted.hex"
            ))))
            .expect("A fixture");
        let request = admitted_fixture.request().clone();
        let test_root = ArtifactExternalStoreTestRoot::new();
        let binding = ArtifactExternalControllerAuthorityBindingV1::try_new(
            test_root.state_root(),
            request.config_commitment(),
        )
        .expect("test authority binding");
        let mut authority = FixedArtifactExternalAuthority {
            binding: binding.clone(),
        };

        let admitted = ArtifactExternalDeploymentControllerStoreV1::admit(&mut authority, &request);
        assert_eq!(
            admitted.change(),
            ArtifactExternalControllerStoreChangeV1::Changed,
        );
        assert_eq!(
            admitted.result().expect("published A").phase(),
            ArtifactExternalControllerPhaseV2::Admitted,
        );

        let queried = ArtifactExternalDeploymentControllerStoreV1::query(
            &mut authority,
            request.operation_id(),
        );
        assert_eq!(
            queried.change(),
            ArtifactExternalControllerStoreChangeV1::Unchanged,
        );
        assert_eq!(
            queried.result().expect("queried A").phase(),
            ArtifactExternalControllerPhaseV2::Admitted,
        );

        let replayed = ArtifactExternalDeploymentControllerStoreV1::admit(&mut authority, &request);
        assert_eq!(
            replayed.change(),
            ArtifactExternalControllerStoreChangeV1::Unchanged,
        );
        assert_eq!(
            replayed.result().expect("replayed A").phase(),
            ArtifactExternalControllerPhaseV2::Admitted,
        );

        let replayed_state = replayed.result().expect("replayed A").clone();
        let (plan_content, execution, runtime_request) =
            artifact_runtime_suffix(&request, replayed_state.admission());
        let committed = replayed_state
            .commit(plan_content, execution, runtime_request.clone())
            .expect("C successor");
        let committed_wire = committed.encode().expect("C wire");
        let top_root = test_root
            .state_root()
            .join("artifact-external-controller-v1");
        let next_path = top_root.join(".artifact-external.pxmj.next");
        let mut next_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&next_path)
            .expect("create canonical fixed next");
        next_file
            .write_all(&committed_wire)
            .expect("write canonical fixed next");
        next_file.sync_all().expect("sync canonical fixed next");
        drop(next_file);
        fs::File::open(&top_root)
            .expect("open top root")
            .sync_all()
            .expect("sync top root with fixed next");

        let next_query = ArtifactExternalDeploymentControllerStoreV1::query(
            &mut authority,
            request.operation_id(),
        );
        assert_eq!(
            next_query.change(),
            ArtifactExternalControllerStoreChangeV1::Unchanged,
        );
        match next_query.into_result() {
            Err(ArtifactExternalControllerStoreFailureV1::PublicationUncertain(Some(state))) => {
                assert_eq!(*state, committed);
            }
            other => panic!("canonical fixed next query: {other:?}"),
        }

        let (locked_binding, mut locked) =
            ArtifactExternalDeploymentControllerStoreV1::open_exclusive(
                &mut authority,
                request.operation_id(),
            )
            .expect("exclusive controller store");
        let contended = ArtifactExternalDeploymentControllerStoreV1::query(
            &mut authority,
            request.operation_id(),
        );
        assert_eq!(
            contended.change(),
            ArtifactExternalControllerStoreChangeV1::Unchanged,
        );
        assert_eq!(
            contended.into_result(),
            Err(ArtifactExternalControllerStoreFailureV1::Contended),
        );

        assert_eq!(
            locked.state().phase(),
            ArtifactExternalControllerPhaseV2::Admitted,
        );
        let committed_result = locked.commit_successor(&mut authority, &locked_binding, committed);
        assert_eq!(
            committed_result.change(),
            ArtifactExternalControllerStoreChangeV1::Changed,
        );
        let committed = committed_result.into_result().expect("durable C");

        let applying = committed.begin_apply([0x54; 16]).expect("P successor");
        let applying_result = locked.commit_successor(&mut authority, &locked_binding, applying);
        assert_eq!(
            applying_result.change(),
            ArtifactExternalControllerStoreChangeV1::Changed,
        );
        let applying = applying_result.into_result().expect("durable P");

        let runtime_terminal = signed_artifact_receipt(
            &runtime_request,
            ManagedModelAgentStackTerminalOutcomeV1::ActiveReady,
        )
        .expect("ActiveReady terminal");
        let active = applying
            .finish_apply(Some(runtime_terminal), None)
            .expect("R successor");
        let active_result = locked.commit_successor(&mut authority, &locked_binding, active);
        assert_eq!(
            active_result.change(),
            ArtifactExternalControllerStoreChangeV1::Changed,
        );
        assert_eq!(
            active_result.into_result().expect("durable R").phase(),
            ArtifactExternalControllerPhaseV2::ActiveReady,
        );
        locked.release().expect("release top lock");

        let final_query = ArtifactExternalDeploymentControllerStoreV1::query(
            &mut authority,
            request.operation_id(),
        );
        assert_eq!(
            final_query.change(),
            ArtifactExternalControllerStoreChangeV1::Unchanged,
        );
        assert_eq!(
            final_query.result().expect("queried R").phase(),
            ArtifactExternalControllerPhaseV2::ActiveReady,
        );

        let public_binding = DeveloperArtifactExternalControllerAuthorityBindingV1::try_new(
            test_root.state_root(),
            request.config_commitment(),
        )
        .expect("public authority binding");
        let mut public_authority = FixedDeveloperArtifactExternalAuthority {
            binding: public_binding,
        };
        let projected = DeveloperArtifactExternalControllerV1::query(
            &mut public_authority,
            request.operation_id(),
        );
        assert_eq!(projected.changed(), Some(false));
        let projected = projected.result().expect("projected R");
        assert_eq!(
            projected.phase(),
            DeveloperArtifactExternalControllerPhaseV1::ActiveReady,
        );
        assert_eq!(projected.object_ref(), request.binding().object_ref());
        assert_eq!(
            projected.materialization_receipt_ref(),
            request.binding().materialization_receipt_ref(),
        );
        assert_eq!(projected.lifecycle_generation(), Some([0x54; 16]));
        assert_eq!(
            projected.deployment_revision().map(NonZeroU64::get),
            Some(1),
        );
        assert_eq!(
            projected
                .committed_controller_snapshot_sequence()
                .map(NonZeroU64::get),
            Some(2),
        );
        assert!(projected.deployment_receipt_ref().is_some());
        assert_eq!(
            projected.terminal_outcome(),
            Some(DeveloperArtifactExternalControllerTerminalOutcomeV1::ActiveReady),
        );
    }

    #[test]
    fn developer_external_controller_facade_admits_without_exposing_owner_state() {
        let admitted_fixture =
            ArtifactExternalControllerStateV2::decode(&decode_fixture_hex(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/wire/artifact_f0_pxmj_v2_admitted.hex"
            ))))
            .expect("A fixture");
        let internal = admitted_fixture.request();
        let request = DeveloperArtifactExternalControllerRequestV1::try_new(
            internal.operation_id(),
            internal.config_commitment(),
            internal.binding().object_ref(),
            internal.binding().materialization_receipt_ref(),
        )
        .expect("public request");
        let test_root = ArtifactExternalStoreTestRoot::new();
        let binding = DeveloperArtifactExternalControllerAuthorityBindingV1::try_new(
            test_root.state_root(),
            request.config_commitment(),
        )
        .expect("public authority binding");
        let mut authority = FixedDeveloperArtifactExternalAuthority { binding };

        let admitted = DeveloperArtifactExternalControllerV1::admit_under_lifecycle_owner(
            &mut authority,
            &request,
        );
        assert_eq!(admitted.changed(), Some(true));
        let projection = admitted.result().expect("projected A");
        assert_eq!(
            projection.phase(),
            DeveloperArtifactExternalControllerPhaseV1::Admitted,
        );
        assert_eq!(projection.operation_id(), request.operation_id());
        assert_eq!(projection.deployment_revision(), None);
        assert_eq!(projection.committed_controller_snapshot_sequence(), None);
        assert_eq!(projection.deployment_receipt_ref(), None);
        assert_eq!(projection.runtime_apply_request_digest(), None);
        assert_eq!(projection.runtime_terminal_receipt_digest(), None);
        assert_eq!(projection.terminal_outcome(), None);
    }

    #[test]
    fn artifact_external_controller_state_v2_rejects_header_and_phase_drift() {
        let (request, admission, _, _, _) = artifact_runtime_prefix();
        let state =
            ArtifactExternalControllerStateV2::try_new(ArtifactExternalControllerStateInputV2 {
                phase: ArtifactExternalControllerPhaseV2::Admitted,
                controller_snapshot_sequence: NonZeroU64::new(1).expect("sequence"),
                request,
                admission,
                plan_content: None,
                execution: None,
                runtime_request: None,
                runtime_terminal: None,
                records: Vec::new(),
                receipt: None,
            })
            .expect("PXMJ2-A");
        let wire = state.encode().expect("PXMJ2-A wire");
        for mut drift in [
            {
                let mut value = wire.to_vec();
                value[12] = b'C';
                value
            },
            {
                let mut value = wire.to_vec();
                value[16..24].copy_from_slice(&2_u64.to_be_bytes());
                value
            },
            {
                let mut value = wire.to_vec();
                value[64..72].copy_from_slice(&1_u64.to_be_bytes());
                value
            },
            {
                let mut value = wire.to_vec();
                value[168..170].copy_from_slice(&1_u16.to_be_bytes());
                value
            },
        ] {
            rewrite_artifact_state_checksum(&mut drift);
            assert!(ArtifactExternalControllerStateV2::decode(&drift).is_err());
        }
        let mut trailing = wire.to_vec();
        trailing.push(0);
        assert!(ArtifactExternalControllerStateV2::decode(&trailing).is_err());
    }

    #[test]
    fn artifact_external_controller_state_v2_matches_independent_shared_initial_goldens() {
        for (wire, phase, sequence, record_state) in [
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_admitted.hex"
                )),
                ArtifactExternalControllerPhaseV2::Admitted,
                1,
                None,
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_failed_pre_c.hex"
                )),
                ArtifactExternalControllerPhaseV2::Failed,
                2,
                Some(ArtifactExternalDeploymentRecordStateV1::Failed),
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_uncertain_pre_c.hex"
                )),
                ArtifactExternalControllerPhaseV2::Uncertain,
                2,
                Some(ArtifactExternalDeploymentRecordStateV1::Uncertain),
            ),
        ] {
            let state = ArtifactExternalControllerStateV2::decode(&wire)
                .expect("independent shared PXMJ2 fixture");
            assert_eq!(state.phase(), phase);
            assert_eq!(state.controller_snapshot_sequence().get(), sequence);
            assert_eq!(state.request().operation_id().as_bytes(), &[0xd1; 16]);
            assert_eq!(state.request().config_commitment().as_bytes(), &[0xa1; 32],);
            assert_eq!(state.admission().controller_store_instance(), &[0xd0; 32]);
            assert_eq!(state.admission().admission_sequence().get(), 1);
            assert!(state.runtime_request().is_none());
            assert!(state.runtime_terminal().is_none());
            assert_eq!(
                state.encode().expect("canonical PXMJ2 re-encode").as_ref(),
                wire.as_slice(),
            );

            match record_state {
                None => {
                    assert!(state.records().is_empty());
                    assert!(state.receipt().is_none());
                }
                Some(expected) => {
                    let record = state.records().last().expect("pre-C terminal PXDM");
                    assert_eq!(state.records().len(), 1);
                    assert_eq!(record.state(), expected);
                    assert_eq!(record.progress().deployment_revision, 0);
                    assert_eq!(record.progress().controller_snapshot_sequence, 0);
                    assert_eq!(record.progress().lifecycle_generation, [0xd2; 16]);
                    assert_eq!(
                        state
                            .receipt()
                            .expect("pre-C PXDO")
                            .receipt_sequence()
                            .get(),
                        1,
                    );
                }
            }
        }
    }

    #[test]
    fn artifact_external_controller_state_v2_matches_all_shared_successor_goldens() {
        for (wire, phase, snapshot_sequence, record_count, expected_terminal_outcome) in [
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_committed.hex"
                )),
                ArtifactExternalControllerPhaseV2::Committed,
                2,
                1,
                None,
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_applying.hex"
                )),
                ArtifactExternalControllerPhaseV2::Applying,
                3,
                2,
                None,
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_active_ready.hex"
                )),
                ArtifactExternalControllerPhaseV2::ActiveReady,
                4,
                3,
                Some(ManagedModelAgentStackTerminalOutcomeV1::ActiveReady),
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_failed_post_c.hex"
                )),
                ArtifactExternalControllerPhaseV2::Failed,
                3,
                2,
                None,
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_uncertain_post_c.hex"
                )),
                ArtifactExternalControllerPhaseV2::Uncertain,
                3,
                2,
                None,
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_failed_post_p.hex"
                )),
                ArtifactExternalControllerPhaseV2::Failed,
                4,
                3,
                Some(ManagedModelAgentStackTerminalOutcomeV1::NoEffectRejected),
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_uncertain_post_p.hex"
                )),
                ArtifactExternalControllerPhaseV2::Uncertain,
                4,
                3,
                Some(ManagedModelAgentStackTerminalOutcomeV1::Uncertain),
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_failed_quarantined_post_p.hex"
                )),
                ArtifactExternalControllerPhaseV2::Failed,
                4,
                3,
                Some(ManagedModelAgentStackTerminalOutcomeV1::Quarantined),
            ),
            (
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxmj_v2_uncertain_post_p_no_pxmt.hex"
                )),
                ArtifactExternalControllerPhaseV2::Uncertain,
                4,
                3,
                None,
            ),
        ] {
            let state = ArtifactExternalControllerStateV2::decode(&wire)
                .expect("independent shared PXMJ2 successor fixture");
            assert_eq!(state.phase(), phase);
            assert_eq!(
                state.controller_snapshot_sequence().get(),
                snapshot_sequence,
            );
            assert_eq!(state.records().len(), record_count);
            assert_eq!(state.request().operation_id().as_bytes(), &[0xd1; 16]);
            let runtime_request = state.runtime_request().expect("shared exact PXAR12");
            assert_eq!(runtime_request.operation_id().as_bytes(), &[0xd4; 16]);
            assert_eq!(
                runtime_request.canonical_wire(),
                decode_fixture_hex(include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxar_v12.hex"
                )),
            );
            let last = state
                .records()
                .last()
                .expect("shared terminal/progress record");
            assert_eq!(last.progress().deployment_revision, 1);
            assert_eq!(last.progress().controller_snapshot_sequence, 2);
            assert_eq!(
                last.progress().runtime_apply_request_digest,
                if record_count == 1
                    || snapshot_sequence == 3
                        && phase != ArtifactExternalControllerPhaseV2::Applying
                {
                    [0; 32]
                } else {
                    *runtime_request.envelope_request_digest().as_bytes()
                },
            );
            match expected_terminal_outcome {
                Some(outcome) => {
                    let terminal = state.runtime_terminal().expect("shared exact PXMT");
                    assert_eq!(terminal.facts().state().outcome(), outcome);
                    terminal
                        .validate_artifact_request_correlation(runtime_request)
                        .expect("PXMJ2 PXMT correlation");
                    assert_eq!(
                        last.progress().runtime_terminal_receipt_digest,
                        *terminal.receipt_digest().as_bytes(),
                    );
                }
                None => {
                    assert!(state.runtime_terminal().is_none());
                    assert_eq!(last.progress().runtime_terminal_receipt_digest, [0; 32]);
                }
            }
            assert_eq!(
                state.receipt().is_some(),
                !matches!(
                    phase,
                    ArtifactExternalControllerPhaseV2::Committed
                        | ArtifactExternalControllerPhaseV2::Applying
                )
            );
            assert_eq!(
                state.encode().expect("canonical PXMJ2 re-encode").as_ref(),
                wire.as_slice(),
            );
        }
    }
}
