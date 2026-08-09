//! Canonical single-target successor for asymmetric remote Agent access.
//!
//! This contract retains one exact PXTE v6 `FabricAndAgent` desired value and
//! adds only the transport requirements needed for one Ubuntu RuntimeHost to
//! listen for one Mac Agent client. It is deliberately not the symmetric
//! PXTE-v7/PXAR-v8 Runtime-peer topology: it carries no peer RuntimeHost and no
//! Runtime-side TLS connect endpoint. Credential values remain opaque refs.
//!
//! PXAD/PXTE-v9/PXAR-v10 are desired-state and request contracts. They do not
//! open a Zenoh session, resolve a credential, grant descriptor access, retry a
//! request, or create a second Runtime lifecycle owner. PXAU is a signed
//! terminal fact; a fresh PXRA/PXRR Describe remains necessary before use.

use core::fmt;

use paraegox_kernel::digest::{Digest32, Digest32Builder, DigestBuildError};
use paraegox_kernel::identity::{PrincipalRef, RuntimeHostId};
use paraegox_kernel::time::{ClockDomainRef, ClockGeneration};

use crate::apply::{ApplyOperationId, RuntimeApplyControl, RuntimeApplyControlCommitment};
use crate::assignment::TargetAssignments;
use crate::distributed_agent_stack_plan::{
    DistributedAgentStackPlanError, DistributedFabricCredentialRefV1,
    DistributedFabricSessionEpochV1, DistributedFabricTlsEndpointV1,
    DistributedFabricTrustAnchorRefV1, DistributedFabricTrustDomainRefV1,
    MAX_DISTRIBUTED_FABRIC_ENDPOINT_BYTES, MAX_RESTRICTED_RUNTIME_APPLY_OPERATION_TIMEOUT_NANOS,
};
use crate::managed_agent_stack_plan::{
    MANAGED_AGENT_STACK_PROJECTION_BYTES, MAX_MANAGED_AGENT_FRAME_BYTES,
    MAX_MANAGED_AGENT_RESPONSE_BODY_BYTES, MAX_MANAGED_AGENT_STACK_TARGET_EXECUTION_BYTES,
    ManagedAgentStackPlanError, ManagedAgentStackProjectionV1, ManagedAgentStackTargetExecutionV1,
    ManagedAgentStackTargetModeV1,
};
use crate::managed_fabric_plan::ManagedFabricListenEndpointV1;
use crate::managed_service::ManagedServiceGeneration;
use crate::provenance::{
    PlanProvenance, RuntimeSliceCommitment, RuntimeSliceHeader, SourceScopeRef,
    TargetAssignmentDigest, TargetSliceDigest,
};
use crate::reference_assembly::{
    ApplyRequestSigningTranscriptV2, MAX_CONTROL_READ_SIGNATURE_BYTES,
    MAX_RUNTIME_APPLY_ENVELOPE_V2_BYTES, RuntimeApplyEnvelopeV2, RuntimeApplyEnvelopeV2Draft,
    RuntimeStoreInstanceId,
};
use crate::temporal::ApplyTemporalConstraint;
use crate::wire::{
    ApplyAuthAlgorithm, ApplyAuthKeyRef, ApplyRequestAuthClaim, ApplyRequestAuthentication,
};

const PROJECTION_MAGIC: &[u8; 4] = b"PXAE";
const PROFILE_MAGIC: &[u8; 4] = b"PXAD";
const TARGET_EXECUTION_MAGIC: &[u8; 4] = b"PXTE";
const APPLY_REQUEST_MAGIC: &[u8; 4] = b"PXAR";
const TERMINAL_RECEIPT_MAGIC: &[u8; 4] = b"PXAU";
const EMPTY_PXTA: [u8; 10] = [b'P', b'X', b'T', b'A', 0, 1, 0, 0, 0, 0];

const APPLY_REQUEST_HEADER_BYTES: usize = 18;
const PROJECTION_BYTES: usize = 4 + 2 + MANAGED_AGENT_STACK_PROJECTION_BYTES + 32 + 2 + 2;
const PROFILE_FIXED_BYTES: usize = 158;
const BOOTSTRAP_CAS_BYTES: usize = (4 * 32) + (2 * 8);
const TARGET_EXECUTION_FIXED_BYTES: usize = 4 + 2 + PROJECTION_BYTES + 2 + 1 + 1 + 4 + 4;
const TERMINAL_FIXED_BYTES: usize = 4
    + 2
    + 16
    + 32
    + 16
    + 16
    + (4 * 32)
    + 16
    + 6
    + 32
    + (3 * 9)
    + 2
    + 1
    + 1
    + (5 * 32)
    + 8
    + 8
    + 16
    + 8
    + 8
    + 16
    + 16
    + 2
    + 2
    + 2;
const ASYMMETRIC_LISTENER_CONNECTOR_PROFILE_KIND: u16 = 1;
const ASYMMETRIC_AGENT_ACL_PROFILE_VERSION: u16 = 1;
const RETAINED_LOCAL_AGENT_BINDING_CENSUS: u16 = 2;

const COMPATIBILITY_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-compatibility.sha256.v1";
const PROFILE_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-data-plane-profile.sha256.v1";
const TARGET_EXECUTION_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.target-execution.sha256.v9";
const TARGET_ASSIGNMENTS_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.target-plan-assignments.sha256.v10";
const APPLY_REQUEST_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-request.sha256.v1";
const TERMINAL_SIGNING_MAGIC: &[u8] = b"ParaEGOX\0remote-agent-data-plane-terminal-signing";
const TERMINAL_RESULT_REF_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-terminal-result.sha256.v1";
const TERMINAL_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-data-plane-terminal.sha256.v1";

/// Exact additive projection version carried by PXAE.
pub const REMOTE_AGENT_DATA_PLANE_PROJECTION_VERSION: u16 = 1;
/// Exact asymmetric transport profile version carried by PXAD.
pub const REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION: u16 = 1;
/// Strict apply request version carried by PXAR.
pub const REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_VERSION: u16 = 10;
/// Strict target-execution version carried by PXTE.
pub const REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_VERSION: u16 = 9;
/// Signed Runtime terminal version carried by PXAU.
pub const REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_VERSION: u16 = 1;
/// Domain-separated PXAU signing transcript version.
pub const REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNING_VERSION: u16 = 1;
/// Exact fixed projection width.
pub const REMOTE_AGENT_DATA_PLANE_PROJECTION_BYTES: usize = PROJECTION_BYTES;
/// Maximum canonical PXAD bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES: usize = PROFILE_FIXED_BYTES
    + crate::managed_fabric_plan::MAX_MANAGED_FABRIC_LISTEN_ENDPOINT_BYTES
    + MAX_DISTRIBUTED_FABRIC_ENDPOINT_BYTES;
/// Maximum canonical PXTE v9 bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES: usize = TARGET_EXECUTION_FIXED_BYTES
    + MAX_MANAGED_AGENT_STACK_TARGET_EXECUTION_BYTES
    + BOOTSTRAP_CAS_BYTES
    + MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES;
/// Maximum canonical PXTA-zero plus PXTE-v9 durable Slice bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_PLAN_SLICE_BYTES: usize =
    EMPTY_PXTA.len() + MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES;
/// Maximum canonical PXAR v10 request bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES: usize = APPLY_REQUEST_HEADER_BYTES
    + MAX_RUNTIME_APPLY_ENVELOPE_V2_BYTES
    + EMPTY_PXTA.len()
    + MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES;
/// Maximum canonical PXAU bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES: usize = 2_048;
/// Maximum Runtime signature retained by PXAU.
pub const MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_BYTES: usize =
    MAX_CONTROL_READ_SIGNATURE_BYTES;

/// Producer fields for one exact listener-only/server and connector-only/client profile.
#[derive(Clone, Debug)]
pub struct RemoteAgentDataPlaneProfileFieldsV1<'a> {
    pub target: RuntimeHostId,
    pub base_loopback_listen_endpoint: &'a str,
    pub ubuntu_tls_listener_endpoint: &'a str,
    pub endpoint_ref: [u8; 16],
    pub endpoint_generation: u64,
    pub trust_domain_ref: DistributedFabricTrustDomainRefV1,
    pub trust_anchor_ref: DistributedFabricTrustAnchorRefV1,
    pub mac_connector_credential_ref: DistributedFabricCredentialRefV1,
    pub ubuntu_listener_credential_ref: DistributedFabricCredentialRefV1,
    pub mac_agent_client_principal: PrincipalRef,
    pub ubuntu_agent_listener_principal: PrincipalRef,
    pub operation_timeout_nanos: u64,
}

/// Canonical PXAD v1 asymmetric Agent data-plane requirement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneProfileV1 {
    target: RuntimeHostId,
    base_loopback_listen_endpoint: ManagedFabricListenEndpointV1,
    ubuntu_tls_listener_endpoint: DistributedFabricTlsEndpointV1,
    endpoint_ref: [u8; 16],
    endpoint_generation: u64,
    trust_domain_ref: DistributedFabricTrustDomainRefV1,
    trust_anchor_ref: DistributedFabricTrustAnchorRefV1,
    mac_connector_credential_ref: DistributedFabricCredentialRefV1,
    ubuntu_listener_credential_ref: DistributedFabricCredentialRefV1,
    mac_agent_client_principal: PrincipalRef,
    ubuntu_agent_listener_principal: PrincipalRef,
    operation_timeout_nanos: u64,
    canonical_wire: Box<[u8]>,
    profile_digest: Digest32,
}

impl RemoteAgentDataPlaneProfileV1 {
    /// Builds the fixed asymmetric profile. No connector endpoint exists on Ubuntu.
    pub fn try_new(
        fields: RemoteAgentDataPlaneProfileFieldsV1<'_>,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let base_loopback_listen_endpoint =
            ManagedFabricListenEndpointV1::try_new(fields.base_loopback_listen_endpoint)?;
        let ubuntu_tls_listener_endpoint =
            DistributedFabricTlsEndpointV1::try_new(fields.ubuntu_tls_listener_endpoint)?;
        if bytes_are_zero(fields.target.as_bytes())
            || bytes_are_zero(&fields.endpoint_ref)
            || fields.endpoint_generation == 0
            || bytes_are_zero(fields.mac_agent_client_principal.as_bytes())
            || bytes_are_zero(fields.ubuntu_agent_listener_principal.as_bytes())
            || fields.mac_agent_client_principal == fields.ubuntu_agent_listener_principal
            || fields.mac_connector_credential_ref == fields.ubuntu_listener_credential_ref
            || fields.operation_timeout_nanos == 0
            || fields.operation_timeout_nanos > MAX_RESTRICTED_RUNTIME_APPLY_OPERATION_TIMEOUT_NANOS
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidProfile);
        }
        let canonical_wire = build_profile_wire(
            &fields,
            &base_loopback_listen_endpoint,
            &ubuntu_tls_listener_endpoint,
        )?;
        let profile_digest = digest_wire(PROFILE_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            target: fields.target,
            base_loopback_listen_endpoint,
            ubuntu_tls_listener_endpoint,
            endpoint_ref: fields.endpoint_ref,
            endpoint_generation: fields.endpoint_generation,
            trust_domain_ref: fields.trust_domain_ref,
            trust_anchor_ref: fields.trust_anchor_ref,
            mac_connector_credential_ref: fields.mac_connector_credential_ref,
            ubuntu_listener_credential_ref: fields.ubuntu_listener_credential_ref,
            mac_agent_client_principal: fields.mac_agent_client_principal,
            ubuntu_agent_listener_principal: fields.ubuntu_agent_listener_principal,
            operation_timeout_nanos: fields.operation_timeout_nanos,
            canonical_wire: canonical_wire.into_boxed_slice(),
            profile_digest,
        })
    }

    /// Strictly decodes PXAD v1 and rejects every other topology/profile wire.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() > MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        if frame.len() < PROFILE_FIXED_BYTES {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.take(4)? != PROFILE_MAGIC
            || cursor.u16()? != REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION
            || cursor.u16()? != ASYMMETRIC_LISTENER_CONNECTOR_PROFILE_KIND
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let base_length = cursor.usize_u16()?;
        let tls_length = cursor.usize_u16()?;
        if cursor.u16()? != ASYMMETRIC_AGENT_ACL_PROFILE_VERSION
            || base_length == 0
            || tls_length == 0
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidLength);
        }
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let endpoint_ref = cursor.array()?;
        let endpoint_generation = cursor.u64()?;
        let trust_domain_ref = DistributedFabricTrustDomainRefV1::try_from_bytes(cursor.array()?)?;
        let trust_anchor_ref = DistributedFabricTrustAnchorRefV1::try_from_bytes(cursor.array()?)?;
        let mac_connector_credential_ref =
            DistributedFabricCredentialRefV1::try_from_bytes(cursor.array()?)?;
        let ubuntu_listener_credential_ref =
            DistributedFabricCredentialRefV1::try_from_bytes(cursor.array()?)?;
        let mac_agent_client_principal = PrincipalRef::from_bytes(cursor.array()?);
        let ubuntu_agent_listener_principal = PrincipalRef::from_bytes(cursor.array()?);
        let operation_timeout_nanos = cursor.u64()?;
        let base_loopback_listen_endpoint = core::str::from_utf8(cursor.take(base_length)?)
            .map_err(|_| RemoteAgentDataPlanePlanError::InvalidProfile)?;
        let ubuntu_tls_listener_endpoint = core::str::from_utf8(cursor.take(tls_length)?)
            .map_err(|_| RemoteAgentDataPlanePlanError::InvalidProfile)?;
        cursor.finish()?;
        let decoded = Self::try_new(RemoteAgentDataPlaneProfileFieldsV1 {
            target,
            base_loopback_listen_endpoint,
            ubuntu_tls_listener_endpoint,
            endpoint_ref,
            endpoint_generation,
            trust_domain_ref,
            trust_anchor_ref,
            mac_connector_credential_ref,
            ubuntu_listener_credential_ref,
            mac_agent_client_principal,
            ubuntu_agent_listener_principal,
            operation_timeout_nanos,
        })?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.target
    }

    #[must_use]
    pub const fn base_loopback_listen_endpoint(&self) -> &ManagedFabricListenEndpointV1 {
        &self.base_loopback_listen_endpoint
    }

    #[must_use]
    pub const fn ubuntu_tls_listener_endpoint(&self) -> &DistributedFabricTlsEndpointV1 {
        &self.ubuntu_tls_listener_endpoint
    }

    #[must_use]
    pub const fn endpoint_ref(&self) -> [u8; 16] {
        self.endpoint_ref
    }

    #[must_use]
    pub const fn endpoint_generation(&self) -> u64 {
        self.endpoint_generation
    }

    #[must_use]
    pub const fn trust_domain_ref(&self) -> DistributedFabricTrustDomainRefV1 {
        self.trust_domain_ref
    }

    #[must_use]
    pub const fn trust_anchor_ref(&self) -> DistributedFabricTrustAnchorRefV1 {
        self.trust_anchor_ref
    }

    #[must_use]
    pub const fn mac_connector_credential_ref(&self) -> DistributedFabricCredentialRefV1 {
        self.mac_connector_credential_ref
    }

    #[must_use]
    pub const fn ubuntu_listener_credential_ref(&self) -> DistributedFabricCredentialRefV1 {
        self.ubuntu_listener_credential_ref
    }

    #[must_use]
    pub const fn mac_agent_client_principal(&self) -> PrincipalRef {
        self.mac_agent_client_principal
    }

    #[must_use]
    pub const fn ubuntu_agent_listener_principal(&self) -> PrincipalRef {
        self.ubuntu_agent_listener_principal
    }

    #[must_use]
    pub const fn operation_timeout_nanos(&self) -> u64 {
        self.operation_timeout_nanos
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub const fn profile_digest(&self) -> Digest32 {
        self.profile_digest
    }
}

/// Additive projection for the single-target asymmetric successor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneProjectionV1 {
    managed_agent_stack: ManagedAgentStackProjectionV1,
    compatibility_digest: Digest32,
    canonical_wire: Box<[u8]>,
}

