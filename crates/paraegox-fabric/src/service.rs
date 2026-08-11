//! The single-session FabricService and its owned request-binding workers.

use core::{fmt, future::Future, num::NonZeroU64, time::Duration};
use std::{
    collections::BTreeMap,
    io::Read,
    net::Ipv4Addr,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use paraegox_kernel::{digest::Digest32, identity::PrincipalRef};
use paraegox_runtime_contracts::{
    assignment::{BindingId, SchemaRef},
    distributed_agent_stack_plan::DistributedFabricSessionEpochV1,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use zenoh::{
    key_expr::OwnedNonWildKeyExpr,
    query::{Query, Queryable},
};

use crate::{
    contract::{
        BindingEpoch, BindingRequestEnvelopeV1, BindingResponseEnvelopeV1, FabricContractError,
        MAX_ENVELOPE_BODY_BYTES, REQUEST_HEADER_BYTES, RequestHeaderDisposition, RequestId,
        ResponseStatus, prevalidate_request_header, validate_binding_id,
    },
    ingress::{FabricIngressSnapshot, IngressBudget, IngressLease, IngressLimits},
    runtime_apply::restricted_runtime_apply_peer_certificate_common_name_v1,
};

const MAX_ENDPOINT_BYTES: usize = 256;
const MAX_ENDPOINTS: usize = 8;
const MAX_EXPERIMENTAL_PEER_COMMON_NAME_BYTES: usize = 253;
const MAX_EXPERIMENTAL_OBSERVED_LINKS: usize = 64;
const MAX_EXPERIMENTAL_ZENOH_ID_TEXT_BYTES: usize = 32;
const MAX_OBSERVED_LOCATOR_BYTES: usize = 512;
const MAX_KEY_EXPRESSION_BYTES: usize = 256;
const MAX_TLS_FILE_PATH_BYTES: usize = 4_096;
const LOOPBACK_TCP_PREFIX: &str = "tcp/127.0.0.1:";
const REMOTE_TLS_PREFIX: &str = "tls/";
const REMOTE_AGENT_ZENOH_FRAMING_ALLOWANCE_BYTES: usize = 64 * 1_024;
// PXFQ has a 104-byte header and PXFP adds four bytes. Bound the larger
// canonical response plus a fixed Zenoh framing allowance.
const REMOTE_AGENT_TRANSPORT_MAX_MESSAGE_BYTES: usize =
    MAX_ENVELOPE_BODY_BYTES + REQUEST_HEADER_BYTES + 4 + REMOTE_AGENT_ZENOH_FRAMING_ALLOWANCE_BYTES;

type OwnedQueryable = Queryable<()>;

fn try_fabric_session_epoch_with(
    fill: impl FnOnce(&mut [u8; 16]) -> Result<(), ()>,
) -> Result<DistributedFabricSessionEpochV1, FabricError> {
    let mut bytes = [0_u8; 16];
    fill(&mut bytes).map_err(|()| FabricError::SessionEpochUnavailable)?;
    DistributedFabricSessionEpochV1::try_from_bytes(bytes)
        .map_err(|_| FabricError::SessionEpochUnavailable)
}

/// A validated explicit plaintext endpoint for the host-local profile.
///
/// This type deliberately admits only canonical IPv4 loopback TCP. A
/// non-loopback endpoint must be represented by [`RemoteTlsEndpoint`] and may
/// enter a session only through the secured-hybrid or role-specific remote
/// Agent constructors on [`FabricServiceConfig`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionEndpoint(String);

impl SessionEndpoint {
    /// Validates canonical `tcp/127.0.0.1:PORT` without exposing a Zenoh type.
    pub fn try_new(value: impl Into<String>) -> Result<Self, FabricConfigError> {
        let value = value.into();
        if value.is_empty() {
            return Err(FabricConfigError::EmptyEndpoint);
        }
        if value.len() > MAX_ENDPOINT_BYTES {
            return Err(FabricConfigError::EndpointTooLong);
        }
        if !value.starts_with("tcp/") {
            return Err(FabricConfigError::UnsupportedEndpointProtocol);
        }
        if !is_canonical_loopback_tcp_endpoint(&value) {
            return Err(FabricConfigError::NonCanonicalLoopbackEndpoint);
        }
        let parsed = value
            .parse::<zenoh::config::EndPoint>()
            .map_err(|_| FabricConfigError::InvalidEndpoint)?;
        if parsed.as_str() != value
            || parsed.protocol().as_str() != "tcp"
            || !parsed.metadata().as_str().is_empty()
            || !parsed.config().as_str().is_empty()
        {
            return Err(FabricConfigError::InvalidEndpoint);
        }
        Ok(Self(value))
    }

    /// Returns the explicit endpoint string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical non-loopback unicast IPv4 TLS-over-TCP endpoint.
///
/// This is the implementation-side counterpart of the P5 distributed Fabric
/// endpoint contract. It contains no credential, trust, peer identity, or
/// authorization claim. It may enter a session only through the secured-hybrid
/// or role-specific remote Agent constructors on [`FabricServiceConfig`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteTlsEndpoint(String);

impl RemoteTlsEndpoint {
    /// Accepts only shortest-form `tls/A.B.C.D:PORT` non-loopback endpoints.
    pub fn try_new(value: impl Into<String>) -> Result<Self, FabricConfigError> {
        let value = value.into();
        if value.is_empty() {
            return Err(FabricConfigError::EmptyEndpoint);
        }
        if value.len() > MAX_ENDPOINT_BYTES {
            return Err(FabricConfigError::EndpointTooLong);
        }
        if !value.starts_with(REMOTE_TLS_PREFIX) {
            return Err(FabricConfigError::UnsupportedEndpointProtocol);
        }
        if !is_canonical_remote_tls_endpoint(&value) {
            return Err(FabricConfigError::NonCanonicalRemoteTlsEndpoint);
        }
        let parsed = value
            .parse::<zenoh::config::EndPoint>()
            .map_err(|_| FabricConfigError::InvalidEndpoint)?;
        if parsed.as_str() != value
            || parsed.protocol().as_str() != "tls"
            || !parsed.metadata().as_str().is_empty()
            || !parsed.config().as_str().is_empty()
        {
            return Err(FabricConfigError::InvalidEndpoint);
        }
        Ok(Self(value))
    }

    /// Returns the exact explicit endpoint string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A bounded canonical certificate Common Name admitted only by the
/// experimental Zenoh 1.9 live-link observation path.
///
/// This value is deliberately narrower than an arbitrary X.509 Common Name:
/// it is a lower-case DNS-style name whose labels contain only ASCII letters,
/// digits, and interior hyphens. The live link must match it byte-for-byte.
/// CN is not presented as a production peer identity; a future production
/// successor requires structured SAN/fingerprint evidence from Zenoh.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExperimentalPeerCommonNameV1(Box<str>);

impl ExperimentalPeerCommonNameV1 {
    /// Validates the exact experimental lower-case DNS-style CN form.
    pub fn try_new(value: impl Into<String>) -> Result<Self, ExperimentalRemoteMtlsConfigErrorV1> {
        let value = value.into();
        if !is_canonical_experimental_peer_common_name(&value) {
            return Err(ExperimentalRemoteMtlsConfigErrorV1::InvalidPeerCommonName);
        }
        Ok(Self(value.into_boxed_str()))
    }

    /// Returns the exact CN used for an in-process byte-for-byte comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ExperimentalPeerCommonNameV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExperimentalPeerCommonNameV1(<redacted-cn>)")
    }
}

/// One resolver-owned experimental CN binding for an explicit TLS peer row.
///
/// The digest is computed by Runtime from the complete PXDT requirement,
/// expected peer-identity reference, and canonical CN. Fabric never receives
/// the opaque identity reference and never treats the CN as production proof.
#[derive(Clone, Eq, PartialEq)]
pub struct ExperimentalRemoteMtlsPeerBindingV1 {
    connect_endpoint: RemoteTlsEndpoint,
    expected_common_name: ExperimentalPeerCommonNameV1,
    identity_binding_digest: Digest32,
}

impl ExperimentalRemoteMtlsPeerBindingV1 {
    /// Binds one explicit TLS connector to one Runtime-computed CN digest.
    #[must_use]
    pub const fn new(
        connect_endpoint: RemoteTlsEndpoint,
        expected_common_name: ExperimentalPeerCommonNameV1,
        identity_binding_digest: Digest32,
    ) -> Self {
        Self {
            connect_endpoint,
            expected_common_name,
            identity_binding_digest,
        }
    }

    /// Returns the exact configured remote TLS connector.
    #[must_use]
    pub const fn connect_endpoint(&self) -> &RemoteTlsEndpoint {
        &self.connect_endpoint
    }

    /// Returns the canonical expected CN for resolver/config validation.
    #[must_use]
    pub const fn expected_common_name(&self) -> &ExperimentalPeerCommonNameV1 {
        &self.expected_common_name
    }

    /// Returns the Runtime-computed identity-binding digest.
    #[must_use]
    pub const fn identity_binding_digest(&self) -> Digest32 {
        self.identity_binding_digest
    }
}

impl fmt::Debug for ExperimentalRemoteMtlsPeerBindingV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExperimentalRemoteMtlsPeerBindingV1")
            .field("connect_endpoint", &self.connect_endpoint)
            .field("expected_common_name", &"<redacted-cn>")
            .field("identity_binding_digest", &self.identity_binding_digest)
            .finish()
    }
}

/// Owner-resolved certificate and private-key file paths for one TLS role.
///
/// The type accepts file paths only: it cannot hold PEM/private-key bytes, has
/// no serializer or path getter, and redacts its `Debug` representation.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedRemoteMtlsIdentityFiles {
    certificate_file: Box<str>,
    private_key_file: Box<str>,
}

impl ResolvedRemoteMtlsIdentityFiles {
    /// Validates absolute, normalized, bounded UTF-8 file paths.
    pub fn try_new(
        certificate_file: PathBuf,
        private_key_file: PathBuf,
    ) -> Result<Self, FabricConfigError> {
        Ok(Self {
            certificate_file: validate_tls_file_path(&certificate_file)?.into(),
            private_key_file: validate_tls_file_path(&private_key_file)?.into(),
        })
    }

    pub(crate) fn certificate_file(&self) -> &str {
        &self.certificate_file
    }

    pub(crate) fn private_key_file(&self) -> &str {
        &self.private_key_file
    }
}

impl fmt::Debug for ResolvedRemoteMtlsIdentityFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedRemoteMtlsIdentityFiles")
            .field("certificate_file", &"<redacted-resolved-path>")
            .field("private_key_file", &"<redacted-resolved-path>")
            .finish()
    }
}

/// Resolved trust and identity files for one listener-only remote Agent role.
///
/// This nominal type cannot carry a connector private key. It is process-local,
/// has no serializer or path getter, and redacts every path from `Debug`.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedRemoteMtlsListenerCredentialFilesV1 {
    root_ca_certificate_file: Box<str>,
    listen_identity: ResolvedRemoteMtlsIdentityFiles,
}

impl ResolvedRemoteMtlsListenerCredentialFilesV1 {
    /// Creates the exact root/listener file set used by an Ubuntu data plane.
    pub fn try_new(
        root_ca_certificate_file: PathBuf,
        listen_identity: ResolvedRemoteMtlsIdentityFiles,
    ) -> Result<Self, FabricConfigError> {
        Ok(Self {
            root_ca_certificate_file: validate_tls_file_path(&root_ca_certificate_file)?.into(),
            listen_identity,
        })
    }
}

impl fmt::Debug for ResolvedRemoteMtlsListenerCredentialFilesV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedRemoteMtlsListenerCredentialFilesV1")
            .field("root_ca_certificate_file", &"<redacted-resolved-path>")
            .field("listen_identity", &"<redacted>")
            .finish()
    }
}

/// Resolved trust and identity files for one connector-only remote Agent role.
///
/// This nominal type cannot carry a listener private key. It is process-local,
/// has no serializer or path getter, and redacts every path from `Debug`.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedRemoteMtlsConnectorCredentialFilesV1 {
    root_ca_certificate_file: Box<str>,
    connect_identity: ResolvedRemoteMtlsIdentityFiles,
}

impl ResolvedRemoteMtlsConnectorCredentialFilesV1 {
    /// Creates the exact root/connector file set used by a Mac data plane.
    pub fn try_new(
        root_ca_certificate_file: PathBuf,
        connect_identity: ResolvedRemoteMtlsIdentityFiles,
    ) -> Result<Self, FabricConfigError> {
        Ok(Self {
            root_ca_certificate_file: validate_tls_file_path(&root_ca_certificate_file)?.into(),
            connect_identity,
        })
    }
}

impl fmt::Debug for ResolvedRemoteMtlsConnectorCredentialFilesV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedRemoteMtlsConnectorCredentialFilesV1")
            .field("root_ca_certificate_file", &"<redacted-resolved-path>")
            .field("connect_identity", &"<redacted>")
            .finish()
    }
}

/// Owner-resolved trust and role-specific identity file paths for remote mTLS.
///
/// Listening and connecting identities remain distinct because production
/// certificates may carry different extended-key usages. This process-local
/// value is neither desired state nor a wire contract.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedRemoteMtlsCredentialFiles {
    root_ca_certificate_file: Box<str>,
    listen_identity: ResolvedRemoteMtlsIdentityFiles,
    connect_identity: ResolvedRemoteMtlsIdentityFiles,
}

impl ResolvedRemoteMtlsCredentialFiles {
    /// Creates a resolved file set for both mandatory TLS roles.
    pub fn try_new(
        root_ca_certificate_file: PathBuf,
        listen_identity: ResolvedRemoteMtlsIdentityFiles,
        connect_identity: ResolvedRemoteMtlsIdentityFiles,
    ) -> Result<Self, FabricConfigError> {
        Ok(Self {
            root_ca_certificate_file: validate_tls_file_path(&root_ca_certificate_file)?.into(),
            listen_identity,
            connect_identity,
        })
    }
}

impl fmt::Debug for ResolvedRemoteMtlsCredentialFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedRemoteMtlsCredentialFiles")
            .field("root_ca_certificate_file", &"<redacted-resolved-path>")
            .field("listen_identity", &"<redacted>")
            .field("connect_identity", &"<redacted>")
            .finish()
    }
}

/// Explicit inputs for exactly one service-owned Zenoh peer session.
#[derive(Clone, Eq, PartialEq)]
pub struct FabricServiceConfig {
    transport: FabricTransportConfig,
}

#[derive(Clone, Eq, PartialEq)]
enum FabricTransportConfig {
    LoopbackTcp {
        listen_endpoints: Vec<SessionEndpoint>,
        connect_endpoints: Vec<SessionEndpoint>,
    },
    SecuredHybridMtls {
        loopback_listen_endpoint: SessionEndpoint,
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        remote_tls_connect_endpoints: Vec<RemoteTlsEndpoint>,
        credentials: ResolvedRemoteMtlsCredentialFiles,
        experimental_peer_bindings: Option<Box<[ExperimentalRemoteMtlsPeerBindingV1]>>,
    },
    RemoteAgentListenerMtlsV1 {
        loopback_listen_endpoint: SessionEndpoint,
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        credentials: ResolvedRemoteMtlsListenerCredentialFilesV1,
        expected_mac_agent_client_principal: PrincipalRef,
        routes: RemoteAgentDataPlaneRoutesV1,
    },
    RemoteAgentProxyListenerMtlsV2 {
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        credentials: ResolvedRemoteMtlsListenerCredentialFilesV1,
        expected_mac_agent_client_principal: PrincipalRef,
        routes: RemoteAgentDataPlaneRoutesV1,
    },
    RemoteAgentConnectorMtlsV1 {
        remote_tls_connect_endpoint: RemoteTlsEndpoint,
        credentials: ResolvedRemoteMtlsConnectorCredentialFilesV1,
        expected_ubuntu_agent_listener_principal: PrincipalRef,
        routes: RemoteAgentDataPlaneRoutesV1,
    },
}

#[derive(Clone, Eq, PartialEq)]
struct RemoteAgentDataPlaneRoutesV1 {
    submit: String,
    control: String,
}

impl RemoteAgentDataPlaneRoutesV1 {
    fn try_new(
        submit: impl Into<String>,
        control: impl Into<String>,
    ) -> Result<Self, FabricConfigError> {
        let submit = validate_concrete_key_expression(submit.into())?;
        let control = validate_concrete_key_expression(control.into())?;
        if submit == control {
            return Err(FabricConfigError::DuplicateRemoteAgentRoute);
        }
        Ok(Self { submit, control })
    }

    fn as_array(&self) -> [&str; 2] {
        [&self.submit, &self.control]
    }
}

impl FabricServiceConfig {
    /// Creates a loopback-only plaintext peer configuration with scouting disabled.
    pub fn try_peer(
        listen_endpoints: Vec<SessionEndpoint>,
        connect_endpoints: Vec<SessionEndpoint>,
    ) -> Result<Self, FabricConfigError> {
        if listen_endpoints.is_empty() && connect_endpoints.is_empty() {
            return Err(FabricConfigError::NoEndpoint);
        }
        if listen_endpoints.len() > MAX_ENDPOINTS || connect_endpoints.len() > MAX_ENDPOINTS {
            return Err(FabricConfigError::TooManyEndpoints);
        }
        Ok(Self {
            transport: FabricTransportConfig::LoopbackTcp {
                listen_endpoints,
                connect_endpoints,
            },
        })
    }

    /// Creates one Ubuntu remote-Agent listener on the sole Fabric session.
    ///
    /// The session retains the exact loopback listener, adds one TLS listener,
    /// has no connector, and exposes only the two concrete Agent routes to the
    /// expected Mac certificate principal. This is a data-plane profile and
    /// carries no Runtime-control channel or apply protocol semantics.
    pub fn try_remote_agent_listener_v1(
        loopback_listen_endpoint: SessionEndpoint,
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        credentials: ResolvedRemoteMtlsListenerCredentialFilesV1,
        expected_mac_agent_client_principal: PrincipalRef,
        submit_key_expression: impl Into<String>,
        control_key_expression: impl Into<String>,
    ) -> Result<Self, FabricConfigError> {
        if principal_is_zero(expected_mac_agent_client_principal) {
            return Err(FabricConfigError::ZeroExpectedRemoteAgentPrincipal);
        }
        Ok(Self {
            transport: FabricTransportConfig::RemoteAgentListenerMtlsV1 {
                loopback_listen_endpoint,
                remote_tls_listen_endpoint,
                credentials,
                expected_mac_agent_client_principal,
                routes: RemoteAgentDataPlaneRoutesV1::try_new(
                    submit_key_expression,
                    control_key_expression,
                )?,
            },
        })
    }

