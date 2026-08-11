from __future__ import annotations

import hashlib
import importlib.util
import json
import struct
from pathlib import Path
from types import ModuleType
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
WIRE = ROOT / "tests" / "fixtures" / "wire"
LEDGER = json.loads((WIRE / "artifact_f0_semantic_ledger_v1.json").read_text())
AGENT_ORACLE_PATH = ROOT / "tests" / "contract" / "test_s7_managed_agent_stack_successor.py"

PXTA_ZERO = bytes.fromhex("50585441000100000000")
MODEL_PROJECTION_BYTES = 270
MODEL_PXTE_FIXED_BYTES = 286
MAX_MODEL_PXTE_BYTES = 1_994
MODEL_COMPATIBILITY_DOMAIN = (
    b"paraegox.runtime.compiled-managed-model-agent-stack-compatibility.sha256.v1"
)
MODEL_EXECUTION_DIGEST_DOMAIN = b"paraegox.runtime.target-execution.sha256.v8"
MODEL_ASSIGNMENT_DIGEST_DOMAIN = b"paraegox.runtime.target-plan-assignments.sha256.v9"
ARTIFACT_COMPATIBILITY_DOMAIN = (
    b"paraegox.runtime.compiled-artifact-bound-managed-model-agent-stack-compatibility.sha256.v1"
)
ARTIFACT_EXECUTION_DIGEST_DOMAIN = b"paraegox.runtime.target-execution.sha256.v11"
ARTIFACT_ASSIGNMENT_DIGEST_DOMAIN = b"paraegox.runtime.target-plan-assignments.sha256.v12"
ARTIFACT_BINDING_DIGEST_DOMAIN = b"paraegox.runtime.artifact-execution-binding.sha256.v1"


def _load_agent_oracle() -> ModuleType:
    spec = importlib.util.spec_from_file_location(
        "_paraegox_artifact_f0_agent_oracle", AGENT_ORACLE_PATH
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


AGENT = _load_agent_oracle()


def _fixture(name: str, size: int) -> bytes:
    text = (WIRE / name).read_text()
    assert text.endswith("\n")
    assert text.count("\n") == 1
    assert text[:-1] == text[:-1].lower()
    frame = bytes.fromhex(text)
    assert len(frame) == size
    return frame


def _raw_digest(domain: bytes, body: bytes) -> bytes:
    return hashlib.sha256(domain + body).digest()


def _canonical_digest(domain: bytes, *fields: bytes) -> bytes:
    digest = hashlib.sha256()
    digest.update(b"ParaEGOX\0canonical-digest")
    digest.update(struct.pack(">H", 1))
    digest.update(struct.pack(">I", len(domain)))
    digest.update(domain)
    for ordinal, field in enumerate(fields, 1):
        digest.update(b"\x01")
        digest.update(struct.pack(">I", ordinal))
        digest.update(struct.pack(">Q", len(field)))
        digest.update(field)
    digest.update(b"\xff")
    digest.update(struct.pack(">I", len(fields)))
    return digest.digest()


def _execution_profile_commitment() -> bytes:
    fields = (
        (b"developer-local-echo-prefix-v1", 30),
        (b"managed_model_data_v1", 21),
        (b"bounded-text-model-data-v1", 26),
        (b"developer-local-managed-model-v1", 32),
        (b"literal-prefix-v1", 17),
    )
    body = b"".join(struct.pack(">H", length) + value for value, length in fields)
    body += struct.pack(">III", 16_384, 32_768, 0)
    return _raw_digest(b"paraegox.artifact.execution-profile.sha256.v1", body)


def _model_compatibility_digest() -> bytes:
    return _canonical_digest(
        MODEL_COMPATIBILITY_DOMAIN,
        AGENT._compatibility_digest(),
        b"PXMM",
        struct.pack(">H", 1),
        struct.pack(">H", MODEL_PROJECTION_BYTES),
        b"PXAR",
        struct.pack(">H", 9),
        b"PXTE",
        struct.pack(">H", 8),
        struct.pack(">I", MAX_MODEL_PXTE_BYTES),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 256),
        b"px-bounded-text1",
        MODEL_EXECUTION_DIGEST_DOMAIN,
        struct.pack(">H", 1),
        struct.pack(">H", 2),
        struct.pack(">H", 1),
        struct.pack(">H", 2),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        b"PXMT",
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">H", 2_048),
        struct.pack(">H", 512),
        b"paraegox.runtime.managed-model-agent-stack-terminal-result.sha256.v1",
        b"ParaEGOX\0managed-model-agent-stack-terminal-signing",
        b"paraegox.runtime.managed-model-agent-stack-terminal-receipt.sha256.v1",
        MODEL_ASSIGNMENT_DIGEST_DOMAIN,
        PXTA_ZERO,
    )


