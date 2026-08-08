use paraegox_kernel::identity::PrincipalRef;

use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    DistributedFabricCredentialRefV1, DistributedFabricTrustAnchorRefV1,
    DistributedFabricTrustDomainRefV1,
};
use paraegox_runtime_contracts::managed_agent_stack_plan::ManagedAgentStackApplyRequestV1;
use paraegox_runtime_contracts::remote_agent_data_plane_plan::{
    RemoteAgentDataPlaneApplyRequestDraftV1, RemoteAgentDataPlaneApplyRequestV1,
    RemoteAgentDataPlaneProfileFieldsV1, RemoteAgentDataPlaneProfileV1,
    RemoteAgentDataPlaneProjectionV1, RemoteAgentDataPlaneTargetExecutionV1,
};

const AGENT_STACK_FIXTURE: &str =
    include_str!("../../../tests/fixtures/wire/s7_managed_agent_stack_successor_v1.json");
const EMPTY_PXTA: &[u8; 10] = b"PXTA\0\x01\0\0\0\0";

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

fn profile_for(
    request: &ManagedAgentStackApplyRequestV1,
    base_loopback_listen_endpoint: &str,
    endpoint_generation: u64,
    mac_agent_client_principal: PrincipalRef,
    ubuntu_agent_listener_principal: PrincipalRef,
) -> Result<RemoteAgentDataPlaneProfileV1, paraegox_runtime_contracts::remote_agent_data_plane_plan::RemoteAgentDataPlanePlanError>
{
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
        mac_connector_credential_ref: DistributedFabricCredentialRefV1::try_from_bytes([
            0x94; 16
        ])
        .expect("Mac credential ref"),
        ubuntu_listener_credential_ref: DistributedFabricCredentialRefV1::try_from_bytes([
            0x95; 16
        ])
        .expect("Ubuntu credential ref"),
        mac_agent_client_principal,
        ubuntu_agent_listener_principal,
        operation_timeout_nanos: 5_000_000_000,
    })
}

#[test]
fn pxae_pxad_pxte9_pxar10_round_trip_retain_exact_pxte6_and_pxta_zero() {
    let predecessor_request = managed_agent_request();
    let projection = RemoteAgentDataPlaneProjectionV1::try_from_managed_agent_stack_projection(
        predecessor_request
            .target_execution()
            .projection()
            .clone(),
    )
    .expect("PXAE projection");
    let projection_round_trip = RemoteAgentDataPlaneProjectionV1::decode(
        projection.canonical_wire(),
    )
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
    let profile_round_trip = RemoteAgentDataPlaneProfileV1::decode(profile.canonical_wire())
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
    .finalize(predecessor_request.authentication().signature())
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

    assert!(profile_for(
        &predecessor_request,
        "tcp/127.0.0.1:7447",
        0,
        PrincipalRef::from_bytes([0xa1; 16]),
        PrincipalRef::from_bytes([0xa2; 16]),
    )
    .is_err());
    assert!(profile_for(
        &predecessor_request,
        "tcp/127.0.0.1:7447",
        101,
        PrincipalRef::from_bytes([0xa1; 16]),
        PrincipalRef::from_bytes([0xa1; 16]),
    )
    .is_err());
    let mut projection_tamper = projection_round_trip.canonical_wire().to_vec();
    projection_tamper[0] ^= 1;
    assert!(RemoteAgentDataPlaneProjectionV1::decode(&projection_tamper).is_err());
}
