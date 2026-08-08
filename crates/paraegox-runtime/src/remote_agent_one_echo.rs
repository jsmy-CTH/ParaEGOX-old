//! Runtime-private fakeable owner for exactly one remote Open and one Echo.
//!
//! The Controller-owned Describe source supplies a fresh authenticated PXRA /
//! PXRR proof before each operation. PXCB remains nested proof material and is
//! never accepted as connector configuration. This tranche owns no real
//! connector, filesystem store, retry, reconnect, background task, or UI.

#![forbid(unsafe_code)]

use paraegox_agent_contracts::control::{
    AgentConversationOpenOutcomeV1, AgentConversationControlV1,
};
use paraegox_agent_contracts::{AgentConversationRequestV1, AgentConversationTerminalV1};
use paraegox_kernel::digest::Digest32;
use paraegox_kernel::identity::{PrincipalRef, RuntimeHostId};
use paraegox_runtime_contracts::remote_agent_access::{
    RemoteAgentAccessKindV1, RemoteAgentAccessRequestV1, RemoteAgentAccessResponseV1,
};
use paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentDataPlaneProfileV1;
use paraegox_runtime_contracts::wire::ApplyAuthKeyRef;

use crate::managed_agent_transport::{
    AgentConversationClientPortV1, AgentConversationPortDescriptorV1,
};
use crate::remote_agent_outbox::{
    RemoteAgentDescribeProofBytesV1, RemoteAgentOutboxCommitFailureV1,
    RemoteAgentOutboxCommitV1, RemoteAgentOutboxError, RemoteAgentOutboxMutationErrorV1,
    RemoteAgentOutboxPhaseV1, RemoteAgentOutboxV1,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentDescribePurposeV1 {
    Open,
    Echo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentDescribeWireProofV1 {
    request_wire: Box<[u8]>,
    response_wire: Box<[u8]>,
}

impl RemoteAgentDescribeWireProofV1 {
    pub(crate) fn new(request_wire: Box<[u8]>, response_wire: Box<[u8]>) -> Self {
        Self {
            request_wire,
            response_wire,
        }
    }
}

pub(crate) trait RemoteAgentDescribeSourceV1 {
    fn fresh_describe(
        &mut self,
        purpose: RemoteAgentDescribePurposeV1,
    ) -> Result<RemoteAgentDescribeWireProofV1, RemoteAgentDescribeSourceErrorV1>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentDescribeSourceErrorV1;

pub(crate) trait RemoteAgentAccessSignatureVerifierV1 {
    fn verify_controller(
        &mut self,
        principal: PrincipalRef,
        key: ApplyAuthKeyRef,
        key_fingerprint: Digest32,
        transcript: &[u8],
        signature: &[u8],
    ) -> bool;

    fn verify_runtime(
        &mut self,
        principal: PrincipalRef,
        key: ApplyAuthKeyRef,
        key_fingerprint: Digest32,
        transcript: &[u8],
        signature: &[u8],
    ) -> bool;
}

pub(crate) trait RemoteAgentOnceTransportV1 {
    fn open_once(
        &mut self,
        binding: &RemoteAgentDataPlaneBindingV1,
        request: &AgentConversationControlV1,
    ) -> Result<AgentConversationOpenOutcomeV1, RemoteAgentOnceTransportErrorV1>;

    fn echo_once(
        &mut self,
        binding: &RemoteAgentDataPlaneBindingV1,
        request: &AgentConversationRequestV1,
    ) -> Result<AgentConversationTerminalV1, RemoteAgentOnceTransportErrorV1>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOnceTransportErrorV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOneEchoScopeV1 {
    target: RuntimeHostId,
    runtime_store_instance_id: [u8; 32],
    runtime_host_epoch: u64,
    expected_pxau_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    profile_digest: Digest32,
    mac_agent_client_principal: PrincipalRef,
}

impl RemoteAgentOneEchoScopeV1 {
    pub(crate) fn try_new(
        target: RuntimeHostId,
        runtime_store_instance_id: [u8; 32],
        runtime_host_epoch: u64,
        expected_pxau_digest: Digest32,
        expected_active_pxst_digest: Digest32,
        profile_digest: Digest32,
        mac_agent_client_principal: PrincipalRef,
    ) -> Result<Self, RemoteAgentOneEchoErrorV1> {
        if bytes_are_zero(target.as_bytes())
            || bytes_are_zero(&runtime_store_instance_id)
            || runtime_host_epoch == 0
            || digest_is_zero(expected_pxau_digest)
            || digest_is_zero(expected_active_pxst_digest)
            || digest_is_zero(profile_digest)
            || bytes_are_zero(mac_agent_client_principal.as_bytes())
        {
            return Err(RemoteAgentOneEchoErrorV1::InvalidScope);
        }
        Ok(Self {
            target,
            runtime_store_instance_id,
            runtime_host_epoch,
            expected_pxau_digest,
            expected_active_pxst_digest,
            profile_digest,
            mac_agent_client_principal,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteAgentDataPlaneBindingV1 {
    profile: RemoteAgentDataPlaneProfileV1,
    port: AgentConversationClientPortV1,
    fabric_generation: u64,
    agent_generation: u64,
    access_generation: u64,
    proof: RemoteAgentDescribeProofBytesV1,
}

impl RemoteAgentDataPlaneBindingV1 {
    pub(crate) const fn profile(&self) -> &RemoteAgentDataPlaneProfileV1 {
        &self.profile
    }

    pub(crate) const fn port(&self) -> &AgentConversationClientPortV1 {
        &self.port
    }

    pub(crate) const fn generations(&self) -> (u64, u64, u64) {
        (
            self.fabric_generation,
            self.agent_generation,
            self.access_generation,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentOneEchoOutcomeV1 {
    OpenNotAdmitted(AgentConversationOpenOutcomeV1),
    EchoTerminal(AgentConversationTerminalV1),
}

pub(crate) fn run_remote_agent_one_echo_v1<Describe, Verify, Transport, Commit>(
    scope: RemoteAgentOneEchoScopeV1,
    outbox: &mut RemoteAgentOutboxV1,
    describe: &mut Describe,
    verifier: &mut Verify,
    transport: &mut Transport,
    commit: &mut Commit,
) -> Result<RemoteAgentOneEchoOutcomeV1, RemoteAgentOneEchoErrorV1>
where
    Describe: RemoteAgentDescribeSourceV1,
    Verify: RemoteAgentAccessSignatureVerifierV1,
    Transport: RemoteAgentOnceTransportV1,
    Commit: RemoteAgentOutboxCommitV1,
{
    loop {
        match outbox.phase().clone() {
            RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent => {
                let binding = fresh_binding(
                    scope,
                    RemoteAgentDescribePurposeV1::Open,
                    None,
                    describe,
                    verifier,
                )?;
                let proof = binding.proof.clone();
                outbox.claim_open(proof, commit)?;
                let outcome = transport
                    .open_once(&binding, outbox.open_request())
                    .map_err(|_| RemoteAgentOneEchoErrorV1::ReconcileRequired)?;
                outbox.commit_open_result(outcome, commit)?;
            }
            RemoteAgentOutboxPhaseV1::OpenUncertain(_) => {
                return Err(RemoteAgentOneEchoErrorV1::ReconcileRequired);
            }
            RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
                open_outcome: AgentConversationOpenOutcomeV1::Opened,
                open_proof,
            } => {
                let binding = fresh_binding(
                    scope,
                    RemoteAgentDescribePurposeV1::Echo,
                    Some(&open_proof),
                    describe,
                    verifier,
                )?;
                let proof = binding.proof.clone();
                outbox.claim_echo(proof, commit)?;
                let terminal = transport
                    .echo_once(&binding, outbox.echo_request())
                    .map_err(|_| RemoteAgentOneEchoErrorV1::ReconcileRequired)?;
                if !terminal.correlates(outbox.echo_request()) {
                    return Err(RemoteAgentOneEchoErrorV1::TerminalCorrelationMismatch);
                }
                outbox.commit_terminal(terminal, commit)?;
            }
            RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent { .. } => {
                return Err(RemoteAgentOneEchoErrorV1::InvalidOutboxState);
            }
            RemoteAgentOutboxPhaseV1::OpenTerminal(outcome) => {
                return Ok(RemoteAgentOneEchoOutcomeV1::OpenNotAdmitted(outcome));
            }
            RemoteAgentOutboxPhaseV1::EchoUncertain(_) => {
                return Err(RemoteAgentOneEchoErrorV1::ReconcileRequired);
            }
            RemoteAgentOutboxPhaseV1::Terminal(terminal) => {
                return Ok(RemoteAgentOneEchoOutcomeV1::EchoTerminal(terminal));
            }
        }
    }
}

fn fresh_binding<Describe, Verify>(
    scope: RemoteAgentOneEchoScopeV1,
    purpose: RemoteAgentDescribePurposeV1,
    previous: Option<&RemoteAgentDescribeProofBytesV1>,
    describe: &mut Describe,
    verifier: &mut Verify,
) -> Result<RemoteAgentDataPlaneBindingV1, RemoteAgentOneEchoErrorV1>
where
    Describe: RemoteAgentDescribeSourceV1,
    Verify: RemoteAgentAccessSignatureVerifierV1,
{
    let wire = describe
        .fresh_describe(purpose)
        .map_err(|_| RemoteAgentOneEchoErrorV1::DescribeUnavailable)?;
    let proof = RemoteAgentDescribeProofBytesV1::try_new(
        &wire.request_wire,
        &wire.response_wire,
    )?;
    let request = RemoteAgentAccessRequestV1::decode(proof.request_wire())
        .map_err(|_| RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?;
    let response = RemoteAgentAccessResponseV1::decode(proof.response_wire())
        .map_err(|_| RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?;
    if request.kind() != RemoteAgentAccessKindV1::DescribeRemoteAccess
        || request.target() != scope.target
        || request.expected_runtime_store_instance_id() != scope.runtime_store_instance_id
        || request.expected_runtime_host_epoch() != scope.runtime_host_epoch
        || request.expected_pxau_digest() != scope.expected_pxau_digest
        || request.expected_active_pxst_digest() != scope.expected_active_pxst_digest
        || request.profile_digest() != scope.profile_digest
        || request.intended_mac_agent_client() != scope.mac_agent_client_principal
    {
        return Err(RemoteAgentOneEchoErrorV1::DescribeScopeMismatch);
    }
    request
        .verify_controller_request(request.carrier(), |principal, key, fingerprint, bytes, sig| {
            verifier.verify_controller(principal, key, fingerprint, bytes, sig)
        })
        .map_err(|_| RemoteAgentOneEchoErrorV1::DescribeAuthenticationFailed)?;
    response
        .verify_runtime_describe_response(
            &request,
            request.carrier(),
            |principal, key, fingerprint, bytes, sig| {
                verifier.verify_runtime(principal, key, fingerprint, bytes, sig)
            },
        )
        .map_err(|_| RemoteAgentOneEchoErrorV1::DescribeAuthenticationFailed)?;
    if previous.is_some_and(|prior| {
        RemoteAgentAccessRequestV1::decode(prior.request_wire()).is_ok_and(|old| {
            old.request_id() == request.request_id()
                || old.authentication().claim().nonce()
                    == request.authentication().claim().nonce()
        })
    }) {
        return Err(RemoteAgentOneEchoErrorV1::DescribeNotFresh);
    }
    let profile = response
        .profile()
        .ok_or(RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?
        .clone();
    if profile.mac_agent_client_principal() != scope.mac_agent_client_principal {
        return Err(RemoteAgentOneEchoErrorV1::DescribeScopeMismatch);
    }
    let descriptor = response
        .descriptor()
        .ok_or(RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?;
    let port = AgentConversationPortDescriptorV1::decode(descriptor)
        .map_err(|_| RemoteAgentOneEchoErrorV1::InvalidPortDescriptor)?
        .into_client_port();
    let fabric_generation = response
        .fabric_generation()
        .ok_or(RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?
        .value();
    let agent_generation = response
        .agent_generation()
        .ok_or(RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?
        .value();
    let access_generation = response
        .access_generation()
        .ok_or(RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?
        .value();
    Ok(RemoteAgentDataPlaneBindingV1 {
        profile,
        port,
        fabric_generation,
        agent_generation,
        access_generation,
        proof,
    })
}

const fn digest_is_zero(value: Digest32) -> bool {
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

#[derive(Debug)]
pub(crate) enum RemoteAgentOneEchoErrorV1 {
    InvalidScope,
    InvalidOutboxState,
    DescribeUnavailable,
    InvalidDescribeProof,
    DescribeScopeMismatch,
    DescribeAuthenticationFailed,
    DescribeNotFresh,
    InvalidPortDescriptor,
    TerminalCorrelationMismatch,
    ReconcileRequired,
    Outbox(RemoteAgentOutboxError),
    Commit(RemoteAgentOutboxCommitFailureV1),
}

impl From<RemoteAgentOutboxError> for RemoteAgentOneEchoErrorV1 {
    fn from(value: RemoteAgentOutboxError) -> Self {
        Self::Outbox(value)
    }
}

impl From<RemoteAgentOutboxMutationErrorV1> for RemoteAgentOneEchoErrorV1 {
    fn from(value: RemoteAgentOutboxMutationErrorV1) -> Self {
        match value {
            RemoteAgentOutboxMutationErrorV1::State(error) => Self::Outbox(error),
            RemoteAgentOutboxMutationErrorV1::Commit(error) => Self::Commit(error),
        }
    }
}
