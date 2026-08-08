//! Runtime-private fakeable owner for exactly one remote Open and one Echo.
//!
//! The Controller-owned Describe source supplies a challenge-bound signed PXRA /
//! PXRR proof before each operation. PXCB remains durable trusted scope and is
//! never accepted as connector configuration. This tranche owns no real
//! Controller exchange, connector, filesystem store, retry, reconnect,
//! background task, or UI, and therefore makes no discovery-currentness claim.

#![forbid(unsafe_code)]

use paraegox_agent_contracts::control::{
    AgentConversationControlV1, AgentConversationOpenOutcomeV1,
};
use paraegox_agent_contracts::{AgentConversationRequestV1, AgentConversationTerminalV1};
use paraegox_runtime_contracts::remote_agent_access::RemoteAgentAccessResponseV1;
use paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentDataPlaneProfileV1;

use crate::managed_agent_transport::{
    AgentConversationClientPortV1, AgentConversationPortDescriptorV1,
};
use crate::remote_agent_outbox::{
    verify_remote_agent_describe_proof_v1, RemoteAgentAccessSignatureVerifierV1,
    RemoteAgentDescribeChallengeV1, RemoteAgentDescribeProofBytesV1, RemoteAgentOneEchoScopeV1,
    RemoteAgentOutboxCommitFailureV1, RemoteAgentOutboxCommitV1, RemoteAgentOutboxError,
    RemoteAgentOutboxMutationErrorV1, RemoteAgentOutboxPhaseV1, RemoteAgentOutboxV1,
    RemoteAgentVerifiedDescribeProofV1,
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
        challenge: &RemoteAgentDescribeChallengeV1,
    ) -> Result<RemoteAgentDescribeWireProofV1, RemoteAgentDescribeSourceErrorV1>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentDescribeSourceErrorV1;

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

#[derive(Clone, Debug)]
pub(crate) struct RemoteAgentDataPlaneBindingV1 {
    profile: RemoteAgentDataPlaneProfileV1,
    port: AgentConversationClientPortV1,
    fabric_generation: u64,
    agent_generation: u64,
    access_generation: u64,
}