    /// Creates one independent Ubuntu remote-Agent proxy listener.
    ///
    /// Unlike [`Self::try_remote_agent_listener_v1`], this successor neither
    /// retains nor creates a plaintext loopback listener. It admits exactly
    /// one TLS listener, no connector, one expected Mac certificate principal,
    /// and the two concrete Agent routes. Preparing and opening this config
    /// requires the role-specific [`PreparedRemoteAgentProxyListenerV2`]
    /// lifecycle; [`FabricService::start`] rejects this profile.
    #[cfg_attr(not(test), expect(dead_code, reason = "T2-B1 internal candidate"))] // GOV-WAIVER-0014
    pub(crate) fn try_remote_agent_proxy_listener_v2(
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        credentials: ResolvedRemoteMtlsListenerCredentialFilesV1,
        expected_mac_agent_client_principal: PrincipalRef,
        submit_key_expression: impl Into<String>,
        control_key_expression: impl Into<String>,
    ) -> Result<Self, FabricConfigError> {
        if principal_is_zero(expected_mac_agent_client_principal) {
            return Err(FabricConfigError::ZeroExpectedRemoteAgentPrincipal);
        }
        Ok(Self {
            transport: FabricTransportConfig::RemoteAgentProxyListenerMtlsV2 {
                remote_tls_listen_endpoint,
                credentials,
                expected_mac_agent_client_principal,
                routes: RemoteAgentDataPlaneRoutesV1::try_new(
                    submit_key_expression,
                    control_key_expression,
                )?,
            },
        })
    }

    /// Creates one Mac remote-Agent connector on its TLS-only Fabric session.
    ///
    /// The session has no listener and exactly one TLS connector. Its ACL is
    /// the role-inverse of [`Self::try_remote_agent_listener_v1`] and is pinned
    /// to the expected Ubuntu listener certificate principal and same routes.
    pub fn try_remote_agent_connector_v1(
        remote_tls_connect_endpoint: RemoteTlsEndpoint,
        credentials: ResolvedRemoteMtlsConnectorCredentialFilesV1,
        expected_ubuntu_agent_listener_principal: PrincipalRef,
        submit_key_expression: impl Into<String>,
        control_key_expression: impl Into<String>,
    ) -> Result<Self, FabricConfigError> {
        if principal_is_zero(expected_ubuntu_agent_listener_principal) {
            return Err(FabricConfigError::ZeroExpectedRemoteAgentPrincipal);
        }
        Ok(Self {
            transport: FabricTransportConfig::RemoteAgentConnectorMtlsV1 {
                remote_tls_connect_endpoint,
                credentials,
                expected_ubuntu_agent_listener_principal,
                routes: RemoteAgentDataPlaneRoutesV1::try_new(
                    submit_key_expression,
                    control_key_expression,
                )?,
            },
        })
    }

    /// Creates the sole admitted secured hybrid topology for one Zenoh session.
    ///
    /// The exact P5 profile retains one predecessor loopback listener, adds one
    /// non-loopback TLS listener plus one to eight explicit TLS connectors,
    /// and requires separate resolved listener/connector identities. No other
    /// TCP endpoint, discovery, or protocol fallback is admitted.
    pub fn try_secured_hybrid_peer(
        loopback_listen_endpoint: SessionEndpoint,
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        remote_tls_connect_endpoints: Vec<RemoteTlsEndpoint>,
        credentials: ResolvedRemoteMtlsCredentialFiles,
    ) -> Result<Self, FabricConfigError> {
        if remote_tls_connect_endpoints.is_empty() {
            return Err(FabricConfigError::NoTlsConnectEndpoint);
        }
        if remote_tls_connect_endpoints.len() > MAX_ENDPOINTS {
            return Err(FabricConfigError::TooManyEndpoints);
        }
        for (index, endpoint) in remote_tls_connect_endpoints.iter().enumerate() {
            if endpoint == &remote_tls_listen_endpoint {
                return Err(FabricConfigError::RemoteListenConnectEndpointConflict);
            }
            if remote_tls_connect_endpoints[..index].contains(endpoint) {
                return Err(FabricConfigError::DuplicateEndpoint);
            }
        }
        Ok(Self {
            transport: FabricTransportConfig::SecuredHybridMtls {
                loopback_listen_endpoint,
                remote_tls_listen_endpoint,
                remote_tls_connect_endpoints,
                credentials,
                experimental_peer_bindings: None,
            },
        })
    }

    /// Creates the experimental secured topology with exact expected-CN rows.
    ///
    /// This is an additive phase-one path over exact Zenoh 1.9.0 unstable link
    /// introspection. It rejects duplicate endpoints, CNs, and binding digests
    /// before a session opens. It does not fall back to the non-observing
    /// secured constructor and does not claim production certificate identity.
    pub fn try_experimental_secured_hybrid_peer_with_cn_bindings(
        loopback_listen_endpoint: SessionEndpoint,
        remote_tls_listen_endpoint: RemoteTlsEndpoint,
        remote_peer_bindings: Vec<ExperimentalRemoteMtlsPeerBindingV1>,
        credentials: ResolvedRemoteMtlsCredentialFiles,
    ) -> Result<Self, ExperimentalRemoteMtlsConfigErrorV1> {
        let remote_tls_connect_endpoints = remote_peer_bindings
            .iter()
            .map(|binding| binding.connect_endpoint.clone())
            .collect();
        let mut config = Self::try_secured_hybrid_peer(
            loopback_listen_endpoint,
            remote_tls_listen_endpoint,
            remote_tls_connect_endpoints,
            credentials,
        )?;
        for (index, binding) in remote_peer_bindings.iter().enumerate() {
            if remote_peer_bindings[..index]
                .iter()
                .any(|earlier| earlier.expected_common_name == binding.expected_common_name)
            {
                return Err(ExperimentalRemoteMtlsConfigErrorV1::DuplicatePeerCommonName);
            }
            if remote_peer_bindings[..index]
                .iter()
                .any(|earlier| earlier.identity_binding_digest == binding.identity_binding_digest)
            {
                return Err(ExperimentalRemoteMtlsConfigErrorV1::DuplicateIdentityBinding);
            }
        }
        let FabricTransportConfig::SecuredHybridMtls {
            experimental_peer_bindings,
            ..
        } = &mut config.transport
        else {
            unreachable!("secured constructor must return the secured transport variant")
        };
        *experimental_peer_bindings = Some(remote_peer_bindings.into_boxed_slice());
        Ok(config)
    }

    fn experimental_peer_bindings(&self) -> Option<Box<[ExperimentalRemoteMtlsPeerBindingV1]>> {
        match &self.transport {
            FabricTransportConfig::SecuredHybridMtls {
                experimental_peer_bindings,
                ..
            } => experimental_peer_bindings.clone(),
            FabricTransportConfig::LoopbackTcp { .. }
            | FabricTransportConfig::RemoteAgentListenerMtlsV1 { .. }
            | FabricTransportConfig::RemoteAgentProxyListenerMtlsV2 { .. }
            | FabricTransportConfig::RemoteAgentConnectorMtlsV1 { .. } => None,
        }
    }

    fn build_zenoh_config(&self) -> Result<zenoh::Config, FabricError> {
        let mut config = zenoh::Config::default();
        let mode = match &self.transport {
            FabricTransportConfig::RemoteAgentConnectorMtlsV1 { .. } => r#""client""#,
            _ => r#""peer""#,
        };
        config
            .insert_json5("mode", mode)
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
        config
            .insert_json5("scouting/multicast/enabled", "false")
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
        config
            .insert_json5("scouting/gossip/enabled", "false")
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
        config
            .insert_json5("connect/timeout_ms", "3000")
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
        match &self.transport {
            FabricTransportConfig::LoopbackTcp {
                listen_endpoints,
                connect_endpoints,
            } => {
                set_protocols(&mut config, r#"["tcp"]"#)?;
                set_endpoints(
                    &mut config,
                    endpoint_array_json(listen_endpoints.iter().map(SessionEndpoint::as_str)),
                    endpoint_array_json(connect_endpoints.iter().map(SessionEndpoint::as_str)),
                )?;
            }
            FabricTransportConfig::SecuredHybridMtls {
                loopback_listen_endpoint,
                remote_tls_listen_endpoint,
                remote_tls_connect_endpoints,
                credentials,
                ..
            } => {
                set_protocols(&mut config, r#"["tcp","tls"]"#)?;
                set_endpoints(
                    &mut config,
                    endpoint_array_json(
                        core::iter::once(loopback_listen_endpoint.as_str())
                            .chain(core::iter::once(remote_tls_listen_endpoint.as_str())),
                    ),
                    endpoint_array_json(
                        remote_tls_connect_endpoints
                            .iter()
                            .map(RemoteTlsEndpoint::as_str),
                    ),
                )?;
                configure_remote_mtls(&mut config, credentials)?;
            }
            FabricTransportConfig::RemoteAgentListenerMtlsV1 {
                loopback_listen_endpoint,
                remote_tls_listen_endpoint,
                credentials,
                expected_mac_agent_client_principal,
                routes,
            } => {
                configure_remote_agent_session(&mut config)?;
                set_protocols(&mut config, r#"["tcp","tls"]"#)?;
                set_endpoints(
                    &mut config,
                    endpoint_array_json(
                        core::iter::once(loopback_listen_endpoint.as_str())
                            .chain(core::iter::once(remote_tls_listen_endpoint.as_str())),
                    ),
                    endpoint_array_json(core::iter::empty()),
                )?;
                configure_remote_agent_mtls_role(
                    &mut config,
                    credentials.root_ca_certificate_file.as_ref(),
                    &credentials.listen_identity,
                    RemoteMtlsRole::Listener,
                )?;
                configure_remote_agent_acl(
                    &mut config,
                    routes,
                    RemoteAgentSessionRoleV1::Listener,
                    *expected_mac_agent_client_principal,
                )?;
            }
            FabricTransportConfig::RemoteAgentProxyListenerMtlsV2 {
                remote_tls_listen_endpoint,
                credentials,
                expected_mac_agent_client_principal,
                routes,
            } => {
                configure_remote_agent_session(&mut config)?;
                set_protocols(&mut config, r#"["tls"]"#)?;
                set_endpoints(
                    &mut config,
                    endpoint_array_json(core::iter::once(remote_tls_listen_endpoint.as_str())),
                    endpoint_array_json(core::iter::empty()),
                )?;
                configure_remote_agent_mtls_role(
                    &mut config,
                    credentials.root_ca_certificate_file.as_ref(),
                    &credentials.listen_identity,
                    RemoteMtlsRole::Listener,
                )?;
                configure_remote_agent_acl(
                    &mut config,
                    routes,
                    RemoteAgentSessionRoleV1::Listener,
                    *expected_mac_agent_client_principal,
                )?;
            }
            FabricTransportConfig::RemoteAgentConnectorMtlsV1 {
                remote_tls_connect_endpoint,
                credentials,
                expected_ubuntu_agent_listener_principal,
                routes,
            } => {
                configure_remote_agent_session(&mut config)?;
                set_protocols(&mut config, r#"["tls"]"#)?;
                set_endpoints(
                    &mut config,
                    endpoint_array_json(core::iter::empty()),
                    endpoint_array_json(core::iter::once(remote_tls_connect_endpoint.as_str())),
                )?;
                configure_remote_agent_mtls_role(
                    &mut config,
                    credentials.root_ca_certificate_file.as_ref(),
                    &credentials.connect_identity,
                    RemoteMtlsRole::Connector,
                )?;
                configure_remote_agent_acl(
                    &mut config,
                    routes,
                    RemoteAgentSessionRoleV1::Connector,
                    *expected_ubuntu_agent_listener_principal,
                )?;
            }
        }
        Ok(config)
    }
}

impl fmt::Debug for FabricServiceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let profile = match &self.transport {
            FabricTransportConfig::LoopbackTcp { .. } => "loopback-tcp",
            FabricTransportConfig::SecuredHybridMtls {
                experimental_peer_bindings: Some(_),
                ..
            } => "experimental-secured-hybrid-mtls-cn",
            FabricTransportConfig::SecuredHybridMtls {
                experimental_peer_bindings: None,
                ..
            } => "secured-hybrid-mtls",
            FabricTransportConfig::RemoteAgentListenerMtlsV1 { .. } => {
                "remote-agent-listener-mtls-v1"
            }
            FabricTransportConfig::RemoteAgentProxyListenerMtlsV2 { .. } => {
                "remote-agent-proxy-listener-mtls-v2"
            }
            FabricTransportConfig::RemoteAgentConnectorMtlsV1 { .. } => {
                "remote-agent-connector-mtls-v1"
            }
        };
        formatter
            .debug_struct("FabricServiceConfig")
            .field("transport_profile", &profile)
            .finish_non_exhaustive()
    }
}

fn validate_concrete_key_expression(key_expression: String) -> Result<String, FabricConfigError> {
    if key_expression.is_empty() {
        return Err(FabricConfigError::EmptyKeyExpression);
    }
    if key_expression.len() > MAX_KEY_EXPRESSION_BYTES {
        return Err(FabricConfigError::KeyExpressionTooLong);
    }
    if !key_expression.is_ascii()
        || key_expression.bytes().any(|byte| byte.is_ascii_control())
        || key_expression.contains('*')
        || key_expression.contains('$')
    {
        return Err(FabricConfigError::NonConcreteKeyExpression);
    }
    let parsed = OwnedNonWildKeyExpr::try_from(key_expression.clone())
        .map_err(|_| FabricConfigError::NonConcreteKeyExpression)?;
    if parsed.as_str() != key_expression.as_str() {
        return Err(FabricConfigError::NonConcreteKeyExpression);
    }
    Ok(key_expression)
}

fn principal_is_zero(principal: PrincipalRef) -> bool {
    principal.as_bytes().iter().all(|byte| *byte == 0)
}

/// One exact request/response route requested from the Fabric owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestResponseBindingSpec {
    binding_id: BindingId,
    expected_active_epoch: Option<BindingEpoch>,
    key_expression: String,
    request_schema: SchemaRef,
    response_schema: SchemaRef,
    ingress_limits: IngressLimits,
}

impl RequestResponseBindingSpec {
    /// Creates an install or exact-CAS replacement request.
    pub fn try_new(
        binding_id: BindingId,
        expected_active_epoch: Option<BindingEpoch>,
        key_expression: impl Into<String>,
        request_schema: SchemaRef,
        response_schema: SchemaRef,
        ingress_limits: IngressLimits,
    ) -> Result<Self, FabricConfigError> {
        validate_binding_id(binding_id).map_err(FabricConfigError::Contract)?;
        let key_expression = validate_concrete_key_expression(key_expression.into())?;
        if ingress_limits.max_frame_bytes() < REQUEST_HEADER_BYTES {
            return Err(FabricConfigError::FrameCannotHoldEnvelopeHeader);
        }
        Ok(Self {
            binding_id,
            expected_active_epoch,
            key_expression,
            request_schema,
            response_schema,
            ingress_limits,
        })
    }
}

/// Fabric-owner-issued lifecycle and exact-CAS token for one live binding.
///
/// Only a successful install can create this type. Request-side bootstrap
/// descriptors reconstruct [`ClientPortBindingV1`] instead, so decoded route
/// facts can never be passed to replacement, ingress-snapshot, or retirement
/// APIs that require this owner token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortBinding {
    binding_id: BindingId,
    binding_epoch: BindingEpoch,
    key_expression: String,
    request_schema: SchemaRef,
    response_schema: SchemaRef,
    ingress_limits: IngressLimits,
}

impl PortBinding {
    #[must_use]
    pub const fn binding_id(&self) -> BindingId {
        self.binding_id
    }

    #[must_use]
    pub const fn binding_epoch(&self) -> BindingEpoch {
        self.binding_epoch
    }

    #[must_use]
    pub fn key_expression(&self) -> &str {
        &self.key_expression
    }

    #[must_use]
    pub const fn request_schema(&self) -> SchemaRef {
        self.request_schema
    }

    #[must_use]
    pub const fn response_schema(&self) -> SchemaRef {
        self.response_schema
    }

    #[must_use]
    pub const fn ingress_limits(&self) -> IngressLimits {
        self.ingress_limits
    }
}

/// Descriptor-recoverable, request-only route for one binding generation.
///
/// This type is deliberately not accepted by any Fabric lifecycle or CAS API.
/// It owns no queryable, worker, session, discovery state, or authority.
///
/// ```compile_fail
/// use paraegox_fabric::{ClientPortBindingV1, FabricService};
///
/// async fn cannot_retire(
///     fabric: &mut FabricService,
///     client: &ClientPortBindingV1,
/// ) {
///     fabric.retire_port_binding(client).await.unwrap();
/// }
/// ```
#[derive(Clone, Eq, PartialEq)]
pub struct ClientPortBindingV1 {
    binding_id: BindingId,
    binding_epoch: BindingEpoch,
    key_expression: String,
    request_schema: SchemaRef,
    response_schema: SchemaRef,
    ingress_limits: IngressLimits,
}

impl ClientPortBindingV1 {
    pub(crate) fn from_descriptor_parts(
        binding_id: BindingId,
        binding_epoch: BindingEpoch,
        key_expression: String,
        request_schema: SchemaRef,
        response_schema: SchemaRef,
        ingress_limits: IngressLimits,
    ) -> Self {
        Self {
            binding_id,
            binding_epoch,
            key_expression,
            request_schema,
            response_schema,
            ingress_limits,
        }
    }

    #[must_use]
    pub const fn binding_id(&self) -> BindingId {
        self.binding_id
    }

    #[must_use]
    pub const fn binding_epoch(&self) -> BindingEpoch {
        self.binding_epoch
    }

    #[must_use]
    pub fn key_expression(&self) -> &str {
        &self.key_expression
    }

    #[must_use]
    pub const fn request_schema(&self) -> SchemaRef {
        self.request_schema
    }

    #[must_use]
    pub const fn response_schema(&self) -> SchemaRef {
        self.response_schema
    }

    #[must_use]
    pub const fn ingress_limits(&self) -> IngressLimits {
        self.ingress_limits
    }
}

impl fmt::Debug for ClientPortBindingV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientPortBindingV1")
            .field("binding_id", &self.binding_id)
            .field("binding_epoch", &self.binding_epoch)
            .field("key_expression", &"<owner-private-route>")
            .field("request_schema", &self.request_schema)
            .field("response_schema", &self.response_schema)
            .field("ingress_limits", &self.ingress_limits)
            .finish()
    }
}

#[derive(Clone, Copy)]
struct RequestRoute<'route> {
    binding_id: BindingId,
    binding_epoch: BindingEpoch,
    key_expression: &'route str,
    request_schema: SchemaRef,
    response_schema: SchemaRef,
    ingress_limits: IngressLimits,
}

impl<'route> From<&'route PortBinding> for RequestRoute<'route> {
    fn from(binding: &'route PortBinding) -> Self {
        Self {
            binding_id: binding.binding_id,
            binding_epoch: binding.binding_epoch,
            key_expression: &binding.key_expression,
            request_schema: binding.request_schema,
            response_schema: binding.response_schema,
            ingress_limits: binding.ingress_limits,
        }
    }
}

