use std::cell::Cell;

use paraegox_kernel::digest::Digest32;
use paraegox_kernel::identity::PrincipalRef;
use paraegox_kernel::time::{BoundedDuration, ClockDomainRef, ClockGeneration};

use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    DistributedFabricCredentialRefV1, DistributedFabricSessionEpochV1,
    DistributedFabricTrustAnchorRefV1, DistributedFabricTrustDomainRefV1,
};
use paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentStackApplyRequestV1;
use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
use paraegox_runtime_contracts::remote_agent_data_plane_plan::{
    MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES,
    MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES,
    MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_V2_BYTES, REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES,
    REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_VERSION, REMOTE_AGENT_DATA_PLANE_PROXY_ROUTE_COUNT_V2,
    REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_VERSION, REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES,
    RemoteAgentActiveS1CasV2, RemoteAgentActiveS1FieldsV2, RemoteAgentDataPlaneApplyRequestDraftV2,
    RemoteAgentDataPlaneApplyRequestV1, RemoteAgentDataPlaneApplyRequestV2,
    RemoteAgentDataPlaneDrainOutcomeV2, RemoteAgentDataPlanePlanError,
    RemoteAgentDataPlaneProfileFieldsV1, RemoteAgentDataPlaneProfileV1,
    RemoteAgentDataPlaneProjectionV1, RemoteAgentDataPlaneRemoteObservationV2,
    RemoteAgentDataPlaneTargetExecutionV1, RemoteAgentDataPlaneTargetExecutionV2,
    RemoteAgentDataPlaneTargetModeV2, RemoteAgentDataPlaneTerminalAuthClaimV2,
    RemoteAgentDataPlaneTerminalEvidenceFieldsV2, RemoteAgentDataPlaneTerminalEvidenceV2,
    RemoteAgentDataPlaneTerminalHeadV2, RemoteAgentDataPlaneTerminalLifecycleEffectV2,
    RemoteAgentDataPlaneTerminalOutcomeV2, RemoteAgentDataPlaneTerminalPhaseV2,
    RemoteAgentDataPlaneTerminalReceiptDraftV2, RemoteAgentDataPlaneTerminalReceiptV1,
    RemoteAgentDataPlaneTerminalReceiptV2, RemoteAgentDataPlaneTerminalStateFieldsV2,
    RemoteAgentDataPlaneTerminalStateV2, RemoteAgentRetainedS0CasFieldsV2,
    RemoteAgentRetainedS0CasV2, remote_agent_proxy_topology_compatibility_digest_v2,
    verify_remote_agent_data_plane_durable_slice_v2,
};
use paraegox_runtime_contracts::temporal::ApplyTemporalConstraint;
use paraegox_runtime_contracts::wire::{
    ApplyAuthAlgorithm, ApplyAuthKeyRef, ApplyRequestAuthClaim,
};

const AGENT_STACK_FIXTURE: &str =
    include_str!("../../../tests/fixtures/wire/s7_managed_agent_stack_successor_v1.json");
const DATA_PLANE_V1_FIXTURE: &str =
    include_str!("../../../tests/fixtures/wire/t2_remote_agent_data_plane_v1.json");
const PROXY_DATA_PLANE_V2_GOLDEN: &str =
    include_str!("../../../tests/fixtures/wire/t2_remote_agent_proxy_data_plane_v2.json");
const EMPTY_PXTA: &[u8; 10] = b"PXTA\0\x01\0\0\0\0";
const OPERATION_TIMEOUT_NANOS: u64 = 5_000_000_000;
const ORIGINAL_BUDGET_NANOS: u64 = OPERATION_TIMEOUT_NANOS * 2;
const ADMITTED_AT_NANOS: u64 = 1_000;
const EXACT_ROUTE_BITMAP: u8 = 0b11;
const FABRIC_GENERATION: u64 = 7;
const AGENT_GENERATION: u64 = 8;
const ACTIVE_PRIOR_HIGH_WATER: u64 = 32;
const ACTIVE_PRIOR_SLOT_REVISION: u64 = 41;
const LOCAL_PRIOR_HIGH_WATER: u64 = 33;
const LOCAL_PRIOR_SLOT_REVISION: u64 = 42;
const FABRIC_SESSION_EPOCH: [u8; 16] = [0xa7; 16];
const ACTIVE_PROXY_SESSION_EPOCH: [u8; 16] = [0xb7; 16];

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("non-hex fixture byte"),
    }
}

fn fixture_string_after<'a>(fixture: &'a str, section: &str, key: &str) -> &'a str {
    let section_start = fixture.find(section).expect("fixture section");
    let key_start = fixture[section_start..]
        .find(key)
        .map(|offset| section_start + offset + key.len())
        .expect("fixture key");
    let quote_start = fixture[key_start..]
        .find('"')
        .map(|offset| key_start + offset + 1)
        .expect("fixture quote");
    let quote_end = fixture[quote_start..]
        .find('"')
        .map(|offset| quote_start + offset)
        .expect("fixture quote end");
    &fixture[quote_start..quote_end]
}

fn fixture_hex_after(fixture: &str, section: &str, key: &str) -> Vec<u8> {
    fixture_string_after(fixture, section, key)
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn fixture_digest_after(fixture: &str, section: &str, key: &str) -> Digest32 {
    Digest32::from_bytes(
        fixture_hex_after(fixture, section, key)
            .try_into()
            .expect("32-byte fixture digest"),
    )
}

fn fixture_u64_after(fixture: &str, section: &str, key: &str) -> u64 {
    let section_start = fixture.find(section).expect("fixture section");
    let key_start = fixture[section_start..]
        .find(key)
        .map(|offset| section_start + offset + key.len())
        .expect("fixture key");
    let value = fixture[key_start..]
        .trim_start_matches(|character: char| character == ':' || character.is_whitespace());
    let end = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    value[..end].parse().expect("fixture integer")
}

fn fixture_section_after<'a>(fixture: &'a str, section: &str) -> &'a str {
    let section_start = fixture.find(section).expect("fixture section");
    &fixture[section_start..]
}

fn temporal_with_remaining(
    authority: ApplyTemporalConstraint,
    remaining_budget_nanos: u64,
) -> ApplyTemporalConstraint {
    ApplyTemporalConstraint::try_new(
        authority.constraint_id(),
        authority.target_clock_domain(),
        authority.target_clock_generation(),
        BoundedDuration::from_nanos(ORIGINAL_BUDGET_NANOS),
        BoundedDuration::from_nanos(remaining_budget_nanos),
    )
    .expect("test temporal budget")
}

fn overwrite_pxar_envelope_u64_tlv(frame: &mut [u8], tag: u16, value: u64) {
    let envelope_length =
        u32::from_be_bytes(frame[6..10].try_into().expect("PXAR v11 envelope length")) as usize;
    let envelope = &mut frame[18..18 + envelope_length];
    let mut header = [0_u8; 6];
    header[..2].copy_from_slice(&tag.to_be_bytes());
    header[2..].copy_from_slice(&8_u32.to_be_bytes());
    let header_offset = envelope
        .windows(header.len())
        .position(|candidate| candidate == header)
        .expect("PXAR v11 envelope u64 TLV");
    let value_start = header_offset + header.len();
    envelope[value_start..value_start + 8].copy_from_slice(&value.to_be_bytes());
}

fn managed_agent_request() -> ManagedAgentStackApplyRequestV1 {
    ManagedAgentStackApplyRequestV1::decode(&fixture_hex_after(
        AGENT_STACK_FIXTURE,
        "\"fabric_and_agent\"",
        "\"outer_v7_hex\"",
    ))
    .expect("managed Agent-stack PXAR v7 fixture")
}

fn generation(value: u64) -> ManagedServiceGeneration {
    ManagedServiceGeneration::try_new(value).expect("nonzero generation")
}

fn fabric_session_epoch(bytes: [u8; 16]) -> DistributedFabricSessionEpochV1 {
    DistributedFabricSessionEpochV1::try_from_bytes(bytes).expect("nonzero Fabric session epoch")
}

fn projection_for(request: &ManagedAgentStackApplyRequestV1) -> RemoteAgentDataPlaneProjectionV1 {
    RemoteAgentDataPlaneProjectionV1::try_from_managed_agent_stack_projection(
        request.target_execution().projection().clone(),
    )
    .expect("PXAE projection")
}

