//! Durable Controller state for the PXAR v9 Fabric/Model/Agent sibling.
//!
//! PXMJ v1 retains the exact PXAR v6 predecessor without claiming PXAR v7
//! executed. Every valid, authenticated PXMT is durable, including uncertain,
//! quarantined, and no-effect outcomes. Only `ActiveReady` opens the explicit
//! empty transition and only `EmptyExactZero` is deactivation success.

use core::num::NonZeroU64;
use core::{fmt, str::FromStr};

use ed25519_dalek::Signature;
use paraegox_artifact::{ArtifactConfigCommitmentV1, ArtifactContractError, ArtifactObjectRefV1};
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
    ManagedModelAgentStackTerminalReceiptV1,
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
pub(crate) struct ArtifactDeploymentOperationIdV1([u8; 16]);

impl ArtifactDeploymentOperationIdV1 {
    pub(crate) const fn try_from_bytes(
        bytes: [u8; 16],
    ) -> Result<Self, ManagedModelAgentStackApplyControllerError> {
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != 0 {
                return Ok(Self(bytes));
            }
            index += 1;
        }
        Err(ManagedModelAgentStackApplyControllerError::InvalidState)
    }

    #[must_use]
    pub(crate) const fn as_bytes(&self) -> &[u8; 16] {
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
        )?;
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
            )?)?;
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

    use ed25519_dalek::{Signer, SigningKey};
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
            artifact_execution_profile_commitment_v1().as_bytes(),
            &[
                0x1f, 0xe2, 0x43, 0xfd, 0x90, 0x34, 0xf0, 0x4d, 0xae, 0xc0, 0xc0, 0x23, 0x66, 0x1c,
                0x6f, 0x3e, 0x8b, 0x48, 0x78, 0xf6, 0xea, 0xab, 0x6b, 0xa0, 0x59, 0x4b, 0x37, 0x16,
                0x1e, 0xc8, 0x8c, 0x1e,
            ],
        );
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
        assert!(ArtifactDeploymentOperationIdV1::try_from_bytes([0; 16]).is_err());
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
                    &deployment_request,
                    &admission,
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
            deployment_request,
            admission,
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
        let facts =
            ManagedModelAgentStackTerminalFactsV1::try_new_artifact_bound(request, state, evidence)?;
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
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&committed.encode().expect("PXMJ2-C wire"))
                .expect("reopen C"),
            committed,
        );

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
            let reopened = ArtifactExternalControllerStateV2::decode(&wire)
                .expect("reopen post-P terminal");
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
                reopened.records().last().expect("terminal record").progress()
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

        assert!(signed_artifact_receipt(
            &runtime_request,
            ManagedModelAgentStackTerminalOutcomeV1::EmptyExactZero,
        )
        .is_err());

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
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(&failed.encode().expect("PXMJ2-F wire"))
                .expect("reopen F"),
            failed,
        );

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
        let pre_commit_uncertain = ArtifactExternalControllerStateV2::try_new(
            ArtifactExternalControllerStateInputV2 {
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
            },
        )
        .expect("PXMJ2-U pre-C");
        assert_eq!(
            ArtifactExternalControllerStateV2::decode(
                &pre_commit_uncertain
                    .encode()
                    .expect("PXMJ2-U pre-C wire"),
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
}