impl<'route> From<&'route ClientPortBindingV1> for RequestRoute<'route> {
    fn from(binding: &'route ClientPortBindingV1) -> Self {
        Self {
            binding_id: binding.binding_id,
            binding_epoch: binding.binding_epoch,
            key_expression: &binding.key_expression,
            request_schema: binding.request_schema,
            response_schema: binding.response_schema,
            ingress_limits: binding.ingress_limits,
        }
    }
}

/// A newly installed live binding plus its only typed request-consumer handle.
pub struct InstalledBinding {
    port_binding: PortBinding,
    requests: RequestReceiver,
}

impl InstalledBinding {
    /// Returns the immutable route identity used by clients.
    #[must_use]
    pub fn port_binding(&self) -> &PortBinding {
        &self.port_binding
    }

    /// Splits the immutable route from its single-consumer request receiver.
    #[must_use]
    pub fn into_parts(self) -> (PortBinding, RequestReceiver) {
        (self.port_binding, self.requests)
    }
}

/// Typed response supplied by the service implementation, not by a Zenoh callback.
#[derive(Debug, Eq, PartialEq)]
pub enum HandlerResponse {
    Ok(Vec<u8>),
    Rejected(Vec<u8>),
}

/// A decoded request delivered after transport prevalidation and epoch fencing.
pub struct InboundRequest {
    envelope: BindingRequestEnvelopeV1,
    responder: Option<oneshot::Sender<HandlerResponse>>,
}

impl InboundRequest {
    #[must_use]
    pub const fn binding_id(&self) -> BindingId {
        self.envelope.binding_id()
    }

    #[must_use]
    pub const fn binding_epoch(&self) -> BindingEpoch {
        self.envelope.binding_epoch()
    }

    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        self.envelope.request_id()
    }

    #[must_use]
    pub const fn schema(&self) -> SchemaRef {
        self.envelope.schema()
    }

    #[must_use]
    pub fn body(&self) -> &[u8] {
        self.envelope.body()
    }

    /// Completes this request exactly once.
    pub fn respond(mut self, response: HandlerResponse) -> Result<(), HandlerResponse> {
        let Some(responder) = self.responder.take() else {
            return Err(response);
        };
        responder.send(response)
    }
}

/// Single-consumer typed request boundary for one live binding generation.
pub struct RequestReceiver {
    receiver: mpsc::Receiver<InboundRequest>,
}

impl RequestReceiver {
    /// Waits for the next decoded request or returns `None` after retirement.
    pub async fn recv(&mut self) -> Option<InboundRequest> {
        self.receiver.recv().await
    }
}

/// One sanitized peer row selected from a real TLS link in one live Session.
///
/// The raw certificate CN is deliberately absent. The binding digest lets the
/// Runtime correlate this row with its resolver-owned mapping, while the ZID
/// and locators remain point-in-time transport facts rather than identity or
/// continuous-health claims. Its nonzero sequence is allocated only by the
/// owning `FabricService` and is unique within that service's session epoch.
#[derive(Clone, Eq, PartialEq)]
pub struct ExperimentalRemoteMtlsPeerLinkObservationV1 {
    identity_binding_digest: Digest32,
    observation_sequence: NonZeroU64,
    peer_zenoh_id: Box<str>,
    source_locator: Box<str>,
    destination_locator: Box<str>,
}

impl ExperimentalRemoteMtlsPeerLinkObservationV1 {
    /// Returns the Runtime-computed CN-binding digest selected by Fabric.
    #[must_use]
    pub const fn identity_binding_digest(&self) -> Digest32 {
        self.identity_binding_digest
    }

    /// Returns this peer row's Fabric-owned service-instance-local sequence.
    #[must_use]
    pub const fn observation_sequence(&self) -> u64 {
        self.observation_sequence.get()
    }

    /// Returns the exact remote Zenoh ID reported by this live link snapshot.
    #[must_use]
    pub fn peer_zenoh_id(&self) -> &str {
        &self.peer_zenoh_id
    }

    /// Returns the source locator reported by Zenoh for the selected link.
    #[must_use]
    pub fn source_locator(&self) -> &str {
        &self.source_locator
    }

    /// Returns the destination locator reported by Zenoh for the selected link.
    #[must_use]
    pub fn destination_locator(&self) -> &str {
        &self.destination_locator
    }
}

impl fmt::Debug for ExperimentalRemoteMtlsPeerLinkObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExperimentalRemoteMtlsPeerLinkObservationV1")
            .field("identity_binding_digest", &self.identity_binding_digest)
            .field("observation_sequence", &self.observation_sequence)
            .field("peer_zenoh_id", &self.peer_zenoh_id)
            .field("source_locator", &"<redacted-locator>")
            .field("destination_locator", &"<redacted-locator>")
            .finish()
    }
}

/// A bounded point-in-time snapshot produced from one private live Session.
///
/// Peer-row sequence values are monotonic only within this `FabricService`
/// instance. `observation_sequence()` is the last peer-row sequence reserved
/// by this snapshot, which is also the service-local high-water after success.
/// The session epoch is generated once by the Fabric owner before opening that
/// instance's sole Session and remains unchanged across Session reconnects.
/// This raw snapshot still has no Runtime service generation, evidence-store
/// ownership, authenticated carrier, or PXTP semantics. Those fences must be
/// supplied by the later managed-Runtime integration slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExperimentalRemoteMtlsLinkSnapshotV1 {
    session_epoch: DistributedFabricSessionEpochV1,
    local_zenoh_id: Box<str>,
    observation_sequence_high_water: NonZeroU64,
    remote_peers: Box<[ExperimentalRemoteMtlsPeerLinkObservationV1]>,
}

impl ExperimentalRemoteMtlsLinkSnapshotV1 {
    /// Returns the nonzero epoch owned by this snapshot's live Fabric Session.
    #[must_use]
    pub const fn session_epoch(&self) -> DistributedFabricSessionEpochV1 {
        self.session_epoch
    }

    /// Returns the local ZID read from the same private Session.
    #[must_use]
    pub fn local_zenoh_id(&self) -> &str {
        &self.local_zenoh_id
    }

    /// Returns the last peer-row sequence atomically reserved by this snapshot.
    ///
    /// This compatibility accessor is the service-local high-water after the
    /// snapshot succeeds, not a second sequence assigned to the snapshot.
    #[must_use]
    pub const fn observation_sequence(&self) -> u64 {
        self.observation_sequence_high_water.get()
    }

    /// Returns one row per configured experimental CN binding, in config order.
    #[must_use]
    pub fn remote_peers(&self) -> &[ExperimentalRemoteMtlsPeerLinkObservationV1] {
        &self.remote_peers
    }
}

/// A fully validated, effect-free plan for one independent TLS-only S1.
///
/// Preparation builds the exact private Zenoh configuration and reserves one
/// fresh nonzero session epoch, but performs no file read, socket open, route
/// declaration, or background task spawn. The caller may therefore persist an
/// S1-open intent containing [`Self::session_epoch`] before consuming
/// [`Self::start`]. The exact two routes remain private and move into the live
/// listener for the later role-specific PXAP binding slice.
///
/// This plan is intentionally neither `Clone` nor `Copy`.
#[cfg_attr(not(test), expect(dead_code, reason = "T2-B1 internal candidate"))] // GOV-WAIVER-0014
pub(crate) struct PreparedRemoteAgentProxyListenerV2 {
    zenoh_config: zenoh::Config,
    session_epoch: DistributedFabricSessionEpochV1,
    routes: RemoteAgentDataPlaneRoutesV1,
}

#[cfg_attr(not(test), expect(dead_code, reason = "T2-B1 internal candidate"))] // GOV-WAIVER-0014
impl PreparedRemoteAgentProxyListenerV2 {
    /// Builds an effect-free start plan for exactly the S1 proxy-listener role.
    ///
    /// Any other [`FabricServiceConfig`] profile fails before entropy is read.
    /// Entropy is sampled exactly once and is never caller supplied.
    pub(crate) fn try_prepare(config: FabricServiceConfig) -> Result<Self, FabricError> {
        Self::try_prepare_with(config, |destination| {
            getrandom::fill(destination).map_err(|_| ())
        })
    }

    fn try_prepare_with(
        config: FabricServiceConfig,
        fill: impl FnOnce(&mut [u8; 16]) -> Result<(), ()>,
    ) -> Result<Self, FabricError> {
        let routes = match &config.transport {
            FabricTransportConfig::RemoteAgentProxyListenerMtlsV2 { routes, .. } => routes.clone(),
            _ => return Err(FabricError::RemoteAgentProxyListenerProfileRequired),
        };
        let zenoh_config = config.build_zenoh_config()?;
        let session_epoch = try_fabric_session_epoch_with(fill)?;
        Ok(Self {
            zenoh_config,
            session_epoch,
            routes,
        })
    }

    /// Returns the reserved nonzero epoch without exposing transport authority.
    #[must_use]
    pub(crate) const fn session_epoch(&self) -> DistributedFabricSessionEpochV1 {
        self.session_epoch
    }

    /// Consumes this plan and performs the first transport effect: Session open.
    ///
    /// A failure returns no live listener token and the consumed plan cannot be
    /// reused for an implicit retry. The caller remains responsible for mapping
    /// a failure after its durable open intent to outcome-uncertain state.
    pub(crate) async fn start(self) -> Result<RemoteAgentProxyListenerV2, FabricError> {
        self.start_with(
            |zenoh_config| async move { zenoh::open(zenoh_config).await.map_err(|_| ()) },
        )
        .await
    }

    async fn start_with<Open, OpenFuture>(
        self,
        open: Open,
    ) -> Result<RemoteAgentProxyListenerV2, FabricError>
    where
        Open: FnOnce(zenoh::Config) -> OpenFuture,
        OpenFuture: Future<Output = Result<zenoh::Session, ()>>,
    {
        let Self {
            zenoh_config,
            session_epoch,
            routes,
        } = self;
        let session = open(zenoh_config)
            .await
            .map_err(|()| FabricError::SessionOpenFailed)?;
        Ok(RemoteAgentProxyListenerV2 {
            session,
            session_epoch,
            routes,
        })
    }
}

/// The sole live owner token for one independent TLS-only remote-Agent S1.
///
/// The raw Zenoh Session, exact routes, and general [`FabricService`] mutation
/// surface remain private. This initial slice exposes only epoch correlation
/// and consuming shutdown; exact PXAP lane installation is added separately.
#[cfg_attr(not(test), expect(dead_code, reason = "T2-B1 internal candidate"))] // GOV-WAIVER-0014
pub(crate) struct RemoteAgentProxyListenerV2 {
    session: zenoh::Session,
    session_epoch: DistributedFabricSessionEpochV1,
    routes: RemoteAgentDataPlaneRoutesV1,
}

#[cfg_attr(not(test), expect(dead_code, reason = "T2-B1 internal candidate"))] // GOV-WAIVER-0014
impl RemoteAgentProxyListenerV2 {
    /// Returns the exact epoch reserved by the consumed prepared plan.
    #[must_use]
    pub(crate) const fn session_epoch(&self) -> DistributedFabricSessionEpochV1 {
        self.session_epoch
    }

    /// Consumes the owner token and closes its sole Session.
    ///
    /// This slice owns no queryables or workers yet. Later binding slices must
    /// preserve the ordered fence, drain, join, then close lifecycle here.
    /// [`FabricError::SessionCloseFailed`] is outcome-uncertain: the consuming
    /// call returns no reusable live token and cannot prove that close had no
    /// effect.
    pub(crate) async fn shutdown(self) -> Result<(), FabricError> {
        let Self {
            session,
            session_epoch: _,
            routes: _routes,
        } = self;
        session
            .close()
            .await
            .map_err(|_| FabricError::SessionCloseFailed)
    }
}

/// Owns exactly one private Zenoh session and all entities declared on it.
pub struct FabricService {
    session: zenoh::Session,
    session_epoch: DistributedFabricSessionEpochV1,
    bindings: BTreeMap<BindingId, ActiveBinding>,
    experimental_peer_bindings: Option<Box<[ExperimentalRemoteMtlsPeerBindingV1]>>,
    experimental_observation_sequence_high_water: u64,
}

impl FabricService {
    /// Generates a fresh nonzero epoch, then opens the sole Zenoh Session.
    pub async fn start(config: FabricServiceConfig) -> Result<Self, FabricError> {
        if matches!(
            &config.transport,
            FabricTransportConfig::RemoteAgentProxyListenerMtlsV2 { .. }
        ) {
            return Err(FabricError::RemoteAgentProxyListenerProfileRequired);
        }
        let experimental_peer_bindings = config.experimental_peer_bindings();
        let zenoh_config = config.build_zenoh_config()?;
        let session_epoch = try_fabric_session_epoch_with(|destination| {
            getrandom::fill(destination).map_err(|_| ())
        })?;
        let session = zenoh::open(zenoh_config)
            .await
            .map_err(|_| FabricError::SessionOpenFailed)?;
        Ok(Self {
            session,
            session_epoch,
            bindings: BTreeMap::new(),
            experimental_peer_bindings,
            experimental_observation_sequence_high_water: 0,
        })
    }

    /// Returns the opaque nonzero epoch of this exact Fabric-owned Session.
    ///
    /// The value grants no access to the raw Session or any route and remains
    /// stable only for this `FabricService` instance.
    #[must_use]
    pub fn session_epoch(&self) -> DistributedFabricSessionEpochV1 {
        self.session_epoch
    }

    /// Reads and classifies the exact current link set from this live Session.
    ///
    /// TCP links are never accepted as remote proof. Every TLS link must carry
    /// an enrolled CN, every configured CN must have at least one live TLS
    /// link, and one CN cannot appear under multiple remote ZIDs. Unknown or
    /// additional TLS peers fail the whole snapshot closed. Zenoh 1.9 exposes
    /// only the first certificate CN here, so this method cannot detect a
    /// multi-CN certificate and must remain experimental.
    /// A successful snapshot atomically advances the service-local high-water
    /// by its configured peer count; any classification error leaves it intact.
    pub async fn observe_experimental_remote_mtls_links(
        &mut self,
    ) -> Result<ExperimentalRemoteMtlsLinkSnapshotV1, ExperimentalRemoteMtlsObservationErrorV1>
    {
        let expected = self
            .experimental_peer_bindings
            .as_deref()
            .ok_or(ExperimentalRemoteMtlsObservationErrorV1::NotConfigured)?;
        let local_zenoh_id = self.session.info().zid().await.to_string();
        let links = self
            .session
            .info()
            .links()
            .await
            .take(MAX_EXPERIMENTAL_OBSERVED_LINKS + 1)
            .map(|link| ExperimentalRawZenohLink {
                peer_zenoh_id: link.zid().to_string(),
                source_locator: link.src().as_str().to_owned(),
                destination_locator: link.dst().as_str().to_owned(),
                auth_identifier: link.auth_identifier().map(str::to_owned),
            })
            .collect::<Vec<_>>();
        classify_and_advance_experimental_remote_mtls_links(
            &mut self.experimental_observation_sequence_high_water,
            expected,
            self.session_epoch,
            local_zenoh_id,
            &links,
        )
    }

    /// Installs or exact-CAS replaces one request/response PortBinding.
    ///
    /// Replacement first retires and joins the old queryable, then declares
    /// the new one. This creates a short fail-closed gap, but never exposes two
    /// callbacks for the same key expression at once.
    pub async fn install_request_response_binding(
        &mut self,
        spec: RequestResponseBindingSpec,
    ) -> Result<InstalledBinding, FabricError> {
        let (binding_epoch, active_epoch) = match self.bindings.get(&spec.binding_id) {
            Some(active) => {
                if spec.expected_active_epoch != Some(active.port_binding.binding_epoch) {
                    return Err(FabricError::ExpectedActiveEpochMismatch);
                }
                (
                    active.port_binding.binding_epoch.next()?,
                    Arc::clone(&active.active_epoch),
                )
            }
            None => {
                if spec.expected_active_epoch.is_some() {
                    return Err(FabricError::ExpectedActiveEpochMismatch);
                }
                (BindingEpoch::try_new(1)?, Arc::new(AtomicU64::new(0)))
            }
        };

        if let Some(retired) = self.bindings.remove(&spec.binding_id) {
            retired.active_epoch.store(0, Ordering::Release);
            retired.stop().await?;
        }

        let port_binding = PortBinding {
            binding_id: spec.binding_id,
            binding_epoch,
            key_expression: spec.key_expression,
            request_schema: spec.request_schema,
            response_schema: spec.response_schema,
            ingress_limits: spec.ingress_limits,
        };
        let budget = IngressBudget::new(port_binding.ingress_limits);
        let (ingress_sender, ingress_receiver) =
            mpsc::channel(port_binding.ingress_limits.max_items());
        let (request_sender, request_receiver) = mpsc::channel(1);
        let (cancel_sender, cancel_receiver) = watch::channel(false);
        let callback_ingress = CallbackIngress {
            port_binding: port_binding.clone(),
            active_epoch: Arc::clone(&active_epoch),
            sender: ingress_sender,
            budget: Arc::clone(&budget),
        };
        let queryable = self
            .session
            .declare_queryable(port_binding.key_expression())
            .callback(move |query| callback_ingress.offer(query))
            .await
            .map_err(|_| FabricError::BindingDeclarationFailed)?;

        let worker_port = port_binding.clone();
        let worker_epoch = Arc::clone(&active_epoch);
        let worker = tokio::spawn(async move {
            run_binding_worker(
                worker_port,
                worker_epoch,
                ingress_receiver,
                request_sender,
                cancel_receiver,
            )
            .await;
        });
        let candidate = ActiveBinding {
            port_binding: port_binding.clone(),
            active_epoch: Arc::clone(&active_epoch),
            budget,
            queryable,
            cancel_sender,
            worker,
        };

        active_epoch.store(binding_epoch.value(), Ordering::Release);
        let replaced = self.bindings.insert(spec.binding_id, candidate);
        debug_assert!(
            replaced.is_none(),
            "old binding must be retired before declaration"
        );

        Ok(InstalledBinding {
            port_binding,
            requests: RequestReceiver {
                receiver: request_receiver,
            },
        })
    }

