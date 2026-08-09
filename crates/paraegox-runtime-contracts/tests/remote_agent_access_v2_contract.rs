use std::cell::{Cell, RefCell};

use paraegox_kernel::digest::Digest32;

use paraegox_runtime_contracts::distributed_agent_stack_plan::{
    RestrictedRuntimeApplyCarrierBindingFieldsV1, RestrictedRuntimeApplyCarrierBindingV1,
};
use paraegox_runtime_contracts::remote_agent_access::{
    MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES, MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES,
    REMOTE_AGENT_ACCESS_REQUEST_MAGIC, REMOTE_AGENT_ACCESS_RESPONSE_MAGIC,
    REMOTE_AGENT_ACCESS_V2_VERSION, RemoteAgentAccessKindV2, RemoteAgentAccessRequestDraftV2,
    RemoteAgentAccessRequestFieldsV2, RemoteAgentAccessRequestIdV2, RemoteAgentAccessRequestV1,
    RemoteAgentAccessRequestV2, RemoteAgentAccessResponseAuthClaimV2,
    RemoteAgentAccessResponseDraftV2, RemoteAgentAccessResponseV1, RemoteAgentAccessResponseV2,
};
use paraegox_runtime_contracts::remote_agent_data_plane_plan::{
    REMOTE_AGENT_ACTIVE_S1_CAS_V2_BYTES, REMOTE_AGENT_RETAINED_S0_CAS_V2_BYTES,
    RemoteAgentActiveS1CasV2, RemoteAgentDataPlaneApplyRequestDraftV2,
    RemoteAgentDataPlaneApplyRequestV2, RemoteAgentDataPlaneTerminalReceiptV2,
};
use paraegox_runtime_contracts::wire::{ApplyAuthAlgorithm, ApplyRequestAuthClaim};

const PROXY_DATA_PLANE_V2_GOLDEN: &str =
    include_str!("../../../tests/fixtures/wire/t2_remote_agent_proxy_data_plane_v2.json");
const ACCESS_V1_GOLDEN: &str =
    include_str!("../../../tests/fixtures/wire/t2_remote_agent_access_v1.json");
const ACCESS_V2_GOLDEN: &str =
    include_str!("../../../tests/fixtures/wire/t2_remote_agent_access_v2.json");
const OUTER_SOURCE: &str = include_str!("../src/remote_agent_access.rs");
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

fn fixture_digest_after(fixture: &str, key: &str) -> Digest32 {
    Digest32::from_bytes(
        fixture_hex_after(fixture, key)
            .try_into()
            .expect("32-byte fixture digest"),
    )
}

fn fixture_u64_after(fixture: &str, key: &str) -> u64 {
    let key_start = fixture
        .find(key)
        .map(|offset| offset + key.len())
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
    let local_only = fixture_section_after(PROXY_DATA_PLANE_V2_GOLDEN, "\"local_only_ready\"");
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
    RestrictedRuntimeApplyCarrierBindingV1::try_new(RestrictedRuntimeApplyCarrierBindingFieldsV1 {
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
    })
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
    apply_access_request_with_outer_signature(inner, terminal, carrier, OUTER_CONTROLLER_SIGNATURE)
}

fn apply_access_request_with_outer_signature(
    inner: &RemoteAgentDataPlaneApplyRequestV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    carrier: RestrictedRuntimeApplyCarrierBindingV1,
    signature: &[u8],
) -> RemoteAgentAccessRequestV2 {
    RemoteAgentAccessRequestDraftV2::try_apply_remote_access(
        access_fields(inner, terminal, carrier, OUTER_CONTROLLER_NONCE),
        inner.clone(),
    )
    .expect("PXRA v2 Apply draft")
    .finalize(signature)
    .expect("PXRA v2 Apply")
}

