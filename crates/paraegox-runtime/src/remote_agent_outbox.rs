//! Runtime-private canonical journal for one remote Agent Open-and-Echo attempt.
//!
//! PXOJ v1 is only the bounded state codec and commit seam. The commit callback
//! is required to make one record durable before returning, but this module
//! deliberately supplies no filesystem implementation and makes no APFS
//! durability claim.

#![forbid(unsafe_code)]

use core::fmt;

use paraegox_agent_contracts::control::{
    AgentConversationControlBodyV1, AgentConversationControlV1,
    AgentConversationOpenOutcomeV1, MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES,
};
use paraegox_agent_contracts::{
    AgentConversationRequestV1, AgentConversationTerminalV1,
    MAX_AGENT_CONVERSATION_FRAME_BYTES,
};
use paraegox_kernel::digest::{Digest32, Digest32Builder};
use paraegox_runtime_contracts::remote_agent_access::{
    MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES, MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES,
    RemoteAgentAccessKindV1, RemoteAgentAccessRequestV1, RemoteAgentAccessResponseV1,
};

pub(crate) const REMOTE_AGENT_OUTBOX_MAGIC: &[u8; 4] = b"PXOJ";
pub(crate) const REMOTE_AGENT_OUTBOX_VERSION: u16 = 1;
pub(crate) const REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES: usize = 96;
pub(crate) const MAX_REMOTE_AGENT_OUTBOX_RECORDS: usize = 5;

const RECORD_DIGEST_DOMAIN: &[u8] = b"paraegox.runtime.remote-agent-outbox.record.sha256.v1";
const ZERO_DIGEST: Digest32 = Digest32::from_bytes([0; 32]);
const PREPARED_PAYLOAD_BYTES: usize = 8
    + MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES
    + MAX_AGENT_CONVERSATION_FRAME_BYTES;
const CLAIM_PAYLOAD_BYTES: usize =
    8 + MAX_REMOTE_AGENT_ACCESS_REQUEST_BYTES + MAX_REMOTE_AGENT_ACCESS_RESPONSE_BYTES;
const MAX_REMOTE_AGENT_OUTBOX_JOURNAL_BYTES: usize =
    MAX_REMOTE_AGENT_OUTBOX_RECORDS * REMOTE_AGENT_OUTBOX_RECORD_HEADER_BYTES
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteAgentOutboxPhaseV1 {
    OpenRequestDurableNotSent,
    OpenUncertain(RemoteAgentDescribeProofBytesV1),
    EchoRequestDurableNotSent {
        open_outcome: AgentConversationOpenOutcomeV1,
        open_proof: RemoteAgentDescribeProofBytesV1,
    },
    OpenTerminal(AgentConversationOpenOutcomeV1),
    EchoUncertain(RemoteAgentDescribeProofBytesV1),
    Terminal(AgentConversationTerminalV1),
}

pub(crate) trait RemoteAgentOutboxCommitV1 {
    fn commit_record(
        &mut self,
        record: &[u8],
    ) -> Result<(), RemoteAgentOutboxCommitFailureV1>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOutboxCommitFailureV1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemoteAgentOutboxV1 {
    attempt_id: [u8; 16],
    open_request: AgentConversationControlV1,
    echo_request: AgentConversationRequestV1,
    phase: RemoteAgentOutboxPhaseV1,
    journal_wire: Vec<u8>,
    last_record_digest: Digest32,
    record_count: usize,
}

impl RemoteAgentOutboxV1 {
    pub(crate) fn try_prepare<Commit>(
        attempt_id: [u8; 16],
        echo_request: AgentConversationRequestV1,
        commit: &mut Commit,
    ) -> Result<Self, RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        if bytes_are_zero(&attempt_id) {
            return Err(RemoteAgentOutboxError::InvalidAttempt.into());
        }
        let open_request = AgentConversationControlV1::open_request(
            echo_request.deck_run_id(),
            echo_request.session_id(),
        );
        let payload = encode_prepared_payload(&open_request, &echo_request)?;
        let record = encode_record(
            attempt_id,
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
            attempt_id,
            open_request,
            echo_request,
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
                    let (open_request, echo_request) = decode_prepared_payload(parsed.payload)?;
                    state = Some(Self {
                        attempt_id: parsed.attempt_id,
                        open_request,
                        echo_request,
                        phase: RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent,
                        journal_wire: parsed.wire.to_vec(),
                        last_record_digest: parsed.record_digest,
                        record_count: 1,
                    });
                }
                Some(value) => {
                    if parsed.attempt_id != value.attempt_id
                        || parsed.previous_digest != value.last_record_digest
                    {
                        return Err(RemoteAgentOutboxError::RecordChainMismatch);
                    }
                    let next_phase = value.next_phase(parsed.kind, parsed.payload)?;
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
        proof: RemoteAgentDescribeProofBytesV1,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        self.append(
            RemoteAgentOutboxRecordKindV1::OpenClaimed,
            encode_proof_payload(&proof)?,
            commit,
        )
    }