def _artifact_compatibility_digest() -> bytes:
    return _canonical_digest(
        ARTIFACT_COMPATIBILITY_DOMAIN,
        _model_compatibility_digest(),
        b"PXTE",
        struct.pack(">H", 11),
        struct.pack(">I", 2_506),
        b"PXAR",
        struct.pack(">H", 12),
        struct.pack(">I", 6_630),
        struct.pack(">H", 1),
        struct.pack(">H", 1),
        struct.pack(">I", 192),
        b"px-art-prefix-v1",
        struct.pack(">I", 1),
        b"px-bounded-text1",
        _execution_profile_commitment(),
        ARTIFACT_EXECUTION_DIGEST_DOMAIN,
        ARTIFACT_ASSIGNMENT_DIGEST_DOMAIN,
        ARTIFACT_BINDING_DIGEST_DOMAIN,
        b"PXMT",
        struct.pack(">H", 1),
        PXTA_ZERO,
    )


def _ledger_agent_plan() -> dict[str, Any]:
    agent = LEDGER["agent_plan"]
    return {
        "service_id_hex": agent["service_id_hex"],
        "prepare_budget_nanos": agent["prepare_budget_nanos"],
        "start_budget_nanos": agent["start_budget_nanos"],
        "readiness_budget_nanos": agent["readiness_budget_nanos"],
        "drain_budget_nanos": agent["drain_budget_nanos"],
        "stop_budget_nanos": agent["stop_budget_nanos"],
        "max_sessions": agent["max_sessions"],
        "max_turns_per_session": agent["max_turns_per_session"],
        "max_requests_per_session": agent["max_requests_per_session"],
        "max_event_batch": agent["max_event_batch"],
        "submit_binding_id_hex": agent["submit_binding_id_hex"],
        "control_binding_id_hex": agent["control_binding_id_hex"],
        "submit_key_expression": agent["submit_key_expression"],
        "control_key_expression": agent["control_key_expression"],
        "max_items": agent["max_items"],
        "max_bytes": agent["max_bytes"],
        "max_frame_bytes": agent["max_frame_bytes"],
        "max_response_body_bytes": agent["max_response_body_bytes"],
        "handler_timeout_nanos": agent["handler_timeout_nanos"],
        "provider_profile": agent["provider_profile"],
        "provider_ref_hex": agent["provider_ref_hex"],
        "config_digest_hex": agent["provider_config_digest_hex"],
        "secret_ref_hex": "00" * 16,
    }


def _encode_provider(plan: dict[str, Any]) -> bytes:
    secret = bytes(16)
    return (
        struct.pack(">HBB", 1, plan["provider_profile"], 0)
        + bytes.fromhex(plan["provider_ref_hex"])
        + bytes.fromhex(plan["provider_config_digest_hex"])
        + bytes([int(plan["provider_secret_present"])])
        + secret
    )