    /// Sends exactly one typed Zenoh query; this method never retries.
    pub async fn request(
        &self,
        binding: &PortBinding,
        request_id: RequestId,
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<BindingResponseEnvelopeV1, FabricError> {
        self.request_route(binding.into(), request_id, body, timeout)
            .await
    }

    /// Sends exactly one typed query through a descriptor-recovered route.
    ///
    /// This is request-only: the client binding cannot be supplied to install,
    /// replacement, ingress-snapshot, or retirement operations.
    pub async fn request_client_v1(
        &self,
        binding: &ClientPortBindingV1,
        request_id: RequestId,
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<BindingResponseEnvelopeV1, FabricError> {
        self.request_route(binding.into(), request_id, body, timeout)
            .await
    }

    async fn request_route(
        &self,
        binding: RequestRoute<'_>,
        request_id: RequestId,
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<BindingResponseEnvelopeV1, FabricError> {
        if timeout.is_zero() {
            return Err(FabricError::ZeroRequestTimeout);
        }
        let max_request_body = binding
            .ingress_limits
            .max_frame_bytes()
            .checked_sub(REQUEST_HEADER_BYTES)
            .ok_or(FabricError::RequestBodyTooLarge)?;
        if body.len() > max_request_body {
            return Err(FabricError::RequestBodyTooLarge);
        }
        let request = BindingRequestEnvelopeV1::try_new(
            binding.binding_id,
            binding.binding_epoch,
            request_id,
            binding.request_schema,
            body,
        )?;
        let deadline = tokio::time::Instant::now()
            .checked_add(timeout)
            .ok_or(FabricError::RequestTimedOut)?;
        let querier = self
            .session
            .declare_querier(binding.key_expression)
            .timeout(timeout)
            .await
            .map_err(|_| FabricError::QuerierDeclarationFailed)?;
        let outcome = async {
            // A declaration is local; the peer's replacement queryable may not
            // yet be present in this session's routing view. Observe matching
            // before the one and only send so a propagation gap cannot turn a
            // valid request into a spurious NoReply or an implicit retry.
            let matching_listener = querier
                .matching_listener()
                .await
                .map_err(|_| FabricError::MatchingObservationFailed)?;
            let mut matching = querier
                .matching_status()
                .await
                .map_err(|_| FabricError::MatchingObservationFailed)?
                .matching();
            while !matching {
                let remaining = remaining_request_budget(deadline)?;
                let status = tokio::time::timeout(remaining, matching_listener.recv_async())
                    .await
                    .map_err(|_| FabricError::RequestTimedOut)?
                    .map_err(|_| FabricError::MatchingObservationFailed)?;
                matching = status.matching();
            }
            drop(matching_listener);

            let replies = tokio::time::timeout(
                remaining_request_budget(deadline)?,
                querier.get().payload(request.encode()),
            )
            .await
            .map_err(|_| FabricError::RequestTimedOut)?
            .map_err(|_| FabricError::QueryStartFailed)?;
            let reply =
                tokio::time::timeout(remaining_request_budget(deadline)?, replies.recv_async())
                    .await
                    .map_err(|_| FabricError::RequestTimedOut)?
                    .map_err(|_| FabricError::NoReply)?;
            let sample = reply
                .into_result()
                .map_err(|_| FabricError::RemoteReplyError)?;
            if sample.key_expr().as_str() != binding.key_expression {
                return Err(FabricError::ResponseCorrelationMismatch);
            }
            let bytes = sample.payload().to_bytes();
            let response = BindingResponseEnvelopeV1::decode(
                bytes.as_ref(),
                binding.ingress_limits.max_response_body_bytes(),
            )?;
            if response.binding_id() != binding.binding_id || response.request_id() != request_id {
                return Err(FabricError::ResponseCorrelationMismatch);
            }
            if response.status() == ResponseStatus::StaleBinding {
                if !response.body().is_empty() {
                    return Err(FabricError::ResponseCorrelationMismatch);
                }
            } else if response.binding_epoch() != binding.binding_epoch
                || response.schema() != binding.response_schema
            {
                return Err(FabricError::ResponseCorrelationMismatch);
            }
            Ok(response)
        }
        .await;
        let undeclared = querier.undeclare().await;
        if undeclared.is_err() {
            return Err(FabricError::QuerierUndeclarationFailed);
        }
        outcome
    }

    /// Returns bounded ingress facts for the exact active generation.
    #[must_use]
    pub fn ingress_snapshot(&self, binding: &PortBinding) -> Option<FabricIngressSnapshot> {
        self.bindings.get(&binding.binding_id).and_then(|active| {
            (active.port_binding.binding_epoch == binding.binding_epoch)
                .then(|| active.budget.snapshot())
        })
    }

    /// Explicitly retires one exact active route.
    pub async fn retire_port_binding(&mut self, binding: &PortBinding) -> Result<(), FabricError> {
        let Some(active) = self.bindings.get(&binding.binding_id) else {
            return Err(FabricError::NoActiveBinding);
        };
        if active.port_binding.binding_epoch != binding.binding_epoch {
            return Err(FabricError::ExpectedActiveEpochMismatch);
        }
        let active = self
            .bindings
            .remove(&binding.binding_id)
            .ok_or(FabricError::NoActiveBinding)?;
        active.active_epoch.store(0, Ordering::Release);
        active.stop().await
    }

    /// Stops admission, joins every owned worker, undeclares entities, and closes the session.
    pub async fn shutdown(mut self) -> Result<(), FabricError> {
        let bindings = core::mem::take(&mut self.bindings);
        for active in bindings.into_values() {
            active.active_epoch.store(0, Ordering::Release);
            active.stop().await?;
        }
        self.session
            .close()
            .await
            .map_err(|_| FabricError::SessionCloseFailed)
    }
}

struct ExperimentalRawZenohLink {
    peer_zenoh_id: String,
    source_locator: String,
    destination_locator: String,
    auth_identifier: Option<String>,
}

fn classify_and_advance_experimental_remote_mtls_links(
    observation_sequence_high_water: &mut u64,
    expected: &[ExperimentalRemoteMtlsPeerBindingV1],
    session_epoch: DistributedFabricSessionEpochV1,
    local_zenoh_id: String,
    links: &[ExperimentalRawZenohLink],
) -> Result<ExperimentalRemoteMtlsLinkSnapshotV1, ExperimentalRemoteMtlsObservationErrorV1> {
    let first_observation_sequence = observation_sequence_high_water
        .checked_add(1)
        .ok_or(ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)?;
    let snapshot = classify_experimental_remote_mtls_links(
        expected,
        session_epoch,
        local_zenoh_id,
        first_observation_sequence,
        links,
    )?;
    *observation_sequence_high_water = snapshot.observation_sequence();
    Ok(snapshot)
}

fn classify_experimental_remote_mtls_links(
    expected: &[ExperimentalRemoteMtlsPeerBindingV1],
    session_epoch: DistributedFabricSessionEpochV1,
    local_zenoh_id: String,
    first_observation_sequence: u64,
    links: &[ExperimentalRawZenohLink],
) -> Result<ExperimentalRemoteMtlsLinkSnapshotV1, ExperimentalRemoteMtlsObservationErrorV1> {
    if expected.is_empty() {
        return Err(ExperimentalRemoteMtlsObservationErrorV1::NotConfigured);
    }
    if links.is_empty() {
        return Err(ExperimentalRemoteMtlsObservationErrorV1::NoLiveLink);
    }
    if links.len() > MAX_EXPERIMENTAL_OBSERVED_LINKS {
        return Err(ExperimentalRemoteMtlsObservationErrorV1::TooManyLiveLinks);
    }
    if first_observation_sequence == 0 || !is_canonical_zenoh_id_text(&local_zenoh_id) {
        return Err(ExperimentalRemoteMtlsObservationErrorV1::InvalidLinkObservation);
    }
    let peer_count = u64::try_from(expected.len())
        .map_err(|_| ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)?;
    let observation_sequence_high_water = first_observation_sequence
        .checked_add(peer_count - 1)
        .and_then(NonZeroU64::new)
        .ok_or(ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)?;

    let mut saw_tls_link = false;
    let mut selected: Vec<Option<ExperimentalRemoteMtlsPeerLinkObservationV1>> =
        vec![None; expected.len()];
    for link in links {
        let source_is_tls = locator_has_protocol(&link.source_locator, "tls");
        let destination_is_tls = locator_has_protocol(&link.destination_locator, "tls");
        if !source_is_tls && !destination_is_tls {
            continue;
        }
        saw_tls_link = true;
        if !source_is_tls
            || !destination_is_tls
            || !is_bounded_observed_locator(&link.source_locator)
            || !is_bounded_observed_locator(&link.destination_locator)
            || !is_canonical_zenoh_id_text(&link.peer_zenoh_id)
            || link.peer_zenoh_id == local_zenoh_id
        {
            return Err(ExperimentalRemoteMtlsObservationErrorV1::InvalidLinkObservation);
        }
        let common_name = link
            .auth_identifier
            .as_deref()
            .ok_or(ExperimentalRemoteMtlsObservationErrorV1::MissingTlsAuthIdentifier)?;
        let expected_index = expected
            .iter()
            .position(|binding| binding.expected_common_name.as_str() == common_name)
            .ok_or(ExperimentalRemoteMtlsObservationErrorV1::UnexpectedTlsPeer)?;
        let candidate = ExperimentalRemoteMtlsPeerLinkObservationV1 {
            identity_binding_digest: expected[expected_index].identity_binding_digest,
            observation_sequence: first_observation_sequence
                .checked_add(u64::try_from(expected_index).map_err(|_| {
                    ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted
                })?)
                .and_then(NonZeroU64::new)
                .ok_or(ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)?,
            peer_zenoh_id: link.peer_zenoh_id.clone().into_boxed_str(),
            source_locator: link.source_locator.clone().into_boxed_str(),
            destination_locator: link.destination_locator.clone().into_boxed_str(),
        };
        match &mut selected[expected_index] {
            Some(current) => {
                if current.peer_zenoh_id != candidate.peer_zenoh_id {
                    return Err(ExperimentalRemoteMtlsObservationErrorV1::PeerZenohIdConflict);
                }
                if (
                    candidate.source_locator.as_ref(),
                    candidate.destination_locator.as_ref(),
                ) < (
                    current.source_locator.as_ref(),
                    current.destination_locator.as_ref(),
                ) {
                    *current = candidate;
                }
            }
            slot @ None => *slot = Some(candidate),
        }
    }
    if !saw_tls_link {
        return Err(ExperimentalRemoteMtlsObservationErrorV1::NoLiveRemoteMtlsLink);
    }
    if selected.iter().any(Option::is_none) {
        return Err(ExperimentalRemoteMtlsObservationErrorV1::MissingExpectedTlsPeer);
    }
    let remote_peers = selected
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or(ExperimentalRemoteMtlsObservationErrorV1::MissingExpectedTlsPeer)?;
    for (index, peer) in remote_peers.iter().enumerate() {
        if remote_peers[..index]
            .iter()
            .any(|earlier| earlier.peer_zenoh_id == peer.peer_zenoh_id)
        {
            return Err(ExperimentalRemoteMtlsObservationErrorV1::PeerZenohIdConflict);
        }
    }
    Ok(ExperimentalRemoteMtlsLinkSnapshotV1 {
        session_epoch,
        local_zenoh_id: local_zenoh_id.into_boxed_str(),
        observation_sequence_high_water,
        remote_peers: remote_peers.into_boxed_slice(),
    })
}

fn locator_has_protocol(locator: &str, protocol: &str) -> bool {
    locator
        .split_once('/')
        .is_some_and(|(actual, _)| actual == protocol)
}

fn is_bounded_observed_locator(locator: &str) -> bool {
    !locator.is_empty()
        && locator.len() <= MAX_OBSERVED_LOCATOR_BYTES
        && !locator.bytes().any(|byte| byte.is_ascii_control())
}

fn is_canonical_zenoh_id_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EXPERIMENTAL_ZENOH_ID_TEXT_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn remaining_request_budget(deadline: tokio::time::Instant) -> Result<Duration, FabricError> {
    deadline
        .checked_duration_since(tokio::time::Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(FabricError::RequestTimedOut)
}

impl Drop for FabricService {
    fn drop(&mut self) {
        for active in self.bindings.values() {
            active.active_epoch.store(0, Ordering::Release);
            let _ = active.cancel_sender.send(true);
            active.worker.abort();
        }
    }
}

struct ActiveBinding {
    port_binding: PortBinding,
    active_epoch: Arc<AtomicU64>,
    budget: Arc<IngressBudget>,
    queryable: OwnedQueryable,
    cancel_sender: watch::Sender<bool>,
    worker: JoinHandle<()>,
}

impl ActiveBinding {
    async fn stop(self) -> Result<(), FabricError> {
        let _ = self.cancel_sender.send(true);
        let undeclaration_failed = self.queryable.undeclare().await.is_err();
        let worker_failed = self.worker.await.is_err();
        if undeclaration_failed {
            return Err(FabricError::BindingUndeclarationFailed);
        }
        if worker_failed {
            return Err(FabricError::BindingWorkerFailed);
        }
        Ok(())
    }
}

struct CallbackIngress {
    port_binding: PortBinding,
    active_epoch: Arc<AtomicU64>,
    sender: mpsc::Sender<IngressFrame>,
    budget: Arc<IngressBudget>,
}

impl CallbackIngress {
    fn offer(&self, query: Query) {
        let Some(payload) = query.payload() else {
            self.budget.rejected_malformed();
            return;
        };
        let total_length = payload.len();
        if total_length > self.port_binding.ingress_limits.max_frame_bytes() {
            let _ = self.budget.try_reserve(total_length);
            return;
        }
        let mut header = [0_u8; REQUEST_HEADER_BYTES];
        if payload.reader().read_exact(&mut header).is_err() {
            self.budget.rejected_malformed();
            return;
        }
        let callback_epoch = self.port_binding.binding_epoch;
        let active_epoch = self.active_epoch.load(Ordering::Acquire);
        let disposition = if active_epoch != callback_epoch.value() {
            request_id_from_header(&header).map_or(RequestHeaderDisposition::Drop, |request_id| {
                RequestHeaderDisposition::Reject {
                    request_id,
                    status: ResponseStatus::StaleBinding,
                }
            })
        } else {
            prevalidate_request_header(
                &header,
                total_length,
                self.port_binding.binding_id,
                callback_epoch,
                self.port_binding.request_schema,
            )
        };
        match disposition {
            RequestHeaderDisposition::Drop => {
                self.budget.rejected_malformed();
                return;
            }
            RequestHeaderDisposition::Reject {
                status: ResponseStatus::StaleBinding,
                ..
            } => self.budget.rejected_stale(),
            RequestHeaderDisposition::Reject { .. } => self.budget.rejected_malformed(),
            RequestHeaderDisposition::Valid { .. } => {}
        }
        let Ok(lease) = self.budget.try_reserve(total_length) else {
            return;
        };
        let frame = IngressFrame {
            query,
            disposition,
            lease,
        };
        match self.sender.try_send(frame) {
            Ok(()) => self.budget.admitted(),
            Err(_) => self.budget.rejected_closed(),
        }
    }
}

struct IngressFrame {
    query: Query,
    disposition: RequestHeaderDisposition,
    lease: IngressLease,
}

async fn run_binding_worker(
    port_binding: PortBinding,
    active_epoch: Arc<AtomicU64>,
    mut ingress: mpsc::Receiver<IngressFrame>,
    request_sender: mpsc::Sender<InboundRequest>,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        let frame = tokio::select! {
            biased;
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    break;
                }
                continue;
            }
            frame = ingress.recv() => match frame {
                Some(frame) => frame,
                None => break,
            }
        };
        handle_frame(
            &port_binding,
            &active_epoch,
            frame,
            &request_sender,
            &mut cancel,
        )
        .await;
    }
}

async fn handle_frame(
    port_binding: &PortBinding,
    active_epoch: &AtomicU64,
    frame: IngressFrame,
    request_sender: &mpsc::Sender<InboundRequest>,
    cancel: &mut watch::Receiver<bool>,
) {
    let IngressFrame {
        query,
        disposition,
        lease,
    } = frame;
    let request_id = match disposition {
        RequestHeaderDisposition::Valid { request_id }
        | RequestHeaderDisposition::Reject { request_id, .. } => request_id,
        RequestHeaderDisposition::Drop => return,
    };

    if active_epoch.load(Ordering::Acquire) != port_binding.binding_epoch.value() {
        drop(lease);
        reply_status(
            &query,
            port_binding,
            request_id,
            ResponseStatus::StaleBinding,
            Vec::new(),
        )
        .await;
        return;
    }
    if let RequestHeaderDisposition::Reject { status, .. } = disposition {
        drop(lease);
        reply_status(&query, port_binding, request_id, status, Vec::new()).await;
        return;
    }

    let Some(payload) = query.payload() else {
        drop(lease);
        reply_status(
            &query,
            port_binding,
            request_id,
            ResponseStatus::MalformedRequest,
            Vec::new(),
        )
        .await;
        return;
    };
    let bytes = payload.to_bytes();
    let max_body = port_binding
        .ingress_limits
        .max_frame_bytes()
        .saturating_sub(REQUEST_HEADER_BYTES);
    let Ok(envelope) = BindingRequestEnvelopeV1::decode(bytes.as_ref(), max_body) else {
        drop(lease);
        reply_status(
            &query,
            port_binding,
            request_id,
            ResponseStatus::MalformedRequest,
            Vec::new(),
        )
        .await;
        return;
    };
    if envelope.binding_id() != port_binding.binding_id
        || envelope.binding_epoch() != port_binding.binding_epoch
        || envelope.schema() != port_binding.request_schema
    {
        drop(lease);
        reply_status(
            &query,
            port_binding,
            request_id,
            ResponseStatus::StaleBinding,
            Vec::new(),
        )
        .await;
        return;
    }
    drop(lease);

    let (response_sender, response_receiver) = oneshot::channel();
    let inbound = InboundRequest {
        envelope,
        responder: Some(response_sender),
    };
    match request_sender.try_send(inbound) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            reply_status(
                &query,
                port_binding,
                request_id,
                ResponseStatus::IngressOverloaded,
                Vec::new(),
            )
            .await;
            return;
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            reply_status(
                &query,
                port_binding,
                request_id,
                ResponseStatus::HandlerUnavailable,
                Vec::new(),
            )
            .await;
            return;
        }
    }

    let handler_result = tokio::select! {
        biased;
        _ = cancel.changed() => None,
        result = tokio::time::timeout(
            port_binding.ingress_limits.handler_timeout(),
            response_receiver,
        ) => Some(result),
    };
    let (status, body) = match handler_result {
        None => (ResponseStatus::HandlerUnavailable, Vec::new()),
        Some(Err(_)) => (ResponseStatus::HandlerTimeout, Vec::new()),
        Some(Ok(Err(_))) => (ResponseStatus::HandlerUnavailable, Vec::new()),
        Some(Ok(Ok(HandlerResponse::Ok(body)))) => (ResponseStatus::Ok, body),
        Some(Ok(Ok(HandlerResponse::Rejected(body)))) => (ResponseStatus::HandlerRejected, body),
    };
    if body.len() > port_binding.ingress_limits.max_response_body_bytes() {
        reply_status(
            &query,
            port_binding,
            request_id,
            ResponseStatus::ResponseTooLarge,
            Vec::new(),
        )
        .await;
    } else {
        reply_status(&query, port_binding, request_id, status, body).await;
    }
}

async fn reply_status(
    query: &Query,
    port_binding: &PortBinding,
    request_id: RequestId,
    status: ResponseStatus,
    body: Vec<u8>,
) {
    let Ok(response) = BindingResponseEnvelopeV1::try_new(
        port_binding.binding_id,
        port_binding.binding_epoch,
        request_id,
        port_binding.response_schema,
        status,
        body,
    ) else {
        return;
    };
    let _ = query
        .reply(port_binding.key_expression(), response.encode())
        .await;
}