impl RemoteAgentDataPlaneProjectionV1 {
    pub fn try_from_managed_agent_stack_projection(
        managed_agent_stack: ManagedAgentStackProjectionV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let compatibility_digest = remote_agent_data_plane_compatibility_digest_v1()?;
        let canonical_wire = build_projection_wire(&managed_agent_stack, compatibility_digest);
        Ok(Self {
            managed_agent_stack,
            compatibility_digest,
            canonical_wire: canonical_wire.into_boxed_slice(),
        })
    }

    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() != PROJECTION_BYTES {
            return Err(if frame.len() < PROJECTION_BYTES {
                RemoteAgentDataPlanePlanError::Truncated
            } else {
                RemoteAgentDataPlanePlanError::FrameTooLarge
            });
        }
        if &frame[..4] != PROJECTION_MAGIC
            || read_u16(&frame[4..6]) != REMOTE_AGENT_DATA_PLANE_PROJECTION_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let predecessor_end = 6 + MANAGED_AGENT_STACK_PROJECTION_BYTES;
        let managed_agent_stack =
            ManagedAgentStackProjectionV1::decode(&frame[6..predecessor_end])?;
        let compatibility_digest =
            Digest32::from_bytes(read_array(&frame[predecessor_end..predecessor_end + 32]));
        if compatibility_digest != remote_agent_data_plane_compatibility_digest_v1()?
            || read_u16(&frame[predecessor_end + 32..predecessor_end + 34])
                != REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_VERSION
            || read_u16(&frame[predecessor_end + 34..]) != REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::CompatibilityMismatch);
        }
        let decoded = Self::try_from_managed_agent_stack_projection(managed_agent_stack)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub const fn managed_agent_stack_projection(&self) -> &ManagedAgentStackProjectionV1 {
        &self.managed_agent_stack
    }

    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.managed_agent_stack.target()
    }

    #[must_use]
    pub const fn compatibility_digest(&self) -> Digest32 {
        self.compatibility_digest
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }
}

/// Exact T1 ActiveReady roots and current live generations required by PXTE v9.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentBootstrapCasV1 {
    expected_active_pxft_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    /// Exact whole PXAH receipt digest, independent of its PXAP payload digest.
    expected_bootstrap_descriptor_receipt_digest: Digest32,
    /// PXAP digest from
    /// [`crate::managed_serving_bootstrap::runtime_agent_control_descriptor_payload_digest_v1`].
    expected_bootstrap_descriptor_payload_digest: Digest32,
    expected_fabric_generation: ManagedServiceGeneration,
    expected_agent_generation: ManagedServiceGeneration,
}

impl RemoteAgentBootstrapCasV1 {
    pub fn try_new(
        expected_active_pxft_digest: Digest32,
        expected_active_pxst_digest: Digest32,
        expected_bootstrap_descriptor_receipt_digest: Digest32,
        expected_bootstrap_descriptor_payload_digest: Digest32,
        expected_fabric_generation: ManagedServiceGeneration,
        expected_agent_generation: ManagedServiceGeneration,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if digest_is_zero(expected_active_pxft_digest)
            || digest_is_zero(expected_active_pxst_digest)
            || digest_is_zero(expected_bootstrap_descriptor_receipt_digest)
            || digest_is_zero(expected_bootstrap_descriptor_payload_digest)
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidBootstrapCas);
        }
        Ok(Self {
            expected_active_pxft_digest,
            expected_active_pxst_digest,
            expected_bootstrap_descriptor_receipt_digest,
            expected_bootstrap_descriptor_payload_digest,
            expected_fabric_generation,
            expected_agent_generation,
        })
    }

    #[must_use]
    pub const fn expected_active_pxft_digest(self) -> Digest32 {
        self.expected_active_pxft_digest
    }

    #[must_use]
    pub const fn expected_active_pxst_digest(self) -> Digest32 {
        self.expected_active_pxst_digest
    }

    #[must_use]
    pub const fn expected_bootstrap_descriptor_receipt_digest(self) -> Digest32 {
        self.expected_bootstrap_descriptor_receipt_digest
    }

    /// Digest of the independently validated inner PXAP payload bytes, built
    /// by the shared T1 descriptor-payload digest helper.
    #[must_use]
    pub const fn expected_bootstrap_descriptor_payload_digest(self) -> Digest32 {
        self.expected_bootstrap_descriptor_payload_digest
    }

    #[must_use]
    pub const fn expected_fabric_generation(self) -> ManagedServiceGeneration {
        self.expected_fabric_generation
    }

    #[must_use]
    pub const fn expected_agent_generation(self) -> ManagedServiceGeneration {
        self.expected_agent_generation
    }
}

/// Exact desired shape admitted by PXTE v9.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RemoteAgentDataPlaneTargetModeV1 {
    /// Retain exact local PXTE v6 and add one Ubuntu listener/Mac connector ACL.
    RemoteAccessActive = 1,
    /// Remove remote listener/ACL while retaining the exact local PXTE v6 stack.
    LocalAgentOnlyDeactivate = 2,
}

/// Canonical PXTE v9 retaining exact PXTE v6 in both modes.
///
/// `LocalAgentOnlyDeactivate` retains the exact PXAD revocation target but
/// carries no bootstrap CAS. It is an explicit remote-access revocation, not
/// exact-zero and not permission for an owner to mutate or stop the retained
/// local Fabric/Agent stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTargetExecutionV1 {
    projection: RemoteAgentDataPlaneProjectionV1,
    mode: RemoteAgentDataPlaneTargetModeV1,
    predecessor: ManagedAgentStackTargetExecutionV1,
    bootstrap_cas: Option<RemoteAgentBootstrapCasV1>,
    profile: RemoteAgentDataPlaneProfileV1,
    canonical_wire: Box<[u8]>,
    execution_digest: Digest32,
}

impl RemoteAgentDataPlaneTargetExecutionV1 {
    pub fn try_remote_access_active(
        projection: RemoteAgentDataPlaneProjectionV1,
        predecessor: ManagedAgentStackTargetExecutionV1,
        bootstrap_cas: RemoteAgentBootstrapCasV1,
        profile: RemoteAgentDataPlaneProfileV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        Self::try_new(
            projection,
            RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
            predecessor,
            Some(bootstrap_cas),
            profile,
        )
    }

    /// Creates explicit remote-access revocation while preserving exact PXTE v6.
    pub fn try_local_agent_only_deactivate(
        projection: RemoteAgentDataPlaneProjectionV1,
        predecessor: ManagedAgentStackTargetExecutionV1,
        profile: RemoteAgentDataPlaneProfileV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        Self::try_new(
            projection,
            RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
            predecessor,
            None,
            profile,
        )
    }