def _encode_model_plan() -> bytes:
    model = LEDGER["model_plan"]
    budgets = (
        model["prepare_budget_nanos"],
        model["start_budget_nanos"],
        model["readiness_budget_nanos"],
        model["drain_budget_nanos"],
        model["stop_budget_nanos"],
    )
    wire = bytearray(struct.pack(">H", 1) + bytes.fromhex(model["service_id_hex"]))
    wire += b"".join(struct.pack(">Q", value) for value in budgets)
    wire += struct.pack(">H", model["max_in_flight"])
    wire += _encode_provider(LEDGER["agent_plan"])
    wire += struct.pack(">H", 1)
    wire += model["artifact_adapter_id"].encode("ascii")
    wire += struct.pack(">I", model["adapter_version"])
    wire += model["capability_id"].encode("ascii")
    assert len(wire) == 167
    return bytes(wire)


def _encode_dependency(kind: int, provider: bytes, consumer: bytes) -> bytes:
    frame = struct.pack(">HBBBB", 1, kind, 1, 1, 0) + provider + consumer
    assert len(frame) == 38
    return frame


def _encode_artifact_execution_stack() -> dict[str, bytes]:
    fabric = AGENT.FABRIC._build_vectors()
    base_projection = fabric["projection"]
    agent_projection = AGENT._encode_projection(base_projection)
    agent_pxte = AGENT._encode_pxte(
        agent_projection,
        AGENT.MODE_FABRIC_AND_AGENT,
        fabric["one"]["pxte"],
        _ledger_agent_plan(),
    )
    model_projection = (
        b"PXMM"
        + struct.pack(">H", 1)
        + agent_projection
        + _model_compatibility_digest()
        + struct.pack(">HH", 9, 1)
    )
    assert len(model_projection) == MODEL_PROJECTION_BYTES
    model = LEDGER["model_plan"]
    agent = LEDGER["agent_plan"]
    predecessor = LEDGER["predecessor"]
    model_pxte = bytearray(b"PXTE" + struct.pack(">H", 8) + model_projection)
    model_pxte += struct.pack(">HBBBBI", 1, 1, 1, 2, 0, len(agent_pxte))
    model_pxte += agent_pxte + _encode_model_plan()
    model_pxte += _encode_dependency(
        1,
        bytes.fromhex(predecessor["fabric_service_id_hex"]),
        bytes.fromhex(agent["service_id_hex"]),
    )
    model_pxte += _encode_dependency(
        2,
        bytes.fromhex(model["service_id_hex"]),
        bytes.fromhex(agent["service_id_hex"]),
    )
    binding = _fixture("artifact_f0_binding_v1.hex", 192)
    artifact_pxte = bytearray(b"PXTE" + struct.pack(">H", 11) + model_projection)
    artifact_pxte += _artifact_compatibility_digest()
    artifact_pxte += struct.pack(">HHII", 1, 1, len(binding), len(model_pxte))
    artifact_pxte += binding + model_pxte
    plan_content = bytearray(b"ParaEGOX\0deployment-plan-content")
    plan_content += struct.pack(">HBBI", 2, 3, 0, 252 + len(artifact_pxte))
    plan_content += bytes.fromhex(predecessor["target_hex"])
    plan_content += binding + struct.pack(">I", len(artifact_pxte)) + artifact_pxte
    return {
        "agent_pxte": agent_pxte,
        "model_projection": model_projection,
        "model_pxte": bytes(model_pxte),
        "artifact_pxte": bytes(artifact_pxte),
        "plan_content": bytes(plan_content),
    }


def _decode_artifact_binding(frame: bytes) -> dict[str, bytes | int]:
    assert len(frame) == 192
    object_ref = frame[:72]
    assert object_ref[:4] == b"PXAK"
    assert struct.unpack(">H", object_ref[4:6])[0] == 1
    assert struct.unpack(">H", object_ref[6:8])[0] == 72
    assert frame[160:192] == _execution_profile_commitment()
    return {
        "object_ref": object_ref,
        "materialization_store_instance": frame[72:104],
        "materialization_sequence": struct.unpack(">Q", frame[104:112])[0],
        "materialization_operation_id": frame[112:128],
        "materialization_receipt_digest": frame[128:160],
    }