fn profile_for(request: &ManagedAgentStackApplyRequestV1) -> RemoteAgentDataPlaneProfileV1 {
    RemoteAgentDataPlaneProfileV1::try_new(RemoteAgentDataPlaneProfileFieldsV1 {
        target: request.target(),
        base_loopback_listen_endpoint: "tcp/127.0.0.1:7447",
        ubuntu_tls_listener_endpoint: "tls/192.0.2.10:7447",
        endpoint_ref: [0x91; 16],
        endpoint_generation: 101,
        trust_domain_ref: DistributedFabricTrustDomainRefV1::try_from_bytes([0x92; 16])
            .expect("trust-domain ref"),
        trust_anchor_ref: DistributedFabricTrustAnchorRefV1::try_from_bytes([0x93; 16])
            .expect("trust-anchor ref"),
        mac_connector_credential_ref: DistributedFabricCredentialRefV1::try_from_bytes([0x94; 16])
            .expect("Mac credential ref"),
        ubuntu_listener_credential_ref: DistributedFabricCredentialRefV1::try_from_bytes(
            [0x95; 16],
        )
        .expect("Ubuntu credential ref"),
        mac_agent_client_principal: PrincipalRef::from_bytes([0xa1; 16]),
        ubuntu_agent_listener_principal: PrincipalRef::from_bytes([0xa2; 16]),
        operation_timeout_nanos: OPERATION_TIMEOUT_NANOS,
    })
    .expect("PXAD profile")
}

fn retained_s0_fields() -> RemoteAgentRetainedS0CasFieldsV2 {
    RemoteAgentRetainedS0CasFieldsV2 {
        expected_active_pxft_digest: Digest32::from_bytes([0x11; 32]),
        expected_active_pxst_digest: Digest32::from_bytes([0x22; 32]),
        expected_descriptor_evidence_record_digest: Digest32::from_bytes([0x33; 32]),
        expected_descriptor_evidence_record_sequence: 17,
        expected_descriptor_receipt_digest: Digest32::from_bytes([0x44; 32]),
        expected_descriptor_payload_digest: Digest32::from_bytes([0x55; 32]),
        expected_fabric_session_epoch: fabric_session_epoch(FABRIC_SESSION_EPOCH),
        expected_fabric_generation: generation(FABRIC_GENERATION),
        expected_agent_generation: generation(AGENT_GENERATION),
    }
}

fn retained_s0_cas() -> RemoteAgentRetainedS0CasV2 {
    RemoteAgentRetainedS0CasV2::try_new(retained_s0_fields()).expect("retained S0 CAS")
}

fn active_s1_fields() -> RemoteAgentActiveS1FieldsV2 {
    RemoteAgentActiveS1FieldsV2 {
        active_pxau_digest: Digest32::from_bytes([0x61; 32]),
        active_request_digest: Digest32::from_bytes([0x62; 32]),
        active_snapshot_digest: Digest32::from_bytes([0x63; 32]),
        active_snapshot_sequence: 29,
        active_access_generation: generation(LOCAL_PRIOR_HIGH_WATER),
        active_proxy_session_epoch: ACTIVE_PROXY_SESSION_EPOCH,
    }
}

fn expected_s1_cas(mode: RemoteAgentDataPlaneTargetModeV2) -> RemoteAgentActiveS1CasV2 {
    match mode {
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => {
            RemoteAgentActiveS1CasV2::try_expect_absent(
                ACTIVE_PRIOR_HIGH_WATER,
                ACTIVE_PRIOR_SLOT_REVISION,
            )
            .expect("absent S1 CAS")
        }
        RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => {
            RemoteAgentActiveS1CasV2::try_expect_active(
                LOCAL_PRIOR_HIGH_WATER,
                LOCAL_PRIOR_SLOT_REVISION,
                active_s1_fields(),
            )
            .expect("active S1 CAS")
        }
    }
}

fn target_execution_for(
    request: &ManagedAgentStackApplyRequestV1,
    mode: RemoteAgentDataPlaneTargetModeV2,
) -> RemoteAgentDataPlaneTargetExecutionV2 {
    let projection = projection_for(request);
    let predecessor = request.target_execution().clone();
    let retained = retained_s0_cas();
    let expected_s1 = expected_s1_cas(mode);
    let profile = profile_for(request);
    match mode {
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive => {
            RemoteAgentDataPlaneTargetExecutionV2::try_remote_access_active(
                projection,
                predecessor,
                retained,
                expected_s1,
                profile,
            )
            .expect("active PXTE v10")
        }
        RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate => {
            RemoteAgentDataPlaneTargetExecutionV2::try_local_agent_only_deactivate(
                projection,
                predecessor,
                retained,
                expected_s1,
                profile,
            )
            .expect("local-only PXTE v10")
        }
    }
}

fn data_plane_request(
    mode: RemoteAgentDataPlaneTargetModeV2,
) -> RemoteAgentDataPlaneApplyRequestV2 {
    data_plane_request_with_signature(mode, &[0xc1; 64])
}

fn data_plane_request_with_signature(
    mode: RemoteAgentDataPlaneTargetModeV2,
    signature: &[u8],
) -> RemoteAgentDataPlaneApplyRequestV2 {
    let predecessor = managed_agent_request();
    let temporal = temporal_with_remaining(predecessor.temporal(), OPERATION_TIMEOUT_NANOS);
    RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
        target_execution_for(&predecessor, mode),
        predecessor.provenance(),
        predecessor.control_commitment().control().clone(),
        temporal,
        predecessor.expected_runtime_store_instance_id(),
        predecessor.authentication().claim().clone(),
    )
    .expect("PXAR v11 draft")
    .finalize(signature)
    .expect("PXAR v11")
}

fn terminal_auth() -> RemoteAgentDataPlaneTerminalAuthClaimV2 {
    RemoteAgentDataPlaneTerminalAuthClaimV2::try_new(
        PrincipalRef::from_bytes([0xb1; 16]),
        ApplyAuthKeyRef::from_bytes([0xb4; 16]),
        ApplyAuthAlgorithm::try_new(9).expect("inner algorithm"),
        2,
    )
    .expect("PXAU v2 auth claim")
}

fn wrong_terminal_auth() -> RemoteAgentDataPlaneTerminalAuthClaimV2 {
    RemoteAgentDataPlaneTerminalAuthClaimV2::try_new(
        PrincipalRef::from_bytes([0xb2; 16]),
        ApplyAuthKeyRef::from_bytes([0xb5; 16]),
        ApplyAuthAlgorithm::try_new(9).expect("inner algorithm"),
        2,
    )
    .expect("wrong PXAU v2 auth claim")
}

#[derive(Clone, Copy)]
struct TerminalStateFixture {
    outcome: RemoteAgentDataPlaneTerminalOutcomeV2,
    lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2,
    phase: RemoteAgentDataPlaneTerminalPhaseV2,
    head: RemoteAgentDataPlaneTerminalHeadV2,
    fabric_generation: Option<u64>,
    agent_generation: Option<u64>,
    access_generation: Option<u64>,
    fabric_epoch: Option<[u8; 16]>,
    proxy_epoch: Option<[u8; 16]>,
}

fn terminal_state(fixture: TerminalStateFixture) -> RemoteAgentDataPlaneTerminalStateV2 {
    RemoteAgentDataPlaneTerminalStateV2::try_new(RemoteAgentDataPlaneTerminalStateFieldsV2 {
        outcome: fixture.outcome,
        lifecycle_effect: fixture.lifecycle_effect,
        phase: fixture.phase,
        head: fixture.head,
        fabric_generation: fixture.fabric_generation.map(generation),
        agent_generation: fixture.agent_generation.map(generation),
        access_generation: fixture.access_generation.map(generation),
        fabric_session_epoch: fixture.fabric_epoch.map(fabric_session_epoch),
        proxy_session_epoch: fixture.proxy_epoch,
    })
    .expect("structural PXAU v2 state")
}

fn active_ready_state() -> RemoteAgentDataPlaneTerminalStateV2 {
    terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::ReadyObservation,
        head: RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
        fabric_generation: Some(FABRIC_GENERATION),
        agent_generation: Some(AGENT_GENERATION),
        access_generation: Some(ACTIVE_PRIOR_HIGH_WATER + 1),
        fabric_epoch: Some(FABRIC_SESSION_EPOCH),
        proxy_epoch: Some([0xc7; 16]),
    })
}

fn local_only_ready_state() -> RemoteAgentDataPlaneTerminalStateV2 {
    terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::LocalOnlyObservation,
        head: RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
        fabric_generation: Some(FABRIC_GENERATION),
        agent_generation: Some(AGENT_GENERATION),
        access_generation: None,
        fabric_epoch: Some(FABRIC_SESSION_EPOCH),
        proxy_epoch: None,
    })
}

fn no_effect_state() -> RemoteAgentDataPlaneTerminalStateV2 {
    terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::ProvenNotStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::PreparedNoEffects,
        head: RemoteAgentDataPlaneTerminalHeadV2::PreservedNone,
        fabric_generation: None,
        agent_generation: None,
        access_generation: None,
        fabric_epoch: None,
        proxy_epoch: None,
    })
}

fn uncertain_state_at(
    phase: RemoteAgentDataPlaneTerminalPhaseV2,
) -> RemoteAgentDataPlaneTerminalStateV2 {
    terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase,
        head: RemoteAgentDataPlaneTerminalHeadV2::PreservedNone,
        fabric_generation: None,
        agent_generation: None,
        access_generation: None,
        fabric_epoch: None,
        proxy_epoch: None,
    })
}