fn request_id_from_header(header: &[u8; REQUEST_HEADER_BYTES]) -> Option<RequestId> {
    let bytes: [u8; 16] = header.get(32..48)?.try_into().ok()?;
    RequestId::try_from_bytes(bytes).ok()
}

fn is_canonical_loopback_tcp_endpoint(value: &str) -> bool {
    let Some(port_text) = value.strip_prefix(LOOPBACK_TCP_PREFIX) else {
        return false;
    };
    parse_canonical_nonzero_port(port_text)
        .is_some_and(|port| format!("{LOOPBACK_TCP_PREFIX}{port}") == value)
}

fn is_canonical_remote_tls_endpoint(value: &str) -> bool {
    let Some(authority) = value.strip_prefix(REMOTE_TLS_PREFIX) else {
        return false;
    };
    let Some((address_text, port_text)) = authority.split_once(':') else {
        return false;
    };
    if authority.matches(':').count() != 1 {
        return false;
    }
    let Ok(address) = address_text.parse::<Ipv4Addr>() else {
        return false;
    };
    let Some(port) = parse_canonical_nonzero_port(port_text) else {
        return false;
    };
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && address != Ipv4Addr::BROADCAST
        && format!("{REMOTE_TLS_PREFIX}{address}:{port}") == value
}

fn is_canonical_experimental_peer_common_name(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_EXPERIMENTAL_PEER_COMMON_NAME_BYTES
        || !value.is_ascii()
    {
        return false;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn parse_canonical_nonzero_port(value: &str) -> Option<u16> {
    if value.is_empty()
        || value.len() > 5
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse::<u16>().ok().filter(|port| *port != 0)
}

pub(crate) fn validate_tls_file_path(path: &Path) -> Result<&str, FabricConfigError> {
    let value = path.to_str().ok_or(FabricConfigError::InvalidTlsFilePath)?;
    let mut normalized = PathBuf::new();
    let mut has_normal_component = false;
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(_) => {
                has_normal_component = true;
                normalized.push(component.as_os_str());
            }
            _ => return Err(FabricConfigError::InvalidTlsFilePath),
        }
    }
    if !path.is_absolute()
        || !has_normal_component
        || normalized.as_os_str() != path.as_os_str()
        || value.len() > MAX_TLS_FILE_PATH_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(FabricConfigError::InvalidTlsFilePath);
    }
    Ok(value)
}

pub(crate) fn set_protocols(
    config: &mut zenoh::Config,
    protocols: &str,
) -> Result<(), FabricError> {
    config
        .insert_json5("transport/link/protocols", protocols)
        .map_err(|_| FabricError::SessionConfigurationFailed)
}

pub(crate) fn set_endpoints(
    config: &mut zenoh::Config,
    listen_endpoints: String,
    connect_endpoints: String,
) -> Result<(), FabricError> {
    config
        .insert_json5("listen/endpoints", &listen_endpoints)
        .map_err(|_| FabricError::SessionConfigurationFailed)?;
    config
        .insert_json5("connect/endpoints", &connect_endpoints)
        .map_err(|_| FabricError::SessionConfigurationFailed)
}

#[derive(Clone, Copy)]
enum RemoteAgentSessionRoleV1 {
    Listener,
    Connector,
}

fn configure_remote_agent_session(config: &mut zenoh::Config) -> Result<(), FabricError> {
    for (key, value) in [
        ("scouting/multicast/enabled", "false"),
        ("scouting/gossip/enabled", "false"),
        ("connect/timeout_ms", "0"),
        ("connect/exit_on_failure", "true"),
        ("listen/timeout_ms", "0"),
        ("listen/exit_on_failure", "true"),
        ("open/return_conditions/connect_scouted", "false"),
        ("open/return_conditions/declares", "true"),
        ("adminspace/enabled", "false"),
        ("plugins_loading/enabled", "false"),
        ("transport/unicast/accept_pending", "1"),
        ("transport/unicast/max_sessions", "1"),
        ("transport/unicast/max_links", "1"),
    ] {
        config
            .insert_json5(key, value)
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
    }
    config
        .insert_json5(
            "transport/link/rx/max_message_size",
            &REMOTE_AGENT_TRANSPORT_MAX_MESSAGE_BYTES.to_string(),
        )
        .map_err(|_| FabricError::SessionConfigurationFailed)
}

fn configure_remote_agent_mtls_role(
    config: &mut zenoh::Config,
    root_ca_certificate_file: &str,
    identity: &ResolvedRemoteMtlsIdentityFiles,
    role: RemoteMtlsRole,
) -> Result<(), FabricError> {
    configure_remote_mtls_role(config, root_ca_certificate_file, identity, role)?;
    for (key, value) in [
        ("transport/link/tls/verify_name_on_connect", "true"),
        ("transport/link/tls/close_link_on_expiration", "true"),
    ] {
        config
            .insert_json5(key, value)
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
    }
    Ok(())
}

fn configure_remote_agent_acl(
    config: &mut zenoh::Config,
    routes: &RemoteAgentDataPlaneRoutesV1,
    role: RemoteAgentSessionRoleV1,
    expected_peer_principal: PrincipalRef,
) -> Result<(), FabricError> {
    let [submit, control] = routes.as_array().map(json_string);
    let expected_peer_common_name = json_string(
        &restricted_runtime_apply_peer_certificate_common_name_v1(expected_peer_principal),
    );
    let (egress_rule, egress_messages, ingress_rule, ingress_messages) = match role {
        RemoteAgentSessionRoleV1::Listener => (
            "remote-agent-listener-egress-v1",
            r#"["reply","declare_queryable"]"#,
            "remote-agent-listener-ingress-v1",
            r#"["query"]"#,
        ),
        RemoteAgentSessionRoleV1::Connector => (
            "remote-agent-connector-egress-v1",
            r#"["query"]"#,
            "remote-agent-connector-ingress-v1",
            r#"["reply","declare_queryable"]"#,
        ),
    };
    let acl = format!(
        r#"{{
            "enabled": true,
            "default_permission": "deny",
            "rules": [{{
                "id": "{egress_rule}",
                "permission": "allow",
                "flows": ["egress"],
                "messages": {egress_messages},
                "key_exprs": [{submit},{control}]
            }}, {{
                "id": "{ingress_rule}",
                "permission": "allow",
                "flows": ["ingress"],
                "messages": {ingress_messages},
                "key_exprs": [{submit},{control}]
            }}],
            "subjects": [{{
                "id": "remote-agent-expected-peer-v1",
                "cert_common_names": [{expected_peer_common_name}],
                "link_protocols": ["tls"]
            }}],
            "policies": [{{
                "rules": ["{egress_rule}", "{ingress_rule}"],
                "subjects": ["remote-agent-expected-peer-v1"]
            }}]
        }}"#
    );
    config
        .insert_json5("access_control", &acl)
        .map_err(|_| FabricError::SessionConfigurationFailed)
}

fn configure_remote_mtls(
    config: &mut zenoh::Config,
    credentials: &ResolvedRemoteMtlsCredentialFiles,
) -> Result<(), FabricError> {
    configure_remote_mtls_role(
        config,
        credentials.root_ca_certificate_file.as_ref(),
        &credentials.listen_identity,
        RemoteMtlsRole::Listener,
    )?;
    configure_remote_mtls_role(
        config,
        credentials.root_ca_certificate_file.as_ref(),
        &credentials.connect_identity,
        RemoteMtlsRole::Connector,
    )?;
    for (key, value) in [
        ("transport/link/tls/verify_name_on_connect", "true"),
        ("transport/link/tls/close_link_on_expiration", "true"),
    ] {
        config
            .insert_json5(key, value)
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum RemoteMtlsRole {
    Listener,
    Connector,
}

pub(crate) fn configure_remote_mtls_role(
    config: &mut zenoh::Config,
    root_ca_certificate_file: &str,
    identity: &ResolvedRemoteMtlsIdentityFiles,
    role: RemoteMtlsRole,
) -> Result<(), FabricError> {
    let (certificate_key, private_key) = match role {
        RemoteMtlsRole::Listener => (
            "transport/link/tls/listen_certificate",
            "transport/link/tls/listen_private_key",
        ),
        RemoteMtlsRole::Connector => (
            "transport/link/tls/connect_certificate",
            "transport/link/tls/connect_private_key",
        ),
    };
    for (key, value) in [
        (
            "transport/link/tls/root_ca_certificate",
            root_ca_certificate_file,
        ),
        (certificate_key, identity.certificate_file()),
        (private_key, identity.private_key_file()),
    ] {
        config
            .insert_json5(key, &json_string(value))
            .map_err(|_| FabricError::SessionConfigurationFailed)?;
    }
    config
        .insert_json5("transport/link/tls/enable_mtls", "true")
        .map_err(|_| FabricError::SessionConfigurationFailed)
}

pub(crate) fn json_string(value: &str) -> String {
    let mut json = String::with_capacity(value.len() + 2);
    json.push('"');
    for character in value.chars() {
        match character {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            _ => json.push(character),
        }
    }
    json.push('"');
    json
}

pub(crate) fn endpoint_array_json<'endpoint>(
    endpoints: impl Iterator<Item = &'endpoint str>,
) -> String {
    let mut json = String::from("[");
    for (index, endpoint) in endpoints.enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push('"');
        json.push_str(endpoint);
        json.push('"');
    }
    json.push(']');
    json
}

/// Invalid explicit configuration rejected before Zenoh starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FabricConfigError {
    EmptyEndpoint,
    EndpointTooLong,
    UnsupportedEndpointProtocol,
    NonCanonicalLoopbackEndpoint,
    NonCanonicalRemoteTlsEndpoint,
    InvalidEndpoint,
    NoEndpoint,
    NoTlsConnectEndpoint,
    TooManyEndpoints,
    DuplicateEndpoint,
    RemoteListenConnectEndpointConflict,
    InvalidTlsFilePath,
    EmptyKeyExpression,
    KeyExpressionTooLong,
    NonConcreteKeyExpression,
    DuplicateRemoteAgentRoute,
    ZeroExpectedRemoteAgentPrincipal,
    FrameCannotHoldEnvelopeHeader,
    Contract(FabricContractError),
}

impl fmt::Display for FabricConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => write!(formatter, "binding contract is invalid: {error}"),
            other => formatter.write_str(match other {
                Self::EmptyEndpoint => "session endpoint must not be empty",
                Self::EndpointTooLong => "session endpoint is too long",
                Self::UnsupportedEndpointProtocol => {
                    "endpoint protocol is not admitted by this constructor"
                }
                Self::NonCanonicalLoopbackEndpoint => {
                    "plaintext endpoint must be canonical IPv4 loopback TCP"
                }
                Self::NonCanonicalRemoteTlsEndpoint => {
                    "remote endpoint must be canonical non-loopback unicast IPv4 TLS"
                }
                Self::InvalidEndpoint => "session endpoint is invalid",
                Self::NoEndpoint => "at least one explicit listen or connect endpoint is required",
                Self::NoTlsConnectEndpoint => {
                    "secured hybrid configuration requires at least one remote TLS connector"
                }
                Self::TooManyEndpoints => "session has too many explicit endpoints",
                Self::DuplicateEndpoint => "session contains a duplicate explicit endpoint",
                Self::RemoteListenConnectEndpointConflict => {
                    "remote TLS listener cannot also be a connector"
                }
                Self::InvalidTlsFilePath => {
                    "TLS file path must be absolute, normalized, bounded UTF-8"
                }
                Self::EmptyKeyExpression => "binding key expression must not be empty",
                Self::KeyExpressionTooLong => "binding key expression is too long",
                Self::NonConcreteKeyExpression => "request binding key expression must be concrete",
                Self::DuplicateRemoteAgentRoute => {
                    "remote Agent submit and control routes must be distinct"
                }
                Self::ZeroExpectedRemoteAgentPrincipal => {
                    "remote Agent expected peer principal must be nonzero"
                }
                Self::FrameCannotHoldEnvelopeHeader => {
                    "maximum frame size cannot hold the request envelope header"
                }
                Self::Contract(_) => unreachable!(),
            }),
        }
    }
}

impl std::error::Error for FabricConfigError {}

/// Configuration failures isolated to the experimental expected-CN path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExperimentalRemoteMtlsConfigErrorV1 {
    InvalidPeerCommonName,
    DuplicatePeerCommonName,
    DuplicateIdentityBinding,
    Fabric(FabricConfigError),
}

impl fmt::Display for ExperimentalRemoteMtlsConfigErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPeerCommonName => formatter
                .write_str("experimental peer CN must be canonical bounded lower-case DNS form"),
            Self::DuplicatePeerCommonName => formatter
                .write_str("experimental secured topology contains a duplicate expected peer CN"),
            Self::DuplicateIdentityBinding => formatter.write_str(
                "experimental secured topology contains a duplicate identity-binding digest",
            ),
            Self::Fabric(error) => write!(formatter, "experimental Fabric configuration: {error}"),
        }
    }
}

impl std::error::Error for ExperimentalRemoteMtlsConfigErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Fabric(error) => Some(error),
            _ => None,
        }
    }
}

impl From<FabricConfigError> for ExperimentalRemoteMtlsConfigErrorV1 {
    fn from(value: FabricConfigError) -> Self {
        Self::Fabric(value)
    }
}

/// Stable service-level failure; raw Zenoh errors remain owner-private.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FabricError {
    SessionConfigurationFailed,
    SessionEpochUnavailable,
    RemoteAgentProxyListenerProfileRequired,
    SessionOpenFailed,
    SessionCloseFailed,
    BindingDeclarationFailed,
    BindingUndeclarationFailed,
    BindingWorkerFailed,
    ExpectedActiveEpochMismatch,
    NoActiveBinding,
    ZeroRequestTimeout,
    RequestBodyTooLarge,
    QuerierDeclarationFailed,
    MatchingObservationFailed,
    QuerierUndeclarationFailed,
    QueryStartFailed,
    RequestTimedOut,
    NoReply,
    RemoteReplyError,
    ResponseCorrelationMismatch,
    Contract(FabricContractError),
}

impl fmt::Display for FabricError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contract(error) => write!(formatter, "Fabric contract failed: {error}"),
            other => formatter.write_str(match other {
                Self::SessionConfigurationFailed => "Zenoh session configuration failed",
                Self::SessionEpochUnavailable => {
                    "Fabric session epoch entropy is unavailable or invalid"
                }
                Self::RemoteAgentProxyListenerProfileRequired => {
                    "operation requires the dedicated remote-Agent proxy listener v2 profile"
                }
                Self::SessionOpenFailed => "Zenoh session failed to open",
                Self::SessionCloseFailed => "Zenoh session failed to close",
                Self::BindingDeclarationFailed => "Zenoh queryable declaration failed",
                Self::BindingUndeclarationFailed => "Zenoh queryable undeclaration failed",
                Self::BindingWorkerFailed => "Fabric binding worker failed to join",
                Self::ExpectedActiveEpochMismatch => "expected active BindingEpoch does not match",
                Self::NoActiveBinding => "no active PortBinding exists",
                Self::ZeroRequestTimeout => "request timeout must be nonzero",
                Self::RequestBodyTooLarge => "request body exceeds the binding frame bound",
                Self::QuerierDeclarationFailed => "Fabric querier declaration failed",
                Self::MatchingObservationFailed => "Fabric queryable matching observation failed",
                Self::QuerierUndeclarationFailed => "Fabric querier undeclaration failed",
                Self::QueryStartFailed => "Zenoh query failed to start",
                Self::RequestTimedOut => "Fabric request timed out",
                Self::NoReply => "Fabric request completed without a reply",
                Self::RemoteReplyError => "Zenoh returned a non-envelope error reply",
                Self::ResponseCorrelationMismatch => "response does not match the exact request",
                Self::Contract(_) => unreachable!(),
            }),
        }
    }
}

impl std::error::Error for FabricError {}

/// Fail-closed classification errors for one experimental live-link snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExperimentalRemoteMtlsObservationErrorV1 {
    NotConfigured,
    ObservationSequenceExhausted,
    NoLiveLink,
    TooManyLiveLinks,
    NoLiveRemoteMtlsLink,
    MissingTlsAuthIdentifier,
    UnexpectedTlsPeer,
    MissingExpectedTlsPeer,
    PeerZenohIdConflict,
    InvalidLinkObservation,
}

impl fmt::Display for ExperimentalRemoteMtlsObservationErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotConfigured => "experimental remote mTLS observation is not configured",
            Self::ObservationSequenceExhausted => {
                "experimental remote mTLS observation sequence exhausted"
            }
            Self::NoLiveLink => "the live Zenoh Session reported no links",
            Self::TooManyLiveLinks => {
                "the live Zenoh Session reported too many links for one bounded snapshot"
            }
            Self::NoLiveRemoteMtlsLink => "the live Zenoh Session reported no remote TLS link",
            Self::MissingTlsAuthIdentifier => "a live TLS link has no experimental certificate CN",
            Self::UnexpectedTlsPeer => {
                "a live TLS link has an unenrolled experimental certificate CN"
            }
            Self::MissingExpectedTlsPeer => "an expected experimental TLS peer has no live link",
            Self::PeerZenohIdConflict => {
                "experimental TLS identity and Zenoh ID mapping is ambiguous"
            }
            Self::InvalidLinkObservation => {
                "Zenoh returned an invalid experimental link observation"
            }
        })
    }
}

impl std::error::Error for ExperimentalRemoteMtlsObservationErrorV1 {}

impl From<FabricContractError> for FabricError {
    fn from(value: FabricContractError) -> Self {
        Self::Contract(value)
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, path::PathBuf};

    use paraegox_kernel::digest::Digest32;
    use paraegox_runtime_contracts::distributed_agent_stack_plan::DistributedFabricSessionEpochV1;
    use serde_json::{Value, json};

    use super::{
        ExperimentalPeerCommonNameV1, ExperimentalRawZenohLink,
        ExperimentalRemoteMtlsConfigErrorV1, ExperimentalRemoteMtlsObservationErrorV1,
        ExperimentalRemoteMtlsPeerBindingV1, FabricConfigError, FabricError, FabricService,
        FabricServiceConfig, MAX_EXPERIMENTAL_OBSERVED_LINKS, MAX_KEY_EXPRESSION_BYTES,
        PreparedRemoteAgentProxyListenerV2, PrincipalRef, REMOTE_AGENT_TRANSPORT_MAX_MESSAGE_BYTES,
        RemoteTlsEndpoint, ResolvedRemoteMtlsConnectorCredentialFilesV1,
        ResolvedRemoteMtlsCredentialFiles, ResolvedRemoteMtlsIdentityFiles,
        ResolvedRemoteMtlsListenerCredentialFilesV1, SessionEndpoint,
        classify_and_advance_experimental_remote_mtls_links,
        classify_experimental_remote_mtls_links, try_fabric_session_epoch_with,
    };
    use crate::restricted_runtime_apply_peer_certificate_common_name_v1;

    fn session_epoch(seed: u8) -> DistributedFabricSessionEpochV1 {
        DistributedFabricSessionEpochV1::try_from_bytes([seed; 16]).expect("session epoch")
    }

