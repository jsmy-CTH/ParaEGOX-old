use paraegox_kernel::digest::Digest32;
use paraegox_kernel::identity::PrincipalRef;
use paraegox_kernel::time::{ClockDomainRef, ClockGeneration};

use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    DistributedFabricCredentialRefV1, DistributedFabricTrustAnchorRefV1,
    DistributedFabricTrustDomainRefV1,
};
use paraegox_runtime_contracts::managed_agent_stack_plan::{
    ManagedAgentStackApplyRequestV1, ManagedAgentStackTerminalReceiptV1,
};
use paraegox_runtime_contracts::managed_fabric_plan::ManagedFabricApplyTerminalReceiptV1;
use paraegox_runtime_contracts::managed_service::ManagedServiceGeneration;
use paraegox_runtime_contracts::managed_serving_bootstrap::runtime_agent_control_descriptor_payload_digest_v1;
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
use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyAuthKeyRef};

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

fn projection_for(
    request: &ManagedAgentStackApplyRequestV1,
) -> RemoteAgentDataPlaneProjectionV1 {
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
    let (echoed_receipt, echoed_payload) = request
        .target_execution()
        .bootstrap_cas()
        .map_or((Digest32::from_bytes([0; 32]), Digest32::from_bytes([0; 32])), |cas| {
            (
                cas.expected_bootstrap_descriptor_receipt_digest(),
                cas.expected_bootstrap_descriptor_payload_digest(),
            )
        });
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
        RemoteAgentDataPlaneProfileV1::decode(profile.canonical_wire())
            .expect("PXAD round trip");
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
    assert!(RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
        &request,
        state,
        evidence,
        terminal_auth(),
    )
    .is_err());

    let mut future_generation = valid;
    future_generation.selection_clock_generation = ClockGeneration::try_new(
        request.temporal().target_clock_generation().value() + 1,
    )
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
    assert!(RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
        &active,
        equal_generation,
        evidence,
        terminal_auth(),
    )
    .is_err());

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
    assert!(RemoteAgentDataPlaneTerminalReceiptDraftV1::try_new(
        &active,
        active_ready,
        evidence,
        terminal_auth(),
    )
    .is_err());
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

    assert!(decoded.validate_against_request(&data_plane_request(false)).is_err());
    assert!(RemoteAgentDataPlaneTerminalReceiptV1::decode(
        managed_agent_receipt().canonical_wire(),
    )
    .is_err());

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
