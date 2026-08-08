use std::cell::Cell;

use paraegox_kernel::digest::Digest32;
use paraegox_kernel::identity::PrincipalRef;
use paraegox_kernel::time::{ClockDomainRef, ClockGeneration};

use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    DistributedFabricCredentialRefV1, DistributedFabricTrustAnchorRefV1,
    DistributedFabricTrustDomainRefV1, RestrictedRuntimeApplyCarrierBindingFieldsV1,
    RestrictedRuntimeApplyCarrierBindingV1,
};
use paraegox_runtime_contracts::managed_agent_stack_plan::{
    ManagedAgentStackApplyRequestV1, ManagedAgentStackTerminalReceiptV1,
};
use paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricApplyTerminalReceiptV1;
use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
use paraegox_runtime_contracts::managed_serving_bootstrap::runtime_agent_control_descriptor_payload_digest_v1;
use paraegox_runtime_contracts::remote_agent_access::{
    ControllerAuthenticatedRemoteAgentAccessRequestV1, RemoteAgentAccessRequestDraftV1,
    RemoteAgentAccessRequestFieldsV1, RemoteAgentAccessRequestIdV1, RemoteAgentAccessRequestV1,
    RemoteAgentAccessResponseAuthClaimV1, RemoteAgentAccessResponseDraftV1,
    RemoteAgentAccessResponseV1,
};
use paraegox_runtime_contracts::remote_agent_data_plane_plan::{
    RemoteAgentBootstrapCasV1, RemoteAgentDataPlaneApplyRequestDraftV1,
    RemoteAgentDataPlaneApplyRequestV1, RemoteAgentDataPlaneProfileFieldsV1,
    RemoteAgentDataPlaneProfileV1, RemoteAgentDataPlaneProjectionV1,
    RemoteAgentDataPlaneRemoteObservationV1, RemoteAgentDataPlaneTargetExecutionV1,
    RemoteAgentDataPlaneTerminalAuthClaimV1, RemoteAgentDataPlaneTerminalEvidenceFieldsV1,
    RemoteAgentDataPlaneTerminalEvidenceV1, RemoteAgentDataPlaneTerminalHeadV1,
    RemoteAgentDataPlaneTerminalLifecycleEffectV1, RemoteAgentDataPlaneTerminalOutcomeV1,
    RemoteAgentDataPlaneTerminalReceiptDraftV1, RemoteAgentDataPlaneTerminalReceiptV1,
    RemoteAgentDataPlaneTerminalStateV1,
};
use paraegox_runtime_contracts::wire::{
    ApplyAuthAlgorithm, ApplyAuthKeyRef, ApplyRequestAuthClaim,
};

const FABRIC_FIXTURE: &str =
    include_str!("../../../tests/fixtures/wire/s7_managed_fabric_successor_v1.json");
const AGENT_STACK_FIXTURE: &str =
    include_str!("../../../tests/fixtures/wire/s7_managed_agent_stack_successor_v1.json");
const EMPTY_PXTA: &[u8; 10] = b"PXTA\0\x01\0\0\0\0";
const OLD_DESCRIPTOR: &[u8] = b"PXAP\0\x01bootstrap-agent-port-v1";
const FRESH_DESCRIPTOR: &[u8] = b"PXAP\0\x01current-agent-port-v2";
const RUNTIME_EPOCH: u64 = 23;

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("non-hex fixture byte"),
    }
}