fn reissue_inner_request(
    request: &RemoteAgentDataPlaneApplyRequestV2,
    signature: &[u8],
) -> RemoteAgentDataPlaneApplyRequestV2 {
    RemoteAgentDataPlaneApplyRequestDraftV2::try_new(
        request.target_execution().clone(),
        request.provenance(),
        request.control_commitment().control().clone(),
        request.temporal(),
        request.expected_runtime_store_instance_id(),
        request.authentication().claim().clone(),
    )
    .expect("reissued PXAR v11 draft")
    .finalize(signature)
    .expect("reissued PXAR v11")
}

fn reissue_inner_terminal(
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    signature: &[u8; 64],
) -> RemoteAgentDataPlaneTerminalReceiptV2 {
    assert_eq!(terminal.authentication_signature().len(), signature.len());
    let mut wire = terminal.canonical_wire().to_vec();
    let signature_start = wire.len() - signature.len();
    wire[signature_start..].copy_from_slice(signature);
    RemoteAgentDataPlaneTerminalReceiptV2::decode(&wire).expect("reissued PXAU v2")
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
    apply_access_response_with_outer_signature(
        request,
        inner,
        terminal,
        carrier,
        OUTER_RUNTIME_SIGNATURE,
    )
}

fn apply_access_response_with_outer_signature(
    request: &RemoteAgentAccessRequestV2,
    inner: &RemoteAgentDataPlaneApplyRequestV2,
    terminal: &RemoteAgentDataPlaneTerminalReceiptV2,
    carrier: &RestrictedRuntimeApplyCarrierBindingV1,
    signature: &[u8],
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
    .finalize(signature)
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

    assert_eq!(
        wire.len(),
        544 + nonce_length + carrier_length + payload_length + 64
    );
    assert_eq!(&wire[0..4], REMOTE_AGENT_ACCESS_REQUEST_MAGIC);
    assert_eq!(read_u16(&wire[4..6]), REMOTE_AGENT_ACCESS_V2_VERSION);
    assert_eq!(
        read_u16(&wire[6..8]),
        RemoteAgentAccessKindV2::ApplyRemoteAccess as u16
    );
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
    assert_eq!(
        &describe_wire[320..472],
        current_active_s1_cas().canonical_wire()
    );
    assert_eq!(&describe_wire[472..504], &[0; 32]);
    assert!(describe.apply_request().is_none());
    assert_eq!(
        RemoteAgentAccessRequestV2::decode(describe_wire).unwrap(),
        describe
    );
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

    assert_eq!(
        wire.len(),
        646 + nonce_length + carrier_length + payload_length + 64
    );
    assert_eq!(&wire[0..4], REMOTE_AGENT_ACCESS_RESPONSE_MAGIC);
    assert_eq!(read_u16(&wire[4..6]), REMOTE_AGENT_ACCESS_V2_VERSION);
    assert_eq!(
        read_u16(&wire[6..8]),
        RemoteAgentAccessKindV2::ApplyRemoteAccess as u16
    );
    assert_eq!(&wire[8..10], &[0, 0]);
    assert_eq!(read_u16(&wire[10..12]) as usize, carrier_length);
    assert_eq!(read_u32(&wire[12..16]) as usize, payload_length);
    assert_eq!(read_u16(&wire[16..18]), 0);
    assert_eq!(read_u32(&wire[18..22]), 0);
    assert_eq!(read_u16(&wire[22..24]) as usize, nonce_length);
    assert_eq!(&wire[24..40], request.request_id().as_bytes());
    assert_eq!(&wire[40..72], request.request_digest().as_bytes());
    assert_eq!(&wire[72..104], carrier.binding_digest().as_bytes());
    assert_eq!(&wire[104..120], request.target().as_bytes());
    assert_eq!(
        &wire[120..152],
        &request.expected_runtime_store_instance_id()
    );
    assert_eq!(
        read_u64(&wire[152..160]),
        request.expected_runtime_host_epoch()
    );
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

#[test]
fn all_four_signatures_are_independent_and_wrong_or_zero_bytes_fail_closed() {
    let inner = active_inner_request();
    let terminal = active_inner_terminal();
    let carrier = carrier_for(&inner, &terminal);
    let expected_inner_controller_signature = inner.authentication().signature().to_vec();
    let expected_inner_runtime_signature = terminal.authentication_signature().to_vec();

    for bad_signature in [[0; 64], [0x7f; 64]] {
        let bad_inner = reissue_inner_request(&inner, &bad_signature);
        let request = apply_access_request(&bad_inner, &terminal, carrier.clone());
        let inner_calls = Cell::new(0);
        let outer_calls = Cell::new(0);
        assert!(
            request
                .verify_controller_apply_request(
                    &carrier,
                    |_, _, _, _, _, signature| {
                        inner_calls.set(inner_calls.get() + 1);
                        signature == expected_inner_controller_signature.as_slice()
                    },
                    |_, _, _, _, _| {
                        outer_calls.set(outer_calls.get() + 1);
                        true
                    },
                )
                .is_err(),
            "wrong inner Controller signature must fail",
        );
        assert_eq!(inner_calls.get(), 1);
        assert_eq!(outer_calls.get(), 0);

        let request = apply_access_request_with_outer_signature(
            &inner,
            &terminal,
            carrier.clone(),
            &bad_signature,
        );
        let inner_calls = Cell::new(0);
        let outer_calls = Cell::new(0);
        assert!(
            request
                .verify_controller_apply_request(
                    &carrier,
                    |_, _, _, _, _, signature| {
                        inner_calls.set(inner_calls.get() + 1);
                        signature == expected_inner_controller_signature.as_slice()
                    },
                    |_, _, _, _, signature| {
                        outer_calls.set(outer_calls.get() + 1);
                        signature == OUTER_CONTROLLER_SIGNATURE
                    },
                )
                .is_err(),
            "wrong outer Controller signature must fail",
        );
        assert_eq!(inner_calls.get(), 1);
        assert_eq!(outer_calls.get(), 1);

        let request = apply_access_request(&inner, &terminal, carrier.clone());
        let bad_terminal = reissue_inner_terminal(&terminal, &bad_signature);
        let response = apply_access_response(&request, &inner, &bad_terminal, &carrier);
        let inner_calls = Cell::new(0);
        let outer_calls = Cell::new(0);
        assert!(
            response
                .verify_runtime_apply_response(
                    &request,
                    &carrier,
                    terminal.authentication(),
                    |_, _, _, _, _, signature| {
                        inner_calls.set(inner_calls.get() + 1);
                        signature == expected_inner_runtime_signature.as_slice()
                    },
                    |_, _, _, _, _| {
                        outer_calls.set(outer_calls.get() + 1);
                        true
                    },
                )
                .is_err(),
            "wrong inner Runtime signature must fail",
        );
        assert_eq!(inner_calls.get(), 1);
        assert_eq!(outer_calls.get(), 0);

        let response = apply_access_response_with_outer_signature(
            &request,
            &inner,
            &terminal,
            &carrier,
            &bad_signature,
        );
        let inner_calls = Cell::new(0);
        let outer_calls = Cell::new(0);
        assert!(
            response
                .verify_runtime_apply_response(
                    &request,
                    &carrier,
                    terminal.authentication(),
                    |_, _, _, _, _, signature| {
                        inner_calls.set(inner_calls.get() + 1);
                        signature == expected_inner_runtime_signature.as_slice()
                    },
                    |_, _, _, _, signature| {
                        outer_calls.set(outer_calls.get() + 1);
                        signature == OUTER_RUNTIME_SIGNATURE
                    },
                )
                .is_err(),
            "wrong outer Runtime signature must fail",
        );
        assert_eq!(inner_calls.get(), 1);
        assert_eq!(outer_calls.get(), 1);
    }
}

#[test]
fn pxra2_strict_wire_rejects_length_reserved_identity_cas_auth_and_payload_tamper() {
    let inner = active_inner_request();
    let terminal = active_inner_terminal();
    let carrier = carrier_for(&inner, &terminal);
    let request = apply_access_request(&inner, &terminal, carrier.clone());
    let wire = request.canonical_wire();
    let carrier_start = 576;
    let payload_start = carrier_start + carrier.canonical_wire().len();

    for (name, offset) in [
        ("magic", 0),
        ("version", 5),
        ("reserved", 8),
        ("carrier length", 11),
        ("payload length", 15),
        ("request id", 16),
        ("PXCB digest", 32),
        ("target", 64),
        ("store", 80),
        ("retained-S0 CAS", 120),
        ("active-S1 CAS", 320),
        ("payload digest", 472),
        ("Controller principal", 504),
        ("Controller key", 520),
        ("algorithm", 537),
        ("algorithm version", 539),
        ("signature length", 575),
        ("PXCB bytes", carrier_start),
        ("PXAR11 payload", payload_start),
    ] {
        let mut tampered = wire.to_vec();
        tampered[offset] ^= 1;
        assert!(
            RemoteAgentAccessRequestV2::decode(&tampered).is_err(),
            "PXRA v2 accepted tampered {name} at offset {offset}",
        );
    }

    assert!(RemoteAgentAccessRequestV2::decode(&wire[..wire.len() - 1]).is_err());
    let mut trailing = wire.to_vec();
    trailing.push(0);
    assert!(RemoteAgentAccessRequestV2::decode(&trailing).is_err());
    let oversized = vec![0; MAX_REMOTE_AGENT_ACCESS_REQUEST_V2_BYTES + 1];
    assert!(RemoteAgentAccessRequestV2::decode(&oversized).is_err());

    let expected_transcript = request.signing_transcript().unwrap().as_bytes().to_vec();
    for (name, offset) in [
        ("RuntimeHost epoch", 119),
        ("Controller nonce", 542),
        ("outer signature", wire.len() - 1),
    ] {
        let mut tampered = wire.to_vec();
        tampered[offset] ^= 1;
        let decoded = RemoteAgentAccessRequestV2::decode(&tampered)
            .unwrap_or_else(|error| panic!("structural {name} tamper: {error}"));
        assert!(
            decoded
                .verify_controller_apply_request(
                    &carrier,
                    |_, _, _, _, _, _| true,
                    |_, _, _, transcript, signature| {
                        transcript == expected_transcript.as_slice()
                            && signature == OUTER_CONTROLLER_SIGNATURE
                    },
                )
                .is_err(),
            "PXRA v2 authenticated tampered {name}",
        );
    }

    let short_draft = RemoteAgentAccessRequestDraftV2::try_apply_remote_access(
        access_fields(&inner, &terminal, carrier.clone(), OUTER_CONTROLLER_NONCE),
        inner.clone(),
    )
    .unwrap();
    assert!(short_draft.clone().finalize(&[0; 63]).is_err());
    assert!(short_draft.finalize(&[0; 65]).is_err());
}

#[test]
fn pxrr2_strict_wire_rejects_length_reserved_auth_payload_and_correlation_tamper() {
    let inner = active_inner_request();
    let terminal = active_inner_terminal();
    let carrier = carrier_for(&inner, &terminal);
    let request = apply_access_request(&inner, &terminal, carrier.clone());
    let response = apply_access_response(&request, &inner, &terminal, &carrier);
    let wire = response.canonical_wire();
    let carrier_start = 646 + OUTER_CONTROLLER_NONCE.len();
    let payload_start = carrier_start + carrier.canonical_wire().len();

    for (name, offset) in [
        ("magic", 0),
        ("version", 5),
        ("reserved", 8),
        ("carrier length", 11),
        ("payload length", 15),
        ("profile length", 17),
        ("descriptor length", 21),
        ("nonce length", 23),
        ("PXCB digest", 72),
        ("target", 104),
        ("store", 120),
        ("RuntimeHost epoch", 159),
        ("payload digest", 512),
        ("Apply descriptor digest", 544),
        ("Runtime principal", 576),
        ("Runtime key", 592),
        ("algorithm", 609),
        ("algorithm version", 611),
        ("auth PXCB digest", 612),
        ("signature length", 645),
        ("PXCB bytes", carrier_start),
        ("PXAU2 payload", payload_start),
    ] {
        let mut tampered = wire.to_vec();
        tampered[offset] ^= 1;
        assert!(
            RemoteAgentAccessResponseV2::decode(&tampered).is_err(),
            "PXRR v2 accepted tampered {name} at offset {offset}",
        );
    }

    assert!(RemoteAgentAccessResponseV2::decode(&wire[..wire.len() - 1]).is_err());
    let mut trailing = wire.to_vec();
    trailing.push(0);
    assert!(RemoteAgentAccessResponseV2::decode(&trailing).is_err());
    let oversized = vec![0; MAX_REMOTE_AGENT_ACCESS_RESPONSE_V2_BYTES + 1];
    assert!(RemoteAgentAccessResponseV2::decode(&oversized).is_err());

    for (name, offset) in [
        ("request id", 24),
        ("request digest", 40),
        ("retained-S0 CAS", 160),
        ("active-S1 CAS", 375),
        ("request nonce", 646),
    ] {
        let mut tampered = wire.to_vec();
        tampered[offset] ^= 1;
        let decoded = RemoteAgentAccessResponseV2::decode(&tampered)
            .unwrap_or_else(|error| panic!("structural {name} tamper: {error}"));
        assert!(
            decoded.validate_against_request(&request).is_err(),
            "PXRR v2 correlated tampered {name}",
        );
    }

    let mut signature_tamper = wire.to_vec();
    *signature_tamper.last_mut().unwrap() ^= 1;
    let signature_tamper = RemoteAgentAccessResponseV2::decode(&signature_tamper)
        .expect("opaque outer signature tamper remains structurally decodable");
    assert!(
        signature_tamper
            .verify_runtime_apply_response(
                &request,
                &carrier,
                terminal.authentication(),
                |_, _, _, _, _, signature| signature == terminal.authentication_signature(),
                |_, _, _, _, signature| signature == OUTER_RUNTIME_SIGNATURE,
            )
            .is_err()
    );

    let authenticated_request = request
        .verify_controller_apply_request(&carrier, |_, _, _, _, _, _| true, |_, _, _, _, _| true)
        .unwrap();
    let authenticated_terminal = terminal
        .verify_runtime_terminal(&inner, terminal.authentication(), |_, _, _, _, _, _| true)
        .unwrap();
    let short_draft = RemoteAgentAccessResponseDraftV2::try_apply_remote_access(
        authenticated_request,
        authenticated_terminal,
        response_auth(&carrier),
    )
    .unwrap();
    assert!(short_draft.clone().finalize(&[0; 63]).is_err());
    assert!(short_draft.finalize(&[0; 65]).is_err());
}

#[test]
fn pxra2_pxrr2_cross_reject_v1_inner_and_other_control_protocols() {
    let inner = active_inner_request();
    let terminal = active_inner_terminal();
    let carrier = carrier_for(&inner, &terminal);
    let request = apply_access_request(&inner, &terminal, carrier.clone());
    let response = apply_access_response(&request, &inner, &terminal, &carrier);

    let access_v1 = fixture_section_after(ACCESS_V1_GOLDEN, "\"access\"");
    let v1_request_wire = fixture_hex_after(access_v1, "\"pxra_apply_hex\"");
    let v1_response_wire = fixture_hex_after(access_v1, "\"pxrr_apply_hex\"");
    assert!(RemoteAgentAccessRequestV1::decode(&v1_request_wire).is_ok());
    assert!(RemoteAgentAccessResponseV1::decode(&v1_response_wire).is_ok());
    assert!(RemoteAgentAccessRequestV2::decode(&v1_request_wire).is_err());
    assert!(RemoteAgentAccessResponseV2::decode(&v1_response_wire).is_err());
    assert!(RemoteAgentAccessRequestV1::decode(request.canonical_wire()).is_err());
    assert!(RemoteAgentAccessResponseV1::decode(response.canonical_wire()).is_err());

    assert!(RemoteAgentAccessRequestV2::decode(inner.canonical_wire()).is_err());
    assert!(RemoteAgentAccessResponseV2::decode(terminal.canonical_wire()).is_err());
    for magic in [b"PXAG", b"PXAH", b"PXCC", b"PXDR"] {
        let mut cross_request = request.canonical_wire().to_vec();
        cross_request[..4].copy_from_slice(magic);
        assert!(RemoteAgentAccessRequestV2::decode(&cross_request).is_err());

        let mut cross_response = response.canonical_wire().to_vec();
        cross_response[..4].copy_from_slice(magic);
        assert!(RemoteAgentAccessResponseV2::decode(&cross_response).is_err());
    }
}

#[test]
fn describe_response_signing_has_no_public_historical_pair_producer() {
    let response_impl = OUTER_SOURCE
        .split_once("impl RemoteAgentAccessResponseDraftV2")
        .expect("PXRR v2 draft implementation")
        .1
        .split_once("/// Strict independently Runtime-signed PXRR v2 response.")
        .expect("PXRR v2 response boundary")
        .0;
    assert!(response_impl.contains("pub fn try_apply_remote_access"));
    assert!(response_impl.contains("fn try_new"));
    assert!(!response_impl.contains("pub fn try_new"));
    assert!(!response_impl.contains("pub fn try_describe_remote_access"));
    assert!(OUTER_SOURCE.contains("current-final, non-Clone authority marker"));

    assert!(OUTER_SOURCE.contains("enum RemoteAgentAccessResponsePayloadV2"));
    assert!(!OUTER_SOURCE.contains("pub enum RemoteAgentAccessResponsePayloadV2"));
}

#[test]
fn independent_python_golden_locks_apply_describe_and_historical_consumer_wires() {
    assert!(ACCESS_V2_GOLDEN.contains("\"format\": \"paraegox-t2-remote-agent-access-v2\""));
    assert!(ACCESS_V2_GOLDEN.contains(
        "\"source\": \"independent Python struct/hashlib/cryptography outer-v2 oracle\""
    ));
    assert!(ACCESS_V2_GOLDEN.contains(
        "\"remote_agent_access.rs_sha256\": \"ee42b276ad30d9fa2c9240af49315db8fd6499a4c45a8a2c9107cdab44a58be7\""
    ));
    assert!(ACCESS_V2_GOLDEN.contains(
        "\"remote_agent_data_plane_plan.rs_sha256\": \"8597f75ec6bb6b41a97fb3ebd2d8cdefcde431ceb0d32d6b125ed17f0a1a64e7\""
    ));
    assert!(ACCESS_V2_GOLDEN.contains(
        "\"corrected_inner_fixture_sha256\": \"983e4449636dd559e9ca0508b756f186ecc34a47ff8a2c5f6c38f3941a31499a\""
    ));

    let carrier_scope = fixture_section_after(ACCESS_V2_GOLDEN, "\"carrier\"");
    let carrier_wire = fixture_hex_after(carrier_scope, "\"wire_hex\"");
    let carrier = RestrictedRuntimeApplyCarrierBindingV1::decode(&carrier_wire)
        .expect("independent-golden PXCB");
    assert_eq!(carrier.canonical_wire(), carrier_wire);
    assert_eq!(carrier.route(), "paraegox/runtime/t2/remote-agent-access/v2/apply");
    assert_eq!(
        carrier.binding_digest(),
        fixture_digest_after(carrier_scope, "\"digest_hex\"")
    );
    assert_eq!(
        carrier_wire.len() as u64,
        fixture_u64_after(carrier_scope, "\"wire_length\"")
    );

    let inner_inputs = fixture_section_after(ACCESS_V2_GOLDEN, "\"inner_signature_inputs\"");
    let expected_inner_controller_transcript =
        fixture_hex_after(inner_inputs, "\"controller_transcript_hex\"");
    let expected_inner_controller_signature =
        fixture_hex_after(inner_inputs, "\"controller_signature_hex\"");
    let expected_inner_runtime_transcript =
        fixture_hex_after(inner_inputs, "\"runtime_transcript_hex\"");
    let expected_inner_runtime_signature =
        fixture_hex_after(inner_inputs, "\"runtime_signature_hex\"");

    let apply_scope = fixture_section_after(ACCESS_V2_GOLDEN, "\"apply\"");
    let apply_request_scope = fixture_section_after(apply_scope, "\"pxra_v2\"");
    let apply_request_wire = fixture_hex_after(apply_request_scope, "\"wire_hex\"");
    let apply_request_transcript =
        fixture_hex_after(apply_request_scope, "\"signing_transcript_hex\"");
    let apply_request_signature = fixture_hex_after(apply_request_scope, "\"signature_hex\"");
    let apply_request = RemoteAgentAccessRequestV2::decode(&apply_request_wire)
        .expect("independent-golden PXRA v2 Apply");
    assert_eq!(apply_request.canonical_wire(), apply_request_wire);
    assert_eq!(
        apply_request_wire.len() as u64,
        fixture_u64_after(apply_request_scope, "\"wire_length\"")
    );
    assert_eq!(
        apply_request.request_digest(),
        fixture_digest_after(apply_request_scope, "\"digest_hex\"")
    );
    assert_eq!(
        apply_request.payload_wire_digest(),
        fixture_digest_after(apply_request_scope, "\"payload_digest_hex\"")
    );
    assert_eq!(
        apply_request.signing_transcript().unwrap().as_bytes(),
        apply_request_transcript
    );
    assert_eq!(
        apply_request.authentication().signature(),
        apply_request_signature
    );
    let embedded_inner = apply_request.apply_request().expect("embedded PXAR v11");
    assert_eq!(embedded_inner, &active_inner_request());
    apply_request
        .verify_controller_apply_request(
            &carrier,
            |principal, key, algorithm, version, transcript, signature| {
                let claim = embedded_inner.authentication().claim();
                principal == claim.principal()
                    && key == claim.key()
                    && algorithm == claim.algorithm()
                    && version == claim.algorithm_version()
                    && transcript == expected_inner_controller_transcript.as_slice()
                    && signature == expected_inner_controller_signature.as_slice()
            },
            |principal, key, fingerprint, transcript, signature| {
                principal == carrier.controller_principal()
                    && key == carrier.controller_request_key()
                    && fingerprint == carrier.controller_request_key_fingerprint()
                    && transcript == apply_request_transcript.as_slice()
                    && signature == apply_request_signature.as_slice()
            },
        )
        .expect("independent-golden Controller signatures");

    let apply_response_scope = fixture_section_after(apply_scope, "\"pxrr_v2\"");
    let apply_response_wire = fixture_hex_after(apply_response_scope, "\"wire_hex\"");
    let apply_response_transcript =
        fixture_hex_after(apply_response_scope, "\"signing_transcript_hex\"");
    let apply_response_signature = fixture_hex_after(apply_response_scope, "\"signature_hex\"");
    let apply_response = RemoteAgentAccessResponseV2::decode(&apply_response_wire)
        .expect("independent-golden PXRR v2 Apply");
    assert_eq!(apply_response.canonical_wire(), apply_response_wire);
    assert_eq!(
        apply_response_wire.len() as u64,
        fixture_u64_after(apply_response_scope, "\"wire_length\"")
    );
    assert_eq!(
        apply_response.response_digest(),
        fixture_digest_after(apply_response_scope, "\"digest_hex\"")
    );
    assert_eq!(
        apply_response.payload_wire_digest(),
        fixture_digest_after(apply_response_scope, "\"payload_digest_hex\"")
    );
    assert_eq!(
        apply_response.signing_transcript().unwrap().as_bytes(),
        apply_response_transcript
    );
    assert_eq!(
        apply_response.authentication_signature(),
        apply_response_signature
    );
    let embedded_terminal = apply_response.apply_receipt().expect("embedded PXAU v2");
    assert_eq!(embedded_terminal, &active_inner_terminal());
    apply_response
        .verify_runtime_apply_response(
            &apply_request,
            &carrier,
            embedded_terminal.authentication(),
            |principal, key, algorithm, version, transcript, signature| {
                let claim = embedded_terminal.authentication();
                principal == claim.runtime_principal()
                    && key == claim.key()
                    && algorithm == claim.algorithm()
                    && version == claim.algorithm_version()
                    && transcript == expected_inner_runtime_transcript.as_slice()
                    && signature == expected_inner_runtime_signature.as_slice()
            },
            |principal, key, fingerprint, transcript, signature| {
                principal == carrier.runtime_principal()
                    && key == carrier.runtime_response_key()
                    && fingerprint == carrier.runtime_response_key_fingerprint()
                    && transcript == apply_response_transcript.as_slice()
                    && signature == apply_response_signature.as_slice()
            },
        )
        .expect("independent-golden Runtime signatures");

    let describe_scope = fixture_section_after(ACCESS_V2_GOLDEN, "\"describe\"");
    let describe_request_scope = fixture_section_after(describe_scope, "\"pxra_v2\"");
    let describe_request_wire = fixture_hex_after(describe_request_scope, "\"wire_hex\"");
    let describe_request_transcript =
        fixture_hex_after(describe_request_scope, "\"signing_transcript_hex\"");
    let describe_request_signature = fixture_hex_after(describe_request_scope, "\"signature_hex\"");
    let describe_request = RemoteAgentAccessRequestV2::decode(&describe_request_wire)
        .expect("independent-golden PXRA v2 Describe");
    assert_eq!(describe_request.canonical_wire(), describe_request_wire);
    assert_eq!(
        describe_request.request_digest(),
        fixture_digest_after(describe_request_scope, "\"digest_hex\"")
    );
    assert_eq!(
        describe_request.payload_wire_digest(),
        fixture_digest_after(describe_request_scope, "\"payload_digest_hex\"")
    );
    assert_eq!(
        describe_request.signing_transcript().unwrap().as_bytes(),
        describe_request_transcript
    );
    assert_eq!(
        describe_request.authentication().signature(),
        describe_request_signature
    );
    describe_request
        .verify_controller_describe_request(
            &carrier,
            |principal, key, fingerprint, transcript, signature| {
                principal == carrier.controller_principal()
                    && key == carrier.controller_request_key()
                    && fingerprint == carrier.controller_request_key_fingerprint()
                    && transcript == describe_request_transcript.as_slice()
                    && signature == describe_request_signature.as_slice()
            },
        )
        .expect("independent-golden Describe Controller signature");

    let historical_scope = fixture_section_after(describe_scope, "\"pxrr_v2_strict_consumer\"");
    assert!(historical_scope.contains("\"classification\": \"synthetic/historical-negative\""));
    assert!(historical_scope.contains("\"currentness_evidence\": false"));
    assert!(historical_scope.contains("\"producer_evidence\": false"));
    let historical_wire = fixture_hex_after(historical_scope, "\"wire_hex\"");
    let historical_transcript = fixture_hex_after(historical_scope, "\"signing_transcript_hex\"");
    let historical_signature = fixture_hex_after(historical_scope, "\"signature_hex\"");
    let historical = RemoteAgentAccessResponseV2::decode(&historical_wire)
        .expect("synthetic historical PXRR v2 Describe consumer fixture");
    assert_eq!(historical.canonical_wire(), historical_wire);
    assert_eq!(
        historical.response_digest(),
        fixture_digest_after(historical_scope, "\"digest_hex\"")
    );
    assert_eq!(
        historical.payload_wire_digest(),
        fixture_digest_after(historical_scope, "\"payload_digest_hex\"")
    );
    assert_eq!(
        historical.signing_transcript().unwrap().as_bytes(),
        historical_transcript
    );
    assert_eq!(historical.authentication_signature(), historical_signature);
    assert!(historical.profile().is_some());
    assert!(historical.descriptor().is_some());
    historical
        .verify_runtime_describe_response(
            &describe_request,
            &carrier,
            |principal, key, fingerprint, transcript, signature| {
                principal == carrier.runtime_principal()
                    && key == carrier.runtime_response_key()
                    && fingerprint == carrier.runtime_response_key_fingerprint()
                    && transcript == historical_transcript.as_slice()
                    && signature == historical_signature.as_slice()
            },
        )
        .expect("strict consumer validation is not producer/currentness evidence");
}
