//! Runtime-private canonical journal for one remote Agent Open-and-Echo attempt.
//!
//! PXOJ v1 is only the bounded state codec and commit seam. Its Prepared root
//! commits the complete attempt scope, exact literal Echo request, trusted
//! carrier, and both preallocated Describe challenges. A successful claim
//! returns one non-cloneable send action; recovery of an Uncertain record never
//! recreates that action. This module deliberately supplies no filesystem
//! implementation and makes no APFS durability claim.

#![forbid(unsafe_code)]

use core::fmt;

use paraegox_agent_contracts::control::{
    AgentConversationControlBodyV1, AgentConversationControlV1, AgentConversationOpenOutcomeV1,
    MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES,
};
use paraegox_agent_contracts::{
    AgentConversationRequestV1, AgentConversationTerminalV1, MAX_AGENT_CONVERSATION_FRAME_BYTES,
};
use paraegox_kernel::digest::{Digest32, Digest32Builder};
use paraegox_kernel::identity::{PrincipalRef, RuntimeHostId};
use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES, RestrictedRuntimeApplyCarrierBindingV1,
};
use paraegox_runtime_contracts::remote_agent_access::{
    MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES, MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES,
    RemoteAgentAccessKindV1, RemoteAgentAccessRequestIdV1, RemoteAgentAccessRequestV1,
    RemoteAgentAccessResponseV1,
};
use paraegox_runtime_contracts::wire::{ApplyAuthKeyRef, MAX_APPLY_AUTH_NONCE_BYTES};

pub(crate) const REMOTE_AGENT_OUTBOX_MAGIC: &[u8; 4] = b"PXOJ";
pub(crate) const REMOTE_AGENT_OUTBOX_VERSION: u16 = 1;
pub(crate) const REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES: usize = 96;
pub(crate) const MAX_REMOTE_AGENT_OUTBOX_RECORDS: usize = 5;

const RECORD_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-outbox.record.sha256.v1";
const ZERO_DIGEST: Digest32 = Digest32::from_bytes([0; 32]);
const PREPARED_FIXED_BYTES: usize = 216;
const PREPARED_PAYLOAD_BYTES: usize = PREPARED_FIXED_BYTES
    + MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES
    + MAX_AGENT_CONVERSATION_FRAME_BYTES
    + MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
    + 2 * MAX_APPLY_AUTH_NONCE_BYTES;
const CLAIM_PAYLOAD_BYTES: usize =
    8 + MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES + MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES;
const MAX_REMOTE_AGENT_OUTBOX_JOURNAL_BYTES: usize = MAX_REMOTE_AGENT_OUTBOX_RECORDS
    * REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES
    + PREPARED_PAYLOAD_BYTES
    + 2 * CLAIM_PAYLOAD_BYTES
    + MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES
    + MAX_AGENT_CONVERSATION_FRAME_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
enum RemoteAgentOutboxRecordKindV1 {
    Prepared = 1,
    OpenClaimed = 2,
    OpenResult = 3,
    EchoClaimed = 4,
    EchoTerminal = 5,
}

impl RemoteAgentOutboxRecordKindV1 {
    fn decode(value: u16) -> Result<Self, RemoteAgentOutboxError> {
        match value {
            1 => Ok(Self::Prepared),
            2 => Ok(Self::OpenClaimed),
            3 => Ok(Self::OpenResult),
            4 => Ok(Self::EchoClaimed),
            5 => Ok(Self::EchoTerminal),
            _ => Err(RemoteAgentOutboxError::UnsupportedRecordKind),
        }
    }

