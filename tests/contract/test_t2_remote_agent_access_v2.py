"""Independent Python oracle for the additive T2 remote-Agent access v2 wires."""

from __future__ import annotations

import hashlib
import json
import struct
from collections.abc import Callable
from functools import lru_cache
from pathlib import Path
from typing import Any

import pytest
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = REPO_ROOT / "tests/fixtures/wire/t2_remote_agent_access_v2.json"
INNER_FIXTURE_PATH = REPO_ROOT / "tests/fixtures/wire/t2_remote_agent_proxy_data_plane_v2.json"
V1_FIXTURE_PATH = REPO_ROOT / "tests/fixtures/wire/t2_remote_agent_access_v1.json"
OUTER_SOURCE_PATH = REPO_ROOT / "crates/paraegox-runtime-contracts/src/remote_agent_access.rs"
PLAN_SOURCE_PATH = (
    REPO_ROOT / "crates/paraegox-runtime-contracts/src/remote_agent_data_plane_plan.rs"
)

R236_REF = "build/mac-source-snapshot-20260809-r236-t2-b2-outer-successor-high-fixes-fmt"
R236_COMMIT = "786d96cded7ca7efbe248fe464a69c26fc6dcbb7"
OUTER_SOURCE_SHA256 = "ee42b276ad30d9fa2c9240af49315db8fd6499a4c45a8a2c9107cdab44a58be7"
PLAN_SOURCE_SHA256 = "8597f75ec6bb6b41a97fb3ebd2d8cdefcde431ceb0d32d6b125ed17f0a1a64e7"
INNER_FIXTURE_SHA256 = "983e4449636dd559e9ca0508b756f186ecc34a47ff8a2c5f6c38f3941a31499a"

DIGEST_MAGIC = b"ParaEGOX\0canonical-digest"
DIGEST_VERSION = 1
SIGNING_MAGIC = b"ParaEGOX\0canonical-signing-transcript"

PXRA_MAGIC = b"PXRA"
PXRR_MAGIC = b"PXRR"
ACCESS_VERSION = 2
ACCESS_SIGNING_VERSION = 2
PXRA_FIXED_BYTES = 544
PXRR_FIXED_BYTES = 646
MAX_PXRA_BYTES = 7_855
MAX_PXRR_BYTES = 5_792
MAX_NONCE_BYTES = 64
SIGNATURE_BYTES = 64
MAX_DESCRIPTOR_BYTES = 2_048
MAX_CANONICAL_PXAU_BYTES = 1_195

PXRA_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-access-request-signing-v2"
PXRR_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-access-response-signing-v2"
PXRA_PAYLOAD_DOMAIN = b"paraegox.runtime.remote-agent-access.request-payload.sha256.v2"
PXRR_PAYLOAD_DOMAIN = b"paraegox.runtime.remote-agent-access.response-payload.sha256.v2"
PXRA_DOMAIN = b"paraegox.runtime.remote-agent-access.request.sha256.v2"
PXRR_DOMAIN = b"paraegox.runtime.remote-agent-access.response.sha256.v2"

PXCB_MAGIC = b"PXCB"
PXCB_VERSION = 1
PXCB_KIND = 1
PXCB_FIXED_BYTES = 228
MAX_PXCB_BYTES = 483
MAX_ROUTE_BYTES = 255
PXCB_DOMAIN = b"paraegox.runtime.restricted-apply-carrier-binding.sha256.v1"
CONTROL_KEY_DOMAIN = b"paraegox.runtime.control-auth.ed25519-public-key.sha256.v1"

PXAR_MAGIC = b"PXAR"
PXAR_VERSION = 11
PXAR_HEADER_BYTES = 18
MAX_PXAR_BYTES = 6_700
PXTA_ZERO = b"PXTA\0\x01\0\0\0\0"
PXTE_MAGIC = b"PXTE"
PXTE_VERSION = 10
PXTE_PREFIX_BYTES = 320
MAX_PXTE_BYTES = 2_576
RETAINED_S0_BYTES = 200
ACTIVE_S1_BYTES = 152
PXAD_MAGIC = b"PXAD"
PXAD_VERSION = 1
PXAD_KIND = 1
PXAD_ACL_VERSION = 1
PXAD_FIXED_BYTES = 158
MAX_PXAD_BYTES = 439

INNER_PXAR_DOMAIN = b"paraegox.runtime.remote-agent-proxy-data-plane-request.sha256.v2"
INNER_PXAU_DOMAIN = b"paraegox.runtime.remote-agent-proxy-data-plane-terminal.sha256.v2"
INNER_PXAU_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-proxy-data-plane-terminal-signing"
INNER_PXAU_VERSION = 2
INNER_PXAU_FIXED_BYTES = 683
MAX_INNER_PXAU_BYTES = 2_048
ENVELOPE_MAGIC = b"ParaEGOX\0runtime-apply-envelope"
ENVELOPE_VERSION = 2
ENVELOPE_FIELD_COUNT = 38
ENVELOPE_AUTH_DOMAIN = b"paraegox.runtime.apply-envelope-auth.signing.v2"
PXAP_DOMAIN = b"paraegox.runtime.agent-control.receipt-payload.sha256.v1"

REQUEST_OFFSETS = {
    "magic": 0,
    "version": 4,
    "kind": 6,
    "reserved": 8,
    "carrier_length": 10,
    "payload_length": 12,
    "request_id": 16,
    "carrier_digest": 32,
    "target": 64,
    "store": 80,
    "epoch": 112,
    "retained_s0_cas": 120,
    "expected_s1_cas": 320,
    "payload_digest": 472,
    "principal": 504,
    "key": 520,
    "algorithm": 536,
    "algorithm_version": 538,
    "nonce_length": 540,
    "nonce": 542,
}
RESPONSE_OFFSETS = {
    "magic": 0,
    "version": 4,
    "kind": 6,
    "reserved": 8,
    "carrier_length": 10,
    "payload_length": 12,
    "profile_length": 16,
    "descriptor_length": 18,
    "nonce_length": 22,
    "request_id": 24,
    "request_digest": 40,
    "carrier_digest": 72,
    "target": 104,
    "store": 120,
    "epoch": 152,
    "retained_s0_cas": 160,
    "expected_s1_cas": 360,
    "payload_digest": 512,
    "descriptor_digest": 544,
    "principal": 576,
    "key": 592,
    "algorithm": 608,
    "algorithm_version": 610,
    "claim_carrier_digest": 612,
    "signature_length": 644,
    "values": 646,
}

CONTROLLER_SEED = bytes.fromhex("22" * 32)
RUNTIME_SEED = bytes.fromhex("43" * 32)
CONTROLLER_PRINCIPAL = bytes.fromhex("09" * 16)
RUNTIME_PRINCIPAL = bytes.fromhex("88" * 16)
CONTROLLER_KEY_REF = bytes.fromhex("0c" * 16)
RUNTIME_KEY_REF = bytes.fromhex("8d" * 16)
RUNTIME_HOST_EPOCH = 23

VerifyInner = Callable[[bytes, bytes, int, int, bytes, bytes], bool]
VerifyOuter = Callable[[bytes, bytes, bytes, bytes, bytes], bool]


class ContractReject(ValueError):
    """The independent strict consumer rejected a noncanonical wire."""