#[derive(Debug)]
struct RemoteAgentVerifiedDataPlaneBindingV1 {
    binding: RemoteAgentDataPlaneBindingV1,
    proof: RemoteAgentVerifiedDescribeProofV1,
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
    expected_scope: &RemoteAgentOneEchoScopeV1,
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
    if !outbox.scope_matches(expected_scope) {
        return Err(RemoteAgentOneEchoErrorV1::ScopeMismatch);
    }
    loop {
        match outbox.phase().clone() {
            RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent => {
                let verified = challenge_bound_binding(
                    expected_scope,
                    RemoteAgentDescribePurposeV1::Open,
                    expected_scope.open_challenge(),
                    describe,
                    verifier,
                )?;
                let RemoteAgentVerifiedDataPlaneBindingV1 { binding, proof } = verified;
                let action = outbox.claim_open(expected_scope, proof, commit)?;
                let exchange = action
                    .exchange(|request| transport.open_once(&binding, request))
                    .map_err(|_| RemoteAgentOneEchoErrorV1::ReconcileRequired)?;
                outbox.commit_open_result(exchange, commit)?;
            }
            RemoteAgentOutboxPhaseV1::OpenUncertain { .. } => {
                return Err(RemoteAgentOneEchoErrorV1::ReconcileRequired);
            }
            RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
                open_outcome: AgentConversationOpenOutcomeV1::Opened,
                open_proof,
            } => {
                let _open = verify_binding(
                    expected_scope,
                    expected_scope.open_challenge(),
                    open_proof,
                    verifier,
                )?;
                let verified = challenge_bound_binding(
                    expected_scope,
                    RemoteAgentDescribePurposeV1::Echo,
                    expected_scope.echo_challenge(),
                    describe,
                    verifier,
                )?;
                let RemoteAgentVerifiedDataPlaneBindingV1 { binding, proof } = verified;
                let action = outbox.claim_echo(expected_scope, proof, commit)?;
                let exchange = action
                    .exchange(|request| transport.echo_once(&binding, request))
                    .map_err(|_| RemoteAgentOneEchoErrorV1::ReconcileRequired)?;
                if !exchange.is_correlated() {
                    return Err(RemoteAgentOneEchoErrorV1::TerminalCorrelationMismatch);
                }
                outbox.commit_terminal(exchange, commit)?;
            }
            RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent { .. } => {
                return Err(RemoteAgentOneEchoErrorV1::InvalidOutboxState);
            }
            RemoteAgentOutboxPhaseV1::OpenTerminal {
                open_outcome,
                open_proof,
            } => {
                let _open = verify_binding(
                    expected_scope,
                    expected_scope.open_challenge(),
                    open_proof,
                    verifier,
                )?;
                return Ok(RemoteAgentOneEchoOutcomeV1::OpenNotAdmitted(open_outcome));
            }
            RemoteAgentOutboxPhaseV1::EchoUncertain { .. } => {
                return Err(RemoteAgentOneEchoErrorV1::ReconcileRequired);
            }
            RemoteAgentOutboxPhaseV1::Terminal {
                open_proof,
                echo_proof,
                terminal,
            } => {
                let _open = verify_binding(
                    expected_scope,
                    expected_scope.open_challenge(),
                    open_proof,
                    verifier,
                )?;
                let _echo = verify_binding(
                    expected_scope,
                    expected_scope.echo_challenge(),
                    echo_proof,
                    verifier,
                )?;
                if !terminal.correlates(expected_scope.echo_request()) {
                    return Err(RemoteAgentOneEchoErrorV1::TerminalCorrelationMismatch);
                }
                return Ok(RemoteAgentOneEchoOutcomeV1::EchoTerminal(terminal));
            }
        }
    }
}

fn challenge_bound_binding<Describe, Verify>(
    scope: &RemoteAgentOneEchoScopeV1,
    purpose: RemoteAgentDescribePurposeV1,
    challenge: &RemoteAgentDescribeChallengeV1,
    describe: &mut Describe,
    verifier: &mut Verify,
) -> Result<RemoteAgentVerifiedDataPlaneBindingV1, RemoteAgentOneEchoErrorV1>
where
    Describe: RemoteAgentDescribeSourceV1,
    Verify: RemoteAgentAccessSignatureVerifierV1,
{
    let wire = describe
        .fresh_describe(purpose, challenge)
        .map_err(|_| RemoteAgentOneEchoErrorV1::DescribeUnavailable)?;
    let proof = RemoteAgentDescribeProofBytesV1::try_new(&wire.request_wire, &wire.response_wire)?;
    verify_binding(scope, challenge, proof, verifier)
}

