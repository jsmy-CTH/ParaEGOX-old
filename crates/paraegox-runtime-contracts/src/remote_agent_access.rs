//! Independent authenticated carrier for remote Agent access.
//!
//! PXRA/PXRR v1 carries only exact PXAR-v10/PXAU-v1 remote-access values or
//! one payload-free Describe request and its bounded PXAD/PXAP description.
//! The embedded PXCB is the public restricted control-carrier binding; it is
//! never a [`crate::reference_control::ReferenceChannelBindingV1`]. A decoded
//! Describe response is structural data, not a liveness, authorization,
//! capability, session, discovery-freshness, or retry claim.

use core::fmt;

use paraegox_kernel::digest::{Digest32, Digest32Builder, DigestBuildError};
use paraegox_kernel::identity::{PrincipalRef, RuntimeHostId};

use crate::distributed_agent_stack_plan::{
    MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES, RestrictedRuntimeApplyCarrierBindingV1,
};
use crate::managed_service::ManagedServiceGeneration;
use crate::managed_serving_bootstrap::{
    MAX_RUNTIME_AGENT_PORT_DESCRIPTOR_BYTES, runtime_agent_control_descriptor_payload_digest_v1,
};
use crate::remote_agent_data_plane_plan::{
    MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES,
    MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES,
    MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES, MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES,
    MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES, REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES,
    REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES, RemoteAgentActiveS1CasV2,
    RemoteAgentDataPlaneApplyRequestV1, RemoteAgentDataPlaneApplyRequestV2,
    RemoteAgentDataPlanePlanError, RemoteAgentDataPlaneProfileV1,
    RemoteAgentDataPlaneTerminalAuthClaimV1, RemoteAgentDataPlaneTerminalAuthClaimV2,
    RemoteAgentDataPlaneTerminalReceiptV1, RemoteAgentDataPlaneTerminalReceiptV2,
    RemoteAgentRetainedS0CasV2,
    RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1,
    RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV2,
};
use crate::wire::{
    ApplyAuthAlgorithm, ApplyAuthError, ApplyAuthKeyRef, ApplyRequestAuthClaim,
    ApplyRequestAuthentication, MAX_APPLY_AUTH_NONCE_BYTES,
};

/// Exact independent Controller request magic.
pub const REMOTE_AGENT_ACCESS_REQUEST_MAGIC: &[u8; 4] = b"PXRA";
/// Exact independent Runtime response magic.
pub const REMOTE_AGENT_ACCESS_RESPONSE_MAGIC: &[u8; 4] = b"PXRR";
const REQUEST_TRANSCRIPT_MAGIC: &[u8] = b"ParaEGOX\0remote-agent-access-request-signing";
const RESPONSE_TRANSCRIPT_MAGIC: &[u8] = b"ParaEGOX\0remote-agent-access-response-signing";
const REQUEST_PAYLOAD_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-access.request-payload.sha256.v1";
const RESPONSE_PAYLOAD_DIGEST_DOMAIN: &[u8] =
    b"paraegox.runtime.remote-agent-access.response-payload.sha256.v1";
const REQUEST_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-access.request.sha256.v1";
const RESPONSE_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-access.response.sha256.v1";
const REQUEST_FIXED_BYTES: usize = 304;
const RESPONSE_FIXED_BYTES: usize = 430;
const RESTRICTED_CARRIER_ED25519_ALGORITHM: u16 = 1;
const RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION: u16 = 1;
const RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES: usize = 64;

/// Exact PXRA/PXRR protocol version.
pub const REMOTE_AGENT_ACCESS_VERSION: u16 = 1;
/// Exact Controller signing-transcript version.
pub const REMOTE_AGENT_ACCESS_REQUEST_SIGNING_VERSION: u16 = 1;
/// Exact Runtime signing-transcript version.
pub const REMOTE_AGENT_ACCESS_RESPONSE_SIGNING_VERSION: u16 = 1;
/// Maximum opaque PXAP bytes retained by Describe.
pub const MAX_REMOTE_AGENT_ACCESS_DESCRIPTOR_BYTES: usize = MAX_RUNTIME_AGENT_PORT_DESCRIPTOR_BYTES;
/// Maximum canonical PXRA request size.
pub const MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES: usize = REQUEST_FIXED_BYTES
    + MAX_APPLY_AUTH_NONCE_BYTES
    + MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
    + MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES
    + RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES;
/// Maximum canonical PXRR response size.
pub const MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES: usize = RESPONSE_FIXED_BYTES
    + MAX_APPLY_AUTH_NONCE_BYTES
    + MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
    + MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES
    + MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES
    + MAX_REMOTE_AGENT_ACCESS_DESCRIPTOR_BYTES
    + RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES;

/// Nonzero identity of one PXRA invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RemoteAgentAccessRequestIdV1([u8; 16]);

impl RemoteAgentAccessRequestIdV1 {
    pub const fn try_from_bytes(bytes: [u8; 16]) -> Result<Self, RemoteAgentAccessError> {
        if bytes_are_zero(&bytes) {
            return Err(RemoteAgentAccessError::InvalidIdentity);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Operation admitted by PXRA v1.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u16)]
pub enum RemoteAgentAccessKindV1 {
    ApplyRemoteAccess = 1,
    DescribeRemoteAccess = 2,
}

impl RemoteAgentAccessKindV1 {
    fn decode(value: u16) -> Result<Self, RemoteAgentAccessError> {
        match value {
            1 => Ok(Self::ApplyRemoteAccess),
            2 => Ok(Self::DescribeRemoteAccess),
            _ => Err(RemoteAgentAccessError::UnsupportedKind),
        }
    }
}

/// Common target, carrier, epoch, and Controller-authentication inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessRequestFieldsV1 {
    pub request_id: RemoteAgentAccessRequestIdV1,
    pub carrier: RestrictedRuntimeApplyCarrierBindingV1,
    pub target: RuntimeHostId,
    pub expected_runtime_store_instance_id: [u8; 32],
    pub expected_runtime_host_epoch: u64,
    pub auth_claim: ApplyRequestAuthClaim,
}

/// Exact domain-separated bytes passed to a Controller or Runtime signer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessSigningTranscriptV1(Box<[u8]>);

impl RemoteAgentAccessSigningTranscriptV1 {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Signature-independent PXRA producer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessRequestDraftV1 {
    request_id: RemoteAgentAccessRequestIdV1,
    kind: RemoteAgentAccessKindV1,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    target: RuntimeHostId,
    expected_runtime_store_instance_id: [u8; 32],
    expected_runtime_host_epoch: u64,
    expected_pxau_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    profile_digest: Digest32,
    intended_mac_agent_client: PrincipalRef,
    apply_request: Option<RemoteAgentDataPlaneApplyRequestV1>,
    payload_wire_digest: Digest32,
    auth_claim: ApplyRequestAuthClaim,
}

impl RemoteAgentAccessRequestDraftV1 {
    /// Wraps one byte-identical PXAR v10 and derives all plan-owned cross-pins.
    pub fn try_apply_remote_access(
        fields: RemoteAgentAccessRequestFieldsV1,
        expected_active_pxst_digest: Digest32,
        request: RemoteAgentDataPlaneApplyRequestV1,
    ) -> Result<Self, RemoteAgentAccessError> {
        let profile = request.target_execution().profile();
        Self::try_new(
            fields,
            RemoteAgentAccessKindV1::ApplyRemoteAccess,
            Digest32::from_bytes([0; 32]),
            expected_active_pxst_digest,
            profile.profile_digest(),
            profile.mac_agent_client_principal(),
            Some(request),
        )
    }

    /// Builds a payload-free request for one exact PXAU/PXST/profile/client root.
    pub fn try_describe_remote_access(
        fields: RemoteAgentAccessRequestFieldsV1,
        expected_pxau_digest: Digest32,
        expected_active_pxst_digest: Digest32,
        profile_digest: Digest32,
        intended_mac_agent_client: PrincipalRef,
    ) -> Result<Self, RemoteAgentAccessError> {
        Self::try_new(
            fields,
            RemoteAgentAccessKindV1::DescribeRemoteAccess,
            expected_pxau_digest,
            expected_active_pxst_digest,
            profile_digest,
            intended_mac_agent_client,
            None,
        )
    }