    fn try_new(
        projection: RemoteAgentDataPlaneProjectionV1,
        mode: RemoteAgentDataPlaneTargetModeV1,
        predecessor: ManagedAgentStackTargetExecutionV1,
        bootstrap_cas: Option<RemoteAgentBootstrapCasV1>,
        profile: RemoteAgentDataPlaneProfileV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if predecessor.projection() != projection.managed_agent_stack_projection()
            || predecessor.mode() != ManagedAgentStackTargetModeV1::FabricAndAgent
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        match (mode, bootstrap_cas) {
            (RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive, Some(_))
                if profile.target() == projection.target()
                    && predecessor.fabric().listen_endpoint()
                        == Some(profile.base_loopback_listen_endpoint()) => {}
            (RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate, None)
                if profile.target() == projection.target()
                    && predecessor.fabric().listen_endpoint()
                        == Some(profile.base_loopback_listen_endpoint()) => {}
            _ => return Err(RemoteAgentDataPlanePlanError::InvalidShape),
        }
        let canonical_wire =
            build_target_execution_wire(&projection, mode, &predecessor, bootstrap_cas, &profile)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        let execution_digest = digest_wire(TARGET_EXECUTION_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            projection,
            mode,
            predecessor,
            bootstrap_cas,
            profile,
            canonical_wire: canonical_wire.into_boxed_slice(),
            execution_digest,
        })
    }

    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() > MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        if frame.len() < TARGET_EXECUTION_FIXED_BYTES {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.take(4)? != TARGET_EXECUTION_MAGIC
            || cursor.u16()? != REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let projection = RemoteAgentDataPlaneProjectionV1::decode(cursor.take(PROJECTION_BYTES)?)?;
        if cursor.u16()? != REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        let mode = match cursor.u8()? {
            1 => RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
            2 => RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
            _ => return Err(RemoteAgentDataPlanePlanError::InvalidShape),
        };
        let profile_present = cursor.u8()?;
        let predecessor_length = cursor.usize_u32()?;
        let profile_length = cursor.usize_u32()?;
        if predecessor_length == 0
            || predecessor_length > MAX_MANAGED_AGENT_STACK_TARGET_EXECUTION_BYTES
            || profile_length > MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidLength);
        }
        let predecessor =
            ManagedAgentStackTargetExecutionV1::decode(cursor.take(predecessor_length)?)?;
        let (bootstrap_cas, profile) = match (mode, profile_present, profile_length) {
            (RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive, 1, 1..) => (
                Some(decode_bootstrap_cas(&mut cursor)?),
                RemoteAgentDataPlaneProfileV1::decode(cursor.take(profile_length)?)?,
            ),
            (RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate, 1, 1..) => (
                None,
                RemoteAgentDataPlaneProfileV1::decode(cursor.take(profile_length)?)?,
            ),
            _ => return Err(RemoteAgentDataPlanePlanError::InvalidShape),
        };
        cursor.finish()?;
        let decoded = Self::try_new(projection, mode, predecessor, bootstrap_cas, profile)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub const fn projection(&self) -> &RemoteAgentDataPlaneProjectionV1 {
        &self.projection
    }

    #[must_use]
    pub const fn mode(&self) -> RemoteAgentDataPlaneTargetModeV1 {
        self.mode
    }

    #[must_use]
    pub const fn predecessor(&self) -> &ManagedAgentStackTargetExecutionV1 {
        &self.predecessor
    }

    #[must_use]
    pub const fn bootstrap_cas(&self) -> Option<RemoteAgentBootstrapCasV1> {
        self.bootstrap_cas
    }

    #[must_use]
    pub const fn profile(&self) -> &RemoteAgentDataPlaneProfileV1 {
        &self.profile
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub const fn execution_digest(&self) -> Digest32 {
        self.execution_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteAgentDataPlaneAssignmentsV1 {
    bindings: TargetAssignments,
    execution: RemoteAgentDataPlaneTargetExecutionV1,
    assignment_digest: TargetAssignmentDigest,
}

impl RemoteAgentDataPlaneAssignmentsV1 {
    fn try_from_execution(
        execution: RemoteAgentDataPlaneTargetExecutionV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let bindings = TargetAssignments::try_new(Vec::new())
            .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
        Self::try_new(bindings, execution)
    }

    fn try_new(
        bindings: TargetAssignments,
        execution: RemoteAgentDataPlaneTargetExecutionV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        bindings
            .validate()
            .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
        if !bindings.is_empty() || bindings.canonical_wire() != EMPTY_PXTA {
            return Err(RemoteAgentDataPlanePlanError::BindingNotAllowed);
        }
        let mut builder = Digest32Builder::try_new(TARGET_ASSIGNMENTS_DIGEST_DOMAIN)?;
        builder.field_digest(bindings.assignment_digest().value())?;
        builder.field_digest(&execution.execution_digest())?;
        Ok(Self {
            bindings,
            execution,
            assignment_digest: TargetAssignmentDigest::new(builder.finish()),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteAgentDataPlanePlanSliceV1 {
    commitment: RuntimeSliceCommitment,
    assignments: RemoteAgentDataPlaneAssignmentsV1,
}

impl RemoteAgentDataPlanePlanSliceV1 {
    fn try_new(
        commitment: RuntimeSliceCommitment,
        assignments: RemoteAgentDataPlaneAssignmentsV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        commitment.validate()?;
        if commitment.header().assignment_digest() != assignments.assignment_digest {
            return Err(RemoteAgentDataPlanePlanError::CommitmentMismatch);
        }
        if commitment.header().target() != assignments.execution.projection().target() {
            return Err(RemoteAgentDataPlanePlanError::TargetMismatch);
        }
        Ok(Self {
            commitment,
            assignments,
        })
    }
}

/// Canonical envelope-v2 Controller signing transcript for PXAR v10.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneApplySigningTranscriptV2(ApplyRequestSigningTranscriptV2);

impl RemoteAgentDataPlaneApplySigningTranscriptV2 {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Signature-independent PXAR v10 producer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneApplyRequestDraftV1 {
    envelope: RuntimeApplyEnvelopeV2Draft,
    slice: RemoteAgentDataPlanePlanSliceV1,
}

impl RemoteAgentDataPlaneApplyRequestDraftV1 {
    pub fn try_new(
        execution: RemoteAgentDataPlaneTargetExecutionV1,
        provenance: PlanProvenance,
        control: RuntimeApplyControl,
        temporal: ApplyTemporalConstraint,
        expected_runtime_store_instance_id: [u8; 32],
        auth_claim: ApplyRequestAuthClaim,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let assignments = RemoteAgentDataPlaneAssignmentsV1::try_from_execution(execution)?;
        let header = RuntimeSliceHeader::new(
            assignments.execution.projection().target(),
            provenance,
            assignments.assignment_digest,
        );
        let commitment = RuntimeSliceCommitment::try_new(header)?;
        let slice = RemoteAgentDataPlanePlanSliceV1::try_new(commitment, assignments)?;
        let control_commitment = RuntimeApplyControlCommitment::try_new(commitment, control)?;
        let store = RuntimeStoreInstanceId::try_from_bytes(expected_runtime_store_instance_id)?;
        let envelope =
            RuntimeApplyEnvelopeV2Draft::try_new(control_commitment, temporal, store, auth_claim)?;
        Ok(Self { envelope, slice })
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentDataPlaneApplySigningTranscriptV2, RemoteAgentDataPlanePlanError> {
        Ok(RemoteAgentDataPlaneApplySigningTranscriptV2(
            self.envelope.signing_transcript()?,
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentDataPlaneApplyRequestV1, RemoteAgentDataPlanePlanError> {
        let envelope = self.envelope.finalize(signature)?;
        RemoteAgentDataPlaneApplyRequestV1::try_new(envelope, self.slice)
    }
}

/// Signed strict PXAR v10 carrying envelope v2, PXTA-zero, and PXTE v9.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneApplyRequestV1 {
    envelope: RuntimeApplyEnvelopeV2,
    slice: RemoteAgentDataPlanePlanSliceV1,
    canonical_wire: Box<[u8]>,
    request_digest: Digest32,
}

impl RemoteAgentDataPlaneApplyRequestV1 {
    fn try_new(
        envelope: RuntimeApplyEnvelopeV2,
        slice: RemoteAgentDataPlanePlanSliceV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if envelope.control_commitment().slice() != slice.commitment {
            return Err(RemoteAgentDataPlanePlanError::CommitmentMismatch);
        }
        let canonical_wire = build_apply_request_wire(&envelope, &slice)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        let request_digest = digest_wire(APPLY_REQUEST_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            envelope,
            slice,
            canonical_wire: canonical_wire.into_boxed_slice(),
            request_digest,
        })
    }

    /// Strictly decodes only PXAR v10. PXAR v1-v9 retain their old decoders.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() > MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        if frame.len() < APPLY_REQUEST_HEADER_BYTES {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        if &frame[..4] != APPLY_REQUEST_MAGIC
            || read_u16(&frame[4..6]) != REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let envelope_length = read_u32(&frame[6..10]) as usize;
        let bindings_length = read_u32(&frame[10..14]) as usize;
        let execution_length = read_u32(&frame[14..18]) as usize;
        if envelope_length > MAX_RUNTIME_APPLY_ENVELOPE_V2_BYTES
            || bindings_length != EMPTY_PXTA.len()
            || execution_length > MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidLength);
        }
        let expected_length = APPLY_REQUEST_HEADER_BYTES
            .checked_add(envelope_length)
            .and_then(|value| value.checked_add(bindings_length))
            .and_then(|value| value.checked_add(execution_length))
            .ok_or(RemoteAgentDataPlanePlanError::FrameTooLarge)?;
        if frame.len() < expected_length {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        if frame.len() > expected_length {
            return Err(RemoteAgentDataPlanePlanError::TrailingBytes);
        }
        let envelope_start = APPLY_REQUEST_HEADER_BYTES;
        let envelope_end = envelope_start + envelope_length;
        let bindings_end = envelope_end + bindings_length;
        let envelope = RuntimeApplyEnvelopeV2::decode(&frame[envelope_start..envelope_end])?;
        let binding_frame = &frame[envelope_end..bindings_end];
        if binding_frame != EMPTY_PXTA {
            return Err(RemoteAgentDataPlanePlanError::BindingNotAllowed);
        }
        let bindings = TargetAssignments::decode(binding_frame)
            .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
        let execution = RemoteAgentDataPlaneTargetExecutionV1::decode(&frame[bindings_end..])?;
        let assignments = RemoteAgentDataPlaneAssignmentsV1::try_new(bindings, execution)?;
        let slice = RemoteAgentDataPlanePlanSliceV1::try_new(
            envelope.control_commitment().slice(),
            assignments,
        )?;
        let decoded = Self::try_new(envelope, slice)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub fn canonical_slice_wire(&self) -> &[u8] {
        let offset = APPLY_REQUEST_HEADER_BYTES + self.envelope.canonical_wire().len();
        &self.canonical_wire[offset..]
    }

    #[must_use]
    pub const fn target_execution(&self) -> &RemoteAgentDataPlaneTargetExecutionV1 {
        &self.slice.assignments.execution
    }

    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.slice.commitment.header().target()
    }

    #[must_use]
    pub const fn provenance(&self) -> PlanProvenance {
        self.slice.commitment.header().provenance()
    }

    #[must_use]
    pub const fn assignment_digest(&self) -> TargetAssignmentDigest {
        self.slice.commitment.header().assignment_digest()
    }

    #[must_use]
    pub const fn target_slice_digest(&self) -> TargetSliceDigest {
        self.slice.commitment.target_slice_digest()
    }

    #[must_use]
    pub const fn control_commitment(&self) -> &RuntimeApplyControlCommitment {
        self.envelope.control_commitment()
    }

    #[must_use]
    pub const fn operation_id(&self) -> ApplyOperationId {
        self.envelope.control_commitment().control().operation_id()
    }

    #[must_use]
    pub const fn temporal(&self) -> ApplyTemporalConstraint {
        self.envelope.temporal()
    }

    #[must_use]
    pub const fn expected_runtime_store_instance_id(&self) -> [u8; 32] {
        *self
            .envelope
            .expected_runtime_store_instance_id()
            .as_bytes()
    }

    #[must_use]
    pub const fn authentication(&self) -> &ApplyRequestAuthentication {
        self.envelope.authentication()
    }

    #[must_use]
    pub const fn envelope_request_digest(&self) -> Digest32 {
        self.envelope.request_digest()
    }

    /// Domain-separated digest of the complete PXAR v10 bytes.
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentDataPlaneApplySigningTranscriptV2, RemoteAgentDataPlanePlanError> {
        Ok(RemoteAgentDataPlaneApplySigningTranscriptV2(
            self.envelope.signing_transcript()?,
        ))
    }

    pub fn validate_expected_store(
        &self,
        local_runtime_store_instance_id: [u8; 32],
    ) -> Result<(), RemoteAgentDataPlanePlanError> {
        let local = RuntimeStoreInstanceId::try_from_bytes(local_runtime_store_instance_id)?;
        self.envelope.validate_expected_store(local)?;
        Ok(())
    }

    pub fn validate_projection(
        &self,
        projection: &RemoteAgentDataPlaneProjectionV1,
    ) -> Result<(), RemoteAgentDataPlanePlanError> {
        if self.target_execution().projection() != projection {
            return Err(RemoteAgentDataPlanePlanError::ProjectionMismatch);
        }
        Ok(())
    }
}

/// Reconstructs one durable `PXTA-zero || PXTE-v9` value from journal authority.
pub fn verify_remote_agent_data_plane_durable_slice_v1(
    canonical_slice_wire: &[u8],
    target: RuntimeHostId,
    provenance: PlanProvenance,
    expected_target_slice_digest: TargetSliceDigest,
    projection: &RemoteAgentDataPlaneProjectionV1,
) -> Result<RemoteAgentDataPlaneTargetExecutionV1, RemoteAgentDataPlanePlanError> {
    if canonical_slice_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_PLAN_SLICE_BYTES {
        return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
    }
    if canonical_slice_wire.len() < EMPTY_PXTA.len() {
        return Err(RemoteAgentDataPlanePlanError::Truncated);
    }
    let (binding_frame, execution_frame) = canonical_slice_wire.split_at(EMPTY_PXTA.len());
    if binding_frame != EMPTY_PXTA {
        return Err(RemoteAgentDataPlanePlanError::BindingNotAllowed);
    }
    let bindings = TargetAssignments::decode(binding_frame)
        .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
    let execution = RemoteAgentDataPlaneTargetExecutionV1::decode(execution_frame)?;
    if execution.projection() != projection || execution.projection().target() != target {
        return Err(RemoteAgentDataPlanePlanError::ProjectionMismatch);
    }
    let assignments = RemoteAgentDataPlaneAssignmentsV1::try_new(bindings, execution)?;
    let commitment = RuntimeSliceCommitment::try_new(RuntimeSliceHeader::new(
        target,
        provenance,
        assignments.assignment_digest,
    ))?;
    if commitment.target_slice_digest() != expected_target_slice_digest {
        return Err(RemoteAgentDataPlanePlanError::CommitmentMismatch);
    }
    let slice = RemoteAgentDataPlanePlanSliceV1::try_new(commitment, assignments)?;
    Ok(slice.assignments.execution)
}

/// Computes the exact compatibility fingerprint embedded in PXAE v1.
pub fn remote_agent_data_plane_compatibility_digest_v1() -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(COMPATIBILITY_DIGEST_DOMAIN)?;
    builder.field_bytes(PROJECTION_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROJECTION_VERSION)?;
    builder.field_u16(PROJECTION_BYTES as u16)?;
    builder.field_bytes(APPLY_REQUEST_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_VERSION)?;
    builder.field_u16(APPLY_REQUEST_HEADER_BYTES as u16)?;
    builder.field_bytes(&(MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES as u32).to_be_bytes())?;
    builder.field_bytes(TARGET_EXECUTION_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_VERSION)?;
    builder
        .field_bytes(&(MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_BYTES as u32).to_be_bytes())?;
    builder.field_bytes(PROFILE_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION)?;
    builder.field_u16(ASYMMETRIC_LISTENER_CONNECTOR_PROFILE_KIND)?;
    builder.field_u16(ASYMMETRIC_AGENT_ACL_PROFILE_VERSION)?;
    builder.field_bytes(&EMPTY_PXTA)?;
    builder.field_u16(RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate as u16)?;
    builder.field_bytes(TARGET_EXECUTION_DIGEST_DOMAIN)?;
    builder.field_bytes(TARGET_ASSIGNMENTS_DIGEST_DOMAIN)?;
    builder.field_bytes(APPLY_REQUEST_DIGEST_DOMAIN)?;
    builder.field_bytes(TERMINAL_RECEIPT_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_VERSION)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNING_VERSION)?;
    builder.field_bytes(&(TERMINAL_FIXED_BYTES as u32).to_be_bytes())?;
    builder.field_u16(MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined as u16)?;
    builder.field_u16(RemoteAgentDataPlaneRemoteObservationV1::Unknown as u16)?;
    builder.field_u16(RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent as u16)?;
    builder.field_u16(RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady as u16)?;
    builder.field_u16(RemoteAgentDataPlaneRemoteObservationV1::PartialOrConflicting as u16)?;
    builder.field_u16(RETAINED_LOCAL_AGENT_BINDING_CENSUS)?;
    builder.field_bytes(TERMINAL_SIGNING_MAGIC)?;
    builder.field_bytes(TERMINAL_DIGEST_DOMAIN)?;
    Ok(builder.finish())
}

/// Runtime terminal classification for one exact PXAR v10 operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RemoteAgentDataPlaneTerminalOutcomeV1 {
    /// Remote listener and exact Mac client ACL are ready over retained local T1.
    ActiveReady = 1,
    /// Remote listener/ACL are absent while retained local T1 remains ready.
    LocalOnlyReady = 2,
    /// Runtime proved lifecycle work did not begin.
    NoEffectRejected = 3,
    /// Runtime cannot prove whether lifecycle work began or completed.
    Uncertain = 4,
    /// Runtime isolated ambiguous resources and denies readiness.
    Quarantined = 5,
}

/// Strongest lifecycle-effect claim made by one PXAU terminal.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RemoteAgentDataPlaneTerminalLifecycleEffectV1 {
    ProvenNotStarted = 1,
    MayHaveStarted = 2,
}

/// Runtime-observed desired head after the exact operation completed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RemoteAgentDataPlaneTerminalHeadV1 {
    PreservedNone,
    PreservedExisting(TargetSliceDigest),
    CommittedIncoming,
}

/// Derived nonzero identity of one PXAU result.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RemoteAgentDataPlaneTerminalResultRefV1([u8; 16]);

impl RemoteAgentDataPlaneTerminalResultRefV1 {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Lifecycle and desired-head facts for one terminal outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalStateV1 {
    outcome: RemoteAgentDataPlaneTerminalOutcomeV1,
    lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV1,
    head: RemoteAgentDataPlaneTerminalHeadV1,
    fabric_generation: Option<ManagedServiceGeneration>,
    agent_generation: Option<ManagedServiceGeneration>,
    access_generation: Option<ManagedServiceGeneration>,
}

impl RemoteAgentDataPlaneTerminalStateV1 {
    pub fn try_new(
        outcome: RemoteAgentDataPlaneTerminalOutcomeV1,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV1,
        head: RemoteAgentDataPlaneTerminalHeadV1,
        fabric_generation: Option<ManagedServiceGeneration>,
        agent_generation: Option<ManagedServiceGeneration>,
        access_generation: Option<ManagedServiceGeneration>,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if agent_generation.is_some() && fabric_generation.is_none()
            || access_generation.is_some()
                && (fabric_generation.is_none() || agent_generation.is_none())
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
        }
        let state = Self {
            outcome,
            lifecycle_effect,
            head,
            fabric_generation,
            agent_generation,
            access_generation,
        };
        validate_terminal_state(state)?;
        Ok(state)
    }

    #[must_use]
    pub const fn outcome(self) -> RemoteAgentDataPlaneTerminalOutcomeV1 {
        self.outcome
    }

    #[must_use]
    pub const fn lifecycle_effect(self) -> RemoteAgentDataPlaneTerminalLifecycleEffectV1 {
        self.lifecycle_effect
    }

    #[must_use]
    pub const fn head(self) -> RemoteAgentDataPlaneTerminalHeadV1 {
        self.head
    }

    #[must_use]
    pub const fn fabric_generation(self) -> Option<ManagedServiceGeneration> {
        self.fabric_generation
    }

    #[must_use]
    pub const fn agent_generation(self) -> Option<ManagedServiceGeneration> {
        self.agent_generation
    }

    #[must_use]
    pub const fn access_generation(self) -> Option<ManagedServiceGeneration> {
        self.access_generation
    }
}

/// Explicit readiness observation for the remote-only listener and Mac ACL.
///
/// `Unknown` is a first-class value so an Uncertain terminal cannot encode
/// missing observation as a false claim that the listener or ACL is absent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RemoteAgentDataPlaneRemoteObservationV1 {
    Unknown = 1,
    RemoteAbsent = 2,
    ListenerAndClientAclReady = 3,
    PartialOrConflicting = 4,
}

/// Runtime observations covered by PXAU and independently checked by outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalEvidenceFieldsV1 {
    /// Exact retained local Agent submit/control binding census when complete.
    pub physical_binding_census: u16,
    pub census_complete: bool,
    pub base_fabric_ready: bool,
    pub base_agent_ready: bool,
    /// Explicit remote-only observation; `Unknown` is not equivalent to absent.
    pub remote_observation: RemoteAgentDataPlaneRemoteObservationV1,
    pub quarantined: bool,
    /// Exact whole PXAH receipt digest echoed from the incoming active CAS;
    /// this is independent of either PXAP payload digest below.
    pub echoed_bootstrap_descriptor_receipt_digest: Digest32,
    /// Exact incoming PXAP digest from
    /// [`crate::managed_serving_bootstrap::runtime_agent_control_descriptor_payload_digest_v1`].
    pub echoed_bootstrap_descriptor_payload_digest: Digest32,
    /// Fresh current PXAP digest from the same shared T1 helper after
    /// successful reassembly.
    pub fresh_current_descriptor_payload_digest: Digest32,
    pub resource_census_digest: Digest32,
    pub raw_outcome_digest: Digest32,
    pub completion_runtime_host_epoch: u64,
    pub completion_snapshot_sequence: u64,
    pub selection_clock_domain: ClockDomainRef,
    pub selection_clock_generation: ClockGeneration,
    pub selection_observed_at_nanos: u64,
}

/// Structurally valid bounded terminal observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalEvidenceV1 {
    fields: RemoteAgentDataPlaneTerminalEvidenceFieldsV1,
}

impl RemoteAgentDataPlaneTerminalEvidenceV1 {
    pub fn try_new(
        fields: RemoteAgentDataPlaneTerminalEvidenceFieldsV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if digest_is_zero(fields.resource_census_digest)
            || digest_is_zero(fields.raw_outcome_digest)
            || fields.completion_runtime_host_epoch == 0
            || fields.completion_snapshot_sequence == 0
            || bytes_are_zero(fields.selection_clock_domain.as_bytes())
            || fields.selection_observed_at_nanos == 0
            || fields.physical_binding_census > RETAINED_LOCAL_AGENT_BINDING_CENSUS
            || fields.base_agent_ready && !fields.base_fabric_ready
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
        }
        Ok(Self { fields })
    }

    #[must_use]
    pub const fn fields(self) -> RemoteAgentDataPlaneTerminalEvidenceFieldsV1 {
        self.fields
    }
}

/// Complete request-correlated facts carried by one PXAU.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalFactsV1 {
    target: RuntimeHostId,
    runtime_store_instance_id: [u8; 32],
    source_scope: SourceScopeRef,
    operation_id: ApplyOperationId,
    envelope_request_digest: Digest32,
    request_digest: Digest32,
    target_slice_digest: TargetSliceDigest,
    assignment_digest: TargetAssignmentDigest,
    terminal_result_ref: RemoteAgentDataPlaneTerminalResultRefV1,
    request_mode: RemoteAgentDataPlaneTargetModeV1,
    state: RemoteAgentDataPlaneTerminalStateV1,
    desired_head_digest: Option<TargetSliceDigest>,
    evidence: RemoteAgentDataPlaneTerminalEvidenceV1,
}

impl RemoteAgentDataPlaneTerminalFactsV1 {
    pub fn try_new(
        request: &RemoteAgentDataPlaneApplyRequestV1,
        state: RemoteAgentDataPlaneTerminalStateV1,
        evidence: RemoteAgentDataPlaneTerminalEvidenceV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let evidence_fields = evidence.fields();
        if evidence_fields.selection_clock_domain != request.temporal().target_clock_domain()
            || evidence_fields.selection_clock_generation.value()
                < request.temporal().target_clock_generation().value()
        {
            return Err(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch);
        }
        let desired_head_digest = resolve_terminal_head(request, state.head())?;
        let facts = Self {
            target: request.target(),
            runtime_store_instance_id: request.expected_runtime_store_instance_id(),
            source_scope: request.provenance().source_scope(),
            operation_id: request.operation_id(),
            envelope_request_digest: request.envelope_request_digest(),
            request_digest: request.request_digest(),
            target_slice_digest: request.target_slice_digest(),
            assignment_digest: request.assignment_digest(),
            terminal_result_ref: derive_terminal_result_ref(request)?,
            request_mode: request.target_execution().mode(),
            state,
            desired_head_digest,
            evidence,
        };
        validate_terminal_facts_shape(&facts)?;
        validate_terminal_facts_against_execution(&facts, request.target_execution())?;
        Ok(facts)
    }

    #[must_use]
    pub const fn target(self) -> RuntimeHostId {
        self.target
    }

    #[must_use]
    pub const fn runtime_store_instance_id(self) -> [u8; 32] {
        self.runtime_store_instance_id
    }

    #[must_use]
    pub const fn source_scope(self) -> SourceScopeRef {
        self.source_scope
    }