fn verify_binding<Verify>(
    scope: &RemoteAgentOneEchoScopeV1,
    challenge: &RemoteAgentDescribeChallengeV1,
    proof: RemoteAgentDescribeProofBytesV1,
    verifier: &mut Verify,
) -> Result<RemoteAgentVerifiedDataPlaneBindingV1, RemoteAgentOneEchoErrorV1>
where
    Verify: RemoteAgentAccessSignatureVerifierV1,
{
    let verified = verify_remote_agent_describe_proof_v1(scope, challenge, proof, verifier)
        .map_err(map_proof_error)?;
    let response = RemoteAgentAccessResponseV1::decode(verified.proof().response_wire())
        .map_err(|_| RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?;
    let profile = response
        .profile()
        .ok_or(RemoteAgentOneEchoErrorV1::InvalidDescribeProof)?
        .clone();
    if profile.mac_agent_client_principal() != scope.mac_agent_client_principal()
        || profile.profile_digest() != scope.profile_digest()
    {
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
    Ok(RemoteAgentVerifiedDataPlaneBindingV1 {
        binding: RemoteAgentDataPlaneBindingV1 {
            profile,
            port,
            fabric_generation,
            agent_generation,
            access_generation,
        },
        proof: verified,
    })
}

fn map_proof_error(error: RemoteAgentOutboxError) -> RemoteAgentOneEchoErrorV1 {
    match error {
        RemoteAgentOutboxError::DescribeCarrierMismatch => {
            RemoteAgentOneEchoErrorV1::DescribeCarrierMismatch
        }
        RemoteAgentOutboxError::DescribeChallengeMismatch => {
            RemoteAgentOneEchoErrorV1::DescribeChallengeMismatch
        }
        RemoteAgentOutboxError::DescribeScopeMismatch => {
            RemoteAgentOneEchoErrorV1::DescribeScopeMismatch
        }
        RemoteAgentOutboxError::DescribeAuthenticationFailed => {
            RemoteAgentOneEchoErrorV1::DescribeAuthenticationFailed
        }
        other => RemoteAgentOneEchoErrorV1::Outbox(other),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentOneEchoErrorV1 {
    ScopeMismatch,
    InvalidOutboxState,
    DescribeUnavailable,
    InvalidDescribeProof,
    DescribeCarrierMismatch,
    DescribeChallengeMismatch,
    DescribeScopeMismatch,
    DescribeAuthenticationFailed,
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
    use paraegox_kernel::digest::Digest32;
    use paraegox_kernel::identity::PrincipalRef;
    use paraegox_runtime_contracts::distributed_agent_stack_plan::{
        RestrictedRuntimeApplyCarrierBindingFieldsV1, RestrictedRuntimeApplyCarrierBindingV1,
    };
    use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
    use paraegox_runtime_contracts::remote_agent_access::{
        RemoteAgentAccessRequestDraftV1, RemoteAgentAccessRequestFieldsV1,
        RemoteAgentAccessRequestIdV1, RemoteAgentAccessRequestV1,
        RemoteAgentAccessResponseAuthClaimV1,
        RemoteAgentAccessResponseDraftV1,
    };
    use paraegox_runtime_contracts::wire::{
        ApplyAuthAlgorithm, ApplyAuthKeyRef, ApplyRequestAuthClaim,
    };

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
            _challenge: &RemoteAgentDescribeChallengeV1,
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
        describe_request(
            base,
            base.carrier().clone(),
            RemoteAgentAccessRequestIdV1::try_from_bytes([0x95; 16]).expect("request id"),
            b"t2-d0-second-fresh-describe",
        )
    }

    fn describe_request(
        base: &RemoteAgentAccessRequestV1,
        carrier: RestrictedRuntimeApplyCarrierBindingV1,
        request_id: RemoteAgentAccessRequestIdV1,
        nonce: &[u8],
    ) -> RemoteAgentAccessRequestV1 {
        let old_claim = base.authentication().claim();
        let claim = ApplyRequestAuthClaim::try_new(
            old_claim.principal(),
            old_claim.key(),
            old_claim.algorithm(),
            old_claim.algorithm_version(),
            nonce,
        )
        .expect("auth claim");
        let draft = RemoteAgentAccessRequestDraftV1::try_describe_remote_access(
            RemoteAgentAccessRequestFieldsV1 {
                request_id,
                carrier,
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
        .expect("PXRA draft");
        let signature = SigningKey::from_bytes(&CONTROLLER_SEED)
            .sign(
                draft
                    .signing_transcript()
                    .expect("PXRA transcript")
                    .as_bytes(),
            )
            .to_bytes();
        draft.finalize(&signature).expect("PXRA")
    }

    fn other_valid_carrier(
        base: &RestrictedRuntimeApplyCarrierBindingV1,
    ) -> RestrictedRuntimeApplyCarrierBindingV1 {
        RestrictedRuntimeApplyCarrierBindingV1::try_new(
            RestrictedRuntimeApplyCarrierBindingFieldsV1 {
                target: base.target(),
                runtime_principal: base.runtime_principal(),
                controller_principal: base.controller_principal(),
                endpoint_ref: [0xed; 16],
                endpoint_generation: base.endpoint_generation() + 1,
                route: "paraegox/runtime-other/apply",
                controller_request_key: base.controller_request_key(),
                controller_request_key_fingerprint: base.controller_request_key_fingerprint(),
                runtime_response_key: base.runtime_response_key(),
                runtime_response_key_fingerprint: base.runtime_response_key_fingerprint(),
                control_transport_profile_ref: base.control_transport_profile_ref(),
                control_transport_profile_digest: base.control_transport_profile_digest(),
            },
        )
        .expect("other valid carrier")
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

    fn verified_proof(
        scope: &RemoteAgentOneEchoScopeV1,
        challenge: &RemoteAgentDescribeChallengeV1,
        proof: &RemoteAgentDescribeWireProofV1,
    ) -> RemoteAgentVerifiedDescribeProofV1 {
        let request = RemoteAgentAccessRequestV1::decode(&proof.request_wire).expect("PXRA");
        let mut verifier = TestVerifier::for_request(&request);
        verify_remote_agent_describe_proof_v1(
            scope,
            challenge,
            RemoteAgentDescribeProofBytesV1::try_new(&proof.request_wire, &proof.response_wire)
                .expect("proof bytes"),
            &mut verifier,
        )
        .expect("verified proof")
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
        let echo_describe = next_request(request);
        scope_for([0x51; 16], echo_request(), request, &echo_describe)
    }

    fn scope_for(
        attempt_id: [u8; 16],
        echo: AgentConversationRequestV1,
        open_describe: &RemoteAgentAccessRequestV1,
        echo_describe: &RemoteAgentAccessRequestV1,
    ) -> RemoteAgentOneEchoScopeV1 {
        RemoteAgentOneEchoScopeV1::try_new(
            crate::remote_agent_outbox::RemoteAgentOneEchoScopeFieldsV1 {
                attempt_id,
                echo_request: echo,
                target: open_describe.target(),
                runtime_store_instance_id: open_describe.expected_runtime_store_instance_id(),
                runtime_host_epoch: open_describe.expected_runtime_host_epoch(),
                expected_pxau_digest: open_describe.expected_pxau_digest(),
                expected_active_pxst_digest: open_describe.expected_active_pxst_digest(),
                profile_digest: open_describe.profile_digest(),
                mac_agent_client_principal: open_describe.intended_mac_agent_client(),
                carrier: open_describe.carrier().clone(),
                open_request_id: open_describe.request_id(),
                open_auth_nonce: open_describe.authentication().claim().nonce(),
                echo_request_id: echo_describe.request_id(),
                echo_auth_nonce: echo_describe.authentication().claim().nonce(),
            },
        )
        .expect("scope")
    }

    fn prepared(events: Events, commit: &mut FakeCommit) -> RemoteAgentOutboxV1 {
        let outbox = RemoteAgentOutboxV1::try_prepare(scope(&golden_request()), commit)
            .expect("prepared outbox");
        assert_eq!(
            events.borrow().as_slice(),
            &[Event::Commit(1)],
            "prepare has no source, transport, or entropy callback"
        );
        outbox
    }

    fn complete_direct(
        scope: &RemoteAgentOneEchoScopeV1,
        open: &RemoteAgentDescribeWireProofV1,
        echo: &RemoteAgentDescribeWireProofV1,
        commit: &mut FakeCommit,
    ) -> RemoteAgentOutboxV1 {
        let mut outbox = RemoteAgentOutboxV1::try_prepare(scope.clone(), commit).unwrap();
        let open_action = outbox
            .claim_open(
                scope,
                verified_proof(scope, scope.open_challenge(), open),
                commit,
            )
            .unwrap();
        let open_exchange = open_action
            .exchange(|_| Ok::<_, ()>(AgentConversationOpenOutcomeV1::Opened))
            .unwrap();
        outbox
            .commit_open_result(open_exchange, commit)
            .unwrap();
        let echo_action = outbox
            .claim_echo(
                scope,
                verified_proof(scope, scope.echo_challenge(), echo),
                commit,
            )
            .unwrap();
        let echo_exchange = echo_action
            .exchange(|request| {
                Ok::<_, ()>(AgentConversationTerminalV1::try_success(request, "Echo").unwrap())
            })
            .unwrap();
        outbox.commit_terminal(echo_exchange, commit).unwrap();
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
        assert!(recovered.scope_matches(&scope(&golden_request())));
    }

    #[test]
    fn scope_accepts_only_exact_literal_echo() {
        let request = golden_request();
        let second = next_request(&request);
        let not_echo = AgentConversationRequestV1::try_new(
            AgentConversationDeckRunId::try_from_bytes([0x31; 16]).unwrap(),
            AgentConversationSessionId::try_from_bytes([0x32; 16]).unwrap(),
            AgentConversationTurnId::try_from_bytes([0x33; 16]).unwrap(),
            AgentConversationRequestId::try_from_bytes([0x34; 16]).unwrap(),
            5_000_000_000,
            "echo",
        )
        .unwrap();
        assert_eq!(
            RemoteAgentOneEchoScopeV1::try_new(
                crate::remote_agent_outbox::RemoteAgentOneEchoScopeFieldsV1 {
                    attempt_id: [0x51; 16],
                    echo_request: not_echo,
                    target: request.target(),
                    runtime_store_instance_id: request.expected_runtime_store_instance_id(),
                    runtime_host_epoch: request.expected_runtime_host_epoch(),
                    expected_pxau_digest: request.expected_pxau_digest(),
                    expected_active_pxst_digest: request.expected_active_pxst_digest(),
                    profile_digest: request.profile_digest(),
                    mac_agent_client_principal: request.intended_mac_agent_client(),
                    carrier: request.carrier().clone(),
                    open_request_id: request.request_id(),
                    open_auth_nonce: request.authentication().claim().nonce(),
                    echo_request_id: second.request_id(),
                    echo_auth_nonce: second.authentication().claim().nonce(),
                },
            ),
            Err(RemoteAgentOutboxError::InvalidScope)
        );
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
            &scope(&request),
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
        assert_eq!(verifier.controller_calls, 5);
        assert_eq!(verifier.runtime_calls, 5);
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
                &scope(&request),
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
        let expected_scope = scope(&request);
        let proof = verified_proof(&expected_scope, expected_scope.open_challenge(), &open);
        let _send_action = outbox
            .claim_open(&expected_scope, proof, &mut commit)
            .unwrap();
        let before = events.borrow().clone();
        let mut describe = FakeDescribe::new(events.clone(), Vec::new());
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &expected_scope,
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
        let expected_scope = scope(&request);
        let open_proof = verified_proof(
            &expected_scope,
            expected_scope.open_challenge(),
            &open,
        );
        let open_action = outbox
            .claim_open(&expected_scope, open_proof, &mut commit)
            .unwrap();
        let open_exchange = open_action
            .exchange(|_| Ok::<_, ()>(AgentConversationOpenOutcomeV1::Opened))
            .unwrap();
        outbox
            .commit_open_result(open_exchange, &mut commit)
            .unwrap();
        let echo_proof = verified_proof(
            &expected_scope,
            expected_scope.echo_challenge(),
            &echo,
        );
        let _echo_action = outbox
            .claim_echo(&expected_scope, echo_proof, &mut commit)
            .unwrap();
        let before = events.borrow().clone();
        let mut describe = FakeDescribe::new(events.clone(), Vec::new());
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &expected_scope,
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
            &scope(&request),
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
            transport.open_calls,
            transport.echo_calls,
            commit.calls,
        );
        let verifier_calls = (verifier.controller_calls, verifier.runtime_calls);
        let resumed = run_remote_agent_one_echo_v1(
            &scope(&request),
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
                transport.open_calls,
                transport.echo_calls,
                commit.calls,
            ),
            calls
        );
        assert_eq!(
            (verifier.controller_calls, verifier.runtime_calls),
            (verifier_calls.0 + 2, verifier_calls.1 + 2)
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
                &scope(request),
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
    fn describe_challenge_request_id_and_nonce_mismatch_is_not_claimed() {
        let (request, open, _) = valid_proofs();
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![open.clone(), open]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events);
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::DescribeChallengeMismatch)
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
                &scope(&request),
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
            RemoteAgentOutboxPhaseV1::EchoUncertain { .. }
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
            &scope(&request),
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
                &scope(&request),
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
    fn other_valid_carrier_with_valid_signatures_is_rejected_before_claim() {
        let (request, _, _) = valid_proofs();
        let other_request = describe_request(
            &request,
            other_valid_carrier(request.carrier()),
            request.request_id(),
            request.authentication().claim().nonce(),
        );
        let proof = proof_for(
            &other_request,
            (9, 10, 11),
            &decode_hex(PORT_GOLDEN.trim()),
        );
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = prepared(events.clone(), &mut commit);
        let mut describe = FakeDescribe::new(events.clone(), vec![proof]);
        let mut verifier = TestVerifier::for_request(&other_request);
        let mut transport = FakeTransport::new(events);
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &scope(&request),
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::DescribeCarrierMismatch)
        );
        assert_eq!(commit.records.len(), 1);
        assert_eq!((transport.open_calls, transport.echo_calls), (0, 0));
        assert_eq!((verifier.controller_calls, verifier.runtime_calls), (0, 0));
    }

    #[test]
    fn restart_with_different_exact_scope_is_rejected_without_external_calls() {
        let (request, _, _) = valid_proofs();
        let second = next_request(&request);
        let scope_a = scope_for([0x51; 16], echo_request(), &request, &second);
        let scope_b = scope_for([0x51; 16], other_echo_request(), &request, &second);
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = RemoteAgentOutboxV1::try_prepare(scope_a, &mut commit).unwrap();
        let before = events.borrow().clone();
        let mut describe = FakeDescribe::new(events.clone(), Vec::new());
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &scope_b,
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::ScopeMismatch)
        );
        assert_eq!(*events.borrow(), before);
        assert_eq!((describe.calls, transport.open_calls, transport.echo_calls), (0, 0, 0));
    }

    #[test]
    fn terminal_commit_failure_consumes_permit_and_resume_never_resends() {
        let (request, open, echo) = valid_proofs();
        let expected_scope = scope(&request);
        let events = Events::default();
        let mut commit = FakeCommit::fail_on(events.clone(), 5);
        let mut outbox = RemoteAgentOutboxV1::try_prepare(expected_scope.clone(), &mut commit)
            .expect("prepare");
        let mut describe = FakeDescribe::new(events.clone(), vec![open, echo]);
        let mut verifier = TestVerifier::for_request(&request);
        let mut transport = FakeTransport::new(events.clone());
        assert!(matches!(
            run_remote_agent_one_echo_v1(
                &expected_scope,
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::Commit(_))
        ));
        assert!(matches!(
            outbox.phase(),
            RemoteAgentOutboxPhaseV1::EchoUncertain { .. }
        ));
        let before = events.borrow().clone();
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &expected_scope,
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::ReconcileRequired)
        );
        assert_eq!(*events.borrow(), before);
        assert_eq!((transport.open_calls, transport.echo_calls), (1, 1));
    }

    #[test]
    fn terminal_resume_rejects_invalid_signature_with_zero_external_calls() {
        let (request, open, echo) = valid_proofs();
        let expected_scope = scope(&request);
        let events = Events::default();
        let mut commit = FakeCommit::new(events.clone());
        let mut outbox = complete_direct(&expected_scope, &open, &echo, &mut commit);
        let before = events.borrow().clone();
        let commit_calls = commit.calls;
        let mut describe = FakeDescribe::new(events.clone(), Vec::new());
        let mut verifier = TestVerifier::for_request(&request);
        verifier.runtime_fingerprint = Digest32::from_bytes([0xfe; 32]);
        let mut transport = FakeTransport::new(events.clone());
        assert_eq!(
            run_remote_agent_one_echo_v1(
                &expected_scope,
                &mut outbox,
                &mut describe,
                &mut verifier,
                &mut transport,
                &mut commit,
            ),
            Err(RemoteAgentOneEchoErrorV1::DescribeAuthenticationFailed)
        );
        assert_eq!(*events.borrow(), before);
        assert_eq!(commit.calls, commit_calls);
        assert_eq!((describe.calls, transport.open_calls, transport.echo_calls), (0, 0, 0));
    }

    #[test]
    fn marker_rejects_request_id_nonce_and_wrong_carrier_before_claim() {
        let (request, _, _) = valid_proofs();
        let expected_scope = scope(&request);
        let wrong_id = describe_request(
            &request,
            request.carrier().clone(),
            RemoteAgentAccessRequestIdV1::try_from_bytes([0x96; 16]).unwrap(),
            request.authentication().claim().nonce(),
        );
        let wrong_nonce = describe_request(
            &request,
            request.carrier().clone(),
            request.request_id(),
            b"t2-d0-wrong-open-challenge",
        );
        let wrong_carrier = describe_request(
            &request,
            other_valid_carrier(request.carrier()),
            request.request_id(),
            request.authentication().claim().nonce(),
        );
        for candidate in [&wrong_id, &wrong_nonce, &wrong_carrier] {
            let wire = proof_for(candidate, (9, 10, 11), &decode_hex(PORT_GOLDEN.trim()));
            let proof = RemoteAgentDescribeProofBytesV1::try_new(
                &wire.request_wire,
                &wire.response_wire,
            )
            .unwrap();
            let mut verifier = TestVerifier::for_request(candidate);
            assert!(verify_remote_agent_describe_proof_v1(
                &expected_scope,
                expected_scope.open_challenge(),
                proof,
                &mut verifier,
            )
            .is_err());
        }
    }

    #[test]
    fn cross_attempt_terminal_splice_and_zero_attempt_records_are_rejected() {
        let (request, open, echo) = valid_proofs();
        let second = next_request(&request);
        let scope_a = scope_for([0x51; 16], echo_request(), &request, &second);
        let scope_b = scope_for([0x52; 16], echo_request(), &request, &second);
        let mut commit_a = FakeCommit::new(Events::default());
        let mut commit_b = FakeCommit::new(Events::default());
        let outbox_a = complete_direct(&scope_a, &open, &echo, &mut commit_a);
        complete_direct(&scope_b, &open, &echo, &mut commit_b);

        let mut spliced = commit_a.records[..4].concat();
        spliced.extend_from_slice(&commit_b.records[4]);
        assert_eq!(
            RemoteAgentOutboxV1::decode(&spliced),
            Err(RemoteAgentOutboxError::RecordChainMismatch)
        );

        let mut zero_single = commit_a.records[0].clone();
        zero_single[16..32].fill(0);
        assert_eq!(
            RemoteAgentOutboxV1::decode(&zero_single),
            Err(RemoteAgentOutboxError::InvalidAttempt)
        );
        let mut zero_later = outbox_a.canonical_wire().to_vec();
        let second_attempt = commit_a.records[0].len() + 16;
        zero_later[second_attempt..second_attempt + 16].fill(0);
        assert_eq!(
            RemoteAgentOutboxV1::decode(&zero_later),
            Err(RemoteAgentOutboxError::InvalidAttempt)
        );
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

        let (request, open, _) = valid_proofs();
        let mut claimed_commit = FakeCommit::new(Events::default());
        let second = next_request(&request);
        let expected_scope = scope_for([0x61; 16], echo_request(), &request, &second);
        let mut claimed = RemoteAgentOutboxV1::try_prepare(
            expected_scope.clone(),
            &mut claimed_commit,
        )
        .unwrap();
        let _send_action = claimed
            .claim_open(
                &expected_scope,
                verified_proof(&expected_scope, expected_scope.open_challenge(), &open),
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
