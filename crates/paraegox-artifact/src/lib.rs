//! Pure canonical Artifact F0 contracts.
//!
//! This crate owns the versioned bytes, typed references, strict snapshot
//! reducer, and verified read bundles. It deliberately owns no filesystem,
//! lock, process, deployment, or Runtime behavior.

mod contract;

pub use contract::{
    ArtifactCapacityInputV1, ArtifactConfigCommitmentV1, ArtifactContractError,
    ArtifactFilesystemClaimV1, ArtifactManifestV1, ArtifactObjectRecordV1,
    ArtifactObjectRefV1, ArtifactOperationIdV1, ArtifactQuarantineFactsV1,
    ArtifactRecoveryStartV1, ArtifactSnapshotSuccessorV1, ArtifactStoreInstanceV1,
    ArtifactStoreSnapshotCandidateV1, ArtifactStoreSnapshotV1, MaterializationAdmissionV1,
    MaterializationOperationV1, MaterializationReceiptRefV1, MaterializationReceiptV1,
    MaterializationRequestV1, MaterializationTerminalStateV1, MaterializationTerminalV1,
    MaterializingRecordV1, VerifiedArtifactPairV1, VerifiedMaterializationReadBundleV1,
    ARTIFACT_DEFENSE_CEILING_BYTES, MAX_ARTIFACT_OBJECTS, MAX_ARTIFACT_OPERATIONS,
    MAX_ARTIFACT_PAYLOAD_BYTES, MAX_ARTIFACT_QUARANTINE_BYTES, MAX_ARTIFACT_SNAPSHOT_BYTES,
    MAX_PXAY_BODY_BYTES, PXAA_BYTES, PXAK_BYTES, PXAM_BYTES, PXAV_BYTES, PXAW_BYTES,
    PXAX_BYTES, PXAQ_BYTES, PXAZ_HEADER_BYTES, PXMU_BYTES, PXOP_HEADER_BYTES,
    PXAY_HEADER_BYTES,
};
