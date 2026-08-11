//! Canonical Artifact F0 contracts and Unix store ownership.
//!
//! This crate owns the versioned bytes, typed references, strict snapshot
//! reducer, verified read bundles, and descriptor-relative Unix filesystem and
//! advisory-lock behavior. It deliberately owns no process, deployment, or
//! Runtime behavior.

mod contract;
#[cfg(unix)]
mod store;

pub use contract::{
    ARTIFACT_DEFENSE_CEILING_BYTES, ArtifactCapacityInputV1, ArtifactConfigCommitmentV1,
    ArtifactContractError, ArtifactFilesystemClaimV1, ArtifactManifestProfileClassificationV1,
    ArtifactManifestV1, ArtifactObjectRecordV1, ArtifactObjectRefV1, ArtifactOperationIdV1,
    ArtifactQuarantineFactsV1, ArtifactRecoveryStartV1, ArtifactSnapshotSuccessorV1,
    ArtifactStoreInstanceV1, ArtifactStoreSnapshotCandidateV1, ArtifactStoreSnapshotV1,
    MAX_ARTIFACT_OBJECTS, MAX_ARTIFACT_OPERATIONS, MAX_ARTIFACT_PAYLOAD_BYTES,
    MAX_ARTIFACT_QUARANTINE_BYTES, MAX_ARTIFACT_SNAPSHOT_BYTES, MAX_PXAY_BODY_BYTES,
    MaterializationAdmissionV1, MaterializationOperationV1, MaterializationReceiptRefV1,
    MaterializationReceiptV1, MaterializationRequestV1, MaterializationTerminalStateV1,
    MaterializationTerminalV1, MaterializingRecordV1, PXAA_BYTES, PXAK_BYTES, PXAM_BYTES,
    PXAQ_BYTES, PXAV_BYTES, PXAW_BYTES, PXAX_BYTES, PXAY_HEADER_BYTES, PXAZ_HEADER_BYTES,
    PXMU_BYTES, PXOP_HEADER_BYTES, VerifiedArtifactPairV1, VerifiedMaterializationReadBundleV1,
};

#[cfg(unix)]
pub use store::{
    ArtifactStoreAuthorityBindingV1, ArtifactStoreAuthorityRecheckFailureV1,
    ArtifactStoreAuthorityV1, ArtifactStoreChangeV1, ArtifactStoreFailureV1,
    ArtifactStoreInvocationV1, ArtifactStoreOperationStateV1, ArtifactStoreOperationViewV1,
    ArtifactStoreReadFailureV1, ArtifactStoreV1,
};
