#![cfg(unix)]

//! Runtime-owned preparation and lifecycle authority for one remote-Agent S1.
//!
//! This bounded closeout enabler is entirely crate-private because no production
//! owner consumes it yet. Exact PXAP routes, reserved Fabric-session authority,
//! listener ownership, and all transition correlation remain Runtime-private
//! and move-only. The next integration batch must consume or remove this seam.

use core::fmt;

use paraegox_fabric::{
    FabricConfigError, FabricError, FabricServiceConfig, PreparedRemoteAgentProxyListenerV2,
    RemoteAgentProxyListenerV2, RemoteTlsEndpoint,
    ResolvedRemoteMtlsListenerCredentialFilesV1,
};
use paraegox_kernel::{
    digest::{Digest32, Digest32Builder, DigestBuildError},
    identity::{PrincipalRef, RuntimeHostId},
};
use paraegox_runtime_contracts::{
    distributed_agent_stack_plan::{
        DistributedFabricCredentialRefV1, DistributedFabricSessionEpochV1,
        DistributedFabricTlsEndpointV1, DistributedFabricTrustAnchorRefV1,
        DistributedFabricTrustDomainRefV1,
    },
    managed_serving_bootstrap::{
        ManagedServingBootstrapError, runtime_agent_control_descriptor_payload_digest_v1,
    },
    remote_agent_access::{RemoteAgentAccessKindV2, RemoteAgentAccessRequestV2},
    remote_agent_data_plane_plan::{
        RemoteAgentDataPlaneProfileV1, RemoteAgentDataPlaneTargetModeV2,
    },
};

use crate::{
    admission::VerifiedRemoteAgentAccessApplyIngressV2,
    managed_agent_transport::{
        AgentConversationPortDescriptorError, RemoteAgentS1ExactPxapMirrorPlanV2,
    },
};

const S1_CREDENTIAL_REQUIREMENT_DIGEST_DOMAIN_V2: &[u8] =
    b"paraegox.runtime.remote-agent-s1-listener-credential-requirement.sha256.v2";

/// Complete non-secret requirement presented to the Ubuntu S1 credential owner.
///
/// The digest binds the exact canonical PXAD bytes and digest, every listener
/// identity/reference needed by the resolver, the expected Mac client
/// principal, and the exact canonical PXAP digest. Resolving the Ubuntu
/// credential reference alone is therefore insufficient.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct RuntimeRemoteAgentS1CredentialRequirementV2 {
    profile_canonical_wire: Box<[u8]>,
    profile_digest: Digest32,
    target: RuntimeHostId,
    ubuntu_tls_listener_endpoint: DistributedFabricTlsEndpointV1,
    endpoint_ref: [u8; 16],
    endpoint_generation: u64,
    trust_domain_ref: DistributedFabricTrustDomainRefV1,
    trust_anchor_ref: DistributedFabricTrustAnchorRefV1,
    ubuntu_listener_credential_ref: DistributedFabricCredentialRefV1,
    ubuntu_agent_listener_principal: PrincipalRef,
    mac_agent_client_principal: PrincipalRef,
    pxap_descriptor_digest: Digest32,
    requirement_digest: Digest32,
}

impl RuntimeRemoteAgentS1CredentialRequirementV2 {
    fn try_from_profile_and_pxap(
        profile: &RemoteAgentDataPlaneProfileV1,
        pxap: &RemoteAgentS1ExactPxapMirrorPlanV2,
    ) -> Result<Self, DigestBuildError> {
        let pxap_descriptor_digest = pxap.descriptor_digest();
        let requirement_digest = s1_credential_requirement_digest_v2(
            profile,
            pxap_descriptor_digest,
        )?;
        Ok(Self {
            profile_canonical_wire: profile.canonical_wire().into(),
            profile_digest: profile.profile_digest(),
            target: profile.target(),
            ubuntu_tls_listener_endpoint: profile.ubuntu_tls_listener_endpoint().clone(),
            endpoint_ref: profile.endpoint_ref(),
            endpoint_generation: profile.endpoint_generation(),
            trust_domain_ref: profile.trust_domain_ref(),
            trust_anchor_ref: profile.trust_anchor_ref(),
            ubuntu_listener_credential_ref: profile.ubuntu_listener_credential_ref(),
            ubuntu_agent_listener_principal: profile.ubuntu_agent_listener_principal(),
            mac_agent_client_principal: profile.mac_agent_client_principal(),
            pxap_descriptor_digest,
            requirement_digest,
        })
    }

    /// Returns the exact canonical PXAD bytes committed by this requirement.
    #[must_use]
    pub(crate) fn profile_canonical_wire(&self) -> &[u8] {
        &self.profile_canonical_wire
    }

    /// Returns the canonical PXAD digest.
    #[must_use]
    pub(crate) const fn profile_digest(&self) -> Digest32 {
        self.profile_digest
    }

    /// Returns the exact Ubuntu RuntimeHost target.
    #[must_use]
    pub(crate) const fn target(&self) -> RuntimeHostId {
        self.target
    }

    /// Returns the exact canonical TLS listener endpoint.
    #[must_use]
    pub(crate) const fn ubuntu_tls_listener_endpoint(&self) -> &DistributedFabricTlsEndpointV1 {
        &self.ubuntu_tls_listener_endpoint
    }

    /// Returns the exact endpoint identity reference.
    #[must_use]
    pub(crate) const fn endpoint_ref(&self) -> [u8; 16] {
        self.endpoint_ref
    }