    fn available_tcp_endpoint() -> SessionEndpoint {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral TCP listener");
        let address = listener.local_addr().expect("ephemeral TCP address");
        drop(listener);
        SessionEndpoint::try_new(format!("tcp/{address}")).expect("loopback endpoint")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn live_service_session_epoch_getter_returns_the_owned_epoch() {
        let config = FabricServiceConfig::try_peer(vec![available_tcp_endpoint()], Vec::new())
            .expect("one listener is a valid peer configuration");
        let service = FabricService::start(config)
            .await
            .expect("test Fabric must start");

        assert_eq!(service.session_epoch(), service.session_epoch);

        service.shutdown().await.expect("test Fabric must stop");
    }

    fn role_identity(role: &str) -> ResolvedRemoteMtlsIdentityFiles {
        ResolvedRemoteMtlsIdentityFiles::try_new(
            PathBuf::from(format!("/run/paraegox/tls/{role}-certificate.pem")),
            PathBuf::from(format!("/run/paraegox/tls/{role}-private-key.pem")),
        )
        .unwrap()
    }

    fn credentials() -> ResolvedRemoteMtlsCredentialFiles {
        ResolvedRemoteMtlsCredentialFiles::try_new(
            PathBuf::from("/run/paraegox/tls/root-ca.pem"),
            role_identity("listen"),
            role_identity("connect"),
        )
        .unwrap()
    }

    fn listener_credentials() -> ResolvedRemoteMtlsListenerCredentialFilesV1 {
        ResolvedRemoteMtlsListenerCredentialFilesV1::try_new(
            PathBuf::from("/run/paraegox/tls/root-ca.pem"),
            role_identity("listen"),
        )
        .unwrap()
    }

    fn connector_credentials() -> ResolvedRemoteMtlsConnectorCredentialFilesV1 {
        ResolvedRemoteMtlsConnectorCredentialFilesV1::try_new(
            PathBuf::from("/run/paraegox/tls/root-ca.pem"),
            role_identity("connect"),
        )
        .unwrap()
    }

    const fn principal(marker: u8) -> PrincipalRef {
        PrincipalRef::from_bytes([marker; 16])
    }

    fn proxy_listener_config() -> FabricServiceConfig {
        FabricServiceConfig::try_remote_agent_proxy_listener_v2(
            RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
            listener_credentials(),
            principal(0x51),
            "paraegox/agent/submit",
            "paraegox/agent/control",
        )
        .unwrap()
    }

    fn prepared_proxy_listener(seed: u8) -> PreparedRemoteAgentProxyListenerV2 {
        PreparedRemoteAgentProxyListenerV2::try_prepare_with(
            proxy_listener_config(),
            |destination| {
                destination.copy_from_slice(&[seed; 16]);
                Ok(())
            },
        )
        .unwrap()
    }

    fn json_config(config: &zenoh::Config, key: &str) -> Value {
        serde_json::from_str(&config.get_json(key).unwrap()).unwrap()
    }

    fn assert_remote_agent_session_hardening(config: &zenoh::Config) {
        for (key, expected) in [
            ("scouting/multicast/enabled", "false"),
            ("scouting/gossip/enabled", "false"),
            ("connect/timeout_ms", "0"),
            ("connect/exit_on_failure", "true"),
            ("listen/timeout_ms", "0"),
            ("listen/exit_on_failure", "true"),
            ("open/return_conditions/connect_scouted", "false"),
            ("open/return_conditions/declares", "true"),
            ("adminspace/enabled", "false"),
            ("plugins_loading/enabled", "false"),
            ("transport/unicast/accept_pending", "1"),
            ("transport/unicast/max_sessions", "1"),
            ("transport/unicast/max_links", "1"),
            ("transport/link/tls/enable_mtls", "true"),
            ("transport/link/tls/verify_name_on_connect", "true"),
            ("transport/link/tls/close_link_on_expiration", "true"),
        ] {
            assert_eq!(config.get_json(key).unwrap(), expected, "{key}");
        }
        assert_eq!(
            config
                .get_json("transport/link/rx/max_message_size")
                .unwrap(),
            REMOTE_AGENT_TRANSPORT_MAX_MESSAGE_BYTES.to_string()
        );
    }

    fn assert_remote_agent_acl(
        config: &zenoh::Config,
        expected_common_name: &str,
        expected_egress_messages: Value,
        expected_ingress_messages: Value,
    ) {
        let acl = json_config(config, "access_control");
        assert_eq!(acl["enabled"], true);
        assert_eq!(acl["default_permission"], "deny");
        let rules = acl["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["permission"], "allow");
        assert_eq!(rules[0]["flows"], json!(["egress"]));
        assert_eq!(rules[0]["messages"], expected_egress_messages);
        assert_eq!(
            rules[0]["key_exprs"],
            json!(["paraegox/agent/submit", "paraegox/agent/control"])
        );
        assert_eq!(rules[1]["permission"], "allow");
        assert_eq!(rules[1]["flows"], json!(["ingress"]));
        assert_eq!(rules[1]["messages"], expected_ingress_messages);
        assert_eq!(
            rules[1]["key_exprs"],
            json!(["paraegox/agent/submit", "paraegox/agent/control"])
        );
        let subjects = acl["subjects"].as_array().unwrap();
        assert_eq!(subjects.len(), 1);
        assert_eq!(
            subjects[0]["cert_common_names"],
            json!([expected_common_name])
        );
        assert_eq!(subjects[0]["link_protocols"], json!(["tls"]));
        assert_eq!(
            acl["policies"],
            json!([{
                "id": null,
                "rules": [rules[0]["id"].as_str().unwrap(), rules[1]["id"].as_str().unwrap()],
                "subjects": ["remote-agent-expected-peer-v1"]
            }])
        );
    }

    fn experimental_peer(
        endpoint: &str,
        common_name: &str,
        digest_byte: u8,
    ) -> ExperimentalRemoteMtlsPeerBindingV1 {
        ExperimentalRemoteMtlsPeerBindingV1::new(
            RemoteTlsEndpoint::try_new(endpoint).unwrap(),
            ExperimentalPeerCommonNameV1::try_new(common_name).unwrap(),
            Digest32::from_bytes([digest_byte; 32]),
        )
    }

    fn observed_link(
        peer_zenoh_id: &str,
        source_locator: &str,
        destination_locator: &str,
        common_name: Option<&str>,
    ) -> ExperimentalRawZenohLink {
        ExperimentalRawZenohLink {
            peer_zenoh_id: peer_zenoh_id.to_owned(),
            source_locator: source_locator.to_owned(),
            destination_locator: destination_locator.to_owned(),
            auth_identifier: common_name.map(str::to_owned),
        }
    }

    #[test]
    fn session_configuration_has_no_discovery_or_protocol_fallback() {
        assert_eq!(
            SessionEndpoint::try_new("udp/127.0.0.1:7447"),
            Err(FabricConfigError::UnsupportedEndpointProtocol)
        );
        assert_eq!(
            FabricServiceConfig::try_peer(Vec::new(), Vec::new()),
            Err(FabricConfigError::NoEndpoint)
        );
        let endpoint = SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap();
        let config = FabricServiceConfig::try_peer(vec![endpoint], Vec::new()).unwrap();
        let zenoh = config.build_zenoh_config().unwrap();
        assert_eq!(
            zenoh.get_json("scouting/multicast/enabled").unwrap(),
            "false"
        );
        assert_eq!(zenoh.get_json("scouting/gossip/enabled").unwrap(), "false");
        assert_eq!(
            zenoh.get_json("transport/link/protocols").unwrap(),
            "[\"tcp\"]"
        );
    }

    #[test]
    fn remote_agent_configs_lock_role_specific_transport_and_exact_acl() {
        let mac_principal = principal(0x51);
        let ubuntu_principal = principal(0x52);
        let listener = FabricServiceConfig::try_remote_agent_listener_v1(
            SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap(),
            RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
            listener_credentials(),
            mac_principal,
            "paraegox/agent/submit",
            "paraegox/agent/control",
        )
        .unwrap();
        assert_eq!(
            format!("{listener:?}"),
            "FabricServiceConfig { transport_profile: \"remote-agent-listener-mtls-v1\", .. }"
        );
        let listener = listener.build_zenoh_config().unwrap();
        assert_remote_agent_session_hardening(&listener);
        assert_eq!(listener.get_json("mode").unwrap(), "\"peer\"");
        assert_eq!(
            listener.get_json("listen/endpoints").unwrap(),
            "[\"tcp/127.0.0.1:7447\",\"tls/192.0.2.10:7447\"]"
        );
        assert_eq!(listener.get_json("connect/endpoints").unwrap(), "[]");
        assert_eq!(
            listener.get_json("transport/link/protocols").unwrap(),
            "[\"tcp\",\"tls\"]"
        );
        assert_eq!(
            listener
                .get_json("transport/link/tls/root_ca_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/root-ca.pem\""
        );
        assert_eq!(
            listener
                .get_json("transport/link/tls/listen_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/listen-certificate.pem\""
        );
        assert_eq!(
            listener
                .get_json("transport/link/tls/listen_private_key")
                .unwrap(),
            "\"/run/paraegox/tls/listen-private-key.pem\""
        );
        assert_eq!(
            listener
                .get_json("transport/link/tls/connect_certificate")
                .unwrap(),
            "null"
        );
        assert_eq!(
            listener
                .get_json("transport/link/tls/connect_private_key")
                .unwrap(),
            "null"
        );
        assert_remote_agent_acl(
            &listener,
            &restricted_runtime_apply_peer_certificate_common_name_v1(mac_principal),
            json!(["reply", "declare_queryable"]),
            json!(["query"]),
        );

        let connector = FabricServiceConfig::try_remote_agent_connector_v1(
            RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
            connector_credentials(),
            ubuntu_principal,
            "paraegox/agent/submit",
            "paraegox/agent/control",
        )
        .unwrap();
        assert_eq!(
            format!("{connector:?}"),
            "FabricServiceConfig { transport_profile: \"remote-agent-connector-mtls-v1\", .. }"
        );
        let connector = connector.build_zenoh_config().unwrap();
        assert_remote_agent_session_hardening(&connector);
        assert_eq!(connector.get_json("mode").unwrap(), "\"client\"");
        assert_eq!(connector.get_json("listen/endpoints").unwrap(), "[]");
        assert_eq!(
            connector.get_json("connect/endpoints").unwrap(),
            "[\"tls/192.0.2.10:7447\"]"
        );
        assert_eq!(
            connector.get_json("transport/link/protocols").unwrap(),
            "[\"tls\"]"
        );
        assert_eq!(
            connector
                .get_json("transport/link/tls/root_ca_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/root-ca.pem\""
        );
        assert_eq!(
            connector
                .get_json("transport/link/tls/connect_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/connect-certificate.pem\""
        );
        assert_eq!(
            connector
                .get_json("transport/link/tls/connect_private_key")
                .unwrap(),
            "\"/run/paraegox/tls/connect-private-key.pem\""
        );
        assert_eq!(
            connector
                .get_json("transport/link/tls/listen_certificate")
                .unwrap(),
            "null"
        );
        assert_eq!(
            connector
                .get_json("transport/link/tls/listen_private_key")
                .unwrap(),
            "null"
        );
        assert_remote_agent_acl(
            &connector,
            &restricted_runtime_apply_peer_certificate_common_name_v1(ubuntu_principal),
            json!(["query"]),
            json!(["reply", "declare_queryable"]),
        );
    }

    #[test]
    fn remote_agent_proxy_listener_v2_is_tls_only_with_exact_hardening_and_acl() {
        let mac_principal = principal(0x51);
        let config = proxy_listener_config();
        assert_eq!(
            format!("{config:?}"),
            "FabricServiceConfig { transport_profile: \"remote-agent-proxy-listener-mtls-v2\", .. }"
        );

        let zenoh = config.build_zenoh_config().unwrap();
        assert_remote_agent_session_hardening(&zenoh);
        assert_eq!(zenoh.get_json("mode").unwrap(), "\"peer\"");
        assert_eq!(
            zenoh.get_json("listen/endpoints").unwrap(),
            "[\"tls/192.0.2.10:7447\"]"
        );
        assert_eq!(zenoh.get_json("connect/endpoints").unwrap(), "[]");
        assert_eq!(
            zenoh.get_json("transport/link/protocols").unwrap(),
            "[\"tls\"]"
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/root_ca_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/root-ca.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/listen_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/listen-certificate.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/listen_private_key")
                .unwrap(),
            "\"/run/paraegox/tls/listen-private-key.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/connect_certificate")
                .unwrap(),
            "null"
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/connect_private_key")
                .unwrap(),
            "null"
        );
        assert_remote_agent_acl(
            &zenoh,
            &restricted_runtime_apply_peer_certificate_common_name_v1(mac_principal),
            json!(["reply", "declare_queryable"]),
            json!(["query"]),
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn proxy_listener_profile_is_confined_to_its_prepared_lifecycle() {
        let connector = FabricServiceConfig::try_remote_agent_connector_v1(
            RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
            connector_credentials(),
            principal(0x52),
            "paraegox/agent/submit",
            "paraegox/agent/control",
        )
        .unwrap();
        assert!(matches!(
            PreparedRemoteAgentProxyListenerV2::try_prepare(connector),
            Err(FabricError::RemoteAgentProxyListenerProfileRequired)
        ));
        assert!(matches!(
            FabricService::start(proxy_listener_config()).await,
            Err(FabricError::RemoteAgentProxyListenerProfileRequired)
        ));
    }

    #[test]
    fn remote_agent_routes_and_principals_fail_closed_before_session_open() {
        let build = |peer_principal, submit: String, control: String| {
            FabricServiceConfig::try_remote_agent_listener_v1(
                SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap(),
                RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
                listener_credentials(),
                peer_principal,
                submit,
                control,
            )
        };
        assert_eq!(
            build(
                PrincipalRef::from_bytes([0; 16]),
                "paraegox/agent/submit".to_owned(),
                "paraegox/agent/control".to_owned(),
            ),
            Err(FabricConfigError::ZeroExpectedRemoteAgentPrincipal)
        );
        assert_eq!(
            FabricServiceConfig::try_remote_agent_connector_v1(
                RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
                connector_credentials(),
                PrincipalRef::from_bytes([0; 16]),
                "paraegox/agent/submit",
                "paraegox/agent/control",
            ),
            Err(FabricConfigError::ZeroExpectedRemoteAgentPrincipal)
        );
        assert_eq!(
            FabricServiceConfig::try_remote_agent_proxy_listener_v2(
                RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
                listener_credentials(),
                PrincipalRef::from_bytes([0; 16]),
                "paraegox/agent/submit",
                "paraegox/agent/control",
            ),
            Err(FabricConfigError::ZeroExpectedRemoteAgentPrincipal)
        );
        assert_eq!(
            build(
                principal(0x51),
                "paraegox/agent/submit".to_owned(),
                "paraegox/agent/submit".to_owned(),
            ),
            Err(FabricConfigError::DuplicateRemoteAgentRoute)
        );
        assert_eq!(
            build(
                principal(0x51),
                String::new(),
                "paraegox/agent/control".to_owned(),
            ),
            Err(FabricConfigError::EmptyKeyExpression)
        );
        assert_eq!(
            build(
                principal(0x51),
                "a".repeat(MAX_KEY_EXPRESSION_BYTES + 1),
                "paraegox/agent/control".to_owned(),
            ),
            Err(FabricConfigError::KeyExpressionTooLong)
        );
        for invalid in [
            "paraegox/**",
            "paraegox/$agent",
            "paraegox/agent\nsubmit",
            "paraegox/agént",
            "paraegox//agent",
            "paraegox/agent/",
        ] {
            assert_eq!(
                build(
                    principal(0x51),
                    invalid.to_owned(),
                    "paraegox/agent/control".to_owned(),
                ),
                Err(FabricConfigError::NonConcreteKeyExpression),
                "{invalid:?} must fail closed"
            );
        }
        assert!(
            build(
                principal(0x51),
                "paraegox/agent".to_owned(),
                "paraegox/agent/control".to_owned(),
            )
            .is_ok(),
            "distinct exact parent and child routes remain valid key expressions"
        );
    }

    #[test]
    fn plaintext_endpoints_are_canonical_loopback_only() {
        for invalid in [
            "tcp/127.0.0.1:0",
            "tcp/127.0.0.1:07447",
            "tcp/127.0.0.1:65536",
            "tcp/127.0.0.2:7447",
            "tcp/0.0.0.0:7447",
            "tcp/localhost:7447",
            "tcp/[::1]:7447",
            "tcp/127.0.0.1:7447?prio=1-2",
            "tcp/127.0.0.1:7447#so_sndbuf=1",
        ] {
            assert_eq!(
                SessionEndpoint::try_new(invalid),
                Err(FabricConfigError::NonCanonicalLoopbackEndpoint),
                "{invalid} must fail closed"
            );
        }
        assert_eq!(
            SessionEndpoint::try_new("tls/192.0.2.10:7447"),
            Err(FabricConfigError::UnsupportedEndpointProtocol)
        );
        assert_eq!(
            SessionEndpoint::try_new("tcp/127.0.0.1:65535")
                .unwrap()
                .as_str(),
            "tcp/127.0.0.1:65535"
        );
    }

    #[test]
    fn remote_tls_endpoints_match_the_non_loopback_ipv4_contract() {
        for valid in [
            "tls/10.0.0.1:1",
            "tls/192.0.2.10:7447",
            "tls/169.254.1.2:65535",
        ] {
            assert_eq!(RemoteTlsEndpoint::try_new(valid).unwrap().as_str(), valid);
        }
        for invalid in [
            "tls/127.0.0.1:7447",
            "tls/0.0.0.0:7447",
            "tls/224.0.0.1:7447",
            "tls/255.255.255.255:7447",
            "tls/192.0.2.10:0",
            "tls/192.0.2.10:07447",
            "tls/192.000.2.10:7447",
            "tls/example.test:7447",
            "tls/[2001:db8::1]:7447",
            "tls/192.0.2.10:7447?prio=1-2",
            "tls/192.0.2.10:7447#bind=0.0.0.0:0",
        ] {
            assert_eq!(
                RemoteTlsEndpoint::try_new(invalid),
                Err(FabricConfigError::NonCanonicalRemoteTlsEndpoint),
                "{invalid} must fail closed"
            );
        }
        assert_eq!(
            RemoteTlsEndpoint::try_new("tcp/192.0.2.10:7447"),
            Err(FabricConfigError::UnsupportedEndpointProtocol)
        );
    }

    #[test]
    fn experimental_peer_common_names_are_bounded_and_canonical() {
        for valid in ["peer-a", "runtime-01.paraegox.internal", "a.b.c"] {
            assert_eq!(
                ExperimentalPeerCommonNameV1::try_new(valid)
                    .unwrap()
                    .as_str(),
                valid
            );
        }
        let overlong = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(63)
        );
        for invalid in [
            "".to_owned(),
            "Peer-A".to_owned(),
            "peer_a".to_owned(),
            "peer a".to_owned(),
            "-peer".to_owned(),
            "peer-".to_owned(),
            ".peer".to_owned(),
            "peer.".to_owned(),
            "peer..a".to_owned(),
            "peer\n".to_owned(),
            format!("{}.internal", "a".repeat(64)),
            overlong,
        ] {
            assert_eq!(
                ExperimentalPeerCommonNameV1::try_new(invalid),
                Err(ExperimentalRemoteMtlsConfigErrorV1::InvalidPeerCommonName)
            );
        }
    }

    #[test]
    fn experimental_secured_topology_rejects_duplicate_cn_and_binding_digest() {
        let build = |peers| {
            FabricServiceConfig::try_experimental_secured_hybrid_peer_with_cn_bindings(
                SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap(),
                RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
                peers,
                credentials(),
            )
        };
        assert_eq!(
            build(vec![
                experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
                experimental_peer("tls/192.0.2.12:7447", "peer-a", 2),
            ]),
            Err(ExperimentalRemoteMtlsConfigErrorV1::DuplicatePeerCommonName)
        );
        assert_eq!(
            build(vec![
                experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
                experimental_peer("tls/192.0.2.12:7447", "peer-b", 1),
            ]),
            Err(ExperimentalRemoteMtlsConfigErrorV1::DuplicateIdentityBinding)
        );
        let config = build(vec![
            experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
            experimental_peer("tls/192.0.2.12:7447", "peer-b", 2),
        ])
        .unwrap();
        assert_eq!(
            format!("{config:?}"),
            "FabricServiceConfig { transport_profile: \"experimental-secured-hybrid-mtls-cn\", .. }"
        );
        assert!(!format!("{config:?}").contains("peer-a"));
    }

    #[test]
    fn experimental_link_snapshot_selects_only_enrolled_live_tls_identities() {
        let expected = [
            experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
            experimental_peer("tls/192.0.2.12:7447", "peer-b", 2),
        ];
        let links = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "b2",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.12:50002",
                Some("peer-b"),
            ),
            observed_link(
                "c3",
                "tcp/127.0.0.1:7447",
                "tcp/127.0.0.1:50003",
                Some("peer-not-tls"),
            ),
        ];
        let snapshot = classify_experimental_remote_mtls_links(
            &expected,
            session_epoch(0xe1),
            "f0".to_owned(),
            7,
            &links,
        )
        .unwrap();
        assert_eq!(snapshot.session_epoch(), session_epoch(0xe1));
        assert_eq!(snapshot.local_zenoh_id(), "f0");
        assert_eq!(snapshot.observation_sequence(), 8);
        assert_eq!(snapshot.remote_peers().len(), 2);
        assert_eq!(
            snapshot
                .remote_peers()
                .iter()
                .map(|peer| peer.observation_sequence())
                .collect::<Vec<_>>(),
            vec![7, 8]
        );
        assert_eq!(
            snapshot.remote_peers()[0].identity_binding_digest(),
            Digest32::from_bytes([1; 32])
        );
        assert_eq!(snapshot.remote_peers()[0].peer_zenoh_id(), "a1");
        assert!(
            snapshot.remote_peers()[0]
                .source_locator()
                .starts_with("tls/")
        );
        assert!(
            snapshot.remote_peers()[0]
                .destination_locator()
                .starts_with("tls/")
        );
        let debug = format!("{snapshot:?}");
        assert!(!debug.contains("peer-a"));
        assert!(!debug.contains("tls/"));
        assert!(!debug.contains("192.0.2.10"));
        assert!(!debug.contains("192.0.2.11"));
    }

    #[test]
    fn experimental_link_snapshot_keeps_config_order_when_links_are_reversed() {
        let expected = [
            experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
            experimental_peer("tls/192.0.2.12:7447", "peer-b", 2),
        ];
        let links = [
            observed_link(
                "b2",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.12:50002",
                Some("peer-b"),
            ),
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
        ];

        let snapshot = classify_experimental_remote_mtls_links(
            &expected,
            session_epoch(0xe1),
            "f0".to_owned(),
            7,
            &links,
        )
        .unwrap();

        assert_eq!(
            snapshot
                .remote_peers()
                .iter()
                .map(|peer| peer.identity_binding_digest())
                .collect::<Vec<_>>(),
            vec![Digest32::from_bytes([1; 32]), Digest32::from_bytes([2; 32])]
        );
        assert_eq!(snapshot.remote_peers()[0].peer_zenoh_id(), "a1");
        assert_eq!(snapshot.remote_peers()[1].peer_zenoh_id(), "b2");
    }

    #[test]
    fn duplicate_same_cn_and_zid_selects_one_stable_minimum_locator_row() {
        let expected = [experimental_peer("tls/192.0.2.11:7447", "peer-a", 1)];
        let larger_then_smaller = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50002",
                Some("peer-a"),
            ),
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
        ];
        let smaller_then_larger = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50002",
                Some("peer-a"),
            ),
        ];

        let first = classify_experimental_remote_mtls_links(
            &expected,
            session_epoch(0xe1),
            "f0".to_owned(),
            17,
            &larger_then_smaller,
        )
        .unwrap();
        let second = classify_experimental_remote_mtls_links(
            &expected,
            session_epoch(0xe1),
            "f0".to_owned(),
            17,
            &smaller_then_larger,
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.remote_peers().len(), 1);
        assert_eq!(first.observation_sequence(), 17);
        assert_eq!(first.remote_peers()[0].observation_sequence(), 17);
        assert_eq!(
            first.remote_peers()[0].destination_locator(),
            "tls/192.0.2.11:50001"
        );
    }