def _decode_pxdq(frame: bytes) -> dict[str, bytes | int]:
    assert frame[:4] == b"PXDQ"
    assert struct.unpack(">H", frame[4:6])[0] == 1
    assert frame[6:8] == b"D\x00"
    assert struct.unpack(">H", frame[8:10])[0] == 288
    assert frame[10:12] == b"\x00\x00"
    assert struct.unpack(">I", frame[12:16])[0] == 288
    assert frame[64:68] == b"PXAK"
    assert frame[224:256] == _execution_profile_commitment()
    assert frame[256:288] == _raw_digest(
        b"paraegox.deployment.external-request.sha256.v1", frame[:256]
    )
    return {
        "operation_id": frame[16:32],
        "config_commitment": frame[32:64],
        "object_ref": frame[64:136],
        "materialization_store_instance": frame[136:168],
        "materialization_sequence": struct.unpack(">Q", frame[168:176])[0],
        "materialization_operation_id": frame[176:192],
        "materialization_receipt_digest": frame[192:224],
        "request_digest": frame[256:288],
    }


def _decode_pxdk(frame: bytes, request: dict[str, bytes | int]) -> dict[str, bytes | int]:
    assert frame[:4] == b"PXDK"
    assert struct.unpack(">H", frame[4:6])[0] == 1
    assert frame[6:8] == b"DA"
    assert struct.unpack(">H", frame[8:10])[0] == 240
    assert frame[10:12] == b"\x00\x00"
    assert struct.unpack(">I", frame[12:16])[0] == 240
    assert frame[56:72] == request["operation_id"]
    assert frame[72:104] == request["request_digest"]
    assert frame[104:176] == request["object_ref"]
    assert frame[176:208] == bytes(32)
    assert frame[208:240] == _raw_digest(
        b"paraegox.deployment.external-admission.sha256.v1", frame[:208]
    )
    return {
        "controller_store_instance": frame[16:48],
        "admission_sequence": struct.unpack(">Q", frame[48:56])[0],
        "admission_digest": frame[208:240],
    }