class _Cursor:
    def __init__(self, wire: bytes) -> None:
        self.wire = wire
        self.offset = 0

    def take(self, size: int) -> bytes:
        end = self.offset + size
        if size < 0 or end > len(self.wire):
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
    return _digest(CONTROL_KEY_DOMAIN, [public_key])


def _replace(wire: bytes, offset: int, value: bytes) -> bytes:
    return wire[:offset] + value + wire[offset + len(value) :]


def _read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text())
    assert isinstance(value, dict)
    return value


@lru_cache(maxsize=1)
def _inner_fixture() -> dict[str, Any]:
    return _read_json(INNER_FIXTURE_PATH)


def _parse_retained_s0(wire: bytes) -> dict[str, Any]:
    if len(wire) != RETAINED_S0_BYTES:
        raise ContractReject("retained S0 CAS length")
    cursor = _Cursor(wire)
    parsed = {
        "active_pxft_digest": cursor.take(32),
        "active_pxst_digest": cursor.take(32),
        "descriptor_record_digest": cursor.take(32),
        "descriptor_record_sequence": cursor.u64(),
        "descriptor_receipt_digest": cursor.take(32),
        "descriptor_payload_digest": cursor.take(32),
        "fabric_session_epoch": cursor.take(16),
        "fabric_generation": cursor.u64(),
        "agent_generation": cursor.u64(),
    }
    cursor.finish()
    byte_values = [value for value in parsed.values() if isinstance(value, bytes)]
    integer_values = [value for value in parsed.values() if isinstance(value, int)]
    if any(value == bytes(len(value)) for value in byte_values) or any(
        value == 0 for value in integer_values
    ):
        raise ContractReject("retained S0 CAS semantic")
    return {**parsed, "wire": wire}


def _parse_active_s1(wire: bytes) -> dict[str, Any]:
    if len(wire) != ACTIVE_S1_BYTES:
        raise ContractReject("active S1 CAS length")
    cursor = _Cursor(wire)
    present = cursor.u8()
    if cursor.take(7) != bytes(7):
        raise ContractReject("active S1 CAS reserved")
    parsed = {
        "present": present,
        "access_generation_high_water": cursor.u64(),
        "owner_slot_revision": cursor.u64(),
        "active_pxau_digest": cursor.take(32),
        "active_request_digest": cursor.take(32),
        "active_snapshot_digest": cursor.take(32),
        "active_snapshot_sequence": cursor.u64(),
        "active_access_generation": cursor.u64(),
        "active_proxy_session_epoch": cursor.take(16),
    }
    cursor.finish()
    if parsed["owner_slot_revision"] == 0:
        raise ContractReject("active S1 owner revision")
    tuple_values = [
        parsed["active_pxau_digest"],
        parsed["active_request_digest"],
        parsed["active_snapshot_digest"],
        _u64(parsed["active_snapshot_sequence"]),
        _u64(parsed["active_access_generation"]),
        parsed["active_proxy_session_epoch"],
    ]
    if present == 0:
        if any(value != bytes(len(value)) for value in tuple_values):
            raise ContractReject("absent S1 active tuple")
    elif present == 1:
        if (
            any(value == bytes(len(value)) for value in tuple_values)
            or parsed["access_generation_high_water"] == 0
            or parsed["active_access_generation"] != parsed["access_generation_high_water"]
        ):
            raise ContractReject("active S1 tuple")
    else:
        raise ContractReject("active S1 presence")
    return {**parsed, "wire": wire}


def _parse_profile(wire: bytes) -> dict[str, Any]:
    if not PXAD_FIXED_BYTES <= len(wire) <= MAX_PXAD_BYTES:
        raise ContractReject("PXAD length")
    cursor = _Cursor(wire)
    if cursor.take(4) != PXAD_MAGIC or cursor.u16() != PXAD_VERSION:
        raise ContractReject("PXAD magic/version")
    if cursor.u16() != PXAD_KIND:
        raise ContractReject("PXAD kind")
    base_length, tls_length = cursor.u16(), cursor.u16()
    if cursor.u16() != PXAD_ACL_VERSION or not base_length or not tls_length:
        raise ContractReject("PXAD header")
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
    if (
        parsed["target"] == bytes(16)
        or parsed["endpoint_generation"] == 0
        or parsed["mac_principal"] in {bytes(16), parsed["ubuntu_principal"]}
        or not 0 < parsed["operation_timeout_nanos"] <= 30_000_000_000
        or parsed["base"] != b"tcp/127.0.0.1:7447"
        or not parsed["tls"].startswith(b"tls/")
    ):
        raise ContractReject("PXAD semantic")
    return {**parsed, "wire": wire}


def _parse_envelope(wire: bytes) -> dict[int, bytes]:
    if len(wire) > 4_096 or not wire.startswith(ENVELOPE_MAGIC):
        raise ContractReject("envelope magic/max")
    cursor = _Cursor(wire)
    cursor.take(len(ENVELOPE_MAGIC))
    if cursor.u16() != ENVELOPE_VERSION or cursor.u16() != ENVELOPE_FIELD_COUNT:
        raise ContractReject("envelope version/count")
    fields: dict[int, bytes] = {}
    for expected_tag in range(1, ENVELOPE_FIELD_COUNT + 1):
        if cursor.u16() != expected_tag:
            raise ContractReject("envelope field order")
        value = cursor.take(cursor.u32())
        if not value:
            raise ContractReject("envelope empty field")
        fields[expected_tag] = value
    cursor.finish()
    if any(fields[tag] == bytes(len(fields[tag])) for tag in (2, 24, 32, 33, 34)):
        raise ContractReject("envelope identity")
    if fields[35] != _u16(1) or fields[36] != _u16(1):
        raise ContractReject("envelope algorithm")
    if not 0 < len(fields[37]) <= MAX_NONCE_BYTES or len(fields[38]) != SIGNATURE_BYTES:
        raise ContractReject("envelope authentication length")
    return fields


def _inner_envelope_transcript(fields: dict[int, bytes]) -> bytes:
    encoded = bytearray(SIGNING_MAGIC)
    encoded += _u16(2) + _u16(len(ENVELOPE_AUTH_DOMAIN)) + ENVELOPE_AUTH_DOMAIN
    encoded += _u16(37)
    for tag in range(1, 38):
        value = fields[tag]
        encoded += _u16(tag) + _u32(len(value)) + value
    return bytes(encoded)


