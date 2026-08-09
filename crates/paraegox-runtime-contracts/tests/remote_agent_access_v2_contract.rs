use std::cell::RefCell;

use paraegox_kernel::digest::Digest32;

use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    RestrictedRuntimeApplyCarrierBindingFieldsV1, RestrictedRuntimeApplyCarrierBindingV1,
};
use paraegox_runtime_contracts::remote_agent_access::{
    MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES, MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES,
    REMOTE_AGENT_ACCESS_REQUEST_MAGIC, REMOTE_AGENT_ACCESS_RESPONSE_MAGIC,
    REMOTE_AGENT_ACCESS_V2_VERSION, RemoteAgentAccessKindV2, RemoteAgentAccessRequestDraftV2,
    RemoteAgentAccessRequestFieldsV2, RemoteAgentAccessRequestIdV2, RemoteAgentAccessRequestV2,
    RemoteAgentAccessResponseAuthClaimV2, RemoteAgentAccessResponseDraftV2,
    RemoteAgentAccessResponseV2,
};
use paraegox_runtime_contracts::remote_agent_data_plane_plan::{
    REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES, REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES,
    RemoteAgentActiveS1CasV2, RemoteAgentDataPlaneApplyRequestV2,
    RemoteAgentDataPlaneTerminalReceiptV2,
};
use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyRequestAuthClaim};

const PROXY_DATA_PLANE_V2_GOLDEN: &str =
    include_str!("../../../tests/fixtures/wire/t2_remote_agent_proxy_data_plane_v2.json");
const OUTER_CONTROLLER_NONCE: &[u8; 32] = &[0xe1; 32];
const DESCRIBE_CONTROLLER_NONCE: &[u8; 32] = &[0xe3; 32];
const OUTER_CONTROLLER_SIGNATURE: &[u8; 64] = &[0xe2; 64];
const DESCRIBE_CONTROLLER_SIGNATURE: &[u8; 64] = &[0xe4; 64];
const OUTER_RUNTIME_SIGNATURE: &[u8; 64] = &[0xe5; 64];

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("non-hex fixture byte"),
    }
}

fn fixture_string_after<'a>(fixture: &'a str, key: &str) -> &'a str {
    let key_start = fixture
        .find(key)
        .map(|offset| offset + key.len())
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

fn fixture_hex_after(fixture: &str, key: &str) -> Vec<u8> {
    fixture_string_after(fixture, key)
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn fixture_section_after<'a>(fixture: &'a str, section: &str) -> &'a str {
    let section_start = fixture.find(section).expect("fixture section");
    &fixture[section_start..]
}

fn active_inner_request() -> RemoteAgentDataPlaneApplyRequestV2 {
    let active = fixture_section_after(PROXY_DATA_PLANE_V2_GOLDEN, "\"active_ready\"");
    let pxar = fixture_section_after(active, "\"pxar_v11\"");
    RemoteAgentDataPlaneApplyRequestV2::decode(&fixture_hex_after(pxar, "\"wire_hex\""))
        .expect("shared-golden PXAR v11")
}

fn active_inner_terminal() -> RemoteAgentDataPlaneTerminalReceiptV2 {
    let active = fixture_section_after(PROXY_DATA_PLANE_V2_GOLDEN, "\"active_ready\"");
    let pxau = fixture_section_after(active, "\"pxau_v2\"");
    RemoteAgentDataPlaneTerminalReceiptV2::decode(&fixture_hex_after(pxau, "\"wire_hex\""))
        .expect("shared-golden PXAU v2")
}

fn current_active_s1_cas() -> RemoteAgentActiveS1CasV2 {
    let local_only =
        fixture_section_after(PROXY_DATA_PLANE_V2_GOLDEN, "\"local_only_ready\"");
    let expected_s1 = fixture_section_after(local_only, "\"expected_s1_cas\"");
    RemoteAgentActiveS1CasV2::decode(&fixture_hex_after(expected_s1, "\"wire_hex\""))
        .expect("shared-golden active S1 CAS")
}