    pub(crate) fn commit_open_result<Commit>(
        &mut self,
        outcome: AgentConversationOpenOutcomeV1,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        let response = AgentConversationControlV1::open_result(
            self.open_request.deck_run_id(),
            self.open_request.session_id(),
            outcome,
        );
        self.append(
            RemoteAgentOutboxRecordKindV1::OpenResult,
            response
                .canonical_wire()
                .map_err(|_| RemoteAgentOutboxError::InvalidOpenResult)?
                .into_vec(),
            commit,
        )
    }

    pub(crate) fn claim_echo<Commit>(
        &mut self,
        proof: RemoteAgentDescribeProofBytesV1,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        self.append(
            RemoteAgentOutboxRecordKindV1::EchoClaimed,
            encode_proof_payload(&proof)?,
            commit,
        )
    }

    pub(crate) fn commit_terminal<Commit>(
        &mut self,
        terminal: AgentConversationTerminalV1,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        if !terminal.correlates(&self.echo_request) {
            return Err(RemoteAgentOutboxError::TerminalCorrelationMismatch.into());
        }
        self.append(
            RemoteAgentOutboxRecordKindV1::EchoTerminal,
            terminal.canonical_wire().into_vec(),
            commit,
        )
    }

    pub(crate) const fn attempt_id(&self) -> [u8; 16] {
        self.attempt_id
    }

    pub(crate) const fn open_request(&self) -> &AgentConversationControlV1 {
        &self.open_request
    }

    pub(crate) const fn echo_request(&self) -> &AgentConversationRequestV1 {
        &self.echo_request
    }

    pub(crate) const fn phase(&self) -> &RemoteAgentOutboxPhaseV1 {
        &self.phase
    }

    pub(crate) fn canonical_wire(&self) -> &[u8] {
        &self.journal_wire
    }

    fn append<Commit>(
        &mut self,
        kind: RemoteAgentOutboxRecordKindV1,
        payload: Vec<u8>,
        commit: &mut Commit,
    ) -> Result<(), RemoteAgentOutboxMutationErrorV1>
    where
        Commit: RemoteAgentOutboxCommitV1,
    {
        let next_phase = self.next_phase(kind, &payload)?;
        let sequence = self
            .record_count
            .checked_add(1)
            .ok_or(RemoteAgentOutboxError::TooManyRecords)?;
        if sequence > MAX_REMOTE_AGENT_OUTBOX_RECORDS {
            return Err(RemoteAgentOutboxError::TooManyRecords.into());
        }
        let record = encode_record(
            self.attempt_id,
            sequence,
            kind,
            self.last_record_digest,
            &payload,
        )?;
        commit
            .commit_record(&record)
            .map_err(RemoteAgentOutboxMutationErrorV1::Commit)?;
        self.phase = next_phase;
        self.last_record_digest = record_digest_from_wire(&record);
        self.record_count = sequence;
        self.journal_wire.extend_from_slice(&record);
        Ok(())
    }