def _parse_inner_pxar(wire: bytes) -> dict[str, Any]:
    if len(wire) > MAX_PXAR_BYTES or len(wire) < PXAR_HEADER_BYTES:
        raise ContractReject("PXAR11 length")
    if wire[:4] != PXAR_MAGIC or struct.unpack_from(">H", wire, 4)[0] != PXAR_VERSION:
        raise ContractReject("PXAR11 magic/version")
    envelope_length, pxta_length, pxte_length = struct.unpack_from(">III", wire, 6)
    if pxta_length != len(PXTA_ZERO) or pxte_length > MAX_PXTE_BYTES:
        raise ContractReject("PXAR11 nested length")
    if PXAR_HEADER_BYTES + envelope_length + pxta_length + pxte_length != len(wire):
        raise ContractReject("PXAR11 declared length")
    envelope_start = PXAR_HEADER_BYTES
    pxta_start = envelope_start + envelope_length
    pxte_start = pxta_start + pxta_length
    envelope_wire = wire[envelope_start:pxta_start]
    if wire[pxta_start:pxte_start] != PXTA_ZERO:
        raise ContractReject("PXAR11 assignments")
    fields = _parse_envelope(envelope_wire)
    pxte = wire[pxte_start:]
    if len(pxte) < PXTE_PREFIX_BYTES + RETAINED_S0_BYTES + ACTIVE_S1_BYTES:
        raise ContractReject("PXTE10 truncated")
    if pxte[:4] != PXTE_MAGIC or struct.unpack_from(">H", pxte, 4)[0] != PXTE_VERSION:
        raise ContractReject("PXTE10 magic/version")
    if struct.unpack_from(">H", pxte, 308)[0] != PXAD_VERSION or pxte[311] != 1:
        raise ContractReject("PXTE10 profile")
    predecessor_length, profile_length = struct.unpack_from(">II", pxte, 312)
    retained_offset = PXTE_PREFIX_BYTES + predecessor_length
    expected_s1_offset = retained_offset + RETAINED_S0_BYTES
    profile_offset = expected_s1_offset + ACTIVE_S1_BYTES
    if profile_offset + profile_length != len(pxte):
        raise ContractReject("PXTE10 declared length")
    retained = _parse_retained_s0(pxte[retained_offset:expected_s1_offset])
    expected_s1 = _parse_active_s1(pxte[expected_s1_offset:profile_offset])
    profile = _parse_profile(pxte[profile_offset:])
    if fields[2] != profile["target"]:
        raise ContractReject("PXAR11 target")
    transcript = _inner_envelope_transcript(fields)
    try:
        Ed25519PublicKey.from_public_bytes(_public(CONTROLLER_SEED)).verify(fields[38], transcript)
    except (InvalidSignature, ValueError) as error:
        raise ContractReject("inner Controller signature") from error
    return {
        "wire": wire,
        "digest": _digest(INNER_PXAR_DOMAIN, [wire]),
        "envelope_wire": envelope_wire,
        "fields": fields,
        "transcript": transcript,
        "signature": fields[38],
        "target": fields[2],
        "operation_id": fields[24],
        "store": fields[32],
        "principal": fields[33],
        "key": fields[34],
        "algorithm": int.from_bytes(fields[35], "big"),
        "algorithm_version": int.from_bytes(fields[36], "big"),
        "nonce": fields[37],
        "retained_s0": retained,
        "expected_s1": expected_s1,
        "profile": profile,
    }


def _parse_inner_pxau(wire: bytes, request: dict[str, Any]) -> dict[str, Any]:
    if len(wire) > MAX_INNER_PXAU_BYTES or len(wire) < INNER_PXAU_FIXED_BYTES:
        raise ContractReject("PXAU2 length")
    if wire[:4] != b"PXAU" or struct.unpack_from(">H", wire, 4)[0] != INNER_PXAU_VERSION:
        raise ContractReject("PXAU2 magic/version")
    signature_length = struct.unpack_from(">H", wire, 681)[0]
    if signature_length != SIGNATURE_BYTES or 683 + signature_length != len(wire):
        raise ContractReject("PXAU2 signature length")
    values = {
        "target": wire[6:22],
        "store": wire[22:54],
        "operation_id": wire[70:86],
        "request_digest": wire[118:150],
        "completion_runtime_host_epoch": struct.unpack_from(">Q", wire, 563)[0],
        "runtime_principal": wire[645:661],
        "runtime_key": wire[661:677],
        "algorithm": struct.unpack_from(">H", wire, 677)[0],
        "algorithm_version": struct.unpack_from(">H", wire, 679)[0],
    }
    if (
        values["target"] != request["target"]
        or values["store"] != request["store"]
        or values["operation_id"] != request["operation_id"]
        or values["request_digest"] != request["digest"]
        or values["completion_runtime_host_epoch"] == 0
        or values["runtime_principal"] == bytes(16)
        or values["runtime_key"] == bytes(16)
        or (values["algorithm"], values["algorithm_version"]) != (1, 1)
    ):
        raise ContractReject("PXAU2 correlation/authentication")
    body = wire[6:681]
    transcript = INNER_PXAU_SIGNING_MAGIC + _u16(2) + body
    signature = wire[683:]
    try:
        Ed25519PublicKey.from_public_bytes(_public(RUNTIME_SEED)).verify(signature, transcript)
    except (InvalidSignature, ValueError) as error:
        raise ContractReject("inner Runtime signature") from error
    return {
        **values,
        "wire": wire,
        "digest": _digest(INNER_PXAU_DOMAIN, [wire]),
        "transcript": transcript,
        "signature": signature,
    }


def _carrier(target: bytes) -> dict[str, Any]:
    route = b"paraegox/runtime/t2/remote-agent-access/v2"
    wire = b"".join(
        [
            PXCB_MAGIC,
            _u16(PXCB_VERSION),
            _u16(PXCB_KIND),
            _u16(len(route)),
            _u16(0),
            target,
            RUNTIME_PRINCIPAL,
            CONTROLLER_PRINCIPAL,
            bytes.fromhex("89" * 16),
            _u64(23),
            CONTROLLER_KEY_REF,
            _fingerprint(_public(CONTROLLER_SEED)),
            RUNTIME_KEY_REF,
            _fingerprint(_public(RUNTIME_SEED)),
            bytes.fromhex("8b" * 16),
            bytes.fromhex("8c" * 32),
            route,
        ]
    )
    return _parse_carrier(wire)


def _parse_carrier(wire: bytes) -> dict[str, Any]:
    if not PXCB_FIXED_BYTES < len(wire) <= MAX_PXCB_BYTES:
        raise ContractReject("PXCB length")
    cursor = _Cursor(wire)
    if cursor.take(4) != PXCB_MAGIC or (cursor.u16(), cursor.u16()) != (1, 1):
        raise ContractReject("PXCB magic/version")
    route_length = cursor.u16()
    if cursor.u16() != 0 or not 0 < route_length <= MAX_ROUTE_BYTES:
        raise ContractReject("PXCB header")
    parsed = {
        "target": cursor.take(16),
        "runtime_principal": cursor.take(16),
        "controller_principal": cursor.take(16),
        "endpoint_ref": cursor.take(16),
        "endpoint_generation": cursor.u64(),
        "controller_key_ref": cursor.take(16),
        "controller_fingerprint": cursor.take(32),
        "runtime_key_ref": cursor.take(16),
        "runtime_fingerprint": cursor.take(32),
        "transport_ref": cursor.take(16),
        "transport_digest": cursor.take(32),
    }
    route = cursor.take(route_length)
    cursor.finish()
    try:
        route_text = route.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractReject("PXCB route") from error
    bytes_values = [value for value in parsed.values() if isinstance(value, bytes)]
    if (
        any(value == bytes(len(value)) for value in bytes_values)
        or parsed["endpoint_generation"] == 0
        or parsed["controller_key_ref"] == parsed["runtime_key_ref"]
        or parsed["controller_fingerprint"] == parsed["runtime_fingerprint"]
        or not route_text
    ):
        raise ContractReject("PXCB semantic")
    return {**parsed, "route": route, "wire": wire, "digest": _digest(PXCB_DOMAIN, [wire])}