fn uncertain_state() -> RemoteAgentDataPlaneTerminalStateV2 {
    uncertain_state_at(RemoteAgentDataPlaneTerminalPhaseV2::S1OpenIntent)
}

fn quarantined_state(
    request: &RemoteAgentDataPlaneApplyRequestV2,
) -> RemoteAgentDataPlaneTerminalStateV2 {
    terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::QuarantineIntent,
        head: RemoteAgentDataPlaneTerminalHeadV2::PreservedExisting(request.target_slice_digest()),
        fabric_generation: Some(FABRIC_GENERATION),
        agent_generation: Some(AGENT_GENERATION),
        access_generation: Some(LOCAL_PRIOR_HIGH_WATER),
        fabric_epoch: Some(FABRIC_SESSION_EPOCH),
        proxy_epoch: Some(ACTIVE_PROXY_SESSION_EPOCH),
    })
}

fn base_evidence(
    request: &RemoteAgentDataPlaneApplyRequestV2,
) -> RemoteAgentDataPlaneTerminalEvidenceFieldsV2 {
    RemoteAgentDataPlaneTerminalEvidenceFieldsV2 {
        retained_s0_current_cas_digest: request.target_execution().retained_s0_cas().cas_digest(),
        retained_s0_census_before_digest: Digest32::from_bytes([0xd1; 32]),
        retained_s0_census_after_digest: Digest32::from_bytes([0xd1; 32]),
        proxy_topology_compatibility_digest: request
            .target_execution()
            .proxy_topology_compatibility_digest(),
        resource_census_digest: Digest32::from_bytes([0xd2; 32]),
        raw_outcome_digest: Digest32::from_bytes([0xd3; 32]),
        submit_admitted_count: 0,
        submit_terminalized_count: 0,
        control_admitted_count: 0,
        control_terminalized_count: 0,
        access_generation_high_water: request
            .target_execution()
            .expected_s1_cas()
            .access_generation_high_water(),
        completion_runtime_host_epoch: 23,
        completion_snapshot_sequence: 29,
        completion_owner_slot_revision: request
            .target_execution()
            .expected_s1_cas()
            .owner_slot_revision(),
        selection_clock_domain: request.temporal().target_clock_domain(),
        selection_clock_generation: request.temporal().target_clock_generation(),
        admitted_at_nanos: ADMITTED_AT_NANOS,
        absolute_deadline_nanos: ADMITTED_AT_NANOS + OPERATION_TIMEOUT_NANOS,
        selection_observed_at_nanos: ADMITTED_AT_NANOS + 1,
        physical_binding_census: 0,
        queryable_declared_bitmap: 0,
        ingress_fenced_bitmap: 0,
        worker_joined_bitmap: 0,
        drain_outcome: RemoteAgentDataPlaneDrainOutcomeV2::NotStarted,
        remote_observation: RemoteAgentDataPlaneRemoteObservationV2::Unknown,
        retained_s0_census_complete: false,
        retained_s0_ready: false,
        s1_tls_ready: false,
        s1_acl_ready: false,
        s1_closed: false,
        s1_listener_released: false,
        quarantined: false,
    }
}

fn active_ready_evidence(
    request: &RemoteAgentDataPlaneApplyRequestV2,
) -> RemoteAgentDataPlaneTerminalEvidenceFieldsV2 {
    let mut fields = base_evidence(request);
    fields.access_generation_high_water = ACTIVE_PRIOR_HIGH_WATER + 1;
    fields.completion_owner_slot_revision = ACTIVE_PRIOR_SLOT_REVISION + 1;
    fields.physical_binding_census = 2;
    fields.queryable_declared_bitmap = EXACT_ROUTE_BITMAP;
    fields.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::S1TlsExactRoutesReady;
    fields.retained_s0_census_complete = true;
    fields.retained_s0_ready = true;
    fields.s1_tls_ready = true;
    fields.s1_acl_ready = true;
    fields
}

fn local_only_ready_evidence(
    request: &RemoteAgentDataPlaneApplyRequestV2,
) -> RemoteAgentDataPlaneTerminalEvidenceFieldsV2 {
    let mut fields = base_evidence(request);
    fields.access_generation_high_water = LOCAL_PRIOR_HIGH_WATER;
    fields.completion_owner_slot_revision = LOCAL_PRIOR_SLOT_REVISION + 1;
    fields.submit_admitted_count = 2;
    fields.submit_terminalized_count = 2;
    fields.control_admitted_count = 1;
    fields.control_terminalized_count = 1;
    fields.physical_binding_census = 2;
    fields.queryable_declared_bitmap = EXACT_ROUTE_BITMAP;
    fields.ingress_fenced_bitmap = EXACT_ROUTE_BITMAP;
    fields.worker_joined_bitmap = EXACT_ROUTE_BITMAP;
    fields.drain_outcome = RemoteAgentDataPlaneDrainOutcomeV2::Drained;
    fields.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::S1Absent;
    fields.retained_s0_census_complete = true;
    fields.retained_s0_ready = true;
    fields.s1_closed = true;
    fields.s1_listener_released = true;
    fields
}

fn quarantined_evidence(
    request: &RemoteAgentDataPlaneApplyRequestV2,
) -> RemoteAgentDataPlaneTerminalEvidenceFieldsV2 {
    let mut fields = base_evidence(request);
    fields.access_generation_high_water = LOCAL_PRIOR_HIGH_WATER;
    fields.completion_owner_slot_revision = LOCAL_PRIOR_SLOT_REVISION + 1;
    fields.submit_admitted_count = 1;
    fields.queryable_declared_bitmap = 0b01;
    fields.ingress_fenced_bitmap = EXACT_ROUTE_BITMAP;
    fields.drain_outcome = RemoteAgentDataPlaneDrainOutcomeV2::OutcomeUncertain;
    fields.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::PartialOrConflicting;
    fields.s1_tls_ready = true;
    fields.quarantined = true;
    fields
}

fn terminal_draft(
    request: &RemoteAgentDataPlaneApplyRequestV2,
    state: RemoteAgentDataPlaneTerminalStateV2,
    fields: RemoteAgentDataPlaneTerminalEvidenceFieldsV2,
) -> Result<RemoteAgentDataPlaneTerminalReceiptDraftV2, RemoteAgentDataPlanePlanError> {
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV2::try_new(fields)?;
    RemoteAgentDataPlaneTerminalReceiptDraftV2::try_new(request, state, evidence, terminal_auth())
}

fn terminal_receipt(
    request: &RemoteAgentDataPlaneApplyRequestV2,
    state: RemoteAgentDataPlaneTerminalStateV2,
    fields: RemoteAgentDataPlaneTerminalEvidenceFieldsV2,
) -> RemoteAgentDataPlaneTerminalReceiptV2 {
    terminal_draft(request, state, fields)
        .expect("valid PXAU v2 matrix member")
        .finalize(&[0xe1; 64])
        .expect("signed PXAU v2")
}

