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