def _request_base(
    *,
    magic: bytes,
    kind: int,
    carrier: dict[str, Any],
    request_id: bytes,
    store: bytes,
    epoch: int,
    retained_s0: bytes,
    expected_s1: bytes,
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
            retained_s0,
            expected_s1,
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
    carrier: dict[str, Any],
    request_id: bytes,
    store: bytes,
    epoch: int,
    retained_s0: bytes,
    expected_s1: bytes,
    payload: bytes,
    nonce: bytes,
) -> dict[str, bytes]:
    args = {
        "kind": kind,
        "carrier": carrier,
        "request_id": request_id,
        "store": store,
        "epoch": epoch,
        "retained_s0": retained_s0,
        "expected_s1": expected_s1,
        "payload": payload,
        "nonce": nonce,
    }
    values = carrier["wire"] + payload
    transcript = _request_base(magic=PXRA_SIGNING_MAGIC, **args) + values
    signature = _private(CONTROLLER_SEED).sign(transcript)
    wire = _request_base(magic=PXRA_MAGIC, **args) + _u16(len(signature)) + values + signature
    return {
        "wire": wire,
        "digest": _digest(PXRA_DOMAIN, [wire]),
        "payload_digest": bytes(32) if not payload else _digest(PXRA_PAYLOAD_DOMAIN, [payload]),
        "transcript": transcript,
        "signature": signature,
    }


def _ed25519_inner(public_key: bytes) -> VerifyInner:
    def verify(
        _principal: bytes,
        _key: bytes,
        _algorithm: int,
        _algorithm_version: int,
        transcript: bytes,
        signature: bytes,
    ) -> bool:
        try:
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, transcript)
        except (InvalidSignature, ValueError):
            return False
        return True

    return verify


def _ed25519_outer(public_key: bytes) -> VerifyOuter:
    def verify(
        _principal: bytes,
        _key: bytes,
        fingerprint: bytes,
        transcript: bytes,
        signature: bytes,
    ) -> bool:
        if fingerprint != _fingerprint(public_key):
            return False
        try:
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, transcript)
        except (InvalidSignature, ValueError):
            return False
        return True

    return verify


def _parse_pxra(
    wire: bytes,
    *,
    verify_inner: VerifyInner | None = None,
    verify_outer: VerifyOuter | None = None,
) -> dict[str, Any]:
    if len(wire) > MAX_PXRA_BYTES or len(wire) < PXRA_FIXED_BYTES:
        raise ContractReject("PXRA2 length")
    cursor = _Cursor(wire)
    if cursor.take(4) != PXRA_MAGIC or cursor.u16() != ACCESS_VERSION:
        raise ContractReject("PXRA2 magic/version")
    kind = cursor.u16()
    if kind not in {1, 2} or cursor.u16() != 0:
        raise ContractReject("PXRA2 kind/reserved")
    carrier_length, payload_length = cursor.u16(), cursor.u32()
    values = {
        "request_id": cursor.take(16),
        "carrier_digest": cursor.take(32),
        "target": cursor.take(16),
        "store": cursor.take(32),
        "epoch": cursor.u64(),
        "retained_s0_wire": cursor.take(RETAINED_S0_BYTES),
        "expected_s1_wire": cursor.take(ACTIVE_S1_BYTES),
        "payload_digest": cursor.take(32),
        "principal": cursor.take(16),
        "key": cursor.take(16),
        "algorithm": cursor.u16(),
        "algorithm_version": cursor.u16(),
    }
    nonce_length = cursor.u16()
    if not 0 < nonce_length <= MAX_NONCE_BYTES:
        raise ContractReject("PXRA2 nonce length")
    nonce = cursor.take(nonce_length)
    signature_length_offset = cursor.offset
    if cursor.u16() != SIGNATURE_BYTES:
        raise ContractReject("PXRA2 signature length")
    if not 0 < carrier_length <= MAX_PXCB_BYTES:
        raise ContractReject("PXRA2 carrier length")
    if (kind == 1 and not 0 < payload_length <= MAX_PXAR_BYTES) or (
        kind == 2 and payload_length != 0
    ):
        raise ContractReject("PXRA2 payload length")
    carrier = _parse_carrier(cursor.take(carrier_length))
    payload = cursor.take(payload_length)
    signature = cursor.take(SIGNATURE_BYTES)
    cursor.finish()
    retained_s0 = _parse_retained_s0(values["retained_s0_wire"])
    expected_s1 = _parse_active_s1(values["expected_s1_wire"])
    if (
        values["request_id"] == bytes(16)
        or values["target"] == bytes(16)
        or values["store"] == bytes(32)
        or values["epoch"] == 0
        or nonce == bytes(len(nonce))
        or carrier["digest"] != values["carrier_digest"]
        or carrier["target"] != values["target"]
        or values["principal"] != carrier["controller_principal"]
        or values["key"] != carrier["controller_key_ref"]
        or (values["algorithm"], values["algorithm_version"]) != (1, 1)
    ):
        raise ContractReject("PXRA2 semantic")
    inner: dict[str, Any] | None = None
    if kind == 1:
        inner = _parse_inner_pxar(payload)
        profile_principals = {
            inner["profile"]["mac_principal"],
            inner["profile"]["ubuntu_principal"],
        }
        if (
            values["request_id"] != inner["operation_id"]
            or values["target"] != inner["target"]
            or values["store"] != inner["store"]
            or values["retained_s0_wire"] != inner["retained_s0"]["wire"]
            or values["expected_s1_wire"] != inner["expected_s1"]["wire"]
            or inner["principal"] != carrier["controller_principal"]
            or inner["key"] != carrier["controller_key_ref"]
            or (inner["algorithm"], inner["algorithm_version"]) != (1, 1)
            or inner["nonce"] == nonce
            or carrier["controller_principal"] in profile_principals
            or carrier["runtime_principal"] in profile_principals
        ):
            raise ContractReject("PXRA2 inner correlation")
        inner_verifier = verify_inner or _ed25519_inner(_public(CONTROLLER_SEED))
        if not inner_verifier(
            inner["principal"],
            inner["key"],
            inner["algorithm"],
            inner["algorithm_version"],
            inner["transcript"],
            inner["signature"],
        ):
            raise ContractReject("PXRA2 inner callback")
    elif expected_s1["present"] != 1:
        raise ContractReject("PXRA2 Describe requires active S1 CAS")
    expected_payload_digest = bytes(32) if not payload else _digest(PXRA_PAYLOAD_DOMAIN, [payload])
    if values["payload_digest"] != expected_payload_digest:
        raise ContractReject("PXRA2 payload digest")
    transcript = PXRA_SIGNING_MAGIC + _u16(2) + wire[6:signature_length_offset]
    transcript += carrier["wire"] + payload
    outer_verifier = verify_outer or _ed25519_outer(_public(CONTROLLER_SEED))
    if not outer_verifier(
        carrier["controller_principal"],
        carrier["controller_key_ref"],
        carrier["controller_fingerprint"],
        transcript,
        signature,
    ):
        raise ContractReject("PXRA2 outer callback")
    return {
        **values,
        "kind": kind,
        "nonce": nonce,
        "carrier": carrier,
        "payload": payload,
        "inner": inner,
        "retained_s0": retained_s0,
        "expected_s1": expected_s1,
        "signature": signature,
        "transcript": transcript,
        "digest": _digest(PXRA_DOMAIN, [wire]),
        "wire": wire,
    }