    #[must_use]
    pub const fn operation_id(self) -> ApplyOperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn envelope_request_digest(self) -> Digest32 {
        self.envelope_request_digest
    }

    #[must_use]
    pub const fn request_digest(self) -> Digest32 {
        self.request_digest
    }

    #[must_use]
    pub const fn target_slice_digest(self) -> TargetSliceDigest {
        self.target_slice_digest
    }

    #[must_use]
    pub const fn assignment_digest(self) -> TargetAssignmentDigest {
        self.assignment_digest
    }

    #[must_use]
    pub const fn terminal_result_ref(self) -> RemoteAgentDataPlaneTerminalResultRefV1 {
        self.terminal_result_ref
    }

    #[must_use]
    pub const fn request_mode(self) -> RemoteAgentDataPlaneTargetModeV1 {
        self.request_mode
    }

    #[must_use]
    pub const fn state(self) -> RemoteAgentDataPlaneTerminalStateV1 {
        self.state
    }

    #[must_use]
    pub const fn desired_head_digest(self) -> Option<TargetSliceDigest> {
        self.desired_head_digest
    }

    #[must_use]
    pub const fn evidence(self) -> RemoteAgentDataPlaneTerminalEvidenceV1 {
        self.evidence
    }
}

/// Runtime signer selected by target/store/epoch authority outside this codec.
///
/// This claim intentionally carries neither PXCB nor the Runtime-local
/// `ReferenceChannelBindingV1`. PXRR later authenticates its own public carrier
/// independently and must not substitute its signature for this one.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalAuthClaimV1 {
    runtime_principal: PrincipalRef,
    key: ApplyAuthKeyRef,
    algorithm: ApplyAuthAlgorithm,
    algorithm_version: u16,
}

impl RemoteAgentDataPlaneTerminalAuthClaimV1 {
    pub fn try_new(
        runtime_principal: PrincipalRef,
        key: ApplyAuthKeyRef,
        algorithm: ApplyAuthAlgorithm,
        algorithm_version: u16,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if bytes_are_zero(runtime_principal.as_bytes())
            || bytes_are_zero(key.as_bytes())
            || algorithm_version == 0
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidResponseAuthentication);
        }
        Ok(Self {
            runtime_principal,
            key,
            algorithm,
            algorithm_version,
        })
    }

    #[must_use]
    pub const fn runtime_principal(self) -> PrincipalRef {
        self.runtime_principal
    }

    #[must_use]
    pub const fn key(self) -> ApplyAuthKeyRef {
        self.key
    }

    #[must_use]
    pub const fn algorithm(self) -> ApplyAuthAlgorithm {
        self.algorithm
    }

    #[must_use]
    pub const fn algorithm_version(self) -> u16 {
        self.algorithm_version
    }
}

/// Exact Runtime signing bytes for PXAU.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalSigningTranscriptV1(Box<[u8]>);

impl RemoteAgentDataPlaneTerminalSigningTranscriptV1 {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Signature-independent Runtime producer for one PXAU terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalReceiptDraftV1 {
    facts: RemoteAgentDataPlaneTerminalFactsV1,
    auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV1,
}

impl RemoteAgentDataPlaneTerminalReceiptDraftV1 {
    pub fn try_new(
        request: &RemoteAgentDataPlaneApplyRequestV1,
        state: RemoteAgentDataPlaneTerminalStateV1,
        evidence: RemoteAgentDataPlaneTerminalEvidenceV1,
        auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let facts = RemoteAgentDataPlaneTerminalFactsV1::try_new(request, state, evidence)?;
        Ok(Self { facts, auth_claim })
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentDataPlaneTerminalSigningTranscriptV1, RemoteAgentDataPlanePlanError>
    {
        let mut wire = Vec::new();
        wire.extend_from_slice(TERMINAL_SIGNING_MAGIC);
        wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNING_VERSION.to_be_bytes());
        append_terminal_body(&mut wire, self.facts, self.auth_claim);
        Ok(RemoteAgentDataPlaneTerminalSigningTranscriptV1(
            wire.into_boxed_slice(),
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentDataPlaneTerminalReceiptV1, RemoteAgentDataPlanePlanError> {
        if signature.is_empty()
            || signature.len() > MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidResponseAuthentication);
        }
        RemoteAgentDataPlaneTerminalReceiptV1::try_new(self.facts, self.auth_claim, signature)
    }
}

/// Strict independently Runtime-signed PXAU terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTerminalReceiptV1 {
    facts: RemoteAgentDataPlaneTerminalFactsV1,
    auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV1,
    signature: Box<[u8]>,
    canonical_wire: Box<[u8]>,
    receipt_digest: Digest32,
}

impl RemoteAgentDataPlaneTerminalReceiptV1 {
    fn try_new(
        facts: RemoteAgentDataPlaneTerminalFactsV1,
        auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV1,
        signature: &[u8],
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        validate_terminal_facts_shape(&facts)?;
        if signature.is_empty()
            || signature.len() > MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidResponseAuthentication);
        }
        let signature_length = u16::try_from(signature.len())
            .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
        let mut canonical_wire = Vec::new();
        canonical_wire.extend_from_slice(TERMINAL_RECEIPT_MAGIC);
        canonical_wire
            .extend_from_slice(&REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_VERSION.to_be_bytes());
        append_terminal_body(&mut canonical_wire, facts, auth_claim);
        canonical_wire.extend_from_slice(&signature_length.to_be_bytes());
        canonical_wire.extend_from_slice(signature);
        if canonical_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        let receipt_digest = digest_wire(TERMINAL_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            facts,
            auth_claim,
            signature: signature.into(),
            canonical_wire: canonical_wire.into_boxed_slice(),
            receipt_digest,
        })
    }

    /// Strictly decodes PXAU v1; PXAT* tenure frames and old terminals fail closed.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() > MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        if frame.len() < TERMINAL_FIXED_BYTES {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.take(4)? != TERMINAL_RECEIPT_MAGIC
            || cursor.u16()? != REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let facts = decode_terminal_facts(&mut cursor)?;
        let auth_claim = decode_terminal_auth_claim(&mut cursor)?;
        let signature_length = cursor.usize_u16()?;
        if signature_length == 0
            || signature_length > MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidLength);
        }
        let signature = cursor.take(signature_length)?;
        cursor.finish()?;
        let decoded = Self::try_new(facts, auth_claim, signature)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    pub fn validate_against_request(
        &self,
        request: &RemoteAgentDataPlaneApplyRequestV1,
    ) -> Result<RemoteAgentDataPlaneTerminalFactsV1, RemoteAgentDataPlanePlanError> {
        let expected = RemoteAgentDataPlaneTerminalFactsV1::try_new(
            request,
            self.facts.state(),
            self.facts.evidence(),
        )?;
        if self.facts != expected {
            return Err(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch);
        }
        Ok(self.facts)
    }

    /// Verifies exact request correlation, signer selection, then PXAU signature.
    pub fn verify_runtime_terminal<'a, Verify>(
        &'a self,
        request: &RemoteAgentDataPlaneApplyRequestV1,
        expected_auth_claim: RemoteAgentDataPlaneTerminalAuthClaimV1,
        verify: Verify,
    ) -> Result<RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1<'a>, RemoteAgentDataPlanePlanError>
    where
        Verify:
            FnOnce(PrincipalRef, ApplyAuthKeyRef, ApplyAuthAlgorithm, u16, &[u8], &[u8]) -> bool,
    {
        self.validate_against_request(request)?;
        if self.auth_claim != expected_auth_claim {
            return Err(RemoteAgentDataPlanePlanError::InvalidResponseAuthentication);
        }
        let transcript = self.signing_transcript()?;
        if !verify(
            self.auth_claim.runtime_principal(),
            self.auth_claim.key(),
            self.auth_claim.algorithm(),
            self.auth_claim.algorithm_version(),
            transcript.as_bytes(),
            &self.signature,
        ) {
            return Err(RemoteAgentDataPlanePlanError::InvalidResponseAuthentication);
        }
        Ok(RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1 { receipt: self })
    }

    #[must_use]
    pub const fn facts(&self) -> RemoteAgentDataPlaneTerminalFactsV1 {
        self.facts
    }

    #[must_use]
    pub const fn authentication(&self) -> RemoteAgentDataPlaneTerminalAuthClaimV1 {
        self.auth_claim
    }

    #[must_use]
    pub fn authentication_signature(&self) -> &[u8] {
        &self.signature
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> Digest32 {
        self.receipt_digest
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentDataPlaneTerminalSigningTranscriptV1, RemoteAgentDataPlanePlanError>
    {
        RemoteAgentDataPlaneTerminalReceiptDraftV1 {
            facts: self.facts,
            auth_claim: self.auth_claim,
        }
        .signing_transcript()
    }
}

/// Marker issued only after PXAU correlation, signer selection, and signature checks.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1<'a> {
    receipt: &'a RemoteAgentDataPlaneTerminalReceiptV1,
}

impl<'a> RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1<'a> {
    #[must_use]
    pub const fn receipt(self) -> &'a RemoteAgentDataPlaneTerminalReceiptV1 {
        self.receipt
    }
}

fn validate_terminal_state(
    state: RemoteAgentDataPlaneTerminalStateV1,
) -> Result<(), RemoteAgentDataPlanePlanError> {
    use RemoteAgentDataPlaneTerminalLifecycleEffectV1::{MayHaveStarted, ProvenNotStarted};
    use RemoteAgentDataPlaneTerminalOutcomeV1::{
        ActiveReady, LocalOnlyReady, NoEffectRejected, Quarantined, Uncertain,
    };
    let valid = match state.outcome() {
        ActiveReady => {
            state.lifecycle_effect() == MayHaveStarted
                && matches!(
                    state.head(),
                    RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming
                )
                && state.fabric_generation().is_some()
                && state.agent_generation().is_some()
                && state.access_generation().is_some()
        }
        LocalOnlyReady => {
            state.lifecycle_effect() == MayHaveStarted
                && matches!(
                    state.head(),
                    RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming
                )
                && state.fabric_generation().is_some()
                && state.agent_generation().is_some()
                && state.access_generation().is_none()
        }
        NoEffectRejected => {
            state.lifecycle_effect() == ProvenNotStarted
                && !matches!(
                    state.head(),
                    RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming
                )
        }
        Uncertain => state.lifecycle_effect() == MayHaveStarted,
        Quarantined => state.lifecycle_effect() == MayHaveStarted,
    };
    if !valid {
        return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
    }
    Ok(())
}

fn validate_terminal_facts_shape(
    facts: &RemoteAgentDataPlaneTerminalFactsV1,
) -> Result<(), RemoteAgentDataPlanePlanError> {
    if bytes_are_zero(facts.target.as_bytes())
        || bytes_are_zero(&facts.runtime_store_instance_id)
        || bytes_are_zero(facts.source_scope.as_bytes())
        || bytes_are_zero(facts.operation_id.as_bytes())
        || digest_is_zero(facts.envelope_request_digest)
        || digest_is_zero(facts.request_digest)
        || digest_is_zero(*facts.target_slice_digest.value())
        || digest_is_zero(*facts.assignment_digest.value())
        || bytes_are_zero(facts.terminal_result_ref.as_bytes())
    {
        return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
    }
    match (facts.state.head(), facts.desired_head_digest) {
        (RemoteAgentDataPlaneTerminalHeadV1::PreservedNone, None) => {}
        (RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(expected), Some(actual))
            if expected == actual && !digest_is_zero(*actual.value()) => {}
        (RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming, Some(actual))
            if actual == facts.target_slice_digest => {}
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    }
    validate_terminal_state(facts.state)?;
    let fields = facts.evidence.fields();
    if fields.base_fabric_ready && facts.state.fabric_generation().is_none()
        || fields.base_agent_ready && facts.state.agent_generation().is_none()
    {
        return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
    }
    use RemoteAgentDataPlaneTerminalOutcomeV1::{
        ActiveReady, LocalOnlyReady, NoEffectRejected, Quarantined, Uncertain,
    };
    let echoed_bootstrap_roots_zero =
        digest_is_zero(fields.echoed_bootstrap_descriptor_receipt_digest)
            && digest_is_zero(fields.echoed_bootstrap_descriptor_payload_digest);
    let fresh_current_descriptor_zero =
        digest_is_zero(fields.fresh_current_descriptor_payload_digest);
    let valid = match facts.state.outcome() {
        ActiveReady => {
            facts.request_mode == RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive
                && fields.census_complete
                && fields.physical_binding_census == RETAINED_LOCAL_AGENT_BINDING_CENSUS
                && fields.base_fabric_ready
                && fields.base_agent_ready
                && fields.remote_observation
                    == RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady
                && !fields.quarantined
                && !digest_is_zero(fields.echoed_bootstrap_descriptor_receipt_digest)
                && !digest_is_zero(fields.echoed_bootstrap_descriptor_payload_digest)
                && !fresh_current_descriptor_zero
                && fields.fresh_current_descriptor_payload_digest
                    != fields.echoed_bootstrap_descriptor_payload_digest
        }
        LocalOnlyReady => {
            facts.request_mode == RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate
                && fields.census_complete
                && fields.physical_binding_census == RETAINED_LOCAL_AGENT_BINDING_CENSUS
                && fields.base_fabric_ready
                && fields.base_agent_ready
                && fields.remote_observation
                    == RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent
                && !fields.quarantined
                && echoed_bootstrap_roots_zero
                && fresh_current_descriptor_zero
        }
        NoEffectRejected => {
            !fields.quarantined
                && match fields.remote_observation {
                    RemoteAgentDataPlaneRemoteObservationV1::Unknown => {
                        fresh_current_descriptor_zero
                    }
                    RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent => {
                        fields.census_complete
                            && fields.physical_binding_census == RETAINED_LOCAL_AGENT_BINDING_CENSUS
                            && fields.base_fabric_ready
                            && fields.base_agent_ready
                            && fresh_current_descriptor_zero
                            && facts.state.access_generation().is_none()
                    }
                    RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady => {
                        matches!(
                            facts.state.head(),
                            RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(_)
                        ) && fields.census_complete
                            && fields.physical_binding_census == RETAINED_LOCAL_AGENT_BINDING_CENSUS
                            && fields.base_fabric_ready
                            && fields.base_agent_ready
                            && !fresh_current_descriptor_zero
                            && facts.state.access_generation().is_some()
                    }
                    RemoteAgentDataPlaneRemoteObservationV1::PartialOrConflicting => false,
                }
        }
        Uncertain => {
            !fields.quarantined
                && matches!(
                    fields.remote_observation,
                    RemoteAgentDataPlaneRemoteObservationV1::Unknown
                        | RemoteAgentDataPlaneRemoteObservationV1::PartialOrConflicting
                )
                && fresh_current_descriptor_zero
        }
        Quarantined => {
            fields.quarantined
                && matches!(
                    fields.remote_observation,
                    RemoteAgentDataPlaneRemoteObservationV1::Unknown
                        | RemoteAgentDataPlaneRemoteObservationV1::PartialOrConflicting
                )
                && fresh_current_descriptor_zero
        }
    };
    if !valid {
        return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
    }
    Ok(())
}

fn validate_terminal_facts_against_execution(
    facts: &RemoteAgentDataPlaneTerminalFactsV1,
    execution: &RemoteAgentDataPlaneTargetExecutionV1,
) -> Result<(), RemoteAgentDataPlanePlanError> {
    if facts.request_mode != execution.mode() {
        return Err(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch);
    }
    let fields = facts.evidence.fields();
    match (execution.mode(), execution.bootstrap_cas()) {
        (RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive, Some(cas)) => {
            if fields.echoed_bootstrap_descriptor_receipt_digest
                != cas.expected_bootstrap_descriptor_receipt_digest()
                || fields.echoed_bootstrap_descriptor_payload_digest
                    != cas.expected_bootstrap_descriptor_payload_digest()
            {
                return Err(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch);
            }
            if facts.state.outcome() == RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady {
                let fabric_generation = facts
                    .state
                    .fabric_generation()
                    .ok_or(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch)?;
                let agent_generation = facts
                    .state
                    .agent_generation()
                    .ok_or(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch)?;
                if fabric_generation.value() <= cas.expected_fabric_generation().value()
                    || agent_generation.value() <= cas.expected_agent_generation().value()
                    || fields.fresh_current_descriptor_payload_digest
                        == cas.expected_bootstrap_descriptor_payload_digest()
                {
                    return Err(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch);
                }
            }
        }
        (RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate, None)
            if digest_is_zero(fields.echoed_bootstrap_descriptor_receipt_digest)
                && digest_is_zero(fields.echoed_bootstrap_descriptor_payload_digest) => {}
        _ => return Err(RemoteAgentDataPlanePlanError::TerminalCorrelationMismatch),
    }
    Ok(())
}