    fn try_new(
        fields: RemoteAgentAccessRequestFieldsV1,
        kind: RemoteAgentAccessKindV1,
        expected_pxau_digest: Digest32,
        expected_active_pxst_digest: Digest32,
        profile_digest: Digest32,
        intended_mac_agent_client: PrincipalRef,
        apply_request: Option<RemoteAgentDataPlaneApplyRequestV1>,
    ) -> Result<Self, RemoteAgentAccessError> {
        validate_request_fields(
            &fields,
            kind,
            expected_pxau_digest,
            expected_active_pxst_digest,
            profile_digest,
            intended_mac_agent_client,
            apply_request.as_ref(),
        )?;
        let payload_wire_digest = apply_request.as_ref().map_or_else(
            || Ok(Digest32::from_bytes([0; 32])),
            |request| digest(REQUEST_PAYLOAD_DIGEST_DOMAIN, request.canonical_wire()),
        )?;
        Ok(Self {
            request_id: fields.request_id,
            kind,
            carrier: fields.carrier,
            target: fields.target,
            expected_runtime_store_instance_id: fields.expected_runtime_store_instance_id,
            expected_runtime_host_epoch: fields.expected_runtime_host_epoch,
            expected_pxau_digest,
            expected_active_pxst_digest,
            profile_digest,
            intended_mac_agent_client,
            apply_request,
            payload_wire_digest,
            auth_claim: fields.auth_claim,
        })
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV1, RemoteAgentAccessError> {
        let mut wire = build_request_base(
            self,
            REQUEST_TRANSCRIPT_MAGIC,
            REMOTE_AGENT_ACCESS_REQUEST_SIGNING_VERSION,
        )?;
        append_request_values(&mut wire, self);
        Ok(RemoteAgentAccessSigningTranscriptV1(
            wire.into_boxed_slice(),
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentAccessRequestV1, RemoteAgentAccessError> {
        if signature.len() != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES {
            return Err(RemoteAgentAccessError::InvalidRequestAuthentication);
        }
        let authentication =
            ApplyRequestAuthentication::try_new(self.auth_claim.clone(), signature)?;
        RemoteAgentAccessRequestV1::try_new(self, authentication)
    }
}

/// Strict Controller-signed PXRA request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessRequestV1 {
    request_id: RemoteAgentAccessRequestIdV1,
    kind: RemoteAgentAccessKindV1,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    target: RuntimeHostId,
    expected_runtime_store_instance_id: [u8; 32],
    expected_runtime_host_epoch: u64,
    expected_pxau_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    profile_digest: Digest32,
    intended_mac_agent_client: PrincipalRef,
    apply_request: Option<RemoteAgentDataPlaneApplyRequestV1>,
    payload_wire_digest: Digest32,
    authentication: ApplyRequestAuthentication,
    canonical_wire: Box<[u8]>,
    request_digest: Digest32,
}

impl RemoteAgentAccessRequestV1 {
    fn try_new(
        draft: RemoteAgentAccessRequestDraftV1,
        authentication: ApplyRequestAuthentication,
    ) -> Result<Self, RemoteAgentAccessError> {
        if authentication.claim() != &draft.auth_claim {
            return Err(RemoteAgentAccessError::AuthenticationMismatch);
        }
        let canonical_wire = build_request_wire(&draft, &authentication)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        let request_digest = digest(REQUEST_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            request_id: draft.request_id,
            kind: draft.kind,
            carrier: draft.carrier,
            target: draft.target,
            expected_runtime_store_instance_id: draft.expected_runtime_store_instance_id,
            expected_runtime_host_epoch: draft.expected_runtime_host_epoch,
            expected_pxau_digest: draft.expected_pxau_digest,
            expected_active_pxst_digest: draft.expected_active_pxst_digest,
            profile_digest: draft.profile_digest,
            intended_mac_agent_client: draft.intended_mac_agent_client,
            apply_request: draft.apply_request,
            payload_wire_digest: draft.payload_wire_digest,
            authentication,
            canonical_wire: canonical_wire.into_boxed_slice(),
            request_digest,
        })
    }

    /// Strictly decodes PXRA v1. PXAG/PXAH, PXCC/PXDR, and PXAR fail closed.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentAccessError> {
        if frame.len() > MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        if frame.len() < REQUEST_FIXED_BYTES {
            return Err(RemoteAgentAccessError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *REMOTE_AGENT_ACCESS_REQUEST_MAGIC
            || cursor.u16()? != REMOTE_AGENT_ACCESS_VERSION
        {
            return Err(RemoteAgentAccessError::UnsupportedWire);
        }
        let kind = RemoteAgentAccessKindV1::decode(cursor.u16()?)?;
        if cursor.u16()? != 0 {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        let carrier_length = cursor.usize_u16()?;
        let payload_length = cursor.usize_u32()?;
        let request_id = RemoteAgentAccessRequestIdV1::try_from_bytes(cursor.array()?)?;
        let carrier_digest = Digest32::from_bytes(cursor.array()?);
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let expected_runtime_store_instance_id = cursor.array()?;
        let expected_runtime_host_epoch = cursor.u64()?;
        let expected_pxau_digest = Digest32::from_bytes(cursor.array()?);
        let expected_active_pxst_digest = Digest32::from_bytes(cursor.array()?);
        let profile_digest = Digest32::from_bytes(cursor.array()?);
        let intended_mac_agent_client = PrincipalRef::from_bytes(cursor.array()?);
        let payload_wire_digest = Digest32::from_bytes(cursor.array()?);
        let auth_claim = decode_request_claim(&mut cursor)?;
        let signature_length = cursor.usize_u16()?;
        validate_request_lengths(kind, carrier_length, payload_length, signature_length)?;
        let carrier = RestrictedRuntimeApplyCarrierBindingV1::decode(cursor.take(carrier_length)?)
            .map_err(|_| RemoteAgentAccessError::InvalidCarrierBinding)?;
        if carrier.binding_digest() != carrier_digest {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let payload = cursor.take(payload_length)?;
        let apply_request = match kind {
            RemoteAgentAccessKindV1::ApplyRemoteAccess => Some(
                RemoteAgentDataPlaneApplyRequestV1::decode(payload)
                    .map_err(|_| RemoteAgentAccessError::InvalidPayload)?,
            ),
            RemoteAgentAccessKindV1::DescribeRemoteAccess => None,
        };
        if (payload.is_empty() && !digest_is_zero(payload_wire_digest))
            || (!payload.is_empty()
                && digest(REQUEST_PAYLOAD_DIGEST_DOMAIN, payload)? != payload_wire_digest)
        {
            return Err(RemoteAgentAccessError::InvalidPayload);
        }
        let signature = cursor.take(signature_length)?;
        cursor.finish()?;
        let fields = RemoteAgentAccessRequestFieldsV1 {
            request_id,
            carrier,
            target,
            expected_runtime_store_instance_id,
            expected_runtime_host_epoch,
            auth_claim,
        };
        let draft = RemoteAgentAccessRequestDraftV1::try_new(
            fields,
            kind,
            expected_pxau_digest,
            expected_active_pxst_digest,
            profile_digest,
            intended_mac_agent_client,
            apply_request,
        )?;
        if draft.payload_wire_digest != payload_wire_digest {
            return Err(RemoteAgentAccessError::InvalidPayload);
        }
        let decoded = draft.finalize(signature)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    /// Verifies only the outer Controller signature and exact PXCB selection.
    /// The embedded PXAR v10 retains its independent authentication owner.
    pub fn verify_controller_request<Verify>(
        &self,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<ControllerAuthenticatedRemoteAgentAccessRequestV1<'_>, RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        if &self.carrier != expected_carrier {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let transcript = self.signing_transcript()?;
        if !verify(
            self.carrier.controller_principal(),
            self.carrier.controller_request_key(),
            self.carrier.controller_request_key_fingerprint(),
            transcript.as_bytes(),
            self.authentication.signature(),
        ) {
            return Err(RemoteAgentAccessError::InvalidRequestAuthentication);
        }
        Ok(ControllerAuthenticatedRemoteAgentAccessRequestV1 { request: self })
    }

    #[must_use]
    pub const fn request_id(&self) -> RemoteAgentAccessRequestIdV1 {
        self.request_id
    }
    #[must_use]
    pub const fn kind(&self) -> RemoteAgentAccessKindV1 {
        self.kind
    }
    #[must_use]
    pub const fn carrier(&self) -> &RestrictedRuntimeApplyCarrierBindingV1 {
        &self.carrier
    }
    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.target
    }
    #[must_use]
    pub const fn expected_runtime_store_instance_id(&self) -> [u8; 32] {
        self.expected_runtime_store_instance_id
    }
    #[must_use]
    pub const fn expected_runtime_host_epoch(&self) -> u64 {
        self.expected_runtime_host_epoch
    }
    #[must_use]
    pub const fn expected_pxau_digest(&self) -> Digest32 {
        self.expected_pxau_digest
    }
    #[must_use]
    pub const fn expected_active_pxst_digest(&self) -> Digest32 {
        self.expected_active_pxst_digest
    }
    #[must_use]
    pub const fn profile_digest(&self) -> Digest32 {
        self.profile_digest
    }
    #[must_use]
    pub const fn intended_mac_agent_client(&self) -> PrincipalRef {
        self.intended_mac_agent_client
    }
    #[must_use]
    pub const fn apply_request(&self) -> Option<&RemoteAgentDataPlaneApplyRequestV1> {
        self.apply_request.as_ref()
    }
    #[must_use]
    pub const fn payload_wire_digest(&self) -> Digest32 {
        self.payload_wire_digest
    }
    #[must_use]
    pub const fn authentication(&self) -> &ApplyRequestAuthentication {
        &self.authentication
    }
    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV1, RemoteAgentAccessError> {
        self.as_draft().signing_transcript()
    }

    fn as_draft(&self) -> RemoteAgentAccessRequestDraftV1 {
        RemoteAgentAccessRequestDraftV1 {
            request_id: self.request_id,
            kind: self.kind,
            carrier: self.carrier.clone(),
            target: self.target,
            expected_runtime_store_instance_id: self.expected_runtime_store_instance_id,
            expected_runtime_host_epoch: self.expected_runtime_host_epoch,
            expected_pxau_digest: self.expected_pxau_digest,
            expected_active_pxst_digest: self.expected_active_pxst_digest,
            profile_digest: self.profile_digest,
            intended_mac_agent_client: self.intended_mac_agent_client,
            apply_request: self.apply_request.clone(),
            payload_wire_digest: self.payload_wire_digest,
            auth_claim: self.authentication.claim().clone(),
        }
    }
}

/// Marker issued only after PXRA Controller authentication and PXCB matching.
#[derive(Clone, Copy, Debug)]
pub struct ControllerAuthenticatedRemoteAgentAccessRequestV1<'a> {
    request: &'a RemoteAgentAccessRequestV1,
}

impl<'a> ControllerAuthenticatedRemoteAgentAccessRequestV1<'a> {
    #[must_use]
    pub const fn request(self) -> &'a RemoteAgentAccessRequestV1 {
        self.request
    }
    #[must_use]
    pub const fn kind(self) -> RemoteAgentAccessKindV1 {
        self.request.kind()
    }
}

/// Runtime response signer bound to the exact public PXCB.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RemoteAgentAccessResponseAuthClaimV1 {
    runtime_principal: PrincipalRef,
    key: ApplyAuthKeyRef,
    algorithm: ApplyAuthAlgorithm,
    algorithm_version: u16,
    carrier_binding_digest: Digest32,
}

impl RemoteAgentAccessResponseAuthClaimV1 {
    pub fn try_new(
        carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        key: ApplyAuthKeyRef,
        algorithm: ApplyAuthAlgorithm,
        algorithm_version: u16,
    ) -> Result<Self, RemoteAgentAccessError> {
        let claim = Self {
            runtime_principal: carrier.runtime_principal(),
            key,
            algorithm,
            algorithm_version,
            carrier_binding_digest: carrier.binding_digest(),
        };
        validate_response_auth(claim, carrier)?;
        Ok(claim)
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
    #[must_use]
    pub const fn carrier_binding_digest(self) -> Digest32 {
        self.carrier_binding_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RemoteAgentAccessResponsePayloadV1 {
    Apply(Box<RemoteAgentDataPlaneTerminalReceiptV1>),
    Describe {
        profile: Box<RemoteAgentDataPlaneProfileV1>,
        descriptor: Box<[u8]>,
    },
}

/// Signature-independent Runtime PXRR producer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessResponseDraftV1 {
    request_id: RemoteAgentAccessRequestIdV1,
    request_digest: Digest32,
    request_nonce: Box<[u8]>,
    kind: RemoteAgentAccessKindV1,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    target: RuntimeHostId,
    runtime_store_instance_id: [u8; 32],
    runtime_host_epoch: u64,
    expected_pxau_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    profile_digest: Digest32,
    intended_mac_agent_client: PrincipalRef,
    payload: RemoteAgentAccessResponsePayloadV1,
    payload_wire_digest: Digest32,
    descriptor_digest: Digest32,
    fabric_generation: Option<ManagedServiceGeneration>,
    agent_generation: Option<ManagedServiceGeneration>,
    access_generation: Option<ManagedServiceGeneration>,
    auth_claim: RemoteAgentAccessResponseAuthClaimV1,
}

impl RemoteAgentAccessResponseDraftV1 {
    /// Wraps exact PXAU only after both outer Controller and inner Runtime markers exist.
    pub fn try_apply_remote_access(
        authenticated_request: ControllerAuthenticatedRemoteAgentAccessRequestV1<'_>,
        authenticated_terminal: RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV1<'_>,
        auth_claim: RemoteAgentAccessResponseAuthClaimV1,
    ) -> Result<Self, RemoteAgentAccessError> {
        let request = authenticated_request.request();
        let inner = request
            .apply_request()
            .ok_or(RemoteAgentAccessError::InvalidResponse)?;
        let receipt = authenticated_terminal.receipt();
        receipt.validate_against_request(inner)?;
        if receipt.authentication().runtime_principal() != request.carrier().runtime_principal()
            || receipt
                .facts()
                .evidence()
                .fields()
                .completion_runtime_host_epoch
                != request.expected_runtime_host_epoch()
        {
            return Err(RemoteAgentAccessError::InvalidResponse);
        }
        Self::try_new(
            request,
            RemoteAgentAccessResponsePayloadV1::Apply(Box::new(receipt.clone())),
            None,
            None,
            None,
            auth_claim,
        )
    }

    /// Carries caller-supplied Runtime-signed structural facts and exact
    /// PXAD/PXAP bytes; the codec asserts neither liveness nor freshness.
    pub fn try_describe_remote_access(
        authenticated_request: ControllerAuthenticatedRemoteAgentAccessRequestV1<'_>,
        profile: RemoteAgentDataPlaneProfileV1,
        descriptor: &[u8],
        fabric_generation: ManagedServiceGeneration,
        agent_generation: ManagedServiceGeneration,
        access_generation: ManagedServiceGeneration,
        auth_claim: RemoteAgentAccessResponseAuthClaimV1,
    ) -> Result<Self, RemoteAgentAccessError> {
        let request = authenticated_request.request();
        Self::try_new(
            request,
            RemoteAgentAccessResponsePayloadV1::Describe {
                profile: Box::new(profile),
                descriptor: descriptor.into(),
            },
            Some(fabric_generation),
            Some(agent_generation),
            Some(access_generation),
            auth_claim,
        )
    }

    fn try_new(
        request: &RemoteAgentAccessRequestV1,
        payload: RemoteAgentAccessResponsePayloadV1,
        fabric_generation: Option<ManagedServiceGeneration>,
        agent_generation: Option<ManagedServiceGeneration>,
        access_generation: Option<ManagedServiceGeneration>,
        auth_claim: RemoteAgentAccessResponseAuthClaimV1,
    ) -> Result<Self, RemoteAgentAccessError> {
        let descriptor_digest = match &payload {
            RemoteAgentAccessResponsePayloadV1::Describe { descriptor, .. } => {
                runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                    .map_err(|_| RemoteAgentAccessError::InvalidDescriptor)?
            }
            RemoteAgentAccessResponsePayloadV1::Apply(_) => Digest32::from_bytes([0; 32]),
        };
        let payload_wire_digest = digest_response_payload(&payload)?;
        let draft = Self {
            request_id: request.request_id,
            request_digest: request.request_digest,
            request_nonce: request.authentication.claim().nonce().into(),
            kind: request.kind,
            carrier: request.carrier.clone(),
            target: request.target,
            runtime_store_instance_id: request.expected_runtime_store_instance_id,
            runtime_host_epoch: request.expected_runtime_host_epoch,
            expected_pxau_digest: request.expected_pxau_digest,
            expected_active_pxst_digest: request.expected_active_pxst_digest,
            profile_digest: request.profile_digest,
            intended_mac_agent_client: request.intended_mac_agent_client,
            payload,
            payload_wire_digest,
            descriptor_digest,
            fabric_generation,
            agent_generation,
            access_generation,
            auth_claim,
        };
        validate_response_draft(&draft)?;
        validate_response_against_request(&draft, request)?;
        Ok(draft)
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV1, RemoteAgentAccessError> {
        let mut wire = build_response_base(
            self,
            RESPONSE_TRANSCRIPT_MAGIC,
            REMOTE_AGENT_ACCESS_RESPONSE_SIGNING_VERSION,
        )?;
        append_response_values(&mut wire, self);
        Ok(RemoteAgentAccessSigningTranscriptV1(
            wire.into_boxed_slice(),
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentAccessResponseV1, RemoteAgentAccessError> {
        if signature.len() != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES {
            return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
        }
        RemoteAgentAccessResponseV1::try_new(self, signature)
    }
}

/// Strict independently Runtime-signed PXRR response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessResponseV1 {
    draft: RemoteAgentAccessResponseDraftV1,
    signature: Box<[u8]>,
    canonical_wire: Box<[u8]>,
    response_digest: Digest32,
}

impl RemoteAgentAccessResponseV1 {
    fn try_new(
        draft: RemoteAgentAccessResponseDraftV1,
        signature: &[u8],
    ) -> Result<Self, RemoteAgentAccessError> {
        let canonical_wire = build_response_wire(&draft, signature)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        let response_digest = digest(RESPONSE_DIGEST_DOMAIN, &canonical_wire)?;
        Ok(Self {
            draft,
            signature: signature.into(),
            canonical_wire: canonical_wire.into_boxed_slice(),
            response_digest,
        })
    }

    /// Strictly decodes PXRR v1. PXAG/PXAH, PXCC/PXDR, and PXAU fail closed.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentAccessError> {
        if frame.len() > MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        if frame.len() < RESPONSE_FIXED_BYTES {
            return Err(RemoteAgentAccessError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *REMOTE_AGENT_ACCESS_RESPONSE_MAGIC
            || cursor.u16()? != REMOTE_AGENT_ACCESS_VERSION
        {
            return Err(RemoteAgentAccessError::UnsupportedWire);
        }
        let kind = RemoteAgentAccessKindV1::decode(cursor.u16()?)?;
        if cursor.u16()? != 0 {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        let carrier_length = cursor.usize_u16()?;
        let payload_length = cursor.usize_u32()?;
        let profile_length = cursor.usize_u16()?;
        let descriptor_length = cursor.usize_u32()?;
        let nonce_length = cursor.usize_u16()?;
        let request_id = RemoteAgentAccessRequestIdV1::try_from_bytes(cursor.array()?)?;
        let request_digest = Digest32::from_bytes(cursor.array()?);
        let carrier_digest = Digest32::from_bytes(cursor.array()?);
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let runtime_store_instance_id = cursor.array()?;
        let runtime_host_epoch = cursor.u64()?;
        let expected_pxau_digest = Digest32::from_bytes(cursor.array()?);
        let expected_active_pxst_digest = Digest32::from_bytes(cursor.array()?);
        let profile_digest = Digest32::from_bytes(cursor.array()?);
        let intended_mac_agent_client = PrincipalRef::from_bytes(cursor.array()?);
        let payload_wire_digest = Digest32::from_bytes(cursor.array()?);
        let descriptor_digest = Digest32::from_bytes(cursor.array()?);
        let fabric_generation = decode_generation(cursor.u64()?)?;
        let agent_generation = decode_generation(cursor.u64()?)?;
        let access_generation = decode_generation(cursor.u64()?)?;
        let auth_claim = decode_response_auth(&mut cursor)?;
        let signature_length = cursor.usize_u16()?;
        validate_response_lengths(
            kind,
            carrier_length,
            payload_length,
            profile_length,
            descriptor_length,
            nonce_length,
            signature_length,
        )?;
        let request_nonce: Box<[u8]> = cursor.take(nonce_length)?.into();
        let carrier = RestrictedRuntimeApplyCarrierBindingV1::decode(cursor.take(carrier_length)?)
            .map_err(|_| RemoteAgentAccessError::InvalidCarrierBinding)?;
        if carrier.binding_digest() != carrier_digest {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let payload = match kind {
            RemoteAgentAccessKindV1::ApplyRemoteAccess => {
                let receipt =
                    RemoteAgentDataPlaneTerminalReceiptV1::decode(cursor.take(payload_length)?)
                        .map_err(|_| RemoteAgentAccessError::InvalidResponsePayload)?;
                RemoteAgentAccessResponsePayloadV1::Apply(Box::new(receipt))
            }
            RemoteAgentAccessKindV1::DescribeRemoteAccess => {
                let profile = RemoteAgentDataPlaneProfileV1::decode(cursor.take(profile_length)?)
                    .map_err(|_| RemoteAgentAccessError::InvalidResponsePayload)?;
                let descriptor = cursor.take(descriptor_length)?;
                runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                    .map_err(|_| RemoteAgentAccessError::InvalidDescriptor)?;
                RemoteAgentAccessResponsePayloadV1::Describe {
                    profile: Box::new(profile),
                    descriptor: descriptor.into(),
                }
            }
        };
        if digest_response_payload(&payload)? != payload_wire_digest {
            return Err(RemoteAgentAccessError::InvalidResponsePayload);
        }
        let signature = cursor.take(signature_length)?;
        cursor.finish()?;
        let draft = RemoteAgentAccessResponseDraftV1 {
            request_id,
            request_digest,
            request_nonce,
            kind,
            carrier,
            target,
            runtime_store_instance_id,
            runtime_host_epoch,
            expected_pxau_digest,
            expected_active_pxst_digest,
            profile_digest,
            intended_mac_agent_client,
            payload,
            payload_wire_digest,
            descriptor_digest,
            fabric_generation,
            agent_generation,
            access_generation,
            auth_claim,
        };
        validate_response_draft(&draft)?;
        let decoded = draft.finalize(signature)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    /// Verifies exact correlation plus separate inner PXAU and outer PXRR signatures.
    pub fn verify_runtime_apply_response<'a, VerifyInner, VerifyOuter>(
        &'a self,
        request: &RemoteAgentAccessRequestV1,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        expected_inner_auth: RemoteAgentDataPlaneTerminalAuthClaimV1,
        verify_inner: VerifyInner,
        verify_outer: VerifyOuter,
    ) -> Result<RuntimeAuthenticatedRemoteAgentAccessResponseV1<'a>, RemoteAgentAccessError>
    where
        VerifyInner:
            FnOnce(PrincipalRef, ApplyAuthKeyRef, ApplyAuthAlgorithm, u16, &[u8], &[u8]) -> bool,
        VerifyOuter: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        self.validate_against_request(request)?;
        let inner_request = request
            .apply_request()
            .ok_or(RemoteAgentAccessError::CorrelationMismatch)?;
        let receipt = self
            .apply_receipt()
            .ok_or(RemoteAgentAccessError::CorrelationMismatch)?;
        if receipt.authentication().runtime_principal() != self.draft.carrier.runtime_principal()
            || receipt
                .facts()
                .evidence()
                .fields()
                .completion_runtime_host_epoch
                != self.draft.runtime_host_epoch
        {
            return Err(RemoteAgentAccessError::CorrelationMismatch);
        }
        receipt.verify_runtime_terminal(inner_request, expected_inner_auth, verify_inner)?;
        self.verify_outer(expected_carrier, verify_outer)?;
        Ok(RuntimeAuthenticatedRemoteAgentAccessResponseV1 { response: self })
    }

    /// Verifies exact Describe correlation and the independent outer PXRR signature.
    pub fn verify_runtime_describe_response<'a, Verify>(
        &'a self,
        request: &RemoteAgentAccessRequestV1,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<RuntimeAuthenticatedRemoteAgentAccessResponseV1<'a>, RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        self.validate_against_request(request)?;
        if self.draft.kind != RemoteAgentAccessKindV1::DescribeRemoteAccess {
            return Err(RemoteAgentAccessError::CorrelationMismatch);
        }
        self.verify_outer(expected_carrier, verify)?;
        Ok(RuntimeAuthenticatedRemoteAgentAccessResponseV1 { response: self })
    }

    fn verify_outer<Verify>(
        &self,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<(), RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        if &self.draft.carrier != expected_carrier {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let transcript = self.signing_transcript()?;
        if !verify(
            self.draft.carrier.runtime_principal(),
            self.draft.carrier.runtime_response_key(),
            self.draft.carrier.runtime_response_key_fingerprint(),
            transcript.as_bytes(),
            &self.signature,
        ) {
            return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
        }
        Ok(())
    }

    pub fn validate_against_request(
        &self,
        request: &RemoteAgentAccessRequestV1,
    ) -> Result<(), RemoteAgentAccessError> {
        validate_response_against_request(&self.draft, request)
    }

    #[must_use]
    pub const fn request_id(&self) -> RemoteAgentAccessRequestIdV1 {
        self.draft.request_id
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.draft.request_digest
    }
    #[must_use]
    pub fn request_nonce(&self) -> &[u8] {
        &self.draft.request_nonce
    }
    #[must_use]
    pub const fn kind(&self) -> RemoteAgentAccessKindV1 {
        self.draft.kind
    }
    #[must_use]
    pub const fn carrier(&self) -> &RestrictedRuntimeApplyCarrierBindingV1 {
        &self.draft.carrier
    }
    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.draft.target
    }
    #[must_use]
    pub const fn runtime_store_instance_id(&self) -> [u8; 32] {
        self.draft.runtime_store_instance_id
    }
    #[must_use]
    pub const fn runtime_host_epoch(&self) -> u64 {
        self.draft.runtime_host_epoch
    }
    #[must_use]
    pub const fn expected_pxau_digest(&self) -> Digest32 {
        self.draft.expected_pxau_digest
    }
    #[must_use]
    pub const fn expected_active_pxst_digest(&self) -> Digest32 {
        self.draft.expected_active_pxst_digest
    }
    #[must_use]
    pub const fn profile_digest(&self) -> Digest32 {
        self.draft.profile_digest
    }
    #[must_use]
    pub const fn intended_mac_agent_client(&self) -> PrincipalRef {
        self.draft.intended_mac_agent_client
    }
    #[must_use]
    pub fn apply_receipt(&self) -> Option<&RemoteAgentDataPlaneTerminalReceiptV1> {
        match &self.draft.payload {
            RemoteAgentAccessResponsePayloadV1::Apply(value) => Some(value),
            _ => None,
        }
    }
    #[must_use]
    pub fn profile(&self) -> Option<&RemoteAgentDataPlaneProfileV1> {
        match &self.draft.payload {
            RemoteAgentAccessResponsePayloadV1::Describe { profile, .. } => Some(profile),
            _ => None,
        }
    }
    #[must_use]
    pub fn descriptor(&self) -> Option<&[u8]> {
        match &self.draft.payload {
            RemoteAgentAccessResponsePayloadV1::Describe { descriptor, .. } => Some(descriptor),
            _ => None,
        }
    }
    #[must_use]
    pub const fn payload_wire_digest(&self) -> Digest32 {
        self.draft.payload_wire_digest
    }
    #[must_use]
    pub const fn descriptor_digest(&self) -> Digest32 {
        self.draft.descriptor_digest
    }
    #[must_use]
    pub const fn fabric_generation(&self) -> Option<ManagedServiceGeneration> {
        self.draft.fabric_generation
    }
    #[must_use]
    pub const fn agent_generation(&self) -> Option<ManagedServiceGeneration> {
        self.draft.agent_generation
    }
    #[must_use]
    pub const fn access_generation(&self) -> Option<ManagedServiceGeneration> {
        self.draft.access_generation
    }
    #[must_use]
    pub const fn authentication(&self) -> RemoteAgentAccessResponseAuthClaimV1 {
        self.draft.auth_claim
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
    pub const fn response_digest(&self) -> Digest32 {
        self.response_digest
    }
    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV1, RemoteAgentAccessError> {
        self.draft.signing_transcript()
    }
}

/// Marker issued after exact PXRR correlation and Runtime signature checks.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeAuthenticatedRemoteAgentAccessResponseV1<'a> {
    response: &'a RemoteAgentAccessResponseV1,
}

impl<'a> RuntimeAuthenticatedRemoteAgentAccessResponseV1<'a> {
    #[must_use]
    pub const fn response(self) -> &'a RemoteAgentAccessResponseV1 {
        self.response
    }
}

fn validate_request_fields(
    fields: &RemoteAgentAccessRequestFieldsV1,
    kind: RemoteAgentAccessKindV1,
    expected_pxau_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    profile_digest: Digest32,
    intended_client: PrincipalRef,
    apply_request: Option<&RemoteAgentDataPlaneApplyRequestV1>,
) -> Result<(), RemoteAgentAccessError> {
    if bytes_are_zero(fields.request_id.as_bytes())
        || bytes_are_zero(fields.target.as_bytes())
        || bytes_are_zero(&fields.expected_runtime_store_instance_id)
        || fields.expected_runtime_host_epoch == 0
        || fields.carrier.target() != fields.target
        || fields.auth_claim.principal() != fields.carrier.controller_principal()
        || fields.auth_claim.key() != fields.carrier.controller_request_key()
        || fields.auth_claim.algorithm().value() != RESTRICTED_CARRIER_ED25519_ALGORITHM
        || fields.auth_claim.algorithm_version() != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
        || fields.auth_claim.nonce().is_empty()
        || fields.auth_claim.nonce().iter().all(|byte| *byte == 0)
        || digest_is_zero(expected_active_pxst_digest)
        || digest_is_zero(profile_digest)
        || bytes_are_zero(intended_client.as_bytes())
        || intended_client == fields.carrier.controller_principal()
        || intended_client == fields.carrier.runtime_principal()
    {
        return Err(RemoteAgentAccessError::InvalidRequest);
    }
    match (kind, apply_request) {
        (RemoteAgentAccessKindV1::ApplyRemoteAccess, Some(request)) => {
            let execution = request.target_execution();
            let profile = execution.profile();
            if !digest_is_zero(expected_pxau_digest)
                || fields.request_id.as_bytes() != request.operation_id().as_bytes()
                || request.target() != fields.target
                || request.expected_runtime_store_instance_id()
                    != fields.expected_runtime_store_instance_id
                || request.authentication().claim().principal()
                    != fields.carrier.controller_principal()
                || request.authentication().claim().key() != fields.carrier.controller_request_key()
                || request.authentication().claim().algorithm().value()
                    != RESTRICTED_CARRIER_ED25519_ALGORITHM
                || request.authentication().claim().algorithm_version()
                    != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
                || request.authentication().signature().len()
                    != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
                || request.authentication().claim().nonce() == fields.auth_claim.nonce()
                || profile.target() != fields.target
                || profile.profile_digest() != profile_digest
                || profile.mac_agent_client_principal() != intended_client
                || profile.mac_agent_client_principal() == fields.carrier.controller_principal()
                || profile.mac_agent_client_principal() == fields.carrier.runtime_principal()
                || profile.ubuntu_agent_listener_principal()
                    == fields.carrier.controller_principal()
                || profile.ubuntu_agent_listener_principal() == fields.carrier.runtime_principal()
                || execution.bootstrap_cas().is_some_and(|cas| {
                    cas.expected_active_pxst_digest() != expected_active_pxst_digest
                })
            {
                return Err(RemoteAgentAccessError::InvalidPayload);
            }
        }
        (RemoteAgentAccessKindV1::DescribeRemoteAccess, None) => {
            if digest_is_zero(expected_pxau_digest) {
                return Err(RemoteAgentAccessError::InvalidPayload);
            }
        }
        _ => return Err(RemoteAgentAccessError::InvalidPayload),
    }
    Ok(())
}

fn validate_request_lengths(
    kind: RemoteAgentAccessKindV1,
    carrier: usize,
    payload: usize,
    signature: usize,
) -> Result<(), RemoteAgentAccessError> {
    if carrier == 0
        || carrier > MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
        || signature != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
        || match kind {
            RemoteAgentAccessKindV1::ApplyRemoteAccess => {
                payload == 0 || payload > MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_BYTES
            }
            RemoteAgentAccessKindV1::DescribeRemoteAccess => payload != 0,
        }
    {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    Ok(())
}

fn build_request_base(
    draft: &RemoteAgentAccessRequestDraftV1,
    magic: &[u8],
    version: u16,
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    let carrier_length = u16::try_from(draft.carrier.canonical_wire().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let payload_length = u32::try_from(
        draft
            .apply_request
            .as_ref()
            .map_or(0, |value| value.canonical_wire().len()),
    )
    .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let nonce_length = u16::try_from(draft.auth_claim.nonce().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(magic);
    wire.extend_from_slice(&version.to_be_bytes());
    wire.extend_from_slice(&(draft.kind as u16).to_be_bytes());
    wire.extend_from_slice(&0_u16.to_be_bytes());
    wire.extend_from_slice(&carrier_length.to_be_bytes());
    wire.extend_from_slice(&payload_length.to_be_bytes());
    wire.extend_from_slice(draft.request_id.as_bytes());
    wire.extend_from_slice(draft.carrier.binding_digest().as_bytes());
    wire.extend_from_slice(draft.target.as_bytes());
    wire.extend_from_slice(&draft.expected_runtime_store_instance_id);
    wire.extend_from_slice(&draft.expected_runtime_host_epoch.to_be_bytes());
    wire.extend_from_slice(draft.expected_pxau_digest.as_bytes());
    wire.extend_from_slice(draft.expected_active_pxst_digest.as_bytes());
    wire.extend_from_slice(draft.profile_digest.as_bytes());
    wire.extend_from_slice(draft.intended_mac_agent_client.as_bytes());
    wire.extend_from_slice(draft.payload_wire_digest.as_bytes());
    encode_request_claim(&mut wire, &draft.auth_claim, nonce_length);
    Ok(wire)
}

fn append_request_values(wire: &mut Vec<u8>, draft: &RemoteAgentAccessRequestDraftV1) {
    wire.extend_from_slice(draft.carrier.canonical_wire());
    if let Some(request) = &draft.apply_request {
        wire.extend_from_slice(request.canonical_wire());
    }
}

fn build_request_wire(
    draft: &RemoteAgentAccessRequestDraftV1,
    authentication: &ApplyRequestAuthentication,
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    let signature_length = u16::try_from(authentication.signature().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = build_request_base(
        draft,
        REMOTE_AGENT_ACCESS_REQUEST_MAGIC,
        REMOTE_AGENT_ACCESS_VERSION,
    )?;
    wire.extend_from_slice(&signature_length.to_be_bytes());
    append_request_values(&mut wire, draft);
    wire.extend_from_slice(authentication.signature());
    Ok(wire)
}

fn validate_response_auth(
    claim: RemoteAgentAccessResponseAuthClaimV1,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> Result<(), RemoteAgentAccessError> {
    if bytes_are_zero(claim.runtime_principal.as_bytes())
        || bytes_are_zero(claim.key.as_bytes())
        || claim.algorithm.value() != RESTRICTED_CARRIER_ED25519_ALGORITHM
        || claim.algorithm_version != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
        || digest_is_zero(claim.carrier_binding_digest)
        || claim.runtime_principal != carrier.runtime_principal()
        || claim.key != carrier.runtime_response_key()
        || claim.carrier_binding_digest != carrier.binding_digest()
    {
        return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
    }
    Ok(())
}

fn validate_response_draft(
    draft: &RemoteAgentAccessResponseDraftV1,
) -> Result<(), RemoteAgentAccessError> {
    if digest_is_zero(draft.request_digest)
        || draft.request_nonce.is_empty()
        || draft.request_nonce.len() > MAX_APPLY_AUTH_NONCE_BYTES
        || draft.request_nonce.iter().all(|byte| *byte == 0)
        || draft.carrier.target() != draft.target
        || bytes_are_zero(draft.target.as_bytes())
        || bytes_are_zero(&draft.runtime_store_instance_id)
        || draft.runtime_host_epoch == 0
        || digest_is_zero(draft.expected_active_pxst_digest)
        || digest_is_zero(draft.profile_digest)
        || bytes_are_zero(draft.intended_mac_agent_client.as_bytes())
        || digest_is_zero(draft.payload_wire_digest)
    {
        return Err(RemoteAgentAccessError::InvalidResponse);
    }
    validate_response_auth(draft.auth_claim, &draft.carrier)?;
    let valid = match (&draft.payload, draft.kind) {
        (
            RemoteAgentAccessResponsePayloadV1::Apply(receipt),
            RemoteAgentAccessKindV1::ApplyRemoteAccess,
        ) => {
            digest_is_zero(draft.expected_pxau_digest)
                && digest_is_zero(draft.descriptor_digest)
                && draft.fabric_generation.is_none()
                && draft.agent_generation.is_none()
                && draft.access_generation.is_none()
                && receipt.facts().target() == draft.target
                && receipt.facts().runtime_store_instance_id() == draft.runtime_store_instance_id
                && receipt.authentication().runtime_principal() == draft.carrier.runtime_principal()
                && receipt
                    .facts()
                    .evidence()
                    .fields()
                    .completion_runtime_host_epoch
                    == draft.runtime_host_epoch
        }
        (
            RemoteAgentAccessResponsePayloadV1::Describe {
                profile,
                descriptor,
            },
            RemoteAgentAccessKindV1::DescribeRemoteAccess,
        ) => {
            !digest_is_zero(draft.expected_pxau_digest)
                && profile.target() == draft.target
                && profile.profile_digest() == draft.profile_digest
                && profile.mac_agent_client_principal() == draft.intended_mac_agent_client
                && profile.mac_agent_client_principal() != draft.carrier.controller_principal()
                && profile.mac_agent_client_principal() != draft.carrier.runtime_principal()
                && profile.ubuntu_agent_listener_principal() != draft.carrier.controller_principal()
                && profile.ubuntu_agent_listener_principal() != draft.carrier.runtime_principal()
                && draft.fabric_generation.is_some()
                && draft.agent_generation.is_some()
                && draft.access_generation.is_some()
                && runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                    .map_err(|_| RemoteAgentAccessError::InvalidDescriptor)?
                    == draft.descriptor_digest
        }
        _ => false,
    };
    if !valid || digest_response_payload(&draft.payload)? != draft.payload_wire_digest {
        return Err(RemoteAgentAccessError::InvalidResponse);
    }
    Ok(())
}

fn validate_response_against_request(
    draft: &RemoteAgentAccessResponseDraftV1,
    request: &RemoteAgentAccessRequestV1,
) -> Result<(), RemoteAgentAccessError> {
    if draft.request_id != request.request_id
        || draft.request_digest != request.request_digest
        || draft.request_nonce.as_ref() != request.authentication.claim().nonce()
        || draft.kind != request.kind
        || draft.carrier != request.carrier
        || draft.target != request.target
        || draft.runtime_store_instance_id != request.expected_runtime_store_instance_id
        || draft.runtime_host_epoch != request.expected_runtime_host_epoch
        || draft.expected_pxau_digest != request.expected_pxau_digest
        || draft.expected_active_pxst_digest != request.expected_active_pxst_digest
        || draft.profile_digest != request.profile_digest
        || draft.intended_mac_agent_client != request.intended_mac_agent_client
    {
        return Err(RemoteAgentAccessError::CorrelationMismatch);
    }
    match (&draft.payload, request.kind) {
        (
            RemoteAgentAccessResponsePayloadV1::Apply(receipt),
            RemoteAgentAccessKindV1::ApplyRemoteAccess,
        ) => {
            let inner = request
                .apply_request
                .as_ref()
                .ok_or(RemoteAgentAccessError::CorrelationMismatch)?;
            receipt
                .validate_against_request(inner)
                .map_err(|_| RemoteAgentAccessError::CorrelationMismatch)?;
            if receipt.authentication().runtime_principal() != request.carrier.runtime_principal()
                || receipt
                    .facts()
                    .evidence()
                    .fields()
                    .completion_runtime_host_epoch
                    != request.expected_runtime_host_epoch
            {
                return Err(RemoteAgentAccessError::CorrelationMismatch);
            }
        }
        (
            RemoteAgentAccessResponsePayloadV1::Describe { .. },
            RemoteAgentAccessKindV1::DescribeRemoteAccess,
        ) if request.apply_request.is_none() => {}
        _ => return Err(RemoteAgentAccessError::CorrelationMismatch),
    }
    Ok(())
}

fn response_payload_lengths(payload: &RemoteAgentAccessResponsePayloadV1) -> (usize, usize, usize) {
    match payload {
        RemoteAgentAccessResponsePayloadV1::Apply(receipt) => {
            (receipt.canonical_wire().len(), 0, 0)
        }
        RemoteAgentAccessResponsePayloadV1::Describe {
            profile,
            descriptor,
        } => (
            profile.canonical_wire().len() + descriptor.len(),
            profile.canonical_wire().len(),
            descriptor.len(),
        ),
    }
}

fn digest_response_payload(
    payload: &RemoteAgentAccessResponsePayloadV1,
) -> Result<Digest32, RemoteAgentAccessError> {
    let mut builder = Digest32Builder::try_new(RESPONSE_PAYLOAD_DIGEST_DOMAIN)?;
    match payload {
        RemoteAgentAccessResponsePayloadV1::Apply(receipt) => {
            builder.field_bytes(receipt.canonical_wire())?;
        }
        RemoteAgentAccessResponsePayloadV1::Describe {
            profile,
            descriptor,
        } => {
            builder.field_bytes(profile.canonical_wire())?;
            builder.field_bytes(descriptor)?;
        }
    }
    Ok(builder.finish())
}

fn build_response_base(
    draft: &RemoteAgentAccessResponseDraftV1,
    magic: &[u8],
    version: u16,
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    validate_response_draft(draft)?;
    let carrier_length = u16::try_from(draft.carrier.canonical_wire().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let (payload_length, profile_length, descriptor_length) =
        response_payload_lengths(&draft.payload);
    let payload_length =
        u32::try_from(payload_length).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let profile_length =
        u16::try_from(profile_length).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let descriptor_length =
        u32::try_from(descriptor_length).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let nonce_length = u16::try_from(draft.request_nonce.len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(magic);
    wire.extend_from_slice(&version.to_be_bytes());
    wire.extend_from_slice(&(draft.kind as u16).to_be_bytes());
    wire.extend_from_slice(&0_u16.to_be_bytes());
    wire.extend_from_slice(&carrier_length.to_be_bytes());
    wire.extend_from_slice(&payload_length.to_be_bytes());
    wire.extend_from_slice(&profile_length.to_be_bytes());
    wire.extend_from_slice(&descriptor_length.to_be_bytes());
    wire.extend_from_slice(&nonce_length.to_be_bytes());
    wire.extend_from_slice(draft.request_id.as_bytes());
    wire.extend_from_slice(draft.request_digest.as_bytes());
    wire.extend_from_slice(draft.carrier.binding_digest().as_bytes());
    wire.extend_from_slice(draft.target.as_bytes());
    wire.extend_from_slice(&draft.runtime_store_instance_id);
    wire.extend_from_slice(&draft.runtime_host_epoch.to_be_bytes());
    wire.extend_from_slice(draft.expected_pxau_digest.as_bytes());
    wire.extend_from_slice(draft.expected_active_pxst_digest.as_bytes());
    wire.extend_from_slice(draft.profile_digest.as_bytes());
    wire.extend_from_slice(draft.intended_mac_agent_client.as_bytes());
    wire.extend_from_slice(draft.payload_wire_digest.as_bytes());
    wire.extend_from_slice(draft.descriptor_digest.as_bytes());
    wire.extend_from_slice(
        &draft
            .fabric_generation
            .map_or(0, ManagedServiceGeneration::value)
            .to_be_bytes(),
    );
    wire.extend_from_slice(
        &draft
            .agent_generation
            .map_or(0, ManagedServiceGeneration::value)
            .to_be_bytes(),
    );
    wire.extend_from_slice(
        &draft
            .access_generation
            .map_or(0, ManagedServiceGeneration::value)
            .to_be_bytes(),
    );
    encode_response_claim(&mut wire, draft.auth_claim);
    Ok(wire)
}

fn append_response_values(wire: &mut Vec<u8>, draft: &RemoteAgentAccessResponseDraftV1) {
    wire.extend_from_slice(&draft.request_nonce);
    wire.extend_from_slice(draft.carrier.canonical_wire());
    match &draft.payload {
        RemoteAgentAccessResponsePayloadV1::Apply(receipt) => {
            wire.extend_from_slice(receipt.canonical_wire())
        }
        RemoteAgentAccessResponsePayloadV1::Describe {
            profile,
            descriptor,
        } => {
            wire.extend_from_slice(profile.canonical_wire());
            wire.extend_from_slice(descriptor);
        }
    }
}

fn build_response_wire(
    draft: &RemoteAgentAccessResponseDraftV1,
    signature: &[u8],
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    let signature_length =
        u16::try_from(signature.len()).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = build_response_base(
        draft,
        REMOTE_AGENT_ACCESS_RESPONSE_MAGIC,
        REMOTE_AGENT_ACCESS_VERSION,
    )?;
    wire.extend_from_slice(&signature_length.to_be_bytes());
    append_response_values(&mut wire, draft);
    wire.extend_from_slice(signature);
    Ok(wire)
}

fn validate_response_lengths(
    kind: RemoteAgentAccessKindV1,
    carrier: usize,
    payload: usize,
    profile: usize,
    descriptor: usize,
    nonce: usize,
    signature: usize,
) -> Result<(), RemoteAgentAccessError> {
    if carrier == 0
        || carrier > MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
        || nonce == 0
        || nonce > MAX_APPLY_AUTH_NONCE_BYTES
        || signature != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
    {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    let valid = match kind {
        RemoteAgentAccessKindV1::ApplyRemoteAccess => {
            payload != 0
                && payload <= MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_BYTES
                && profile == 0
                && descriptor == 0
        }
        RemoteAgentAccessKindV1::DescribeRemoteAccess => {
            profile != 0
                && profile <= MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES
                && (6..=MAX_REMOTE_AGENT_ACCESS_DESCRIPTOR_BYTES).contains(&descriptor)
                && payload == profile + descriptor
        }
    };
    if !valid {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    Ok(())
}

fn encode_request_claim(wire: &mut Vec<u8>, claim: &ApplyRequestAuthClaim, nonce_length: u16) {
    wire.extend_from_slice(claim.principal().as_bytes());
    wire.extend_from_slice(claim.key().as_bytes());
    wire.extend_from_slice(&claim.algorithm().value().to_be_bytes());
    wire.extend_from_slice(&claim.algorithm_version().to_be_bytes());
    wire.extend_from_slice(&nonce_length.to_be_bytes());
    wire.extend_from_slice(claim.nonce());
}

fn decode_request_claim(
    cursor: &mut Cursor<'_>,
) -> Result<ApplyRequestAuthClaim, RemoteAgentAccessError> {
    let principal = PrincipalRef::from_bytes(cursor.array()?);
    let key = ApplyAuthKeyRef::from_bytes(cursor.array()?);
    let algorithm = ApplyAuthAlgorithm::try_new(cursor.u16()?)?;
    let version = cursor.u16()?;
    let nonce_length = cursor.usize_u16()?;
    if nonce_length == 0 || nonce_length > MAX_APPLY_AUTH_NONCE_BYTES {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    Ok(ApplyRequestAuthClaim::try_new(
        principal,
        key,
        algorithm,
        version,
        cursor.take(nonce_length)?,
    )?)
}

fn encode_response_claim(wire: &mut Vec<u8>, claim: RemoteAgentAccessResponseAuthClaimV1) {
    wire.extend_from_slice(claim.runtime_principal.as_bytes());
    wire.extend_from_slice(claim.key.as_bytes());
    wire.extend_from_slice(&claim.algorithm.value().to_be_bytes());
    wire.extend_from_slice(&claim.algorithm_version.to_be_bytes());
    wire.extend_from_slice(claim.carrier_binding_digest.as_bytes());
}

fn decode_response_auth(
    cursor: &mut Cursor<'_>,
) -> Result<RemoteAgentAccessResponseAuthClaimV1, RemoteAgentAccessError> {
    let claim = RemoteAgentAccessResponseAuthClaimV1 {
        runtime_principal: PrincipalRef::from_bytes(cursor.array()?),
        key: ApplyAuthKeyRef::from_bytes(cursor.array()?),
        algorithm: ApplyAuthAlgorithm::try_new(cursor.u16()?)?,
        algorithm_version: cursor.u16()?,
        carrier_binding_digest: Digest32::from_bytes(cursor.array()?),
    };
    if bytes_are_zero(claim.runtime_principal.as_bytes())
        || bytes_are_zero(claim.key.as_bytes())
        || claim.algorithm.value() != RESTRICTED_CARRIER_ED25519_ALGORITHM
        || claim.algorithm_version != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
        || digest_is_zero(claim.carrier_binding_digest)
    {
        return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
    }
    Ok(claim)
}

fn decode_generation(
    value: u64,
) -> Result<Option<ManagedServiceGeneration>, RemoteAgentAccessError> {
    if value == 0 {
        Ok(None)
    } else {
        ManagedServiceGeneration::try_new(value)
            .map(Some)
            .map_err(|_| RemoteAgentAccessError::InvalidResponse)
    }
}

fn digest(domain: &[u8], wire: &[u8]) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(domain)?;
    builder.field_bytes(wire)?;
    Ok(builder.finish())
}

fn digest_is_zero(value: Digest32) -> bool {
    value.as_bytes().iter().all(|byte| *byte == 0)
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

struct Cursor<'a> {
    frame: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    const fn new(frame: &'a [u8]) -> Self {
        Self { frame, offset: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], RemoteAgentAccessError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(RemoteAgentAccessError::FrameTooLarge)?;
        let value = self
            .frame
            .get(self.offset..end)
            .ok_or(RemoteAgentAccessError::Truncated)?;
        self.offset = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], RemoteAgentAccessError> {
        self.take(N)?
            .try_into()
            .map_err(|_| RemoteAgentAccessError::Truncated)
    }
    fn u16(&mut self) -> Result<u16, RemoteAgentAccessError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, RemoteAgentAccessError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, RemoteAgentAccessError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn usize_u16(&mut self) -> Result<usize, RemoteAgentAccessError> {
        Ok(usize::from(self.u16()?))
    }
    fn usize_u32(&mut self) -> Result<usize, RemoteAgentAccessError> {
        usize::try_from(self.u32()?).map_err(|_| RemoteAgentAccessError::InvalidLength)
    }
    fn finish(self) -> Result<(), RemoteAgentAccessError> {
        if self.offset == self.frame.len() {
            Ok(())
        } else {
            Err(RemoteAgentAccessError::TrailingBytes)
        }
    }
}

/// Strict PXRA/PXRR contract failures.
#[derive(Debug)]
pub enum RemoteAgentAccessError {
    InvalidIdentity,
    InvalidRequest,
    AuthenticationMismatch,
    InvalidRequestAuthentication,
    UnsupportedKind,
    InvalidCarrierBinding,
    InvalidPayload,
    InvalidResponse,
    InvalidResponsePayload,
    InvalidDescriptor,
    CorrelationMismatch,
    InvalidResponseAuthentication,
    UnsupportedWire,
    InvalidLength,
    Truncated,
    TrailingBytes,
    FrameTooLarge,
    NonCanonicalFrame,
    Authentication(ApplyAuthError),
    DataPlane(RemoteAgentDataPlanePlanError),
    Digest(DigestBuildError),
}

impl From<ApplyAuthError> for RemoteAgentAccessError {
    fn from(value: ApplyAuthError) -> Self {
        Self::Authentication(value)
    }
}
impl From<RemoteAgentDataPlanePlanError> for RemoteAgentAccessError {
    fn from(value: RemoteAgentDataPlanePlanError) -> Self {
        Self::DataPlane(value)
    }
}
impl From<DigestBuildError> for RemoteAgentAccessError {
    fn from(value: DigestBuildError) -> Self {
        Self::Digest(value)
    }
}
impl fmt::Display for RemoteAgentAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "remote Agent access rejected: {self:?}")
    }
}
impl std::error::Error for RemoteAgentAccessError {}

const REQUEST_TRANSCRIPT_MAGIC_V2: &[u8] = b"ParaEGOX\0remote-agent-access-request-signing-v2";
const RESPONSE_TRANSCRIPT_MAGIC_V2: &[u8] = b"ParaEGOX\0remote-agent-access-response-signing-v2";
const REQUEST_PAYLOAD_DIGEST_DOMAIN_V2: &[u8] =
    b"paraegox.runtime.remote-agent-access.request-payload.sha256.v2";
const RESPONSE_PAYLOAD_DIGEST_DOMAIN_V2: &[u8] =
    b"paraegox.runtime.remote-agent-access.response-payload.sha256.v2";
const REQUEST_DIGEST_DOMAIN_V2: &[u8] = b"paraegox.runtime.remote-agent-access.request.sha256.v2";
const RESPONSE_DIGEST_DOMAIN_V2: &[u8] = b"paraegox.runtime.remote-agent-access.response.sha256.v2";
const REQUEST_V2_FIXED_BYTES: usize = 544;
const RESPONSE_V2_FIXED_BYTES: usize = 646;

/// Exact additive PXRA/PXRR protocol version for CAS-authorized S1 transitions.
pub const REMOTE_AGENT_ACCESS_V2_VERSION: u16 = 2;
/// Exact Controller signing-transcript version for PXRA v2.
pub const REMOTE_AGENT_ACCESS_REQUEST_SIGNING_V2_VERSION: u16 = 2;
/// Exact Runtime signing-transcript version for PXRR v2.
pub const REMOTE_AGENT_ACCESS_RESPONSE_SIGNING_V2_VERSION: u16 = 2;
/// Defensive carrier ceiling for one canonical PXRA v2 request.
pub const MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES: usize = REQUEST_V2_FIXED_BYTES
    + MAX_APPLY_AUTH_NONCE_BYTES
    + MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
    + MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES
    + RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES;
/// Defensive carrier ceiling for one canonical PXRR v2 response.
pub const MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES: usize = 5_792;

/// Nonzero identity of one PXRA v2 invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RemoteAgentAccessRequestIdV2([u8; 16]);

impl RemoteAgentAccessRequestIdV2 {
    pub const fn try_from_bytes(bytes: [u8; 16]) -> Result<Self, RemoteAgentAccessError> {
        if bytes_are_zero(&bytes) {
            return Err(RemoteAgentAccessError::InvalidIdentity);
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Operation admitted by PXRA v2.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u16)]
pub enum RemoteAgentAccessKindV2 {
    ApplyRemoteAccess = 1,
    DescribeRemoteAccess = 2,
}

impl RemoteAgentAccessKindV2 {
    fn decode(value: u16) -> Result<Self, RemoteAgentAccessError> {
        match value {
            1 => Ok(Self::ApplyRemoteAccess),
            2 => Ok(Self::DescribeRemoteAccess),
            _ => Err(RemoteAgentAccessError::UnsupportedKind),
        }
    }
}

/// Common target, carrier, epoch, and Controller-authentication inputs for PXRA v2.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessRequestFieldsV2 {
    pub request_id: RemoteAgentAccessRequestIdV2,
    pub carrier: RestrictedRuntimeApplyCarrierBindingV1,
    pub target: RuntimeHostId,
    pub expected_runtime_store_instance_id: [u8; 32],
    pub expected_runtime_host_epoch: u64,
    pub auth_claim: ApplyRequestAuthClaim,
}

/// Exact domain-separated bytes passed to a PXRA v2 Controller or Runtime signer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessSigningTranscriptV2(Box<[u8]>);

impl RemoteAgentAccessSigningTranscriptV2 {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Signature-independent PXRA v2 producer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessRequestDraftV2 {
    request_id: RemoteAgentAccessRequestIdV2,
    kind: RemoteAgentAccessKindV2,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    target: RuntimeHostId,
    expected_runtime_store_instance_id: [u8; 32],
    expected_runtime_host_epoch: u64,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    apply_request: Option<RemoteAgentDataPlaneApplyRequestV2>,
    payload_wire_digest: Digest32,
    auth_claim: ApplyRequestAuthClaim,
}

impl RemoteAgentAccessRequestDraftV2 {
    /// Wraps one byte-identical PXAR v11 and derives both S0/S1 CAS values.
    pub fn try_apply_remote_access(
        fields: RemoteAgentAccessRequestFieldsV2,
        request: RemoteAgentDataPlaneApplyRequestV2,
    ) -> Result<Self, RemoteAgentAccessError> {
        let execution = request.target_execution();
        Self::try_new(
            fields,
            RemoteAgentAccessKindV2::ApplyRemoteAccess,
            execution.retained_s0_cas(),
            execution.expected_s1_cas(),
            Some(request),
        )
    }

    /// Builds a payload-free Describe over exact retained-S0 and current active-S1 CAS values.
    pub fn try_describe_remote_access(
        fields: RemoteAgentAccessRequestFieldsV2,
        retained_s0_cas: RemoteAgentRetainedS0CasV2,
        expected_s1_cas: RemoteAgentActiveS1CasV2,
    ) -> Result<Self, RemoteAgentAccessError> {
        Self::try_new(
            fields,
            RemoteAgentAccessKindV2::DescribeRemoteAccess,
            retained_s0_cas,
            expected_s1_cas,
            None,
        )
    }

    fn try_new(
        fields: RemoteAgentAccessRequestFieldsV2,
        kind: RemoteAgentAccessKindV2,
        retained_s0_cas: RemoteAgentRetainedS0CasV2,
        expected_s1_cas: RemoteAgentActiveS1CasV2,
        apply_request: Option<RemoteAgentDataPlaneApplyRequestV2>,
    ) -> Result<Self, RemoteAgentAccessError> {
        validate_request_fields_v2(
            &fields,
            kind,
            retained_s0_cas,
            expected_s1_cas,
            apply_request.as_ref(),
        )?;
        let payload_wire_digest = apply_request.as_ref().map_or_else(
            || Ok(Digest32::from_bytes([0; 32])),
            |request| digest(REQUEST_PAYLOAD_DIGEST_DOMAIN_V2, request.canonical_wire()),
        )?;
        Ok(Self {
            request_id: fields.request_id,
            kind,
            carrier: fields.carrier,
            target: fields.target,
            expected_runtime_store_instance_id: fields.expected_runtime_store_instance_id,
            expected_runtime_host_epoch: fields.expected_runtime_host_epoch,
            retained_s0_cas,
            expected_s1_cas,
            apply_request,
            payload_wire_digest,
            auth_claim: fields.auth_claim,
        })
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV2, RemoteAgentAccessError> {
        let mut wire = build_request_base_v2(
            self,
            REQUEST_TRANSCRIPT_MAGIC_V2,
            REMOTE_AGENT_ACCESS_REQUEST_SIGNING_V2_VERSION,
        )?;
        append_request_values_v2(&mut wire, self);
        Ok(RemoteAgentAccessSigningTranscriptV2(
            wire.into_boxed_slice(),
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentAccessRequestV2, RemoteAgentAccessError> {
        if signature.len() != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES {
            return Err(RemoteAgentAccessError::InvalidRequestAuthentication);
        }
        let authentication =
            ApplyRequestAuthentication::try_new(self.auth_claim.clone(), signature)?;
        RemoteAgentAccessRequestV2::try_new(self, authentication)
    }
}

/// Strict Controller-signed PXRA v2 request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessRequestV2 {
    request_id: RemoteAgentAccessRequestIdV2,
    kind: RemoteAgentAccessKindV2,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    target: RuntimeHostId,
    expected_runtime_store_instance_id: [u8; 32],
    expected_runtime_host_epoch: u64,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    apply_request: Option<RemoteAgentDataPlaneApplyRequestV2>,
    payload_wire_digest: Digest32,
    authentication: ApplyRequestAuthentication,
    canonical_wire: Box<[u8]>,
    request_digest: Digest32,
}

impl RemoteAgentAccessRequestV2 {
    fn try_new(
        draft: RemoteAgentAccessRequestDraftV2,
        authentication: ApplyRequestAuthentication,
    ) -> Result<Self, RemoteAgentAccessError> {
        if authentication.claim() != &draft.auth_claim {
            return Err(RemoteAgentAccessError::AuthenticationMismatch);
        }
        let canonical_wire = build_request_wire_v2(&draft, &authentication)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        let request_digest = digest(REQUEST_DIGEST_DOMAIN_V2, &canonical_wire)?;
        Ok(Self {
            request_id: draft.request_id,
            kind: draft.kind,
            carrier: draft.carrier,
            target: draft.target,
            expected_runtime_store_instance_id: draft.expected_runtime_store_instance_id,
            expected_runtime_host_epoch: draft.expected_runtime_host_epoch,
            retained_s0_cas: draft.retained_s0_cas,
            expected_s1_cas: draft.expected_s1_cas,
            apply_request: draft.apply_request,
            payload_wire_digest: draft.payload_wire_digest,
            authentication,
            canonical_wire: canonical_wire.into_boxed_slice(),
            request_digest,
        })
    }

    /// Strictly decodes only PXRA v2 and its exact PXAR v11 payload shape.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentAccessError> {
        if frame.len() > MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        if frame.len() < REQUEST_V2_FIXED_BYTES {
            return Err(RemoteAgentAccessError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *REMOTE_AGENT_ACCESS_REQUEST_MAGIC
            || cursor.u16()? != REMOTE_AGENT_ACCESS_V2_VERSION
        {
            return Err(RemoteAgentAccessError::UnsupportedWire);
        }
        let kind = RemoteAgentAccessKindV2::decode(cursor.u16()?)?;
        if cursor.u16()? != 0 {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        let carrier_length = cursor.usize_u16()?;
        let payload_length = cursor.usize_u32()?;
        let request_id = RemoteAgentAccessRequestIdV2::try_from_bytes(cursor.array()?)?;
        let carrier_digest = Digest32::from_bytes(cursor.array()?);
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let expected_runtime_store_instance_id = cursor.array()?;
        let expected_runtime_host_epoch = cursor.u64()?;
        let retained_s0_cas = RemoteAgentRetainedS0CasV2::decode(
            cursor.take(REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES)?,
        )?;
        let expected_s1_cas =
            RemoteAgentActiveS1CasV2::decode(cursor.take(REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES)?)?;
        let payload_wire_digest = Digest32::from_bytes(cursor.array()?);
        let auth_claim = decode_request_claim(&mut cursor)?;
        let signature_length = cursor.usize_u16()?;
        validate_request_lengths_v2(kind, carrier_length, payload_length, signature_length)?;
        let carrier = RestrictedRuntimeApplyCarrierBindingV1::decode(cursor.take(carrier_length)?)
            .map_err(|_| RemoteAgentAccessError::InvalidCarrierBinding)?;
        if carrier.binding_digest() != carrier_digest {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let payload = cursor.take(payload_length)?;
        let apply_request = match kind {
            RemoteAgentAccessKindV2::ApplyRemoteAccess => Some(
                RemoteAgentDataPlaneApplyRequestV2::decode(payload)
                    .map_err(|_| RemoteAgentAccessError::InvalidPayload)?,
            ),
            RemoteAgentAccessKindV2::DescribeRemoteAccess => None,
        };
        if (payload.is_empty() && !digest_is_zero(payload_wire_digest))
            || (!payload.is_empty()
                && digest(REQUEST_PAYLOAD_DIGEST_DOMAIN_V2, payload)? != payload_wire_digest)
        {
            return Err(RemoteAgentAccessError::InvalidPayload);
        }
        let signature = cursor.take(signature_length)?;
        cursor.finish()?;
        let fields = RemoteAgentAccessRequestFieldsV2 {
            request_id,
            carrier,
            target,
            expected_runtime_store_instance_id,
            expected_runtime_host_epoch,
            auth_claim,
        };
        let draft = RemoteAgentAccessRequestDraftV2::try_new(
            fields,
            kind,
            retained_s0_cas,
            expected_s1_cas,
            apply_request,
        )?;
        if draft.payload_wire_digest != payload_wire_digest {
            return Err(RemoteAgentAccessError::InvalidPayload);
        }
        let decoded = draft.finalize(signature)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    /// Verifies the independent inner PXAR-v11 and outer PXRA-v2 Controller signatures.
    pub fn verify_controller_apply_request<VerifyInner, VerifyOuter>(
        &self,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify_inner: VerifyInner,
        verify_outer: VerifyOuter,
    ) -> Result<ControllerAuthenticatedRemoteAgentAccessRequestV2<'_>, RemoteAgentAccessError>
    where
        VerifyInner:
            FnOnce(PrincipalRef, ApplyAuthKeyRef, ApplyAuthAlgorithm, u16, &[u8], &[u8]) -> bool,
        VerifyOuter: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        if self.kind != RemoteAgentAccessKindV2::ApplyRemoteAccess {
            return Err(RemoteAgentAccessError::InvalidRequest);
        }
        if &self.carrier != expected_carrier {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let inner = self
            .apply_request
            .as_ref()
            .ok_or(RemoteAgentAccessError::InvalidPayload)?;
        let inner_claim = inner.authentication().claim();
        let inner_transcript = inner.signing_transcript()?;
        if !verify_inner(
            inner_claim.principal(),
            inner_claim.key(),
            inner_claim.algorithm(),
            inner_claim.algorithm_version(),
            inner_transcript.as_bytes(),
            inner.authentication().signature(),
        ) {
            return Err(RemoteAgentAccessError::InvalidRequestAuthentication);
        }
        self.verify_outer_controller(expected_carrier, verify_outer)?;
        Ok(ControllerAuthenticatedRemoteAgentAccessRequestV2 { request: self })
    }

    /// Verifies one payload-free Describe request using only its outer PXRA-v2 signature.
    pub fn verify_controller_describe_request<Verify>(
        &self,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<ControllerAuthenticatedRemoteAgentAccessRequestV2<'_>, RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        if self.kind != RemoteAgentAccessKindV2::DescribeRemoteAccess
            || self.apply_request.is_some()
        {
            return Err(RemoteAgentAccessError::InvalidRequest);
        }
        self.verify_outer_controller(expected_carrier, verify)?;
        Ok(ControllerAuthenticatedRemoteAgentAccessRequestV2 { request: self })
    }

    fn verify_outer_controller<Verify>(
        &self,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<(), RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        if &self.carrier != expected_carrier {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let transcript = self.signing_transcript()?;
        if !verify(
            self.carrier.controller_principal(),
            self.carrier.controller_request_key(),
            self.carrier.controller_request_key_fingerprint(),
            transcript.as_bytes(),
            self.authentication.signature(),
        ) {
            return Err(RemoteAgentAccessError::InvalidRequestAuthentication);
        }
        Ok(())
    }

    #[must_use]
    pub const fn request_id(&self) -> RemoteAgentAccessRequestIdV2 {
        self.request_id
    }
    #[must_use]
    pub const fn kind(&self) -> RemoteAgentAccessKindV2 {
        self.kind
    }
    #[must_use]
    pub const fn carrier(&self) -> &RestrictedRuntimeApplyCarrierBindingV1 {
        &self.carrier
    }
    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.target
    }
    #[must_use]
    pub const fn expected_runtime_store_instance_id(&self) -> [u8; 32] {
        self.expected_runtime_store_instance_id
    }
    #[must_use]
    pub const fn expected_runtime_host_epoch(&self) -> u64 {
        self.expected_runtime_host_epoch
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
    pub const fn apply_request(&self) -> Option<&RemoteAgentDataPlaneApplyRequestV2> {
        self.apply_request.as_ref()
    }
    #[must_use]
    pub const fn payload_wire_digest(&self) -> Digest32 {
        self.payload_wire_digest
    }
    #[must_use]
    pub const fn authentication(&self) -> &ApplyRequestAuthentication {
        &self.authentication
    }
    #[must_use]
    pub fn canonical_wire(&self) -> &[u8] {
        &self.canonical_wire
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV2, RemoteAgentAccessError> {
        self.as_draft().signing_transcript()
    }

    fn as_draft(&self) -> RemoteAgentAccessRequestDraftV2 {
        RemoteAgentAccessRequestDraftV2 {
            request_id: self.request_id,
            kind: self.kind,
            carrier: self.carrier.clone(),
            target: self.target,
            expected_runtime_store_instance_id: self.expected_runtime_store_instance_id,
            expected_runtime_host_epoch: self.expected_runtime_host_epoch,
            retained_s0_cas: self.retained_s0_cas,
            expected_s1_cas: self.expected_s1_cas,
            apply_request: self.apply_request.clone(),
            payload_wire_digest: self.payload_wire_digest,
            auth_claim: self.authentication.claim().clone(),
        }
    }
}

/// Marker issued only after PXRA v2 Controller authentication and PXCB matching.
#[derive(Clone, Copy, Debug)]
pub struct ControllerAuthenticatedRemoteAgentAccessRequestV2<'a> {
    request: &'a RemoteAgentAccessRequestV2,
}

impl<'a> ControllerAuthenticatedRemoteAgentAccessRequestV2<'a> {
    #[must_use]
    pub const fn request(self) -> &'a RemoteAgentAccessRequestV2 {
        self.request
    }
    #[must_use]
    pub const fn kind(self) -> RemoteAgentAccessKindV2 {
        self.request.kind()
    }
}

/// Runtime response signer bound to the exact public PXCB for PXRR v2.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RemoteAgentAccessResponseAuthClaimV2 {
    runtime_principal: PrincipalRef,
    key: ApplyAuthKeyRef,
    algorithm: ApplyAuthAlgorithm,
    algorithm_version: u16,
    carrier_binding_digest: Digest32,
}

impl RemoteAgentAccessResponseAuthClaimV2 {
    pub fn try_new(
        carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        key: ApplyAuthKeyRef,
        algorithm: ApplyAuthAlgorithm,
        algorithm_version: u16,
    ) -> Result<Self, RemoteAgentAccessError> {
        let claim = Self {
            runtime_principal: carrier.runtime_principal(),
            key,
            algorithm,
            algorithm_version,
            carrier_binding_digest: carrier.binding_digest(),
        };
        validate_response_auth_v2(claim, carrier)?;
        Ok(claim)
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
    #[must_use]
    pub const fn carrier_binding_digest(self) -> Digest32 {
        self.carrier_binding_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RemoteAgentAccessResponsePayloadV2 {
    Apply(Box<RemoteAgentDataPlaneTerminalReceiptV2>),
    Describe {
        profile: Box<RemoteAgentDataPlaneProfileV1>,
        descriptor: Box<[u8]>,
    },
}

/// Signature-independent Runtime PXRR v2 producer.
///
/// Describe remains a consumer wire only until PXRS v2 can supply a
/// current-final, non-Clone authority marker; raw historical PXAR/PXAU pairs
/// are deliberately insufficient to produce a signed Describe response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessResponseDraftV2 {
    request_id: RemoteAgentAccessRequestIdV2,
    request_digest: Digest32,
    request_nonce: Box<[u8]>,
    kind: RemoteAgentAccessKindV2,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    target: RuntimeHostId,
    runtime_store_instance_id: [u8; 32],
    runtime_host_epoch: u64,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    payload: RemoteAgentAccessResponsePayloadV2,
    payload_wire_digest: Digest32,
    descriptor_digest: Digest32,
    auth_claim: RemoteAgentAccessResponseAuthClaimV2,
}

impl RemoteAgentAccessResponseDraftV2 {
    /// Wraps exact PXAU v2 only after outer Controller and inner Runtime markers exist.
    pub fn try_apply_remote_access(
        authenticated_request: ControllerAuthenticatedRemoteAgentAccessRequestV2<'_>,
        authenticated_terminal: RuntimeAuthenticatedRemoteAgentDataPlaneTerminalV2<'_>,
        auth_claim: RemoteAgentAccessResponseAuthClaimV2,
    ) -> Result<Self, RemoteAgentAccessError> {
        if authenticated_request.kind() != RemoteAgentAccessKindV2::ApplyRemoteAccess {
            return Err(RemoteAgentAccessError::InvalidResponse);
        }
        let request = authenticated_request.request();
        let inner = request
            .apply_request()
            .ok_or(RemoteAgentAccessError::InvalidResponse)?;
        let receipt = authenticated_terminal.receipt();
        receipt.validate_against_request(inner)?;
        if !inner_terminal_auth_matches_carrier_v2(receipt, request.carrier())
            || receipt.authentication_signature().len()
                != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
            || receipt
                .facts()
                .evidence()
                .fields()
                .completion_runtime_host_epoch
                != request.expected_runtime_host_epoch()
        {
            return Err(RemoteAgentAccessError::InvalidResponse);
        }
        Self::try_new(
            request,
            RemoteAgentAccessResponsePayloadV2::Apply(Box::new(receipt.clone())),
            auth_claim,
        )
    }

    fn try_new(
        request: &RemoteAgentAccessRequestV2,
        payload: RemoteAgentAccessResponsePayloadV2,
        auth_claim: RemoteAgentAccessResponseAuthClaimV2,
    ) -> Result<Self, RemoteAgentAccessError> {
        let descriptor_digest = match &payload {
            RemoteAgentAccessResponsePayloadV2::Describe { descriptor, .. } => {
                runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                    .map_err(|_| RemoteAgentAccessError::InvalidDescriptor)?
            }
            RemoteAgentAccessResponsePayloadV2::Apply(_) => Digest32::from_bytes([0; 32]),
        };
        let payload_wire_digest = digest_response_payload_v2(&payload)?;
        let draft = Self {
            request_id: request.request_id,
            request_digest: request.request_digest,
            request_nonce: request.authentication.claim().nonce().into(),
            kind: request.kind,
            carrier: request.carrier.clone(),
            target: request.target,
            runtime_store_instance_id: request.expected_runtime_store_instance_id,
            runtime_host_epoch: request.expected_runtime_host_epoch,
            retained_s0_cas: request.retained_s0_cas,
            expected_s1_cas: request.expected_s1_cas,
            payload,
            payload_wire_digest,
            descriptor_digest,
            auth_claim,
        };
        validate_response_draft_v2(&draft)?;
        validate_response_against_request_v2(&draft, request)?;
        Ok(draft)
    }

    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV2, RemoteAgentAccessError> {
        let mut wire = build_response_base_v2(
            self,
            RESPONSE_TRANSCRIPT_MAGIC_V2,
            REMOTE_AGENT_ACCESS_RESPONSE_SIGNING_V2_VERSION,
        )?;
        append_response_values_v2(&mut wire, self);
        Ok(RemoteAgentAccessSigningTranscriptV2(
            wire.into_boxed_slice(),
        ))
    }

    pub fn finalize(
        self,
        signature: &[u8],
    ) -> Result<RemoteAgentAccessResponseV2, RemoteAgentAccessError> {
        if signature.len() != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES {
            return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
        }
        RemoteAgentAccessResponseV2::try_new(self, signature)
    }
}

/// Strict independently Runtime-signed PXRR v2 response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAgentAccessResponseV2 {
    draft: RemoteAgentAccessResponseDraftV2,
    signature: Box<[u8]>,
    canonical_wire: Box<[u8]>,
    response_digest: Digest32,
}

impl RemoteAgentAccessResponseV2 {
    fn try_new(
        draft: RemoteAgentAccessResponseDraftV2,
        signature: &[u8],
    ) -> Result<Self, RemoteAgentAccessError> {
        let canonical_wire = build_response_wire_v2(&draft, signature)?;
        if canonical_wire.len() > MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        let response_digest = digest(RESPONSE_DIGEST_DOMAIN_V2, &canonical_wire)?;
        Ok(Self {
            draft,
            signature: signature.into(),
            canonical_wire: canonical_wire.into_boxed_slice(),
            response_digest,
        })
    }

    /// Strictly decodes only PXRR v2 and its exact PXAU-v2 or PXAD1||PXAP payload shape.
    pub fn decode(frame: &[u8]) -> Result<Self, RemoteAgentAccessError> {
        if frame.len() > MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES {
            return Err(RemoteAgentAccessError::FrameTooLarge);
        }
        if frame.len() < RESPONSE_V2_FIXED_BYTES {
            return Err(RemoteAgentAccessError::Truncated);
        }
        let mut cursor = Cursor::new(frame);
        if cursor.array::<4>()? != *REMOTE_AGENT_ACCESS_RESPONSE_MAGIC
            || cursor.u16()? != REMOTE_AGENT_ACCESS_V2_VERSION
        {
            return Err(RemoteAgentAccessError::UnsupportedWire);
        }
        let kind = RemoteAgentAccessKindV2::decode(cursor.u16()?)?;
        if cursor.u16()? != 0 {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        let carrier_length = cursor.usize_u16()?;
        let payload_length = cursor.usize_u32()?;
        let profile_length = cursor.usize_u16()?;
        let descriptor_length = cursor.usize_u32()?;
        let nonce_length = cursor.usize_u16()?;
        let request_id = RemoteAgentAccessRequestIdV2::try_from_bytes(cursor.array()?)?;
        let request_digest = Digest32::from_bytes(cursor.array()?);
        let carrier_digest = Digest32::from_bytes(cursor.array()?);
        let target = RuntimeHostId::from_bytes(cursor.array()?);
        let runtime_store_instance_id = cursor.array()?;
        let runtime_host_epoch = cursor.u64()?;
        let retained_s0_cas = RemoteAgentRetainedS0CasV2::decode(
            cursor.take(REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES)?,
        )?;
        let expected_s1_cas =
            RemoteAgentActiveS1CasV2::decode(cursor.take(REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES)?)?;
        let payload_wire_digest = Digest32::from_bytes(cursor.array()?);
        let descriptor_digest = Digest32::from_bytes(cursor.array()?);
        let auth_claim = decode_response_auth_v2(&mut cursor)?;
        let signature_length = cursor.usize_u16()?;
        validate_response_lengths_v2(
            kind,
            carrier_length,
            payload_length,
            profile_length,
            descriptor_length,
            nonce_length,
            signature_length,
        )?;
        let request_nonce: Box<[u8]> = cursor.take(nonce_length)?.into();
        let carrier = RestrictedRuntimeApplyCarrierBindingV1::decode(cursor.take(carrier_length)?)
            .map_err(|_| RemoteAgentAccessError::InvalidCarrierBinding)?;
        if carrier.binding_digest() != carrier_digest {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let payload = match kind {
            RemoteAgentAccessKindV2::ApplyRemoteAccess => {
                let receipt =
                    RemoteAgentDataPlaneTerminalReceiptV2::decode(cursor.take(payload_length)?)
                        .map_err(|_| RemoteAgentAccessError::InvalidResponsePayload)?;
                RemoteAgentAccessResponsePayloadV2::Apply(Box::new(receipt))
            }
            RemoteAgentAccessKindV2::DescribeRemoteAccess => {
                let profile = RemoteAgentDataPlaneProfileV1::decode(cursor.take(profile_length)?)
                    .map_err(|_| RemoteAgentAccessError::InvalidResponsePayload)?;
                let descriptor = cursor.take(descriptor_length)?;
                runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                    .map_err(|_| RemoteAgentAccessError::InvalidDescriptor)?;
                RemoteAgentAccessResponsePayloadV2::Describe {
                    profile: Box::new(profile),
                    descriptor: descriptor.into(),
                }
            }
        };
        if digest_response_payload_v2(&payload)? != payload_wire_digest {
            return Err(RemoteAgentAccessError::InvalidResponsePayload);
        }
        let signature = cursor.take(signature_length)?;
        cursor.finish()?;
        let draft = RemoteAgentAccessResponseDraftV2 {
            request_id,
            request_digest,
            request_nonce,
            kind,
            carrier,
            target,
            runtime_store_instance_id,
            runtime_host_epoch,
            retained_s0_cas,
            expected_s1_cas,
            payload,
            payload_wire_digest,
            descriptor_digest,
            auth_claim,
        };
        validate_response_draft_v2(&draft)?;
        let decoded = draft.finalize(signature)?;
        if decoded.canonical_wire() != frame {
            return Err(RemoteAgentAccessError::NonCanonicalFrame);
        }
        Ok(decoded)
    }

    /// Verifies exact correlation plus independent inner PXAU-v2 and outer PXRR-v2 signatures.
    pub fn verify_runtime_apply_response<'a, VerifyInner, VerifyOuter>(
        &'a self,
        request: &RemoteAgentAccessRequestV2,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        expected_inner_auth: RemoteAgentDataPlaneTerminalAuthClaimV2,
        verify_inner: VerifyInner,
        verify_outer: VerifyOuter,
    ) -> Result<RuntimeAuthenticatedRemoteAgentAccessResponseV2<'a>, RemoteAgentAccessError>
    where
        VerifyInner:
            FnOnce(PrincipalRef, ApplyAuthKeyRef, ApplyAuthAlgorithm, u16, &[u8], &[u8]) -> bool,
        VerifyOuter: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        self.validate_against_request(request)?;
        let inner_request = request
            .apply_request()
            .ok_or(RemoteAgentAccessError::CorrelationMismatch)?;
        let receipt = self
            .apply_receipt()
            .ok_or(RemoteAgentAccessError::CorrelationMismatch)?;
        if !inner_terminal_auth_matches_carrier_v2(receipt, &self.draft.carrier)
            || expected_inner_auth != receipt.authentication()
        {
            return Err(RemoteAgentAccessError::CorrelationMismatch);
        }
        receipt.verify_runtime_terminal(inner_request, expected_inner_auth, verify_inner)?;
        self.verify_outer(expected_carrier, verify_outer)?;
        Ok(RuntimeAuthenticatedRemoteAgentAccessResponseV2 { response: self })
    }

    /// Verifies exact Describe correlation and the independent outer PXRR-v2 signature.
    /// This is consumer-only until PXRS v2 supplies a current-final, non-Clone
    /// authority marker to a future Runtime Describe producer.
    pub fn verify_runtime_describe_response<'a, Verify>(
        &'a self,
        request: &RemoteAgentAccessRequestV2,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<RuntimeAuthenticatedRemoteAgentAccessResponseV2<'a>, RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        self.validate_against_request(request)?;
        if self.draft.kind != RemoteAgentAccessKindV2::DescribeRemoteAccess {
            return Err(RemoteAgentAccessError::CorrelationMismatch);
        }
        self.verify_outer(expected_carrier, verify)?;
        Ok(RuntimeAuthenticatedRemoteAgentAccessResponseV2 { response: self })
    }

    fn verify_outer<Verify>(
        &self,
        expected_carrier: &RestrictedRuntimeApplyCarrierBindingV1,
        verify: Verify,
    ) -> Result<(), RemoteAgentAccessError>
    where
        Verify: FnOnce(PrincipalRef, ApplyAuthKeyRef, Digest32, &[u8], &[u8]) -> bool,
    {
        if &self.draft.carrier != expected_carrier {
            return Err(RemoteAgentAccessError::InvalidCarrierBinding);
        }
        let transcript = self.signing_transcript()?;
        if !verify(
            self.draft.carrier.runtime_principal(),
            self.draft.carrier.runtime_response_key(),
            self.draft.carrier.runtime_response_key_fingerprint(),
            transcript.as_bytes(),
            &self.signature,
        ) {
            return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
        }
        Ok(())
    }

    pub fn validate_against_request(
        &self,
        request: &RemoteAgentAccessRequestV2,
    ) -> Result<(), RemoteAgentAccessError> {
        validate_response_against_request_v2(&self.draft, request)
    }

    #[must_use]
    pub const fn request_id(&self) -> RemoteAgentAccessRequestIdV2 {
        self.draft.request_id
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.draft.request_digest
    }
    #[must_use]
    pub fn request_nonce(&self) -> &[u8] {
        &self.draft.request_nonce
    }
    #[must_use]
    pub const fn kind(&self) -> RemoteAgentAccessKindV2 {
        self.draft.kind
    }
    #[must_use]
    pub const fn carrier(&self) -> &RestrictedRuntimeApplyCarrierBindingV1 {
        &self.draft.carrier
    }
    #[must_use]
    pub const fn target(&self) -> RuntimeHostId {
        self.draft.target
    }
    #[must_use]
    pub const fn runtime_store_instance_id(&self) -> [u8; 32] {
        self.draft.runtime_store_instance_id
    }
    #[must_use]
    pub const fn runtime_host_epoch(&self) -> u64 {
        self.draft.runtime_host_epoch
    }
    #[must_use]
    pub const fn retained_s0_cas(&self) -> RemoteAgentRetainedS0CasV2 {
        self.draft.retained_s0_cas
    }
    #[must_use]
    pub const fn expected_s1_cas(&self) -> RemoteAgentActiveS1CasV2 {
        self.draft.expected_s1_cas
    }
    #[must_use]
    pub fn apply_receipt(&self) -> Option<&RemoteAgentDataPlaneTerminalReceiptV2> {
        match &self.draft.payload {
            RemoteAgentAccessResponsePayloadV2::Apply(value) => Some(value),
            RemoteAgentAccessResponsePayloadV2::Describe { .. } => None,
        }
    }
    #[must_use]
    pub fn profile(&self) -> Option<&RemoteAgentDataPlaneProfileV1> {
        match &self.draft.payload {
            RemoteAgentAccessResponsePayloadV2::Describe { profile, .. } => Some(profile),
            RemoteAgentAccessResponsePayloadV2::Apply(_) => None,
        }
    }
    #[must_use]
    pub fn descriptor(&self) -> Option<&[u8]> {
        match &self.draft.payload {
            RemoteAgentAccessResponsePayloadV2::Describe { descriptor, .. } => Some(descriptor),
            RemoteAgentAccessResponsePayloadV2::Apply(_) => None,
        }
    }
    #[must_use]
    pub const fn payload_wire_digest(&self) -> Digest32 {
        self.draft.payload_wire_digest
    }
    #[must_use]
    pub const fn descriptor_digest(&self) -> Digest32 {
        self.draft.descriptor_digest
    }
    #[must_use]
    pub const fn authentication(&self) -> RemoteAgentAccessResponseAuthClaimV2 {
        self.draft.auth_claim
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
    pub const fn response_digest(&self) -> Digest32 {
        self.response_digest
    }
    pub fn signing_transcript(
        &self,
    ) -> Result<RemoteAgentAccessSigningTranscriptV2, RemoteAgentAccessError> {
        self.draft.signing_transcript()
    }
}

/// Marker issued after exact PXRR-v2 correlation and Runtime signature checks.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeAuthenticatedRemoteAgentAccessResponseV2<'a> {
    response: &'a RemoteAgentAccessResponseV2,
}

impl<'a> RuntimeAuthenticatedRemoteAgentAccessResponseV2<'a> {
    #[must_use]
    pub const fn response(self) -> &'a RemoteAgentAccessResponseV2 {
        self.response
    }
}

fn validate_request_fields_v2(
    fields: &RemoteAgentAccessRequestFieldsV2,
    kind: RemoteAgentAccessKindV2,
    retained_s0_cas: RemoteAgentRetainedS0CasV2,
    expected_s1_cas: RemoteAgentActiveS1CasV2,
    apply_request: Option<&RemoteAgentDataPlaneApplyRequestV2>,
) -> Result<(), RemoteAgentAccessError> {
    if bytes_are_zero(fields.request_id.as_bytes())
        || bytes_are_zero(fields.target.as_bytes())
        || bytes_are_zero(&fields.expected_runtime_store_instance_id)
        || fields.expected_runtime_host_epoch == 0
        || fields.carrier.target() != fields.target
        || fields.auth_claim.principal() != fields.carrier.controller_principal()
        || fields.auth_claim.key() != fields.carrier.controller_request_key()
        || fields.auth_claim.algorithm().value() != RESTRICTED_CARRIER_ED25519_ALGORITHM
        || fields.auth_claim.algorithm_version() != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
        || fields.auth_claim.nonce().is_empty()
        || fields.auth_claim.nonce().iter().all(|byte| *byte == 0)
        || digest_is_zero(retained_s0_cas.cas_digest())
        || digest_is_zero(expected_s1_cas.cas_digest())
    {
        return Err(RemoteAgentAccessError::InvalidRequest);
    }
    match (kind, apply_request) {
        (RemoteAgentAccessKindV2::ApplyRemoteAccess, Some(request)) => {
            let execution = request.target_execution();
            let profile = execution.profile();
            if fields.request_id.as_bytes() != request.operation_id().as_bytes()
                || request.target() != fields.target
                || request.expected_runtime_store_instance_id()
                    != fields.expected_runtime_store_instance_id
                || !inner_request_auth_matches_carrier_v2(request, &fields.carrier)
                || request.authentication().signature().len()
                    != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
                || request.authentication().claim().nonce() == fields.auth_claim.nonce()
                || execution.retained_s0_cas() != retained_s0_cas
                || execution.expected_s1_cas() != expected_s1_cas
                || profile.target() != fields.target
                || !profile_principals_match_carrier_v2(profile, &fields.carrier)
            {
                return Err(RemoteAgentAccessError::InvalidPayload);
            }
        }
        (RemoteAgentAccessKindV2::DescribeRemoteAccess, None) => {
            if expected_s1_cas.active().is_none() {
                return Err(RemoteAgentAccessError::InvalidPayload);
            }
        }
        _ => return Err(RemoteAgentAccessError::InvalidPayload),
    }
    Ok(())
}

fn validate_request_lengths_v2(
    kind: RemoteAgentAccessKindV2,
    carrier: usize,
    payload: usize,
    signature: usize,
) -> Result<(), RemoteAgentAccessError> {
    if carrier == 0
        || carrier > MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
        || signature != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
        || match kind {
            RemoteAgentAccessKindV2::ApplyRemoteAccess => {
                payload == 0 || payload > MAX_REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_BYTES
            }
            RemoteAgentAccessKindV2::DescribeRemoteAccess => payload != 0,
        }
    {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    Ok(())
}

fn build_request_base_v2(
    draft: &RemoteAgentAccessRequestDraftV2,
    magic: &[u8],
    version: u16,
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    let carrier_length = u16::try_from(draft.carrier.canonical_wire().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let payload_length = u32::try_from(
        draft
            .apply_request
            .as_ref()
            .map_or(0, |value| value.canonical_wire().len()),
    )
    .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let nonce_length = u16::try_from(draft.auth_claim.nonce().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(magic);
    wire.extend_from_slice(&version.to_be_bytes());
    wire.extend_from_slice(&(draft.kind as u16).to_be_bytes());
    wire.extend_from_slice(&0_u16.to_be_bytes());
    wire.extend_from_slice(&carrier_length.to_be_bytes());
    wire.extend_from_slice(&payload_length.to_be_bytes());
    wire.extend_from_slice(draft.request_id.as_bytes());
    wire.extend_from_slice(draft.carrier.binding_digest().as_bytes());
    wire.extend_from_slice(draft.target.as_bytes());
    wire.extend_from_slice(&draft.expected_runtime_store_instance_id);
    wire.extend_from_slice(&draft.expected_runtime_host_epoch.to_be_bytes());
    wire.extend_from_slice(draft.retained_s0_cas.canonical_wire());
    wire.extend_from_slice(draft.expected_s1_cas.canonical_wire());
    wire.extend_from_slice(draft.payload_wire_digest.as_bytes());
    encode_request_claim(&mut wire, &draft.auth_claim, nonce_length);
    Ok(wire)
}

fn append_request_values_v2(wire: &mut Vec<u8>, draft: &RemoteAgentAccessRequestDraftV2) {
    wire.extend_from_slice(draft.carrier.canonical_wire());
    if let Some(request) = &draft.apply_request {
        wire.extend_from_slice(request.canonical_wire());
    }
}

fn build_request_wire_v2(
    draft: &RemoteAgentAccessRequestDraftV2,
    authentication: &ApplyRequestAuthentication,
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    let signature_length = u16::try_from(authentication.signature().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = build_request_base_v2(
        draft,
        REMOTE_AGENT_ACCESS_REQUEST_MAGIC,
        REMOTE_AGENT_ACCESS_V2_VERSION,
    )?;
    wire.extend_from_slice(&signature_length.to_be_bytes());
    append_request_values_v2(&mut wire, draft);
    wire.extend_from_slice(authentication.signature());
    Ok(wire)
}

fn validate_response_auth_v2(
    claim: RemoteAgentAccessResponseAuthClaimV2,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> Result<(), RemoteAgentAccessError> {
    if bytes_are_zero(claim.runtime_principal.as_bytes())
        || bytes_are_zero(claim.key.as_bytes())
        || claim.algorithm.value() != RESTRICTED_CARRIER_ED25519_ALGORITHM
        || claim.algorithm_version != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
        || digest_is_zero(claim.carrier_binding_digest)
        || claim.runtime_principal != carrier.runtime_principal()
        || claim.key != carrier.runtime_response_key()
        || claim.carrier_binding_digest != carrier.binding_digest()
    {
        return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
    }
    Ok(())
}

fn inner_request_auth_matches_carrier_v2(
    request: &RemoteAgentDataPlaneApplyRequestV2,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> bool {
    let claim = request.authentication().claim();
    claim.principal() == carrier.controller_principal()
        && claim.key() == carrier.controller_request_key()
        && claim.algorithm().value() == RESTRICTED_CARRIER_ED25519_ALGORITHM
        && claim.algorithm_version() == RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
}

fn inner_terminal_auth_matches_carrier_v2(
    receipt: &RemoteAgentDataPlaneTerminalReceiptV2,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> bool {
    let claim = receipt.authentication();
    claim.runtime_principal() == carrier.runtime_principal()
        && claim.key() == carrier.runtime_response_key()
        && claim.algorithm().value() == RESTRICTED_CARRIER_ED25519_ALGORITHM
        && claim.algorithm_version() == RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
}

fn profile_principals_match_carrier_v2(
    profile: &RemoteAgentDataPlaneProfileV1,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> bool {
    let mac = profile.mac_agent_client_principal();
    let ubuntu = profile.ubuntu_agent_listener_principal();
    mac != ubuntu
        && mac != carrier.controller_principal()
        && mac != carrier.runtime_principal()
        && ubuntu != carrier.controller_principal()
        && ubuntu != carrier.runtime_principal()
}

fn validate_response_draft_v2(
    draft: &RemoteAgentAccessResponseDraftV2,
) -> Result<(), RemoteAgentAccessError> {
    if digest_is_zero(draft.request_digest)
        || draft.request_nonce.is_empty()
        || draft.request_nonce.len() > MAX_APPLY_AUTH_NONCE_BYTES
        || draft.request_nonce.iter().all(|byte| *byte == 0)
        || draft.carrier.target() != draft.target
        || bytes_are_zero(draft.target.as_bytes())
        || bytes_are_zero(&draft.runtime_store_instance_id)
        || draft.runtime_host_epoch == 0
        || digest_is_zero(draft.retained_s0_cas.cas_digest())
        || digest_is_zero(draft.expected_s1_cas.cas_digest())
        || digest_is_zero(draft.payload_wire_digest)
    {
        return Err(RemoteAgentAccessError::InvalidResponse);
    }
    validate_response_auth_v2(draft.auth_claim, &draft.carrier)?;
    let valid = match (&draft.payload, draft.kind) {
        (
            RemoteAgentAccessResponsePayloadV2::Apply(receipt),
            RemoteAgentAccessKindV2::ApplyRemoteAccess,
        ) => {
            digest_is_zero(draft.descriptor_digest)
                && receipt.facts().target() == draft.target
                && receipt.facts().runtime_store_instance_id() == draft.runtime_store_instance_id
                && receipt
                    .facts()
                    .evidence()
                    .fields()
                    .completion_runtime_host_epoch
                    == draft.runtime_host_epoch
                && receipt.authentication_signature().len()
                    == RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
                && inner_terminal_auth_matches_carrier_v2(receipt, &draft.carrier)
        }
        (
            RemoteAgentAccessResponsePayloadV2::Describe {
                profile,
                descriptor,
            },
            RemoteAgentAccessKindV2::DescribeRemoteAccess,
        ) => {
            draft.expected_s1_cas.active().is_some()
                && profile.target() == draft.target
                && profile_principals_match_carrier_v2(profile, &draft.carrier)
                && runtime_agent_control_descriptor_payload_digest_v1(descriptor)
                    .map_err(|_| RemoteAgentAccessError::InvalidDescriptor)?
                    == draft.descriptor_digest
                && draft.descriptor_digest
                    == draft
                        .retained_s0_cas
                        .fields()
                        .expected_descriptor_payload_digest
        }
        _ => false,
    };
    if !valid || digest_response_payload_v2(&draft.payload)? != draft.payload_wire_digest {
        return Err(RemoteAgentAccessError::InvalidResponse);
    }
    Ok(())
}

fn validate_response_against_request_v2(
    draft: &RemoteAgentAccessResponseDraftV2,
    request: &RemoteAgentAccessRequestV2,
) -> Result<(), RemoteAgentAccessError> {
    if draft.request_id != request.request_id
        || draft.request_digest != request.request_digest
        || draft.request_nonce.as_ref() != request.authentication.claim().nonce()
        || draft.kind != request.kind
        || draft.carrier != request.carrier
        || draft.target != request.target
        || draft.runtime_store_instance_id != request.expected_runtime_store_instance_id
        || draft.runtime_host_epoch != request.expected_runtime_host_epoch
        || draft.retained_s0_cas != request.retained_s0_cas
        || draft.expected_s1_cas != request.expected_s1_cas
    {
        return Err(RemoteAgentAccessError::CorrelationMismatch);
    }
    match (&draft.payload, request.kind) {
        (
            RemoteAgentAccessResponsePayloadV2::Apply(receipt),
            RemoteAgentAccessKindV2::ApplyRemoteAccess,
        ) => {
            let inner = request
                .apply_request
                .as_ref()
                .ok_or(RemoteAgentAccessError::CorrelationMismatch)?;
            receipt
                .validate_against_request(inner)
                .map_err(|_| RemoteAgentAccessError::CorrelationMismatch)?;
            if !inner_terminal_auth_matches_carrier_v2(receipt, &request.carrier)
                || receipt
                    .facts()
                    .evidence()
                    .fields()
                    .completion_runtime_host_epoch
                    != request.expected_runtime_host_epoch
            {
                return Err(RemoteAgentAccessError::CorrelationMismatch);
            }
        }
        (
            RemoteAgentAccessResponsePayloadV2::Describe { .. },
            RemoteAgentAccessKindV2::DescribeRemoteAccess,
        ) if request.apply_request.is_none() && request.expected_s1_cas.active().is_some() => {}
        _ => return Err(RemoteAgentAccessError::CorrelationMismatch),
    }
    Ok(())
}

fn response_payload_lengths_v2(
    payload: &RemoteAgentAccessResponsePayloadV2,
) -> (usize, usize, usize) {
    match payload {
        RemoteAgentAccessResponsePayloadV2::Apply(receipt) => {
            (receipt.canonical_wire().len(), 0, 0)
        }
        RemoteAgentAccessResponsePayloadV2::Describe {
            profile,
            descriptor,
        } => (
            profile.canonical_wire().len() + descriptor.len(),
            profile.canonical_wire().len(),
            descriptor.len(),
        ),
    }
}

fn digest_response_payload_v2(
    payload: &RemoteAgentAccessResponsePayloadV2,
) -> Result<Digest32, RemoteAgentAccessError> {
    let mut builder = Digest32Builder::try_new(RESPONSE_PAYLOAD_DIGEST_DOMAIN_V2)?;
    match payload {
        RemoteAgentAccessResponsePayloadV2::Apply(receipt) => {
            builder.field_bytes(receipt.canonical_wire())?;
        }
        RemoteAgentAccessResponsePayloadV2::Describe {
            profile,
            descriptor,
        } => {
            builder.field_bytes(profile.canonical_wire())?;
            builder.field_bytes(descriptor)?;
        }
    }
    Ok(builder.finish())
}

fn build_response_base_v2(
    draft: &RemoteAgentAccessResponseDraftV2,
    magic: &[u8],
    version: u16,
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    validate_response_draft_v2(draft)?;
    let carrier_length = u16::try_from(draft.carrier.canonical_wire().len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let (payload_length, profile_length, descriptor_length) =
        response_payload_lengths_v2(&draft.payload);
    let payload_length =
        u32::try_from(payload_length).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let profile_length =
        u16::try_from(profile_length).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let descriptor_length =
        u32::try_from(descriptor_length).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let nonce_length = u16::try_from(draft.request_nonce.len())
        .map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = Vec::new();
    wire.extend_from_slice(magic);
    wire.extend_from_slice(&version.to_be_bytes());
    wire.extend_from_slice(&(draft.kind as u16).to_be_bytes());
    wire.extend_from_slice(&0_u16.to_be_bytes());
    wire.extend_from_slice(&carrier_length.to_be_bytes());
    wire.extend_from_slice(&payload_length.to_be_bytes());
    wire.extend_from_slice(&profile_length.to_be_bytes());
    wire.extend_from_slice(&descriptor_length.to_be_bytes());
    wire.extend_from_slice(&nonce_length.to_be_bytes());
    wire.extend_from_slice(draft.request_id.as_bytes());
    wire.extend_from_slice(draft.request_digest.as_bytes());
    wire.extend_from_slice(draft.carrier.binding_digest().as_bytes());
    wire.extend_from_slice(draft.target.as_bytes());
    wire.extend_from_slice(&draft.runtime_store_instance_id);
    wire.extend_from_slice(&draft.runtime_host_epoch.to_be_bytes());
    wire.extend_from_slice(draft.retained_s0_cas.canonical_wire());
    wire.extend_from_slice(draft.expected_s1_cas.canonical_wire());
    wire.extend_from_slice(draft.payload_wire_digest.as_bytes());
    wire.extend_from_slice(draft.descriptor_digest.as_bytes());
    encode_response_claim_v2(&mut wire, draft.auth_claim);
    Ok(wire)
}

fn append_response_values_v2(wire: &mut Vec<u8>, draft: &RemoteAgentAccessResponseDraftV2) {
    wire.extend_from_slice(&draft.request_nonce);
    wire.extend_from_slice(draft.carrier.canonical_wire());
    match &draft.payload {
        RemoteAgentAccessResponsePayloadV2::Apply(receipt) => {
            wire.extend_from_slice(receipt.canonical_wire())
        }
        RemoteAgentAccessResponsePayloadV2::Describe {
            profile,
            descriptor,
        } => {
            wire.extend_from_slice(profile.canonical_wire());
            wire.extend_from_slice(descriptor);
        }
    }
}

fn build_response_wire_v2(
    draft: &RemoteAgentAccessResponseDraftV2,
    signature: &[u8],
) -> Result<Vec<u8>, RemoteAgentAccessError> {
    let signature_length =
        u16::try_from(signature.len()).map_err(|_| RemoteAgentAccessError::InvalidLength)?;
    let mut wire = build_response_base_v2(
        draft,
        REMOTE_AGENT_ACCESS_RESPONSE_MAGIC,
        REMOTE_AGENT_ACCESS_V2_VERSION,
    )?;
    wire.extend_from_slice(&signature_length.to_be_bytes());
    append_response_values_v2(&mut wire, draft);
    wire.extend_from_slice(signature);
    Ok(wire)
}

fn validate_response_lengths_v2(
    kind: RemoteAgentAccessKindV2,
    carrier: usize,
    payload: usize,
    profile: usize,
    descriptor: usize,
    nonce: usize,
    signature: usize,
) -> Result<(), RemoteAgentAccessError> {
    if carrier == 0
        || carrier > MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
        || nonce == 0
        || nonce > MAX_APPLY_AUTH_NONCE_BYTES
        || signature != RESTRICTED_CARRIER_ED25519_SIGNATURE_BYTES
    {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    let valid = match kind {
        RemoteAgentAccessKindV2::ApplyRemoteAccess => {
            payload != 0
                && payload <= MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES
                && profile == 0
                && descriptor == 0
        }
        RemoteAgentAccessKindV2::DescribeRemoteAccess => {
            profile != 0
                && profile <= MAX_REMOTE_AGENT_DATA_PLANE_PROFILE_BYTES
                && (6..=MAX_REMOTE_AGENT_ACCESS_DESCRIPTOR_BYTES).contains(&descriptor)
                && payload == profile + descriptor
        }
    };
    if !valid {
        return Err(RemoteAgentAccessError::InvalidLength);
    }
    Ok(())
}

fn encode_response_claim_v2(wire: &mut Vec<u8>, claim: RemoteAgentAccessResponseAuthClaimV2) {
    wire.extend_from_slice(claim.runtime_principal.as_bytes());
    wire.extend_from_slice(claim.key.as_bytes());
    wire.extend_from_slice(&claim.algorithm.value().to_be_bytes());
    wire.extend_from_slice(&claim.algorithm_version.to_be_bytes());
    wire.extend_from_slice(claim.carrier_binding_digest.as_bytes());
}

fn decode_response_auth_v2(
    cursor: &mut Cursor<'_>,
) -> Result<RemoteAgentAccessResponseAuthClaimV2, RemoteAgentAccessError> {
    let claim = RemoteAgentAccessResponseAuthClaimV2 {
        runtime_principal: PrincipalRef::from_bytes(cursor.array()?),
        key: ApplyAuthKeyRef::from_bytes(cursor.array()?),
        algorithm: ApplyAuthAlgorithm::try_new(cursor.u16()?)?,
        algorithm_version: cursor.u16()?,
        carrier_binding_digest: Digest32::from_bytes(cursor.array()?),
    };
    if bytes_are_zero(claim.runtime_principal.as_bytes())
        || bytes_are_zero(claim.key.as_bytes())
        || claim.algorithm.value() != RESTRICTED_CARRIER_ED25519_ALGORITHM
        || claim.algorithm_version != RESTRICTED_CARRIER_ED25519_ALGORITHM_VERSION
        || digest_is_zero(claim.carrier_binding_digest)
    {
        return Err(RemoteAgentAccessError::InvalidResponseAuthentication);
    }
    Ok(claim)
}