fn fixture_hex_after(fixture: &str, section: &str, key: &str) -> Vec<u8> {
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
    fixture.as_bytes()[quote_start..quote_end]
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn managed_agent_request() -> ManagedAgentStackApplyRequestV1 {
    ManagedAgentStackApplyRequestV1::decode(&fixture_hex_after(
        AGENT_STACK_FIXTURE,
        "\"fabric_and_agent\"",
        "\"outer_v7_hex\"",
    ))
    .expect("managed Agent-stack PXAR v7 fixture")
}

fn managed_agent_receipt() -> ManagedAgentStackTerminalReceiptV1 {
    ManagedAgentStackTerminalReceiptV1::decode(&fixture_hex_after(
        AGENT_STACK_FIXTURE,
        "\"fabric_and_agent\"",
        "\"wire_hex\"",
    ))
    .expect("managed Agent-stack PXST fixture")
}

fn managed_fabric_receipt() -> ManagedFabricApplyTerminalReceiptV1 {
    ManagedFabricApplyTerminalReceiptV1::decode(&fixture_hex_after(
        FABRIC_FIXTURE,
        "\"active_ready\"",
        "\"wire_hex\"",
    ))
    .expect("managed Fabric PXFT fixture")
}

fn generation(value: u64) -> ManagedServiceGeneration {
    ManagedServiceGeneration::try_new(value).expect("nonzero generation")
}

fn profile_for(
    request: &ManagedAgentStackApplyRequestV1,
    base_loopback_listen_endpoint: &str,
    endpoint_generation: u64,
    mac_agent_client_principal: PrincipalRef,
    ubuntu_agent_listener_principal: PrincipalRef,
) -> Result<
    RemoteAgentDataPlaneProfileV1,
    paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentDataPlanePlanError,
> {
    RemoteAgentDataPlaneProfileV1::try_new(RemoteAgentDataPlaneProfileFieldsV1 {
        target: request.target(),
        base_loopback_listen_endpoint,
        ubuntu_tls_listener_endpoint: "tls/192.0.2.10:7447",
        endpoint_ref: [0x91; 16],
        endpoint_generation,
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
        mac_agent_client_principal,
        ubuntu_agent_listener_principal,
        operation_timeout_nanos: 5_000_000_000,
    })
}

fn projection_for(request: &ManagedAgentStackApplyRequestV1) -> RemoteAgentDataPlaneProjectionV1 {
    RemoteAgentDataPlaneProjectionV1::try_from_managed_agent_stack_projection(
        request.target_execution().projection().clone(),
    )
    .expect("PXAE projection")
}

fn standard_profile(request: &ManagedAgentStackApplyRequestV1) -> RemoteAgentDataPlaneProfileV1 {
    profile_for(
        request,
        "tcp/127.0.0.1:7447",
        101,
        PrincipalRef::from_bytes([0xa1; 16]),
        PrincipalRef::from_bytes([0xa2; 16]),
    )
    .expect("PXAD profile")
}

fn bootstrap_cas() -> RemoteAgentBootstrapCasV1 {
    RemoteAgentBootstrapCasV1::try_new(
        managed_fabric_receipt().receipt_digest(),
        managed_agent_receipt().receipt_digest(),
        Digest32::from_bytes([0x9a; 32]),
        runtime_agent_control_descriptor_payload_digest_v1(OLD_DESCRIPTOR)
            .expect("old PXAP digest"),
        generation(7),
        generation(8),
    )
    .expect("active bootstrap CAS")
}

fn data_plane_request(active: bool) -> RemoteAgentDataPlaneApplyRequestV1 {
    let predecessor_request = managed_agent_request();
    let projection = projection_for(&predecessor_request);
    let profile = standard_profile(&predecessor_request);
    let execution = if active {
        RemoteAgentDataPlaneTargetExecutionV1::try_remote_access_active(
            projection,
            predecessor_request.target_execution().clone(),
            bootstrap_cas(),
            profile,
        )
        .expect("active PXTE v9")
    } else {
        RemoteAgentDataPlaneTargetExecutionV1::try_local_agent_only_deactivate(
            projection,
            predecessor_request.target_execution().clone(),
            profile,
        )
        .expect("local-only PXTE v9")
    };
    RemoteAgentDataPlaneApplyRequestDraftV1::try_new(
        execution,
        predecessor_request.provenance(),
        predecessor_request.control_commitment().control().clone(),
        predecessor_request.temporal(),
        predecessor_request.expected_runtime_store_instance_id(),
        predecessor_request.authentication().claim().clone(),
    )
    .expect("PXAR v10 draft")
    .finalize(&[0xc1; 64])
    .expect("structural PXAR v10")
}

fn terminal_auth() -> RemoteAgentDataPlaneTerminalAuthClaimV1 {
    RemoteAgentDataPlaneTerminalAuthClaimV1::try_new(
        PrincipalRef::from_bytes([0xb1; 16]),
        ApplyAuthKeyRef::from_bytes([0xb4; 16]),
        ApplyAuthAlgorithm::try_new(9).expect("inner algorithm"),
        2,
    )
    .expect("independent PXAU auth claim")
}

fn evidence_fields(
    request: &RemoteAgentDataPlaneApplyRequestV1,
    census_complete: bool,
    base_fabric_ready: bool,
    base_agent_ready: bool,
    remote_observation: RemoteAgentDataPlaneRemoteObservationV1,
    quarantined: bool,
    fresh_current_descriptor_payload_digest: Digest32,
) -> RemoteAgentDataPlaneTerminalEvidenceFieldsV1 {
    let (echoed_receipt, echoed_payload) = request.target_execution().bootstrap_cas().map_or(
        (Digest32::from_bytes([0; 32]), Digest32::from_bytes([0; 32])),
        |cas| {
            (
                cas.expected_bootstrap_descriptor_receipt_digest(),
                cas.expected_bootstrap_descriptor_payload_digest(),
            )
        },
    );
    RemoteAgentDataPlaneTerminalEvidenceFieldsV1 {
        physical_binding_census: if census_complete { 2 } else { 0 },
        census_complete,
        base_fabric_ready,
        base_agent_ready,
        remote_observation,
        quarantined,
        echoed_bootstrap_descriptor_receipt_digest: echoed_receipt,
        echoed_bootstrap_descriptor_payload_digest: echoed_payload,
        fresh_current_descriptor_payload_digest,
        resource_census_digest: Digest32::from_bytes([0xd1; 32]),
        raw_outcome_digest: Digest32::from_bytes([0xd2; 32]),
        completion_runtime_host_epoch: RUNTIME_EPOCH,
        completion_snapshot_sequence: 17,
        selection_clock_domain: request.temporal().target_clock_domain(),
        selection_clock_generation: request.temporal().target_clock_generation(),
        selection_observed_at_nanos: 19,
    }
}

fn terminal_state(
    outcome: RemoteAgentDataPlaneTerminalOutcomeV1,
    lifecycle: RemoteAgentDataPlaneTerminalLifecycleEffectV1,
    head: RemoteAgentDataPlaneTerminalHeadV1,
    fabric_generation: Option<u64>,
    agent_generation: Option<u64>,
    access_generation: Option<u64>,
) -> RemoteAgentDataPlaneTerminalStateV1 {
    RemoteAgentDataPlaneTerminalStateV1::try_new(
        outcome,
        lifecycle,
        head,
        fabric_generation.map(generation),
        agent_generation.map(generation),
        access_generation.map(generation),
    )
    .expect("terminal state shape")
}

fn accepts_terminal(
    request: &RemoteAgentDataPlaneApplyRequestV1,
    state: RemoteAgentDataPlaneTerminalStateV1,
    fields: RemoteAgentDataPlaneTerminalEvidenceFieldsV1,
) {
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(fields)
        .expect("structural terminal evidence");
    RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(request, state, evidence, terminal_auth())
        .expect("request-correlated terminal matrix member");
}

fn carrier_for(
    request: &RemoteAgentDataPlaneApplyRequestV1,
) -> RestrictedRuntimeApplyCarrierBindingV1 {
    RestrictedRuntimeApplyCarrierBindingV1::try_new(
        RestrictedRuntimeApplyCarrierBindingFieldsV1 {
            target: request.target(),
            runtime_principal: terminal_auth().runtime_principal(),
            controller_principal: request.authentication().claim().principal(),
            endpoint_ref: [0xb5; 16],
            endpoint_generation: 11,
            route: "paraegox/runtime/control/v1/apply",
            controller_request_key: request.authentication().claim().key(),
            controller_request_key_fingerprint: Digest32::from_bytes([0xb6; 32]),
            runtime_response_key: ApplyAuthKeyRef::from_bytes([0xb7; 16]),
            runtime_response_key_fingerprint: Digest32::from_bytes([0xb8; 32]),
            control_transport_profile_ref: [0xb9; 16],
            control_transport_profile_digest: Digest32::from_bytes([0xba; 32]),
        },
    )
    .expect("restricted PXCB")
}

fn access_fields(
    request: &RemoteAgentDataPlaneApplyRequestV1,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    nonce: &[u8],
) -> RemoteAgentAccessRequestFieldsV1 {
    RemoteAgentAccessRequestFieldsV1 {
        request_id: RemoteAgentAccessRequestIdV1::try_from_bytes(*request.operation_id().as_bytes())
            .expect("PXRA request id"),
        target: request.target(),
        expected_runtime_store_instance_id: request.expected_runtime_store_instance_id(),
        expected_runtime_host_epoch: RUNTIME_EPOCH,
        auth_claim: ApplyRequestAuthClaim::try_new(
            carrier.controller_principal(),
            carrier.controller_request_key(),
            ApplyAuthAlgorithm::try_new(1).expect("outer Ed25519 algorithm"),
            1,
            nonce,
        )
        .expect("PXRA auth claim"),
        carrier,
    }
}

fn authenticate_access_request<'a>(
    request: &'a RemoteAgentAccessRequestV1,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
    signature: &[u8],
) -> ControllerAuthenticatedRemoteAgentAccessRequestV1<'a> {
    request
        .verify_controller_request(
            carrier,
            |principal, key, fingerprint, transcript, actual_signature| {
                principal == carrier.controller_principal()
                    && key == carrier.controller_request_key()
                    && fingerprint == carrier.controller_request_key_fingerprint()
                    && !transcript.is_empty()
                    && actual_signature == signature
            },
        )
        .expect("Controller-authenticated PXRA")
}