#[test]
fn retained_s0_and_active_s1_cas_are_fixed_width_canonical_and_offset_exact() {
    let retained = retained_s0_cas();
    let wire = retained.canonical_wire();
    assert_eq!(wire.len(), REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES);
    assert_eq!(&wire[0..32], &[0x11; 32]);
    assert_eq!(&wire[32..64], &[0x22; 32]);
    assert_eq!(&wire[64..96], &[0x33; 32]);
    assert_eq!(&wire[96..104], &17_u64.to_be_bytes());
    assert_eq!(&wire[104..136], &[0x44; 32]);
    assert_eq!(&wire[136..168], &[0x55; 32]);
    assert_eq!(&wire[168..184], &FABRIC_SESSION_EPOCH);
    assert_eq!(&wire[184..192], &FABRIC_GENERATION.to_be_bytes());
    assert_eq!(&wire[192..200], &AGENT_GENERATION.to_be_bytes());
    assert_eq!(RemoteAgentRetainedS0CasV2::decode(wire).unwrap(), retained);

    for range in [0..32, 32..64, 64..96, 104..136, 136..168, 168..184] {
        let mut zeroed = wire.to_vec();
        zeroed[range].fill(0);
        assert!(RemoteAgentRetainedS0CasV2::decode(&zeroed).is_err());
    }
    let mut zero_sequence = wire.to_vec();
    zero_sequence[96..104].fill(0);
    assert!(RemoteAgentRetainedS0CasV2::decode(&zero_sequence).is_err());
    let mut zero_generation = wire.to_vec();
    zero_generation[184..192].fill(0);
    assert!(RemoteAgentRetainedS0CasV2::decode(&zero_generation).is_err());

    let absent = expected_s1_cas(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let absent_wire = absent.canonical_wire();
    assert_eq!(absent_wire.len(), REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES);
    assert_eq!(absent_wire[0], 0);
    assert_eq!(&absent_wire[1..8], &[0; 7]);
    assert_eq!(&absent_wire[8..16], &ACTIVE_PRIOR_HIGH_WATER.to_be_bytes());
    assert_eq!(
        &absent_wire[16..24],
        &ACTIVE_PRIOR_SLOT_REVISION.to_be_bytes()
    );
    assert_eq!(&absent_wire[24..152], &[0; 128]);
    assert_eq!(
        RemoteAgentActiveS1CasV2::decode(absent_wire).unwrap(),
        absent
    );

    let active = expected_s1_cas(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);
    let active_wire = active.canonical_wire();
    assert_eq!(active_wire[0], 1);
    assert_eq!(&active_wire[1..8], &[0; 7]);
    assert_eq!(&active_wire[8..16], &LOCAL_PRIOR_HIGH_WATER.to_be_bytes());
    assert_eq!(
        &active_wire[16..24],
        &LOCAL_PRIOR_SLOT_REVISION.to_be_bytes()
    );
    assert_eq!(&active_wire[24..56], &[0x61; 32]);
    assert_eq!(&active_wire[56..88], &[0x62; 32]);
    assert_eq!(&active_wire[88..120], &[0x63; 32]);
    assert_eq!(&active_wire[120..128], &29_u64.to_be_bytes());
    assert_eq!(
        &active_wire[128..136],
        &LOCAL_PRIOR_HIGH_WATER.to_be_bytes()
    );
    assert_eq!(&active_wire[136..152], &ACTIVE_PROXY_SESSION_EPOCH);
    assert_eq!(
        RemoteAgentActiveS1CasV2::decode(active_wire).unwrap(),
        active
    );

    assert!(RemoteAgentActiveS1CasV2::try_expect_absent(0, 0).is_err());
    assert!(
        RemoteAgentActiveS1CasV2::try_expect_active(
            LOCAL_PRIOR_HIGH_WATER + 1,
            LOCAL_PRIOR_SLOT_REVISION,
            active_s1_fields(),
        )
        .is_err()
    );
    let mut reserved = absent_wire.to_vec();
    reserved[1] = 1;
    assert!(RemoteAgentActiveS1CasV2::decode(&reserved).is_err());
    let mut false_present = absent_wire.to_vec();
    false_present[0] = 1;
    assert!(RemoteAgentActiveS1CasV2::decode(&false_present).is_err());
}

#[test]
fn pxte10_modes_reject_wrong_s1_authority_and_generation_or_revision_exhaustion() {
    let predecessor = managed_agent_request();
    let projection = projection_for(&predecessor);
    let profile = profile_for(&predecessor);
    let retained = retained_s0_cas();
    let absent = expected_s1_cas(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let active = expected_s1_cas(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);

    assert!(
        RemoteAgentDataPlaneTargetExecutionV2::try_remote_access_active(
            projection.clone(),
            predecessor.target_execution().clone(),
            retained,
            active,
            profile.clone(),
        )
        .is_err()
    );
    assert!(
        RemoteAgentDataPlaneTargetExecutionV2::try_local_agent_only_deactivate(
            projection.clone(),
            predecessor.target_execution().clone(),
            retained,
            absent,
            profile.clone(),
        )
        .is_err()
    );

    let exhausted_generation =
        RemoteAgentActiveS1CasV2::try_expect_absent(u64::MAX, 1).expect("bounded absent CAS");
    assert!(
        RemoteAgentDataPlaneTargetExecutionV2::try_remote_access_active(
            projection.clone(),
            predecessor.target_execution().clone(),
            retained,
            exhausted_generation,
            profile.clone(),
        )
        .is_err()
    );
    let exhausted_revision =
        RemoteAgentActiveS1CasV2::try_expect_absent(0, u64::MAX).expect("bounded absent CAS");
    assert!(
        RemoteAgentDataPlaneTargetExecutionV2::try_remote_access_active(
            projection,
            predecessor.target_execution().clone(),
            retained,
            exhausted_revision,
            profile,
        )
        .is_err()
    );
}

#[test]
fn pxte10_pxar11_round_trip_cross_reject_predecessors_and_verify_durable_slice() {
    assert_eq!(REMOTE_AGENT_DATA_PLANE_PROXY_ROUTE_COUNT_V2, 2);
    assert_ne!(
        remote_agent_proxy_topology_compatibility_digest_v2().unwrap(),
        Digest32::from_bytes([0; 32])
    );

    for mode in [
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
        RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
    ] {
        let request = data_plane_request(mode);
        let execution = request.target_execution();
        assert_eq!(execution.mode(), mode);
        assert_eq!(&execution.canonical_wire()[..4], b"PXTE");
        assert_eq!(
            &execution.canonical_wire()[4..6],
            &REMOTE_AGENT_DATA_PLANE_TARGET_EXECUTION_V2_VERSION.to_be_bytes()
        );
        assert_eq!(
            execution.predecessor().canonical_wire(),
            managed_agent_request().target_execution().canonical_wire()
        );
        assert_eq!(
            RemoteAgentDataPlaneTargetExecutionV2::decode(execution.canonical_wire()).unwrap(),
            *execution
        );
        assert!(RemoteAgentDataPlaneTargetExecutionV1::decode(execution.canonical_wire()).is_err());

        assert_eq!(&request.canonical_wire()[..4], b"PXAR");
        assert_eq!(
            &request.canonical_wire()[4..6],
            &REMOTE_AGENT_DATA_PLANE_APPLY_REQUEST_V2_VERSION.to_be_bytes()
        );
        assert_eq!(
            &request.canonical_slice_wire()[..EMPTY_PXTA.len()],
            EMPTY_PXTA
        );
        assert_eq!(
            RemoteAgentDataPlaneApplyRequestV2::decode(request.canonical_wire()).unwrap(),
            request
        );
        assert!(RemoteAgentDataPlaneApplyRequestV1::decode(request.canonical_wire()).is_err());

        let reconstructed = verify_remote_agent_data_plane_durable_slice_v2(
            request.canonical_slice_wire(),
            request.target(),
            request.provenance(),
            request.target_slice_digest(),
            request.target_execution().projection(),
        )
        .expect("durable PXTA-zero plus PXTE-v10 slice");
        assert_eq!(reconstructed, *request.target_execution());

        let mut tampered_slice = request.canonical_slice_wire().to_vec();
        tampered_slice[5] ^= 1;
        assert!(
            verify_remote_agent_data_plane_durable_slice_v2(
                &tampered_slice,
                request.target(),
                request.provenance(),
                request.target_slice_digest(),
                request.target_execution().projection(),
            )
            .is_err()
        );
    }

    let old_pxte = fixture_hex_after(DATA_PLANE_V1_FIXTURE, "\"data_plane\"", "\"pxte_v9_hex\"");
    assert!(RemoteAgentDataPlaneTargetExecutionV1::decode(&old_pxte).is_ok());
    assert!(RemoteAgentDataPlaneTargetExecutionV2::decode(&old_pxte).is_err());

    let old_pxar = fixture_hex_after(DATA_PLANE_V1_FIXTURE, "\"data_plane\"", "\"pxar_v10_hex\"");
    assert!(RemoteAgentDataPlaneApplyRequestV1::decode(&old_pxar).is_ok());
    assert!(RemoteAgentDataPlaneApplyRequestV2::decode(&old_pxar).is_err());
}

#[test]
fn successor_decoders_reject_truncation_trailing_reserved_lengths_and_unknown_tags() {
    let retained = retained_s0_cas();
    assert!(
        RemoteAgentRetainedS0CasV2::decode(
            &retained.canonical_wire()[..REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES - 1],
        )
        .is_err()
    );
    let mut retained_trailing = retained.canonical_wire().to_vec();
    retained_trailing.push(0);
    assert!(RemoteAgentRetainedS0CasV2::decode(&retained_trailing).is_err());

    let active_s1 = expected_s1_cas(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);
    assert!(
        RemoteAgentActiveS1CasV2::decode(
            &active_s1.canonical_wire()[..REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES - 1],
        )
        .is_err()
    );
    let mut active_s1_trailing = active_s1.canonical_wire().to_vec();
    active_s1_trailing.push(0);
    assert!(RemoteAgentActiveS1CasV2::decode(&active_s1_trailing).is_err());

    let request = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let execution = request.target_execution();
    assert!(
        RemoteAgentDataPlaneTargetExecutionV2::decode(
            &execution.canonical_wire()[..execution.canonical_wire().len() - 1],
        )
        .is_err()
    );
    let mut execution_trailing = execution.canonical_wire().to_vec();
    execution_trailing.push(0);
    assert!(RemoteAgentDataPlaneTargetExecutionV2::decode(&execution_trailing).is_err());

    let mode_offset = 6 + execution.projection().canonical_wire().len() + 32 + 2;
    let mut unknown_mode = execution.canonical_wire().to_vec();
    unknown_mode[mode_offset] = 0xff;
    assert!(RemoteAgentDataPlaneTargetExecutionV2::decode(&unknown_mode).is_err());
    let mut unknown_profile_presence = execution.canonical_wire().to_vec();
    unknown_profile_presence[mode_offset + 1] = 0xff;
    assert!(RemoteAgentDataPlaneTargetExecutionV2::decode(&unknown_profile_presence).is_err());
    let mut wrong_compatibility = execution.canonical_wire().to_vec();
    wrong_compatibility[6 + execution.projection().canonical_wire().len()] ^= 1;
    assert!(RemoteAgentDataPlaneTargetExecutionV2::decode(&wrong_compatibility).is_err());

    assert!(
        RemoteAgentDataPlaneApplyRequestV2::decode(
            &request.canonical_wire()[..request.canonical_wire().len() - 1],
        )
        .is_err()
    );
    let mut request_trailing = request.canonical_wire().to_vec();
    request_trailing.push(0);
    assert!(RemoteAgentDataPlaneApplyRequestV2::decode(&request_trailing).is_err());
    let mut wrong_empty_assignment_length = request.canonical_wire().to_vec();
    wrong_empty_assignment_length[10..14].copy_from_slice(&11_u32.to_be_bytes());
    assert!(RemoteAgentDataPlaneApplyRequestV2::decode(&wrong_empty_assignment_length).is_err());
    let mut wrong_execution_length = request.canonical_wire().to_vec();
    let execution_length = u32::from_be_bytes(
        wrong_execution_length[14..18]
            .try_into()
            .expect("PXAR v11 execution length"),
    );
    wrong_execution_length[14..18].copy_from_slice(&(execution_length + 1).to_be_bytes());
    assert!(RemoteAgentDataPlaneApplyRequestV2::decode(&wrong_execution_length).is_err());

    let receipt = terminal_receipt(
        &request,
        active_ready_state(),
        active_ready_evidence(&request),
    );
    assert_eq!(
        receipt.canonical_wire().len() - receipt.authentication_signature().len(),
        683
    );
    assert!(
        RemoteAgentDataPlaneTerminalReceiptV2::decode(
            &receipt.canonical_wire()[..receipt.canonical_wire().len() - 1],
        )
        .is_err()
    );
    let mut receipt_trailing = receipt.canonical_wire().to_vec();
    receipt_trailing.push(0);
    assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&receipt_trailing).is_err());

    for offset in [230, 231, 232, 233, 234, 640, 641] {
        let mut unknown_tag = receipt.canonical_wire().to_vec();
        unknown_tag[offset] = 0;
        assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&unknown_tag).is_err());
    }
    let mut nonzero_reserved_u16 = receipt.canonical_wire().to_vec();
    nonzero_reserved_u16[236] = 1;
    assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&nonzero_reserved_u16).is_err());
    let mut nonzero_reserved_byte = receipt.canonical_wire().to_vec();
    nonzero_reserved_byte[642] = 1;
    assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&nonzero_reserved_byte).is_err());
    let mut unknown_evidence_flag = receipt.canonical_wire().to_vec();
    unknown_evidence_flag[644] |= 0x80;
    assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&unknown_evidence_flag).is_err());
}