    #[test]
    fn session_epoch_generation_fails_closed_once_on_unavailable_or_zero_entropy() {
        for outcome in [Err(()), Ok([0_u8; 16])] {
            let mut calls = 0;
            assert!(matches!(
                try_fabric_session_epoch_with(|destination| {
                    calls += 1;
                    let bytes = outcome?;
                    destination.copy_from_slice(&bytes);
                    Ok(())
                }),
                Err(FabricError::SessionEpochUnavailable)
            ));
            assert_eq!(calls, 1, "session epoch generation must not retry");
        }
    }

    #[test]
    fn proxy_listener_prepare_builds_without_open_and_entropy_fails_closed_once() {
        let mut calls = 0;
        let prepared = PreparedRemoteAgentProxyListenerV2::try_prepare_with(
            proxy_listener_config(),
            |destination| {
                calls += 1;
                destination.copy_from_slice(&[0xa5; 16]);
                Ok(())
            },
        )
        .expect("prepare must not open the nonlocal endpoint or read nonexistent TLS files");
        assert_eq!(calls, 1);
        assert_eq!(prepared.session_epoch(), session_epoch(0xa5));
        assert_eq!(
            prepared.zenoh_config.get_json("listen/endpoints").unwrap(),
            "[\"tls/192.0.2.10:7447\"]"
        );
        assert_eq!(
            prepared.zenoh_config.get_json("connect/endpoints").unwrap(),
            "[]"
        );

        for outcome in [Err(()), Ok([0_u8; 16])] {
            let mut entropy_calls = 0;
            assert!(matches!(
                PreparedRemoteAgentProxyListenerV2::try_prepare_with(
                    proxy_listener_config(),
                    |destination| {
                        entropy_calls += 1;
                        let bytes = outcome?;
                        destination.copy_from_slice(&bytes);
                        Ok(())
                    },
                ),
                Err(FabricError::SessionEpochUnavailable)
            ));
            assert_eq!(entropy_calls, 1, "prepare entropy must not retry");
        }

        let mut wrong_profile_entropy_calls = 0;
        let wrong_profile = FabricServiceConfig::try_peer(
            vec![SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap()],
            Vec::new(),
        )
        .unwrap();
        assert!(matches!(
            PreparedRemoteAgentProxyListenerV2::try_prepare_with(wrong_profile, |destination| {
                wrong_profile_entropy_calls += 1;
                destination.copy_from_slice(&[0xa6; 16]);
                Ok(())
            },),
            Err(FabricError::RemoteAgentProxyListenerProfileRequired)
        ));
        assert_eq!(
            wrong_profile_entropy_calls, 0,
            "wrong transport role must fail before entropy is consumed"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn proxy_listener_start_failure_returns_no_live_owner_token() {
        let prepared = prepared_proxy_listener(0xa7);
        let mut open_calls = 0;
        let result = prepared
            .start_with(|_zenoh_config| {
                open_calls += 1;
                async { Err::<zenoh::Session, ()>(()) }
            })
            .await;

        assert!(matches!(result, Err(FabricError::SessionOpenFailed)));
        assert_eq!(
            open_calls, 1,
            "a consumed prepared plan must not retry open"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reserved_proxy_epoch_becomes_live_and_shutdown_releases_its_session() {
        let loopback_endpoint = available_tcp_endpoint();
        let loopback_address = loopback_endpoint
            .as_str()
            .strip_prefix("tcp/")
            .unwrap()
            .to_owned();
        let loopback_config = FabricServiceConfig::try_peer(vec![loopback_endpoint], Vec::new())
            .unwrap()
            .build_zenoh_config()
            .unwrap();
        let prepared = prepared_proxy_listener(0xa8);
        let reserved_epoch = prepared.session_epoch();

        let live = prepared
            .start_with(move |proxy_config| async move {
                assert_eq!(
                    proxy_config.get_json("transport/link/protocols").unwrap(),
                    "[\"tls\"]"
                );
                zenoh::open(loopback_config).await.map_err(|_| ())
            })
            .await
            .expect("test opener must create one live private Session");
        assert_eq!(live.session_epoch(), reserved_epoch);
        assert_eq!(
            live.routes.as_array(),
            ["paraegox/agent/submit", "paraegox/agent/control"]
        );
        let bind_error = TcpListener::bind(&loopback_address)
            .expect_err("live wrapper must retain ownership of its private Session");
        assert_eq!(bind_error.kind(), std::io::ErrorKind::AddrInUse);

        live.shutdown()
            .await
            .expect("consuming shutdown must close the private Session");
        let rebound = TcpListener::bind(&loopback_address)
            .expect("shutdown must release the private Session listener");
        drop(rebound);
    }

    #[test]
    fn one_fabric_service_epoch_and_peer_sequence_high_water_continue_across_snapshots() {
        let mut calls = 0;
        let service_epoch = try_fabric_session_epoch_with(|destination| {
            calls += 1;
            destination.copy_from_slice(&[0xe2; 16]);
            Ok(())
        })
        .unwrap();
        let expected = [
            experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
            experimental_peer("tls/192.0.2.12:7447", "peer-b", 2),
        ];
        let links = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "b2",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.12:50002",
                Some("peer-b"),
            ),
        ];
        let mut observation_sequence_high_water = 0;
        let first = classify_and_advance_experimental_remote_mtls_links(
            &mut observation_sequence_high_water,
            &expected,
            service_epoch,
            "f0".to_owned(),
            &links,
        )
        .unwrap();
        let second = classify_and_advance_experimental_remote_mtls_links(
            &mut observation_sequence_high_water,
            &expected,
            service_epoch,
            "f0".to_owned(),
            &links,
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(first.session_epoch(), session_epoch(0xe2));
        assert_eq!(second.session_epoch(), first.session_epoch());
        assert_eq!(first.observation_sequence(), 2);
        assert_eq!(second.observation_sequence(), 4);
        assert_eq!(observation_sequence_high_water, 4);
        assert_eq!(first.remote_peers()[0].observation_sequence(), 1);
        assert_eq!(first.remote_peers()[1].observation_sequence(), 2);
        assert_eq!(second.remote_peers()[0].observation_sequence(), 3);
        assert_eq!(second.remote_peers()[1].observation_sequence(), 4);
    }

    #[test]
    fn peer_sequence_high_water_does_not_advance_on_failure_or_overflow() {
        let expected = [
            experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
            experimental_peer("tls/192.0.2.12:7447", "peer-b", 2),
        ];
        let links = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "b2",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.12:50002",
                Some("peer-b"),
            ),
        ];
        let mut high_water = 40;
        assert_eq!(
            classify_and_advance_experimental_remote_mtls_links(
                &mut high_water,
                &expected,
                session_epoch(0xe3),
                "f0".to_owned(),
                &links[..1],
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::MissingExpectedTlsPeer)
        );
        assert_eq!(high_water, 40);

        let snapshot = classify_and_advance_experimental_remote_mtls_links(
            &mut high_water,
            &expected,
            session_epoch(0xe3),
            "f0".to_owned(),
            &links,
        )
        .unwrap();
        assert_eq!(high_water, 42);
        assert_eq!(snapshot.remote_peers()[0].observation_sequence(), 41);
        assert_eq!(snapshot.remote_peers()[1].observation_sequence(), 42);

        high_water = u64::MAX - 1;
        assert_eq!(
            classify_and_advance_experimental_remote_mtls_links(
                &mut high_water,
                &expected,
                session_epoch(0xe3),
                "f0".to_owned(),
                &links,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)
        );
        assert_eq!(high_water, u64::MAX - 1);

        high_water = u64::MAX;
        assert_eq!(
            classify_and_advance_experimental_remote_mtls_links(
                &mut high_water,
                &expected[..1],
                session_epoch(0xe3),
                "f0".to_owned(),
                &links[..1],
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)
        );
        assert_eq!(high_water, u64::MAX);
    }

    #[test]
    fn single_peer_allocates_u64_max_once_before_sequence_exhaustion() {
        let expected = [experimental_peer("tls/192.0.2.11:7447", "peer-a", 1)];
        let links = [observed_link(
            "a1",
            "tls/192.0.2.10:7447",
            "tls/192.0.2.11:50001",
            Some("peer-a"),
        )];
        let mut high_water = u64::MAX - 1;

        let snapshot = classify_and_advance_experimental_remote_mtls_links(
            &mut high_water,
            &expected,
            session_epoch(0xe3),
            "f0".to_owned(),
            &links,
        )
        .unwrap();
        assert_eq!(snapshot.observation_sequence(), u64::MAX);
        assert_eq!(snapshot.remote_peers()[0].observation_sequence(), u64::MAX);
        assert_eq!(high_water, u64::MAX);

        assert_eq!(
            classify_and_advance_experimental_remote_mtls_links(
                &mut high_water,
                &expected,
                session_epoch(0xe3),
                "f0".to_owned(),
                &links,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::ObservationSequenceExhausted)
        );
        assert_eq!(high_water, u64::MAX);
    }

    #[test]
    fn experimental_link_snapshot_rejects_wrong_or_additional_tls_cn() {
        let expected = [experimental_peer("tls/192.0.2.11:7447", "peer-a", 1)];
        let wrong = [observed_link(
            "a1",
            "tls/192.0.2.10:7447",
            "tls/192.0.2.11:50001",
            Some("peer-wrong"),
        )];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &wrong,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::UnexpectedTlsPeer)
        );
        let additional = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "b2",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.12:50002",
                Some("peer-extra"),
            ),
        ];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &additional,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::UnexpectedTlsPeer)
        );
    }

    #[test]
    fn tcp_or_absent_links_cannot_become_an_experimental_tls_snapshot() {
        let expected = [experimental_peer("tls/192.0.2.11:7447", "peer-a", 1)];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &[],
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::NoLiveLink)
        );
        let tcp = [observed_link(
            "a1",
            "tcp/127.0.0.1:7447",
            "tcp/127.0.0.1:50001",
            Some("peer-a"),
        )];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &tcp,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::NoLiveRemoteMtlsLink)
        );
        let too_many = (0..=MAX_EXPERIMENTAL_OBSERVED_LINKS)
            .map(|index| {
                observed_link(
                    &format!("{:x}", index + 1),
                    "tcp/127.0.0.1:7447",
                    "tcp/127.0.0.1:50001",
                    Some("peer-a"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &too_many,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::TooManyLiveLinks)
        );
    }

    #[test]
    fn experimental_tls_snapshot_rejects_missing_cn_peer_or_unique_zid() {
        let expected = [
            experimental_peer("tls/192.0.2.11:7447", "peer-a", 1),
            experimental_peer("tls/192.0.2.12:7447", "peer-b", 2),
        ];
        let missing_cn = [observed_link(
            "a1",
            "tls/192.0.2.10:7447",
            "tls/192.0.2.11:50001",
            None,
        )];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected[..1],
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &missing_cn,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::MissingTlsAuthIdentifier)
        );
        let one_peer = [observed_link(
            "a1",
            "tls/192.0.2.10:7447",
            "tls/192.0.2.11:50001",
            Some("peer-a"),
        )];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &one_peer,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::MissingExpectedTlsPeer)
        );
        let shared_zid = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.12:50002",
                Some("peer-b"),
            ),
        ];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected,
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &shared_zid,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::PeerZenohIdConflict)
        );
        let one_cn_multiple_zids = [
            observed_link(
                "a1",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50001",
                Some("peer-a"),
            ),
            observed_link(
                "b2",
                "tls/192.0.2.10:7447",
                "tls/192.0.2.11:50002",
                Some("peer-a"),
            ),
        ];
        assert_eq!(
            classify_experimental_remote_mtls_links(
                &expected[..1],
                session_epoch(0xe1),
                "f0".to_owned(),
                1,
                &one_cn_multiple_zids,
            ),
            Err(ExperimentalRemoteMtlsObservationErrorV1::PeerZenohIdConflict)
        );
    }

    #[test]
    fn resolved_tls_files_accept_paths_only_and_redact_debug() {
        for invalid_certificate_path in [
            "relative/certificate.pem",
            "/",
            "/run/paraegox/tls/../certificate.pem",
            "/run/paraegox/tls/./certificate.pem",
            "/run/paraegox//tls/certificate.pem",
            "/run/paraegox/tls/certificate.pem/",
            "/run/paraegox/tls/certificate\n.pem",
        ] {
            assert_eq!(
                ResolvedRemoteMtlsIdentityFiles::try_new(
                    PathBuf::from(invalid_certificate_path),
                    PathBuf::from("/run/paraegox/tls/key.pem"),
                ),
                Err(FabricConfigError::InvalidTlsFilePath),
                "{invalid_certificate_path:?} must not be admitted as a normalized file path"
            );
        }
        let identity = role_identity("node-a");
        let identity_debug = format!("{identity:?}");
        assert!(!identity_debug.contains("node-a"));
        assert!(!identity_debug.contains("private-key"));
        let credentials = credentials();
        let credentials_debug = format!("{credentials:?}");
        assert!(!credentials_debug.contains("root-ca.pem"));
        assert!(!credentials_debug.contains("private-key"));
    }

    #[test]
    fn secured_hybrid_requires_the_exact_bounded_topology() {
        let loopback = SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap();
        let listen = RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap();
        assert_eq!(
            FabricServiceConfig::try_secured_hybrid_peer(
                loopback.clone(),
                listen.clone(),
                Vec::new(),
                credentials(),
            ),
            Err(FabricConfigError::NoTlsConnectEndpoint)
        );
        let repeated = RemoteTlsEndpoint::try_new("tls/192.0.2.11:7447").unwrap();
        assert_eq!(
            FabricServiceConfig::try_secured_hybrid_peer(
                loopback.clone(),
                listen.clone(),
                vec![repeated.clone(), repeated],
                credentials(),
            ),
            Err(FabricConfigError::DuplicateEndpoint)
        );
        assert_eq!(
            FabricServiceConfig::try_secured_hybrid_peer(
                loopback,
                listen.clone(),
                vec![listen],
                credentials(),
            ),
            Err(FabricConfigError::RemoteListenConnectEndpointConflict)
        );

        let maximum = (11..=18)
            .map(|octet| RemoteTlsEndpoint::try_new(format!("tls/192.0.2.{octet}:7447")).unwrap())
            .collect();
        assert!(
            FabricServiceConfig::try_secured_hybrid_peer(
                SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap(),
                RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
                maximum,
                credentials(),
            )
            .is_ok(),
            "exactly eight explicit TLS connectors must be admitted"
        );

        let too_many = (11..=19)
            .map(|octet| RemoteTlsEndpoint::try_new(format!("tls/192.0.2.{octet}:7447")).unwrap())
            .collect();
        assert_eq!(
            FabricServiceConfig::try_secured_hybrid_peer(
                SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap(),
                RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
                too_many,
                credentials(),
            ),
            Err(FabricConfigError::TooManyEndpoints)
        );
    }

    #[test]
    fn secured_hybrid_builds_fixed_mtls_without_scouting_or_fallback() {
        let config = FabricServiceConfig::try_secured_hybrid_peer(
            SessionEndpoint::try_new("tcp/127.0.0.1:7447").unwrap(),
            RemoteTlsEndpoint::try_new("tls/192.0.2.10:7447").unwrap(),
            vec![RemoteTlsEndpoint::try_new("tls/192.0.2.11:7447").unwrap()],
            credentials(),
        )
        .unwrap();
        let config_debug = format!("{config:?}");
        assert_eq!(
            config_debug,
            "FabricServiceConfig { transport_profile: \"secured-hybrid-mtls\", .. }"
        );
        assert!(!config_debug.contains("/run/paraegox"));

        let zenoh = config.build_zenoh_config().unwrap();
        assert_eq!(
            zenoh.get_json("listen/endpoints").unwrap(),
            "[\"tcp/127.0.0.1:7447\",\"tls/192.0.2.10:7447\"]"
        );
        assert_eq!(
            zenoh.get_json("connect/endpoints").unwrap(),
            "[\"tls/192.0.2.11:7447\"]"
        );
        assert_eq!(
            zenoh.get_json("transport/link/protocols").unwrap(),
            "[\"tcp\",\"tls\"]"
        );
        assert_eq!(
            zenoh.get_json("scouting/multicast/enabled").unwrap(),
            "false"
        );
        assert_eq!(zenoh.get_json("scouting/gossip/enabled").unwrap(), "false");
        assert_eq!(
            zenoh.get_json("transport/link/tls/enable_mtls").unwrap(),
            "true"
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/verify_name_on_connect")
                .unwrap(),
            "true"
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/close_link_on_expiration")
                .unwrap(),
            "true"
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/root_ca_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/root-ca.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/listen_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/listen-certificate.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/listen_private_key")
                .unwrap(),
            "\"/run/paraegox/tls/listen-private-key.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/connect_certificate")
                .unwrap(),
            "\"/run/paraegox/tls/connect-certificate.pem\""
        );
        assert_eq!(
            zenoh
                .get_json("transport/link/tls/connect_private_key")
                .unwrap(),
            "\"/run/paraegox/tls/connect-private-key.pem\""
        );
    }

    #[test]
    fn proxy_listener_candidate_remains_crate_private_and_move_only() {
        let service_source = include_str!("service.rs");
        let lib_source = include_str!("lib.rs");

        for declaration in [
            concat!("pub(crate) ", "fn try_remote_agent_proxy_listener_v2("),
            concat!("pub(crate) ", "struct PreparedRemoteAgentProxyListenerV2 {"),
            concat!("pub(crate) ", "struct RemoteAgentProxyListenerV2 {"),
        ] {
            assert!(
                service_source.contains(declaration),
                "missing crate-private declaration: {declaration}"
            );
        }
        for public_escape in [
            concat!("pub ", "fn try_remote_agent_proxy_listener_v2("),
            concat!("pub ", "struct PreparedRemoteAgentProxyListenerV2 {"),
            concat!("pub ", "struct RemoteAgentProxyListenerV2 {"),
            concat!("impl Clone ", "for PreparedRemoteAgentProxyListenerV2"),
            concat!("impl Copy ", "for PreparedRemoteAgentProxyListenerV2"),
            concat!("impl Clone ", "for RemoteAgentProxyListenerV2"),
            concat!("impl Copy ", "for RemoteAgentProxyListenerV2"),
        ] {
            assert!(
                !service_source.contains(public_escape),
                "crate-private lifecycle escaped through: {public_escape}"
            );
        }
        for root_escape in [
            "PreparedRemoteAgentProxyListenerV2",
            "RemoteAgentProxyListenerV2",
        ] {
            assert!(
                !lib_source.contains(root_escape),
                "crate root must not re-export {root_escape}"
            );
        }

        let live_impl = service_source
            .split(concat!("impl ", "RemoteAgentProxyListenerV2 {"))
            .nth(1)
            .expect("live owner impl")
            .split("/// Owns exactly one private Zenoh session")
            .next()
            .expect("bounded live owner impl");
        let visible_methods: Vec<_> = live_impl
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("pub"))
            .collect();
        assert_eq!(
            visible_methods,
            [
                "pub(crate) const fn session_epoch(&self) -> DistributedFabricSessionEpochV1 {",
                "pub(crate) async fn shutdown(self) -> Result<(), FabricError> {",
            ],
            "the live owner must not expose its raw Session or routes"
        );
    }

    #[cfg(target_os = "linux")]
    mod proxy_listener_real_network {
        use std::{
            fs,
            net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket},
            os::unix::fs::PermissionsExt,
            path::{Path, PathBuf},
            process::Command,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            time::{Duration, SystemTime, UNIX_EPOCH},
        };

        use paraegox_kernel::{digest::Digest32, identity::PrincipalRef};
        use paraegox_runtime_contracts::assignment::{BindingId, SchemaRef};
        use tokio::task::JoinHandle;

        use super::super::{
            FabricService, FabricServiceConfig, HandlerResponse, PortBinding,
            PreparedRemoteAgentProxyListenerV2, RemoteTlsEndpoint, RequestReceiver,
            RequestResponseBindingSpec, ResolvedRemoteMtlsIdentityFiles,
            ResolvedRemoteMtlsListenerCredentialFilesV1, SessionEndpoint,
        };
        use crate::{
            contract::{RequestId, ResponseStatus},
            ingress::IngressLimits,
            runtime_apply::restricted_runtime_apply_peer_certificate_common_name_v1,
        };

        const SUBMIT_ROUTE: &str = "paraegox/agent/submit";
        const CONTROL_ROUTE: &str = "paraegox/agent/control";
        const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

        struct TestDirectory(PathBuf);

        impl TestDirectory {
            fn new() -> Self {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("wall clock after Unix epoch")
                    .as_nanos();
                let path = std::env::temp_dir().join(format!(
                    "paraegox-internal-s1-mtls-{}-{nonce}",
                    std::process::id()
                ));
                fs::create_dir(&path).expect("create private test directory");
                let mut permissions = fs::metadata(&path)
                    .expect("read test directory metadata")
                    .permissions();
                permissions.set_mode(0o700);
                fs::set_permissions(&path, permissions).expect("restrict test directory");
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TestDirectory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        struct IdentityMaterial {
            certificate: PathBuf,
            private_key: PathBuf,
        }

        struct ListenerPki {
            root_ca: PathBuf,
            listener: IdentityMaterial,
        }

        impl ListenerPki {
            fn generate(directory: &Path, listener_ip: Ipv4Addr, common_name: &str) -> Self {
                let root_ca = directory.join("root-ca.pem");
                let root_key = directory.join("root-ca.key");
                run_openssl(&[
                    "req".to_owned(),
                    "-x509".to_owned(),
                    "-newkey".to_owned(),
                    "rsa:2048".to_owned(),
                    "-nodes".to_owned(),
                    "-sha256".to_owned(),
                    "-days".to_owned(),
                    "2".to_owned(),
                    "-subj".to_owned(),
                    "/CN=paraegox-internal-s1-test-ca".to_owned(),
                    "-addext".to_owned(),
                    "basicConstraints=critical,CA:TRUE".to_owned(),
                    "-addext".to_owned(),
                    "keyUsage=critical,keyCertSign,cRLSign".to_owned(),
                    "-keyout".to_owned(),
                    path_text(&root_key),
                    "-out".to_owned(),
                    path_text(&root_ca),
                ]);
                protect_private_key(&root_key);

                let listener =
                    issue_listener(directory, listener_ip, common_name, &root_ca, &root_key);
                Self { root_ca, listener }
            }
        }

        fn issue_listener(
            directory: &Path,
            listener_ip: Ipv4Addr,
            common_name: &str,
            root_ca: &Path,
            root_key: &Path,
        ) -> IdentityMaterial {
            let certificate = directory.join("listener.pem");
            let private_key = directory.join("listener.key");
            let request = directory.join("listener.csr");
            let extension_file = directory.join("listener.ext");
            fs::write(
                &extension_file,
                format!(
                    "basicConstraints=critical,CA:FALSE\n\
                     keyUsage=critical,digitalSignature,keyEncipherment\n\
                     extendedKeyUsage=serverAuth\n\
                     subjectAltName=IP:{listener_ip}\n"
                ),
            )
            .expect("write listener extension file");
            run_openssl(&[
                "req".to_owned(),
                "-new".to_owned(),
                "-newkey".to_owned(),
                "rsa:2048".to_owned(),
                "-nodes".to_owned(),
                "-sha256".to_owned(),
                "-subj".to_owned(),
                format!("/CN={common_name}"),
                "-keyout".to_owned(),
                path_text(&private_key),
                "-out".to_owned(),
                path_text(&request),
            ]);
            run_openssl(&[
                "x509".to_owned(),
                "-req".to_owned(),
                "-in".to_owned(),
                path_text(&request),
                "-CA".to_owned(),
                path_text(root_ca),
                "-CAkey".to_owned(),
                path_text(root_key),
                "-set_serial".to_owned(),
                "1001".to_owned(),
                "-days".to_owned(),
                "2".to_owned(),
                "-sha256".to_owned(),
                "-extfile".to_owned(),
                path_text(&extension_file),
                "-out".to_owned(),
                path_text(&certificate),
            ]);
            protect_private_key(&private_key);
            IdentityMaterial {
                certificate,
                private_key,
            }
        }

        fn run_openssl(arguments: &[String]) {
            let output = Command::new("openssl")
                .args(arguments)
                .output()
                .expect("openssl must be installed for the Linux mTLS test");
            assert!(
                output.status.success(),
                "openssl {arguments:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn protect_private_key(path: &Path) {
            let mut permissions = fs::metadata(path)
                .expect("read generated key metadata")
                .permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(path, permissions).expect("restrict generated private key");
        }

        fn path_text(path: &Path) -> String {
            path.to_str().expect("test path must be UTF-8").to_owned()
        }

        fn actual_non_loopback_ipv4() -> Ipv4Addr {
            let probe = UdpSocket::bind("0.0.0.0:0").expect("bind IPv4 route probe");
            probe
                .connect("192.0.2.1:9")
                .expect("select the host's non-loopback IPv4 route");
            let SocketAddr::V4(address) = probe.local_addr().expect("read route probe address")
            else {
                panic!("IPv4 route probe returned an IPv6 address");
            };
            let address = *address.ip();
            assert!(!address.is_unspecified());
            assert!(!address.is_loopback());
            assert!(!address.is_multicast());
            assert_ne!(address, Ipv4Addr::BROADCAST);
            address
        }

        fn available_port(address: Ipv4Addr) -> u16 {
            let listener = TcpListener::bind(SocketAddrV4::new(address, 0))
                .expect("reserve an available TCP port");
            let port = listener
                .local_addr()
                .expect("read reserved TCP port")
                .port();
            drop(listener);
            port
        }

        fn identity(material: &IdentityMaterial) -> ResolvedRemoteMtlsIdentityFiles {
            ResolvedRemoteMtlsIdentityFiles::try_new(
                material.certificate.clone(),
                material.private_key.clone(),
            )
            .expect("resolved generated identity files")
        }

        fn schema(marker: u8) -> SchemaRef {
            SchemaRef::try_new([marker; 16], 1, Digest32::from_bytes([marker; 32]))
                .expect("test schema")
        }

        fn binding_spec(marker: u8) -> RequestResponseBindingSpec {
            RequestResponseBindingSpec::try_new(
                BindingId::from_bytes([marker; 16]),
                None,
                SUBMIT_ROUTE,
                schema(marker.wrapping_add(0x40)),
                schema(marker.wrapping_add(0x60)),
                IngressLimits::try_new(4, 16_384, 4_096, 4_096, Duration::from_secs(2))
                    .expect("test ingress limits"),
            )
            .expect("test binding spec")
        }

        async fn install_counting_binding(
            service: &mut FabricService,
        ) -> (PortBinding, Arc<AtomicUsize>, JoinHandle<()>) {
            let installed = service
                .install_request_response_binding(binding_spec(0x41))
                .await
                .expect("install S0 test binding");
            let (binding, mut requests): (PortBinding, RequestReceiver) = installed.into_parts();
            let callbacks = Arc::new(AtomicUsize::new(0));
            let handler_callbacks = Arc::clone(&callbacks);
            let handler = tokio::spawn(async move {
                while let Some(request) = requests.recv().await {
                    handler_callbacks.fetch_add(1, Ordering::SeqCst);
                    let body = request.body().to_vec();
                    request
                        .respond(HandlerResponse::Ok(body))
                        .expect("test response receiver remains live");
                }
            });
            (binding, callbacks, handler)
        }

        async fn expect_echo(
            service: &FabricService,
            binding: &PortBinding,
            request_marker: u8,
            body: &[u8],
        ) {
            let response = service
                .request(
                    binding,
                    RequestId::try_from_bytes([request_marker; 16]).expect("request id"),
                    body.to_vec(),
                    REQUEST_TIMEOUT,
                )
                .await
                .expect("admitted exact route must respond");
            assert_eq!(response.status(), ResponseStatus::Ok);
            assert_eq!(response.body(), body);
        }

        fn assert_port_is_owned(address: SocketAddrV4, owner: &str) {
            let error =
                TcpListener::bind(address).expect_err("live session must retain its listener port");
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::AddrInUse,
                "{owner} listener {address} failed for an unexpected reason"
            );
        }

        async fn assert_port_rebinds(address: SocketAddrV4) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            loop {
                match TcpListener::bind(address) {
                    Ok(listener) => {
                        drop(listener);
                        return;
                    }
                    Err(_) if tokio::time::Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    Err(error) => panic!("listener {address} was not released: {error}"),
                }
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn prepared_proxy_listener_owns_independent_tls_s1_without_mutating_s0() {
            let ubuntu_listener_principal = PrincipalRef::from_bytes([0x71; 16]);
            let mac_client_principal = PrincipalRef::from_bytes([0x72; 16]);
            let remote_ip = actual_non_loopback_ipv4();
            let s0_socket =
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, available_port(Ipv4Addr::LOCALHOST));
            let s1_socket = SocketAddrV4::new(remote_ip, available_port(remote_ip));
            let s0_endpoint =
                SessionEndpoint::try_new(format!("tcp/{s0_socket}")).expect("S0 loopback endpoint");
            let s1_endpoint =
                RemoteTlsEndpoint::try_new(format!("tls/{s1_socket}")).expect("S1 TLS endpoint");

            let directory = TestDirectory::new();
            let pki = ListenerPki::generate(
                directory.path(),
                remote_ip,
                &restricted_runtime_apply_peer_certificate_common_name_v1(
                    ubuntu_listener_principal,
                ),
            );

            let mut s0 = FabricService::start(
                FabricServiceConfig::try_peer(vec![s0_endpoint], Vec::new())
                    .expect("independent S0 loopback config"),
            )
            .await
            .expect("open independent S0 loopback owner");
            let s0_epoch = s0.session_epoch();
            let (s0_submit, s0_callbacks, s0_handler) = install_counting_binding(&mut s0).await;
            expect_echo(&s0, &s0_submit, 0x61, b"s0-before-s1").await;
            assert_eq!(s0_callbacks.load(Ordering::SeqCst), 1);
            assert_port_is_owned(s0_socket, "S0");

            let proxy_config = FabricServiceConfig::try_remote_agent_proxy_listener_v2(
                s1_endpoint,
                ResolvedRemoteMtlsListenerCredentialFilesV1::try_new(
                    pki.root_ca.clone(),
                    identity(&pki.listener),
                )
                .expect("S1 listener credentials"),
                mac_client_principal,
                SUBMIT_ROUTE,
                CONTROL_ROUTE,
            )
            .expect("TLS-only S1 proxy-listener config");
            let prepared = PreparedRemoteAgentProxyListenerV2::try_prepare(proxy_config)
                .expect("prepare S1 without transport effects");
            let reserved_s1_epoch = prepared.session_epoch();
            let unbound_s1 =
                TcpListener::bind(s1_socket).expect("prepare must not bind the S1 TLS endpoint");
            drop(unbound_s1);
            assert_eq!(s0.session_epoch(), s0_epoch);
            assert_port_is_owned(s0_socket, "S0 after S1 prepare");

            let s1 = prepared
                .start()
                .await
                .expect("production prepared start must bind the S1 TLS listener");
            assert_eq!(s1.session_epoch(), reserved_s1_epoch);
            assert_port_is_owned(s1_socket, "S1 TLS listener");
            assert_port_is_owned(s0_socket, "S0 while S1 is live");
            assert_eq!(s0.session_epoch(), s0_epoch);
            expect_echo(&s0, &s0_submit, 0x62, b"s0-while-s1-live").await;
            assert_eq!(s0_callbacks.load(Ordering::SeqCst), 2);

            s1.shutdown()
                .await
                .expect("consuming S1 shutdown must close its TLS listener");
            assert_port_rebinds(s1_socket).await;
            assert_port_is_owned(s0_socket, "S0 after S1 shutdown");
            assert_eq!(s0.session_epoch(), s0_epoch);
            expect_echo(&s0, &s0_submit, 0x63, b"s0-after-s1").await;
            assert_eq!(s0_callbacks.load(Ordering::SeqCst), 3);

            s0.shutdown().await.expect("shutdown independent S0 owner");
            tokio::time::timeout(Duration::from_secs(2), s0_handler)
                .await
                .expect("S0 handler must stop")
                .expect("S0 handler must join");
            assert_port_rebinds(s0_socket).await;
        }
    }
}
