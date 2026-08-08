//! Runtime-private fakeable owner for exactly one remote Open and one Echo.
//!
//! The Controller-owned Describe source supplies a fresh authenticated PXRA /
//! PXRR proof before each operation. PXCB remains nested proof material and is
//! never accepted as connector configuration. This tranche owns no real
//! connector, filesystem store, retry, reconnect, background task, or UI.

#![forbid(unsafe_code)]

use paraegox_agent_contracts::control::{
    AgentConversationControlV1, AgentConversationOpenOutcomeV1,
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
    RemoteAgentDescribeProofBytesV1, RemoteAgentOutboxCommitFailureV1, RemoteAgentOutboxCommitV1,
    RemoteAgentOutboxError, RemoteAgentOutboxMutationErrorV1, RemoteAgentOutboxPhaseV1,
    RemoteAgentOutboxV1,
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
    let proof = RemoteAgentDescribeProofBytesV1::try_new(&wire.request_wire, &wire.response_wire)?;
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
        .verify_controller_request(
            request.carrier(),
            |principal, key, fingerprint, bytes, sig| {
                verifier.verify_controller(principal, key, fingerprint, bytes, sig)
            },
        )
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
                || old.authentication().claim().nonce() == request.authentication().claim().nonce()
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

#[derive(Debug, Eq, PartialEq)]
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use ed25519_dalek::{Signature, Signer, SigningKey};
    use paraegox_agent_contracts::control::AgentConversationOpenOutcomeV1;
    use paraegox_agent_contracts::{
        AgentConversationDeckRunId, AgentConversationRequestId, AgentConversationSessionId,
        AgentConversationTurnId,
    };
    use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
    use paraegox_runtime_contracts::remote_agent_access::{
        RemoteAgentAccessRequestDraftV1, RemoteAgentAccessRequestFieldsV1,
        RemoteAgentAccessRequestIdV1, RemoteAgentAccessResponseAuthClaimV1,
        RemoteAgentAccessResponseDraftV1,
    };
    use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyRequestAuthClaim};

    use super::*;

    const ACCESS_GOLDEN: &str =
        include_str!("../../../tests/fixtures/wire/t2_remote_agent_access_v1.json");
    const PORT_GOLDEN: &str =
        include_str!("../tests/fixtures/agent_conversation_port_descriptor_v1.hex");
    const CONTROLLER_SEED: [u8; 32] = [0x22; 32];
    const RUNTIME_SEED: [u8; 32] = [0x44; 32];

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Commit(u16),
        Describe(RemoteAgentDescribePurposeV1),
        SendOpen,
        SendEcho,
    }

    type Events = Rc<RefCell<Vec<Event>>>;

    struct FakeCommit {
        events: Events,
        records: Vec<Vec<u8>>,
        calls: usize,
        fail_on_call: Option<usize>,
    }

    impl FakeCommit {
        fn new(events: Events) -> Self {
            Self {
                events,
                records: Vec::new(),
                calls: 0,
                fail_on_call: None,
            }
        }

        fn fail_on(events: Events, call: usize) -> Self {
            Self {
                fail_on_call: Some(call),
                ..Self::new(events)
            }
        }
    }

    impl RemoteAgentOutboxCommitV1 for FakeCommit {
        fn commit_record(&mut self, record: &[u8]) -> Result<(), RemoteAgentOutboxCommitFailureV1> {
            self.calls += 1;
            if self.fail_on_call == Some(self.calls) {
                return Err(RemoteAgentOutboxCommitFailureV1);
            }
            let kind = u16::from_be_bytes(record[8..10].try_into().expect("record kind"));
            self.events.borrow_mut().push(Event::Commit(kind));
            self.records.push(record.to_vec());
            Ok(())
        }
    }

    struct FakeDescribe {
        events: Events,
        proofs: VecDeque<RemoteAgentDescribeWireProofV1>,
        calls: usize,
    }

    impl FakeDescribe {
        fn new(events: Events, proofs: Vec<RemoteAgentDescribeWireProofV1>) -> Self {
            Self {
                events,
                proofs: proofs.into(),
                calls: 0,
            }
        }
    }

    impl RemoteAgentDescribeSourceV1 for FakeDescribe {
        fn fresh_describe(
            &mut self,
            purpose: RemoteAgentDescribePurposeV1,
        ) -> Result<RemoteAgentDescribeWireProofV1, RemoteAgentDescribeSourceErrorV1> {
            self.calls += 1;
            self.events.borrow_mut().push(Event::Describe(purpose));
            self.proofs
                .pop_front()
                .ok_or(RemoteAgentDescribeSourceErrorV1)
        }
    }

    struct TestVerifier {
        controller_principal: PrincipalRef,
        controller_key_ref: ApplyAuthKeyRef,
        controller_fingerprint: Digest32,
        runtime_principal: PrincipalRef,
        runtime_key_ref: ApplyAuthKeyRef,
        runtime_fingerprint: Digest32,
        controller_calls: usize,
        runtime_calls: usize,
    }

    impl TestVerifier {
        fn for_request(request: &RemoteAgentAccessRequestV1) -> Self {
            Self {
                controller_principal: request.carrier().controller_principal(),
                controller_key_ref: request.carrier().controller_request_key(),
                controller_fingerprint: request.carrier().controller_request_key_fingerprint(),
                runtime_principal: request.carrier().runtime_principal(),
                runtime_key_ref: request.carrier().runtime_response_key(),
                runtime_fingerprint: request.carrier().runtime_response_key_fingerprint(),
                controller_calls: 0,
                runtime_calls: 0,
            }
        }

        fn verify(seed: &[u8; 32], transcript: &[u8], signature: &[u8]) -> bool {
            let Ok(signature) = Signature::from_slice(signature) else {
                return false;
            };
            SigningKey::from_bytes(seed)
                .verifying_key()
                .verify_strict(transcript, &signature)
                .is_ok()
        }
    }

    impl RemoteAgentAccessSignatureVerifierV1 for TestVerifier {
        fn verify_controller(
            &mut self,
            principal: PrincipalRef,
            key: ApplyAuthKeyRef,
            key_fingerprint: Digest32,
            transcript: &[u8],
            signature: &[u8],
        ) -> bool {
            self.controller_calls += 1;
            principal == self.controller_principal
                && key == self.controller_key_ref
                && key_fingerprint == self.controller_fingerprint
                && Self::verify(&CONTROLLER_SEED, transcript, signature)
        }

        fn verify_runtime(
            &mut self,
            principal: PrincipalRef,
            key: ApplyAuthKeyRef,
            key_fingerprint: Digest32,
            transcript: &[u8],
            signature: &[u8],
        ) -> bool {
            self.runtime_calls += 1;
            principal == self.runtime_principal
                && key == self.runtime_key_ref
                && key_fingerprint == self.runtime_fingerprint
                && Self::verify(&RUNTIME_SEED, transcript, signature)
        }
    }

    struct FakeTransport {
        events: Events,
        open_outcome: AgentConversationOpenOutcomeV1,
        open_calls: usize,
        echo_calls: usize,
        observed_generations: Vec<(u64, u64, u64)>,
        observed_profile_digests: Vec<Digest32>,
        observed_binding_facts: Vec<[([u8; 16], u64); 2]>,
        mismatched_terminal: bool,
    }

    impl FakeTransport {
        fn new(events: Events) -> Self {
            Self {
                events,
                open_outcome: AgentConversationOpenOutcomeV1::Opened,
                open_calls: 0,
                echo_calls: 0,
                observed_generations: Vec::new(),
                observed_profile_digests: Vec::new(),
                observed_binding_facts: Vec::new(),
                mismatched_terminal: false,
            }
        }
    }

    impl RemoteAgentOnceTransportV1 for FakeTransport {
        fn open_once(
            &mut self,
            binding: &RemoteAgentDataPlaneBindingV1,
            _request: &AgentConversationControlV1,
        ) -> Result<AgentConversationOpenOutcomeV1, RemoteAgentOnceTransportErrorV1> {
            self.open_calls += 1;
            self.observed_generations.push(binding.generations());
            self.observed_profile_digests
                .push(binding.profile().profile_digest());
            self.observed_binding_facts
                .push(binding.port().binding_facts());
            self.events.borrow_mut().push(Event::SendOpen);
            Ok(self.open_outcome)
        }

        fn echo_once(
            &mut self,
            binding: &RemoteAgentDataPlaneBindingV1,
            request: &AgentConversationRequestV1,
        ) -> Result<AgentConversationTerminalV1, RemoteAgentOnceTransportErrorV1> {
            self.echo_calls += 1;
            self.observed_generations.push(binding.generations());
            self.observed_profile_digests
                .push(binding.profile().profile_digest());
            self.observed_binding_facts
                .push(binding.port().binding_facts());
            self.events.borrow_mut().push(Event::SendEcho);
            if self.mismatched_terminal {
                return AgentConversationTerminalV1::try_success(&other_echo_request(), "Echo")
                    .map_err(|_| RemoteAgentOnceTransportErrorV1);
            }
            AgentConversationTerminalV1::try_success(request, "Echo")
                .map_err(|_| RemoteAgentOnceTransportErrorV1)
        }
    }

    fn echo_request() -> AgentConversationRequestV1 {
        AgentConversationRequestV1::try_new(
            AgentConversationDeckRunId::try_from_bytes([0x31; 16]).expect("DeckRun"),
            AgentConversationSessionId::try_from_bytes([0x32; 16]).expect("Session"),
            AgentConversationTurnId::try_from_bytes([0x33; 16]).expect("turn"),
            AgentConversationRequestId::try_from_bytes([0x34; 16]).expect("request"),
            5_000_000_000,
            "Echo",
        )
        .expect("Echo request")
    }

    fn other_echo_request() -> AgentConversationRequestV1 {
        AgentConversationRequestV1::try_new(
            AgentConversationDeckRunId::try_from_bytes([0x41; 16]).expect("DeckRun"),
            AgentConversationSessionId::try_from_bytes([0x42; 16]).expect("Session"),
            AgentConversationTurnId::try_from_bytes([0x43; 16]).expect("turn"),
            AgentConversationRequestId::try_from_bytes([0x44; 16]).expect("request"),
            5_000_000_000,
            "Echo",
        )
        .expect("other Echo request")
    }

    fn golden_request() -> RemoteAgentAccessRequestV1 {
        RemoteAgentAccessRequestV1::decode(&fixture_hex("pxra_describe_hex"))
            .expect("golden PXRA Describe")
    }

    fn golden_profile() -> RemoteAgentDataPlaneProfileV1 {
        RemoteAgentAccessResponseV1::decode(&fixture_hex("pxrr_describe_hex"))
            .expect("golden PXRR Describe")
            .profile()
            .expect("PXAD")
            .clone()
    }

    fn next_request(base: &RemoteAgentAccessRequestV1) -> RemoteAgentAccessRequestV1 {
        let old_claim = base.authentication().claim();
        let claim = ApplyRequestAuthClaim::try_new(
            old_claim.principal(),
            old_claim.key(),
            old_claim.algorithm(),
            old_claim.algorithm_version(),
            b"t2-d0-second-fresh-describe",
        )
        .expect("second auth claim");
        let draft = RemoteAgentAccessRequestDraftV1::try_describe_remote_access(
            RemoteAgentAccessRequestFieldsV1 {
                request_id: RemoteAgentAccessRequestIdV1::try_from_bytes([0x95; 16])
                    .expect("request id"),
                carrier: base.carrier().clone(),
                target: base.target(),
                expected_runtime_store_instance_id: base.expected_runtime_store_instance_id(),
                expected_runtime_host_epoch: base.expected_runtime_host_epoch(),
                auth_claim: claim,
            },
            base.expected_pxau_digest(),
            base.expected_active_pxst_digest(),
            base.profile_digest(),
            base.intended_mac_agent_client(),
        )
        .expect("second PXRA draft");
        let signature = SigningKey::from_bytes(&CONTROLLER_SEED)
            .sign(
                draft
                    .signing_transcript()
                    .expect("PXRA transcript")
                    .as_bytes(),
            )
            .to_bytes();
        draft.finalize(&signature).expect("second PXRA")
    }

    fn proof_for(
        request: &RemoteAgentAccessRequestV1,
        generations: (u64, u64, u64),
        descriptor: &[u8],
    ) -> RemoteAgentDescribeWireProofV1 {
        let controller_key = SigningKey::from_bytes(&CONTROLLER_SEED).verifying_key();
        let authenticated = request
            .verify_controller_request(request.carrier(), |_, _, _, transcript, signature| {
                let Ok(signature) = Signature::from_slice(signature) else {
                    return false;
                };
                controller_key.verify_strict(transcript, &signature).is_ok()
            })
            .expect("Controller-authenticated PXRA");
        let auth = RemoteAgentAccessResponseAuthClaimV1::try_new(
            request.carrier(),
            request.carrier().runtime_response_key(),
            ApplyAuthAlgorithm::try_new(1).expect("Ed25519"),
            1,
        )
        .expect("PXRR claim");
        let draft = RemoteAgentAccessResponseDraftV1::try_describe_remote_access(
            authenticated,
            golden_profile(),
            descriptor,
            generation(generations.0),
            generation(generations.1),
            generation(generations.2),
            auth,
        )
        .expect("PXRR draft");
        let signature = SigningKey::from_bytes(&RUNTIME_SEED)
            .sign(
                draft
                    .signing_transcript()
                    .expect("PXRR transcript")
                    .as_bytes(),
            )
            .to_bytes();
        let response = draft.finalize(&signature).expect("PXRR");
        RemoteAgentDescribeWireProofV1::new(
            request.canonical_wire().into(),
            response.canonical_wire().into(),
        )
    }

    fn valid_proofs() -> (
        RemoteAgentAccessRequestV1,
        RemoteAgentDescribeWireProofV1,
        RemoteAgentDescribeWireProofV1,
    ) {
        let first = golden_request();
        let second = next_request(&first);
        let descriptor = decode_hex(PORT_GOLDEN.trim());
        let open = proof_for(&first, (9, 10, 11), &descriptor);
        let echo = proof_for(&second, (12, 13, 14), &descriptor);
        (first, open, echo)
    }

    fn scope(request: &RemoteAgentAccessRequestV1) -> RemoteAgentOneEchoScopeV1 {
        RemoteAgentOneEchoScopeV1::try_new(
            request.target(),
            request.expected_runtime_store_instance_id(),
            request.expected_runtime_host_epoch(),
            request.expected_pxau_digest(),
            request.expected_active_pxst_digest(),
            request.profile_digest(),
            request.intended_mac_agent_client(),
        )
        .expect("scope")
    }

    fn prepared(events: Events, commit: &mut FakeCommit) -> RemoteAgentOutboxV1 {
        let outbox = RemoteAgentOutboxV1::try_prepare([0x51; 16], echo_request(), commit)
            .expect("prepared outbox");
        assert_eq!(
            events.borrow().as_slice(),
            &[Event::Commit(1)],
            "prepare has no source, transport, or entropy callback"
        );
        outbox
    }

    fn generation(value: u64) -> ManagedServiceGeneration {
        ManagedServiceGeneration::try_new(value).expect("generation")
    }

    fn fixture_hex(key: &str) -> Vec<u8> {
        let marker = format!("\"{key}\": \"");
        let start = ACCESS_GOLDEN.find(&marker).expect("fixture key") + marker.len();
        let end = start + ACCESS_GOLDEN[start..].find('"').expect("fixture value end");
        decode_hex(&ACCESS_GOLDEN[start..end])
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
            .collect()
    }

    fn hex_nibble(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("invalid fixture hex"),
        }
    }

    #[test]
    fn prepare_commits_exact_open_and_echo_before_any_external_call() {
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let outbox = prepared(events, &mut commit);
        assert_eq!(commit.records.len(), 1);
        let recovered = RemoteAgentOutboxV1::decode(&commit.records.concat()).expect("recovery");
        assert_eq!(recovered, outbox);
        assert_eq!(recovered.attempt_id(), [0x51; 16]);
        assert_eq!(recovered.echo_request(), &echo_request());
        assert!(matches!(
            recovered.open_request().body(),
            paraegox_agent_contracts::control::AgentConversationControlBodyV1::OpenRequest
        ));
    }

    #[test]
    fn open_and_echo_claims_commit_before_each_send_and_rebind_fresh_generations() {
        let (request, open, echo) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![open, echo]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        let outcome = run_remote_agent_one_echo_v1(
            scope(&request),
            &mut outbox,
            &mut describe,
            &mut verifier,
            &mut transport,
            &mut commit,
        )
        .expect("one Echo");
        assert!(matches!(
            outcome,
            RemoteAgentOneEchoOutcomeV1::EchoTerminal(_)
        ));
        assert_eq!(
            transport.observed_generations,
            vec![(9, 10, 11), (12, 13, 14)]
        );
        assert_eq!(
            transport.observed_profile_digests,
            vec![request.profile_digest(), request.profile_digest()]
        );
        assert_eq!(
            transport.observed_binding_facts,
            vec![
                [([0x31; 16], 3), ([0x32; 16], 4)],
                [([0x31; 16], 3), ([0x32; 16], 4)]
            ]
        );
        assert_eq!(verifier.controller_calls, 2);
        assert_eq!(verifier.runtime_calls, 2);
        assert_eq!(
            events.borrow().as_slice(),
            &[
                Event::Commit(1),
                Event::Describe(RemoteAgentDescribePurposeV1::Open),
                Event::Commit(2),
                Event::SendOpen,
                Event::Commit(3),
                Event::Describe(RemoteAgentDescribePurposeV1::Echo),
                Event::Commit(4),
                Event::SendEcho,
                Event::Commit(5),
            ]
        );
        assert_eq!(
            RemoteAgentOutboxV1::decode(outbox.canonical_wire()).unwrap(),
            outbox
        );
    }

    #[test]
    fn claim_commit_failure_sends_nothing_and_leaves_prepared() {
        let (request, open, _) = valid_proofs();
        let events = Events::default();
        let mut prepare_commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut prepare_commit);
        let mut commit = FakeCommit::fail_on(events.clone(), 1);
        let mut describe = FakeDescribe::new(events.clone(), vec![open]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events);
        assert!(matches!(
            run_remote_agent_one_echo_v1(
                scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::Commit(_))
        ));
        assert_eq!(transport.open_calls, 0);
        assert_eq!(transport.echo_calls, 0);
        assert!(matches!(
            outbox.phase(),
            RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent
        ));
    }

    #[test]
    fn open_uncertain_resume_has_zero_external_calls_and_never_replays() {
        let (request, open, _) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let proof =
            RemoteAgentDescribeProofBytesV1::try_new(&open.request_wire, &open.response_wire)
                .unwrap();
        outbox.claim_open(proof, &mut commit).unwrap();
        let before = events.borrow().clone();
        let mut describe = FakeDescribe::new(events.clone(), Vec::new());
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        assert_eq!(
            run_remote_agent_one_echo_v1(
                scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::ReconcileRequired)
        );
        assert_eq!(*events.borrow(), before);
        assert_eq!(
            (describe.calls, transport.open_calls, transport.echo_calls),
            (0, 0, 0)
        );
        assert_eq!((verifier.controller_calls, verifier.runtime_calls), (0, 0));
    }

    #[test]
    fn echo_uncertain_resume_has_zero_external_calls_and_never_replays() {
        let (request, open, echo) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        outbox
            .claim_open(
                RemoteAgentDescribeProofBytesV1::try_new(&open.request_wire, &open.response_wire)
                    .unwrap(),
                &mut commit,
            )
            .unwrap();
        outbox
            .commit_open_result(AgentConversationOpenOutcomeV1::Opened, &mut commit)
            .unwrap();
        outbox
            .claim_echo(
                RemoteAgentDescribeProofBytesV1::try_new(&echo.request_wire, &echo.response_wire)
                    .unwrap(),
                &mut commit,
            )
            .unwrap();
        let before = events.borrow().clone();
        let mut describe = FakeDescribe::new(events.clone(), Vec::new());
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        assert_eq!(
            run_remote_agent_one_echo_v1(
                scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::ReconcileRequired)
        );
        assert_eq!(*events.borrow(), before);
        assert_eq!(
            (describe.calls, transport.open_calls, transport.echo_calls),
            (0, 0, 0)
        );
        assert_eq!((verifier.controller_calls, verifier.runtime_calls), (0, 0));
    }

    #[test]
    fn terminal_resume_returns_exact_terminal_with_zero_external_calls() {
        let (request, open, echo) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![open, echo]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        let first = run_remote_agent_one_echo_v1(
            scope(&request),
            &mut outbox,
            &mut describe,
            &mut verifier,
            &mut transport,
            &mut commit,
        )
        .unwrap();
        let before = events.borrow().clone();
        let calls = (
            describe.calls,
            verifier.controller_calls,
            verifier.runtime_calls,
            transport.open_calls,
            transport.echo_calls,
            commit.calls,
        );
        let resumed = run_remote_agent_one_echo_v1(
            scope(&request),
            &mut outbox,
            &mut describe,
            &mut verifier,
            &mut transport,
            &mut commit,
        )
        .unwrap();
        assert_eq!(resumed, first);
        assert_eq!(*events.borrow(), before);
        assert_eq!(
            (
                describe.calls,
                verifier.controller_calls,
                verifier.runtime_calls,
                transport.open_calls,
                transport.echo_calls,
                commit.calls,
            ),
            calls
        );
    }

    #[test]
    fn runtime_signature_tamper_and_noncanonical_pxap_fail_before_claim_or_send() {
        let (request, open, _) = valid_proofs();
        let mut bad_signature = open.clone();
        *bad_signature.response_wire.last_mut().unwrap() ^= 1;
        assert_rejected_before_claim(&request, bad_signature);

        let bad_descriptor = proof_for(
            &request,
            (9, 10, 11),
            b"PXAP\0\x01not-a-canonical-two-lane-descriptor",
        );
        assert_rejected_before_claim(&request, bad_descriptor);
    }

    fn assert_rejected_before_claim(
        request: &RemoteAgentAccessRequestV1,
        proof: RemoteAgentDescribeWireProofV1,
    ) {
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![proof]);
        let mut verifier = TestVerifier::for_request(request);
        let mut transport = FakeTransport::new(events.clone());
        assert!(
            run_remote_agent_one_echo_v1(
                scope(request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            )
            .is_err()
        );
        assert_eq!(commit.records.len(), 1);
        assert_eq!((transport.open_calls, transport.echo_calls), (0, 0));
        assert!(matches!(
            outbox.phase(),
            RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent
        ));
    }

    #[test]
    fn repeated_describe_identity_is_not_fresh_and_echo_is_not_claimed() {
        let (request, open, _) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![open.clone(), open]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events);
        assert_eq!(
            run_remote_agent_one_echo_v1(
                scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::DescribeNotFresh)
        );
        assert_eq!((transport.open_calls, transport.echo_calls), (1, 0));
        assert_eq!(commit.records.len(), 3);
        assert!(matches!(
            outbox.phase(),
            RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent { .. }
        ));
    }

    #[test]
    fn mismatched_terminal_is_not_committed_and_leaves_echo_uncertain() {
        let (request, open, echo) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![open, echo]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events);
        transport.mismatched_terminal = true;
        assert_eq!(
            run_remote_agent_one_echo_v1(
                scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::TerminalCorrelationMismatch)
        );
        assert_eq!(commit.records.len(), 4);
        assert!(matches!(
            outbox.phase(),
            RemoteAgentOutboxPhaseV1::EchoUncertain(_)
        ));
    }

    #[test]
    fn nonopened_result_is_terminal_and_never_describes_or_sends_echo() {
        let (request, open, _) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![open]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        transport.open_outcome = AgentConversationOpenOutcomeV1::Existing;
        let first = run_remote_agent_one_echo_v1(
            scope(&request),
            &mut outbox,
            &mut describe,
            &mut verifier,
            &mut transport,
            &mut commit,
        )
        .unwrap();
        assert_eq!(
            first,
            RemoteAgentOneEchoOutcomeV1::OpenNotAdmitted(AgentConversationOpenOutcomeV1::Existing)
        );
        let before = events.borrow().clone();
        assert_eq!(
            run_remote_agent_one_echo_v1(
                scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            )
            .unwrap(),
            first
        );
        assert_eq!(*events.borrow(), before);
        assert_eq!((transport.open_calls, transport.echo_calls), (1, 0));
    }

    #[test]
    fn pxoj_rejects_cross_magic_checksum_and_record_gaps() {
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let outbox = prepared(events, &mut commit);

        let mut cross_magic = outbox.canonical_wire().to_vec();
        cross_magic[..4].copy_from_slice(b"PXRA");
        assert_eq!(
            RemoteAgentOutboxV1::decode(&cross_magic),
            Err(RemoteAgentOutboxError::UnsupportedWire)
        );

        let mut bad_checksum = outbox.canonical_wire().to_vec();
        *bad_checksum.last_mut().unwrap() ^= 1;
        assert_eq!(
            RemoteAgentOutboxV1::decode(&bad_checksum),
            Err(RemoteAgentOutboxError::RecordChecksumMismatch)
        );

        let duplicate = [outbox.canonical_wire(), outbox.canonical_wire()].concat();
        assert_eq!(
            RemoteAgentOutboxV1::decode(&duplicate),
            Err(RemoteAgentOutboxError::RecordGap)
        );

        let (_, open, _) = valid_proofs();
        let mut claimed_commit = FakeCommit::new(Events::default());
        let mut claimed =
            RemoteAgentOutboxV1::try_prepare([0x61; 16], echo_request(), &mut claimed_commit)
                .unwrap();
        claimed
            .claim_open(
                RemoteAgentDescribeProofBytesV1::try_new(&open.request_wire, &open.response_wire)
                    .unwrap(),
                &mut claimed_commit,
            )
            .unwrap();
        let first_length = claimed_commit.records[0].len();
        assert_eq!(
            RemoteAgentOutboxV1::decode(&claimed.canonical_wire()[first_length..]),
            Err(RemoteAgentOutboxError::RecordGap)
        );
    }
}