def _response_payload_digest(kind: int, payload: bytes, profile_length: int) -> bytes:
    if kind == 1:
        return _digest(PXRR_PAYLOAD_DOMAIN, [payload])
    return _digest(PXRR_PAYLOAD_DOMAIN, [payload[:profile_length], payload[profile_length:]])


def _response_base(
    *,
    magic: bytes,
    request: dict[str, Any],
    payload: bytes,
    profile_length: int,
    descriptor_length: int,
    descriptor_digest: bytes,
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
            request["retained_s0_wire"],
            request["expected_s1_wire"],
            _response_payload_digest(request["kind"], payload, profile_length),
            descriptor_digest,
            carrier["runtime_principal"],
            carrier["runtime_key_ref"],
            _u16(1),
            _u16(1),
            carrier["digest"],
        ]
    )


def _encode_pxrr(
    request_wire: bytes,
    payload: bytes,
    *,
    profile_length: int = 0,
    descriptor_length: int = 0,
    descriptor_digest: bytes = bytes(32),
) -> dict[str, bytes]:
    request = _parse_pxra(request_wire)
    args = {
        "request": request,
        "payload": payload,
        "profile_length": profile_length,
        "descriptor_length": descriptor_length,
        "descriptor_digest": descriptor_digest,
    }
    values = request["nonce"] + request["carrier"]["wire"] + payload
    transcript = _response_base(magic=PXRR_SIGNING_MAGIC, **args) + values
    signature = _private(RUNTIME_SEED).sign(transcript)
    wire = _response_base(magic=PXRR_MAGIC, **args) + _u16(len(signature)) + values + signature
    return {
        "wire": wire,
        "digest": _digest(PXRR_DOMAIN, [wire]),
        "payload_digest": _response_payload_digest(request["kind"], payload, profile_length),
        "transcript": transcript,
        "signature": signature,
    }


def _parse_pxrr(
    wire: bytes,
    request_wire: bytes,
    *,
    verify_inner: VerifyInner | None = None,
    verify_outer: VerifyOuter | None = None,
) -> dict[str, Any]:
    request = _parse_pxra(request_wire)
    if len(wire) > MAX_PXRR_BYTES or len(wire) < PXRR_FIXED_BYTES:
        raise ContractReject("PXRR2 length")
    cursor = _Cursor(wire)
    if cursor.take(4) != PXRR_MAGIC or cursor.u16() != ACCESS_VERSION:
        raise ContractReject("PXRR2 magic/version")
    kind = cursor.u16()
    if kind not in {1, 2} or cursor.u16() != 0:
        raise ContractReject("PXRR2 kind/reserved")
    carrier_length, payload_length = cursor.u16(), cursor.u32()
    profile_length, descriptor_length, nonce_length = cursor.u16(), cursor.u32(), cursor.u16()
    values = {
        "request_id": cursor.take(16),
        "request_digest": cursor.take(32),
        "carrier_digest": cursor.take(32),
        "target": cursor.take(16),
        "store": cursor.take(32),
        "epoch": cursor.u64(),
        "retained_s0_wire": cursor.take(RETAINED_S0_BYTES),
        "expected_s1_wire": cursor.take(ACTIVE_S1_BYTES),
        "payload_digest": cursor.take(32),
        "descriptor_digest": cursor.take(32),
        "principal": cursor.take(16),
        "key": cursor.take(16),
        "algorithm": cursor.u16(),
        "algorithm_version": cursor.u16(),
        "claim_carrier_digest": cursor.take(32),
    }
    signature_length_offset = cursor.offset
    if cursor.u16() != SIGNATURE_BYTES:
        raise ContractReject("PXRR2 signature length")
    if not 0 < carrier_length <= MAX_PXCB_BYTES or not 0 < nonce_length <= MAX_NONCE_BYTES:
        raise ContractReject("PXRR2 variable length")
    if kind == 1:
        if (
            not 0 < payload_length <= MAX_CANONICAL_PXAU_BYTES
            or profile_length
            or descriptor_length
        ):
            raise ContractReject("PXRR2 Apply length")
    elif (
        not 0 < profile_length <= MAX_PXAD_BYTES
        or not 6 <= descriptor_length <= MAX_DESCRIPTOR_BYTES
        or payload_length != profile_length + descriptor_length
    ):
        raise ContractReject("PXRR2 Describe length")
    nonce = cursor.take(nonce_length)
    carrier = _parse_carrier(cursor.take(carrier_length))
    payload = cursor.take(payload_length)
    signature = cursor.take(SIGNATURE_BYTES)
    cursor.finish()
    correlated = {
        "request_id": request["request_id"],
        "request_digest": request["digest"],
        "target": request["target"],
        "store": request["store"],
        "epoch": request["epoch"],
        "retained_s0_wire": request["retained_s0_wire"],
        "expected_s1_wire": request["expected_s1_wire"],
    }
    if any(values[key] != expected for key, expected in correlated.items()):
        raise ContractReject("PXRR2 request correlation")
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
        raise ContractReject("PXRR2 semantic")
    inner: dict[str, Any] | None = None
    profile: dict[str, Any] | None = None
    descriptor = b""
    if kind == 1:
        if values["descriptor_digest"] != bytes(32) or request["inner"] is None:
            raise ContractReject("PXRR2 Apply shape")
        inner = _parse_inner_pxau(payload, request["inner"])
        if (
            inner["runtime_principal"] != carrier["runtime_principal"]
            or inner["runtime_key"] != carrier["runtime_key_ref"]
            or inner["completion_runtime_host_epoch"] != request["epoch"]
        ):
            raise ContractReject("PXRR2 inner correlation")
        inner_verifier = verify_inner or _ed25519_inner(_public(RUNTIME_SEED))
        if not inner_verifier(
            inner["runtime_principal"],
            inner["runtime_key"],
            inner["algorithm"],
            inner["algorithm_version"],
            inner["transcript"],
            inner["signature"],
        ):
            raise ContractReject("PXRR2 inner callback")
    else:
        profile = _parse_profile(payload[:profile_length])
        descriptor = payload[profile_length:]
        profile_principals = {profile["mac_principal"], profile["ubuntu_principal"]}
        if (
            request["expected_s1"]["present"] != 1
            or profile["target"] != request["target"]
            or carrier["controller_principal"] in profile_principals
            or carrier["runtime_principal"] in profile_principals
            or descriptor[:6] != b"PXAP\0\x01"
            or values["descriptor_digest"] != _digest(PXAP_DOMAIN, [descriptor])
            or values["descriptor_digest"] != request["retained_s0"]["descriptor_payload_digest"]
        ):
            raise ContractReject("PXRR2 Describe consumer")
    transcript = PXRR_SIGNING_MAGIC + _u16(2) + wire[6:signature_length_offset]
    transcript += nonce + carrier["wire"] + payload
    outer_verifier = verify_outer or _ed25519_outer(_public(RUNTIME_SEED))
    if not outer_verifier(
        carrier["runtime_principal"],
        carrier["runtime_key_ref"],
        carrier["runtime_fingerprint"],
        transcript,
        signature,
    ):
        raise ContractReject("PXRR2 outer callback")
    return {
        **values,
        "kind": kind,
        "nonce": nonce,
        "carrier": carrier,
        "payload": payload,
        "inner": inner,
        "profile": profile,
        "descriptor": descriptor,
        "signature": signature,
        "transcript": transcript,
        "digest": _digest(PXRR_DOMAIN, [wire]),
        "wire": wire,
    }