fn access_response_auth(
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> RemoteAgentAccessResponseAuthClaimV1 {
    RemoteAgentAccessResponseAuthClaimV1::try_new(
        carrier,
        carrier.runtime_response_key(),
        ApplyAuthAlgorithm::try_new(1).expect("outer Ed25519 algorithm"),
        1,
    )
    .expect("PXRR auth claim")
}

fn active_terminal_receipt(
    request: &RemoteAgentDataPlaneApplyRequestV1,
    auth: RemoteAgentDataPlaneTerminalAuthClaimV1,
    completion_runtime_host_epoch: u64,
) -> RemoteAgentDataPlaneTerminalReceiptV1 {
    let state = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        Some(8),
        Some(9),
        Some(33),
    );
    let fresh = runtime_agent_control_descriptor_payload_digest_v1(FRESH_DESCRIPTOR)
        .expect("fresh PXAP digest");
    let mut fields = evidence_fields(
        request,
        true,
        true,
        true,
        RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
        false,
        fresh,
    );
    fields.completion_runtime_host_epoch = completion_runtime_host_epoch;
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(fields)
        .expect("active terminal evidence");
    RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(request, state, evidence, auth)
        .expect("active PXAU draft")
        .finalize(&[0xd3; 64])
        .expect("active PXAU")
}

fn apply_access_request(
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
) -> RemoteAgentAccessRequestV1 {
    RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        access_fields(inner, carrier, &[0xe1; 32]),
        managed_agent_receipt().receipt_digest(),
        inner.clone(),
    )
    .expect("PXRA Apply draft")
    .finalize(&[0xe2; 64])
    .expect("PXRA Apply")
}