fn resolve_terminal_head(
    request: &RemoteAgentDataPlaneApplyRequestV1,
    head: RemoteAgentDataPlaneTerminalHeadV1,
) -> Result<Option<TargetSliceDigest>, RemoteAgentDataPlanePlanError> {
    match head {
        RemoteAgentDataPlaneTerminalHeadV1::PreservedNone => Ok(None),
        RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(value)
            if !digest_is_zero(*value.value()) =>
        {
            Ok(Some(value))
        }
        RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(_) => {
            Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts)
        }
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming => {
            Ok(Some(request.target_slice_digest()))
        }
    }
}

fn derive_terminal_result_ref(
    request: &RemoteAgentDataPlaneApplyRequestV1,
) -> Result<RemoteAgentDataPlaneTerminalResultRefV1, RemoteAgentDataPlanePlanError> {
    let mut builder = Digest32Builder::try_new(TERMINAL_RESULT_REF_DOMAIN)?;
    builder.field_bytes(TERMINAL_RECEIPT_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_VERSION)?;
    builder.field_bytes(request.target().as_bytes())?;
    builder.field_bytes(&request.expected_runtime_store_instance_id())?;
    builder.field_bytes(request.provenance().source_scope().as_bytes())?;
    builder.field_bytes(request.operation_id().as_bytes())?;
    builder.field_digest(&request.envelope_request_digest())?;
    builder.field_digest(&request.request_digest())?;
    let digest = builder.finish();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    if bytes_are_zero(&bytes) {
        return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts);
    }
    Ok(RemoteAgentDataPlaneTerminalResultRefV1(bytes))
}

fn append_terminal_body(
    wire: &mut Vec<u8>,
    facts: RemoteAgentDataPlaneTerminalFactsV1,
    auth: RemoteAgentDataPlaneTerminalAuthClaimV1,
) {
    wire.extend_from_slice(facts.target().as_bytes());
    wire.extend_from_slice(&facts.runtime_store_instance_id());
    wire.extend_from_slice(facts.source_scope().as_bytes());
    wire.extend_from_slice(facts.operation_id().as_bytes());
    wire.extend_from_slice(facts.envelope_request_digest().as_bytes());
    wire.extend_from_slice(facts.request_digest().as_bytes());
    wire.extend_from_slice(facts.target_slice_digest().value().as_bytes());
    wire.extend_from_slice(facts.assignment_digest().value().as_bytes());
    wire.extend_from_slice(facts.terminal_result_ref().as_bytes());
    wire.push(facts.request_mode() as u8);
    wire.push(facts.state().outcome() as u8);
    wire.push(facts.state().lifecycle_effect() as u8);
    wire.push(match facts.state().head() {
        RemoteAgentDataPlaneTerminalHeadV1::PreservedNone => 1,
        RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(_) => 2,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming => 3,
    });
    wire.push(u8::from(facts.desired_head_digest().is_some()));
    wire.push(0);
    wire.extend_from_slice(
        &facts
            .desired_head_digest()
            .map_or([0; 32], |value| *value.value().as_bytes()),
    );
    encode_generation(wire, facts.state().fabric_generation());
    encode_generation(wire, facts.state().agent_generation());
    encode_generation(wire, facts.state().access_generation());
    let evidence = facts.evidence().fields();
    wire.extend_from_slice(&evidence.physical_binding_census.to_be_bytes());
    wire.push(terminal_evidence_flags(evidence));
    wire.push(evidence.remote_observation as u8);
    wire.extend_from_slice(
        evidence
            .echoed_bootstrap_descriptor_receipt_digest
            .as_bytes(),
    );
    wire.extend_from_slice(
        evidence
            .echoed_bootstrap_descriptor_payload_digest
            .as_bytes(),
    );
    wire.extend_from_slice(evidence.fresh_current_descriptor_payload_digest.as_bytes());
    wire.extend_from_slice(evidence.resource_census_digest.as_bytes());
    wire.extend_from_slice(evidence.raw_outcome_digest.as_bytes());
    wire.extend_from_slice(&evidence.completion_runtime_host_epoch.to_be_bytes());
    wire.extend_from_slice(&evidence.completion_snapshot_sequence.to_be_bytes());
    wire.extend_from_slice(evidence.selection_clock_domain.as_bytes());
    wire.extend_from_slice(&evidence.selection_clock_generation.value().to_be_bytes());
    wire.extend_from_slice(&evidence.selection_observed_at_nanos.to_be_bytes());
    wire.extend_from_slice(auth.runtime_principal().as_bytes());
    wire.extend_from_slice(auth.key().as_bytes());
    wire.extend_from_slice(&auth.algorithm().value().to_be_bytes());
    wire.extend_from_slice(&auth.algorithm_version().to_be_bytes());
}

fn terminal_evidence_flags(fields: RemoteAgentDataPlaneTerminalEvidenceFieldsV1) -> u8 {
    u8::from(fields.census_complete)
        | (u8::from(fields.base_fabric_ready) << 1)
        | (u8::from(fields.base_agent_ready) << 2)
        | (u8::from(fields.quarantined) << 3)
}

fn encode_generation(wire: &mut Vec<u8>, generation: Option<ManagedServiceGeneration>) {
    wire.push(u8::from(generation.is_some()));
    wire.extend_from_slice(
        &generation
            .map_or(0, ManagedServiceGeneration::value)
            .to_be_bytes(),
    );
}

fn decode_generation(
    cursor: &mut Cursor<'_>,
) -> Result<Option<ManagedServiceGeneration>, RemoteAgentDataPlanePlanError> {
    match (cursor.u8()?, cursor.u64()?) {
        (0, 0) => Ok(None),
        (1, value) => ManagedServiceGeneration::try_new(value)
            .map(Some)
            .map_err(|_| RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
        _ => Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    }
}

fn decode_terminal_facts(
    cursor: &mut Cursor<'_>,
) -> Result<RemoteAgentDataPlaneTerminalFactsV1, RemoteAgentDataPlanePlanError> {
    let target = RuntimeHostId::from_bytes(cursor.array()?);
    let runtime_store_instance_id = cursor.array()?;
    let source_scope = SourceScopeRef::from_bytes(cursor.array()?);
    let operation_id = ApplyOperationId::from_bytes(cursor.array()?);
    let envelope_request_digest = Digest32::from_bytes(cursor.array()?);
    let request_digest = Digest32::from_bytes(cursor.array()?);
    let target_slice_digest = TargetSliceDigest::new(Digest32::from_bytes(cursor.array()?));
    let assignment_digest = TargetAssignmentDigest::new(Digest32::from_bytes(cursor.array()?));
    let terminal_result_ref = RemoteAgentDataPlaneTerminalResultRefV1(cursor.array()?);
    let request_mode = match cursor.u8()? {
        1 => RemoteAgentDataPlaneTargetModeV1::RemoteAccessActive,
        2 => RemoteAgentDataPlaneTargetModeV1::LocalAgentOnlyDeactivate,
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    };
    let outcome = match cursor.u8()? {
        1 => RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
        2 => RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
        3 => RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        4 => RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain,
        5 => RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined,
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    };
    let lifecycle_effect = match cursor.u8()? {
        1 => RemoteAgentDataPlaneTerminalLifecycleEffectV1::ProvenNotStarted,
        2 => RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    };
    let head_tag = cursor.u8()?;
    let desired_present = cursor.u8()?;
    if cursor.u8()? != 0 {
        return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
    }
    let desired_bytes: [u8; 32] = cursor.array()?;
    let desired_head_digest = match (desired_present, bytes_are_zero(&desired_bytes)) {
        (0, true) => None,
        (1, false) => Some(TargetSliceDigest::new(Digest32::from_bytes(desired_bytes))),
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    };
    let head = match (head_tag, desired_head_digest) {
        (1, None) => RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
        (2, Some(value)) => RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(value),
        (3, Some(_)) => RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    };
    let state = RemoteAgentDataPlaneTerminalStateV1::try_new(
        outcome,
        lifecycle_effect,
        head,
        decode_generation(cursor)?,
        decode_generation(cursor)?,
        decode_generation(cursor)?,
    )?;
    let physical_binding_census = cursor.u16()?;
    let flags = cursor.u8()?;
    if flags & 0b1111_0000 != 0 {
        return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
    }
    let remote_observation = match cursor.u8()? {
        1 => RemoteAgentDataPlaneRemoteObservationV1::Unknown,
        2 => RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent,
        3 => RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
        4 => RemoteAgentDataPlaneRemoteObservationV1::PartialOrConflicting,
        _ => return Err(RemoteAgentDataPlanePlanError::InvalidTerminalFacts),
    };
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(
        RemoteAgentDataPlaneTerminalEvidenceFieldsV1 {
            physical_binding_census,
            census_complete: flags & 1 != 0,
            base_fabric_ready: flags & 2 != 0,
            base_agent_ready: flags & 4 != 0,
            remote_observation,
            quarantined: flags & 8 != 0,
            echoed_bootstrap_descriptor_receipt_digest: Digest32::from_bytes(cursor.array()?),
            echoed_bootstrap_descriptor_payload_digest: Digest32::from_bytes(cursor.array()?),
            fresh_current_descriptor_payload_digest: Digest32::from_bytes(cursor.array()?),
            resource_census_digest: Digest32::from_bytes(cursor.array()?),
            raw_outcome_digest: Digest32::from_bytes(cursor.array()?),
            completion_runtime_host_epoch: cursor.u64()?,
            completion_snapshot_sequence: cursor.u64()?,
            selection_clock_domain: ClockDomainRef::from_bytes(cursor.array()?),
            selection_clock_generation: ClockGeneration::try_new(cursor.u64()?)
                .map_err(|_| RemoteAgentDataPlanePlanError::InvalidTerminalFacts)?,
            selection_observed_at_nanos: cursor.u64()?,
        },
    )?;
    let facts = RemoteAgentDataPlaneTerminalFactsV1 {
        target,
        runtime_store_instance_id,
        source_scope,
        operation_id,
        envelope_request_digest,
        request_digest,
        target_slice_digest,
        assignment_digest,
        terminal_result_ref,
        request_mode,
        state,
        desired_head_digest,
        evidence,
    };
    validate_terminal_facts_shape(&facts)?;
    Ok(facts)
}

fn decode_terminal_auth_claim(
    cursor: &mut Cursor<'_>,
) -> Result<RemoteAgentDataPlaneTerminalAuthClaimV1, RemoteAgentDataPlanePlanError> {
    let runtime_principal = PrincipalRef::from_bytes(cursor.array()?);
    let key = ApplyAuthKeyRef::from_bytes(cursor.array()?);
    let algorithm = ApplyAuthAlgorithm::try_new(cursor.u16()?)
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidResponseAuthentication)?;
    let algorithm_version = cursor.u16()?;
    RemoteAgentDataPlaneTerminalAuthClaimV1::try_new(
        runtime_principal,
        key,
        algorithm,
        algorithm_version,
    )
}

fn build_profile_wire(
    fields: &RemoteAgentDataPlaneProfileFieldsV1<'_>,
    base: &ManagedFabricListenEndpointV1,
    tls: &DistributedFabricTlsEndpointV1,
) -> Result<Vec<u8>, RemoteAgentDataPlanePlanError> {
    let base_length = u16::try_from(base.as_str().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let tls_length = u16::try_from(tls.as_str().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(PROFILE_MAGIC);
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION.to_be_bytes());
    wire.extend_from_slice(&ASYMMETRIC_LISTENER_CONNECTOR_PROFILE_KIND.to_be_bytes());
    wire.extend_from_slice(&base_length.to_be_bytes());
    wire.extend_from_slice(&tls_length.to_be_bytes());
    wire.extend_from_slice(&ASYMMETRIC_AGENT_ACL_PROFILE_VERSION.to_be_bytes());
    wire.extend_from_slice(fields.target.as_bytes());
    wire.extend_from_slice(&fields.endpoint_ref);
    wire.extend_from_slice(&fields.endpoint_generation.to_be_bytes());
    wire.extend_from_slice(fields.trust_domain_ref.as_bytes());
    wire.extend_from_slice(fields.trust_anchor_ref.as_bytes());
    wire.extend_from_slice(fields.mac_connector_credential_ref.as_bytes());
    wire.extend_from_slice(fields.ubuntu_listener_credential_ref.as_bytes());
    wire.extend_from_slice(fields.mac_agent_client_principal.as_bytes());
    wire.extend_from_slice(fields.ubuntu_agent_listener_principal.as_bytes());
    wire.extend_from_slice(&fields.operation_timeout_nanos.to_be_bytes());
    wire.extend_from_slice(base.as_str().as_bytes());
    wire.extend_from_slice(tls.as_str().as_bytes());
    Ok(wire)
}

fn build_projection_wire(
    predecessor: &ManagedAgentStackProjectionV1,
    compatibility_digest: Digest32,
) -> Vec<u8> {
    let mut wire = Vec::with_capacity(PROJECTION_BYTES);
    wire.extend_from_slice(PROJECTION_MAGIC);
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_PROJECTION_VERSION.to_be_bytes());
    wire.extend_from_slice(predecessor.canonical_wire());
    wire.extend_from_slice(compatibility_digest.as_bytes());
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_VERSION.to_be_bytes());
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION.to_be_bytes());
    wire
}

fn build_target_execution_wire(
    projection: &RemoteAgentDataPlaneProjectionV1,
    mode: RemoteAgentDataPlaneTargetModeV1,
    predecessor: &ManagedAgentStackTargetExecutionV1,
    bootstrap_cas: Option<RemoteAgentBootstrapCasV1>,
    profile: &RemoteAgentDataPlaneProfileV1,
) -> Result<Vec<u8>, RemoteAgentDataPlanePlanError> {
    let predecessor_length = u32::try_from(predecessor.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let profile_length = u32::try_from(profile.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(TARGET_EXECUTION_MAGIC);
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_VERSION.to_be_bytes());
    wire.extend_from_slice(projection.canonical_wire());
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION.to_be_bytes());
    wire.push(mode as u8);
    wire.push(1);
    wire.extend_from_slice(&predecessor_length.to_be_bytes());
    wire.extend_from_slice(&profile_length.to_be_bytes());
    wire.extend_from_slice(predecessor.canonical_wire());
    if let Some(cas) = bootstrap_cas {
        append_bootstrap_cas(&mut wire, cas);
    }
    wire.extend_from_slice(profile.canonical_wire());
    Ok(wire)
}

fn append_bootstrap_cas(wire: &mut Vec<u8>, cas: RemoteAgentBootstrapCasV1) {
    wire.extend_from_slice(cas.expected_active_pxft_digest().as_bytes());
    wire.extend_from_slice(cas.expected_active_pxst_digest().as_bytes());
    wire.extend_from_slice(
        cas.expected_bootstrap_descriptor_receipt_digest()
            .as_bytes(),
    );
    wire.extend_from_slice(
        cas.expected_bootstrap_descriptor_payload_digest()
            .as_bytes(),
    );
    wire.extend_from_slice(&cas.expected_fabric_generation().value().to_be_bytes());
    wire.extend_from_slice(&cas.expected_agent_generation().value().to_be_bytes());
}

fn decode_bootstrap_cas(
    cursor: &mut Cursor<'_>,
) -> Result<RemoteAgentBootstrapCasV1, RemoteAgentDataPlanePlanError> {
    let expected_active_pxft_digest = Digest32::from_bytes(cursor.array()?);
    let expected_active_pxst_digest = Digest32::from_bytes(cursor.array()?);
    let expected_bootstrap_descriptor_receipt_digest = Digest32::from_bytes(cursor.array()?);
    let expected_bootstrap_descriptor_payload_digest = Digest32::from_bytes(cursor.array()?);
    let expected_fabric_generation = ManagedServiceGeneration::try_new(cursor.u64()?)
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidBootstrapCas)?;
    let expected_agent_generation = ManagedServiceGeneration::try_new(cursor.u64()?)
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidBootstrapCas)?;
    RemoteAgentBootstrapCasV1::try_new(
        expected_active_pxft_digest,
        expected_active_pxst_digest,
        expected_bootstrap_descriptor_receipt_digest,
        expected_bootstrap_descriptor_payload_digest,
        expected_fabric_generation,
        expected_agent_generation,
    )
}