#[test]
fn pxar11_public_pipeline_enforces_profile_operation_timeout_budget() {
    let predecessor = managed_agent_request();
    let execution = target_execution_for(
        &predecessor,
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
    );
    assert_eq!(
        execution.profile().operation_timeout_nanos(),
        OPERATION_TIMEOUT_NANOS
    );
    let draft_with_temporal = |temporal| {
        RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
            execution.clone(),
            predecessor.provenance(),
            predecessor.control_commitment().control().clone(),
            temporal,
            predecessor.expected_runtime_store_instance_id(),
            predecessor.authentication().claim().clone(),
        )
    };

    let short_temporal =
        temporal_with_remaining(predecessor.temporal(), OPERATION_TIMEOUT_NANOS - 1);
    assert!(matches!(
        draft_with_temporal(short_temporal),
        Err(RemoteAgentDataPlanePlanError::InvalidShape)
    ));
    assert!(matches!(
        draft_with_temporal(short_temporal).and_then(|draft| draft.finalize(&[0xc1; 64])),
        Err(RemoteAgentDataPlanePlanError::InvalidShape)
    ));

    let exact_temporal = temporal_with_remaining(predecessor.temporal(), OPERATION_TIMEOUT_NANOS);
    let request = draft_with_temporal(exact_temporal)
        .expect("PXAR v11 draft at exact profile timeout")
        .finalize(&[0xc1; 64])
        .expect("PXAR v11 at exact profile timeout");
    assert_eq!(
        request.temporal().remaining_budget().value(),
        OPERATION_TIMEOUT_NANOS
    );
    assert_eq!(
        RemoteAgentDataPlaneApplyRequestV2::decode(request.canonical_wire()).unwrap(),
        request
    );

    let mut short_wire = request.canonical_wire().to_vec();
    overwrite_pxar_envelope_u64_tlv(&mut short_wire, 31, OPERATION_TIMEOUT_NANOS - 1);
    assert!(matches!(
        RemoteAgentDataPlaneApplyRequestV2::decode(&short_wire),
        Err(RemoteAgentDataPlanePlanError::InvalidShape)
    ));
}

#[test]
fn pxau2_positive_outcome_matrix_is_round_trippable() {
    let active = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let local = data_plane_request(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);
    let matrix = [
        terminal_receipt(
            &active,
            active_ready_state(),
            active_ready_evidence(&active),
        ),
        terminal_receipt(
            &local,
            local_only_ready_state(),
            local_only_ready_evidence(&local),
        ),
        terminal_receipt(&active, no_effect_state(), base_evidence(&active)),
        terminal_receipt(&active, uncertain_state(), base_evidence(&active)),
        terminal_receipt(
            &local,
            quarantined_state(&local),
            quarantined_evidence(&local),
        ),
    ];
    let expected_outcomes = [
        RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
        RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
        RemoteAgentDataPlaneTerminalOutcomeV2::NoEffectRejected,
        RemoteAgentDataPlaneTerminalOutcomeV2::Uncertain,
        RemoteAgentDataPlaneTerminalOutcomeV2::Quarantined,
    ];
    for (receipt, expected_outcome) in matrix.into_iter().zip(expected_outcomes) {
        let decoded = RemoteAgentDataPlaneTerminalReceiptV2::decode(receipt.canonical_wire())
            .expect("PXAU v2 round trip");
        assert_eq!(decoded, receipt);
        assert_eq!(decoded.facts().state().outcome(), expected_outcome);
    }
}

#[test]
fn pxau2_freezes_fixed_body_signed_length_runtime_verification_and_version_isolation() {
    let request = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let state = active_ready_state();
    let fields = active_ready_evidence(&request);
    let draft = terminal_draft(&request, state, fields).expect("active PXAU v2 draft");
    let transcript = draft.signing_transcript().expect("PXAU v2 transcript");
    assert_eq!(transcript.as_bytes().len(), 732);
    let receipt = draft.finalize(&[0xe1; 64]).expect("PXAU v2");
    assert_eq!(
        receipt.canonical_wire().len() - receipt.authentication_signature().len(),
        683
    );
    assert_eq!(receipt.canonical_wire().len(), 747);
    assert!(
        receipt.canonical_wire().len() <= MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES
    );
    assert!(
        terminal_draft(&request, state, fields)
            .unwrap()
            .finalize(&[])
            .is_err()
    );

    let one_byte = terminal_draft(&request, state, fields)
        .unwrap()
        .finalize(&[0xe2])
        .expect("one-byte PXAU v2 signature");
    assert_eq!(one_byte.canonical_wire().len(), 684);
    assert_eq!(
        RemoteAgentDataPlaneTerminalReceiptV2::decode(one_byte.canonical_wire()).unwrap(),
        one_byte
    );

    let maximum_signature = [0xe3; MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_V2_BYTES];
    let maximum = terminal_draft(&request, state, fields)
        .unwrap()
        .finalize(&maximum_signature)
        .expect("maximum PXAU v2 signature");
    assert_eq!(
        MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES,
        1_195
    );
    assert_eq!(
        maximum.canonical_wire().len(),
        MAX_CANONICAL_REMOTE_AGENT_DATA_PLANE_TERMINAL_RECEIPT_V2_BYTES
    );
    assert_eq!(
        RemoteAgentDataPlaneTerminalReceiptV2::decode(maximum.canonical_wire()).unwrap(),
        maximum
    );

    let oversized_signature = [0xe4; MAX_REMOTE_AGENT_DATA_PLANE_TERMINAL_SIGNATURE_V2_BYTES + 1];
    assert!(
        terminal_draft(&request, state, fields)
            .unwrap()
            .finalize(&oversized_signature)
            .is_err()
    );

    let decoded = RemoteAgentDataPlaneTerminalReceiptV2::decode(receipt.canonical_wire())
        .expect("PXAU v2 round trip");
    let expected_transcript = decoded
        .signing_transcript()
        .expect("decoded signing transcript")
        .as_bytes()
        .to_vec();
    let verifier_called = Cell::new(false);
    let authenticated = decoded
        .verify_runtime_terminal(
            &request,
            terminal_auth(),
            |principal, key, algorithm, version, actual_transcript, signature| {
                verifier_called.set(true);
                assert_eq!(principal, terminal_auth().runtime_principal());
                assert_eq!(key, terminal_auth().key());
                assert_eq!(algorithm, terminal_auth().algorithm());
                assert_eq!(version, terminal_auth().algorithm_version());
                assert_eq!(actual_transcript, expected_transcript);
                signature == [0xe1; 64]
            },
        )
        .expect("Runtime-authenticated PXAU v2");
    assert!(verifier_called.get());
    assert_eq!(authenticated.receipt(), &decoded);

    let mut signature_tamper = receipt.canonical_wire().to_vec();
    *signature_tamper.last_mut().unwrap() ^= 1;
    let structurally_valid_tamper =
        RemoteAgentDataPlaneTerminalReceiptV2::decode(&signature_tamper)
            .expect("signature bytes are opaque before Runtime verification");
    assert!(
        structurally_valid_tamper
            .verify_runtime_terminal(&request, terminal_auth(), |_, _, _, _, _, signature| {
                signature == [0xe1; 64]
            })
            .is_err()
    );

    let old_pxau = fixture_hex_after(DATA_PLANE_V1_FIXTURE, "\"data_plane\"", "\"pxau_hex\"");
    assert!(RemoteAgentDataPlaneTerminalReceiptV1::decode(&old_pxau).is_ok());
    assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&old_pxau).is_err());
    assert!(RemoteAgentDataPlaneTerminalReceiptV1::decode(receipt.canonical_wire()).is_err());

    let mut wrong_magic = receipt.canonical_wire().to_vec();
    wrong_magic[..4].copy_from_slice(b"PXAR");
    assert!(RemoteAgentDataPlaneTerminalReceiptV2::decode(&wrong_magic).is_err());
    assert!(RemoteAgentDataPlaneApplyRequestV2::decode(receipt.canonical_wire()).is_err());
}