    /// Returns the exact endpoint generation.
    #[must_use]
    pub(crate) const fn endpoint_generation(&self) -> u64 {
        self.endpoint_generation
    }

    /// Returns the exact enrolled trust-domain reference.
    #[must_use]
    pub(crate) const fn trust_domain_ref(&self) -> DistributedFabricTrustDomainRefV1 {
        self.trust_domain_ref
    }

    /// Returns the exact enrolled trust-anchor reference.
    #[must_use]
    pub(crate) const fn trust_anchor_ref(&self) -> DistributedFabricTrustAnchorRefV1 {
        self.trust_anchor_ref
    }

    /// Returns the Ubuntu listener credential reference to resolve.
    #[must_use]
    pub(crate) const fn ubuntu_listener_credential_ref(&self) -> DistributedFabricCredentialRefV1 {
        self.ubuntu_listener_credential_ref
    }

    /// Returns the principal required from the resolved Ubuntu identity.
    #[must_use]
    pub(crate) const fn ubuntu_agent_listener_principal(&self) -> PrincipalRef {
        self.ubuntu_agent_listener_principal
    }

    /// Returns the only Mac client principal admitted by the S1 ACL.
    #[must_use]
    pub(crate) const fn mac_agent_client_principal(&self) -> PrincipalRef {
        self.mac_agent_client_principal
    }

    /// Returns the digest of the complete strict PXAP mirrored by S1.
    #[must_use]
    pub(crate) const fn pxap_descriptor_digest(&self) -> Digest32 {
        self.pxap_descriptor_digest
    }

    /// Returns the complete domain-separated requirement digest to echo.
    #[must_use]
    pub(crate) const fn requirement_digest(&self) -> Digest32 {
        self.requirement_digest
    }
}

impl fmt::Debug for RuntimeRemoteAgentS1CredentialRequirementV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeRemoteAgentS1CredentialRequirementV2")
            .field("profile_digest", &self.profile_digest)
            .field("target", &self.target)
            .field(
                "ubuntu_tls_listener_endpoint",
                &self.ubuntu_tls_listener_endpoint,
            )
            .field("endpoint_ref", &self.endpoint_ref)
            .field("endpoint_generation", &self.endpoint_generation)
            .field("trust_domain_ref", &self.trust_domain_ref)
            .field("trust_anchor_ref", &self.trust_anchor_ref)
            .field(
                "ubuntu_listener_credential_ref",
                &self.ubuntu_listener_credential_ref,
            )
            .field(
                "ubuntu_agent_listener_principal",
                &self.ubuntu_agent_listener_principal,
            )
            .field(
                "mac_agent_client_principal",
                &self.mac_agent_client_principal,
            )
            .field("pxap_descriptor_digest", &self.pxap_descriptor_digest)
            .field("requirement_digest", &self.requirement_digest)
            .finish_non_exhaustive()
    }
}

/// Stable display-safe S1 listener-credential resolution failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeRemoteAgentS1CredentialResolveErrorV2 {
    /// The credential owner could not resolve the complete exact requirement.
    ResolutionFailed,
}

impl fmt::Display for RuntimeRemoteAgentS1CredentialResolveErrorV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Runtime remote-Agent S1 credential resolution failed closed")
    }
}

impl std::error::Error for RuntimeRemoteAgentS1CredentialResolveErrorV2 {}

/// Process-local Ubuntu listener credential files plus the exact echoed digest.
///
/// The role-specific Fabric value cannot contain a connector private key. Its
/// paths remain private and are never delegated to `Debug`.
pub(crate) struct RuntimeResolvedRemoteAgentS1CredentialV2 {
    requirement_digest: Digest32,
    credential_files: ResolvedRemoteMtlsListenerCredentialFilesV1,
}

impl RuntimeResolvedRemoteAgentS1CredentialV2 {
    /// Binds one resolved listener-only file set to the requirement handled by
    /// the composition-owned resolver.
    #[must_use]
    pub(crate) const fn new(
        requirement_digest: Digest32,
        credential_files: ResolvedRemoteMtlsListenerCredentialFilesV1,
    ) -> Self {
        Self {
            requirement_digest,
            credential_files,
        }
    }

    /// Returns the resolver-echoed complete requirement digest.
    #[must_use]
    pub(crate) const fn requirement_digest(&self) -> Digest32 {
        self.requirement_digest
    }

    fn into_parts(
        self,
    ) -> (
        Digest32,
        ResolvedRemoteMtlsListenerCredentialFilesV1,
    ) {
        (self.requirement_digest, self.credential_files)
    }
}

impl fmt::Debug for RuntimeResolvedRemoteAgentS1CredentialV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeResolvedRemoteAgentS1CredentialV2")
            .field("requirement_digest", &self.requirement_digest)
            .field("credential_files", &"<redacted-resolved-paths>")
            .finish()
    }
}

/// Repeatable composition seam for one exact Ubuntu S1 listener requirement.
///
/// Implementations may own secret-store clients, but return only Fabric's
/// already validated listener-role file value. The Runtime independently
/// rejects any response that does not echo the complete requirement digest.
pub(crate) trait RuntimeRemoteAgentS1CredentialResolverV2: Send + Sync + 'static {
    fn resolve(
        &self,
        requirement: &RuntimeRemoteAgentS1CredentialRequirementV2,
    ) -> Result<
        RuntimeResolvedRemoteAgentS1CredentialV2,
        RuntimeRemoteAgentS1CredentialResolveErrorV2,
    >;
}