fn carrier_for(
    request: &RemoteAgentDataPlaneApplyRequestV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
) -> RestrictedRuntimeApplyCarrierBindingV1 {
    let request_auth = request.authentication().claim();
    let terminal_auth = terminal.authentication();
    RestrictedRuntimeApplyCarrierBindingV1::try_new(
        RestrictedRuntimeApplyCarrierBindingFieldsV1 {
            target: request.target(),
            runtime_principal: terminal_auth.runtime_principal(),
            controller_principal: request_auth.principal(),
            endpoint_ref: [0xb5; 16],
            endpoint_generation: 11,
            route: "paraegox/runtime/control/v1/apply",
            controller_request_key: request_auth.key(),
            controller_request_key_fingerprint: Digest32::from_bytes([0xb6; 32]),
            runtime_response_key: terminal_auth.key(),
            runtime_response_key_fingerprint: Digest32::from_bytes([0xb8; 32]),
            control_transport_profile_ref: [0xb9; 16],
            control_transport_profile_digest: Digest32::from_bytes([0xba; 32]),
        },
    )
    .expect("restricted PXCB")
}

fn access_fields(
    request: &RemoteAgentDataPlaneApplyRequestV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    nonce: &[u8],
) -> RemoteAgentAccessRequestFieldsV2 {
    RemoteAgentAccessRequestFieldsV2 {
        request_id: RemoteAgentAccessRequestIdV2::try_from_bytes(
            *request.operation_id().as_bytes(),
        )
        .expect("PXRA v2 request id"),
        target: request.target(),
        expected_runtime_store_instance_id: request.expected_runtime_store_instance_id(),
        expected_runtime_host_epoch: terminal
            .facts()
            .evidence()
            .fields()
            .completion_runtime_host_epoch,
        auth_claim: ApplyRequestAuthClaim::try_new(
            carrier.controller_principal(),
            carrier.controller_request_key(),
            ApplyAuthAlgorithm::try_new(1).expect("Ed25519 algorithm"),
            1,
            nonce,
        )
        .expect("outer Controller authentication claim"),
        carrier,
    }
}

fn apply_access_request(
    inner: &RemoteAgentDataPlaneApplyRequestV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
) -> RemoteAgentAccessRequestV2 {
    RemoteAgentAccessRequestDraftV2::try_apply_remote_access(
        access_fields(inner, terminal, carrier, OUTER_CONTROLLER_NONCE),
        inner.clone(),
    )
    .expect("PXRA v2 Apply draft")
    .finalize(OUTER_CONTROLLER_SIGNATURE)
    .expect("PXRA v2 Apply")
}

fn response_auth(
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> RemoteAgentAccessResponseAuthClaimV2 {
    RemoteAgentAccessResponseAuthClaimV2::try_new(
        carrier,
        carrier.runtime_response_key(),
        ApplyAuthAlgorithm::try_new(1).expect("Ed25519 algorithm"),
        1,
    )
    .expect("outer Runtime authentication claim")
}

fn apply_access_response(
    request: &RemoteAgentAccessRequestV2,
    inner: &RemoteAgentDataPlaneApplyRequestV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
) -> RemoteAgentAccessResponseV2 {
    let authenticated_request = request
        .verify_controller_apply_request(carrier, |_, _, _, _, _, _| true, |_, _, _, _, _| true)
        .expect("structurally authenticated PXRA v2");
    let authenticated_terminal = terminal
        .verify_runtime_terminal(inner, terminal.authentication(), |_, _, _, _, _, _| true)
        .expect("structurally authenticated PXAU v2");
    RemoteAgentAccessResponseDraftV2::try_apply_remote_access(
        authenticated_request,
        authenticated_terminal,
        response_auth(carrier),
    )
    .expect("PXRR v2 Apply draft")
    .finalize(OUTER_RUNTIME_SIGNATURE)
    .expect("PXRR v2 Apply")
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().expect("u16 field"))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("u32 field"))
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("u64 field"))
}