#[test]
fn pxau2_raw_decode_never_substitutes_for_signer_and_request_correlation() {
    let request = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let receipt = terminal_receipt(
        &request,
        active_ready_state(),
        active_ready_evidence(&request),
    );
    let decoded = RemoteAgentDataPlaneTerminalReceiptV2::decode(receipt.canonical_wire())
        .expect("raw PXAU v2 decode");

    let wrong_signer_verifier_called = Cell::new(false);
    assert!(
        decoded
            .verify_runtime_terminal(&request, wrong_terminal_auth(), |_, _, _, _, _, _| {
                wrong_signer_verifier_called.set(true);
                true
            },)
            .is_err()
    );
    assert!(!wrong_signer_verifier_called.get());

    let mut correlation_tamper_with_replaced_signature = receipt.canonical_wire().to_vec();
    correlation_tamper_with_replaced_signature[118] ^= 1;
    correlation_tamper_with_replaced_signature[683..].fill(0xf1);
    let raw_tamper =
        RemoteAgentDataPlaneTerminalReceiptV2::decode(&correlation_tamper_with_replaced_signature)
            .expect("canonical but request-uncorrelated PXAU v2");
    let correlation_verifier_called = Cell::new(false);
    assert!(
        raw_tamper
            .verify_runtime_terminal(&request, terminal_auth(), |_, _, _, _, _, _| {
                correlation_verifier_called.set(true);
                true
            },)
            .is_err()
    );
    assert!(!correlation_verifier_called.get());

    let resigned_request = data_plane_request_with_signature(
        RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
        &[0xc2; 64],
    );
    assert_ne!(resigned_request.request_digest(), request.request_digest());
    let resigned_request_receipt = terminal_receipt(
        &resigned_request,
        active_ready_state(),
        active_ready_evidence(&resigned_request),
    );
    let decoded_resigned_request =
        RemoteAgentDataPlaneTerminalReceiptV2::decode(resigned_request_receipt.canonical_wire())
            .expect("PXAU v2 for independently signed PXAR v11");
    let resigned_request_verifier_called = Cell::new(false);
    assert!(
        decoded_resigned_request
            .verify_runtime_terminal(&request, terminal_auth(), |_, _, _, _, _, _| {
                resigned_request_verifier_called.set(true);
                true
            },)
            .is_err()
    );
    assert!(!resigned_request_verifier_called.get());
}

#[test]
fn pxau2_deadline_is_single_admission_derived_and_s0_identity_cannot_drift() {
    let request = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let valid = active_ready_evidence(&request);
    assert_eq!(
        valid.absolute_deadline_nanos - valid.admitted_at_nanos,
        request
            .target_execution()
            .profile()
            .operation_timeout_nanos()
    );
    assert!(terminal_draft(&request, active_ready_state(), valid).is_ok());

    let mut selected_before_admission = valid;
    selected_before_admission.selection_observed_at_nanos = valid.admitted_at_nanos - 1;
    assert!(terminal_draft(&request, active_ready_state(), selected_before_admission).is_err());

    let mut selected_at_admission = valid;
    selected_at_admission.selection_observed_at_nanos = valid.admitted_at_nanos;
    let selected_at_admission_receipt = terminal_draft(
        &request,
        active_ready_state(),
        selected_at_admission,
    )
    .expect("PXAU v2 draft selected exactly at admission")
    .finalize(&[0xe1; 64])
    .expect("PXAU v2 selected exactly at admission");
    let selected_at_admission_decoded = RemoteAgentDataPlaneTerminalReceiptV2::decode(
        selected_at_admission_receipt.canonical_wire(),
    )
    .expect("round-tripped PXAU v2 selected exactly at admission");
    assert_eq!(selected_at_admission_decoded, selected_at_admission_receipt);
    assert_eq!(
        selected_at_admission_decoded
            .validate_against_request(&request)
            .expect("request-correlated PXAU v2 selected exactly at admission")
            .evidence()
            .fields()
            .selection_observed_at_nanos,
        valid.admitted_at_nanos
    );

    let mut selected_before_deadline = valid;
    selected_before_deadline.selection_observed_at_nanos = valid.absolute_deadline_nanos - 1;
    assert!(terminal_draft(&request, active_ready_state(), selected_before_deadline).is_ok());

    let mut selected_at_deadline = valid;
    selected_at_deadline.selection_observed_at_nanos = valid.absolute_deadline_nanos;
    assert!(terminal_draft(&request, active_ready_state(), selected_at_deadline).is_err());

    let mut reset_from_later_observation = valid;
    reset_from_later_observation.absolute_deadline_nanos =
        reset_from_later_observation.selection_observed_at_nanos + OPERATION_TIMEOUT_NANOS;
    assert!(terminal_draft(&request, active_ready_state(), reset_from_later_observation,).is_err());

    let mut completed_after_deadline = valid;
    completed_after_deadline.selection_observed_at_nanos = valid.absolute_deadline_nanos + 1;
    assert!(terminal_draft(&request, active_ready_state(), completed_after_deadline).is_err());

    let local = data_plane_request(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);
    let mut local_after_deadline = local_only_ready_evidence(&local);
    local_after_deadline.selection_observed_at_nanos =
        local_after_deadline.absolute_deadline_nanos + 1;
    assert!(terminal_draft(&local, local_only_ready_state(), local_after_deadline).is_ok());

    let mut census_drift = valid;
    census_drift.retained_s0_census_after_digest = Digest32::from_bytes([0xd4; 32]);
    assert!(terminal_draft(&request, active_ready_state(), census_drift).is_err());

    let mut incomplete_census = valid;
    incomplete_census.retained_s0_census_complete = false;
    assert!(terminal_draft(&request, active_ready_state(), incomplete_census).is_err());

    let fabric_generation_drift = terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::ReadyObservation,
        head: RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
        fabric_generation: Some(FABRIC_GENERATION + 1),
        agent_generation: Some(AGENT_GENERATION),
        access_generation: Some(ACTIVE_PRIOR_HIGH_WATER + 1),
        fabric_epoch: Some(FABRIC_SESSION_EPOCH),
        proxy_epoch: Some([0xc7; 16]),
    });
    assert!(terminal_draft(&request, fabric_generation_drift, valid).is_err());

    let agent_generation_drift = terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::ReadyObservation,
        head: RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
        fabric_generation: Some(FABRIC_GENERATION),
        agent_generation: Some(AGENT_GENERATION + 1),
        access_generation: Some(ACTIVE_PRIOR_HIGH_WATER + 1),
        fabric_epoch: Some(FABRIC_SESSION_EPOCH),
        proxy_epoch: Some([0xc7; 16]),
    });
    assert!(terminal_draft(&request, agent_generation_drift, valid).is_err());

    let session_drift = terminal_state(TerminalStateFixture {
        outcome: RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
        lifecycle_effect: RemoteAgentDataPlaneTerminalLifecycleEffectV2::MayHaveStarted,
        phase: RemoteAgentDataPlaneTerminalPhaseV2::ReadyObservation,
        head: RemoteAgentDataPlaneTerminalHeadV2::CommittedIncoming,
        fabric_generation: Some(FABRIC_GENERATION),
        agent_generation: Some(AGENT_GENERATION),
        access_generation: Some(ACTIVE_PRIOR_HIGH_WATER + 1),
        fabric_epoch: Some([0xf7; 16]),
        proxy_epoch: Some([0xc7; 16]),
    });
    assert!(terminal_draft(&request, session_drift, valid).is_err());
}