fn describe_access_request(
    inner: &RemoteAgentDataPlaneApplyRequestV1,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV1,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
) -> RemoteAgentAccessRequestV1 {
    let profile = inner.target_execution().profile();
    RemoteAgentAccessRequestDraftV1::try_describe_remote_access(
        access_fields(inner, carrier, &[0xe3; 32]),
        terminal.receipt_digest(),
        managed_agent_receipt().receipt_digest(),
        profile.profile_digest(),
        profile.mac_agent_client_principal(),
    )
    .expect("PXRA Describe draft")
    .finalize(&[0xe4; 64])
    .expect("PXRA Describe")
}

fn reissue_data_plane_request(
    request: &RemoteAgentDataPlaneApplyRequestV1,
    auth_claim: ApplyRequestAuthClaim,
    signature: &[u8],
) -> RemoteAgentDataPlaneApplyRequestV1 {
    let predecessor = managed_agent_request();
    RemoteAgentDataPlaneApplyRequestDraftV1::try_new(
        request.target_execution().clone(),
        predecessor.provenance(),
        predecessor.control_commitment().control().clone(),
        predecessor.temporal(),
        predecessor.expected_runtime_store_instance_id(),
        auth_claim,
    )
    .expect("reissued PXAR v10 draft")
    .finalize(signature)
    .expect("reissued structural PXAR v10")
}

#[test]
fn pxae_pxad_pxte9_pxar10_round_trip_retain_exact_pxte6_and_pxta_zero() {
    let predecessor_request = managed_agent_request();
    let projection = RemoteAgentDataPlaneProjectionV1::try_from_managed_agent_stack_projection(
        predecessor_request.target_execution().projection().clone(),
    )
    .expect("PXAE projection");
    let projection_round_trip =
        RemoteAgentDataPlaneProjectionV1::decode(projection.canonical_wire())
            .expect("PXAE round trip");
    assert_eq!(projection_round_trip, projection);

    let profile = profile_for(
        &predecessor_request,
        "tcp/127.0.0.1:7447",
        101,
        PrincipalRef::from_bytes([0xa1; 16]),
        PrincipalRef::from_bytes([0xa2; 16]),
    )
    .expect("PXAD profile");
    let profile_round_trip =
        RemoteAgentDataPlaneProfileV1::decode(profile.canonical_wire()).expect("PXAD round trip");
    assert_eq!(profile_round_trip, profile);

    let execution = RemoteAgentDataPlaneTargetExecutionV1::try_local_agent_only_deactivate(
        projection,
        predecessor_request.target_execution().clone(),
        profile,
    )
    .expect("PXTE v9 local-only execution");
    let execution_round_trip =
        RemoteAgentDataPlaneTargetExecutionV1::decode(execution.canonical_wire())
            .expect("PXTE v9 round trip");
    assert_eq!(execution_round_trip, execution);
    assert_eq!(
        execution_round_trip.predecessor().canonical_wire(),
        predecessor_request.target_execution().canonical_wire(),
        "PXTE v9 must retain the byte-exact PXTE v6 predecessor",
    );

    let request = RemoteAgentDataPlaneApplyRequestDraftV1::try_new(
        execution,
        predecessor_request.provenance(),
        predecessor_request.control_commitment().control().clone(),
        predecessor_request.temporal(),
        predecessor_request.expected_runtime_store_instance_id(),
        predecessor_request.authentication().claim().clone(),
    )
    .expect("PXAR v10 draft")
    .finalize(&[0xc1; 64])
    .expect("PXAR v10");
    let request_round_trip = RemoteAgentDataPlaneApplyRequestV1::decode(request.canonical_wire())
        .expect("PXAR v10 round trip");
    assert_eq!(request_round_trip, request);
    assert_eq!(&request_round_trip.canonical_slice_wire()[..10], EMPTY_PXTA);
    assert_eq!(
        request_round_trip
            .target_execution()
            .predecessor()
            .canonical_wire(),
        predecessor_request.target_execution().canonical_wire(),
    );

    assert!(
        profile_for(
            &predecessor_request,
            "tcp/127.0.0.1:7447",
            0,
            PrincipalRef::from_bytes([0xa1; 16]),
            PrincipalRef::from_bytes([0xa2; 16]),
        )
        .is_err()
    );
    assert!(
        profile_for(
            &predecessor_request,
            "tcp/127.0.0.1:7447",
            101,
            PrincipalRef::from_bytes([0xa1; 16]),
            PrincipalRef::from_bytes([0xa1; 16]),
        )
        .is_err()
    );
    let mut projection_tamper = projection_round_trip.canonical_wire().to_vec();
    projection_tamper[0] ^= 1;
    assert!(RemoteAgentDataPlaneProjectionV1::decode(&projection_tamper).is_err());
}

#[test]
fn terminal_evidence_rejects_zero_time_wrong_temporal_lineage_and_impossible_census() {
    let request = data_plane_request(false);
    let valid = evidence_fields(
        &request,
        true,
        true,
        true,
        RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent,
        false,
        Digest32::from_bytes([0; 32]),
    );

    let mut zero_time = valid;
    zero_time.selection_observed_at_nanos = 0;
    assert!(RemoteAgentDataPlaneTerminalEvidenceV1::try_new(zero_time).is_err());

    let mut impossible_census = valid;
    impossible_census.physical_binding_census = 3;
    assert!(RemoteAgentDataPlaneTerminalEvidenceV1::try_new(impossible_census).is_err());

    let mut impossible_readiness = valid;
    impossible_readiness.base_fabric_ready = false;
    assert!(RemoteAgentDataPlaneTerminalEvidenceV1::try_new(impossible_readiness).is_err());

    let state = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        Some(8),
        Some(9),
        None,
    );
    let mut wrong_lineage = valid;
    wrong_lineage.selection_clock_domain = ClockDomainRef::from_bytes([0xee; 16]);
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(wrong_lineage)
        .expect("wrong lineage remains structurally bounded");
    assert!(
        RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
            &request,
            state,
            evidence,
            terminal_auth(),
        )
        .is_err()
    );

    let mut future_generation = valid;
    future_generation.selection_clock_generation =
        ClockGeneration::try_new(request.temporal().target_clock_generation().value() + 1)
            .expect("future clock generation");
    accepts_terminal(&request, state, future_generation);
}

