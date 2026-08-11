from __future__ import annotations

import hashlib
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WIRE = ROOT / "tests" / "fixtures" / "wire"
LEDGER = json.loads((WIRE / "artifact_f0_semantic_ledger_v1.json").read_text())


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