#[test]
fn pxau2_uncertain_requires_exact_known_s0_and_mode_specific_effect_phase() {
    let active = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let local = data_plane_request(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);

    let mut mismatched_s0 = base_evidence(&active);
    mismatched_s0.retained_s0_current_cas_digest = Digest32::from_bytes([0xfe; 32]);
    assert!(terminal_draft(&active, uncertain_state(), mismatched_s0).is_err());

    let mut known_absent = base_evidence(&active);
    known_absent.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::S1Absent;
    known_absent.s1_closed = true;
    known_absent.s1_listener_released = true;
    let known_absent_receipt = terminal_receipt(&active, uncertain_state(), known_absent);
    assert_eq!(
        RemoteAgentDataPlaneTerminalReceiptV2::decode(known_absent_receipt.canonical_wire())
            .unwrap(),
        known_absent_receipt
    );

    for phase in [
        RemoteAgentDataPlaneTerminalPhaseV2::IngressFenceIntent,
        RemoteAgentDataPlaneTerminalPhaseV2::DrainIntent,
    ] {
        assert!(
            terminal_draft(&active, uncertain_state_at(phase), base_evidence(&active)).is_err()
        );
    }
    for phase in [
        RemoteAgentDataPlaneTerminalPhaseV2::S1OpenIntent,
        RemoteAgentDataPlaneTerminalPhaseV2::QueryablesDeclareIntent,
    ] {
        assert!(terminal_draft(&local, uncertain_state_at(phase), base_evidence(&local)).is_err());
    }
    assert!(
        terminal_draft(
            &local,
            uncertain_state_at(RemoteAgentDataPlaneTerminalPhaseV2::IngressFenceIntent),
            base_evidence(&local),
        )
        .is_ok()
    );
}

#[test]
fn pxau2_hwm_revision_mode_drain_counts_and_observation_are_orthogonal_authority() {
    let active = data_plane_request(RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive);
    let local = data_plane_request(RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate);

    let mut stale_high_water = active_ready_evidence(&active);
    stale_high_water.access_generation_high_water = ACTIVE_PRIOR_HIGH_WATER;
    assert!(terminal_draft(&active, active_ready_state(), stale_high_water).is_err());

    let mut stale_revision = active_ready_evidence(&active);
    stale_revision.completion_owner_slot_revision = ACTIVE_PRIOR_SLOT_REVISION;
    assert!(terminal_draft(&active, active_ready_state(), stale_revision).is_err());

    assert!(
        terminal_draft(
            &active,
            local_only_ready_state(),
            local_only_ready_evidence(&active),
        )
        .is_err()
    );

    let mut incomplete_join = local_only_ready_evidence(&local);
    incomplete_join.worker_joined_bitmap = 0b01;
    assert!(terminal_draft(&local, local_only_ready_state(), incomplete_join).is_err());

    let mut unterminalized = local_only_ready_evidence(&local);
    unterminalized.submit_terminalized_count -= 1;
    assert!(terminal_draft(&local, local_only_ready_state(), unterminalized).is_err());

    let mut out_of_contract_bitmap = local_only_ready_evidence(&local);
    out_of_contract_bitmap.ingress_fenced_bitmap = 0b100;
    assert!(RemoteAgentDataPlaneTerminalEvidenceV2::try_new(out_of_contract_bitmap).is_err());

    let mut observation_alias = active_ready_evidence(&active);
    observation_alias.remote_observation = RemoteAgentDataPlaneRemoteObservationV2::S1Absent;
    assert!(terminal_draft(&active, active_ready_state(), observation_alias).is_err());

    let mut uncertain_absence_without_close = base_evidence(&active);
    uncertain_absence_without_close.remote_observation =
        RemoteAgentDataPlaneRemoteObservationV2::S1Absent;
    assert!(terminal_draft(&active, uncertain_state(), uncertain_absence_without_close).is_err());

    let mut impossible_counts = base_evidence(&active);
    impossible_counts.control_terminalized_count = 1;
    assert!(RemoteAgentDataPlaneTerminalEvidenceV2::try_new(impossible_counts).is_err());

    let mut wrong_clock = active_ready_evidence(&active);
    wrong_clock.selection_clock_domain = ClockDomainRef::from_bytes([0xee; 16]);
    assert!(terminal_draft(&active, active_ready_state(), wrong_clock).is_err());

    let mut later_clock_generation = active_ready_evidence(&active);
    later_clock_generation.selection_clock_generation =
        ClockGeneration::try_new(active.temporal().target_clock_generation().value() + 1)
            .expect("later clock generation");
    assert!(terminal_draft(&active, active_ready_state(), later_clock_generation).is_err());
}