fn build_apply_request_wire(
    envelope: &RuntimeApplyEnvelopeV2,
    slice: &RemoteAgentDataPlanePlanSliceV1,
) -> Result<Vec<u8>, RemoteAgentDataPlanePlanError> {
    let envelope_length = u32::try_from(envelope.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let bindings_length = u32::try_from(slice.assignments.bindings.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let execution_length = u32::try_from(slice.assignments.execution.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(APPLY_REQUEST_MAGIC);
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_VERSION.to_be_bytes());
    wire.extend_from_slice(&envelope_length.to_be_bytes());
    wire.extend_from_slice(&bindings_length.to_be_bytes());
    wire.extend_from_slice(&execution_length.to_be_bytes());
    wire.extend_from_slice(envelope.canonical_wire());
    wire.extend_from_slice(slice.assignments.bindings.canonical_wire());
    wire.extend_from_slice(slice.assignments.execution.canonical_wire());
    Ok(wire)
}

fn digest_wire(domain: &[u8], wire: &[u8]) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(domain)?;
    builder.field_bytes(wire)?;
    Ok(builder.finish())
}

fn digest_is_zero(value: Digest32) -> bool {
    bytes_are_zero(value.as_bytes())
}

const fn bytes_are_zero<const N: usize>(bytes: &[u8; N]) -> bool {
    let mut index = 0;
    while index < N {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(read_array(bytes))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(read_array(bytes))
}

fn read_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    bytes.try_into().unwrap_or([0; N])
}

struct Cursor<'a> {
    frame: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(frame: &'a [u8]) -> Self {
        Self { frame, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RemoteAgentDataPlanePlanError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(RemoteAgentDataPlanePlanError::FrameTooLarge)?;
        let value = self
            .frame
            .get(self.position..end)
            .ok_or(RemoteAgentDataPlanePlanError::Truncated)?;
        self.position = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], RemoteAgentDataPlanePlanError> {
        self.take(N)?
            .try_into()
            .map_err(|_| RemoteAgentDataPlanePlanError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, RemoteAgentDataPlanePlanError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, RemoteAgentDataPlanePlanError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, RemoteAgentDataPlanePlanError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, RemoteAgentDataPlanePlanError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn usize_u16(&mut self) -> Result<usize, RemoteAgentDataPlanePlanError> {
        Ok(usize::from(self.u16()?))
    }

    fn usize_u32(&mut self) -> Result<usize, RemoteAgentDataPlanePlanError> {
        usize::try_from(self.u32()?).map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)
    }

    fn finish(self) -> Result<(), RemoteAgentDataPlanePlanError> {
        if self.position == self.frame.len() {
            Ok(())
        } else {
            Err(RemoteAgentDataPlanePlanError::TrailingBytes)
        }
    }
}

/// Stable construction, codec, correlation, and auth failures for T2 tranche A.
#[derive(Debug)]
pub enum RemoteAgentDataPlanePlanError {
    InvalidProfile,
    InvalidBootstrapCas,
    InvalidShape,
    InvalidTerminalFacts,
    TerminalCorrelationMismatch,
    InvalidResponseAuthentication,
    InvalidLength,
    ProjectionMismatch,
    CompatibilityMismatch,
    CommitmentMismatch,
    TargetMismatch,
    BindingNotAllowed,
    UnsupportedWire,
    Truncated,
    TrailingBytes,
    FrameTooLarge,
    NonCanonicalFrame,
    Digest(DigestBuildError),
    Predecessor(ManagedAgentStackPlanError),
    Distributed(DistributedAgentStackPlanError),
    Fabric(crate::managed_fabric_plan::ManagedFabricPlanError),
    Provenance(crate::provenance::ProvenanceContractError),
    Apply(crate::apply::ApplyContractError),
    ReferenceContract,
    ReferenceWire,
}

impl From<DigestBuildError> for RemoteAgentDataPlanePlanError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}

impl From<ManagedAgentStackPlanError> for RemoteAgentDataPlanePlanError {
    fn from(value: ManagedAgentStackPlanError) -> Self {
        Self::Predecessor(value)
    }
}

impl From<DistributedAgentStackPlanError> for RemoteAgentDataPlanePlanError {
    fn from(value: DistributedAgentStackPlanError) -> Self {
        Self::Distributed(value)
    }
}

impl From<crate::managed_fabric_plan::ManagedFabricPlanError> for RemoteAgentDataPlanePlanError {
    fn from(value: crate::managed_fabric_plan::ManagedFabricPlanError) -> Self {
        Self::Fabric(value)
    }
}

impl From<crate::provenance::ProvenanceContractError> for RemoteAgentDataPlanePlanError {
    fn from(value: crate::provenance::ProvenanceContractError) -> Self {
        Self::Provenance(value)
    }
}

impl From<crate::apply::ApplyContractError> for RemoteAgentDataPlanePlanError {
    fn from(value: crate::apply::ApplyContractError) -> Self {
        Self::Apply(value)
    }
}

impl From<crate::reference_assembly::ReferenceContractError> for RemoteAgentDataPlanePlanError {
    fn from(_value: crate::reference_assembly::ReferenceContractError) -> Self {
        Self::ReferenceContract
    }
}

impl From<crate::reference_assembly::ReferenceWireError> for RemoteAgentDataPlanePlanError {
    fn from(_value: crate::reference_assembly::ReferenceWireError) -> Self {
        Self::ReferenceWire
    }
}

impl fmt::Display for RemoteAgentDataPlanePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "remote Agent data-plane contract rejected: {self:?}"
        )
    }
}

impl std::error::Error for RemoteAgentDataPlanePlanError {}

// -----------------------------------------------------------------------------
// Additive S0-retaining/S1-proxy successor (PXTE v10 / PXAR v11 / PXAU v2).
// -----------------------------------------------------------------------------

const TARGET_EXECUTION_V2_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.target-execution.sha256.v10";
const TARGET_ASSIGNMENTS_V2_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.target-plan-assignments.sha256.v11";
const APPLY_REQUEST_V2_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-proxy-data-plane-request.sha256.v2";
const PROXY_TOPOLOGY_COMPATIBILITY_V2_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-proxy-topology-compatibility.sha256.v2";
const RETAINED_S0_CAS_V2_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-retained-s0-cas.sha256.v2";
const ACTIVE_S1_CAS_V2_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-active-s1-cas.sha256.v2";
const TERMINAL_V2_SIGNING_MAGIC: &[u8] =
    b"ParaEGOX\0remote-agent-proxy-data-plane-terminal-signing";
const TERMINAL_V2_RESULT_REF_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-proxy-data-plane-terminal-result.sha256.v2";
const TERMINAL_V2_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-proxy-data-plane-terminal.sha256.v2";

const TARGET_EXECUTION_V2_PREFIX_BYTES: usize = 4 + 2 + PROJECTION_BYTES + 32 + 2 + 1 + 1 + 4 + 4;
const TARGET_EXECUTION_V2_PROFILE_PRESENT: u8 = 1;
const ACTIVE_S1_CAS_V2_PRESENT: u8 = 1;
const ACTIVE_S1_CAS_V2_ABSENT: u8 = 0;
const REMOTE_AGENT_PROXY_EXACT_ROUTE_BITMAP: u8 = 0b11;
const TERMINAL_V2_EVIDENCE_KNOWN_FLAGS: u16 = 0b0111_1111;
const TERMINAL_V2_RETAINED_S0_CENSUS_COMPLETE: u16 = 1;
const TERMINAL_V2_RETAINED_S0_READY: u16 = 1 << 1;
const TERMINAL_V2_S1_TLS_READY: u16 = 1 << 2;
const TERMINAL_V2_S1_ACL_READY: u16 = 1 << 3;
const TERMINAL_V2_S1_CLOSED: u16 = 1 << 4;
const TERMINAL_V2_S1_LISTENER_RELEASED: u16 = 1 << 5;
const TERMINAL_V2_QUARANTINED: u16 = 1 << 6;
const TERMINAL_V2_FIXED_BYTES: usize = 4
    + 2
    + 16
    + 32
    + 16
    + 16
    + (4 * 32)
    + 16
    + 8
    + 32
    + (3 * 9)
    + 17
    + (5 * 32)
    + (4 * 8)
    + 8
    + 8
    + 8
    + 8
    + 16
    + 8
    + 8
    + 2
    + 6
    + 2
    + 16
    + 16
    + 2
    + 2
    + 2;

/// Strict target-execution successor version carried by PXTE v10.
pub const REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_VERSION: u16 = 10;
/// Strict apply-request successor version carried by PXAR v11.
pub const REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_VERSION: u16 = 11;
/// Strict Runtime terminal successor version carried by PXAU v2.
pub const REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_VERSION: u16 = 2;
/// Domain-separated Runtime signing transcript version for PXAU v2.
pub const REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNING_V2_VERSION: u16 = 2;
/// Exact number of PXAP-derived routes admitted by the S1 proxy.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_ROUTE_COUNT_V2: u16 = 2;
/// Exact bounded queue capacity owned by each route-specific S1 forwarder.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_QUEUE_CAPACITY_V2: u16 = 1;
/// Exact worker count owned by each route-specific S1 forwarder.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_WORKERS_PER_ROUTE_V2: u16 = 1;
/// Exact maximum pending TLS sessions admitted by S1.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_MAX_PENDING_SESSIONS_V2: u16 = 1;
/// Exact maximum live TLS sessions admitted by S1.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_MAX_SESSIONS_V2: u16 = 1;
/// Exact maximum links admitted by S1.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_MAX_LINKS_V2: u16 = 1;
/// Fixed Zenoh receive-message ceiling used by the pinned S1 transport profile.
pub const REMOTE_AGENT_DATA_PLANE_PROXY_MAX_MESSAGE_BYTES_V2: u32 = 1_114_220;
/// Fixed width of one retained-S0 compare-and-swap value.
pub const REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES: usize = 200;
/// Fixed width of one expected-active-S1 compare-and-swap value.
pub const REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES: usize = 152;
/// Maximum canonical PXTE v10 bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES: usize =
    TARGET_EXECUTION_V2_PREFIX_BYTES
        + MAX_MANAGED_AGENT_STACK_TARGET_EXECUTION_BYTES
        + REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES
        + REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES
        + MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES;
/// Maximum canonical PXTA-zero plus PXTE-v10 durable Slice bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_PLAN_SLICE_V2_BYTES: usize =
    EMPTY_PXTA.len() + MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES;
/// Maximum canonical PXAR v11 bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES: usize = APPLY_REQUEST_HEADER_BYTES
    + MAX_RUNTIME_APPLY_ENVELOPE_V2_BYTES
    + EMPTY_PXTA.len()
    + MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES;
/// Maximum canonical PXAU v2 bytes.
pub const MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES: usize = 2_048;
/// Maximum Runtime signature retained by PXAU v2.
pub const MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_V2_BYTES: usize =
    MAX_CONTROL_READ_SIGNATURE_BYTES;

/// Exact live S0 facts that the proxy successor is forbidden to mutate.
#[derive(Clone, Copy, Debug)]
pub struct RemoteAgentRetainedS0CasFieldsV2 {
    pub expected_active_pxft_digest: Digest32,
    pub expected_active_pxst_digest: Digest32,
    pub expected_descriptor_evidence_record_digest: Digest32,
    pub expected_descriptor_evidence_record_sequence: u64,
    pub expected_descriptor_receipt_digest: Digest32,
    pub expected_descriptor_payload_digest: Digest32,
    pub expected_fabric_session_epoch: DistributedFabricSessionEpochV1,
    pub expected_fabric_generation: ManagedServiceGeneration,
    pub expected_agent_generation: ManagedServiceGeneration,
}

/// Fixed-width CAS over the existing S0 Fabric/Agent/PXDE/PXAP authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentRetainedS0CasV2 {
    fields: RemoteAgentRetainedS0CasFieldsV2,
    canonical_wire: [u8; REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES],
    cas_digest: Digest32,
}

impl PartialEq for RemoteAgentRetainedS0CasFieldsV2 {
    fn eq(&self, other: &Self) -> bool {
        self.expected_active_pxft_digest == other.expected_active_pxft_digest
            && self.expected_active_pxst_digest == other.expected_active_pxst_digest
            && self.expected_descriptor_evidence_record_digest
                == other.expected_descriptor_evidence_record_digest
            && self.expected_descriptor_evidence_record_sequence
                == other.expected_descriptor_evidence_record_sequence
            && self.expected_descriptor_receipt_digest == other.expected_descriptor_receipt_digest
            && self.expected_descriptor_payload_digest == other.expected_descriptor_payload_digest
            && self.expected_fabric_session_epoch == other.expected_fabric_session_epoch
            && self.expected_fabric_generation == other.expected_fabric_generation
            && self.expected_agent_generation == other.expected_agent_generation
    }
}

impl Eq for RemoteAgentRetainedS0CasFieldsV2 {}

impl RemoteAgentRetainedS0CasV2 {
    pub fn try_new(
        fields: RemoteAgentRetainedS0CasFieldsV2,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if digest_is_zero(fields.expected_active_pxft_digest)
            || digest_is_zero(fields.expected_active_pxst_digest)
            || digest_is_zero(fields.expected_descriptor_evidence_record_digest)
            || fields.expected_descriptor_evidence_record_sequence == 0
            || digest_is_zero(fields.expected_descriptor_receipt_digest)
            || digest_is_zero(fields.expected_descriptor_payload_digest)
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidBootstrapCas);
        }
        let canonical_wire = encode_retained_s0_cas_v2(fields);
        let cas_digest = digest_wire(RETAINED_S0_CAS_V2_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            fields,
            canonical_wire,
            cas_digest,
        })
    }

    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() != REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES {
            return Err(if frame.len() < REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES {
                RemoteAgentDataPlanePlanError::Truncated
            } else {
                RemoteAgentDataPlanePlanError::FrameTooLarge
            });
        }
        let mut cursor = Cursor::new(frame);
        let fields = RemoteAgentRetainedS0CasFieldsV2 {
            expected_active_pxft_digest: Digest32::from_bytes(cursor.array()?),
            expected_active_pxst_digest: Digest32::from_bytes(cursor.array()?),
            expected_descriptor_evidence_record_digest: Digest32::from_bytes(cursor.array()?),
            expected_descriptor_evidence_record_sequence: cursor.u64()?,
            expected_descriptor_receipt_digest: Digest32::from_bytes(cursor.array()?),
            expected_descriptor_payload_digest: Digest32::from_bytes(cursor.array()?),
            expected_fabric_session_epoch: DistributedFabricSessionEpochV1::try_from_bytes(
                cursor.array()?,
            )?,
            expected_fabric_generation: ManagedServiceGeneration::try_new(cursor.u64()?)
                .map_err(|_| RemoteAgentDataPlanePlanError::InvalidBootstrapCas)?,
            expected_agent_generation: ManagedServiceGeneration::try_new(cursor.u64()?)
                .map_err(|_| RemoteAgentDataPlanePlanError::InvalidBootstrapCas)?,
        };
        cursor.finish()?;
        let decoded = Self::try_new(fields)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub const fn fields(self) -> RemoteAgentRetainedS0CasFieldsV2 {
        self.fields
    }

    #[must_use]
    pub const fn canonical_wire(&self) -> &[u8; REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES] {
        &self.canonical_wire
    }

    #[must_use]
    pub const fn cas_digest(self) -> Digest32 {
        self.cas_digest
    }
}

/// Exact active-S1 roots required by a LocalOnly PXTE v10 operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentActiveS1FieldsV2 {
    pub active_pxau_digest: Digest32,
    pub active_request_digest: Digest32,
    pub active_snapshot_digest: Digest32,
    pub active_snapshot_sequence: u64,
    pub active_access_generation: ManagedServiceGeneration,
    pub active_proxy_session_epoch: [u8; 16],
}