#[test]
fn pxau_outcome_evidence_matrix_is_mode_and_generation_strict() {
    let active = data_plane_request(true);
    let local = data_plane_request(false);
    let zero = Digest32::from_bytes([0; 32]);
    let fresh = runtime_agent_control_descriptor_payload_digest_v1(FRESH_DESCRIPTOR)
        .expect("fresh PXAP digest");

    let active_ready = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        Some(8),
        Some(9),
        Some(33),
    );
    accepts_terminal(
        &active,
        active_ready,
        evidence_fields(
            &active,
            true,
            true,
            true,
            RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
            false,
            fresh,
        ),
    );

    let local_ready = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::LocalOnlyReady,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        Some(8),
        Some(9),
        None,
    );
    accepts_terminal(
        &local,
        local_ready,
        evidence_fields(
            &local,
            true,
            true,
            true,
            RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent,
            false,
            zero,
        ),
    );

    let no_effect_unknown = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::ProvenNotStarted,
        RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
        None,
        None,
        None,
    );
    accepts_terminal(
        &active,
        no_effect_unknown,
        evidence_fields(
            &active,
            false,
            false,
            false,
            RemoteAgentDataPlaneRemoteObservationV1::Unknown,
            false,
            zero,
        ),
    );

    let preserved = managed_agent_request().target_slice_digest();
    let no_effect_absent = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::ProvenNotStarted,
        RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(preserved),
        Some(7),
        Some(8),
        None,
    );
    accepts_terminal(
        &local,
        no_effect_absent,
        evidence_fields(
            &local,
            true,
            true,
            true,
            RemoteAgentDataPlaneRemoteObservationV1::RemoteAbsent,
            false,
            zero,
        ),
    );

    let no_effect_preserved_active = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::NoEffectRejected,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::ProvenNotStarted,
        RemoteAgentDataPlaneTerminalHeadV1::PreservedExisting(preserved),
        Some(7),
        Some(8),
        Some(32),
    );
    accepts_terminal(
        &active,
        no_effect_preserved_active,
        evidence_fields(
            &active,
            true,
            true,
            true,
            RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
            false,
            fresh,
        ),
    );

    let uncertain = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::Uncertain,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
        None,
        None,
        None,
    );
    accepts_terminal(
        &active,
        uncertain,
        evidence_fields(
            &active,
            false,
            false,
            false,
            RemoteAgentDataPlaneRemoteObservationV1::PartialOrConflicting,
            false,
            zero,
        ),
    );

    let quarantined = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::Quarantined,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::PreservedNone,
        None,
        None,
        None,
    );
    accepts_terminal(
        &active,
        quarantined,
        evidence_fields(
            &active,
            false,
            false,
            false,
            RemoteAgentDataPlaneRemoteObservationV1::Unknown,
            true,
            zero,
        ),
    );

    let cas = active
        .target_execution()
        .bootstrap_cas()
        .expect("active CAS");
    let equal_generation = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        Some(cas.expected_fabric_generation().value()),
        Some(cas.expected_agent_generation().value() + 1),
        Some(33),
    );
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(evidence_fields(
        &active,
        true,
        true,
        true,
        RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
        false,
        fresh,
    ))
    .expect("active evidence");
    assert!(
        RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
            &active,
            equal_generation,
            evidence,
            terminal_auth(),
        )
        .is_err()
    );

    let same_descriptor = evidence_fields(
        &active,
        true,
        true,
        true,
        RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
        false,
        cas.expected_bootstrap_descriptor_payload_digest(),
    );
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(same_descriptor)
        .expect("same descriptor remains structurally bounded");
    assert!(
        RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
            &active,
            active_ready,
            evidence,
            terminal_auth(),
        )
        .is_err()
    );
}