/// Pure, move-only S1 preparation retained across the durable open-intent edge.
///
/// Construction resolves and echoes the complete credential requirement,
/// strictly decodes the exact PXAP, builds a TLS-only Fabric configuration,
/// and reserves the nonzero session epoch. It opens no session and declares no
/// queryable. The only production transition to a live value consumes this
/// token through [`Self::start`].
pub(crate) struct RemoteAgentS1PreparedActivationV2 {
    prepared_listener: PreparedRemoteAgentProxyListenerV2,
    pxap: RemoteAgentS1ExactPxapMirrorPlanV2,
    correlation: RemoteAgentS1ActivationCorrelationV2,
}

/// Sole live process-local owner of the independent TLS-only S1 listener.
///
/// The exact PXAP mirror remains attached for later dedicated lane installs.
/// No generic Fabric service, raw route, or Session accessor is exposed.
pub(crate) struct RemoteAgentS1LiveActivationV2 {
    listener: RemoteAgentProxyListenerV2,
    pxap: RemoteAgentS1ExactPxapMirrorPlanV2,
    correlation: RemoteAgentS1ActivationCorrelationV2,
}

struct RemoteAgentS1ActivationCorrelationV2 {
    target: RuntimeHostId,
    outer_request_digest: Digest32,
    inner_request_digest: Digest32,
    profile_canonical_wire: Box<[u8]>,
    profile_digest: Digest32,
    pxap_descriptor_digest: Digest32,
    pxap_payload_digest: Digest32,
    credential_requirement_digest: Digest32,
    mac_agent_client_principal: PrincipalRef,
    proxy_session_epoch: DistributedFabricSessionEpochV1,
}

impl RemoteAgentS1ActivationCorrelationV2 {
    fn matches_request(&self, request: &RemoteAgentAccessRequestV2) -> bool {
        let Some(inner) = request.apply_request() else {
            return false;
        };
        let execution = inner.target_execution();
        let profile = execution.profile();
        request.kind() == RemoteAgentAccessKindV2::ApplyRemoteAccess
            && request.target() == self.target
            && inner.target() == self.target
            && request.request_digest() == self.outer_request_digest
            && inner.request_digest() == self.inner_request_digest
            && execution.mode() == RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive
            && profile.target() == self.target
            && profile.canonical_wire() == self.profile_canonical_wire.as_ref()
            && profile.profile_digest() == self.profile_digest
            && profile.mac_agent_client_principal() == self.mac_agent_client_principal
            && request
                .retained_s0_cas()
                .fields()
                .expected_descriptor_payload_digest
                == self.pxap_payload_digest
            && execution
                .retained_s0_cas()
                .fields()
                .expected_descriptor_payload_digest
                == self.pxap_payload_digest
    }
}

impl RemoteAgentS1PreparedActivationV2 {
    /// Performs every fallible resolver, PXAD/PXAP, endpoint, configuration,
    /// and epoch-reservation check before the first transport effect.
    pub(crate) fn try_prepare<'request, 'running>(
        verified_ingress: &VerifiedRemoteAgentAccessApplyIngressV2<'request, 'running>,
        exact_pxap: &[u8],
        resolver: &dyn RuntimeRemoteAgentS1CredentialResolverV2,
        expected_target: RuntimeHostId,
    ) -> Result<Self, RemoteAgentS1PreparationErrorV2> {
        Self::try_prepare_request(
            verified_ingress.request(),
            exact_pxap,
            resolver,
            expected_target,
        )
    }