    fn next_phase(
        &self,
        kind: RemoteAgentOutboxRecordKindV1,
        payload: &[u8],
    ) -> Result<RemoteAgentOutboxPhaseV1, RemoteAgentOutboxError> {
        match (&self.phase, kind) {
            (
                RemoteAgentOutboxPhaseV1::OpenRequestDurableNotSent,
                RemoteAgentOutboxRecordKindV1::OpenClaimed,
            ) => Ok(RemoteAgentOutboxPhaseV1::OpenUncertain(
                decode_proof_payload(payload)?,
            )),
            (
                RemoteAgentOutboxPhaseV1::OpenUncertain(open_proof),
                RemoteAgentOutboxRecordKindV1::OpenResult,
            ) => {
                let outcome = decode_open_result(
                    payload,
                    self.open_request.deck_run_id(),
                    self.open_request.session_id(),
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
                        RemoteAgentOutboxPhaseV1::OpenTerminal(outcome)
                    }
                })
            }
            (
                RemoteAgentOutboxPhaseV1::EchoRequestDurableNotSent {
                    open_outcome: AgentConversationOpenOutcomeV1::Opened,
                    ..
                },
                RemoteAgentOutboxRecordKindV1::EchoClaimed,
            ) => Ok(RemoteAgentOutboxPhaseV1::EchoUncertain(
                decode_proof_payload(payload)?,
            )),
            (
                RemoteAgentOutboxPhaseV1::EchoUncertain(_),
                RemoteAgentOutboxRecordKindV1::EchoTerminal,
            ) => {
                let terminal = AgentConversationTerminalV1::decode(payload)
                    .map_err(|_| RemoteAgentOutboxError::InvalidTerminal)?;
                if !terminal.correlates(&self.echo_request)
                    || terminal.canonical_wire().as_ref() != payload
                {
                    return Err(RemoteAgentOutboxError::TerminalCorrelationMismatch);
                }
                Ok(RemoteAgentOutboxPhaseV1::Terminal(terminal))
            }
            _ => Err(RemoteAgentOutboxError::InvalidPhaseTransition),
        }
    }
}

fn encode_prepared_payload(
    open_request: &AgentConversationControlV1,
    echo_request: &AgentConversationRequestV1,
) -> Result<Vec<u8>, RemoteAgentOutboxError> {
    if !matches!(
        open_request.body(),
        AgentConversationControlBodyV1::OpenRequest
    ) || open_request.request_id().is_some()
        || open_request.deck_run_id() != echo_request.deck_run_id()
        || open_request.session_id() != echo_request.session_id()
    {
        return Err(RemoteAgentOutboxError::InvalidPreparedRequests);
    }
    let open_wire = open_request
        .canonical_wire()
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?;
    let echo_wire = echo_request.canonical_wire();
    let mut payload = Vec::with_capacity(8 + open_wire.len() + echo_wire.len());
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
    payload.extend_from_slice(&open_wire);
    payload.extend_from_slice(&echo_wire);
    Ok(payload)
}

fn decode_prepared_payload(
    payload: &[u8],
) -> Result<(AgentConversationControlV1, AgentConversationRequestV1), RemoteAgentOutboxError> {
    if payload.len() < 8 {
        return Err(RemoteAgentOutboxError::TruncatedRecord);
    }
    let open_length = usize::try_from(read_u32(&payload[..4]))
        .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    let echo_length = usize::try_from(read_u32(&payload[4..8]))
        .map_err(|_| RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    let expected = 8_usize
        .checked_add(open_length)
        .and_then(|value| value.checked_add(echo_length))
        .ok_or(RemoteAgentOutboxError::RecordPayloadTooLarge)?;
    if expected != payload.len()
        || open_length > MAX_AGENT_CONVERSATION_CONTROL_FRAME_BYTES
        || echo_length > MAX_AGENT_CONVERSATION_FRAME_BYTES
    {
        return Err(RemoteAgentOutboxError::InvalidRecordLength);
    }
    let open_end = 8 + open_length;
    let open = AgentConversationControlV1::decode(&payload[8..open_end])
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?;
    let echo = AgentConversationRequestV1::decode(&payload[open_end..])
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?;
    if open
        .canonical_wire()
        .map_err(|_| RemoteAgentOutboxError::InvalidPreparedRequests)?
        .as_ref()
        != &payload[8..open_end]
        || echo.canonical_wire().as_ref() != &payload[open_end..]
    {
        return Err(RemoteAgentOutboxError::NonCanonicalRecord);
    }
    encode_prepared_payload(&open, &echo)?;
    Ok((open, echo))
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
    RemoteAgentDescribeProofBytesV1::try_new(
        &payload[8..request_end],
        &payload[request_end..],
    )
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
        attempt_id: read_array(&wire[16..32]),
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

fn read_array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(bytes);
    value
}

const fn bytes_are_zero(bytes: &[u8; 16]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
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