#[test]
fn pxra2_apply_and_describe_freeze_fixed_header_offsets_bounds_and_controller_order() {
    assert_eq!(MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES, 7_855);
    assert_eq!(REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES, 200);
    assert_eq!(REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES, 152);

    let inner = active_inner_request();
    let terminal = active_inner_terminal();
    let carrier = carrier_for(&inner, &terminal);
    let request = apply_access_request(&inner, &terminal, carrier.clone());
    let wire = request.canonical_wire();
    let carrier_length = carrier.canonical_wire().len();
    let payload_length = inner.canonical_wire().len();
    let nonce_length = OUTER_CONTROLLER_NONCE.len();

    assert_eq!(wire.len(), 544 + nonce_length + carrier_length + payload_length + 64);
    assert_eq!(&wire[0..4], REMOTE_AGENT_ACCESS_REQUEST_MAGIC);
    assert_eq!(read_u16(&wire[4..6]), REMOTE_AGENT_ACCESS_V2_VERSION);
    assert_eq!(read_u16(&wire[6..8]), RemoteAgentAccessKindV2::ApplyRemoteAccess as u16);
    assert_eq!(&wire[8..10], &[0, 0]);
    assert_eq!(read_u16(&wire[10..12]) as usize, carrier_length);
    assert_eq!(read_u32(&wire[12..16]) as usize, payload_length);
    assert_eq!(&wire[16..32], inner.operation_id().as_bytes());
    assert_eq!(&wire[32..64], carrier.binding_digest().as_bytes());
    assert_eq!(&wire[64..80], inner.target().as_bytes());
    assert_eq!(&wire[80..112], &inner.expected_runtime_store_instance_id());
    assert_eq!(
        read_u64(&wire[112..120]),
        terminal
            .facts()
            .evidence()
            .fields()
            .completion_runtime_host_epoch,
    );
    assert_eq!(
        &wire[120..320],
        inner.target_execution().retained_s0_cas().canonical_wire(),
    );
    assert_eq!(
        &wire[320..472],
        inner.target_execution().expected_s1_cas().canonical_wire(),
    );
    assert_eq!(&wire[472..504], request.payload_wire_digest().as_bytes());
    assert_eq!(&wire[504..520], carrier.controller_principal().as_bytes());
    assert_eq!(&wire[520..536], carrier.controller_request_key().as_bytes());
    assert_eq!(read_u16(&wire[536..538]), 1);
    assert_eq!(read_u16(&wire[538..540]), 1);
    assert_eq!(read_u16(&wire[540..542]) as usize, nonce_length);
    assert_eq!(&wire[542..574], OUTER_CONTROLLER_NONCE);
    assert_eq!(read_u16(&wire[574..576]), 64);
    assert_eq!(&wire[576..576 + carrier_length], carrier.canonical_wire());
    assert_eq!(
        &wire[576 + carrier_length..576 + carrier_length + payload_length],
        inner.canonical_wire(),
    );
    assert_eq!(&wire[wire.len() - 64..], OUTER_CONTROLLER_SIGNATURE);
    assert_eq!(RemoteAgentAccessRequestV2::decode(wire).unwrap(), request);

    let inner_transcript = inner.signing_transcript().unwrap();
    let outer_transcript = request.signing_transcript().unwrap();
    let order = RefCell::new(Vec::new());
    request
        .verify_controller_apply_request(
            &carrier,
            |principal, key, algorithm, version, transcript, signature| {
                order.borrow_mut().push("inner-controller");
                principal == inner.authentication().claim().principal()
                    && key == inner.authentication().claim().key()
                    && algorithm == inner.authentication().claim().algorithm()
                    && version == inner.authentication().claim().algorithm_version()
                    && transcript == inner_transcript.as_bytes()
                    && signature == inner.authentication().signature()
            },
            |principal, key, fingerprint, transcript, signature| {
                order.borrow_mut().push("outer-controller");
                principal == carrier.controller_principal()
                    && key == carrier.controller_request_key()
                    && fingerprint == carrier.controller_request_key_fingerprint()
                    && transcript == outer_transcript.as_bytes()
                    && signature == OUTER_CONTROLLER_SIGNATURE
            },
        )
        .expect("independent inner then outer Controller authentication");
    assert_eq!(order.into_inner(), ["inner-controller", "outer-controller"]);

    let describe = RemoteAgentAccessRequestDraftV2::try_describe_remote_access(
        access_fields(
            &inner,
            &terminal,
            carrier.clone(),
            DESCRIBE_CONTROLLER_NONCE,
        ),
        inner.target_execution().retained_s0_cas(),
        current_active_s1_cas(),
    )
    .expect("PXRA v2 Describe draft")
    .finalize(DESCRIBE_CONTROLLER_SIGNATURE)
    .expect("PXRA v2 Describe");
    let describe_wire = describe.canonical_wire();
    assert_eq!(
        describe_wire.len(),
        544 + DESCRIBE_CONTROLLER_NONCE.len() + carrier_length + 64,
    );
    assert_eq!(read_u16(&describe_wire[6..8]), 2);
    assert_eq!(read_u32(&describe_wire[12..16]), 0);
    assert_eq!(&describe_wire[320..472], current_active_s1_cas().canonical_wire());
    assert_eq!(&describe_wire[472..504], &[0; 32]);
    assert!(describe.apply_request().is_none());
    assert_eq!(RemoteAgentAccessRequestV2::decode(describe_wire).unwrap(), describe);
    let describe_transcript = describe.signing_transcript().unwrap();
    describe
        .verify_controller_describe_request(
            &carrier,
            |principal, key, fingerprint, transcript, signature| {
                principal == carrier.controller_principal()
                    && key == carrier.controller_request_key()
                    && fingerprint == carrier.controller_request_key_fingerprint()
                    && transcript == describe_transcript.as_bytes()
                    && signature == DESCRIBE_CONTROLLER_SIGNATURE
            },
        )
        .expect("outer-only Controller-authenticated Describe");
}