    fn try_prepare_request(
        request: &RemoteAgentAccessRequestV2,
        exact_pxap: &[u8],
        resolver: &dyn RuntimeRemoteAgentS1CredentialResolverV2,
        expected_target: RuntimeHostId,
    ) -> Result<Self, RemoteAgentS1PreparationErrorV2> {
        if request.kind() != RemoteAgentAccessKindV2::ApplyRemoteAccess {
            return Err(RemoteAgentS1PreparationErrorV2::NotApplyRequest);
        }
        let inner = request
            .apply_request()
            .ok_or(RemoteAgentS1PreparationErrorV2::NotApplyRequest)?;
        let execution = inner.target_execution();
        let profile = execution.profile();
        if request.target() != expected_target
            || inner.target() != expected_target
            || profile.target() != expected_target
        {
            return Err(RemoteAgentS1PreparationErrorV2::TargetMismatch);
        }
        if execution.mode() != RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive {
            return Err(RemoteAgentS1PreparationErrorV2::ModeMismatch);
        }

        let pxap = RemoteAgentS1ExactPxapMirrorPlanV2::try_from_exact_pxap(exact_pxap)
            .map_err(RemoteAgentS1PreparationErrorV2::Pxap)?;
        let pxap_payload_digest = runtime_agent_control_descriptor_payload_digest_v1(exact_pxap)
            .map_err(RemoteAgentS1PreparationErrorV2::PxapPayloadDigest)?;
        if request
            .retained_s0_cas()
            .fields()
            .expected_descriptor_payload_digest
            != pxap_payload_digest
            || execution
                .retained_s0_cas()
                .fields()
                .expected_descriptor_payload_digest
                != pxap_payload_digest
        {
            return Err(RemoteAgentS1PreparationErrorV2::PxapCorrelationMismatch);
        }

        let requirement =
            RuntimeRemoteAgentS1CredentialRequirementV2::try_from_profile_and_pxap(profile, &pxap)
                .map_err(RemoteAgentS1PreparationErrorV2::RequirementDigest)?;
        let resolved = resolver
            .resolve(&requirement)
            .map_err(RemoteAgentS1PreparationErrorV2::CredentialResolution)?;
        let (echoed_requirement_digest, credential_files) = resolved.into_parts();
        if echoed_requirement_digest != requirement.requirement_digest() {
            return Err(RemoteAgentS1PreparationErrorV2::CredentialEchoMismatch);
        }

        let remote_endpoint = RemoteTlsEndpoint::try_new(
            profile.ubuntu_tls_listener_endpoint().as_str().to_owned(),
        )
        .map_err(RemoteAgentS1PreparationErrorV2::FabricConfiguration)?;
        if remote_endpoint.as_str() != profile.ubuntu_tls_listener_endpoint().as_str() {
            return Err(RemoteAgentS1PreparationErrorV2::EndpointMappingMismatch);
        }
        let config = FabricServiceConfig::try_remote_agent_proxy_listener_v2(
            remote_endpoint,
            credential_files,
            profile.mac_agent_client_principal(),
            pxap.submit_lane().key_expression().to_owned(),
            pxap.control_lane().key_expression().to_owned(),
        )
        .map_err(RemoteAgentS1PreparationErrorV2::FabricConfiguration)?;
        let prepared_listener = PreparedRemoteAgentProxyListenerV2::try_prepare(config)
            .map_err(RemoteAgentS1PreparationErrorV2::FabricPreparation)?;
        let proxy_session_epoch = prepared_listener.session_epoch();
        let correlation = RemoteAgentS1ActivationCorrelationV2 {
            target: expected_target,
            outer_request_digest: request.request_digest(),
            inner_request_digest: inner.request_digest(),
            profile_canonical_wire: profile.canonical_wire().into(),
            profile_digest: profile.profile_digest(),
            pxap_descriptor_digest: pxap.descriptor_digest(),
            pxap_payload_digest,
            credential_requirement_digest: requirement.requirement_digest(),
            mac_agent_client_principal: profile.mac_agent_client_principal(),
            proxy_session_epoch,
        };
        if !correlation.matches_request(request) {
            return Err(RemoteAgentS1PreparationErrorV2::RequestCorrelationMismatch);
        }
        Ok(Self {
            prepared_listener,
            pxap,
            correlation,
        })
    }

    /// Returns the exact nonzero epoch that must be committed by S1OpenIntent.
    #[must_use]
    pub(crate) const fn reserved_proxy_session_epoch(&self) -> DistributedFabricSessionEpochV1 {
        self.correlation.proxy_session_epoch
    }

    /// Rechecks exact request/profile/client/PXAP correlation at the state seam.
    #[must_use]
    pub(crate) fn matches_request(&self, request: &RemoteAgentAccessRequestV2) -> bool {
        self.correlation.matches_request(request)
            && self.pxap.descriptor_digest() == self.correlation.pxap_descriptor_digest
    }

    /// Returns the complete resolver requirement digest retained by this plan.
    #[must_use]
    pub(crate) const fn credential_requirement_digest(&self) -> Digest32 {
        self.correlation.credential_requirement_digest
    }

    /// Consumes the one-shot prepared listener and performs the first transport
    /// effect. Any failure returns no live owner token and cannot be retried.
    pub(crate) async fn start(
        self,
    ) -> Result<RemoteAgentS1LiveActivationV2, RemoteAgentS1StartErrorV2> {
        self.start_inner(|prepared| prepared.start()).await
    }

    async fn start_inner<Start, StartFuture>(
        self,
        start: Start,
    ) -> Result<RemoteAgentS1LiveActivationV2, RemoteAgentS1StartErrorV2>
    where
        Start: FnOnce(PreparedRemoteAgentProxyListenerV2) -> StartFuture,
        StartFuture: core::future::Future<Output = Result<RemoteAgentProxyListenerV2, FabricError>>,
    {
        let Self {
            prepared_listener,
            pxap,
            correlation,
        } = self;
        let listener = start(prepared_listener)
            .await
            .map_err(RemoteAgentS1StartErrorV2::Start)?;
        if listener.session_epoch() != correlation.proxy_session_epoch {
            return match listener.shutdown().await {
                Ok(()) => Err(RemoteAgentS1StartErrorV2::LiveEpochMismatch),
                Err(error) => Err(RemoteAgentS1StartErrorV2::LiveEpochMismatchCleanupUncertain(
                    error,
                )),
            };
        }
        Ok(RemoteAgentS1LiveActivationV2 {
            listener,
            pxap,
            correlation,
        })
    }
}

impl fmt::Debug for RemoteAgentS1PreparedActivationV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAgentS1PreparedActivationV2")
            .field("target", &self.correlation.target)
            .field("profile_digest", &self.correlation.profile_digest)
            .field(
                "pxap_descriptor_digest",
                &self.correlation.pxap_descriptor_digest,
            )
            .field(
                "credential_requirement_digest",
                &self.correlation.credential_requirement_digest,
            )
            .field("proxy_session_epoch", &"<reserved-nonzero>")
            .finish_non_exhaustive()
    }
}

impl RemoteAgentS1LiveActivationV2 {
    /// Returns the exact live epoch already checked against the reserved plan.
    #[must_use]
    pub(crate) fn live_proxy_session_epoch(&self) -> DistributedFabricSessionEpochV1 {
        self.listener.session_epoch()
    }