    const fn max_payload_bytes(self) -> usize {
        match self {
            Self::Prepared => PREPARED_PAYLOAD_BYTES,
            Self::OpenClaimed | Self::EchoClaimed => CLAIM_PAYLOAD_BYTES,
            Self::OpenResult => MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES,
            Self::EchoTerminal => MAX_AGENT_CONVERSATION_FRAME_BYTES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentDescribeChallengeV1 {
    request_id: RemoteAgentAccessRequestIdV1,
    auth_nonce: Box<[u8]>,
}

impl RemoteAgentDescribeChallengeV1 {
    pub(crate) fn try_new(
        request_id: RemoteAgentAccessRequestIdV1,
        auth_nonce: &[u8],
    ) -> Result<Self, RemoteAgentOutboxError> {
        if auth_nonce.is_empty()
            || auth_nonce.len() > MAX_APPLY_AUTH_NONCE_BYTES
            || auth_nonce.iter().all(|byte| *byte == 0)
        {
            return Err(RemoteAgentOutboxError::InvalidChallenge);
        }
        Ok(Self {
            request_id,
            auth_nonce: auth_nonce.into(),
        })
    }

    pub(crate) const fn request_id(&self) -> RemoteAgentAccessRequestIdV1 {
        self.request_id
    }

    pub(crate) fn auth_nonce(&self) -> &[u8] {
        &self.auth_nonce
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RemoteAgentOneEchoScopeFieldsV1<'a> {
    pub attempt_id: [u8; 16],
    pub echo_request: AgentConversationRequestV1,
    pub target: RuntimeHostId,
    pub runtime_store_instance_id: [u8; 32],
    pub runtime_host_epoch: u64,
    pub expected_pxau_digest: Digest32,
    pub expected_active_pxst_digest: Digest32,
    pub profile_digest: Digest32,
    pub mac_agent_client_principal: PrincipalRef,
    pub carrier: RestrictedRuntimeApplyCarrierBindingV1,
    pub open_request_id: RemoteAgentAccessRequestIdV1,
    pub open_auth_nonce: &'a [u8],
    pub echo_request_id: RemoteAgentAccessRequestIdV1,
    pub echo_auth_nonce: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOneEchoScopeV1 {
    attempt_id: [u8; 16],
    echo_request: AgentConversationRequestV1,
    target: RuntimeHostId,
    runtime_store_instance_id: [u8; 32],
    runtime_host_epoch: u64,
    expected_pxau_digest: Digest32,
    expected_active_pxst_digest: Digest32,
    profile_digest: Digest32,
    mac_agent_client_principal: PrincipalRef,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    open_challenge: RemoteAgentDescribeChallengeV1,
    echo_challenge: RemoteAgentDescribeChallengeV1,
}

impl RemoteAgentOneEchoScopeV1 {
    pub(crate) fn try_new(
        fields: RemoteAgentOneEchoScopeFieldsV1<'_>,
    ) -> Result<Self, RemoteAgentOutboxError> {
        let open_challenge = RemoteAgentDescribeChallengeV1::try_new(
            fields.open_request_id,
            fields.open_auth_nonce,
        )?;
        let echo_challenge = RemoteAgentDescribeChallengeV1::try_new(
            fields.echo_request_id,
            fields.echo_auth_nonce,
        )?;
        if bytes_are_zero(&fields.attempt_id)
            || fields.echo_request.input() != "Echo"
            || bytes_are_zero(fields.target.as_bytes())
            || bytes_are_zero(&fields.runtime_store_instance_id)
            || fields.runtime_host_epoch == 0
            || digest_is_zero(fields.expected_pxau_digest)
            || digest_is_zero(fields.expected_active_pxst_digest)
            || digest_is_zero(fields.profile_digest)
            || bytes_are_zero(fields.mac_agent_client_principal.as_bytes())
            || fields.carrier.target() != fields.target
            || fields.mac_agent_client_principal == fields.carrier.controller_principal()
            || fields.mac_agent_client_principal == fields.carrier.runtime_principal()
            || open_challenge.request_id == echo_challenge.request_id
            || open_challenge.auth_nonce == echo_challenge.auth_nonce
        {
            return Err(RemoteAgentOutboxError::InvalidScope);
        }
        Ok(Self {
            attempt_id: fields.attempt_id,
            echo_request: fields.echo_request,
            target: fields.target,
            runtime_store_instance_id: fields.runtime_store_instance_id,
            runtime_host_epoch: fields.runtime_host_epoch,
            expected_pxau_digest: fields.expected_pxau_digest,
            expected_active_pxst_digest: fields.expected_active_pxst_digest,
            profile_digest: fields.profile_digest,
            mac_agent_client_principal: fields.mac_agent_client_principal,
            carrier: fields.carrier,
            open_challenge,
            echo_challenge,
        })
    }

    pub(crate) const fn attempt_id(&self) -> [u8; 16] {
        self.attempt_id
    }

    pub(crate) const fn echo_request(&self) -> &AgentConversationRequestV1 {
        &self.echo_request
    }

    pub(crate) const fn target(&self) -> RuntimeHostId {
        self.target
    }

    pub(crate) const fn runtime_store_instance_id(&self) -> [u8; 32] {
        self.runtime_store_instance_id
    }

    pub(crate) const fn runtime_host_epoch(&self) -> u64 {
        self.runtime_host_epoch
    }

    pub(crate) const fn expected_pxau_digest(&self) -> Digest32 {
        self.expected_pxau_digest
    }

    pub(crate) const fn expected_active_pxst_digest(&self) -> Digest32 {
        self.expected_active_pxst_digest
    }

    pub(crate) const fn profile_digest(&self) -> Digest32 {
        self.profile_digest
    }

    pub(crate) const fn mac_agent_client_principal(&self) -> PrincipalRef {
        self.mac_agent_client_principal
    }

    pub(crate) const fn carrier(&self) -> &RestrictedRuntimeApplyCarrierBindingV1 {
        &self.carrier
    }

    pub(crate) const fn open_challenge(&self) -> &RemoteAgentDescribeChallengeV1 {
        &self.open_challenge
    }

    pub(crate) const fn echo_challenge(&self) -> &RemoteAgentDescribeChallengeV1 {
        &self.echo_challenge
    }

    fn open_request(&self) -> AgentConversationControlV1 {
        AgentConversationControlV1::open_request(
            self.echo_request.deck_run_id(),
            self.echo_request.session_id(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentDescribeProofBytesV1 {
    request_wire: Box<[u8]>,
    response_wire: Box<[u8]>,
}

impl RemoteAgentDescribeProofBytesV1 {
    pub(crate) fn try_new(
        request_wire: &[u8],
        response_wire: &[u8],
    ) -> Result<Self, RemoteAgentOutboxError> {
        if request_wire.len() > MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES
            || response_wire.len() > MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES
        {
            return Err(RemoteAgentOutboxError::RecordPayloadTooLarge);
        }
        let request = RemoteAgentAccessRequestV1::decode(request_wire)
            .map_err(|_| RemoteAgentOutboxError::InvalidDescribeProof)?;
        let response = RemoteAgentAccessResponseV1::decode(response_wire)
            .map_err(|_| RemoteAgentOutboxError::InvalidDescribeProof)?;
        if request.kind() != RemoteAgentAccessKindV1::DescribeRemoteAccess
            || response.kind() != RemoteAgentAccessKindV1::DescribeRemoteAccess
            || response.validate_against_request(&request).is_err()
        {
            return Err(RemoteAgentOutboxError::InvalidDescribeProof);
        }
        Ok(Self {
            request_wire: request_wire.into(),
            response_wire: response_wire.into(),
        })
    }

    pub(crate) fn request_wire(&self) -> &[u8] {
        &self.request_wire
    }

    pub(crate) fn response_wire(&self) -> &[u8] {
        &self.response_wire
    }
}

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

#[derive(Debug)]
pub(crate) struct RemoteAgentVerifiedDescribeProofV1 {
    attempt_id: [u8; 16],
    challenge: RemoteAgentDescribeChallengeV1,
    proof: RemoteAgentDescribeProofBytesV1,
}

impl RemoteAgentVerifiedDescribeProofV1 {
    pub(crate) const fn proof(&self) -> &RemoteAgentDescribeProofBytesV1 {
        &self.proof
    }
}

pub(crate) fn verify_remote_agent_describe_proof_v1<Verify>(
    scope: &RemoteAgentOneEchoScopeV1,
    challenge: &RemoteAgentDescribeChallengeV1,
    proof: RemoteAgentDescribeProofBytesV1,
    verifier: &mut Verify,
) -> Result<RemoteAgentVerifiedDescribeProofV1, RemoteAgentOutboxError>
where
    Verify: RemoteAgentAccessSignatureVerifierV1,
{
    let (request, response) = validate_proof_scope_challenge(scope, challenge, &proof)?;
    request
        .verify_controller_request(
            scope.carrier(),
            |principal, key, fingerprint, transcript, signature| {
                verifier.verify_controller(principal, key, fingerprint, transcript, signature)
            },
        )
        .map_err(|_| RemoteAgentOutboxError::DescribeAuthenticationFailed)?;
    response
        .verify_runtime_describe_response(
            &request,
            scope.carrier(),
            |principal, key, fingerprint, transcript, signature| {
                verifier.verify_runtime(principal, key, fingerprint, transcript, signature)
            },
        )
        .map_err(|_| RemoteAgentOutboxError::DescribeAuthenticationFailed)?;
    Ok(RemoteAgentVerifiedDescribeProofV1 {
        attempt_id: scope.attempt_id,
        challenge: challenge.clone(),
        proof,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentOutboxPhaseV1 {
    OpenRequestDurableNotSent,
    OpenUncertain {
        open_proof: RemoteAgentDescribeProofBytesV1,
        claim_digest: Digest32,
    },
    EchoRequestDurableNotSent {
        open_outcome: AgentConversationOpenOutcomeV1,
        open_proof: RemoteAgentDescribeProofBytesV1,
    },
    OpenTerminal {
        open_outcome: AgentConversationOpenOutcomeV1,
        open_proof: RemoteAgentDescribeProofBytesV1,
    },
    EchoUncertain {
        open_proof: RemoteAgentDescribeProofBytesV1,
        echo_proof: RemoteAgentDescribeProofBytesV1,
        claim_digest: Digest32,
    },
    Terminal {
        open_proof: RemoteAgentDescribeProofBytesV1,
        echo_proof: RemoteAgentDescribeProofBytesV1,
        terminal: AgentConversationTerminalV1,
    },
}

pub(crate) trait RemoteAgentOutboxCommitV1 {
    fn commit_record(&mut self, record: &[u8]) -> Result<(), RemoteAgentOutboxCommitFailureV1>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOutboxCommitFailureV1;

#[derive(Debug)]
pub(crate) struct RemoteAgentOpenSendActionV1 {
    attempt_id: [u8; 16],
    claim_digest: Digest32,
    request: AgentConversationControlV1,
}

impl RemoteAgentOpenSendActionV1 {
    pub(crate) fn exchange<Send, Error>(
        self,
        send: Send,
    ) -> Result<RemoteAgentOpenExchangeOutcomeV1, Error>
    where
        Send: FnOnce(&AgentConversationControlV1) -> Result<AgentConversationOpenOutcomeV1, Error>,
    {
        let outcome = send(&self.request)?;
        Ok(RemoteAgentOpenExchangeOutcomeV1 {
            attempt_id: self.attempt_id,
            claim_digest: self.claim_digest,
            outcome,
        })
    }
}

#[derive(Debug)]
pub(crate) struct RemoteAgentOpenExchangeOutcomeV1 {
    attempt_id: [u8; 16],
    claim_digest: Digest32,
    outcome: AgentConversationOpenOutcomeV1,
}

#[derive(Debug)]
pub(crate) struct RemoteAgentEchoSendActionV1 {
    attempt_id: [u8; 16],
    claim_digest: Digest32,
    request: AgentConversationRequestV1,
}

impl RemoteAgentEchoSendActionV1 {
    pub(crate) fn exchange<Send, Error>(
        self,
        send: Send,
    ) -> Result<RemoteAgentEchoExchangeOutcomeV1, Error>
    where
        Send: FnOnce(&AgentConversationRequestV1) -> Result<AgentConversationTerminalV1, Error>,
    {
        let terminal = send(&self.request)?;
        if !terminal.correlates(&self.request) {
            return Ok(RemoteAgentEchoExchangeOutcomeV1 {
                attempt_id: self.attempt_id,
                claim_digest: self.claim_digest,
                terminal,
                correlated: false,
            });
        }
        Ok(RemoteAgentEchoExchangeOutcomeV1 {
            attempt_id: self.attempt_id,
            claim_digest: self.claim_digest,
            terminal,
            correlated: true,
        })
    }
}

#[derive(Debug)]
pub(crate) struct RemoteAgentEchoExchangeOutcomeV1 {
    attempt_id: [u8; 16],
    claim_digest: Digest32,
    terminal: AgentConversationTerminalV1,
    correlated: bool,
}

impl RemoteAgentEchoExchangeOutcomeV1 {
    pub(crate) const fn is_correlated(&self) -> bool {
        self.correlated
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOutboxV1 {
    scope: RemoteAgentOneEchoScopeV1,
    phase: RemoteAgentOutboxPhaseV1,
    journal_wire: Vec<u8>,
    last_record_digest: Digest32,
    record_count: usize,
}

impl RemoteAgentOutboxV1 {
    pub(crate) fn try_prepare<Commit>(
        scope: RemoteAgentOneEchoScopeV1,
        commit: &mut Commit,
    ) -> Result<Self, RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        let payload = encode_prepared_payload(&scope)?;
        let record = encode_record(
            scope.attempt_id,
            1,
            RemoteAgentOutboxRecordKindV1::Prepared,
            ZERO_DIGEST,
            &payload,
        )?;
        commit
            .commit_record(&record)
            .map_err(RemoteAgentOutboxMutationErrorV1::Commit)?;
        let last_record_digest = record_digest_from_wire(&record);
        Ok(Self {
            scope,
            phase: RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent,
            journal_wire: record,
            last_record_digest,
            record_count: 1,
        })
    }

    pub(crate) fn decode(frame: &[u8]) -> Result<Self, RemoteAgentOutboxError> {
        if frame.len() > MAX_REMOTE_AGENT_OUTBOX_JOURNAL_BYTES {
            return Err(RemoteAgentOutboxError::JournalTooLarge);
        }
        let mut offset = 0;
        let mut state: Option<Self> = None;
        while offset < frame.len() {
            let parsed = parse_record(&frame[offset..])?;
            let expected_sequence = state.as_ref().map_or(1, |value| value.record_count + 1);
            if parsed.sequence != expected_sequence {
                return Err(RemoteAgentOutboxError::RecordGap);
            }
            match state.as_mut() {
                None => {
                    if parsed.kind != RemoteAgentOutboxRecordKindV1::Prepared
                        || parsed.previous_digest != ZERO_DIGEST
                    {
                        return Err(RemoteAgentOutboxError::InvalidPhaseTransition);
                    }
                    let scope = decode_prepared_payload(parsed.attempt_id, parsed.payload)?;
                    state = Some(Self {
                        scope,
                        phase: RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent,
                        journal_wire: parsed.wire.to_vec(),
                        last_record_digest: parsed.record_digest,
                        record_count: 1,
                    });
                }
                Some(value) => {
                    if parsed.attempt_id != value.scope.attempt_id
                        || parsed.previous_digest != value.last_record_digest
                    {
                        return Err(RemoteAgentOutboxError::RecordChainMismatch);
                    }
                    let next_phase =
                        value.next_phase(parsed.kind, parsed.payload, parsed.record_digest)?;
                    value.phase = next_phase;
                    value.journal_wire.extend_from_slice(parsed.wire);
                    value.last_record_digest = parsed.record_digest;
                    value.record_count += 1;
                }
            }
            offset = offset
                .checked_add(parsed.wire.len())
                .ok_or(RemoteAgentOutboxError::JournalTooLarge)?;
        }
        state.ok_or(RemoteAgentOutboxError::TruncatedRecord)
    }

    pub(crate) fn claim_open<Commit>(
        &mut self,
        expected_scope: &RemoteAgentOneEchoScopeV1,
        verified: RemoteAgentVerifiedDescribeProofV1,
        commit: &mut Commit,
    ) -> Result<RemoteAgentOpenSendActionV1, RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        self.require_scope(expected_scope)?;
        if !matches!(
            self.phase,
            RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent
        ) {
            return Err(RemoteAgentOutboxError::InvalidPhaseTransition.into());
        }
        if verified.attempt_id != self.scope.attempt_id
            || verified.challenge != self.scope.open_challenge
        {
            return Err(RemoteAgentOutboxError::ActionMismatch.into());
        }
        validate_proof_scope_challenge(&self.scope, &self.scope.open_challenge, &verified.proof)?;
        let proof = verified.proof;
        let payload = encode_proof_payload(&proof)?;
        let claim_digest =
            self.commit_record(RemoteAgentOutboxRecordKindV1::OpenClaimed, payload, commit)?;
        self.phase = RemoteAgentOutboxPhaseV1::OpenUncertain {
            open_proof: proof,
            claim_digest,
        };
        Ok(RemoteAgentOpenSendActionV1 {
            attempt_id: self.scope.attempt_id,
            claim_digest,
            request: self.scope.open_request(),
        })
    }

    pub(crate) fn commit_open_result<Commit>(
        &mut self,
        exchange: RemoteAgentOpenExchangeOutcomeV1,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        let RemoteAgentOutboxPhaseV1::OpenUncertain {
            open_proof,
            claim_digest,
        } = &self.phase
        else {
            return Err(RemoteAgentOutboxError::InvalidPhaseTransition.into());
        };
        if exchange.attempt_id != self.scope.attempt_id || exchange.claim_digest != *claim_digest {
            return Err(RemoteAgentOutboxError::ActionMismatch.into());
        }
        let open_proof = open_proof.clone();
        let response = AgentConversationControlV1::open_result(
            self.scope.echo_request.deck_run_id(),
            self.scope.echo_request.session_id(),
            exchange.outcome,
        );
        self.commit_record(
            RemoteAgentOutboxRecordKindV1::OpenResult,
            response
                .canonical_wire()
                .map_err(|_| RemoteAgentOutboxError::InvalidOpenResult)?
                .into_vec(),
            commit,
        )?;
        self.phase = match exchange.outcome {
            AgentConversationOpenOutcomeV1::Opened => {
                RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
                    open_outcome: exchange.outcome,
                    open_proof,
                }
            }
            AgentConversationOpenOutcomeV1::Existing
            | AgentConversationOpenOutcomeV1::DeckRunSealed
            | AgentConversationOpenOutcomeV1::CapacityExhausted => {
                RemoteAgentOutboxPhaseV1::OpenTerminal {
                    open_outcome: exchange.outcome,
                    open_proof,
                }
            }
        };
        Ok(())
    }

    pub(crate) fn claim_echo<Commit>(
        &mut self,
        expected_scope: &RemoteAgentOneEchoScopeV1,
        verified: RemoteAgentVerifiedDescribeProofV1,
        commit: &mut Commit,
    ) -> Result<RemoteAgentEchoSendActionV1, RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        self.require_scope(expected_scope)?;
        let RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
            open_outcome: AgentConversationOpenOutcomeV1::Opened,
            open_proof,
        } = &self.phase
        else {
            return Err(RemoteAgentOutboxError::InvalidPhaseTransition.into());
        };
        if verified.attempt_id != self.scope.attempt_id
            || verified.challenge != self.scope.echo_challenge
        {
            return Err(RemoteAgentOutboxError::ActionMismatch.into());
        }
        validate_proof_scope_challenge(&self.scope, &self.scope.echo_challenge, &verified.proof)?;
        let open_proof = open_proof.clone();
        let proof = verified.proof;
        let payload = encode_proof_payload(&proof)?;
        let claim_digest =
            self.commit_record(RemoteAgentOutboxRecordKindV1::EchoClaimed, payload, commit)?;
        self.phase = RemoteAgentOutboxPhaseV1::EchoUncertain {
            open_proof,
            echo_proof: proof,
            claim_digest,
        };
        Ok(RemoteAgentEchoSendActionV1 {
            attempt_id: self.scope.attempt_id,
            claim_digest,
            request: self.scope.echo_request.clone(),
        })
    }

    pub(crate) fn commit_terminal<Commit>(
        &mut self,
        exchange: RemoteAgentEchoExchangeOutcomeV1,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        let RemoteAgentOutboxPhaseV1::EchoUncertain {
            open_proof,
            echo_proof,
            claim_digest,
        } = &self.phase
        else {
            return Err(RemoteAgentOutboxError::InvalidPhaseTransition.into());
        };
        if exchange.attempt_id != self.scope.attempt_id
            || exchange.claim_digest != *claim_digest
            || !exchange.correlated
            || !exchange.terminal.correlates(&self.scope.echo_request)
        {
            return Err(RemoteAgentOutboxError::TerminalCorrelationMismatch.into());
        }
        let open_proof = open_proof.clone();
        let echo_proof = echo_proof.clone();
        let terminal = exchange.terminal;
        self.commit_record(
            RemoteAgentOutboxRecordKindV1::EchoTerminal,
            terminal.canonical_wire().into_vec(),
            commit,
        )?;
        self.phase = RemoteAgentOutboxPhaseV1::Terminal {
            open_proof,
            echo_proof,
            terminal,
        };
        Ok(())
    }

    pub(crate) fn scope_matches(&self, expected: &RemoteAgentOneEchoScopeV1) -> bool {
        &self.scope == expected
    }

    pub(crate) const fn phase(&self) -> &RemoteAgentOutboxPhaseV1 {
        &self.phase
    }

    pub(crate) fn canonical_wire(&self) -> &[u8] {
        &self.journal_wire
    }

    fn require_scope(
        &self,
        expected: &RemoteAgentOneEchoScopeV1,
    ) -> Result<(), RemoteAgentOutboxError> {
        if self.scope_matches(expected) {
            Ok(())
        } else {
            Err(RemoteAgentOutboxError::ScopeMismatch)
        }
    }

    fn commit_record<Commit>(
        &mut self,
        kind: RemoteAgentOutboxRecordKindV1,
        payload: Vec<u8>,
        commit: &mut Commit,
    ) -> Result<Digest32, RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        let sequence = self
            .record_count
            .checked_add(1)
            .ok_or(RemoteAgentOutboxError::TooManyRecords)?;
        if sequence > MAX_REMOTE_AGENT_OUTBOX_RECORDS {
            return Err(RemoteAgentOutboxError::TooManyRecords.into());
        }
        let record = encode_record(
            self.scope.attempt_id,
            sequence,
            kind,
            self.last_record_digest,
            &payload,
        )?;
        commit
            .commit_record(&record)
            .map_err(RemoteAgentOutboxMutationErrorV1::Commit)?;
        let record_digest = record_digest_from_wire(&record);
        self.last_record_digest = record_digest;
        self.record_count = sequence;
        self.journal_wire.extend_from_slice(&record);
        Ok(record_digest)
    }

    fn next_phase(
        &self,
        kind: RemoteAgentOutboxRecordKindV1,
        payload: &[u8],
        record_digest: Digest32,
    ) -> Result<RemoteAgentOutboxPhaseV1, RemoteAgentOutboxError> {
        match (&self.phase, kind) {
            (
                RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent,
                RemoteAgentOutboxRecordKindV1::OpenClaimed,
            ) => {
                let open_proof = decode_proof_payload(payload)?;
                validate_proof_scope_challenge(
                    &self.scope,
                    &self.scope.open_challenge,
                    &open_proof,
                )?;
                Ok(RemoteAgentOutboxPhaseV1::OpenUncertain {
                    open_proof,
                    claim_digest: record_digest,
                })
            }
            (
                RemoteAgentOutboxPhaseV1::OpenUncertain { open_proof, .. },
                RemoteAgentOutboxRecordKindV1::OpenResult,
            ) => {
                let outcome = decode_open_result(
                    payload,
                    self.scope.echo_request.deck_run_id(),
                    self.scope.echo_request.session_id(),
                )?;
                Ok(match outcome {
                    AgentConversationOpenOutcomeV1::Opened => {
                        RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
                            open_outcome: outcome,
                            open_proof: open_proof.clone(),
                        }
                    }
                    AgentConversationOpenOutcomeV1::Existing
                    | AgentConversationOpenOutcomeV1::DeckRunSealed
                    | AgentConversationOpenOutcomeV1::CapacityExhausted => {
                        RemoteAgentOutboxPhaseV1::OpenTerminal {
                            open_outcome: outcome,
                            open_proof: open_proof.clone(),
                        }
                    }
                })
            }
            (
                RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
                    open_outcome: AgentConversationOpenOutcomeV1::Opened,
                    open_proof,
                },
                RemoteAgentOutboxRecordKindV1::EchoClaimed,
            ) => {
                let echo_proof = decode_proof_payload(payload)?;
                validate_proof_scope_challenge(
                    &self.scope,
                    &self.scope.echo_challenge,
                    &echo_proof,
                )?;
                Ok(RemoteAgentOutboxPhaseV1::EchoUncertain {
                    open_proof: open_proof.clone(),
                    echo_proof,
                    claim_digest: record_digest,
                })
            }
            (
                RemoteAgentOutboxPhaseV1::EchoUncertain {
                    open_proof,
                    echo_proof,
                    ..
                },
                RemoteAgentOutboxRecordKindV1::EchoTerminal,
            ) => {
                let terminal = AgentConversationTerminalV1::decode(payload)
                    .map_err(|_| RemoteAgentOutboxError::InvalidTerminal)?;
                if !terminal.correlates(&self.scope.echo_request)
                    || terminal.canonical_wire().as_ref() != payload
                {
                    return Err(RemoteAgentOutboxError::TerminalCorrelationMismatch);
                }
                Ok(RemoteAgentOutboxPhaseV1::Terminal {
                    open_proof: open_proof.clone(),
                    echo_proof: echo_proof.clone(),
                    terminal,
                })
            }
            _ => Err(RemoteAgentOutboxError::InvalidPhaseTransition),
        }
    }
}

fn encode_prepared_payload(
    scope: &RemoteAgentOneEchoScopeV1,
) -> Result<Vec<u8>, RemoteAgentOutboxError> {
    let open_request = scope.open_request();
    let open_wire = open_request
        .canonical_wire()
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?;
    let echo_wire = scope.echo_request.canonical_wire();
    let carrier_wire = scope.carrier.canonical_wire();
    let mut payload = Vec::with_capacity(
        PREPARED_FIXED_BYTES
            + open_wire.len()
            + echo_wire.len()
            + carrier_wire.len()
            + scope.open_challenge.auth_nonce.len()
            + scope.echo_challenge.auth_nonce.len(),
    );
    payload.extend_from_slice(
        &u32::try_from(open_wire.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u32::try_from(echo_wire.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u16::try_from(carrier_wire.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u16::try_from(scope.open_challenge.auth_nonce.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u16::try_from(scope.echo_challenge.auth_nonce.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(&0_u16.to_be_bytes());
    payload.extend_from_slice(scope.target.as_bytes());
    payload.extend_from_slice(&scope.runtime_store_instance_id);
    payload.extend_from_slice(&scope.runtime_host_epoch.to_be_bytes());
    payload.extend_from_slice(scope.expected_pxau_digest.as_bytes());
    payload.extend_from_slice(scope.expected_active_pxst_digest.as_bytes());
    payload.extend_from_slice(scope.profile_digest.as_bytes());
    payload.extend_from_slice(scope.mac_agent_client_principal.as_bytes());
    payload.extend_from_slice(scope.open_challenge.request_id.as_bytes());
    payload.extend_from_slice(scope.echo_challenge.request_id.as_bytes());
    debug_assert_eq!(payload.len(), PREPARED_FIXED_BYTES);
    payload.extend_from_slice(&open_wire);
    payload.extend_from_slice(&echo_wire);
    payload.extend_from_slice(carrier_wire);
    payload.extend_from_slice(&scope.open_challenge.auth_nonce);
    payload.extend_from_slice(&scope.echo_challenge.auth_nonce);
    Ok(payload)
}

fn decode_prepared_payload(
    attempt_id: [u8; 16],
    payload: &[u8],
) -> Result<RemoteAgentOneEchoScopeV1, RemoteAgentOutboxError> {
    if payload.len() < PREPARED_FIXED_BYTES {
        return Err(RemoteAgentOutboxError::TruncatedRecord);
    }
    let open_length = usize::try_from(read_u32(&payload[..4]))
        .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    let echo_length = usize::try_from(read_u32(&payload[4..8]))
        .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    let carrier_length = usize::from(read_u16(&payload[8..10]));
    let open_nonce_length = usize::from(read_u16(&payload[10..12]));
    let echo_nonce_length = usize::from(read_u16(&payload[12..14]));
    if read_u16(&payload[14..16]) != 0
        || open_length > MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES
        || echo_length > MAX_AGENT_CONVERSATION_FRAME_BYTES
        || carrier_length == 0
        || carrier_length > MAX_RESTRICTED_RUNTIME_APPLY_CARRIER_BINDING_BYTES
        || open_nonce_length == 0
        || open_nonce_length > MAX_APPLY_AUTH_NONCE_BYTES
        || echo_nonce_length == 0
        || echo_nonce_length > MAX_APPLY_AUTH_NONCE_BYTES
    {
        return Err(RemoteAgentOutboxError::InvalidRecordLength);
    }
    let expected = PREPARED_FIXED_BYTES
        .checked_add(open_length)
        .and_then(|value| value.checked_add(echo_length))
        .and_then(|value| value.checked_add(carrier_length))
        .and_then(|value| value.checked_add(open_nonce_length))
        .and_then(|value| value.checked_add(echo_nonce_length))
        .ok_or(RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    if expected != payload.len() {
        return Err(RemoteAgentOutboxError::InvalidRecordLength);
    }
    let target = RuntimeHostId::from_bytes(read_array(&payload[16..32]));
    let runtime_store_instance_id = read_array(&payload[32..64]);
    let runtime_host_epoch = read_u64(&payload[64..72]);
    let expected_pxau_digest = Digest32::from_bytes(read_array(&payload[72..104]));
    let expected_active_pxst_digest = Digest32::from_bytes(read_array(&payload[104..136]));
    let profile_digest = Digest32::from_bytes(read_array(&payload[136..168]));
    let mac_agent_client_principal = PrincipalRef::from_bytes(read_array(&payload[168..184]));
    let open_request_id =
        RemoteAgentAccessRequestIdV1::try_from_bytes(read_array(&payload[184..200]))
            .map_err(|_| RemoteAgentOutboxError::InvalidChallenge)?;
    let echo_request_id =
        RemoteAgentAccessRequestIdV1::try_from_bytes(read_array(&payload[200..216]))
            .map_err(|_| RemoteAgentOutboxError::InvalidChallenge)?;
    let open_start = PREPARED_FIXED_BYTES;
    let open_end = open_start + open_length;
    let echo_end = open_end + echo_length;
    let carrier_end = echo_end + carrier_length;
    let open_nonce_end = carrier_end + open_nonce_length;
    let open = AgentConversationControlV1::decode(&payload[open_start..open_end])
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?;
    let echo = AgentConversationRequestV1::decode(&payload[open_end..echo_end])
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?;
    let carrier = RestrictedRuntimeApplyCarrierBindingV1::decode(&payload[echo_end..carrier_end])
        .map_err(|_| RemoteAgentOutboxError::InvalidScope)?;
    if open
        .canonical_wire()
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?
        .as_ref()
        != &payload[open_start..open_end]
        || echo.canonical_wire().as_ref() != &payload[open_end..echo_end]
        || carrier.canonical_wire() != &payload[echo_end..carrier_end]
    {
        return Err(RemoteAgentOutboxError::NonCanonicalRecord);
    }
    let scope = RemoteAgentOneEchoScopeV1::try_new(RemoteAgentOneEchoScopeFieldsV1 {
        attempt_id,
        echo_request: echo,
        target,
        runtime_store_instance_id,
        runtime_host_epoch,
        expected_pxau_digest,
        expected_active_pxst_digest,
        profile_digest,
        mac_agent_client_principal,
        carrier,
        open_request_id,
        open_auth_nonce: &payload[carrier_end..open_nonce_end],
        echo_request_id,
        echo_auth_nonce: &payload[open_nonce_end..],
    })?;
    if scope.open_request() != open || encode_prepared_payload(&scope)?.as_slice() != payload {
        return Err(RemoteAgentOutboxError::NonCanonicalRecord);
    }
    Ok(scope)
}

fn encode_proof_payload(
    proof: &RemoteAgentDescribeProofBytesV1,
) -> Result<Vec<u8>, RemoteAgentOutboxError> {
    let mut payload = Vec::with_capacity(8 + proof.request_wire.len() + proof.response_wire.len());
    payload.extend_from_slice(
        &u32::try_from(proof.request_wire.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u32::try_from(proof.response_wire.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(&proof.request_wire);
    payload.extend_from_slice(&proof.response_wire);
    Ok(payload)
}

fn decode_proof_payload(
    payload: &[u8],
) -> Result<RemoteAgentDescribeProofBytesV1, RemoteAgentOutboxError> {
    if payload.len() < 8 {
        return Err(RemoteAgentOutboxError::TruncatedRecord);
    }
    let request_length = usize::try_from(read_u32(&payload[..4]))
        .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    let response_length = usize::try_from(read_u32(&payload[4..8]))
        .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    let expected = 8_usize
        .checked_add(request_length)
        .and_then(|value| value.checked_add(response_length))
        .ok_or(RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    if expected != payload.len()
        || request_length > MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES
        || response_length > MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES
    {
        return Err(RemoteAgentOutboxError::InvalidRecordLength);
    }
    let request_end = 8 + request_length;
    RemoteAgentDescribeProofBytesV1::try_new(&payload[8..request_end], &payload[request_end..])
}

fn validate_proof_scope_challenge(
    scope: &RemoteAgentOneEchoScopeV1,
    challenge: &RemoteAgentDescribeChallengeV1,
    proof: &RemoteAgentDescribeProofBytesV1,
) -> Result<(RemoteAgentAccessRequestV1, RemoteAgentAccessResponseV1), RemoteAgentOutboxError> {
    let request = RemoteAgentAccessRequestV1::decode(proof.request_wire())
        .map_err(|_| RemoteAgentOutboxError::InvalidDescribeProof)?;
    let response = RemoteAgentAccessResponseV1::decode(proof.response_wire())
        .map_err(|_| RemoteAgentOutboxError::InvalidDescribeProof)?;
    if request.carrier() != scope.carrier() {
        return Err(RemoteAgentOutboxError::DescribeCarrierMismatch);
    }
    if request.request_id() != challenge.request_id()
        || request.authentication().claim().nonce() != challenge.auth_nonce()
    {
        return Err(RemoteAgentOutboxError::DescribeChallengeMismatch);
    }
    if request.kind() != RemoteAgentAccessKindV1::DescribeRemoteAccess
        || request.target() != scope.target()
        || request.expected_runtime_store_instance_id() != scope.runtime_store_instance_id()
        || request.expected_runtime_host_epoch() != scope.runtime_host_epoch()
        || request.expected_pxau_digest() != scope.expected_pxau_digest()
        || request.expected_active_pxst_digest() != scope.expected_active_pxst_digest()
        || request.profile_digest() != scope.profile_digest()
        || request.intended_mac_agent_client() != scope.mac_agent_client_principal()
        || response.validate_against_request(&request).is_err()
    {
        return Err(RemoteAgentOutboxError::DescribeScopeMismatch);
    }
    Ok((request, response))
}

fn decode_open_result(
    payload: &[u8],
    deck_run_id: paraegox_agent_contracts::AgentConversationDeckRunId,
    session_id: paraegox_agent_contracts::AgentConversationSessionId,
) -> Result<AgentConversationOpenOutcomeV1, RemoteAgentOutboxError> {
    let response = AgentConversationControlV1::decode(payload)
        .map_err(|_| RemoteAgentOutboxError::InvalidOpenResult)?;
    if response.deck_run_id() != deck_run_id
        || response.session_id() != session_id
        || response.request_id().is_some()
        || response
            .canonical_wire()
            .map_err(|_| RemoteAgentOutboxError::InvalidOpenResult)?
            .as_ref()
            != payload
    {
        return Err(RemoteAgentOutboxError::InvalidOpenResult);
    }
    match response.body() {
        AgentConversationControlBodyV1::OpenResult(outcome) => Ok(*outcome),
        _ => Err(RemoteAgentOutboxError::InvalidOpenResult),
    }
}

fn encode_record(
    attempt_id: [u8; 16],
    sequence: usize,
    kind: RemoteAgentOutboxRecordKindV1,
    previous_digest: Digest32,
    payload: &[u8],
) -> Result<Vec<u8>, RemoteAgentOutboxError> {
    if bytes_are_zero(&attempt_id) {
        return Err(RemoteAgentOutboxError::InvalidAttempt);
    }
    if sequence == 0
        || sequence > MAX_REMOTE_AGENT_OUTBOX_RECORDS
        || payload.len() > kind.max_payload_bytes()
    {
        return Err(RemoteAgentOutboxError::RecordPayloadTooLarge);
    }
    let mut record = vec![0; REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES + payload.len()];
    record[..4].copy_from_slice(REMOTE_AGENT_OUTBOX_MAGIC);
    record[4..6].copy_from_slice(&REMOTE_AGENT_OUTBOX_VERSION.to_be_bytes());
    record[6..8].copy_from_slice(
        &u16::try_from(REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES)
            .map_err(|_| RemoteAgentOutboxError::InvalidRecordLength)?
            .to_be_bytes(),
    );
    record[8..10].copy_from_slice(&(kind as u16).to_be_bytes());
    record[10..12].copy_from_slice(
        &u16::try_from(sequence)
            .map_err(|_| RemoteAgentOutboxError::TooManyRecords)?
            .to_be_bytes(),
    );
    record[12..16].copy_from_slice(
        &u32::try_from(payload.len())
            .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?
            .to_be_bytes(),
    );
    record[16..32].copy_from_slice(&attempt_id);
    record[32..64].copy_from_slice(previous_digest.as_bytes());
    record[REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES..].copy_from_slice(payload);
    let digest = record_digest(&record[..64], payload)?;
    record[64..96].copy_from_slice(digest.as_bytes());
    Ok(record)
}

struct ParsedRecord<'a> {
    attempt_id: [u8; 16],
    sequence: usize,
    kind: RemoteAgentOutboxRecordKindV1,
    previous_digest: Digest32,
    record_digest: Digest32,
    payload: &'a [u8],
    wire: &'a [u8],
}

fn parse_record(frame: &[u8]) -> Result<ParsedRecord<'_>, RemoteAgentOutboxError> {
    if frame.len() < REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES {
        return Err(RemoteAgentOutboxError::TruncatedRecord);
    }
    if &frame[..4] != REMOTE_AGENT_OUTBOX_MAGIC
        || read_u16(&frame[4..6]) != REMOTE_AGENT_OUTBOX_VERSION
    {
        return Err(RemoteAgentOutboxError::UnsupportedWire);
    }
    if usize::from(read_u16(&frame[6..8])) != REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES {
        return Err(RemoteAgentOutboxError::NonCanonicalRecord);
    }
    let kind = RemoteAgentOutboxRecordKindV1::decode(read_u16(&frame[8..10]))?;
    let sequence = usize::from(read_u16(&frame[10..12]));
    let payload_length = usize::try_from(read_u32(&frame[12..16]))
        .map_err(|_| RemoteAgentOutboxError::InvalidRecordLength)?;
    let attempt_id = read_array(&frame[16..32]);
    if bytes_are_zero(&attempt_id) {
        return Err(RemoteAgentOutboxError::InvalidAttempt);
    }
    if sequence == 0
        || sequence > MAX_REMOTE_AGENT_OUTBOX_RECORDS
        || payload_length > kind.max_payload_bytes()
    {
        return Err(RemoteAgentOutboxError::InvalidRecordLength);
    }
    let record_length = REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES
        .checked_add(payload_length)
        .ok_or(RemoteAgentOutboxError::InvalidRecordLength)?;
    let wire = frame
        .get(..record_length)
        .ok_or(RemoteAgentOutboxError::TruncatedRecord)?;
    let payload = &wire[REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES..];
    let declared_digest = Digest32::from_bytes(read_array(&wire[64..96]));
    let expected_digest = record_digest(&wire[..64], payload)?;
    if declared_digest != expected_digest {
        return Err(RemoteAgentOutboxError::RecordChecksumMismatch);
    }
    Ok(ParsedRecord {
        attempt_id,
        sequence,
        kind,
        previous_digest: Digest32::from_bytes(read_array(&wire[32..64])),
        record_digest: declared_digest,
        payload,
        wire,
    })
}

fn record_digest(header: &[u8], payload: &[u8]) -> Result<Digest32, RemoteAgentOutboxError> {
    let mut builder = Digest32Builder::try_new(RECORD_DIGEST_DOMAIN)
        .map_err(|_| RemoteAgentOutboxError::DigestEncodingFailed)?;
    builder
        .field_bytes(header)
        .and_then(|value| value.field_bytes(payload))
        .map_err(|_| RemoteAgentOutboxError::DigestEncodingFailed)?;
    Ok(builder.finish())
}

fn record_digest_from_wire(record: &[u8]) -> Digest32 {
    Digest32::from_bytes(read_array(&record[64..96]))
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(read_array(bytes))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(read_array(bytes))
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(read_array(bytes))
}

fn read_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(bytes);
    value
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentOutboxMutationErrorV1 {
    State(RemoteAgentOutboxError),
    Commit(RemoteAgentOutboxCommitFailureV1),
}

impl From<RemoteAgentOutboxError> for RemoteAgentOutboxMutationErrorV1 {
    fn from(value: RemoteAgentOutboxError) -> Self {
        Self::State(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentOutboxError {
    InvalidAttempt,
    InvalidScope,
    InvalidChallenge,
    ScopeMismatch,
    ActionMismatch,
    JournalTooLarge,
    TooManyRecords,
    UnsupportedWire,
    UnsupportedRecordKind,
    TruncatedRecord,
    InvalidRecordLength,
    RecordPayloadTooLarge,
    RecordGap,
    RecordChainMismatch,
    RecordChecksumMismatch,
    NonCanonicalRecord,
    InvalidPhaseTransition,
    InvalidPreparedRequests,
    InvalidDescribeProof,
    DescribeCarrierMismatch,
    DescribeChallengeMismatch,
    DescribeScopeMismatch,
    DescribeAuthenticationFailed,
    InvalidOpenResult,
    InvalidTerminal,
    TerminalCorrelationMismatch,
    DigestEncodingFailed,
}

impl fmt::Display for RemoteAgentOutboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "remote Agent outbox rejected: {self:?}")
    }
}

impl std::error::Error for RemoteAgentOutboxError {}