#[test]
fn pxau_round_trip_authenticates_and_rejects_tamper_wrong_request_and_old_protocols() {
    let request = data_plane_request(true);
    let state = terminal_state(
        RemoteAgentDataPlaneTerminalOutcomeV1::ActiveReady,
        RemoteAgentDataPlaneTerminalLifecycleEffectV1::MayHaveStarted,
        RemoteAgentDataPlaneTerminalHeadV1::CommittedIncoming,
        Some(8),
        Some(9),
        Some(33),
    );
    let fresh = runtime_agent_control_descriptor_payload_digest_v1(FRESH_DESCRIPTOR)
        .expect("fresh PXAP digest");
    let evidence = RemoteAgentDataPlaneTerminalEvidenceV1::try_new(evidence_fields(
        &request,
        true,
        true,
        true,
        RemoteAgentDataPlaneRemoteObservationV1::ListenerAndClientAclReady,
        false,
        fresh,
    ))
    .expect("active evidence");
    let auth = terminal_auth();
    let receipt =
        RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(&request, state, evidence, auth)
            .expect("PXAU draft")
            .finalize(&[0xd3; 64])
            .expect("PXAU");
    let decoded = RemoteAgentDataPlaneTerminalReceiptV1::decode(receipt.canonical_wire())
        .expect("PXAU round trip");
    assert_eq!(decoded, receipt);
    decoded
        .verify_runtime_terminal(
            &request,
            auth,
            |principal, key, algorithm, version, transcript, signature| {
                principal == auth.runtime_principal()
                    && key == auth.key()
                    && algorithm == auth.algorithm()
                    && version == auth.algorithm_version()
                    && !transcript.is_empty()
                    && signature == [0xd3; 64]
            },
        )
        .expect("authenticated PXAU");

    assert!(
        decoded
            .validate_against_request(&data_plane_request(false))
            .is_err()
    );
    assert!(
        RemoteAgentDataPlaneTerminalReceiptV1::decode(managed_agent_receipt().canonical_wire(),)
            .is_err()
    );

    let mut magic_tamper = receipt.canonical_wire().to_vec();
    magic_tamper[0] ^= 1;
    assert!(RemoteAgentDataPlaneTerminalReceiptV1::decode(&magic_tamper).is_err());
    let mut version_tamper = receipt.canonical_wire().to_vec();
    version_tamper[5] ^= 1;
    assert!(RemoteAgentDataPlaneTerminalReceiptV1::decode(&version_tamper).is_err());
    let mut payload_tamper = receipt.canonical_wire().to_vec();
    payload_tamper[64] ^= 1;
    let payload_tamper = RemoteAgentDataPlaneTerminalReceiptV1::decode(&payload_tamper)
        .expect("opaque signature does not make decode a verifier");
    assert!(payload_tamper.validate_against_request(&request).is_err());
}

#[test]
fn pxra_apply_and_describe_round_trip_authenticate_exact_cross_pins() {
    let inner = data_plane_request(true);
    let carrier = carrier_for(&inner);
    let terminal = active_terminal_receipt(&inner, terminal_auth(), RUNTIME_EPOCH);

    let apply = apply_access_request(&inner, carrier.clone());
    let decoded_apply = RemoteAgentAccessRequestV1::decode(apply.canonical_wire())
        .expect("PXRA Apply round trip");
    assert_eq!(decoded_apply, apply);
    assert_eq!(decoded_apply.request_id().as_bytes(), inner.operation_id().as_bytes());
    assert_eq!(decoded_apply.carrier(), &carrier);
    assert_eq!(
        decoded_apply
            .apply_request()
            .expect("exact PXAR v10 payload")
            .canonical_wire(),
        inner.canonical_wire(),
    );
    assert_eq!(
        decoded_apply.expected_active_pxst_digest(),
        managed_agent_receipt().receipt_digest(),
    );
    authenticate_access_request(&decoded_apply, &carrier, &[0xe2; 64]);

    let describe = describe_access_request(&inner, &terminal, carrier.clone());
    let decoded_describe = RemoteAgentAccessRequestV1::decode(describe.canonical_wire())
        .expect("PXRA Describe round trip");
    assert_eq!(decoded_describe, describe);
    assert!(decoded_describe.apply_request().is_none());
    assert_eq!(decoded_describe.expected_pxau_digest(), terminal.receipt_digest());
    assert_eq!(
        decoded_describe.profile_digest(),
        inner.target_execution().profile().profile_digest(),
    );
    assert_eq!(
        decoded_describe.intended_mac_agent_client(),
        inner
            .target_execution()
            .profile()
            .mac_agent_client_principal(),
    );
    authenticate_access_request(&decoded_describe, &carrier, &[0xe4; 64]);
}