def _resign_pxra(wire: bytes) -> bytes:
    nonce_length = struct.unpack_from(">H", wire, REQUEST_OFFSETS["nonce_length"])[0]
    signature_length_offset = REQUEST_OFFSETS["nonce"] + nonce_length
    if wire[signature_length_offset : signature_length_offset + 2] != _u16(SIGNATURE_BYTES):
        raise AssertionError("PXRA2 signature offset")
    transcript = PXRA_SIGNING_MAGIC + _u16(2) + wire[6:signature_length_offset]
    transcript += wire[signature_length_offset + 2 : -SIGNATURE_BYTES]
    return wire[:-SIGNATURE_BYTES] + _private(CONTROLLER_SEED).sign(transcript)


def _resign_pxrr(wire: bytes) -> bytes:
    offset = RESPONSE_OFFSETS["signature_length"]
    if wire[offset : offset + 2] != _u16(SIGNATURE_BYTES):
        raise AssertionError("PXRR2 signature offset")
    transcript = PXRR_SIGNING_MAGIC + _u16(2) + wire[6:offset]
    transcript += wire[offset + 2 : -SIGNATURE_BYTES]
    return wire[:-SIGNATURE_BYTES] + _private(RUNTIME_SEED).sign(transcript)


@lru_cache(maxsize=1)
def _vectors() -> dict[str, Any]:
    inner = _inner_fixture()
    active = inner["active_ready"]
    local = inner["local_only_ready"]
    inner_pxar = bytes.fromhex(active["pxar_v11"]["wire_hex"])
    inner_pxau = bytes.fromhex(active["pxau_v2"]["wire_hex"])
    parsed_inner = _parse_inner_pxar(inner_pxar)
    retained_s0 = bytes.fromhex(inner["retained_s0_cas"]["wire_hex"])
    absent_s1 = bytes.fromhex(active["expected_s1_cas"]["wire_hex"])
    active_s1 = bytes.fromhex(local["expected_s1_cas"]["wire_hex"])
    carrier = _carrier(parsed_inner["target"])
    apply_request = _encode_pxra(
        kind=1,
        carrier=carrier,
        request_id=parsed_inner["operation_id"],
        store=parsed_inner["store"],
        epoch=RUNTIME_HOST_EPOCH,
        retained_s0=retained_s0,
        expected_s1=absent_s1,
        payload=inner_pxar,
        nonce=b"t2-proxy-active-outer",
    )
    parsed_apply_request = _parse_pxra(apply_request["wire"])
    apply_response = _encode_pxrr(apply_request["wire"], inner_pxau)
    describe_request = _encode_pxra(
        kind=2,
        carrier=carrier,
        request_id=bytes.fromhex("d4" * 16),
        store=parsed_inner["store"],
        epoch=RUNTIME_HOST_EPOCH,
        retained_s0=retained_s0,
        expected_s1=active_s1,
        payload=b"",
        nonce=b"t2-proxy-describe-outer",
    )
    parsed_describe_request = _parse_pxra(describe_request["wire"])
    descriptor = bytes.fromhex(inner["retained_s0_cas"]["bootstrap_pxap_hex"])
    profile = parsed_inner["profile"]["wire"]
    describe_response = _encode_pxrr(
        describe_request["wire"],
        profile + descriptor,
        profile_length=len(profile),
        descriptor_length=len(descriptor),
        descriptor_digest=_digest(PXAP_DOMAIN, [descriptor]),
    )
    return {
        "inner": inner,
        "inner_pxar": inner_pxar,
        "inner_pxau": inner_pxau,
        "parsed_inner": parsed_inner,
        "carrier": carrier,
        "apply_request": apply_request,
        "parsed_apply_request": parsed_apply_request,
        "apply_response": apply_response,
        "describe_request": describe_request,
        "parsed_describe_request": parsed_describe_request,
        "describe_response": describe_response,
        "profile": profile,
        "descriptor": descriptor,
    }


def _wire_entry(value: dict[str, bytes]) -> dict[str, Any]:
    wire = value["wire"]
    transcript = value["transcript"]
    return {
        "wire_hex": wire.hex(),
        "wire_length": len(wire),
        "digest_hex": value["digest"].hex(),
        "payload_digest_hex": value["payload_digest"].hex(),
        "signing_transcript_hex": transcript.hex(),
        "signing_transcript_length": len(transcript),
        "signing_transcript_sha256_hex": hashlib.sha256(transcript).hexdigest(),
        "signature_hex": value["signature"].hex(),
        "signature_length": len(value["signature"]),
    }