    /// Rechecks exact request/profile/client/PXAP correlation at the state seam.
    #[must_use]
    pub(crate) fn matches_request(&self, request: &RemoteAgentAccessRequestV2) -> bool {
        self.correlation.matches_request(request)
            && self.pxap.descriptor_digest() == self.correlation.pxap_descriptor_digest
            && self.live_proxy_session_epoch() == self.correlation.proxy_session_epoch
    }

    /// Returns the complete credential requirement retained from preparation.
    #[must_use]
    pub(crate) const fn credential_requirement_digest(&self) -> Digest32 {
        self.correlation.credential_requirement_digest
    }

    /// Borrows the exact move-only PXAP plan only for later dedicated S1 lane
    /// installation. It cannot be converted into generic Fabric mutation.
    #[must_use]
    pub(crate) const fn pxap(&self) -> &RemoteAgentS1ExactPxapMirrorPlanV2 {
        &self.pxap
    }

    /// Consumes the live owner and closes S1. A close error is outcome-uncertain
    /// and returns no reusable listener authority.
    pub(crate) async fn shutdown(self) -> Result<(), RemoteAgentS1ShutdownErrorV2> {
        self.shutdown_inner(|listener| listener.shutdown()).await
    }

    async fn shutdown_inner<Shutdown, ShutdownFuture>(
        self,
        shutdown: Shutdown,
    ) -> Result<(), RemoteAgentS1ShutdownErrorV2>
    where
        Shutdown: FnOnce(RemoteAgentProxyListenerV2) -> ShutdownFuture,
        ShutdownFuture: core::future::Future<Output = Result<(), FabricError>>,
    {
        let Self {
            listener,
            pxap: _pxap,
            correlation: _correlation,
        } = self;
        shutdown(listener)
            .await
            .map_err(RemoteAgentS1ShutdownErrorV2::Shutdown)
    }
}

impl fmt::Debug for RemoteAgentS1LiveActivationV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAgentS1LiveActivationV2")
            .field("target", &self.correlation.target)
            .field("profile_digest", &self.correlation.profile_digest)
            .field(
                "pxap_descriptor_digest",
                &self.correlation.pxap_descriptor_digest,
            )
            .field("proxy_session_epoch", &"<live-nonzero>")
            .finish_non_exhaustive()
    }
}

/// Fail-closed pure preparation errors. No variant contains resolved paths.
#[derive(Debug)]
pub(crate) enum RemoteAgentS1PreparationErrorV2 {
    NotApplyRequest,
    TargetMismatch,
    ModeMismatch,
    Pxap(AgentConversationPortDescriptorError),
    PxapPayloadDigest(ManagedServingBootstrapError),
    PxapCorrelationMismatch,
    RequirementDigest(DigestBuildError),
    CredentialResolution(RuntimeRemoteAgentS1CredentialResolveErrorV2),
    CredentialEchoMismatch,
    EndpointMappingMismatch,
    FabricConfiguration(FabricConfigError),
    FabricPreparation(FabricError),
    RequestCorrelationMismatch,
}

impl fmt::Display for RemoteAgentS1PreparationErrorV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("remote Agent S1 preparation failed closed: ")?;
        match self {
            Self::Pxap(error) => write!(formatter, "PXAP: {error}"),
            Self::PxapPayloadDigest(error) => write!(formatter, "PXAP payload digest: {error}"),
            Self::RequirementDigest(error) => write!(formatter, "requirement digest: {error}"),
            Self::CredentialResolution(error) => write!(formatter, "credential owner: {error}"),
            Self::FabricConfiguration(error) => write!(formatter, "Fabric config: {error}"),
            Self::FabricPreparation(error) => write!(formatter, "Fabric prepare: {error}"),
            other => write!(formatter, "{other:?}"),
        }
    }
}

impl std::error::Error for RemoteAgentS1PreparationErrorV2 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pxap(error) => Some(error),
            Self::PxapPayloadDigest(error) => Some(error),
            Self::RequirementDigest(error) => Some(error),
            Self::CredentialResolution(error) => Some(error),
            Self::FabricConfiguration(error) => Some(error),
            Self::FabricPreparation(error) => Some(error),
            _ => None,
        }
    }
}

/// A consumed start failed or could not prove the reserved/live epoch relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentS1StartErrorV2 {
    Start(FabricError),
    LiveEpochMismatch,
    LiveEpochMismatchCleanupUncertain(FabricError),
}

impl fmt::Display for RemoteAgentS1StartErrorV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("remote Agent S1 start outcome is fail-closed")
    }
}

impl std::error::Error for RemoteAgentS1StartErrorV2 {}

/// A consuming S1 shutdown could not prove listener closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentS1ShutdownErrorV2 {
    Shutdown(FabricError),
}

impl fmt::Display for RemoteAgentS1ShutdownErrorV2 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("remote Agent S1 shutdown outcome is uncertain")
    }
}

impl std::error::Error for RemoteAgentS1ShutdownErrorV2 {}

