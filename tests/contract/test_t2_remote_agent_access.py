"""Independent Python oracle for the frozen T2 remote-Agent wire contracts."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import struct
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
DATA_PLANE_FIXTURE = REPO_ROOT / "tests/fixtures/wire/t2_remote_agent_data_plane_v1.json"
ACCESS_FIXTURE = REPO_ROOT / "tests/fixtures/wire/t2_remote_agent_access_v1.json"
S7_ORACLE_PATH = REPO_ROOT / "tests/contract/test_s7_managed_agent_stack_successor.py"

FROZEN_CONTRACTS = {
    "remote_agent_data_plane_plan.rs": (
        "d444e7914045fc4f0d914a8483dd5fadbca78e008a00bf27ae1f7fbe233be736"
    ),
    "remote_agent_access.rs": ("9c1dbd62db65ac59612759633d713986d78a8a811f6a1c4629824abee74dcb74"),
    "managed_serving_bootstrap.rs": (
        "57cbe94fcd52b93b1471446c5cdf804a5536768c8f5dc03041794668e91da038"
    ),
    "lib.rs": "762061ea1295803be96a267714fe3f5f13caa3c4ac4fd7eb6a3b3865cf17e79f",
}

DIGEST_MAGIC = b"ParaEGOX\0canonical-digest"
DIGEST_VERSION = 1
PXTA_ZERO = b"PXTA\0\x01\0\0\0\0"

PROJECTION_MAGIC = b"PXAE"
PROJECTION_VERSION = 1
PROJECTION_BYTES = 270
PROFILE_MAGIC = b"PXAD"
PROFILE_VERSION = 1
PROFILE_KIND = 1
PROFILE_ACL_VERSION = 1
PROFILE_FIXED_BYTES = 158
PXTE_MAGIC = b"PXTE"
PXTE_VERSION = 9
PXAR_MAGIC = b"PXAR"
PXAR_VERSION = 10
PXAR_HEADER_BYTES = 18
PXAU_MAGIC = b"PXAU"
PXAU_VERSION = 1
PXAU_SIGNING_VERSION = 1

MAX_ENVELOPE_BYTES = 4_096
MAX_PREDECESSOR_PXTE_BYTES = 1_465
MAX_PROFILE_BYTES = 439
BOOTSTRAP_CAS_BYTES = 144
PXTE_FIXED_BYTES = 288
MAX_PXTE_BYTES = (
    PXTE_FIXED_BYTES + MAX_PREDECESSOR_PXTE_BYTES + BOOTSTRAP_CAS_BYTES + MAX_PROFILE_BYTES
)
MAX_PXAR_BYTES = PXAR_HEADER_BYTES + MAX_ENVELOPE_BYTES + len(PXTA_ZERO) + MAX_PXTE_BYTES
PXAU_FIXED_BYTES = (
    4
    + 2
    + 16
    + 32
    + 16
    + 16
    + (4 * 32)
    + 16
    + 6
    + 32
    + (3 * 9)
    + 2
    + 1
    + 1
    + (5 * 32)
    + 8
    + 8
    + 16
    + 8
    + 8
    + 16
    + 16
    + 2
    + 2
    + 2
)

COMPATIBILITY_DOMAIN = b"paraegox.runtime.remote-agent-data-plane-compatibility.sha256.v1"
PROFILE_DOMAIN = b"paraegox.runtime.remote-agent-data-plane-profile.sha256.v1"
PXTE_DOMAIN = b"paraegox.runtime.target-execution.sha256.v9"
PXTA_DOMAIN = b"paraegox.runtime.target-assignments.sha256.v1"
ASSIGNMENT_DOMAIN = b"paraegox.runtime.target-plan-assignments.sha256.v10"
PXAR_DOMAIN = b"paraegox.runtime.remote-agent-data-plane-request.sha256.v1"
PXAU_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-data-plane-terminal-signing"
PXAU_RESULT_REF_DOMAIN = b"paraegox.runtime.remote-agent-data-plane-terminal-result.sha256.v1"
PXAU_DOMAIN = b"paraegox.runtime.remote-agent-data-plane-terminal.sha256.v1"

PXRA_MAGIC = b"PXRA"
PXRR_MAGIC = b"PXRR"
ACCESS_VERSION = 1
PXRA_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-access-request-signing"
PXRR_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-access-response-signing"
ACCESS_SIGNING_VERSION = 1
PXRA_PAYLOAD_DOMAIN = b"paraegox.runtime.remote-agent-access.request-payload.sha256.v1"
PXRR_PAYLOAD_DOMAIN = b"paraegox.runtime.remote-agent-access.response-payload.sha256.v1"
PXRA_DOMAIN = b"paraegox.runtime.remote-agent-access.request.sha256.v1"
PXRR_DOMAIN = b"paraegox.runtime.remote-agent-access.response.sha256.v1"
PXCB_MAGIC = b"PXCB"
PXCB_VERSION = 1
PXCB_KIND = 1
PXCB_DOMAIN = b"paraegox.runtime.restricted-apply-carrier-binding.sha256.v1"
CONTROL_KEY_DOMAIN = b"paraegox.runtime.control-auth.ed25519-public-key.sha256.v1"
PXAP_SHARED_DOMAIN = b"paraegox.runtime.agent-control.receipt-payload.sha256.v1"
PXAH_WHOLE_DOMAIN = b"paraegox.runtime.agent-control.receipt.sha256.v1"

# This is the same deterministic key used by the frozen envelope-v2 predecessor:
# PXCB names one Controller request key for both the inner PXAR and outer PXRA.
CONTROLLER_SEED = bytes.fromhex("22" * 32)
INNER_RUNTIME_SEED = bytes.fromhex("43" * 32)
OUTER_RUNTIME_SEED = bytes.fromhex("44" * 32)


class ContractReject(ValueError):
    """Raised when the independent strict parser rejects a frame."""


def _load_s7_oracle() -> ModuleType:
    spec = importlib.util.spec_from_file_location("_paraegox_s7_t2_predecessor", S7_ORACLE_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


S7 = _load_s7_oracle()


def _u8(value: int) -> bytes:
    return struct.pack(">B", value)


def _u16(value: int) -> bytes:
    return struct.pack(">H", value)


def _u32(value: int) -> bytes:
    return struct.pack(">I", value)


def _u64(value: int) -> bytes:
    return struct.pack(">Q", value)


def _digest(domain: bytes, fields: list[bytes]) -> bytes:
    encoded = bytearray(DIGEST_MAGIC)
    encoded += _u16(DIGEST_VERSION) + _u32(len(domain)) + domain
    for ordinal, field in enumerate(fields, start=1):
        encoded += b"\x01" + _u32(ordinal) + _u64(len(field)) + field
    encoded += b"\xff" + _u32(len(fields))
    return hashlib.sha256(encoded).digest()


def _private(seed: bytes) -> Ed25519PrivateKey:
    return Ed25519PrivateKey.from_private_bytes(seed)


def _public(seed: bytes) -> bytes:
    return _private(seed).public_key().public_bytes_raw()


def _fingerprint(public_key: bytes) -> bytes:
    return _digest(CONTROL_KEY_DOMAIN, [_u16(1), b"Ed25519", public_key])


def _compatibility_digest() -> bytes:
    return _digest(
        COMPATIBILITY_DOMAIN,
        [
            PROJECTION_MAGIC,
            _u16(PROJECTION_VERSION),
            _u16(PROJECTION_BYTES),
            PXAR_MAGIC,
            _u16(PXAR_VERSION),
            _u16(PXAR_HEADER_BYTES),
            _u32(MAX_PXAR_BYTES),
            PXTE_MAGIC,
            _u16(PXTE_VERSION),
            _u32(MAX_PXTE_BYTES),
            PROFILE_MAGIC,
            _u16(PROFILE_VERSION),
            _u16(PROFILE_KIND),
            _u16(PROFILE_ACL_VERSION),
            PXTA_ZERO,
            _u16(1),
            _u16(2),
            PXTE_DOMAIN,
            ASSIGNMENT_DOMAIN,
            PXAR_DOMAIN,
            PXAU_MAGIC,
            _u16(PXAU_VERSION),
            _u16(PXAU_SIGNING_VERSION),
            _u32(PXAU_FIXED_BYTES),
            _u16(2_048),
            *[_u16(value) for value in range(1, 6)],
            *[_u16(value) for value in range(1, 5)],
            _u16(2),
            PXAU_SIGNING_MAGIC,
            PXAU_DOMAIN,
        ],
    )


def _profile() -> dict[str, Any]:
    base = b"tcp/127.0.0.1:7447"
    tls = b"tls/10.0.0.2:7448"
    values = {
        "target": bytes.fromhex("05" * 16),
        "endpoint_ref": bytes.fromhex("81" * 16),
        "endpoint_generation": 17,
        "trust_domain_ref": bytes.fromhex("82" * 16),
        "trust_anchor_ref": bytes.fromhex("83" * 16),
        "mac_connector_credential_ref": bytes.fromhex("84" * 16),
        "ubuntu_listener_credential_ref": bytes.fromhex("85" * 16),
        "mac_principal": bytes.fromhex("86" * 16),
        "ubuntu_principal": bytes.fromhex("87" * 16),
        "operation_timeout_nanos": 20_000_000_000,
    }
    wire = b"".join(
        [
            PROFILE_MAGIC,
            _u16(PROFILE_VERSION),
            _u16(PROFILE_KIND),
            _u16(len(base)),
            _u16(len(tls)),
            _u16(PROFILE_ACL_VERSION),
            values["target"],
            values["endpoint_ref"],
            _u64(values["endpoint_generation"]),
            values["trust_domain_ref"],
            values["trust_anchor_ref"],
            values["mac_connector_credential_ref"],
            values["ubuntu_listener_credential_ref"],
            values["mac_principal"],
            values["ubuntu_principal"],
            _u64(values["operation_timeout_nanos"]),
            base,
            tls,
        ]
    )
    return {
        **values,
        "base": base,
        "tls": tls,
        "wire": wire,
        "digest": _digest(PROFILE_DOMAIN, [wire]),
    }


class _Cursor:
    def __init__(self, wire: bytes) -> None:
        self.wire = wire
        self.offset = 0

    def take(self, size: int) -> bytes:
        end = self.offset + size
        if end > len(self.wire):
            raise ContractReject("truncated")
        value = self.wire[self.offset : end]
        self.offset = end
        return value

    def u8(self) -> int:
        return self.take(1)[0]

    def u16(self) -> int:
        return struct.unpack(">H", self.take(2))[0]

    def u32(self) -> int:
        return struct.unpack(">I", self.take(4))[0]

    def u64(self) -> int:
        return struct.unpack(">Q", self.take(8))[0]

    def finish(self) -> None:
        if self.offset != len(self.wire):
            raise ContractReject("trailing bytes")


def _parse_profile(wire: bytes) -> dict[str, Any]:
    if len(wire) < PROFILE_FIXED_BYTES or len(wire) > MAX_PROFILE_BYTES:
        raise ContractReject("PXAD length")
    cursor = _Cursor(wire)
    if cursor.take(4) != PROFILE_MAGIC or cursor.u16() != PROFILE_VERSION:
        raise ContractReject("PXAD magic/version")
    if cursor.u16() != PROFILE_KIND:
        raise ContractReject("PXAD kind")
    base_length, tls_length = cursor.u16(), cursor.u16()
    if cursor.u16() != PROFILE_ACL_VERSION or not base_length or not tls_length:
        raise ContractReject("PXAD lengths")
    parsed = {
        "target": cursor.take(16),
        "endpoint_ref": cursor.take(16),
        "endpoint_generation": cursor.u64(),
        "trust_domain_ref": cursor.take(16),
        "trust_anchor_ref": cursor.take(16),
        "mac_connector_credential_ref": cursor.take(16),
        "ubuntu_listener_credential_ref": cursor.take(16),
        "mac_principal": cursor.take(16),
        "ubuntu_principal": cursor.take(16),
        "operation_timeout_nanos": cursor.u64(),
        "base": cursor.take(base_length),
        "tls": cursor.take(tls_length),
    }
    cursor.finish()
    fixed_values = [value for key, value in parsed.items() if key.endswith("ref")]
    if (
        any(value == bytes(len(value)) for value in fixed_values)
        or parsed["target"] == bytes(16)
        or parsed["endpoint_generation"] == 0
        or parsed["mac_principal"] in {bytes(16), parsed["ubuntu_principal"]}
        or parsed["operation_timeout_nanos"] not in range(1, 30_000_000_001)
        or parsed["base"] != b"tcp/127.0.0.1:7447"
        or not parsed["tls"].startswith(b"tls/")
    ):
        raise ContractReject("PXAD semantic")
    parsed["wire"] = wire
    parsed["digest"] = _digest(PROFILE_DOMAIN, [wire])
    return parsed


def _projection(predecessor_projection: bytes) -> bytes:
    S7._decode_projection(predecessor_projection)
    wire = b"".join(
        [
            PROJECTION_MAGIC,
            _u16(PROJECTION_VERSION),
            predecessor_projection,
            _compatibility_digest(),
            _u16(PXAR_VERSION),
            _u16(PROFILE_VERSION),
        ]
    )
    if len(wire) != PROJECTION_BYTES:
        raise AssertionError("PXAE width")
    return wire


def _parse_projection(wire: bytes) -> dict[str, bytes]:
    if len(wire) != PROJECTION_BYTES:
        raise ContractReject("PXAE length")
    if wire[:6] != PROJECTION_MAGIC + _u16(PROJECTION_VERSION):
        raise ContractReject("PXAE magic/version")
    predecessor = wire[6:234]
    S7._decode_projection(predecessor)
    if wire[234:266] != _compatibility_digest() or wire[266:] != _u16(10) + _u16(1):
        raise ContractReject("PXAE compatibility")
    return {"predecessor": predecessor, "target": predecessor[44:60]}


def _encode_pxte(
    projection: bytes,
    predecessor: bytes,
    profile: bytes,
    bootstrap_cas: dict[str, Any],
) -> bytes:
    S7._decode_pxte(predecessor)
    _parse_profile(profile)
    cas = b"".join(
        [
            bootstrap_cas["active_pxft_digest"],
            bootstrap_cas["active_pxst_digest"],
            bootstrap_cas["descriptor_receipt_digest"],
            bootstrap_cas["descriptor_payload_digest"],
            _u64(bootstrap_cas["fabric_generation"]),
            _u64(bootstrap_cas["agent_generation"]),
        ]
    )
    assert len(cas) == BOOTSTRAP_CAS_BYTES
    return b"".join(
        [
            PXTE_MAGIC,
            _u16(PXTE_VERSION),
            projection,
            _u16(PROFILE_VERSION),
            _u8(1),
            _u8(1),
            _u32(len(predecessor)),
            _u32(len(profile)),
            predecessor,
            cas,
            profile,
        ]
    )


def _parse_pxte(wire: bytes) -> dict[str, Any]:
    cursor = _Cursor(wire)
    if cursor.take(4) != PXTE_MAGIC or cursor.u16() != PXTE_VERSION:
        raise ContractReject("PXTE9 magic/version")
    projection = cursor.take(PROJECTION_BYTES)
    projection_values = _parse_projection(projection)
    if cursor.u16() != PROFILE_VERSION or cursor.u8() != 1 or cursor.u8() != 1:
        raise ContractReject("PXTE9 mode/profile")
    predecessor_length, profile_length = cursor.u32(), cursor.u32()
    predecessor = cursor.take(predecessor_length)
    S7._decode_pxte(predecessor)
    cas = {
        "active_pxft_digest": cursor.take(32),
        "active_pxst_digest": cursor.take(32),
        "descriptor_receipt_digest": cursor.take(32),
        "descriptor_payload_digest": cursor.take(32),
        "fabric_generation": cursor.u64(),
        "agent_generation": cursor.u64(),
    }
    profile = _parse_profile(cursor.take(profile_length))
    cursor.finish()
    if any(value == bytes(32) for key, value in cas.items() if key.endswith("digest")):
        raise ContractReject("PXTE9 zero CAS")
    if profile["target"] != projection_values["target"] or profile["base"] != b"tcp/127.0.0.1:7447":
        raise ContractReject("PXTE9 target/profile")
    return {
        "projection": projection,
        "predecessor": predecessor,
        "cas": cas,
        "profile": profile,
        "wire": wire,
        "digest": _digest(PXTE_DOMAIN, [wire]),
    }


def _assignment_digest(pxte: bytes) -> bytes:
    _parse_pxte(pxte)
    return _digest(
        ASSIGNMENT_DOMAIN,
        [_digest(PXTA_DOMAIN, [PXTA_ZERO]), _digest(PXTE_DOMAIN, [pxte])],
    )


def _encode_pxar(envelope: bytes, pxte: bytes) -> bytes:
    S7.FABRIC.LEGACY._decode_envelope(envelope)
    _parse_pxte(pxte)
    return b"".join(
        [
            PXAR_MAGIC,
            _u16(PXAR_VERSION),
            _u32(len(envelope)),
            _u32(len(PXTA_ZERO)),
            _u32(len(pxte)),
            envelope,
            PXTA_ZERO,
            pxte,
        ]
    )


def _parse_pxar(wire: bytes) -> dict[str, Any]:
    if len(wire) < PXAR_HEADER_BYTES or len(wire) > MAX_PXAR_BYTES:
        raise ContractReject("PXAR10 length")
    if wire[:4] != PXAR_MAGIC or struct.unpack_from(">H", wire, 4)[0] != PXAR_VERSION:
        raise ContractReject("PXAR10 magic/version")
    envelope_length, bindings_length, pxte_length = struct.unpack_from(">III", wire, 6)
    if bindings_length != len(PXTA_ZERO):
        raise ContractReject("PXAR10 bindings")
    expected = PXAR_HEADER_BYTES + envelope_length + bindings_length + pxte_length
    if expected != len(wire):
        raise ContractReject("PXAR10 declared length")
    envelope_end = PXAR_HEADER_BYTES + envelope_length
    bindings_end = envelope_end + bindings_length
    envelope = S7.FABRIC.LEGACY._decode_envelope(wire[PXAR_HEADER_BYTES:envelope_end])
    if wire[envelope_end:bindings_end] != PXTA_ZERO:
        raise ContractReject("PXAR10 nonempty assignments")
    execution = _parse_pxte(wire[bindings_end:])
    assignment = _assignment_digest(execution["wire"])
    if envelope[7] != assignment or envelope[2] != execution["profile"]["target"]:
        raise ContractReject("PXAR10 commitment")
    return {
        "wire": wire,
        "envelope": envelope,
        "execution": execution,
        "assignment_digest": assignment,
        "request_digest": _digest(PXAR_DOMAIN, [wire]),
    }


def _terminal_result_ref(request: dict[str, Any]) -> bytes:
    envelope = request["envelope"]
    return _digest(
        PXAU_RESULT_REF_DOMAIN,
        [
            PXAU_MAGIC,
            _u16(PXAU_VERSION),
            envelope[2],
            envelope[32],
            envelope[3],
            envelope[24],
            _digest(S7.FABRIC.LEGACY.REQUEST_DIGEST_DOMAIN, [request["envelope_wire"]]),
            request["request_digest"],
        ],
    )[:16]


def _terminal_body(request: dict[str, Any], bootstrap: dict[str, Any]) -> bytes:
    envelope = request["envelope"]
    envelope_request_digest = _digest(
        S7.FABRIC.LEGACY.REQUEST_DIGEST_DOMAIN, [request["envelope_wire"]]
    )
    desired_head = envelope[8]
    terminal_ref_request = {**request, "envelope_wire": request["envelope_wire"]}
    body = bytearray()
    body += envelope[2] + envelope[32] + envelope[3] + envelope[24]
    body += envelope_request_digest + request["request_digest"] + envelope[8]
    body += request["assignment_digest"] + _terminal_result_ref(terminal_ref_request)
    body += _u8(1) + _u8(1) + _u8(2) + _u8(3) + _u8(1) + _u8(0)
    body += desired_head
    for generation in (9, 10, 11):
        body += _u8(1) + _u64(generation)
    body += _u16(2) + _u8(0b0111) + _u8(3)
    body += bootstrap["descriptor_receipt_digest"]
    body += bootstrap["descriptor_payload_digest"]
    body += bootstrap["fresh_descriptor_payload_digest"]
    body += bytes.fromhex("a1" * 32) + bytes.fromhex("a2" * 32)
    body += _u64(23) + _u64(29) + envelope[28] + _u64(5) + _u64(43)
    body += bootstrap["runtime_principal"] + bootstrap["inner_runtime_key_ref"]
    body += _u16(1) + _u16(1)
    return bytes(body)


def _encode_pxau(request_wire: bytes, bootstrap: dict[str, Any]) -> dict[str, bytes]:
    request = _parse_pxar(request_wire)
    envelope_length = struct.unpack_from(">I", request_wire, 6)[0]
    request["envelope_wire"] = request_wire[18 : 18 + envelope_length]
    body = _terminal_body(request, bootstrap)
    transcript = PXAU_SIGNING_MAGIC + _u16(PXAU_SIGNING_VERSION) + body
    signature = _private(INNER_RUNTIME_SEED).sign(transcript)
    wire = PXAU_MAGIC + _u16(PXAU_VERSION) + body + _u16(len(signature)) + signature
    return {
        "wire": wire,
        "digest": _digest(PXAU_DOMAIN, [wire]),
        "transcript": transcript,
        "signature": signature,
        "public_key": _public(INNER_RUNTIME_SEED),
    }


def _resign_pxau(wire: bytes) -> bytes:
    signature_length_offset = len(wire) - 66
    assert wire[signature_length_offset : signature_length_offset + 2] == _u16(64)
    transcript = PXAU_SIGNING_MAGIC + _u16(1) + wire[6:signature_length_offset]
    signature = _private(INNER_RUNTIME_SEED).sign(transcript)
    Ed25519PublicKey.from_public_bytes(_public(INNER_RUNTIME_SEED)).verify(signature, transcript)
    return wire[:-64] + signature


def _parse_pxau(wire: bytes, request_wire: bytes, public_key: bytes) -> dict[str, Any]:
    request = _parse_pxar(request_wire)
    envelope_length = struct.unpack_from(">I", request_wire, 6)[0]
    envelope_wire = request_wire[18 : 18 + envelope_length]
    cursor = _Cursor(wire)
    if cursor.take(4) != PXAU_MAGIC or cursor.u16() != PXAU_VERSION:
        raise ContractReject("PXAU magic/version")
    facts = {
        "target": cursor.take(16),
        "store": cursor.take(32),
        "source_scope": cursor.take(16),
        "operation_id": cursor.take(16),
        "envelope_digest": cursor.take(32),
        "request_digest": cursor.take(32),
        "target_slice_digest": cursor.take(32),
        "assignment_digest": cursor.take(32),
        "terminal_result_ref": cursor.take(16),
        "mode": cursor.u8(),
        "outcome": cursor.u8(),
        "effect": cursor.u8(),
        "head": cursor.u8(),
        "desired_present": cursor.u8(),
        "reserved": cursor.u8(),
        "desired_head": cursor.take(32),
    }
    generations = []
    for _ in range(3):
        present, value = cursor.u8(), cursor.u64()
        if present != 1 or value == 0:
            raise ContractReject("PXAU generations")
        generations.append(value)
    evidence = {
        "census": cursor.u16(),
        "flags": cursor.u8(),
        "observation": cursor.u8(),
        "descriptor_receipt_digest": cursor.take(32),
        "descriptor_payload_digest": cursor.take(32),
        "fresh_descriptor_payload_digest": cursor.take(32),
        "resource_digest": cursor.take(32),
        "raw_digest": cursor.take(32),
        "epoch": cursor.u64(),
        "snapshot": cursor.u64(),
        "clock_domain": cursor.take(16),
        "clock_generation": cursor.u64(),
        "observed_at": cursor.u64(),
    }
    auth = {
        "principal": cursor.take(16),
        "key": cursor.take(16),
        "algorithm": cursor.u16(),
        "version": cursor.u16(),
    }
    signature_length_offset = cursor.offset
    signature_length = cursor.u16()
    signature = cursor.take(signature_length)
    cursor.finish()
    expected_facts = {
        "target": request["envelope"][2],
        "store": request["envelope"][32],
        "source_scope": request["envelope"][3],
        "operation_id": request["envelope"][24],
        "envelope_digest": _digest(S7.FABRIC.LEGACY.REQUEST_DIGEST_DOMAIN, [envelope_wire]),
        "request_digest": request["request_digest"],
        "target_slice_digest": request["envelope"][8],
        "assignment_digest": request["assignment_digest"],
    }
    if any(facts[key] != value for key, value in expected_facts.items()):
        raise ContractReject("PXAU correlation")
    cas = request["execution"]["cas"]
    if (
        facts["mode"] != 1
        or (facts["outcome"], facts["effect"], facts["head"]) != (1, 2, 3)
        or facts["desired_present"] != 1
        or facts["reserved"] != 0
        or facts["desired_head"] != request["envelope"][8]
        or generations[0] <= cas["fabric_generation"]
        or generations[1] <= cas["agent_generation"]
        or evidence["census"] != 2
        or evidence["flags"] != 0b0111
        or evidence["observation"] != 3
        or evidence["descriptor_receipt_digest"] != cas["descriptor_receipt_digest"]
        or evidence["descriptor_payload_digest"] != cas["descriptor_payload_digest"]
        or evidence["fresh_descriptor_payload_digest"]
        in {
            bytes(32),
            cas["descriptor_payload_digest"],
        }
        or evidence["epoch"] == 0
        or evidence["clock_domain"] != request["envelope"][28]
        or evidence["clock_generation"] < struct.unpack(">Q", request["envelope"][29])[0]
        or evidence["observed_at"] == 0
        or auth["algorithm"] != 1
        or auth["version"] != 1
    ):
        raise ContractReject("PXAU semantic")
    transcript = PXAU_SIGNING_MAGIC + _u16(1) + wire[6:signature_length_offset]
    try:
        Ed25519PublicKey.from_public_bytes(public_key).verify(signature, transcript)
    except (InvalidSignature, ValueError) as error:
        raise ContractReject("PXAU signature") from error
    return {"facts": facts, "evidence": evidence, "auth": auth, "signature": signature}


def _carrier(target: bytes, controller_public: bytes, runtime_public: bytes) -> dict[str, bytes]:
    route = b"paraegox/runtime/t2/remote-agent-access/apply"
    values = {
        "target": target,
        "runtime_principal": bytes.fromhex("88" * 16),
        "controller_principal": bytes.fromhex("09" * 16),
        "endpoint_ref": bytes.fromhex("89" * 16),
        "controller_key_ref": bytes.fromhex("0c" * 16),
        "runtime_key_ref": bytes.fromhex("8a" * 16),
    }
    wire = b"".join(
        [
            PXCB_MAGIC,
            _u16(PXCB_VERSION),
            _u16(PXCB_KIND),
            _u16(len(route)),
            _u16(0),
            target,
            values["runtime_principal"],
            values["controller_principal"],
            values["endpoint_ref"],
            _u64(19),
            values["controller_key_ref"],
            _fingerprint(controller_public),
            values["runtime_key_ref"],
            _fingerprint(runtime_public),
            bytes.fromhex("8b" * 16),
            bytes.fromhex("8c" * 32),
            route,
        ]
    )
    return {**values, "wire": wire, "digest": _digest(PXCB_DOMAIN, [wire])}


def _parse_carrier(wire: bytes) -> dict[str, bytes]:
    cursor = _Cursor(wire)
    if cursor.take(4) != PXCB_MAGIC or (cursor.u16(), cursor.u16()) != (1, 1):
        raise ContractReject("PXCB magic/version")
    route_length = cursor.u16()
    if cursor.u16() != 0 or not route_length:
        raise ContractReject("PXCB header")
    values = {
        "target": cursor.take(16),
        "runtime_principal": cursor.take(16),
        "controller_principal": cursor.take(16),
        "endpoint_ref": cursor.take(16),
    }
    if cursor.u64() == 0:
        raise ContractReject("PXCB generation")
    values["controller_key_ref"] = cursor.take(16)
    values["controller_fingerprint"] = cursor.take(32)
    values["runtime_key_ref"] = cursor.take(16)
    values["runtime_fingerprint"] = cursor.take(32)
    values["transport_ref"] = cursor.take(16)
    values["transport_digest"] = cursor.take(32)
    route = cursor.take(route_length)
    cursor.finish()
    if any(value == bytes(len(value)) for value in values.values()) or not route:
        raise ContractReject("PXCB zero field")
    return {**values, "wire": wire, "digest": _digest(PXCB_DOMAIN, [wire])}


def _request_base(
    magic: bytes,
    kind: int,
    carrier: dict[str, bytes],
    request_id: bytes,
    store: bytes,
    epoch: int,
    pxau_digest: bytes,
    pxst_digest: bytes,
    profile_digest: bytes,
    intended_client: bytes,
    payload: bytes,
    nonce: bytes,
) -> bytes:
    payload_digest = bytes(32) if not payload else _digest(PXRA_PAYLOAD_DOMAIN, [payload])
    return b"".join(
        [
            magic,
            _u16(ACCESS_SIGNING_VERSION if magic == PXRA_SIGNING_MAGIC else ACCESS_VERSION),
            _u16(kind),
            _u16(0),
            _u16(len(carrier["wire"])),
            _u32(len(payload)),
            request_id,
            carrier["digest"],
            carrier["target"],
            store,
            _u64(epoch),
            pxau_digest,
            pxst_digest,
            profile_digest,
            intended_client,
            payload_digest,
            carrier["controller_principal"],
            carrier["controller_key_ref"],
            _u16(1),
            _u16(1),
            _u16(len(nonce)),
            nonce,
        ]
    )


def _encode_pxra(
    *,
    kind: int,
    carrier: dict[str, bytes],
    request_id: bytes,
    store: bytes,
    epoch: int,
    pxau_digest: bytes,
    pxst_digest: bytes,
    profile_digest: bytes,
    intended_client: bytes,
    payload: bytes,
    nonce: bytes,
) -> dict[str, bytes]:
    values = (kind, carrier, request_id, store, epoch, pxau_digest, pxst_digest, profile_digest)
    tail = (intended_client, payload, nonce)
    transcript_base = _request_base(PXRA_SIGNING_MAGIC, *values, *tail)
    transcript = transcript_base + carrier["wire"] + payload
    signature = _private(CONTROLLER_SEED).sign(transcript)
    wire_base = _request_base(PXRA_MAGIC, *values, *tail)
    wire = wire_base + _u16(len(signature)) + carrier["wire"] + payload + signature
    return {
        "wire": wire,
        "digest": _digest(PXRA_DOMAIN, [wire]),
        "payload_digest": bytes(32) if not payload else _digest(PXRA_PAYLOAD_DOMAIN, [payload]),
        "transcript": transcript,
        "signature": signature,
    }


def _resign_pxra(wire: bytes) -> bytes:
    nonce_length = struct.unpack_from(">H", wire, 300)[0]
    signature_length_offset = 302 + nonce_length
    assert wire[signature_length_offset : signature_length_offset + 2] == _u16(64)
    transcript = PXRA_SIGNING_MAGIC + _u16(1) + wire[6:signature_length_offset]
    transcript += wire[signature_length_offset + 2 : -64]
    signature = _private(CONTROLLER_SEED).sign(transcript)
    Ed25519PublicKey.from_public_bytes(_public(CONTROLLER_SEED)).verify(signature, transcript)
    return wire[:-64] + signature


def _parse_pxra(wire: bytes, controller_public: bytes) -> dict[str, Any]:
    cursor = _Cursor(wire)
    if cursor.take(4) != PXRA_MAGIC or cursor.u16() != ACCESS_VERSION:
        raise ContractReject("PXRA magic/version")
    kind = cursor.u16()
    if kind not in {1, 2} or cursor.u16() != 0:
        raise ContractReject("PXRA kind/reserved")
    carrier_length, payload_length = cursor.u16(), cursor.u32()
    values = {
        "request_id": cursor.take(16),
        "carrier_digest": cursor.take(32),
        "target": cursor.take(16),
        "store": cursor.take(32),
        "epoch": cursor.u64(),
        "pxau_digest": cursor.take(32),
        "pxst_digest": cursor.take(32),
        "profile_digest": cursor.take(32),
        "intended_client": cursor.take(16),
        "payload_digest": cursor.take(32),
        "principal": cursor.take(16),
        "key": cursor.take(16),
        "algorithm": cursor.u16(),
        "algorithm_version": cursor.u16(),
    }
    nonce = cursor.take(cursor.u16())
    signature_length_offset = cursor.offset
    if cursor.u16() != 64:
        raise ContractReject("PXRA signature length")
    carrier = _parse_carrier(cursor.take(carrier_length))
    payload = cursor.take(payload_length)
    signature = cursor.take(64)
    cursor.finish()
    if (
        carrier["digest"] != values["carrier_digest"]
        or carrier["target"] != values["target"]
        or values["principal"] != carrier["controller_principal"]
        or values["key"] != carrier["controller_key_ref"]
        or (values["algorithm"], values["algorithm_version"]) != (1, 1)
        or not nonce
        or not values["epoch"]
        or values["pxst_digest"] == bytes(32)
        or values["profile_digest"] == bytes(32)
        or values["intended_client"]
        in {
            bytes(16),
            carrier["controller_principal"],
            carrier["runtime_principal"],
        }
    ):
        raise ContractReject("PXRA semantic")
    if kind == 1:
        inner = _parse_pxar(payload)
        if (
            values["pxau_digest"] != bytes(32)
            or values["request_id"] != inner["envelope"][24]
            or values["store"] != inner["envelope"][32]
            or values["profile_digest"] != inner["execution"]["profile"]["digest"]
            or values["intended_client"] != inner["execution"]["profile"]["mac_principal"]
        ):
            raise ContractReject("PXRA apply correlation")
    elif payload or values["pxau_digest"] == bytes(32):
        raise ContractReject("PXRA describe shape")
    expected_payload = bytes(32) if not payload else _digest(PXRA_PAYLOAD_DOMAIN, [payload])
    if values["payload_digest"] != expected_payload:
        raise ContractReject("PXRA payload digest")
    transcript = PXRA_SIGNING_MAGIC + _u16(1) + wire[6:signature_length_offset]
    transcript += carrier["wire"] + payload
    try:
        Ed25519PublicKey.from_public_bytes(controller_public).verify(signature, transcript)
    except (InvalidSignature, ValueError) as error:
        raise ContractReject("PXRA signature") from error
    return {
        **values,
        "kind": kind,
        "nonce": nonce,
        "carrier": carrier,
        "payload": payload,
        "digest": _digest(PXRA_DOMAIN, [wire]),
    }


def _response_payload_digest(kind: int, payload: bytes, profile_length: int) -> bytes:
    if kind == 1:
        return _digest(PXRR_PAYLOAD_DOMAIN, [payload])
    return _digest(PXRR_PAYLOAD_DOMAIN, [payload[:profile_length], payload[profile_length:]])


def _response_base(
    magic: bytes,
    request: dict[str, Any],
    payload: bytes,
    profile_length: int,
    descriptor_length: int,
    descriptor_digest: bytes,
    generations: tuple[int, int, int],
) -> bytes:
    carrier = request["carrier"]
    return b"".join(
        [
            magic,
            _u16(ACCESS_SIGNING_VERSION if magic == PXRR_SIGNING_MAGIC else ACCESS_VERSION),
            _u16(request["kind"]),
            _u16(0),
            _u16(len(carrier["wire"])),
            _u32(len(payload)),
            _u16(profile_length),
            _u32(descriptor_length),
            _u16(len(request["nonce"])),
            request["request_id"],
            request["digest"],
            carrier["digest"],
            request["target"],
            request["store"],
            _u64(request["epoch"]),
            request["pxau_digest"],
            request["pxst_digest"],
            request["profile_digest"],
            request["intended_client"],
            _response_payload_digest(request["kind"], payload, profile_length),
            descriptor_digest,
            *[_u64(value) for value in generations],
            carrier["runtime_principal"],
            carrier["runtime_key_ref"],
            _u16(1),
            _u16(1),
            carrier["digest"],
        ]
    )


def _encode_pxrr(
    request_wire: bytes,
    controller_public: bytes,
    payload: bytes,
    *,
    profile_length: int = 0,
    descriptor_length: int = 0,
    descriptor_digest: bytes = bytes(32),
    generations: tuple[int, int, int] = (0, 0, 0),
) -> dict[str, bytes]:
    request = _parse_pxra(request_wire, controller_public)
    args = (request, payload, profile_length, descriptor_length, descriptor_digest, generations)
    transcript_base = _response_base(PXRR_SIGNING_MAGIC, *args)
    values = request["nonce"] + request["carrier"]["wire"] + payload
    transcript = transcript_base + values
    signature = _private(OUTER_RUNTIME_SEED).sign(transcript)
    wire = _response_base(PXRR_MAGIC, *args) + _u16(64) + values + signature
    return {
        "wire": wire,
        "digest": _digest(PXRR_DOMAIN, [wire]),
        "payload_digest": _response_payload_digest(request["kind"], payload, profile_length),
        "transcript": transcript,
        "signature": signature,
    }


def _resign_pxrr(wire: bytes) -> bytes:
    signature_length_offset = 428
    assert wire[signature_length_offset : signature_length_offset + 2] == _u16(64)
    transcript = PXRR_SIGNING_MAGIC + _u16(1) + wire[6:signature_length_offset]
    transcript += wire[signature_length_offset + 2 : -64]
    signature = _private(OUTER_RUNTIME_SEED).sign(transcript)
    Ed25519PublicKey.from_public_bytes(_public(OUTER_RUNTIME_SEED)).verify(signature, transcript)
    return wire[:-64] + signature


def _parse_pxrr(
    wire: bytes,
    request_wire: bytes,
    controller_public: bytes,
    runtime_public: bytes,
) -> dict[str, Any]:
    request = _parse_pxra(request_wire, controller_public)
    cursor = _Cursor(wire)
    if cursor.take(4) != PXRR_MAGIC or cursor.u16() != ACCESS_VERSION:
        raise ContractReject("PXRR magic/version")
    kind = cursor.u16()
    if kind not in {1, 2} or cursor.u16() != 0:
        raise ContractReject("PXRR kind/reserved")
    carrier_length, payload_length = cursor.u16(), cursor.u32()
    profile_length, descriptor_length, nonce_length = cursor.u16(), cursor.u32(), cursor.u16()
    values = {
        "request_id": cursor.take(16),
        "request_digest": cursor.take(32),
        "carrier_digest": cursor.take(32),
        "target": cursor.take(16),
        "store": cursor.take(32),
        "epoch": cursor.u64(),
        "pxau_digest": cursor.take(32),
        "pxst_digest": cursor.take(32),
        "profile_digest": cursor.take(32),
        "intended_client": cursor.take(16),
        "payload_digest": cursor.take(32),
        "descriptor_digest": cursor.take(32),
        "fabric_generation": cursor.u64(),
        "agent_generation": cursor.u64(),
        "access_generation": cursor.u64(),
        "principal": cursor.take(16),
        "key": cursor.take(16),
        "algorithm": cursor.u16(),
        "algorithm_version": cursor.u16(),
        "claim_carrier_digest": cursor.take(32),
    }
    signature_length_offset = cursor.offset
    if cursor.u16() != 64:
        raise ContractReject("PXRR signature length")
    nonce = cursor.take(nonce_length)
    carrier = _parse_carrier(cursor.take(carrier_length))
    payload = cursor.take(payload_length)
    signature = cursor.take(64)
    cursor.finish()
    correlated = {
        "request_id": request["request_id"],
        "request_digest": request["digest"],
        "target": request["target"],
        "store": request["store"],
        "epoch": request["epoch"],
        "pxau_digest": request["pxau_digest"],
        "pxst_digest": request["pxst_digest"],
        "profile_digest": request["profile_digest"],
        "intended_client": request["intended_client"],
    }
    if any(values[key] != value for key, value in correlated.items()):
        raise ContractReject("PXRR request correlation")
    if (
        kind != request["kind"]
        or nonce != request["nonce"]
        or carrier["wire"] != request["carrier"]["wire"]
        or values["carrier_digest"] != carrier["digest"]
        or values["claim_carrier_digest"] != carrier["digest"]
        or values["principal"] != carrier["runtime_principal"]
        or values["key"] != carrier["runtime_key_ref"]
        or (values["algorithm"], values["algorithm_version"]) != (1, 1)
        or values["payload_digest"] != _response_payload_digest(kind, payload, profile_length)
    ):
        raise ContractReject("PXRR semantic")
    if kind == 1:
        if (
            profile_length
            or descriptor_length
            or any(
                values[key]
                for key in ("fabric_generation", "agent_generation", "access_generation")
            )
        ):
            raise ContractReject("PXRR apply shape")
        inner = _parse_pxau(payload, request["payload"], _public(INNER_RUNTIME_SEED))
        if (
            inner["evidence"]["epoch"] != values["epoch"]
            or inner["auth"]["principal"] != carrier["runtime_principal"]
        ):
            raise ContractReject("PXRR inner correlation")
    else:
        if payload_length != profile_length + descriptor_length or descriptor_length < 6:
            raise ContractReject("PXRR describe lengths")
        profile = _parse_profile(payload[:profile_length])
        descriptor = payload[profile_length:]
        if (
            descriptor[:6] != b"PXAP\0\x01"
            or values["descriptor_digest"] != _digest(PXAP_SHARED_DOMAIN, [descriptor])
            or profile["digest"] != values["profile_digest"]
            or profile["mac_principal"] != values["intended_client"]
            or not all(
                values[key]
                for key in ("fabric_generation", "agent_generation", "access_generation")
            )
        ):
            raise ContractReject("PXRR describe payload")
    transcript = PXRR_SIGNING_MAGIC + _u16(1) + wire[6:signature_length_offset]
    transcript += nonce + carrier["wire"] + payload
    try:
        Ed25519PublicKey.from_public_bytes(runtime_public).verify(signature, transcript)
    except (InvalidSignature, ValueError) as error:
        raise ContractReject("PXRR signature") from error
    return {**values, "kind": kind, "payload": payload, "signature": signature}


def _vectors() -> dict[str, Any]:
    predecessor = S7._build_vectors()
    profile = _profile()
    bootstrap_pxap = b"PXAP\0\x01t2-bootstrap-descriptor"
    fresh_pxap = b"PXAP\0\x01t2-fresh-descriptor"
    bootstrap_pxah = b"PXAH\0\x01t2-whole-bootstrap-receipt"
    bootstrap = {
        "active_pxft_digest": bytes.fromhex("91" * 32),
        "active_pxst_digest": predecessor["active"]["terminal"]["receipt_digest"],
        "descriptor_receipt_digest": _digest(PXAH_WHOLE_DOMAIN, [bootstrap_pxah]),
        "descriptor_payload_digest": _digest(PXAP_SHARED_DOMAIN, [bootstrap_pxap]),
        "fresh_descriptor_payload_digest": _digest(PXAP_SHARED_DOMAIN, [fresh_pxap]),
        "fabric_generation": 7,
        "agent_generation": 8,
        "runtime_principal": bytes.fromhex("88" * 16),
        "inner_runtime_key_ref": bytes.fromhex("8d" * 16),
    }
    projection = _projection(predecessor["projection"])
    pxte = _encode_pxte(projection, predecessor["active"]["pxte"], profile["wire"], bootstrap)
    envelope = S7.FABRIC.LEGACY._build_envelope(
        _assignment_digest(pxte),
        source_revision=140,
        operation_byte="92",
        temporal_byte="93",
        auth_nonce=b"t2-inner-data-plane-apply",
    )
    pxar = _encode_pxar(envelope["wire"], pxte)
    pxau = _encode_pxau(pxar, bootstrap)
    controller_public = _public(CONTROLLER_SEED)
    assert controller_public == envelope["request_public_key"]
    outer_runtime_public = _public(OUTER_RUNTIME_SEED)
    carrier = _carrier(profile["target"], controller_public, outer_runtime_public)
    common = {
        "carrier": carrier,
        "store": envelope["wire"] and bytes.fromhex("44" * 32),
        "epoch": 23,
        "pxst_digest": bootstrap["active_pxst_digest"],
        "profile_digest": profile["digest"],
        "intended_client": profile["mac_principal"],
    }
    apply_request = _encode_pxra(
        kind=1,
        request_id=bytes.fromhex("92" * 16),
        pxau_digest=bytes(32),
        payload=pxar,
        nonce=b"t2-outer-apply-request",
        **common,
    )
    describe_request = _encode_pxra(
        kind=2,
        request_id=bytes.fromhex("94" * 16),
        pxau_digest=pxau["digest"],
        payload=b"",
        nonce=b"t2-outer-describe-request",
        **common,
    )
    apply_response = _encode_pxrr(apply_request["wire"], controller_public, pxau["wire"])
    describe_payload = profile["wire"] + fresh_pxap
    describe_response = _encode_pxrr(
        describe_request["wire"],
        controller_public,
        describe_payload,
        profile_length=len(profile["wire"]),
        descriptor_length=len(fresh_pxap),
        descriptor_digest=bootstrap["fresh_descriptor_payload_digest"],
        generations=(9, 10, 11),
    )
    return {
        "profile": profile,
        "projection": projection,
        "pxte": pxte,
        "envelope": envelope,
        "pxar": pxar,
        "pxau": pxau,
        "bootstrap": bootstrap,
        "bootstrap_pxap": bootstrap_pxap,
        "fresh_pxap": fresh_pxap,
        "bootstrap_pxah": bootstrap_pxah,
        "carrier": carrier,
        "controller_public": controller_public,
        "outer_runtime_public": outer_runtime_public,
        "apply_request": apply_request,
        "describe_request": describe_request,
        "apply_response": apply_response,
        "describe_response": describe_response,
    }


def _generated_data_plane_fixture() -> dict[str, Any]:
    value = _vectors()
    return {
        "format": "paraegox-t2-remote-agent-data-plane-v1",
        "source": "independent Python struct/hashlib/cryptography T2 oracle",
        "frozen_contract_sha256": FROZEN_CONTRACTS,
        "data_plane": {
            "pxad_hex": value["profile"]["wire"].hex(),
            "pxad_digest_hex": value["profile"]["digest"].hex(),
            "pxae_hex": value["projection"].hex(),
            "compatibility_digest_hex": _compatibility_digest().hex(),
            "pxte_v9_hex": value["pxte"].hex(),
            "pxte_v9_digest_hex": _digest(PXTE_DOMAIN, [value["pxte"]]).hex(),
            "assignment_v10_digest_hex": _assignment_digest(value["pxte"]).hex(),
            "envelope_v2_hex": value["envelope"]["wire"].hex(),
            "inner_apply_transcript_hex": value["envelope"]["signing_transcript"].hex(),
            "inner_apply_signature_hex": value["envelope"]["request_signature"].hex(),
            "inner_apply_public_key_hex": value["envelope"]["request_public_key"].hex(),
            "pxar_v10_hex": value["pxar"].hex(),
            "pxar_v10_digest_hex": _digest(PXAR_DOMAIN, [value["pxar"]]).hex(),
            "pxau_hex": value["pxau"]["wire"].hex(),
            "pxau_digest_hex": value["pxau"]["digest"].hex(),
            "pxau_transcript_hex": value["pxau"]["transcript"].hex(),
            "pxau_signature_hex": value["pxau"]["signature"].hex(),
            "pxau_public_key_hex": value["pxau"]["public_key"].hex(),
            "bootstrap_pxap_hex": value["bootstrap_pxap"].hex(),
            "bootstrap_pxap_shared_digest_hex": value["bootstrap"][
                "descriptor_payload_digest"
            ].hex(),
            "fresh_pxap_hex": value["fresh_pxap"].hex(),
            "fresh_pxap_shared_digest_hex": value["bootstrap"][
                "fresh_descriptor_payload_digest"
            ].hex(),
            "bootstrap_pxah_hex": value["bootstrap_pxah"].hex(),
            "bootstrap_pxah_whole_digest_hex": value["bootstrap"][
                "descriptor_receipt_digest"
            ].hex(),
            "pxap_shared_digest_domain_hex": PXAP_SHARED_DOMAIN.hex(),
            "pxah_whole_digest_domain_hex": PXAH_WHOLE_DOMAIN.hex(),
        },
    }


def _access_entry(value: dict[str, bytes]) -> dict[str, Any]:
    return {
        "wire_hex": value["wire"].hex(),
        "wire_length": len(value["wire"]),
        "digest_hex": value["digest"].hex(),
        "payload_digest_hex": value["payload_digest"].hex(),
        "transcript_hex": value["transcript"].hex(),
        "transcript_sha256_hex": hashlib.sha256(value["transcript"]).hexdigest(),
        "signature_hex": value["signature"].hex(),
    }


def _generated_access_fixture() -> dict[str, Any]:
    value = _vectors()
    return {
        "format": "paraegox-t2-remote-agent-access-v1",
        "source": "independent Python struct/hashlib/cryptography T2 oracle",
        "frozen_contract_sha256": FROZEN_CONTRACTS,
        "access": {
            "pxcb_hex": value["carrier"]["wire"].hex(),
            "pxcb_digest_hex": value["carrier"]["digest"].hex(),
            "controller_public_key_hex": value["controller_public"].hex(),
            "runtime_public_key_hex": value["outer_runtime_public"].hex(),
            "pxra_apply_hex": value["apply_request"]["wire"].hex(),
            "pxra_apply": _access_entry(value["apply_request"]),
            "pxra_describe_hex": value["describe_request"]["wire"].hex(),
            "pxra_describe": _access_entry(value["describe_request"]),
            "pxrr_apply_hex": value["apply_response"]["wire"].hex(),
            "pxrr_apply": _access_entry(value["apply_response"]),
            "pxrr_describe_hex": value["describe_response"]["wire"].hex(),
            "pxrr_describe": _access_entry(value["describe_response"]),
        },
    }


def test_independent_python_oracle_matches_checked_in_golden() -> None:
    assert json.loads(DATA_PLANE_FIXTURE.read_text()) == _generated_data_plane_fixture()
    assert json.loads(ACCESS_FIXTURE.read_text()) == _generated_access_fixture()


def test_data_plane_golden_wire_digest_and_terminal_signature() -> None:
    value = _vectors()
    assert _parse_profile(value["profile"]["wire"])["digest"] == value["profile"]["digest"]
    request = _parse_pxar(value["pxar"])
    assert request["request_digest"] == _digest(PXAR_DOMAIN, [value["pxar"]])
    _parse_pxau(value["pxau"]["wire"], value["pxar"], value["pxau"]["public_key"])


def test_access_apply_and_describe_golden_with_nested_and_outer_signatures() -> None:
    value = _vectors()
    _parse_pxrr(
        value["apply_response"]["wire"],
        value["apply_request"]["wire"],
        value["controller_public"],
        value["outer_runtime_public"],
    )
    _parse_pxrr(
        value["describe_response"]["wire"],
        value["describe_request"]["wire"],
        value["controller_public"],
        value["outer_runtime_public"],
    )
    _parse_pxau(value["pxau"]["wire"], value["pxar"], value["pxau"]["public_key"])


def test_access_domains_keep_pxap_shared_and_pxah_independent() -> None:
    value = _vectors()
    assert (
        _digest(PXAP_SHARED_DOMAIN, [value["bootstrap_pxap"]])
        == value["bootstrap"]["descriptor_payload_digest"]
    )
    assert (
        _digest(PXAH_WHOLE_DOMAIN, [value["bootstrap_pxah"]])
        == value["bootstrap"]["descriptor_receipt_digest"]
    )
    assert (
        _digest(PXAP_SHARED_DOMAIN, [value["bootstrap_pxah"]])
        != value["bootstrap"]["descriptor_receipt_digest"]
    )


@pytest.mark.parametrize("offset", [0, 4])
def test_cross_magic_and_version_are_rejected(offset: int) -> None:
    value = _vectors()
    for parser, wire in (
        (_parse_profile, value["profile"]["wire"]),
        (_parse_pxar, value["pxar"]),
    ):
        changed = bytearray(wire)
        changed[offset] ^= 1
        with pytest.raises(ContractReject):
            parser(bytes(changed))


def test_inner_and_outer_signature_tamper_is_rejected() -> None:
    value = _vectors()
    inner = bytearray(value["pxau"]["wire"])
    inner[-1] ^= 1
    with pytest.raises(ContractReject, match="signature"):
        _parse_pxau(bytes(inner), value["pxar"], value["pxau"]["public_key"])
    outer = bytearray(value["describe_response"]["wire"])
    outer[-1] ^= 1
    with pytest.raises(ContractReject, match="signature"):
        _parse_pxrr(
            bytes(outer),
            value["describe_request"]["wire"],
            value["controller_public"],
            value["outer_runtime_public"],
        )


def test_epoch_and_request_correlation_tamper_is_rejected() -> None:
    value = _vectors()
    response = bytearray(value["describe_response"]["wire"])
    response[152:160] = _u64(24)
    response = bytearray(_resign_pxrr(bytes(response)))
    with pytest.raises(ContractReject, match="correlation"):
        _parse_pxrr(
            bytes(response),
            value["describe_request"]["wire"],
            value["controller_public"],
            value["outer_runtime_public"],
        )


def test_temporal_domain_generation_and_observed_time_fail_closed() -> None:
    value = _vectors()
    wire = value["pxau"]["wire"]
    for offset, replacement in ((475, bytes(16)), (491, _u64(0)), (499, _u64(0))):
        changed = bytearray(wire)
        changed[offset : offset + len(replacement)] = replacement
        changed = bytearray(_resign_pxau(bytes(changed)))
        with pytest.raises(ContractReject, match="semantic"):
            _parse_pxau(bytes(changed), value["pxar"], value["pxau"]["public_key"])


def test_active_ready_census_and_principal_invariants_fail_closed() -> None:
    value = _vectors()
    receipt = bytearray(value["pxau"]["wire"])
    receipt[323:325] = _u16(1)
    receipt = bytearray(_resign_pxau(bytes(receipt)))
    with pytest.raises(ContractReject, match="semantic"):
        _parse_pxau(bytes(receipt), value["pxar"], value["pxau"]["public_key"])
    request = bytearray(value["describe_request"]["wire"])
    request[264:280] = value["carrier"]["runtime_principal"]
    request = bytearray(_resign_pxra(bytes(request)))
    with pytest.raises(ContractReject, match="semantic"):
        _parse_pxra(bytes(request), value["controller_public"])


def test_payload_digest_tamper_and_cross_protocol_frames_fail_closed() -> None:
    value = _vectors()
    changed = bytearray(value["describe_response"]["wire"])
    changed[272] ^= 1
    changed = bytearray(_resign_pxrr(bytes(changed)))
    with pytest.raises(ContractReject, match="semantic"):
        _parse_pxrr(
            bytes(changed),
            value["describe_request"]["wire"],
            value["controller_public"],
            value["outer_runtime_public"],
        )
    with pytest.raises(ContractReject):
        _parse_pxra(value["pxar"], value["controller_public"])
    with pytest.raises(ContractReject):
        _parse_pxrr(
            value["pxau"]["wire"],
            value["apply_request"]["wire"],
            value["controller_public"],
            value["outer_runtime_public"],
        )