#[test]
fn pxra_rejects_non_ed25519_inner_outer_nonce_and_principal_aliases() {
    let inner = data_plane_request(true);
    let carrier = carrier_for(&inner);
    let pxst = managed_agent_receipt().receipt_digest();

    let mut non_ed25519_outer = access_fields(&inner, carrier.clone(), &[0xe1; 32]);
    non_ed25519_outer.auth_claim = ApplyRequestAuthClaim::try_new(
        carrier.controller_principal(),
        carrier.controller_request_key(),
        ApplyAuthAlgorithm::try_new(2).expect("non-Ed25519 algorithm"),
        1,
        &[0xe1; 32],
    )
    .expect("algorithm-agile base claim");
    assert!(RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        non_ed25519_outer,
        pxst,
        inner.clone(),
    )
    .is_err());

    let mut same_nonce = access_fields(&inner, carrier.clone(), &[0xe1; 32]);
    same_nonce.auth_claim = ApplyRequestAuthClaim::try_new(
        carrier.controller_principal(),
        carrier.controller_request_key(),
        ApplyAuthAlgorithm::try_new(1).expect("Ed25519 algorithm"),
        1,
        inner.authentication().claim().nonce(),
    )
    .expect("same-nonce claim remains structurally valid");
    assert!(RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        same_nonce,
        pxst,
        inner.clone(),
    )
    .is_err());

    let mut zero_nonce = access_fields(&inner, carrier.clone(), &[0xe1; 32]);
    zero_nonce.auth_claim = ApplyRequestAuthClaim::try_new(
        carrier.controller_principal(),
        carrier.controller_request_key(),
        ApplyAuthAlgorithm::try_new(1).expect("Ed25519 algorithm"),
        1,
        &[0; 32],
    )
    .expect("base claim permits opaque zero bytes");
    assert!(RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        zero_nonce,
        pxst,
        inner.clone(),
    )
    .is_err());

    let invalid_inner_claim = ApplyRequestAuthClaim::try_new(
        inner.authentication().claim().principal(),
        inner.authentication().claim().key(),
        ApplyAuthAlgorithm::try_new(2).expect("non-Ed25519 algorithm"),
        1,
        inner.authentication().claim().nonce(),
    )
    .expect("algorithm-agile inner claim");
    let invalid_inner = reissue_data_plane_request(&inner, invalid_inner_claim, &[0xc2; 64]);
    assert!(RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        access_fields(&invalid_inner, carrier.clone(), &[0xe1; 32]),
        pxst,
        invalid_inner,
    )
    .is_err());

    let short_inner = reissue_data_plane_request(
        &inner,
        inner.authentication().claim().clone(),
        &[0xc3; 63],
    );
    assert!(RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        access_fields(&short_inner, carrier.clone(), &[0xe1; 32]),
        pxst,
        short_inner,
    )
    .is_err());

    let apply_draft = RemoteAgentAccessRequestDraftV1::try_apply_remote_access(
        access_fields(&inner, carrier.clone(), &[0xe1; 32]),
        pxst,
        inner.clone(),
    )
    .expect("valid PXRA Apply draft");
    assert!(apply_draft.finalize(&[0xe2; 63]).is_err());

    let terminal = active_terminal_receipt(&inner, terminal_auth(), RUNTIME_EPOCH);
    let profile = inner.target_execution().profile();
    assert!(RemoteAgentAccessRequestDraftV1::try_describe_remote_access(
        access_fields(&inner, carrier.clone(), &[0xe3; 32]),
        terminal.receipt_digest(),
        pxst,
        profile.profile_digest(),
        carrier.controller_principal(),
    )
    .is_err());
    assert!(RemoteAgentAccessRequestDraftV1::try_describe_remote_access(
        access_fields(&inner, carrier.clone(), &[0xe3; 32]),
        terminal.receipt_digest(),
        pxst,
        profile.profile_digest(),
        carrier.runtime_principal(),
    )
    .is_err());
}

#[test]
fn pxrr_apply_requires_both_signatures_exact_runtime_epoch_and_runtime_principal() {
    let inner = data_plane_request(true);
    let carrier = carrier_for(&inner);
    let request = apply_access_request(&inner, carrier.clone());
    let authenticated_request = authenticate_access_request(&request, &carrier, &[0xe2; 64]);
    let inner_auth = terminal_auth();
    assert_ne!(inner_auth.key(), carrier.runtime_response_key());
    let terminal = active_terminal_receipt(&inner, inner_auth, RUNTIME_EPOCH);
    let authenticated_terminal = terminal
        .verify_runtime_terminal(
            &inner,
            inner_auth,
            |_, _, _, _, transcript, signature| {
                !transcript.is_empty() && signature == [0xd3; 64]
            },
        )
        .expect("Runtime-authenticated inner PXAU");
    let response = RemoteAgentAccessResponseDraftV1::try_apply_remote_access(
        authenticated_request,
        authenticated_terminal,
        access_response_auth(&carrier),
    )
    .expect("PXRR Apply draft")
    .finalize(&[0xe5; 64])
    .expect("PXRR Apply");
    let decoded = RemoteAgentAccessResponseV1::decode(response.canonical_wire())
        .expect("PXRR Apply round trip");
    assert_eq!(decoded, response);
    assert_eq!(
        decoded
            .apply_receipt()
            .expect("exact PXAU payload")
            .canonical_wire(),
        terminal.canonical_wire(),
    );

    let inner_calls = Cell::new(0);
    let outer_calls = Cell::new(0);
    decoded
        .verify_runtime_apply_response(
            &request,
            &carrier,
            inner_auth,
            |principal, key, algorithm, version, transcript, signature| {
                inner_calls.set(inner_calls.get() + 1);
                principal == inner_auth.runtime_principal()
                    && key == inner_auth.key()
                    && algorithm == inner_auth.algorithm()
                    && version == inner_auth.algorithm_version()
                    && !transcript.is_empty()
                    && signature == [0xd3; 64]
            },
            |principal, key, fingerprint, transcript, signature| {
                outer_calls.set(outer_calls.get() + 1);
                principal == carrier.runtime_principal()
                    && key == carrier.runtime_response_key()
                    && fingerprint == carrier.runtime_response_key_fingerprint()
                    && !transcript.is_empty()
                    && signature == [0xe5; 64]
            },
        )
        .expect("independently authenticated inner PXAU and outer PXRR");
    assert_eq!(inner_calls.get(), 1);
    assert_eq!(outer_calls.get(), 1);

    assert!(decoded
        .verify_runtime_apply_response(
            &request,
            &carrier,
            inner_auth,
            |_, _, _, _, _, _| false,
            |_, _, _, _, _| true,
        )
        .is_err());
    assert!(decoded
        .verify_runtime_apply_response(
            &request,
            &carrier,
            inner_auth,
            |_, _, _, _, _, _| true,
            |_, _, _, _, _| false,
        )
        .is_err());

    let wrong_epoch_terminal =
        active_terminal_receipt(&inner, inner_auth, RUNTIME_EPOCH + 1);
    let wrong_epoch_marker = wrong_epoch_terminal
        .verify_runtime_terminal(&inner, inner_auth, |_, _, _, _, _, _| true)
        .expect("independently valid wrong-epoch PXAU");
    assert!(RemoteAgentAccessResponseDraftV1::try_apply_remote_access(
        authenticated_request,
        wrong_epoch_marker,
        access_response_auth(&carrier),
    )
    .is_err());

    let wrong_principal_auth = RemoteAgentDataPlaneTerminalAuthClaimV1::try_new(
        PrincipalRef::from_bytes([0xbc; 16]),
        inner_auth.key(),
        inner_auth.algorithm(),
        inner_auth.algorithm_version(),
    )
    .expect("independent wrong-principal PXAU auth");
    let wrong_principal_terminal =
        active_terminal_receipt(&inner, wrong_principal_auth, RUNTIME_EPOCH);
    let wrong_principal_marker = wrong_principal_terminal
        .verify_runtime_terminal(&inner, wrong_principal_auth, |_, _, _, _, _, _| true)
        .expect("independently valid wrong-principal PXAU");
    assert!(RemoteAgentAccessResponseDraftV1::try_apply_remote_access(
        authenticated_request,
        wrong_principal_marker,
        access_response_auth(&carrier),
    )
    .is_err());

    let mut epoch_tamper = response.canonical_wire().to_vec();
    epoch_tamper[159] ^= 1;
    assert!(RemoteAgentAccessResponseV1::decode(&epoch_tamper).is_err());
}