#[test]
fn pxrr2_apply_freezes_fixed_header_offsets_and_runtime_verification_order() {
    assert_eq!(MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES, 5_792);

    let inner = active_inner_request();
    let terminal = active_inner_terminal();
    let carrier = carrier_for(&inner, &terminal);
    let request = apply_access_request(&inner, &terminal, carrier.clone());
    let response = apply_access_response(&request, &inner, &terminal, &carrier);
    let wire = response.canonical_wire();
    let nonce_length = OUTER_CONTROLLER_NONCE.len();
    let carrier_length = carrier.canonical_wire().len();
    let payload_length = terminal.canonical_wire().len();

    assert_eq!(wire.len(), 646 + nonce_length + carrier_length + payload_length + 64);
    assert_eq!(&wire[0..4], REMOTE_AGENT_ACCESS_RESPONSE_MAGIC);
    assert_eq!(read_u16(&wire[4..6]), REMOTE_AGENT_ACCESS_V2_VERSION);
    assert_eq!(read_u16(&wire[6..8]), RemoteAgentAccessKindV2::ApplyRemoteAccess as u16);
    assert_eq!(&wire[8..10], &[0, 0]);
    assert_eq!(read_u16(&wire[10..12]) as usize, carrier_length);
    assert_eq!(read_u32(&wire[12..16]) as usize, payload_length);
    assert_eq!(&wire[16..24], &[0; 8]);
    assert_eq!(&wire[24..40], request.request_id().as_bytes());
    assert_eq!(&wire[40..72], request.request_digest().as_bytes());
    assert_eq!(&wire[72..104], carrier.binding_digest().as_bytes());
    assert_eq!(&wire[104..120], request.target().as_bytes());
    assert_eq!(&wire[120..152], &request.expected_runtime_store_instance_id());
    assert_eq!(read_u64(&wire[152..160]), request.expected_runtime_host_epoch());
    assert_eq!(&wire[160..360], request.retained_s0_cas().canonical_wire());
    assert_eq!(&wire[360..512], request.expected_s1_cas().canonical_wire());
    assert_eq!(&wire[512..544], response.payload_wire_digest().as_bytes());
    assert_eq!(&wire[544..576], &[0; 32]);
    assert_eq!(&wire[576..592], carrier.runtime_principal().as_bytes());
    assert_eq!(&wire[592..608], carrier.runtime_response_key().as_bytes());
    assert_eq!(read_u16(&wire[608..610]), 1);
    assert_eq!(read_u16(&wire[610..612]), 1);
    assert_eq!(&wire[612..644], carrier.binding_digest().as_bytes());
    assert_eq!(read_u16(&wire[644..646]), 64);
    assert_eq!(&wire[646..678], OUTER_CONTROLLER_NONCE);
    assert_eq!(&wire[678..678 + carrier_length], carrier.canonical_wire());
    assert_eq!(
        &wire[678 + carrier_length..678 + carrier_length + payload_length],
        terminal.canonical_wire(),
    );
    assert_eq!(&wire[wire.len() - 64..], OUTER_RUNTIME_SIGNATURE);
    assert_eq!(RemoteAgentAccessResponseV2::decode(wire).unwrap(), response);
    assert_eq!(response.apply_receipt(), Some(&terminal));

    let inner_request_transcript = inner.signing_transcript().unwrap();
    let outer_request_transcript = request.signing_transcript().unwrap();
    let inner_response_transcript = terminal.signing_transcript().unwrap();
    let outer_response_transcript = response.signing_transcript().unwrap();
    let order = RefCell::new(Vec::new());
    request
        .verify_controller_apply_request(
            &carrier,
            |principal, key, algorithm, version, transcript, signature| {
                order.borrow_mut().push("inner-controller");
                principal == inner.authentication().claim().principal()
                    && key == inner.authentication().claim().key()
                    && algorithm == inner.authentication().claim().algorithm()
                    && version == inner.authentication().claim().algorithm_version()
                    && transcript == inner_request_transcript.as_bytes()
                    && signature == inner.authentication().signature()
            },
            |principal, key, fingerprint, transcript, signature| {
                order.borrow_mut().push("outer-controller");
                principal == carrier.controller_principal()
                    && key == carrier.controller_request_key()
                    && fingerprint == carrier.controller_request_key_fingerprint()
                    && transcript == outer_request_transcript.as_bytes()
                    && signature == OUTER_CONTROLLER_SIGNATURE
            },
        )
        .expect("both Controller signatures");
    response
        .verify_runtime_apply_response(
            &request,
            &carrier,
            terminal.authentication(),
            |principal, key, algorithm, version, transcript, signature| {
                order.borrow_mut().push("inner-runtime");
                principal == terminal.authentication().runtime_principal()
                    && key == terminal.authentication().key()
                    && algorithm == terminal.authentication().algorithm()
                    && version == terminal.authentication().algorithm_version()
                    && transcript == inner_response_transcript.as_bytes()
                    && signature == terminal.authentication_signature()
            },
            |principal, key, fingerprint, transcript, signature| {
                order.borrow_mut().push("outer-runtime");
                principal == carrier.runtime_principal()
                    && key == carrier.runtime_response_key()
                    && fingerprint == carrier.runtime_response_key_fingerprint()
                    && transcript == outer_response_transcript.as_bytes()
                    && signature == OUTER_RUNTIME_SIGNATURE
            },
        )
        .expect("both Runtime signatures");
    assert_eq!(
        order.into_inner(),
        [
            "inner-controller",
            "outer-controller",
            "inner-runtime",
            "outer-runtime",
        ],
    );
}