def _generated_fixture() -> dict[str, Any]:
    vectors = _vectors()
    inner_request = vectors["parsed_inner"]
    inner_terminal = _parse_inner_pxau(vectors["inner_pxau"], inner_request)
    return {
        "format": "paraegox-t2-remote-agent-access-v2",
        "source": "independent Python struct/hashlib/cryptography outer-v2 oracle",
        "authority_note": (
            "PXRR2 Describe is strict-consumer-only; its checked-in sample is "
            "synthetic/historical-negative and is neither producer nor currentness evidence."
        ),
        "source_freeze": {
            "ref": R236_REF,
            "commit": R236_COMMIT,
            "remote_agent_access.rs_sha256": OUTER_SOURCE_SHA256,
            "remote_agent_data_plane_plan.rs_sha256": PLAN_SOURCE_SHA256,
            "corrected_inner_fixture_sha256": INNER_FIXTURE_SHA256,
        },
        "semantic_constants": {
            "pxra_v2_fixed_bytes": PXRA_FIXED_BYTES,
            "pxra_v2_max_bytes": MAX_PXRA_BYTES,
            "pxrr_v2_fixed_bytes": PXRR_FIXED_BYTES,
            "pxrr_v2_max_bytes": MAX_PXRR_BYTES,
            "pxcb_fixed_bytes": PXCB_FIXED_BYTES,
            "pxcb_max_bytes": MAX_PXCB_BYTES,
            "signature_bytes": SIGNATURE_BYTES,
            "max_nonce_bytes": MAX_NONCE_BYTES,
            "max_descriptor_bytes": MAX_DESCRIPTOR_BYTES,
            "max_canonical_pxau_v2_bytes": MAX_CANONICAL_PXAU_BYTES,
            "retained_s0_cas_bytes": RETAINED_S0_BYTES,
            "active_s1_cas_bytes": ACTIVE_S1_BYTES,
            "request_offsets": REQUEST_OFFSETS,
            "response_offsets": RESPONSE_OFFSETS,
        },
        "domains": {
            "pxra_v2_signing_magic_hex": PXRA_SIGNING_MAGIC.hex(),
            "pxrr_v2_signing_magic_hex": PXRR_SIGNING_MAGIC.hex(),
            "pxra_v2_payload_digest_hex": PXRA_PAYLOAD_DOMAIN.hex(),
            "pxrr_v2_payload_digest_hex": PXRR_PAYLOAD_DOMAIN.hex(),
            "pxra_v2_digest_hex": PXRA_DOMAIN.hex(),
            "pxrr_v2_digest_hex": PXRR_DOMAIN.hex(),
            "pxcb_v1_digest_hex": PXCB_DOMAIN.hex(),
            "control_key_fingerprint_hex": CONTROL_KEY_DOMAIN.hex(),
        },
        "keys": {
            "controller_public_key_hex": _public(CONTROLLER_SEED).hex(),
            "controller_key_ref_hex": CONTROLLER_KEY_REF.hex(),
            "controller_principal_hex": CONTROLLER_PRINCIPAL.hex(),
            "controller_fingerprint_hex": _fingerprint(_public(CONTROLLER_SEED)).hex(),
            "runtime_public_key_hex": _public(RUNTIME_SEED).hex(),
            "runtime_key_ref_hex": RUNTIME_KEY_REF.hex(),
            "runtime_principal_hex": RUNTIME_PRINCIPAL.hex(),
            "runtime_fingerprint_hex": _fingerprint(_public(RUNTIME_SEED)).hex(),
        },
        "carrier": {
            "wire_hex": vectors["carrier"]["wire"].hex(),
            "wire_length": len(vectors["carrier"]["wire"]),
            "digest_hex": vectors["carrier"]["digest"].hex(),
        },
        "inner_signature_inputs": {
            "source_fixture": "t2_remote_agent_proxy_data_plane_v2.json#active_ready",
            "pxar_v11_wire_length": len(vectors["inner_pxar"]),
            "pxar_v11_digest_hex": inner_request["digest"].hex(),
            "controller_transcript_hex": inner_request["transcript"].hex(),
            "controller_transcript_length": len(inner_request["transcript"]),
            "controller_transcript_sha256_hex": hashlib.sha256(
                inner_request["transcript"]
            ).hexdigest(),
            "controller_signature_hex": inner_request["signature"].hex(),
            "pxau_v2_wire_length": len(vectors["inner_pxau"]),
            "pxau_v2_digest_hex": inner_terminal["digest"].hex(),
            "runtime_transcript_hex": inner_terminal["transcript"].hex(),
            "runtime_transcript_length": len(inner_terminal["transcript"]),
            "runtime_transcript_sha256_hex": hashlib.sha256(
                inner_terminal["transcript"]
            ).hexdigest(),
            "runtime_signature_hex": inner_terminal["signature"].hex(),
        },
        "apply": {
            "pxra_v2": _wire_entry(vectors["apply_request"]),
            "pxrr_v2": _wire_entry(vectors["apply_response"]),
        },
        "describe": {
            "pxra_v2": _wire_entry(vectors["describe_request"]),
            "pxrr_v2_strict_consumer": {
                "classification": "synthetic/historical-negative",
                "producer_evidence": False,
                "currentness_evidence": False,
                **_wire_entry(vectors["describe_response"]),
            },
        },
    }


def test_r236_source_freeze_widths_offsets_and_maxima() -> None:
    assert hashlib.sha256(OUTER_SOURCE_PATH.read_bytes()).hexdigest() == OUTER_SOURCE_SHA256
    assert hashlib.sha256(PLAN_SOURCE_PATH.read_bytes()).hexdigest() == PLAN_SOURCE_SHA256
    assert hashlib.sha256(INNER_FIXTURE_PATH.read_bytes()).hexdigest() == INNER_FIXTURE_SHA256
    assert PXRA_FIXED_BYTES == 544
    assert MAX_PXRA_BYTES == 544 + 64 + 483 + 6_700 + 64
    assert PXRR_FIXED_BYTES == 646
    assert MAX_PXRR_BYTES == 5_792
    assert MAX_DESCRIPTOR_BYTES == 2_048
    assert MAX_CANONICAL_PXAU_BYTES == 1_195
    assert REQUEST_OFFSETS["nonce"] == 542
    assert RESPONSE_OFFSETS["signature_length"] == 644
    assert RESPONSE_OFFSETS["values"] == PXRR_FIXED_BYTES


def test_independent_outer_v2_oracle_matches_checked_in_golden() -> None:
    assert _read_json(FIXTURE_PATH) == _generated_fixture()


def test_apply_round_trip_exercises_four_signature_callbacks() -> None:
    vectors = _vectors()
    calls: list[tuple[str, bytes, bytes, int, int, bytes, bytes]] = []

    def inner_controller(
        principal: bytes,
        key: bytes,
        algorithm: int,
        version: int,
        transcript: bytes,
        signature: bytes,
    ) -> bool:
        calls.append(
            ("inner-controller", principal, key, algorithm, version, transcript, signature)
        )
        return _ed25519_inner(_public(CONTROLLER_SEED))(
            principal, key, algorithm, version, transcript, signature
        )

    def outer_controller(
        principal: bytes,
        key: bytes,
        fingerprint: bytes,
        transcript: bytes,
        signature: bytes,
    ) -> bool:
        calls.append(("outer-controller", principal, key, 1, 1, transcript, signature))
        return _ed25519_outer(_public(CONTROLLER_SEED))(
            principal, key, fingerprint, transcript, signature
        )

    request = _parse_pxra(
        vectors["apply_request"]["wire"],
        verify_inner=inner_controller,
        verify_outer=outer_controller,
    )

    def inner_runtime(
        principal: bytes,
        key: bytes,
        algorithm: int,
        version: int,
        transcript: bytes,
        signature: bytes,
    ) -> bool:
        calls.append(("inner-runtime", principal, key, algorithm, version, transcript, signature))
        return _ed25519_inner(_public(RUNTIME_SEED))(
            principal, key, algorithm, version, transcript, signature
        )

    def outer_runtime(
        principal: bytes,
        key: bytes,
        fingerprint: bytes,
        transcript: bytes,
        signature: bytes,
    ) -> bool:
        calls.append(("outer-runtime", principal, key, 1, 1, transcript, signature))
        return _ed25519_outer(_public(RUNTIME_SEED))(
            principal, key, fingerprint, transcript, signature
        )

    response = _parse_pxrr(
        vectors["apply_response"]["wire"],
        request["wire"],
        verify_inner=inner_runtime,
        verify_outer=outer_runtime,
    )
    assert [call[0] for call in calls] == [
        "inner-controller",
        "outer-controller",
        "inner-runtime",
        "outer-runtime",
    ]
    assert calls[0][1:5] == (CONTROLLER_PRINCIPAL, CONTROLLER_KEY_REF, 1, 1)
    assert calls[1][1:3] == (CONTROLLER_PRINCIPAL, CONTROLLER_KEY_REF)
    assert calls[2][1:5] == (RUNTIME_PRINCIPAL, RUNTIME_KEY_REF, 1, 1)
    assert calls[3][1:3] == (RUNTIME_PRINCIPAL, RUNTIME_KEY_REF)
    assert len({call[5] for call in calls}) == 4
    assert response["request_digest"] == request["digest"]


def test_describe_request_and_synthetic_historical_response_are_consumer_only() -> None:
    vectors = _vectors()
    request = _parse_pxra(vectors["describe_request"]["wire"])
    response = _parse_pxrr(vectors["describe_response"]["wire"], request["wire"])
    fixture = _generated_fixture()["describe"]["pxrr_v2_strict_consumer"]
    assert request["kind"] == response["kind"] == 2
    assert request["payload"] == b""
    assert request["expected_s1"]["present"] == 1
    assert response["descriptor"] == vectors["descriptor"]
    assert fixture["classification"] == "synthetic/historical-negative"
    assert fixture["producer_evidence"] is False
    assert fixture["currentness_evidence"] is False