def _encode_pre_c_terminal_record(
    state: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> bytes:
    assert state in (b"F", b"U")
    lifecycle_generation = bytes.fromhex(
        LEDGER["deployment"]["lifecycle_generation_hex"]
    )
    frame = bytearray()
    frame += b"PXDM" + struct.pack(">H", 1) + state + state
    frame += struct.pack(">H", 496) + bytes(2) + struct.pack(">I", 496)
    frame += pxdk[16:48] + struct.pack(">Q", 1)
    frame += pxdq[16:32] + pxdq[256:288] + pxdk[208:240]
    frame += pxdq[64:136] + pxdq[192:224]
    frame += struct.pack(">QQ", 0, 0) + bytes(96)
    frame += lifecycle_generation + pxdq[224:256] + bytes(64)
    assert len(frame) == 464
    frame += _raw_digest(
        b"paraegox.deployment.external-operation-record.sha256.v1", frame
    )
    assert len(frame) == 496
    return bytes(frame)


def _encode_pre_c_terminal_receipt(
    state: bytes,
    record: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> bytes:
    lifecycle_generation = bytes.fromhex(
        LEDGER["deployment"]["lifecycle_generation_hex"]
    )
    frame = bytearray()
    frame += b"PXDO" + struct.pack(">H", 1) + b"D" + state
    frame += struct.pack(">H", 432) + bytes(2) + struct.pack(">I", 432)
    frame += pxdk[16:48] + struct.pack(">Q", 1)
    frame += pxdq[16:32] + pxdq[256:288] + record[464:496]
    frame += pxdq[64:136] + pxdq[192:224]
    frame += struct.pack(">QQ", 0, 0) + bytes(96)
    frame += lifecycle_generation + bytes(32)
    assert len(frame) == 400
    frame += _raw_digest(
        b"paraegox.deployment.external-receipt.sha256.v1", frame
    )
    assert len(frame) == 432
    return bytes(frame)


def _encode_pxmj2_prefix(
    phase: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> bytes:
    assert phase in (b"A", b"F", b"U")
    record = b""
    receipt = b""
    if phase != b"A":
        record = _encode_pre_c_terminal_record(phase, pxdq, pxdk)
        receipt = _encode_pre_c_terminal_receipt(phase, record, pxdq, pxdk)
    body = pxdq + pxdk + record + receipt
    frame_length = 192 + len(body) + 32
    header = bytearray()
    header += b"PXMJ" + struct.pack(">HHI", 2, 192, frame_length)
    header += phase + bytes(3)
    header += struct.pack(">Q", 1 if phase == b"A" else 2)
    header += pxdk[16:48]
    header += struct.pack(">Q", struct.unpack(">Q", pxdk[48:56])[0])
    header += struct.pack(">Q", 0 if phase == b"A" else 1)
    header += struct.pack(">Q", 0) + bytes(64)
    header += struct.pack(">IIIIII", 288, 240, 0, 0, 0, 0)
    header += struct.pack(">HHI", 0 if phase == b"A" else 1, 0 if phase == b"A" else 1, len(body))
    header += bytes(16)
    assert len(header) == 192
    frame = bytes(header) + body
    frame += _canonical_digest(
        b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2",
        frame,
    )
    assert len(frame) == frame_length
    return frame


def _decode_pxmj2_prefix(
    frame: bytes,
    phase: bytes,
    pxdq: bytes,
    pxdk: bytes,
) -> None:
    assert frame[:4] == b"PXMJ"
    assert struct.unpack(">H", frame[4:6])[0] == 2
    assert struct.unpack(">H", frame[6:8])[0] == 192
    assert struct.unpack(">I", frame[8:12])[0] == len(frame)
    assert frame[12:13] == phase
    assert frame[13:16] == bytes(3)
    assert struct.unpack(">Q", frame[16:24])[0] == (1 if phase == b"A" else 2)
    assert frame[24:56] == pxdk[16:48]
    assert struct.unpack(">Q", frame[56:64])[0] == 1
    assert struct.unpack(">Q", frame[64:72])[0] == (0 if phase == b"A" else 1)
    assert frame[72:144] == bytes(72)
    assert struct.unpack(">IIIIII", frame[144:168]) == (288, 240, 0, 0, 0, 0)
    assert struct.unpack(">HH", frame[168:172]) == (
        0 if phase == b"A" else 1,
        0 if phase == b"A" else 1,
    )
    body_length = struct.unpack(">I", frame[172:176])[0]
    assert frame[176:192] == bytes(16)
    assert body_length == len(frame) - 224
    assert frame[192:480] == pxdq
    assert frame[480:720] == pxdk
    assert frame[-32:] == _canonical_digest(
        b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2",
        frame[:-32],
    )
    if phase == b"A":
        assert len(frame) == 752
        assert frame[720:-32] == b""
    else:
        assert len(frame) == 1680
        record = frame[720:1216]
        receipt = frame[1216:1648]
        assert record == _encode_pre_c_terminal_record(phase, pxdq, pxdk)
        assert receipt == _encode_pre_c_terminal_receipt(phase, record, pxdq, pxdk)


def test_artifact_execution_binding_shared_golden_is_independently_derived() -> None:
    binding_wire = _fixture("artifact_f0_binding_v1.hex", 192)
    binding = _decode_artifact_binding(binding_wire)
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    request = _decode_pxdq(pxdq)
    receipt = _fixture("artifact_f0_pxax_materialized_v1.hex", 240)
    artifact = LEDGER["artifact_store"]

    assert binding_wire == pxdq[64:256]
    assert binding["object_ref"] == _fixture("artifact_f0_pxak_v1.hex", 72)
    assert binding["materialization_store_instance"] == receipt[16:48]
    assert binding["materialization_sequence"] == struct.unpack(">Q", receipt[48:56])[0]
    assert binding["materialization_operation_id"] == receipt[56:72]
    assert binding["materialization_receipt_digest"] == receipt[208:240]
    assert binding["materialization_store_instance"] == bytes.fromhex(
        artifact["store_instance_hex"]
    )
    assert binding["materialization_sequence"] == artifact["primary_operation_sequence"]
    assert binding["materialization_operation_id"] == bytes.fromhex(
        artifact["primary_operation_id_hex"]
    )
    assert binding["object_ref"] == request["object_ref"]
    assert binding["materialization_receipt_digest"] == request[
        "materialization_receipt_digest"
    ]


def test_artifact_pxte11_and_plan_content_goldens_are_independently_derived() -> None:
    expected = _encode_artifact_execution_stack()
    pxte = _fixture("artifact_f0_pxte_v11.hex", 1_805)
    plan = _fixture("artifact_f0_plan_content_v2.hex", 2_057)
    binding = _fixture("artifact_f0_binding_v1.hex", 192)

    assert pxte == expected["artifact_pxte"]
    assert pxte[:6] == b"PXTE" + struct.pack(">H", 11)
    assert pxte[6 : 6 + MODEL_PROJECTION_BYTES] == expected["model_projection"]
    cursor = 6 + MODEL_PROJECTION_BYTES
    assert pxte[cursor : cursor + 32] == _artifact_compatibility_digest()
    cursor += 32
    assert struct.unpack(">HHII", pxte[cursor : cursor + 12]) == (
        1,
        1,
        len(binding),
        len(expected["model_pxte"]),
    )
    cursor += 12
    assert pxte[cursor : cursor + len(binding)] == binding
    cursor += len(binding)
    assert pxte[cursor:] == expected["model_pxte"]
    assert expected["model_pxte"][:6] == b"PXTE" + struct.pack(">H", 8)
    model_tail = 6 + MODEL_PROJECTION_BYTES
    profile, mode, present, dependency_count, reserved, embedded_length = struct.unpack(
        ">HBBBBI", expected["model_pxte"][model_tail:MODEL_PXTE_FIXED_BYTES]
    )
    assert (profile, mode, present, dependency_count, reserved) == (1, 1, 1, 2, 0)
    agent_end = MODEL_PXTE_FIXED_BYTES + embedded_length
    assert expected["model_pxte"][MODEL_PXTE_FIXED_BYTES:agent_end] == expected[
        "agent_pxte"
    ]
    AGENT._decode_pxte(expected["agent_pxte"])

    assert plan == expected["plan_content"]
    assert plan[:32] == b"ParaEGOX\0deployment-plan-content"
    assert struct.unpack(">HBBI", plan[32:40]) == (2, 3, 0, len(plan))
    assert plan[40:56] == bytes.fromhex(LEDGER["predecessor"]["target_hex"])
    assert plan[56:248] == binding
    assert struct.unpack(">I", plan[248:252])[0] == len(pxte)
    assert plan[252:] == pxte


def test_pxdq_pxdk_shared_goldens_have_independent_exact_layout_and_correlation() -> None:
    request = _decode_pxdq(_fixture("artifact_f0_pxdq_v1.hex", 288))
    admission = _decode_pxdk(_fixture("artifact_f0_pxdk_v1.hex", 240), request)
    artifact = LEDGER["artifact_store"]
    deployment = LEDGER["deployment"]
    assert request["operation_id"] == bytes.fromhex(deployment["operation_id_hex"])
    assert request["config_commitment"] == bytes.fromhex(
        artifact["config_commitment_hex"]
    )
    assert request["materialization_store_instance"] == bytes.fromhex(
        artifact["store_instance_hex"]
    )
    assert request["materialization_sequence"] == artifact["primary_operation_sequence"]
    assert request["materialization_operation_id"] == bytes.fromhex(
        artifact["primary_operation_id_hex"]
    )
    assert admission["controller_store_instance"] == bytes.fromhex(
        deployment["controller_store_instance_hex"]
    )
    assert admission["admission_sequence"] == deployment["admission_sequence"]


def test_pxdq_pxdk_reserved_or_digest_mutation_fails_independent_decoder() -> None:
    pxdq = bytearray(_fixture("artifact_f0_pxdq_v1.hex", 288))
    pxdq[7] = 1
    try:
        _decode_pxdq(bytes(pxdq))
    except AssertionError:
        pass
    else:
        raise AssertionError("reserved PXDQ byte was accepted")

    pxdq = bytearray(_fixture("artifact_f0_pxdq_v1.hex", 288))
    pxdq[-1] ^= 1
    try:
        _decode_pxdq(bytes(pxdq))
    except AssertionError:
        pass
    else:
        raise AssertionError("PXDQ digest drift was accepted")

    request = _decode_pxdq(_fixture("artifact_f0_pxdq_v1.hex", 288))
    pxdk = bytearray(_fixture("artifact_f0_pxdk_v1.hex", 240))
    pxdk[176] = 1
    try:
        _decode_pxdk(bytes(pxdk), request)
    except AssertionError:
        pass
    else:
        raise AssertionError("fresh-only previous desired head was accepted")


def test_pxmj2_admitted_and_pre_c_terminal_goldens_are_independently_derived() -> None:
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    pxdk = _fixture("artifact_f0_pxdk_v1.hex", 240)
    for name, phase, size in (
        ("artifact_f0_pxmj_v2_admitted.hex", b"A", 752),
        ("artifact_f0_pxmj_v2_failed_pre_c.hex", b"F", 1680),
        ("artifact_f0_pxmj_v2_uncertain_pre_c.hex", b"U", 1680),
    ):
        fixture = _fixture(name, size)
        assert fixture == _encode_pxmj2_prefix(phase, pxdq, pxdk)
        _decode_pxmj2_prefix(fixture, phase, pxdq, pxdk)


def test_pxmj2_pre_c_outer_checksum_and_terminal_pin_drift_are_rejected() -> None:
    pxdq = _fixture("artifact_f0_pxdq_v1.hex", 288)
    pxdk = _fixture("artifact_f0_pxdk_v1.hex", 240)
    admitted = bytearray(_fixture("artifact_f0_pxmj_v2_admitted.hex", 752))
    admitted[-1] ^= 1
    try:
        _decode_pxmj2_prefix(bytes(admitted), b"A", pxdq, pxdk)
    except AssertionError:
        pass
    else:
        raise AssertionError("PXMJ2 outer checksum drift was accepted")

    failed = bytearray(_fixture("artifact_f0_pxmj_v2_failed_pre_c.hex", 1680))
    failed[720 + 248 : 720 + 256] = struct.pack(">Q", 2)
    failed[720 + 464 : 720 + 496] = _raw_digest(
        b"paraegox.deployment.external-operation-record.sha256.v1",
        failed[720 : 720 + 464],
    )
    failed[1216 + 104 : 1216 + 136] = failed[720 + 464 : 720 + 496]
    failed[1216 + 248 : 1216 + 256] = struct.pack(">Q", 2)
    failed[1216 + 400 : 1216 + 432] = _raw_digest(
        b"paraegox.deployment.external-receipt.sha256.v1",
        failed[1216 : 1216 + 400],
    )
    failed[-32:] = _canonical_digest(
        b"paraegox.deployment.artifact-bound-managed-model-agent-stack-state.sha256.v2",
        failed[:-32],
    )
    try:
        _decode_pxmj2_prefix(bytes(failed), b"F", pxdq, pxdk)
    except AssertionError:
        pass
    else:
        raise AssertionError("pre-C committed snapshot pin drift was accepted")