/// Fixed-width absence-or-active CAS for the Runtime-owned S1 lifecycle slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteAgentActiveS1CasV2 {
    access_generation_high_water: u64,
    owner_slot_revision: u64,
    active: Option<RemoteAgentActiveS1FieldsV2>,
    canonical_wire: [u8; REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES],
    cas_digest: Digest32,
}

impl RemoteAgentActiveS1CasV2 {
    pub fn try_expect_absent(
        access_generation_high_water: u64,
        owner_slot_revision: u64,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        Self::try_new(access_generation_high_water, owner_slot_revision, None)
    }

    pub fn try_expect_active(
        access_generation_high_water: u64,
        owner_slot_revision: u64,
        active: RemoteAgentActiveS1FieldsV2,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        Self::try_new(
            access_generation_high_water,
            owner_slot_revision,
            Some(active),
        )
    }

    fn try_new(
        access_generation_high_water: u64,
        owner_slot_revision: u64,
        active: Option<RemoteAgentActiveS1FieldsV2>,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if let Some(active) = active {
            if access_generation_high_water == 0
                || owner_slot_revision == 0
                || digest_is_zero(active.active_pxau_digest)
                || digest_is_zero(active.active_request_digest)
                || digest_is_zero(active.active_snapshot_digest)
                || active.active_snapshot_sequence == 0
                || active.active_access_generation.value() != access_generation_high_water
                || bytes_are_zero(&active.active_proxy_session_epoch)
            {
                return Err(RemoteAgentDataPlanePlanError::InvalidBootstrapCas);
            }
        }
        let canonical_wire =
            encode_active_s1_cas_v2(access_generation_high_water, owner_slot_revision, active);
        let cas_digest = digest_wire(ACTIVE_S1_CAS_V2_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            access_generation_high_water,
            owner_slot_revision,
            active,
            canonical_wire,
            cas_digest,
        })
    }

    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() != REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES {
            return Err(if frame.len() < REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES {
                RemoteAgentDataPlanePlanError::Truncated
            } else {
                RemoteAgentDataPlanePlanError::FrameTooLarge
            });
        }
        let mut cursor = Cursor::new(frame);
        let present = cursor.u8()?;
        if cursor.take(7)?.iter().any(|byte| *byte != 0) {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        let access_generation_high_water = cursor.u64()?;
        let owner_slot_revision = cursor.u64()?;
        let active_pxau_digest = Digest32::from_bytes(cursor.array()?);
        let active_request_digest = Digest32::from_bytes(cursor.array()?);
        let active_snapshot_digest = Digest32::from_bytes(cursor.array()?);
        let active_snapshot_sequence = cursor.u64()?;
        let active_generation_value = cursor.u64()?;
        let active_proxy_session_epoch: [u8; 16] = cursor.array()?;
        cursor.finish()?;
        let all_active_zero = digest_is_zero(active_pxau_digest)
            && digest_is_zero(active_request_digest)
            && digest_is_zero(active_snapshot_digest)
            && active_snapshot_sequence == 0
            && active_generation_value == 0
            && bytes_are_zero(&active_proxy_session_epoch);
        let decoded = match present {
            ACTIVE_S1_CAS_V2_ABSENT if all_active_zero => {
                Self::try_expect_absent(access_generation_high_water, owner_slot_revision)?
            }
            ACTIVE_S1_CAS_V2_PRESENT if !all_active_zero => Self::try_expect_active(
                access_generation_high_water,
                owner_slot_revision,
                RemoteAgentActiveS1FieldsV2 {
                    active_pxau_digest,
                    active_request_digest,
                    active_snapshot_digest,
                    active_snapshot_sequence,
                    active_access_generation: ManagedServiceGeneration::try_new(
                        active_generation_value,
                    )
                    .map_err(|_| RemoteAgentDataPlanePlanError::InvalidBootstrapCas)?,
                    active_proxy_session_epoch,
                },
            )?,
            _ => return Err(RemoteAgentDataPlanePlanError::InvalidBootstrapCas),
        };
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub const fn access_generation_high_water(self) -> u64 {
        self.access_generation_high_water
    }

    #[must_use]
    pub const fn owner_slot_revision(self) -> u64 {
        self.owner_slot_revision
    }

    #[must_use]
    pub const fn active(self) -> Option<RemoteAgentActiveS1FieldsV2> {
        self.active
    }

    #[must_use]
    pub const fn canonical_wire(&self) -> &[u8; REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES] {
        &self.canonical_wire
    }

    #[must_use]
    pub const fn cas_digest(self) -> Digest32 {
        self.cas_digest
    }
}

fn encode_retained_s0_cas_v2(
    fields: RemoteAgentRetainedS0CasFieldsV2,
) -> [u8; REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES] {
    let mut wire = [0_u8; REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES];
    let mut offset = 0;
    for digest in [
        fields.expected_active_pxft_digest,
        fields.expected_active_pxst_digest,
        fields.expected_descriptor_evidence_record_digest,
    ] {
        wire[offset..offset + 32].copy_from_slice(digest.as_bytes());
        offset += 32;
    }
    wire[offset..offset + 8].copy_from_slice(
        &fields
            .expected_descriptor_evidence_record_sequence
            .to_be_bytes(),
    );
    offset += 8;
    for digest in [
        fields.expected_descriptor_receipt_digest,
        fields.expected_descriptor_payload_digest,
    ] {
        wire[offset..offset + 32].copy_from_slice(digest.as_bytes());
        offset += 32;
    }
    wire[offset..offset + 16].copy_from_slice(fields.expected_fabric_session_epoch.as_bytes());
    offset += 16;
    wire[offset..offset + 8]
        .copy_from_slice(&fields.expected_fabric_generation.value().to_be_bytes());
    offset += 8;
    wire[offset..offset + 8]
        .copy_from_slice(&fields.expected_agent_generation.value().to_be_bytes());
    debug_assert_eq!(offset + 8, REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES);
    wire
}

fn encode_active_s1_cas_v2(
    access_generation_high_water: u64,
    owner_slot_revision: u64,
    active: Option<RemoteAgentActiveS1FieldsV2>,
) -> [u8; REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES] {
    let mut wire = [0_u8; REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES];
    wire[0] = u8::from(active.is_some());
    wire[8..16].copy_from_slice(&access_generation_high_water.to_be_bytes());
    wire[16..24].copy_from_slice(&owner_slot_revision.to_be_bytes());
    if let Some(active) = active {
        wire[24..56].copy_from_slice(active.active_pxau_digest.as_bytes());
        wire[56..88].copy_from_slice(active.active_request_digest.as_bytes());
        wire[88..120].copy_from_slice(active.active_snapshot_digest.as_bytes());
        wire[120..128].copy_from_slice(&active.active_snapshot_sequence.to_be_bytes());
        wire[128..136].copy_from_slice(&active.active_access_generation.value().to_be_bytes());
        wire[136..152].copy_from_slice(&active.active_proxy_session_epoch);
    }
    wire
}

/// Computes the successor-only S1 topology fingerprint without reinterpreting PXAE v1.
pub fn remote_agent_proxy_topology_compatibility_digest_v2() -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(PROXY_TOPOLOGY_COMPATIBILITY_V2_DIGEST_DOMAIN)?;
    builder.field_digest(&remote_agent_data_plane_compatibility_digest_v1()?)?;
    builder.field_bytes(PROJECTION_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROJECTION_VERSION)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROJECTION_BYTES as u16)?;
    builder.field_bytes(PROFILE_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION)?;
    builder.field_u16(ASYMMETRIC_LISTENER_CONNECTOR_PROFILE_KIND)?;
    builder.field_u16(ASYMMETRIC_AGENT_ACL_PROFILE_VERSION)?;
    builder.field_bytes(&(MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES as u32).to_be_bytes())?;
    builder.field_bytes(TARGET_EXECUTION_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_VERSION)?;
    builder.field_bytes(
        &(MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES as u32).to_be_bytes(),
    )?;
    builder.field_bytes(APPLY_REQUEST_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_VERSION)?;
    builder.field_u16(APPLY_REQUEST_HEADER_BYTES as u16)?;
    builder
        .field_bytes(&(MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES as u32).to_be_bytes())?;
    builder.field_bytes(TERMINAL_RECEIPT_MAGIC)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_VERSION)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNING_V2_VERSION)?;
    builder.field_bytes(&(TERMINAL_V2_FIXED_BYTES as u32).to_be_bytes())?;
    builder.field_u16(MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES as u16)?;
    builder.field_bytes(&EMPTY_PXTA)?;
    builder.field_u16(REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES as u16)?;
    builder.field_u16(REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive as u16)?;
    builder.field_u16(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate as u16)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROXY_ROUTE_COUNT_V2)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROXY_QUEUE_CAPACITY_V2)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROXY_WORKERS_PER_ROUTE_V2)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROXY_MAX_PENDING_SESSIONS_V2)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROXY_MAX_SESSIONS_V2)?;
    builder.field_u16(REMOTE_AGENT_DATA_PLANE_PROXY_MAX_LINKS_V2)?;
    builder.field_bytes(&REMOTE_AGENT_DATA_PLANE_PROXY_MAX_MESSAGE_BYTES_V2.to_be_bytes())?;
    builder.field_bytes(&MAX_MANAGED_AGENT_FRAME_BYTES.to_be_bytes())?;
    builder.field_bytes(&MAX_MANAGED_AGENT_RESPONSE_BODY_BYTES.to_be_bytes())?;
    builder.field_u64(MAX_RESTRICTED_RUNTIME_APPLY_OPERATION_TIMEOUT_NANOS)?;
    builder.field_bytes(b"s1-listener-only-tls")?;
    builder.field_bytes(b"retained-pxap-submit-control")?;
    builder.field_bytes(b"queue-one-worker-one-per-route")?;
    builder.field_bytes(b"no-request-retry")?;
    builder.field_bytes(b"admission-absolute-deadline-from-pxad-v1")?;
    builder.field_bytes(b"fence-drain-join-close")?;
    builder.field_bytes(TARGET_EXECUTION_V2_DIGEST_DOMAIN)?;
    builder.field_bytes(TARGET_ASSIGNMENTS_V2_DIGEST_DOMAIN)?;
    builder.field_bytes(APPLY_REQUEST_V2_DIGEST_DOMAIN)?;
    builder.field_bytes(RETAINED_S0_CAS_V2_DIGEST_DOMAIN)?;
    builder.field_bytes(ACTIVE_S1_CAS_V2_DIGEST_DOMAIN)?;
    builder.field_bytes(TERMINAL_V2_SIGNING_MAGIC)?;
    builder.field_bytes(TERMINAL_V2_RESULT_REF_DOMAIN)?;
    builder.field_bytes(TERMINAL_V2_DIGEST_DOMAIN)?;
    Ok(builder.finish())
}

/// Exact desired S1 lifecycle transition admitted by PXTE v10.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum RemoteAgentDataPlaneTargetModeV2 {
    /// Add one listener-only TLS S1 over an exact retained S0 CAS.
    RemoteAccessActive = 1,
    /// Fence, drain, join, and close one exact active S1 while retaining S0.
    LocalAgentOnlyDeactivate = 2,
}

/// Canonical PXTE v10 carrying byte-exact PXAE/PXAD predecessors and S0/S1 CAS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneTargetExecutionV2 {
    projection: RemoteAgentDataPlaneProjectionV1,
    proxy_topology_compatibility_digest: Digest32,
    mode: RemoteAgentDataPlaneTargetModeV2,
    predecessor: ManagedAgentStackTargetExecutionV1,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    profile: RemoteAgentDataPlaneProfileV1,
    canonical_wire: Box<[u8]>,
    execution_digest: Digest32,
}

impl RemoteAgentDataPlaneTargetExecutionV2 {
    pub fn try_remote_access_active(
        projection: RemoteAgentDataPlaneProjectionV1,
        predecessor: ManagedAgentStackTargetExecutionV1,
        retained_s0_cas: RemoteAgentRetainedS0CasV2,
        expected_s1_cas: RemoteAgentActiveS1CasV2,
        profile: RemoteAgentDataPlaneProfileV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if expected_s1_cas.active().is_some() {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        Self::try_new(
            projection,
            RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
            predecessor,
            retained_s0_cas,
            expected_s1_cas,
            profile,
        )
    }

    pub fn try_local_agent_only_deactivate(
        projection: RemoteAgentDataPlaneProjectionV1,
        predecessor: ManagedAgentStackTargetExecutionV1,
        retained_s0_cas: RemoteAgentRetainedS0CasV2,
        expected_s1_cas: RemoteAgentActiveS1CasV2,
        profile: RemoteAgentDataPlaneProfileV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if expected_s1_cas.active().is_none() {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        Self::try_new(
            projection,
            RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
            predecessor,
            retained_s0_cas,
            expected_s1_cas,
            profile,
        )
    }

    fn try_new(
        projection: RemoteAgentDataPlaneProjectionV1,
        mode: RemoteAgentDataPlaneTargetModeV2,
        predecessor: ManagedAgentStackTargetExecutionV1,
        retained_s0_cas: RemoteAgentRetainedS0CasV2,
        expected_s1_cas: RemoteAgentActiveS1CasV2,
        profile: RemoteAgentDataPlaneProfileV1,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if predecessor.projection() != projection.managed_agent_stack_projection()
            || predecessor.mode() != ManagedAgentStackTargetModeV1::FabricAndAgent
            || profile.target() != projection.target()
            || predecessor.fabric().listen_endpoint()
                != Some(profile.base_loopback_listen_endpoint())
            || matches!(
                (mode, expected_s1_cas.active()),
                (
                    RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
                    Some(_)
                ) | (
                    RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
                    None
                )
            )
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        let proxy_topology_compatibility_digest =
            remote_agent_proxy_topology_compatibility_digest_v2()?;
        let canonical_wire = build_target_execution_wire_v2(
            &projection,
            proxy_topology_compatibility_digest,
            mode,
            &predecessor,
            retained_s0_cas,
            expected_s1_cas,
            &profile,
        )?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        let execution_digest = digest_wire(TARGET_EXECUTION_V2_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            projection,
            proxy_topology_compatibility_digest,
            mode,
            predecessor,
            retained_s0_cas,
            expected_s1_cas,
            profile,
            canonical_wire: canonical_wire.into_boxed_slice(),
            execution_digest,
        })
    }

    /// Strictly decodes only PXTE v10; the v9 decoder remains byte-exact.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() > MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        if frame.len()
            < TARGET_EXECUTION_V2_PREFIX_BYTES
                + REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES
                + REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.take(4)? != TARGET_EXECUTION_MAGIC
            || cursor.u16()? != REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let projection = RemoteAgentDataPlaneProjectionV1::decode(cursor.take(PROJECTION_BYTES)?)?;
        let compatibility_digest = Digest32::from_bytes(cursor.array()?);
        if compatibility_digest != remote_agent_proxy_topology_compatibility_digest_v2()? {
            return Err(RemoteAgentDataPlanePlanError::CompatibilityMismatch);
        }
        if cursor.u16()? != REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        let mode = match cursor.u8()? {
            1 => RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
            2 => RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
            _ => return Err(RemoteAgentDataPlanePlanError::InvalidShape),
        };
        if cursor.u8()? != TARGET_EXECUTION_V2_PROFILE_PRESENT {
            return Err(RemoteAgentDataPlanePlanError::InvalidShape);
        }
        let predecessor_length = cursor.usize_u32()?;
        let profile_length = cursor.usize_u32()?;
        if predecessor_length == 0
            || predecessor_length > MAX_MANAGED_AGENT_STACK_TARGET_EXECUTION_BYTES
            || profile_length == 0
            || profile_length > MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidLength);
        }
        let predecessor =
            ManagedAgentStackTargetExecutionV1::decode(cursor.take(predecessor_length)?)?;
        let retained_s0_cas = RemoteAgentRetainedS0CasV2::decode(
            cursor.take(REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES)?,
        )?;
        let expected_s1_cas =
            RemoteAgentActiveS1CasV2::decode(cursor.take(REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES)?)?;
        let profile = RemoteAgentDataPlaneProfileV1::decode(cursor.take(profile_length)?)?;
        cursor.finish()?;
        let decoded = Self::try_new(
            projection,
            mode,
            predecessor,
            retained_s0_cas,
            expected_s1_cas,
            profile,
        )?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub const fn projection(&self) -> &RemoteAgentDataPlaneProjectionV1 {
        &self.projection
    }

    #[must_use]
    pub const fn proxy_topology_compatibility_digest(&self) -> Digest32 {
        self.proxy_topology_compatibility_digest
    }

    #[must_use]
    pub const fn mode(&self) -> RemoteAgentDataPlaneTargetModeV2 {
        self.mode
    }

    #[must_use]
    pub const fn predecessor(&self) -> &ManagedAgentStackTargetExecutionV1 {
        &self.predecessor
    }

    #[must_use]
    pub const fn retained_s0_cas(&self) -> RemoteAgentRetainedS0CasV2 {
        self.retained_s0_cas
    }

    #[must_use]
    pub const fn expected_s1_cas(&self) -> RemoteAgentActiveS1CasV2 {
        self.expected_s1_cas
    }

    #[must_use]
    pub const fn profile(&self) -> &RemoteAgentDataPlaneProfileV1 {
        &self.profile
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub const fn execution_digest(&self) -> Digest32 {
        self.execution_digest
    }
}