fn s1_credential_requirement_digest_v2(
    profile: &RemoteAgentDataPlaneProfileV1,
    pxap_descriptor_digest: Digest32,
) -> Result<Digest32, DigestBuildError> {
    let mut builder = Digest32Builder::try_new(S1_CREDENTIAL_REQUIREMENT_DIGEST_DOMAIN_V2)?;
    builder.field_bytes(profile.canonical_wire())?;
    builder.field_digest(&profile.profile_digest())?;
    builder.field_bytes(profile.target().as_bytes())?;
    builder.field_bytes(profile.ubuntu_tls_listener_endpoint().as_str().as_bytes())?;
    builder.field_bytes(&profile.endpoint_ref())?;
    builder.field_u64(profile.endpoint_generation())?;
    builder.field_bytes(profile.trust_domain_ref().as_bytes())?;
    builder.field_bytes(profile.trust_anchor_ref().as_bytes())?;
    builder.field_bytes(profile.ubuntu_listener_credential_ref().as_bytes())?;
    builder.field_bytes(profile.ubuntu_agent_listener_principal().as_bytes())?;
    builder.field_bytes(profile.mac_agent_client_principal().as_bytes())?;
    builder.field_digest(&pxap_descriptor_digest)?;
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use core::time::Duration;
    use std::path::PathBuf;

    use paraegox_fabric::{
        BindingEpoch, IngressLimits, PortBindingDescriptorV1,
        ResolvedRemoteMtlsIdentityFiles,
    };
    use paraegox_runtime_contracts::{
        assignment::{BindingId, SchemaRef},
        remote_agent_data_plane_plan::RemoteAgentDataPlaneProfileFieldsV1,
    };

    use super::*;
    use crate::managed_agent_transport::AgentConversationPortDescriptorV1;

    const COMMAND_SCHEMA_ID: [u8; 16] = [
        0x50, 0x58, 0x41, 0x43, 0x2d, 0x43, 0x4f, 0x4d, 0x4d, 0x41, 0x4e, 0x44, 0x2d, 0x56,
        0x31, 0x00,
    ];
    const RESULT_SCHEMA_ID: [u8; 16] = [
        0x50, 0x58, 0x41, 0x43, 0x2d, 0x52, 0x45, 0x53, 0x55, 0x4c, 0x54, 0x2d, 0x56, 0x31,
        0x00, 0x00,
    ];
    const COMMAND_SCHEMA_DIGEST: [u8; 32] = [
        0x33, 0xe4, 0x47, 0x1d, 0x88, 0x93, 0x05, 0x94, 0xf2, 0x3d, 0xa3, 0x99, 0x9c, 0x3d,
        0x52, 0x7d, 0xf1, 0xb2, 0x20, 0x13, 0x3b, 0x40, 0xde, 0x78, 0xdd, 0xbd, 0xa9, 0x17,
        0x69, 0xe5, 0x7e, 0x76,
    ];
    const RESULT_SCHEMA_DIGEST: [u8; 32] = [
        0x60, 0x13, 0x67, 0xec, 0x50, 0xbf, 0x9e, 0xbc, 0x7b, 0xc7, 0xbb, 0x94, 0xd0, 0x44,
        0xc5, 0x4c, 0x44, 0x06, 0x4b, 0xb9, 0x85, 0x93, 0xb7, 0xfc, 0xea, 0x0b, 0xd0, 0x29,
        0x84, 0x4a, 0xd2, 0x64,
    ];

    #[derive(Clone)]
    struct ProfileFixture {
        target: [u8; 16],
        base_endpoint: &'static str,
        tls_endpoint: &'static str,
        endpoint_ref: [u8; 16],
        endpoint_generation: u64,
        trust_domain: [u8; 16],
        trust_anchor: [u8; 16],
        mac_credential: [u8; 16],
        ubuntu_credential: [u8; 16],
        mac_principal: [u8; 16],
        ubuntu_principal: [u8; 16],
    }

    impl Default for ProfileFixture {
        fn default() -> Self {
            Self {
                target: [0x81; 16],
                base_endpoint: "tcp/127.0.0.1:7447",
                tls_endpoint: "tls/192.0.2.81:7448",
                endpoint_ref: [0x82; 16],
                endpoint_generation: 7,
                trust_domain: [0x83; 16],
                trust_anchor: [0x84; 16],
                mac_credential: [0x85; 16],
                ubuntu_credential: [0x86; 16],
                mac_principal: [0x87; 16],
                ubuntu_principal: [0x88; 16],
            }
        }
    }

    fn profile(fixture: &ProfileFixture) -> RemoteAgentDataPlaneProfileV1 {
        RemoteAgentDataPlaneProfileV1::try_new(RemoteAgentDataPlaneProfileFieldsV1 {
            target: RuntimeHostId::from_bytes(fixture.target),
            base_loopback_listen_endpoint: fixture.base_endpoint,
            ubuntu_tls_listener_endpoint: fixture.tls_endpoint,
            endpoint_ref: fixture.endpoint_ref,
            endpoint_generation: fixture.endpoint_generation,
            trust_domain_ref: DistributedFabricTrustDomainRefV1::try_from_bytes(
                fixture.trust_domain,
            )
            .expect("nonzero trust domain"),
            trust_anchor_ref: DistributedFabricTrustAnchorRefV1::try_from_bytes(
                fixture.trust_anchor,
            )
            .expect("nonzero trust anchor"),
            mac_connector_credential_ref: DistributedFabricCredentialRefV1::try_from_bytes(
                fixture.mac_credential,
            )
            .expect("nonzero Mac credential"),
            ubuntu_listener_credential_ref: DistributedFabricCredentialRefV1::try_from_bytes(
                fixture.ubuntu_credential,
            )
            .expect("nonzero Ubuntu credential"),
            mac_agent_client_principal: PrincipalRef::from_bytes(fixture.mac_principal),
            ubuntu_agent_listener_principal: PrincipalRef::from_bytes(fixture.ubuntu_principal),
            operation_timeout_nanos: 1_000_000_000,
        })
        .expect("exact PXAD fixture")
    }

    fn schema(id: [u8; 16], digest: [u8; 32]) -> SchemaRef {
        SchemaRef::try_new(id, 1, Digest32::from_bytes(digest)).expect("nonzero schema version")
    }

    fn pxap(seed: u8) -> RemoteAgentS1ExactPxapMirrorPlanV2 {
        let limits = IngressLimits::try_new(
            4,
            16_384,
            4_096,
            4_096,
            Duration::from_secs(2),
        )
        .expect("bounded ingress");
        let descriptor = AgentConversationPortDescriptorV1::try_new(
            PortBindingDescriptorV1::try_new(
                BindingId::from_bytes([seed; 16]),
                BindingEpoch::try_new(3).expect("nonzero submit epoch"),
                "paraegox/agent/s1/submit",
                schema(COMMAND_SCHEMA_ID, COMMAND_SCHEMA_DIGEST),
                schema(RESULT_SCHEMA_ID, RESULT_SCHEMA_DIGEST),
                limits,
            )
            .expect("submit descriptor"),
            PortBindingDescriptorV1::try_new(
                BindingId::from_bytes([seed.wrapping_add(1); 16]),
                BindingEpoch::try_new(4).expect("nonzero control epoch"),
                "paraegox/agent/s1/control",
                schema(COMMAND_SCHEMA_ID, COMMAND_SCHEMA_DIGEST),
                schema(RESULT_SCHEMA_ID, RESULT_SCHEMA_DIGEST),
                limits,
            )
            .expect("control descriptor"),
        )
        .expect("exact PXAP");
        RemoteAgentS1ExactPxapMirrorPlanV2::try_from_exact_pxap(descriptor.canonical_wire())
            .expect("strict S1 PXAP plan")
    }

    fn requirement(
        fixture: &ProfileFixture,
        pxap: &RemoteAgentS1ExactPxapMirrorPlanV2,
    ) -> RuntimeRemoteAgentS1CredentialRequirementV2 {
        RuntimeRemoteAgentS1CredentialRequirementV2::try_from_profile_and_pxap(
            &profile(fixture),
            pxap,
        )
        .expect("bounded requirement digest")
    }

    #[test]
    fn s1_requirement_binds_complete_profile_listener_identity_client_and_pxap() {
        let base = ProfileFixture::default();
        let pxap_a = pxap(0x91);
        let expected = requirement(&base, &pxap_a);
        let expected_profile = profile(&base);
        assert_eq!(
            expected.profile_canonical_wire(),
            expected_profile.canonical_wire()
        );
        assert_eq!(expected.profile_digest(), expected_profile.profile_digest());
        assert_eq!(expected.target(), RuntimeHostId::from_bytes(base.target));
        assert_eq!(
            expected.ubuntu_tls_listener_endpoint().as_str(),
            base.tls_endpoint
        );
        assert_eq!(expected.endpoint_ref(), base.endpoint_ref);
        assert_eq!(expected.endpoint_generation(), base.endpoint_generation);
        assert_eq!(expected.trust_domain_ref().as_bytes(), &base.trust_domain);
        assert_eq!(expected.trust_anchor_ref().as_bytes(), &base.trust_anchor);
        assert_eq!(
            expected.ubuntu_listener_credential_ref().as_bytes(),
            &base.ubuntu_credential
        );
        assert_eq!(
            expected.ubuntu_agent_listener_principal().as_bytes(),
            &base.ubuntu_principal
        );
        assert_eq!(
            expected.mac_agent_client_principal().as_bytes(),
            &base.mac_principal
        );
        assert_eq!(expected.pxap_descriptor_digest(), pxap_a.descriptor_digest());

        let mut variants = Vec::new();
        let mut changed = base.clone();
        changed.target = [0xa1; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.base_endpoint = "tcp/127.0.0.1:7449";
        variants.push(changed);
        let mut changed = base.clone();
        changed.tls_endpoint = "tls/192.0.2.82:7448";
        variants.push(changed);
        let mut changed = base.clone();
        changed.endpoint_ref = [0xa2; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.endpoint_generation += 1;
        variants.push(changed);
        let mut changed = base.clone();
        changed.trust_domain = [0xa3; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.trust_anchor = [0xa4; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.mac_credential = [0xa5; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.ubuntu_credential = [0xa6; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.mac_principal = [0xa7; 16];
        variants.push(changed);
        let mut changed = base.clone();
        changed.ubuntu_principal = [0xa8; 16];
        variants.push(changed);

        for changed in variants {
            assert_ne!(
                requirement(&changed, &pxap_a).requirement_digest(),
                expected.requirement_digest()
            );
        }
        assert_ne!(
            requirement(&base, &pxap(0xb1)).requirement_digest(),
            expected.requirement_digest()
        );

        let mut other_domain = Digest32Builder::try_new(
            b"paraegox.runtime.remote-agent-s1-listener-credential-requirement.sha256.other",
        )
        .expect("bounded alternate domain");
        other_domain
            .field_bytes(expected.profile_canonical_wire())
            .expect("bounded field");
        assert_ne!(other_domain.finish(), expected.requirement_digest());
    }

    #[test]
    fn resolved_s1_credentials_echo_only_digest_and_debug_redacts_every_path() {
        let expected = requirement(&ProfileFixture::default(), &pxap(0x91));
        let files = ResolvedRemoteMtlsListenerCredentialFilesV1::try_new(
            PathBuf::from("/secret/s1/root-ca.pem"),
            ResolvedRemoteMtlsIdentityFiles::try_new(
                PathBuf::from("/secret/s1/listener-cert.pem"),
                PathBuf::from("/secret/s1/listener-key.pem"),
            )
            .expect("absolute listener identity paths"),
        )
        .expect("absolute listener credential paths");
        let resolved = RuntimeResolvedRemoteAgentS1CredentialV2::new(
            expected.requirement_digest(),
            files,
        );
        assert_eq!(
            resolved.requirement_digest(),
            expected.requirement_digest()
        );
        let debug = format!("{resolved:?}");
        assert!(debug.contains("<redacted-resolved-paths>"));
        for secret in ["/secret", "root-ca.pem", "listener-cert.pem", "listener-key.pem"] {
            assert!(!debug.contains(secret));
        }
        let (echo, _files) = resolved.into_parts();
        assert_eq!(echo, expected.requirement_digest());

        let source = include_str!("remote_agent_s1_runtime.rs");
        let before_impl = source
            .split_once("impl RuntimeResolvedRemoteAgentS1CredentialV2 {")
            .expect("resolved credential impl must remain present")
            .0;
        let declaration = before_impl
            .rsplit_once("pub(crate) struct RuntimeResolvedRemoteAgentS1CredentialV2 {")
            .expect("resolved credential declaration must remain present")
            .1;
        assert!(!declaration.contains("derive(Clone"));
        assert!(!declaration.contains("derive(Copy"));
        assert!(!source.contains("impl Clone for RuntimeResolvedRemoteAgentS1CredentialV2"));
        assert!(!source.contains("impl Copy for RuntimeResolvedRemoteAgentS1CredentialV2"));
        assert!(!source.contains("pub fn credential_files"));
        assert!(!source.contains("pub fn into_parts"));
    }

    #[test]
    fn s1_closeout_enabler_stays_internal_move_only_and_effect_ordered() {
        let source = include_str!("remote_agent_s1_runtime.rs");
        let crate_root = include_str!("lib.rs");
        assert!(crate_root.contains(concat!(
            "reason = \"Bounded closeout enabler; production owner absent; ",
            "next batch must consume or remove\""
        )));
        assert!(!crate_root.contains("pub use remote_agent_s1_runtime"));
        for declaration in [
            "pub(crate) struct RuntimeRemoteAgentS1CredentialRequirementV2",
            "pub(crate) enum RuntimeRemoteAgentS1CredentialResolveErrorV2",
            "pub(crate) struct RuntimeResolvedRemoteAgentS1CredentialV2",
            "pub(crate) trait RuntimeRemoteAgentS1CredentialResolverV2",
            "pub(crate) struct RemoteAgentS1PreparedActivationV2",
            "pub(crate) struct RemoteAgentS1LiveActivationV2",
        ] {
            assert!(source.contains(declaration), "missing internal seam: {declaration}");
        }
        for public_declaration in [
            "pub struct RuntimeRemoteAgentS1CredentialRequirementV2",
            "pub enum RuntimeRemoteAgentS1CredentialResolveErrorV2",
            "pub struct RuntimeResolvedRemoteAgentS1CredentialV2",
            "pub trait RuntimeRemoteAgentS1CredentialResolverV2",
        ] {
            assert!(!source.contains(public_declaration));
        }
        for token in [
            "RemoteAgentS1PreparedActivationV2",
            "RemoteAgentS1LiveActivationV2",
        ] {
            assert!(!source.contains(&format!("#[derive(Clone)]\npub(crate) struct {token}")));
            assert!(!source.contains(&format!("#[derive(Copy)]\npub(crate) struct {token}")));
            assert!(!source.contains(&format!("impl Clone for {token}")));
            assert!(!source.contains(&format!("impl Copy for {token}")));
        }

        let pure_prepare = source
            .split_once("    fn try_prepare_request(")
            .and_then(|(_, tail)| {
                tail.split_once("    /// Returns the exact nonzero epoch")
                    .map(|(body, _)| body)
            })
            .expect("pure preparation implementation missing");
        assert!(pure_prepare.contains("PreparedRemoteAgentProxyListenerV2::try_prepare(config)"));
        assert!(!pure_prepare.contains(".start("));
        assert!(!pure_prepare.contains(".await"));
        assert!(source.contains("pub(crate) async fn start(\n        self,"));
        assert!(source.contains("pub(crate) async fn shutdown(self)"));
        for escape in [
            "pub(crate) fn session(",
            "pub(crate) fn into_session(",
            "pub(crate) fn routes(",
            "pub(crate) fn declare_queryable(",
            "pub(crate) fn install_binding(",
        ] {
            assert!(!source.contains(escape), "raw effect escape admitted: {escape}");
        }

        let _ = RuntimeRemoteAgentS1CredentialResolveErrorV2::ResolutionFailed;
        let _ = RemoteAgentS1PreparedActivationV2::try_prepare;
        let _ = RemoteAgentS1PreparedActivationV2::start;
        let _ = RemoteAgentS1LiveActivationV2::pxap;
        let _ = RemoteAgentS1LiveActivationV2::shutdown;
    }
}