#[test]
fn shared_python_v2_golden_decodes_with_exact_digests_transcripts_and_signatures() {
    let golden = PROXY_DATA_PLANE_V2_GOLDEN;
    assert!(golden.contains("\"format\": \"paraegox-t2-remote-agent-proxy-data-plane-v2\""));
    assert!(
        golden.contains(
            "\"source\": \"independent Python struct/hashlib/cryptography T2-B2 oracle\""
        )
    );
    assert_eq!(
        fixture_string_after(
            golden,
            "\"rust_source_freeze\"",
            "\"remote_agent_data_plane_plan.rs_sha256\"",
        ),
        "2649e0457c47e63a2132df2b8db9de52818b71645c54b656747c978fb2d882e7"
    );

    let semantic_constants = fixture_section_after(golden, "\"semantic_constants\"");
    let golden_profile_timeout = fixture_u64_after(
        semantic_constants,
        "",
        "\"profile_operation_timeout_nanos\"",
    );
    let golden_original_budget =
        fixture_u64_after(semantic_constants, "", "\"envelope_original_budget_nanos\"");
    let golden_remaining_budget = fixture_u64_after(
        semantic_constants,
        "",
        "\"envelope_remaining_budget_nanos\"",
    );
    assert!(golden_remaining_budget >= golden_profile_timeout);
    assert!(golden_original_budget >= golden_remaining_budget);
    let temporal_authority_rule =
        fixture_hex_after(semantic_constants, "", "\"temporal_authority_rule_hex\"");
    assert_eq!(
        temporal_authority_rule.as_slice(),
        b"temporal-remaining>=operation-timeout;selection>=admitted;active-ready<deadline;local-cleanup-may-complete-after-deadline"
    );
    assert_eq!(
        fixture_u64_after(semantic_constants, "", "\"retained_s0_cas_bytes\""),
        REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES as u64
    );
    assert_eq!(
        fixture_u64_after(semantic_constants, "", "\"active_s1_cas_bytes\""),
        REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES as u64
    );
    assert_eq!(
        fixture_u64_after(semantic_constants, "", "\"pxau_v2_fixed_bytes\""),
        683
    );
    assert_eq!(
        fixture_u64_after(semantic_constants, "", "\"pxau_v2_signature_bytes\""),
        64
    );
    assert_eq!(
        fixture_u64_after(semantic_constants, "", "\"pxau_v2_canonical_bytes\""),
        747
    );
    assert_eq!(
        fixture_u64_after(semantic_constants, "", "\"terminal_transcript_bytes\""),
        732
    );

    let retained_scope = fixture_section_after(golden, "\"retained_s0_cas\"");
    let retained_wire = fixture_hex_after(retained_scope, "", "\"wire_hex\"");
    let retained =
        RemoteAgentRetainedS0CasV2::decode(&retained_wire).expect("shared golden retained-S0 CAS");
    assert_eq!(retained.canonical_wire(), retained_wire.as_slice());
    assert_eq!(
        retained_wire.len() as u64,
        fixture_u64_after(retained_scope, "", "\"wire_length\"")
    );
    assert_eq!(retained_wire.len(), REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES);
    assert_eq!(
        retained.cas_digest(),
        fixture_digest_after(retained_scope, "", "\"digest_hex\"")
    );

    let topology_digest = fixture_digest_after(golden, "", "\"topology_compatibility_digest_hex\"");
    assert_eq!(
        topology_digest,
        remote_agent_proxy_topology_compatibility_digest_v2().unwrap()
    );
    let runtime_public_key = fixture_hex_after(golden, "", "\"runtime_signing_public_key_hex\"");
    assert_eq!(runtime_public_key.len(), 32);

    for (ready_key, expected_mode, expected_outcome) in [
        (
            "\"active_ready\"",
            RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
            RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady,
        ),
        (
            "\"local_only_ready\"",
            RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate,
            RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady,
        ),
    ] {
        let ready = fixture_section_after(golden, ready_key);
        let expected_s1_scope = fixture_section_after(ready, "\"expected_s1_cas\"");
        let expected_s1_wire = fixture_hex_after(expected_s1_scope, "", "\"wire_hex\"");
        let expected_s1 = RemoteAgentActiveS1CasV2::decode(&expected_s1_wire)
            .expect("shared golden expected-S1 CAS");
        assert_eq!(expected_s1.canonical_wire(), expected_s1_wire.as_slice());
        assert_eq!(
            expected_s1_wire.len() as u64,
            fixture_u64_after(expected_s1_scope, "", "\"wire_length\"")
        );
        assert_eq!(expected_s1_wire.len(), REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES);
        assert_eq!(
            expected_s1.cas_digest(),
            fixture_digest_after(expected_s1_scope, "", "\"digest_hex\"")
        );
        assert_eq!(
            expected_s1.active().is_some(),
            expected_mode == RemoteAgentDataPlaneTargetModeV2::LocalAgentOnlyDeactivate
        );

        let pxte_scope = fixture_section_after(ready, "\"pxte_v10\"");
        let pxte_wire = fixture_hex_after(pxte_scope, "", "\"wire_hex\"");
        let pxte = RemoteAgentDataPlaneTargetExecutionV2::decode(&pxte_wire)
            .expect("shared golden PXTE v10");
        assert_eq!(pxte.canonical_wire(), pxte_wire.as_slice());
        assert_eq!(
            pxte_wire.len() as u64,
            fixture_u64_after(pxte_scope, "", "\"wire_length\"")
        );
        assert_eq!(
            pxte.execution_digest(),
            fixture_digest_after(pxte_scope, "", "\"digest_hex\"")
        );
        assert_eq!(pxte.mode(), expected_mode);
        assert_eq!(
            pxte.profile().operation_timeout_nanos(),
            golden_profile_timeout
        );
        assert_eq!(pxte.retained_s0_cas(), retained);
        assert_eq!(pxte.expected_s1_cas(), expected_s1);
        assert_eq!(pxte.proxy_topology_compatibility_digest(), topology_digest);

        let pxar_scope = fixture_section_after(ready, "\"pxar_v11\"");
        let pxar_wire = fixture_hex_after(pxar_scope, "", "\"wire_hex\"");
        let pxar =
            RemoteAgentDataPlaneApplyRequestV2::decode(&pxar_wire).expect("shared golden PXAR v11");
        assert_eq!(pxar.canonical_wire(), pxar_wire.as_slice());
        assert_eq!(
            pxar_wire.len() as u64,
            fixture_u64_after(pxar_scope, "", "\"wire_length\"")
        );
        assert_eq!(
            pxar.request_digest(),
            fixture_digest_after(pxar_scope, "", "\"digest_hex\"")
        );
        assert_eq!(pxar.target_execution(), &pxte);
        assert_eq!(
            pxar.assignment_digest().value(),
            &fixture_digest_after(ready, "", "\"assignment_v11_digest_hex\"")
        );
        assert_eq!(
            pxar.temporal().original_budget().value(),
            golden_original_budget
        );
        assert_eq!(
            pxar.temporal().remaining_budget().value(),
            golden_remaining_budget
        );

        let envelope_scope = fixture_section_after(ready, "\"envelope_v2\"");
        assert_eq!(
            u32::from_be_bytes(
                pxar_wire[6..10]
                    .try_into()
                    .expect("PXAR v11 envelope length"),
            ) as u64,
            fixture_u64_after(envelope_scope, "", "\"wire_length\"")
        );
        assert_eq!(
            pxar.envelope_request_digest(),
            fixture_digest_after(envelope_scope, "", "\"request_digest_hex\"")
        );
        assert_eq!(
            pxar.target_slice_digest().value(),
            &fixture_digest_after(envelope_scope, "", "\"target_slice_digest_hex\"")
        );
        let envelope_transcript =
            fixture_hex_after(envelope_scope, "", "\"signing_transcript_hex\"");
        let envelope_signature = fixture_hex_after(envelope_scope, "", "\"signature_hex\"");
        assert_eq!(
            pxar.signing_transcript()
                .expect("shared golden PXAR v11 transcript")
                .as_bytes(),
            envelope_transcript.as_slice()
        );
        assert_eq!(pxar.authentication().signature(), envelope_signature);
        assert_eq!(
            fixture_hex_after(envelope_scope, "", "\"public_key_hex\"").len(),
            32
        );
        assert_eq!(envelope_signature.len(), 64);

        let pxau_scope = fixture_section_after(ready, "\"pxau_v2\"");
        let pxau_wire = fixture_hex_after(pxau_scope, "", "\"wire_hex\"");
        let pxau = RemoteAgentDataPlaneTerminalReceiptV2::decode(&pxau_wire)
            .expect("shared golden PXAU v2");
        assert_eq!(pxau.canonical_wire(), pxau_wire.as_slice());
        assert_eq!(
            pxau_wire.len() as u64,
            fixture_u64_after(pxau_scope, "", "\"wire_length\"")
        );
        assert_eq!(pxau_wire.len(), 747);
        assert_eq!(
            pxau.receipt_digest(),
            fixture_digest_after(pxau_scope, "", "\"digest_hex\"")
        );
        assert_eq!(
            pxau.validate_against_request(&pxar)
                .expect("shared golden PXAU v2 correlation")
                .state()
                .outcome(),
            expected_outcome
        );
        let temporal_evidence = pxau.facts().evidence().fields();
        assert_eq!(
            temporal_evidence.selection_clock_domain,
            pxar.temporal().target_clock_domain()
        );
        assert_eq!(
            temporal_evidence.selection_clock_generation,
            pxar.temporal().target_clock_generation()
        );
        assert_eq!(
            temporal_evidence
                .absolute_deadline_nanos
                .checked_sub(temporal_evidence.admitted_at_nanos),
            Some(golden_profile_timeout)
        );
        assert!(
            temporal_evidence.selection_observed_at_nanos >= temporal_evidence.admitted_at_nanos
        );
        match expected_outcome {
            RemoteAgentDataPlaneTerminalOutcomeV2::ActiveReady => assert!(
                temporal_evidence.selection_observed_at_nanos
                    < temporal_evidence.absolute_deadline_nanos
            ),
            RemoteAgentDataPlaneTerminalOutcomeV2::LocalOnlyReady => assert!(
                temporal_evidence.selection_observed_at_nanos
                    > temporal_evidence.absolute_deadline_nanos
            ),
            _ => unreachable!("shared golden covers only ready outcomes"),
        }
        let terminal_transcript = fixture_hex_after(pxau_scope, "", "\"signing_transcript_hex\"");
        let terminal_signature = fixture_hex_after(pxau_scope, "", "\"signature_hex\"");
        assert_eq!(
            terminal_transcript.len() as u64,
            fixture_u64_after(pxau_scope, "", "\"signing_transcript_length\"")
        );
        assert_eq!(terminal_transcript.len(), 732);
        assert_eq!(
            pxau.signing_transcript()
                .expect("shared golden PXAU v2 transcript")
                .as_bytes(),
            terminal_transcript.as_slice()
        );
        assert_eq!(pxau.authentication_signature(), terminal_signature);
        assert_eq!(terminal_signature.len(), 64);
        assert_eq!(
            fixture_hex_after(pxau_scope, "", "\"public_key_hex\""),
            runtime_public_key
        );
        assert_eq!(
            pxau.canonical_wire().len() - pxau.authentication_signature().len(),
            683
        );

        let expected_auth = pxau.authentication();
        let authenticated = pxau
            .verify_runtime_terminal(
                &pxar,
                expected_auth,
                |principal, key, algorithm, version, transcript, signature| {
                    principal == expected_auth.runtime_principal()
                        && key == expected_auth.key()
                        && algorithm == expected_auth.algorithm()
                        && version == expected_auth.algorithm_version()
                        && transcript == terminal_transcript.as_slice()
                        && signature == terminal_signature.as_slice()
                },
            )
            .expect("shared golden Runtime-authenticated PXAU v2");
        assert_eq!(authenticated.receipt(), &pxau);
    }
}

#[test]
fn pxar11_keeps_the_predecessor_controller_authentication_transcript_structural() {
    let predecessor = managed_agent_request();
    let temporal = temporal_with_remaining(predecessor.temporal(), OPERATION_TIMEOUT_NANOS);
    let claim = ApplyRequestAuthClaim::try_new(
        predecessor.authentication().claim().principal(),
        predecessor.authentication().claim().key(),
        predecessor.authentication().claim().algorithm(),
        predecessor.authentication().claim().algorithm_version(),
        predecessor.authentication().claim().nonce(),
    )
    .expect("byte-identical predecessor auth claim");
    let draft = RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
        target_execution_for(
            &predecessor,
            RemoteAgentDataPlaneTargetModeV2::RemoteAccessActive,
        ),
        predecessor.provenance(),
        predecessor.control_commitment().control().clone(),
        temporal,
        predecessor.expected_runtime_store_instance_id(),
        claim,
    )
    .expect("PXAR v11 draft");
    assert!(!draft.signing_transcript().unwrap().as_bytes().is_empty());
    assert!(draft.finalize(&[]).is_err());
}