fn build_target_execution_wire_v2(
    projection: &RemoteAgentDataPlaneProjectionV1,
    proxy_topology_compatibility_digest: Digest32,
    mode: RemoteAgentDataPlaneTargetModeV2,
    predecessor: &ManagedAgentStackTargetExecutionV1,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    profile: &RemoteAgentDataPlaneProfileV1,
) -> Result<Vec<u8>, RemoteAgentDataPlanePlanError> {
    let predecessor_length = u32::try_from(predecessor.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let profile_length = u32::try_from(profile.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let mut wire = Vec::with_capacity(
        TARGET_EXECUTION_V2_PREFIX_BYTES
            + predecessor.canonical_wire().len()
            + REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES
            + REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES
            + profile.canonical_wire().len(),
    );
    wire.extend_from_slice(TARGET_EXECUTION_MAGIC);
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_VERSION.to_be_bytes());
    wire.extend_from_slice(projection.canonical_wire());
    wire.extend_from_slice(proxy_topology_compatibility_digest.as_bytes());
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_PROFILE_VERSION.to_be_bytes());
    wire.push(mode as u8);
    wire.push(TARGET_EXECUTION_V2_PROFILE_PRESENT);
    wire.extend_from_slice(&predecessor_length.to_be_bytes());
    wire.extend_from_slice(&profile_length.to_be_bytes());
    wire.extend_from_slice(predecessor.canonical_wire());
    wire.extend_from_slice(retained_s0_cas.canonical_wire());
    wire.extend_from_slice(expected_s1_cas.canonical_wire());
    wire.extend_from_slice(profile.canonical_wire());
    Ok(wire)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteAgentDataPlaneAssignmentsV2 {
    bindings: TargetAssignments,
    execution: RemoteAgentDataPlaneTargetExecutionV2,
    assignment_digest: TargetAssignmentDigest,
}

impl RemoteAgentDataPlaneAssignmentsV2 {
    fn try_from_execution(
        execution: RemoteAgentDataPlaneTargetExecutionV2,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let bindings = TargetAssignments::try_new(Vec::new())
            .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
        Self::try_new(bindings, execution)
    }

    fn try_new(
        bindings: TargetAssignments,
        execution: RemoteAgentDataPlaneTargetExecutionV2,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        bindings
            .validate()
            .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
        if !bindings.is_empty() || bindings.canonical_wire() != EMPTY_PXTA {
            return Err(RemoteAgentDataPlanePlanError::BindingNotAllowed);
        }
        let mut builder = Digest32Builder::try_new(TARGET_ASSIGNMENTS_V2_DIGEST_DOMAIN)?;
        builder.field_digest(bindings.assignment_digest().value())?;
        builder.field_digest(&execution.execution_digest())?;
        Ok(Self {
            bindings,
            execution,
            assignment_digest: TargetAssignmentDigest::new(builder.finish()),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteAgentDataPlanePlanSliceV2 {
    commitment: RuntimeSliceCommitment,
    assignments: RemoteAgentDataPlaneAssignmentsV2,
}

impl RemoteAgentDataPlanePlanSliceV2 {
    fn try_new(
        commitment: RuntimeSliceCommitment,
        assignments: RemoteAgentDataPlaneAssignmentsV2,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        commitment.validate()?;
        if commitment.header().assignment_digest() != assignments.assignment_digest {
            return Err(RemoteAgentDataPlanePlanError::CommitmentMismatch);
        }
        if commitment.header().target() != assignments.execution.projection().target() {
            return Err(RemoteAgentDataPlanePlanError::TargetMismatch);
        }
        Ok(Self {
            commitment,
            assignments,
        })
    }
}

/// Signature-independent PXAR v11 producer using the unchanged envelope-v2 transcript.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneApplyRequestDraftV2 {
    envelope: RuntimeApplyEnvelopeV2Draft,
    slice: RemoteAgentDataPlanePlanSliceV2,
}

impl RemoteAgentDataPlaneApplyRequestDraftV2 {
    pub fn try_new(
        execution: RemoteAgentDataPlaneTargetExecutionV2,
        provenance: PlanProvenance,
        control: RuntimeApplyControl,
        temporal: ApplyTemporalConstraint,
        expected_runtime_store_instance_id: [u8; 32],
        auth_claim: ApplyRequestAuthClaim,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        let assignments = RemoteAgentDataPlaneAssignmentsV2::try_from_execution(execution)?;
        let header = RuntimeSliceHeader::new(
            assignments.execution.projection().target(),
            provenance,
            assignments.assignment_digest,
        );
        let commitment = RuntimeSliceCommitment::try_new(header)?;
        let slice = RemoteAgentDataPlanePlanSliceV2::try_new(commitment, assignments)?;
        let control_commitment = RuntimeApplyControlCommitment::try_new(commitment, control)?;
        let store = RuntimeStoreInstanceId::try_from_bytes(expected_runtime_store_instance_id)?;
        let envelope =
            RuntimeApplyEnvelopeV2Draft::try_new(control_commitment, temporal, store, auth_claim)?;
        Ok(Self { envelope, slice })
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentDataPlaneApplySigningTranscriptV2, RemoteAgentDataPlanePlanError> {
        Ok(RemoteAgentDataPlaneApplySigningTranscriptV2(
            self.envelope.signing_transcript()?,
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentDataPlaneApplyRequestV2, RemoteAgentDataPlanePlanError> {
        let envelope = self.envelope.finalize(signature)?;
        RemoteAgentDataPlaneApplyRequestV2::try_new(envelope, self.slice)
    }
}

/// Signed strict PXAR v11 carrying envelope v2, PXTA-zero, and PXTE v10.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentDataPlaneApplyRequestV2 {
    envelope: RuntimeApplyEnvelopeV2,
    slice: RemoteAgentDataPlanePlanSliceV2,
    canonical_wire: Box<[u8]>,
    request_digest: Digest32,
}

impl RemoteAgentDataPlaneApplyRequestV2 {
    fn try_new(
        envelope: RuntimeApplyEnvelopeV2,
        slice: RemoteAgentDataPlanePlanSliceV2,
    ) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if envelope.control_commitment().slice() != slice.commitment {
            return Err(RemoteAgentDataPlanePlanError::CommitmentMismatch);
        }
        let canonical_wire = build_apply_request_wire_v2(&envelope, &slice)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        let request_digest = digest_wire(APPLY_REQUEST_V2_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            envelope,
            slice,
            canonical_wire: canonical_wire.into_boxed_slice(),
            request_digest,
        })
    }

    /// Strictly decodes only PXAR v11; the v10 decoder remains byte-exact.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentDataPlanePlanError> {
        if frame.len() > MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES {
            return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
        }
        if frame.len() < APPLY_REQUEST_HEADER_BYTES {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        if &frame[..4] != APPLY_REQUEST_MAGIC
            || read_u16(&frame[4..6]) != REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_VERSION
        {
            return Err(RemoteAgentDataPlanePlanError::UnsupportedWire);
        }
        let envelope_length = read_u32(&frame[6..10]) as usize;
        let bindings_length = read_u32(&frame[10..14]) as usize;
        let execution_length = read_u32(&frame[14..18]) as usize;
        if envelope_length > MAX_RUNTIME_APPLY_ENVELOPE_V2_BYTES
            || bindings_length != EMPTY_PXTA.len()
            || execution_length > MAX_REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_BYTES
        {
            return Err(RemoteAgentDataPlanePlanError::InvalidLength);
        }
        let expected_length = APPLY_REQUEST_HEADER_BYTES
            .checked_add(envelope_length)
            .and_then(|value| value.checked_add(bindings_length))
            .and_then(|value| value.checked_add(execution_length))
            .ok_or(RemoteAgentDataPlanePlanError::FrameTooLarge)?;
        if frame.len() < expected_length {
            return Err(RemoteAgentDataPlanePlanError::Truncated);
        }
        if frame.len() > expected_length {
            return Err(RemoteAgentDataPlanePlanError::TrailingBytes);
        }
        let envelope_start = APPLY_REQUEST_HEADER_BYTES;
        let envelope_end = envelope_start + envelope_length;
        let bindings_end = envelope_end + bindings_length;
        let envelope = RuntimeApplyEnvelopeV2::decode(&frame[envelope_start..envelope_end])?;
        let binding_frame = &frame[envelope_end..bindings_end];
        if binding_frame != EMPTY_PXTA {
            return Err(RemoteAgentDataPlanePlanError::BindingNotAllowed);
        }
        let bindings = TargetAssignments::decode(binding_frame)
            .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
        let execution = RemoteAgentDataPlaneTargetExecutionV2::decode(&frame[bindings_end..])?;
        let assignments = RemoteAgentDataPlaneAssignmentsV2::try_new(bindings, execution)?;
        let slice = RemoteAgentDataPlanePlanSliceV2::try_new(
            envelope.control_commitment().slice(),
            assignments,
        )?;
        let decoded = Self::try_new(envelope, slice)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentDataPlanePlanError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }

    #[must_use]
    pub fn canonical_slice_wire(&self) -> &[u8] {
        let offset = APPLY_REQUEST_HEADER_BYTES + self.envelope.canonical_wire().len();
        &self.canonical_wire[offset..]
    }

    #[must_use]
    pub const fn target_execution(&self) -> &RemoteAgentDataPlaneTargetExecutionV2 {
        &self.slice.assignments.execution
    }

    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.slice.commitment.header().target()
    }

    #[must_use]
    pub const fn provenance(&self) -> PlanProvenance {
        self.slice.commitment.header().provenance()
    }

    #[must_use]
    pub const fn assignment_digest(&self) -> TargetAssignmentDigest {
        self.slice.commitment.header().assignment_digest()
    }

    #[must_use]
    pub const fn target_slice_digest(&self) -> TargetSliceDigest {
        self.slice.commitment.target_slice_digest()
    }

    #[must_use]
    pub const fn control_commitment(&self) -> &RuntimeApplyControlCommitment {
        self.envelope.control_commitment()
    }

    #[must_use]
    pub const fn operation_id(&self) -> ApplyOperationId {
        self.envelope.control_commitment().control().operation_id()
    }

    #[must_use]
    pub const fn temporal(&self) -> ApplyTemporalConstraint {
        self.envelope.temporal()
    }

    #[must_use]
    pub const fn expected_runtime_store_instance_id(&self) -> [u8; 32] {
        *self
            .envelope
            .expected_runtime_store_instance_id()
            .as_bytes()
    }

    #[must_use]
    pub const fn authentication(&self) -> &ApplyRequestAuthentication {
        self.envelope.authentication()
    }

    #[must_use]
    pub const fn envelope_request_digest(&self) -> Digest32 {
        self.envelope.request_digest()
    }

    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentDataPlaneApplySigningTranscriptV2, RemoteAgentDataPlanePlanError> {
        Ok(RemoteAgentDataPlaneApplySigningTranscriptV2(
            self.envelope.signing_transcript()?,
        ))
    }

    pub fn validate_expected_store(
        &self,
        local_runtime_store_instance_id: [u8; 32],
    ) -> Result<(), RemoteAgentDataPlanePlanError> {
        let local = RuntimeStoreInstanceId::try_from_bytes(local_runtime_store_instance_id)?;
        self.envelope.validate_expected_store(local)?;
        Ok(())
    }

    pub fn validate_projection(
        &self,
        projection: &RemoteAgentDataPlaneProjectionV1,
    ) -> Result<(), RemoteAgentDataPlanePlanError> {
        if self.target_execution().projection() != projection {
            return Err(RemoteAgentDataPlanePlanError::ProjectionMismatch);
        }
        Ok(())
    }
}

/// Reconstructs one durable `PXTA-zero || PXTE-v10` value from journal authority.
pub fn verify_remote_agent_data_plane_durable_slice_v2(
    canonical_slice_wire: &[u8],
    target: RuntimeHostId,
    provenance: PlanProvenance,
    expected_target_slice_digest: TargetSliceDigest,
    projection: &RemoteAgentDataPlaneProjectionV1,
) -> Result<RemoteAgentDataPlaneTargetExecutionV2, RemoteAgentDataPlanePlanError> {
    if canonical_slice_wire.len() > MAX_REMOTE_AGENT_DATA_PLANE_PLAN_SLICE_V2_BYTES {
        return Err(RemoteAgentDataPlanePlanError::FrameTooLarge);
    }
    if canonical_slice_wire.len() < EMPTY_PXTA.len() {
        return Err(RemoteAgentDataPlanePlanError::Truncated);
    }
    let (binding_frame, execution_frame) = canonical_slice_wire.split_at(EMPTY_PXTA.len());
    if binding_frame != EMPTY_PXTA {
        return Err(RemoteAgentDataPlanePlanError::BindingNotAllowed);
    }
    let bindings = TargetAssignments::decode(binding_frame)
        .map_err(|_| RemoteAgentDataPlanePlanError::BindingNotAllowed)?;
    let execution = RemoteAgentDataPlaneTargetExecutionV2::decode(execution_frame)?;
    if execution.projection() != projection || execution.projection().target() != target {
        return Err(RemoteAgentDataPlanePlanError::ProjectionMismatch);
    }
    let assignments = RemoteAgentDataPlaneAssignmentsV2::try_new(bindings, execution)?;
    let commitment = RuntimeSliceCommitment::try_new(RuntimeSliceHeader::new(
        target,
        provenance,
        assignments.assignment_digest,
    ))?;
    if commitment.target_slice_digest() != expected_target_slice_digest {
        return Err(RemoteAgentDataPlanePlanError::CommitmentMismatch);
    }
    let slice = RemoteAgentDataPlanePlanSliceV2::try_new(commitment, assignments)?;
    Ok(slice.assignments.execution)
}

fn build_apply_request_wire_v2(
    envelope: &RuntimeApplyEnvelopeV2,
    slice: &RemoteAgentDataPlanePlanSliceV2,
) -> Result<Vec<u8>, RemoteAgentDataPlanePlanError> {
    let envelope_length = u32::try_from(envelope.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let bindings_length = u32::try_from(slice.assignments.bindings.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let execution_length = u32::try_from(slice.assignments.execution.canonical_wire().len())
        .map_err(|_| RemoteAgentDataPlanePlanError::InvalidLength)?;
    let mut wire = Vec::with_capacity(
        APPLY_REQUEST_HEADER_BYTES
            + envelope.canonical_wire().len()
            + slice.assignments.bindings.canonical_wire().len()
            + slice.assignments.execution.canonical_wire().len(),
    );
    wire.extend_from_slice(APPLY_REQUEST_MAGIC);
    wire.extend_from_slice(&REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_VERSION.to_be_bytes());
    wire.extend_from_slice(&envelope_length.to_be_bytes());
    wire.extend_from_slice(&bindings_length.to_be_bytes());
    wire.extend_from_slice(&execution_length.to_be_bytes());
    wire.extend_from_slice(envelope.canonical_wire());
    wire.extend_from_slice(slice.assignments.bindings.canonical_wire());
    wire.extend_from_slice(slice.assignments.execution.canonical_wire());
    Ok(wire)
}