@pytest.mark.parametrize(
    ("offset", "replacement"),
    [
        (REQUEST_OFFSETS["request_id"], bytes.fromhex("ef" * 16)),
        (REQUEST_OFFSETS["target"], bytes.fromhex("ef" * 16)),
        (REQUEST_OFFSETS["store"], bytes.fromhex("ef" * 32)),
        (REQUEST_OFFSETS["retained_s0_cas"], bytes.fromhex("ef" * 32)),
        (REQUEST_OFFSETS["expected_s1_cas"] + 16, _u64(99)),
        (REQUEST_OFFSETS["principal"], bytes.fromhex("ef" * 16)),
        (REQUEST_OFFSETS["key"], bytes.fromhex("ef" * 16)),
        (REQUEST_OFFSETS["algorithm"], _u16(2)),
        (REQUEST_OFFSETS["algorithm_version"], _u16(2)),
    ],
)
def test_apply_request_cas_correlation_principal_algorithm_and_version_fail_closed(
    offset: int, replacement: bytes
) -> None:
    wire = _vectors()["apply_request"]["wire"]
    with pytest.raises(ContractReject):
        _parse_pxra(_resign_pxra(_replace(wire, offset, replacement)))


def test_request_nonce_is_nonzero_distinct_and_response_nonce_is_correlated() -> None:
    vectors = _vectors()
    wire = vectors["apply_request"]["wire"]
    inner_nonce = vectors["parsed_inner"]["nonce"]
    outer_nonce_length = struct.unpack_from(">H", wire, REQUEST_OFFSETS["nonce_length"])[0]
    assert len(inner_nonce) == outer_nonce_length
    tampered = _replace(wire, REQUEST_OFFSETS["nonce"], inner_nonce)
    with pytest.raises(ContractReject):
        _parse_pxra(_resign_pxra(tampered))
    with pytest.raises(ContractReject):
        _parse_pxra(
            _resign_pxra(_replace(wire, REQUEST_OFFSETS["nonce"], bytes(outer_nonce_length)))
        )

    response = vectors["apply_response"]["wire"]
    response_nonce_length = struct.unpack_from(">H", response, RESPONSE_OFFSETS["nonce_length"])[0]
    response_nonce = bytes.fromhex("ed" * response_nonce_length)
    with pytest.raises(ContractReject):
        _parse_pxrr(
            _resign_pxrr(_replace(response, RESPONSE_OFFSETS["values"], response_nonce)),
            wire,
        )


@pytest.mark.parametrize(
    ("offset", "replacement"),
    [
        (RESPONSE_OFFSETS["request_id"], bytes.fromhex("ed" * 16)),
        (RESPONSE_OFFSETS["request_digest"], bytes.fromhex("ed" * 32)),
        (RESPONSE_OFFSETS["epoch"], _u64(99)),
        (RESPONSE_OFFSETS["retained_s0_cas"], bytes.fromhex("ed" * 32)),
        (RESPONSE_OFFSETS["expected_s1_cas"] + 16, _u64(99)),
        (RESPONSE_OFFSETS["principal"], bytes.fromhex("ed" * 16)),
        (RESPONSE_OFFSETS["key"], bytes.fromhex("ed" * 16)),
        (RESPONSE_OFFSETS["algorithm"], _u16(2)),
        (RESPONSE_OFFSETS["algorithm_version"], _u16(2)),
    ],
)
def test_apply_response_correlation_cas_principal_algorithm_and_version_fail_closed(
    offset: int, replacement: bytes
) -> None:
    vectors = _vectors()
    tampered = _resign_pxrr(_replace(vectors["apply_response"]["wire"], offset, replacement))
    with pytest.raises(ContractReject):
        _parse_pxrr(tampered, vectors["apply_request"]["wire"])


def test_v1_v2_cross_rejection_cross_magic_and_maxima() -> None:
    vectors = _vectors()
    old = _read_json(V1_FIXTURE_PATH)["access"]
    with pytest.raises(ContractReject):
        _parse_pxra(bytes.fromhex(old["pxra_apply"]["wire_hex"]))
    with pytest.raises(ContractReject):
        _parse_pxrr(
            bytes.fromhex(old["pxrr_apply"]["wire_hex"]),
            vectors["apply_request"]["wire"],
        )
    with pytest.raises(ContractReject):
        _parse_pxra(_replace(vectors["apply_request"]["wire"], 4, _u16(1)))
    with pytest.raises(ContractReject):
        _parse_pxrr(
            _replace(vectors["apply_response"]["wire"], 4, _u16(1)),
            vectors["apply_request"]["wire"],
        )
    for magic in (b"PXRR", b"PXAR", b"PXAU", b"PXRS"):
        with pytest.raises(ContractReject):
            _parse_pxra(_replace(vectors["apply_request"]["wire"], 0, magic))
    for magic in (b"PXRA", b"PXAR", b"PXAU", b"PXRS"):
        with pytest.raises(ContractReject):
            _parse_pxrr(
                _replace(vectors["apply_response"]["wire"], 0, magic),
                vectors["apply_request"]["wire"],
            )
    with pytest.raises(ContractReject):
        _parse_pxra(bytes(MAX_PXRA_BYTES + 1))
    with pytest.raises(ContractReject):
        _parse_pxrr(bytes(MAX_PXRR_BYTES + 1), vectors["apply_request"]["wire"])


def test_outer_and_inner_signature_and_payload_tamper_are_rejected() -> None:
    vectors = _vectors()
    request_wire = vectors["apply_request"]["wire"]
    response_wire = vectors["apply_response"]["wire"]
    with pytest.raises(ContractReject):
        _parse_pxra(request_wire[:-1] + bytes([request_wire[-1] ^ 1]))
    with pytest.raises(ContractReject):
        _parse_pxrr(
            response_wire[:-1] + bytes([response_wire[-1] ^ 1]),
            request_wire,
        )

    inner_pxar = vectors["inner_pxar"]
    envelope_length = struct.unpack_from(">I", inner_pxar, 6)[0]
    inner_signature_offset = PXAR_HEADER_BYTES + envelope_length - SIGNATURE_BYTES
    tampered_inner_pxar = _replace(inner_pxar, inner_signature_offset, b"\x00")
    request = vectors["parsed_apply_request"]
    rebuilt = _encode_pxra(
        kind=1,
        carrier=request["carrier"],
        request_id=request["request_id"],
        store=request["store"],
        epoch=request["epoch"],
        retained_s0=request["retained_s0_wire"],
        expected_s1=request["expected_s1_wire"],
        payload=tampered_inner_pxar,
        nonce=request["nonce"],
    )
    with pytest.raises(ContractReject):
        _parse_pxra(rebuilt["wire"])

    tampered_pxau = vectors["inner_pxau"][:-1] + bytes([vectors["inner_pxau"][-1] ^ 1])
    rebuilt_response = _encode_pxrr(request_wire, tampered_pxau)
    with pytest.raises(ContractReject):
        _parse_pxrr(rebuilt_response["wire"], request_wire)