#[test]
fn pxrr_describe_is_correlated_opaque_structure_not_capability_and_cross_rejects_protocols() {
    let inner = data_plane_request(true);
    let carrier = carrier_for(&inner);
    let terminal = active_terminal_receipt(&inner, terminal_auth(), RUNTIME_EPOCH);
    let request = describe_access_request(&inner, &terminal, carrier.clone());
    let authenticated = authenticate_access_request(&request, &carrier, &[0xe4; 64]);
    let profile = inner.target_execution().profile().clone();
    let response = RemoteAgentAccessResponseDraftV1::try_describe_remote_access(
        authenticated,
        profile.clone(),
        FRESH_DESCRIPTOR,
        generation(8),
        generation(9),
        generation(33),
        access_response_auth(&carrier),
    )
    .expect("structural PXRR Describe draft")
    .finalize(&[0xe6; 64])
    .expect("PXRR Describe");
    let decoded = RemoteAgentAccessResponseV1::decode(response.canonical_wire())
        .expect("PXRR Describe round trip");
    assert_eq!(decoded, response);
    assert_eq!(decoded.profile(), Some(&profile));
    assert_eq!(decoded.descriptor(), Some(FRESH_DESCRIPTOR));
    assert_eq!(
        decoded.descriptor_digest(),
        runtime_agent_control_descriptor_payload_digest_v1(FRESH_DESCRIPTOR)
            .expect("shared PXAP digest"),
    );
    assert_ne!(
        profile.endpoint_generation(),
        decoded
            .access_generation()
            .expect("physical access generation")
            .value(),
        "PXAD endpoint generation and physical access generation are independent domains",
    );
    decoded
        .verify_runtime_describe_response(
            &request,
            &carrier,
            |principal, key, fingerprint, transcript, signature| {
                principal == carrier.runtime_principal()
                    && key == carrier.runtime_response_key()
                    && fingerprint == carrier.runtime_response_key_fingerprint()
                    && !transcript.is_empty()
                    && signature == [0xe6; 64]
            },
        )
        .expect("Runtime-authenticated structural Describe facts");

    let other_request = RemoteAgentAccessRequestDraftV1::try_describe_remote_access(
        access_fields(&inner, carrier.clone(), &[0xe7; 32]),
        terminal.receipt_digest(),
        managed_agent_receipt().receipt_digest(),
        profile.profile_digest(),
        profile.mac_agent_client_principal(),
    )
    .expect("other Describe draft")
    .finalize(&[0xe8; 64])
    .expect("other Describe");
    assert!(decoded.validate_against_request(&other_request).is_err());
    assert!(RemoteAgentAccessRequestV1::decode(inner.canonical_wire()).is_err());
    assert!(RemoteAgentAccessResponseV1::decode(terminal.canonical_wire()).is_err());

    for magic in [b"PXAG", b"PXCC", b"PXDR"] {
        let mut cross_request = request.canonical_wire().to_vec();
        cross_request[..4].copy_from_slice(magic);
        assert!(RemoteAgentAccessRequestV1::decode(&cross_request).is_err());
    }
    for magic in [b"PXAH", b"PXCC", b"PXDR", b"PXAU"] {
        let mut cross_response = response.canonical_wire().to_vec();
        cross_response[..4].copy_from_slice(magic);
        assert!(RemoteAgentAccessResponseV1::decode(&cross_response).is_err());
    }
}
