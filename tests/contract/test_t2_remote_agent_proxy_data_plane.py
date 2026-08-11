from __future__ import annotations

import hashlib
import importlib.util
import json
import struct
from functools import lru_cache
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
FIXTURE_PATH = REPO_ROOT / "tests/fixtures/wire/t2_remote_agent_proxy_data_plane_v2.json"
V1_ORACLE_PATH = REPO_ROOT / "tests/contract/test_t2_remote_agent_access.py"
RUST_SOURCE_PATH = (
    REPO_ROOT / "crates/paraegox-runtime-contracts/src/remote_agent_data_plane_plan.rs"
)

RUST_SOURCE_REF = "r244"
RUST_SOURCE_COMMIT = "e5f6c414e5f3450df4f86a7c0347ffc5ceb582bf"
RUST_SOURCE_SHA256 = "2649e0457c47e63a2132df2b8db9de52818b71645c54b656747c978fb2d882e7"

DIGEST_MAGIC = b"ParaEGOX\0canonical-digest"
DIGEST_VERSION = 1
PXTA_ZERO = b"PXTA\0\x01\0\0\0\0"

PXAE_MAGIC = b"PXAE"
PXAE_VERSION = 1
PXAE_BYTES = 270
PXAD_MAGIC = b"PXAD"
PXAD_VERSION = 1
PXAD_KIND = 1
PXAD_ACL_VERSION = 1
MAX_PXAD_BYTES = 439
PXTE_MAGIC = b"PXTE"
PXTE_VERSION = 10
PXTE_PREFIX_BYTES = 320
MAX_PREDECESSOR_PXTE_BYTES = 1_465
MAX_PXTE_BYTES = 2_576
PXAR_MAGIC = b"PXAR"
PXAR_VERSION = 11
PXAR_HEADER_BYTES = 18
MAX_ENVELOPE_BYTES = 4_096
MAX_PXAR_BYTES = 6_700
PXAU_MAGIC = b"PXAU"
PXAU_VERSION = 2
PXAU_SIGNING_VERSION = 2
PXAU_FIXED_BYTES = 683
PXAU_SIGNATURE_BYTES = 64
PXAU_CANONICAL_BYTES = 747
MAX_PXAU_SIGNATURE_BYTES = 512
MAX_CANONICAL_PXAU_BYTES = PXAU_FIXED_BYTES + MAX_PXAU_SIGNATURE_BYTES
MAX_PXAU_CARRIER_BYTES = 2_048

RETAINED_S0_CAS_BYTES = 200
ACTIVE_S1_CAS_BYTES = 152
EXACT_ROUTE_COUNT = 2
EXACT_ROUTE_BITMAP = 0b11
QUEUE_CAPACITY = 1
WORKERS_PER_ROUTE = 1
MAX_PENDING_SESSIONS = 1
MAX_SESSIONS = 1
MAX_LINKS = 1
MAX_MESSAGE_BYTES = 1_114_220
MAX_AGENT_FRAME_BYTES = 1_048_680
MAX_AGENT_RESPONSE_BYTES = 1_048_576
MAX_OPERATION_TIMEOUT_NANOS = 30_000_000_000
PROFILE_OPERATION_TIMEOUT_NANOS = 20_000_000_000
ENVELOPE_ORIGINAL_BUDGET_NANOS = 30_000_000_000
ENVELOPE_REMAINING_BUDGET_NANOS = 25_000_000_000
HARDENING_FEATURES = 0b11_1111_1111_1111_1111
HARDENING_PROFILE = (
    b"tls-listener=1;tls-connector=0;plaintext=0;acl=default-deny;"
    b"routes=pxap-submit,pxap-control;scouting=0;admin=0;plugins=0;"
    b"verify-name=1;verify-expiry=1;accept-pending=1;max-sessions=1;"
    b"max-links=1;queue-per-route=1;workers-per-route=1;retry=0;"
    b"deadline=single-admission-absolute-pxad-v1;shutdown=fence,drain,join,close"
)
DEADLINE_RULE = b"pxau-v2-deadline=admitted-at+pxad-operation-timeout;budget-reset=forbidden"
TEMPORAL_AUTHORITY_RULE = (
    b"temporal-remaining>=operation-timeout;selection>=admitted;"
    b"active-ready<deadline;local-cleanup-may-complete-after-deadline"
)

PXTE_DOMAIN = b"paraegox.runtime.target-execution.sha256.v10"
ASSIGNMENT_DOMAIN = b"paraegox.runtime.target-plan-assignments.sha256.v11"
PXAR_DOMAIN = b"paraegox.runtime.remote-agent-proxy-data-plane-request.sha256.v2"
TOPOLOGY_DOMAIN = b"paraegox.runtime.remote-agent-proxy-topology-compatibility.sha256.v2"
RETAINED_S0_CAS_DOMAIN = b"paraegox.runtime.remote-agent-retained-s0-cas.sha256.v2"
ACTIVE_S1_CAS_DOMAIN = b"paraegox.runtime.remote-agent-active-s1-cas.sha256.v2"
PXAU_SIGNING_MAGIC = b"ParaEGOX\0remote-agent-proxy-data-plane-terminal-signing"
PXAU_RESULT_REF_DOMAIN = b"paraegox.runtime.remote-agent-proxy-data-plane-terminal-result.sha256.v2"
PXAU_DOMAIN = b"paraegox.runtime.remote-agent-proxy-data-plane-terminal.sha256.v2"

MODE_REMOTE_ACCESS_ACTIVE = 1
MODE_LOCAL_AGENT_ONLY_DEACTIVATE = 2
OUTCOME_ACTIVE_READY = 1
OUTCOME_LOCAL_ONLY_READY = 2
OUTCOME_NO_EFFECT_REJECTED = 3
OUTCOME_UNCERTAIN = 4
OUTCOME_QUARANTINED = 5
LIFECYCLE_MAY_HAVE_STARTED = 2
PHASE_READY_OBSERVATION = 4
PHASE_LOCAL_ONLY_OBSERVATION = 8
HEAD_COMMITTED_INCOMING = 3
DRAIN_NOT_STARTED = 1
DRAIN_DRAINED = 2
OBSERVATION_S1_ABSENT = 2
OBSERVATION_S1_READY = 3
FLAG_RETAINED_CENSUS_COMPLETE = 1
FLAG_RETAINED_S0_READY = 1 << 1
FLAG_S1_TLS_READY = 1 << 2
FLAG_S1_ACL_READY = 1 << 3
FLAG_S1_CLOSED = 1 << 4
FLAG_S1_LISTENER_RELEASED = 1 << 5
KNOWN_FLAGS = 0b0111_1111

PXAU_OFFSETS = {
    "target": 6,
    "store": 22,
    "source_scope": 54,
    "operation_id": 70,
    "envelope_request_digest": 86,
    "request_digest": 118,
    "target_slice_digest": 150,
    "assignment_digest": 182,
    "result_ref": 214,
    "request_mode": 230,
    "outcome": 231,
    "lifecycle_effect": 232,
    "phase": 233,
    "head_tag": 234,
    "desired_present": 235,
    "reserved_u16": 236,
    "desired_head_digest": 238,
    "fabric_generation": 270,
    "agent_generation": 279,
    "access_generation": 288,
    "fabric_session_epoch": 297,
    "proxy_session_epoch": 314,
    "retained_s0_current_cas_digest": 331,
    "retained_s0_census_before_digest": 363,
    "retained_s0_census_after_digest": 395,
    "topology_digest": 427,
    "resource_census_digest": 459,
    "raw_outcome_digest": 491,
    "submit_admitted_count": 523,
    "submit_terminalized_count": 531,
    "control_admitted_count": 539,
    "control_terminalized_count": 547,
    "access_generation_high_water": 555,
    "completion_runtime_host_epoch": 563,
    "completion_snapshot_sequence": 571,
    "completion_owner_slot_revision": 579,
    "selection_clock_domain": 587,
    "selection_clock_generation": 603,
    "admitted_at_nanos": 611,
    "absolute_deadline_nanos": 619,
    "selection_observed_at_nanos": 627,
    "physical_binding_census": 635,
    "queryable_declared_bitmap": 637,
    "ingress_fenced_bitmap": 638,
    "worker_joined_bitmap": 639,
    "drain_outcome": 640,
    "remote_observation": 641,
    "reserved_u8": 642,
    "flags": 643,
    "runtime_principal": 645,
    "runtime_key": 661,
    "algorithm": 677,
    "algorithm_version": 679,
    "signature_length": 681,
    "signature": 683,
}

RUNTIME_SEED = bytes.fromhex("43" * 32)
RUNTIME_PRINCIPAL = bytes.fromhex("88" * 16)
RUNTIME_KEY_REF = bytes.fromhex("8d" * 16)
FABRIC_SESSION_EPOCH = bytes.fromhex("95" * 16)
PROXY_SESSION_EPOCH = bytes.fromhex("a6" * 16)
RUNTIME_HOST_EPOCH = 23
INITIAL_OWNER_SLOT_REVISION = 41
ACTIVE_OWNER_SLOT_REVISION = 42
LOCAL_OWNER_SLOT_REVISION = 43


class ContractReject(ValueError):
    pass


def _load_v1_oracle() -> ModuleType:
    spec = importlib.util.spec_from_file_location(
        "t2_remote_agent_access_v1_oracle",
        V1_ORACLE_PATH,
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load frozen T2 v1 oracle")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


V1 = _load_v1_oracle()


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


def _topology_compatibility_digest() -> bytes:
    fields = [
        V1._compatibility_digest(),
        PXAE_MAGIC,
        _u16(PXAE_VERSION),
        _u16(PXAE_BYTES),
        PXAD_MAGIC,
        _u16(PXAD_VERSION),
        _u16(PXAD_KIND),
        _u16(PXAD_ACL_VERSION),
        _u32(MAX_PXAD_BYTES),
        PXTE_MAGIC,
        _u16(PXTE_VERSION),
        _u32(MAX_PXTE_BYTES),
        PXAR_MAGIC,
        _u16(PXAR_VERSION),
        _u16(PXAR_HEADER_BYTES),
        _u32(MAX_PXAR_BYTES),
        PXAU_MAGIC,
        _u16(PXAU_VERSION),
        _u16(PXAU_SIGNING_VERSION),
        _u32(PXAU_FIXED_BYTES),
        _u16(MAX_PXAU_CARRIER_BYTES),
        _u16(MAX_CANONICAL_PXAU_BYTES),
        _u16(MAX_PXAU_SIGNATURE_BYTES),
        PXTA_ZERO,
        _u16(RETAINED_S0_CAS_BYTES),
        _u16(ACTIVE_S1_CAS_BYTES),
        _u16(MODE_REMOTE_ACCESS_ACTIVE),
        _u16(MODE_LOCAL_AGENT_ONLY_DEACTIVATE),
        _u16(EXACT_ROUTE_COUNT),
        _u8(EXACT_ROUTE_BITMAP),
        _u16(QUEUE_CAPACITY),
        _u16(WORKERS_PER_ROUTE),
        _u16(MAX_PENDING_SESSIONS),
        _u16(MAX_SESSIONS),
        _u16(MAX_LINKS),
        _u32(MAX_MESSAGE_BYTES),
        _u32(MAX_AGENT_FRAME_BYTES),
        _u32(MAX_AGENT_RESPONSE_BYTES),
        _u64(MAX_OPERATION_TIMEOUT_NANOS),
        _u32(HARDENING_FEATURES),
        HARDENING_PROFILE,
        DEADLINE_RULE,
        TEMPORAL_AUTHORITY_RULE,
        _u16(KNOWN_FLAGS),
        *[_u16(value) for value in (1, 2, 4, 8, 16, 32, 64)],
        *[_u16(value) for value in range(1, 10)],
        *[_u16(value) for value in range(1, 6)],
        _u16(1),
        _u16(2),
        *[_u16(value) for value in range(1, 4)],
        *[_u16(value) for value in range(1, 4)],
        *[_u16(value) for value in range(1, 5)],
        PXTE_DOMAIN,
        ASSIGNMENT_DOMAIN,
        PXAR_DOMAIN,
        RETAINED_S0_CAS_DOMAIN,
        ACTIVE_S1_CAS_DOMAIN,
        PXAU_SIGNING_MAGIC,
        PXAU_RESULT_REF_DOMAIN,
        PXAU_DOMAIN,
    ]
    return _digest(TOPOLOGY_DOMAIN, fields)


def _retained_s0_values() -> dict[str, Any]:
    predecessor = _predecessor_vectors()
    bootstrap_pxap = b"PXAP\0\x01t2-proxy-bootstrap-descriptor"
    bootstrap_pxah = b"PXAH\0\x01t2-proxy-whole-bootstrap-receipt"
    return {
        "active_pxft_digest": bytes.fromhex("91" * 32),
        "active_pxst_digest": predecessor["active"]["terminal"]["receipt_digest"],
        "descriptor_record_digest": bytes.fromhex("94" * 32),
        "descriptor_record_sequence": 12,
        "descriptor_receipt_digest": _digest(V1.PXAH_WHOLE_DOMAIN, [bootstrap_pxah]),
        "descriptor_payload_digest": _digest(V1.PXAP_SHARED_DOMAIN, [bootstrap_pxap]),
        "fabric_session_epoch": FABRIC_SESSION_EPOCH,
        "fabric_generation": 7,
        "agent_generation": 8,
        "bootstrap_pxap": bootstrap_pxap,
        "bootstrap_pxah": bootstrap_pxah,
    }


def _encode_retained_s0_cas(values: dict[str, Any]) -> bytes:
    wire = b"".join(
        [
            values["active_pxft_digest"],
            values["active_pxst_digest"],
            values["descriptor_record_digest"],
            _u64(values["descriptor_record_sequence"]),
            values["descriptor_receipt_digest"],
            values["descriptor_payload_digest"],
            values["fabric_session_epoch"],
            _u64(values["fabric_generation"]),
            _u64(values["agent_generation"]),
        ]
    )
    assert len(wire) == RETAINED_S0_CAS_BYTES
    _parse_retained_s0_cas(wire)
    return wire


def _parse_retained_s0_cas(wire: bytes) -> dict[str, Any]:
    if len(wire) != RETAINED_S0_CAS_BYTES:
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
    fixed = [
        parsed["active_pxft_digest"],
        parsed["active_pxst_digest"],
        parsed["descriptor_record_digest"],
        parsed["descriptor_receipt_digest"],
        parsed["descriptor_payload_digest"],
        parsed["fabric_session_epoch"],
    ]
    if any(value == bytes(len(value)) for value in fixed) or any(
        parsed[key] == 0
        for key in (
            "descriptor_record_sequence",
            "fabric_generation",
            "agent_generation",
        )
    ):
        raise ContractReject("retained S0 CAS semantic")
    parsed["wire"] = wire
    parsed["digest"] = _digest(RETAINED_S0_CAS_DOMAIN, [wire])
    return parsed


def _encode_active_s1_cas_absent(high_water: int, owner_revision: int) -> bytes:
    wire = _u8(0) + bytes(7) + _u64(high_water) + _u64(owner_revision) + bytes(128)
    assert len(wire) == ACTIVE_S1_CAS_BYTES
    _parse_active_s1_cas(wire)
    return wire


def _encode_active_s1_cas_active(
    high_water: int,
    owner_revision: int,
    *,
    pxau_digest: bytes,
    request_digest: bytes,
    snapshot_digest: bytes,
    snapshot_sequence: int,
    access_generation: int,
    proxy_session_epoch: bytes,
) -> bytes:
    wire = b"".join(
        [
            _u8(1),
            bytes(7),
            _u64(high_water),
            _u64(owner_revision),
            pxau_digest,
            request_digest,
            snapshot_digest,
            _u64(snapshot_sequence),
            _u64(access_generation),
            proxy_session_epoch,
        ]
    )
    assert len(wire) == ACTIVE_S1_CAS_BYTES
    _parse_active_s1_cas(wire)
    return wire


def _parse_active_s1_cas(wire: bytes) -> dict[str, Any]:
    if len(wire) != ACTIVE_S1_CAS_BYTES:
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
        raise ContractReject("active S1 CAS revision")
    active_values = [
        parsed["active_pxau_digest"],
        parsed["active_request_digest"],
        parsed["active_snapshot_digest"],
        _u64(parsed["active_snapshot_sequence"]),
        _u64(parsed["active_access_generation"]),
        parsed["active_proxy_session_epoch"],
    ]
    all_active_zero = all(value == bytes(len(value)) for value in active_values)
    if present == 0:
        if not all_active_zero:
            raise ContractReject("absent S1 has active tuple")
    elif present == 1:
        if (
            all_active_zero
            or parsed["access_generation_high_water"] == 0
            or any(
                parsed[key] == bytes(len(parsed[key]))
                for key in (
                    "active_pxau_digest",
                    "active_request_digest",
                    "active_snapshot_digest",
                    "active_proxy_session_epoch",
                )
            )
            or parsed["active_snapshot_sequence"] == 0
            or parsed["active_access_generation"] != parsed["access_generation_high_water"]
        ):
            raise ContractReject("active S1 tuple")
    else:
        raise ContractReject("active S1 presence")
    parsed["wire"] = wire
    parsed["digest"] = _digest(ACTIVE_S1_CAS_DOMAIN, [wire])
    return parsed


@lru_cache(maxsize=1)
def _predecessor_vectors() -> dict[str, Any]:
    return V1.S7._build_vectors()


def _encode_pxte(
    *,
    mode: int,
    projection: bytes,
    predecessor: bytes,
    retained_s0_cas: bytes,
    expected_s1_cas: bytes,
    profile: bytes,
) -> bytes:
    wire = b"".join(
        [
            PXTE_MAGIC,
            _u16(PXTE_VERSION),
            projection,
            _topology_compatibility_digest(),
            _u16(PXAD_VERSION),
            _u8(mode),
            _u8(1),
            _u32(len(predecessor)),
            _u32(len(profile)),
            predecessor,
            retained_s0_cas,
            expected_s1_cas,
            profile,
        ]
    )
    _parse_pxte(wire)
    return wire


def _parse_pxte(wire: bytes) -> dict[str, Any]:
    if len(wire) > MAX_PXTE_BYTES:
        raise ContractReject("PXTE10 max")
    if len(wire) < PXTE_PREFIX_BYTES + RETAINED_S0_CAS_BYTES + ACTIVE_S1_CAS_BYTES:
        raise ContractReject("PXTE10 truncated")
    cursor = _Cursor(wire)
    if cursor.take(4) != PXTE_MAGIC or cursor.u16() != PXTE_VERSION:
        raise ContractReject("PXTE10 magic/version")
    projection = cursor.take(PXAE_BYTES)
    projection_values = V1._parse_projection(projection)
    topology_digest = cursor.take(32)
    if topology_digest != _topology_compatibility_digest():
        raise ContractReject("PXTE10 topology")
    if cursor.u16() != PXAD_VERSION:
        raise ContractReject("PXTE10 profile version")
    mode = cursor.u8()
    if mode not in {MODE_REMOTE_ACCESS_ACTIVE, MODE_LOCAL_AGENT_ONLY_DEACTIVATE}:
        raise ContractReject("PXTE10 mode")
    if cursor.u8() != 1:
        raise ContractReject("PXTE10 profile presence")
    predecessor_length, profile_length = cursor.u32(), cursor.u32()
    if not 0 < predecessor_length <= MAX_PREDECESSOR_PXTE_BYTES:
        raise ContractReject("PXTE10 predecessor length")
    if not 0 < profile_length <= MAX_PXAD_BYTES:
        raise ContractReject("PXTE10 profile length")
    predecessor = cursor.take(predecessor_length)
    V1.S7._decode_pxte(predecessor)
    retained = _parse_retained_s0_cas(cursor.take(RETAINED_S0_CAS_BYTES))
    expected_s1 = _parse_active_s1_cas(cursor.take(ACTIVE_S1_CAS_BYTES))
    profile = V1._parse_profile(cursor.take(profile_length))
    cursor.finish()
    if profile["target"] != projection_values["target"]:
        raise ContractReject("PXTE10 target/profile")
    if (mode == MODE_REMOTE_ACCESS_ACTIVE) != (expected_s1["present"] == 0):
        raise ContractReject("PXTE10 mode/CAS")
    return {
        "wire": wire,
        "projection": projection,
        "topology_digest": topology_digest,
        "mode": mode,
        "predecessor": predecessor,
        "retained_s0_cas": retained,
        "expected_s1_cas": expected_s1,
        "profile": profile,
        "execution_digest": _digest(PXTE_DOMAIN, [wire]),
    }


def _assignment_digest(pxte: bytes) -> bytes:
    execution = _parse_pxte(pxte)
    return _digest(
        ASSIGNMENT_DOMAIN,
        [V1._digest(V1.PXTA_DOMAIN, [PXTA_ZERO]), execution["execution_digest"]],
    )


def _encode_pxar(envelope: bytes, pxte: bytes) -> bytes:
    wire = b"".join(
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
    _parse_pxar(wire)
    return wire


def _parse_pxar(wire: bytes) -> dict[str, Any]:
    if len(wire) > MAX_PXAR_BYTES:
        raise ContractReject("PXAR11 max")
    if len(wire) < PXAR_HEADER_BYTES:
        raise ContractReject("PXAR11 truncated")
    if wire[:4] != PXAR_MAGIC or struct.unpack_from(">H", wire, 4)[0] != PXAR_VERSION:
        raise ContractReject("PXAR11 magic/version")
    envelope_length, bindings_length, execution_length = struct.unpack_from(">III", wire, 6)
    if envelope_length > MAX_ENVELOPE_BYTES or bindings_length != len(PXTA_ZERO):
        raise ContractReject("PXAR11 nested length")
    if execution_length > MAX_PXTE_BYTES:
        raise ContractReject("PXAR11 PXTE length")
    expected_length = PXAR_HEADER_BYTES + envelope_length + bindings_length + execution_length
    if expected_length != len(wire):
        raise ContractReject("PXAR11 declared length")
    envelope_end = PXAR_HEADER_BYTES + envelope_length
    bindings_end = envelope_end + bindings_length
    envelope_wire = wire[PXAR_HEADER_BYTES:envelope_end]
    envelope = V1.S7.FABRIC.LEGACY._decode_envelope(envelope_wire)
    if wire[envelope_end:bindings_end] != PXTA_ZERO:
        raise ContractReject("PXAR11 nonempty assignments")
    execution = _parse_pxte(wire[bindings_end:])
    assignment = _assignment_digest(execution["wire"])
    if envelope[7] != assignment or envelope[2] != execution["profile"]["target"]:
        raise ContractReject("PXAR11 commitment")
    if int.from_bytes(envelope[31], "big") < execution["profile"]["operation_timeout_nanos"]:
        raise ContractReject("PXAR11 remaining budget shorter than operation timeout")
    return {
        "wire": wire,
        "envelope": envelope,
        "envelope_wire": envelope_wire,
        "execution": execution,
        "assignment_digest": assignment,
        "envelope_request_digest": _digest(
            V1.S7.FABRIC.LEGACY.REQUEST_DIGEST_DOMAIN,
            [envelope_wire],
        ),
        "request_digest": _digest(PXAR_DOMAIN, [wire]),
    }


def _encode_optional_u64(value: int | None) -> bytes:
    return _u8(value is not None) + _u64(value or 0)


def _decode_optional_u64(cursor: _Cursor) -> int | None:
    present, value = cursor.u8(), cursor.u64()
    if present == 0 and value == 0:
        return None
    if present == 1 and value != 0:
        return value
    raise ContractReject("noncanonical optional generation")


def _encode_optional_epoch(value: bytes | None) -> bytes:
    return _u8(value is not None) + (value or bytes(16))


def _decode_optional_epoch(cursor: _Cursor) -> bytes | None:
    present, value = cursor.u8(), cursor.take(16)
    if present == 0 and value == bytes(16):
        return None
    if present == 1 and value != bytes(16):
        return value
    raise ContractReject("noncanonical optional epoch")


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
            request["envelope_request_digest"],
            request["request_digest"],
        ],
    )[:16]


def _terminal_values(request: dict[str, Any], outcome: int) -> dict[str, Any]:
    execution = request["execution"]
    retained = execution["retained_s0_cas"]
    expected_s1 = execution["expected_s1_cas"]
    census_digest = hashlib.sha256(b"t2-proxy-retained-s0-physical-census").digest()
    if outcome == OUTCOME_ACTIVE_READY:
        admitted_at = 1_000_000_000
        state = {
            "outcome": OUTCOME_ACTIVE_READY,
            "lifecycle_effect": LIFECYCLE_MAY_HAVE_STARTED,
            "phase": PHASE_READY_OBSERVATION,
            "head_tag": HEAD_COMMITTED_INCOMING,
            "fabric_generation": retained["fabric_generation"],
            "agent_generation": retained["agent_generation"],
            "access_generation": expected_s1["access_generation_high_water"] + 1,
            "fabric_session_epoch": retained["fabric_session_epoch"],
            "proxy_session_epoch": PROXY_SESSION_EPOCH,
        }
        evidence = {
            "submit_admitted_count": 2,
            "submit_terminalized_count": 2,
            "control_admitted_count": 1,
            "control_terminalized_count": 1,
            "access_generation_high_water": 1,
            "completion_snapshot_sequence": 73,
            "completion_owner_slot_revision": ACTIVE_OWNER_SLOT_REVISION,
            "admitted_at_nanos": admitted_at,
            "selection_observed_at_nanos": admitted_at + 19_000_000_000,
            "queryable_declared_bitmap": EXACT_ROUTE_BITMAP,
            "ingress_fenced_bitmap": 0,
            "worker_joined_bitmap": 0,
            "drain_outcome": DRAIN_NOT_STARTED,
            "remote_observation": OBSERVATION_S1_READY,
            "flags": (
                FLAG_RETAINED_CENSUS_COMPLETE
                | FLAG_RETAINED_S0_READY
                | FLAG_S1_TLS_READY
                | FLAG_S1_ACL_READY
            ),
            "resource_census_digest": hashlib.sha256(b"t2-proxy-active-resources").digest(),
            "raw_outcome_digest": hashlib.sha256(b"t2-proxy-active-ready").digest(),
        }
    elif outcome == OUTCOME_LOCAL_ONLY_READY:
        admitted_at = 30_000_000_000
        state = {
            "outcome": OUTCOME_LOCAL_ONLY_READY,
            "lifecycle_effect": LIFECYCLE_MAY_HAVE_STARTED,
            "phase": PHASE_LOCAL_ONLY_OBSERVATION,
            "head_tag": HEAD_COMMITTED_INCOMING,
            "fabric_generation": retained["fabric_generation"],
            "agent_generation": retained["agent_generation"],
            "access_generation": None,
            "fabric_session_epoch": retained["fabric_session_epoch"],
            "proxy_session_epoch": None,
        }
        evidence = {
            "submit_admitted_count": 3,
            "submit_terminalized_count": 3,
            "control_admitted_count": 2,
            "control_terminalized_count": 2,
            "access_generation_high_water": expected_s1["access_generation_high_water"],
            "completion_snapshot_sequence": 74,
            "completion_owner_slot_revision": LOCAL_OWNER_SLOT_REVISION,
            "admitted_at_nanos": admitted_at,
            "selection_observed_at_nanos": admitted_at + 21_000_000_000,
            "queryable_declared_bitmap": EXACT_ROUTE_BITMAP,
            "ingress_fenced_bitmap": EXACT_ROUTE_BITMAP,
            "worker_joined_bitmap": EXACT_ROUTE_BITMAP,
            "drain_outcome": DRAIN_DRAINED,
            "remote_observation": OBSERVATION_S1_ABSENT,
            "flags": (
                FLAG_RETAINED_CENSUS_COMPLETE
                | FLAG_RETAINED_S0_READY
                | FLAG_S1_CLOSED
                | FLAG_S1_LISTENER_RELEASED
            ),
            "resource_census_digest": hashlib.sha256(b"t2-proxy-local-resources").digest(),
            "raw_outcome_digest": hashlib.sha256(b"t2-proxy-local-only-ready").digest(),
        }
    else:
        raise AssertionError("oracle freezes only ActiveReady and LocalOnlyReady")
    evidence.update(
        {
            "retained_s0_current_cas_digest": retained["digest"],
            "retained_s0_census_before_digest": census_digest,
            "retained_s0_census_after_digest": census_digest,
            "topology_digest": execution["topology_digest"],
            "completion_runtime_host_epoch": RUNTIME_HOST_EPOCH,
            "selection_clock_domain": request["envelope"][28],
            "selection_clock_generation": int.from_bytes(request["envelope"][29], "big"),
            "absolute_deadline_nanos": (
                admitted_at + execution["profile"]["operation_timeout_nanos"]
            ),
            "physical_binding_census": 2,
        }
    )
    return {
        "target": request["envelope"][2],
        "store": request["envelope"][32],
        "source_scope": request["envelope"][3],
        "operation_id": request["envelope"][24],
        "envelope_request_digest": request["envelope_request_digest"],
        "request_digest": request["request_digest"],
        "target_slice_digest": request["envelope"][8],
        "assignment_digest": request["assignment_digest"],
        "result_ref": _terminal_result_ref(request),
        "request_mode": execution["mode"],
        "state": state,
        "desired_head_digest": request["envelope"][8],
        "evidence": evidence,
        "runtime_principal": RUNTIME_PRINCIPAL,
        "runtime_key": RUNTIME_KEY_REF,
        "algorithm": 1,
        "algorithm_version": 1,
    }


def _terminal_body(values: dict[str, Any]) -> bytes:
    state = values["state"]
    evidence = values["evidence"]
    body = bytearray()
    for key in (
        "target",
        "store",
        "source_scope",
        "operation_id",
        "envelope_request_digest",
        "request_digest",
        "target_slice_digest",
        "assignment_digest",
        "result_ref",
    ):
        body += values[key]
    body += _u8(values["request_mode"])
    body += _u8(state["outcome"])
    body += _u8(state["lifecycle_effect"])
    body += _u8(state["phase"])
    body += _u8(state["head_tag"])
    body += _u8(1)
    body += _u16(0)
    body += values["desired_head_digest"]
    body += _encode_optional_u64(state["fabric_generation"])
    body += _encode_optional_u64(state["agent_generation"])
    body += _encode_optional_u64(state["access_generation"])
    body += _encode_optional_epoch(state["fabric_session_epoch"])
    body += _encode_optional_epoch(state["proxy_session_epoch"])
    for key in (
        "retained_s0_current_cas_digest",
        "retained_s0_census_before_digest",
        "retained_s0_census_after_digest",
        "topology_digest",
        "resource_census_digest",
        "raw_outcome_digest",
    ):
        body += evidence[key]
    for key in (
        "submit_admitted_count",
        "submit_terminalized_count",
        "control_admitted_count",
        "control_terminalized_count",
        "access_generation_high_water",
        "completion_runtime_host_epoch",
        "completion_snapshot_sequence",
        "completion_owner_slot_revision",
    ):
        body += _u64(evidence[key])
    body += evidence["selection_clock_domain"]
    for key in (
        "selection_clock_generation",
        "admitted_at_nanos",
        "absolute_deadline_nanos",
        "selection_observed_at_nanos",
    ):
        body += _u64(evidence[key])
    body += _u16(evidence["physical_binding_census"])
    body += _u8(evidence["queryable_declared_bitmap"])
    body += _u8(evidence["ingress_fenced_bitmap"])
    body += _u8(evidence["worker_joined_bitmap"])
    body += _u8(evidence["drain_outcome"])
    body += _u8(evidence["remote_observation"])
    body += _u8(0)
    body += _u16(evidence["flags"])
    body += values["runtime_principal"]
    body += values["runtime_key"]
    body += _u16(values["algorithm"])
    body += _u16(values["algorithm_version"])
    if len(body) != PXAU_FIXED_BYTES - 8:
        raise AssertionError("PXAU v2 body width")
    return bytes(body)


def _encode_pxau(request_wire: bytes, outcome: int) -> dict[str, Any]:
    request = _parse_pxar(request_wire)
    values = _terminal_values(request, outcome)
    body = _terminal_body(values)
    transcript = PXAU_SIGNING_MAGIC + _u16(PXAU_SIGNING_VERSION) + body
    signature = _private(RUNTIME_SEED).sign(transcript)
    wire = PXAU_MAGIC + _u16(PXAU_VERSION) + body + _u16(len(signature)) + signature
    if len(wire) != PXAU_CANONICAL_BYTES:
        raise AssertionError("PXAU v2 canonical width")
    parsed = _parse_pxau(wire, request_wire, _public(RUNTIME_SEED))
    return {
        "wire": wire,
        "digest": parsed["digest"],
        "transcript": transcript,
        "signature": signature,
        "public_key": _public(RUNTIME_SEED),
        "values": values,
    }


def _parse_terminal_body(body: bytes) -> dict[str, Any]:
    cursor = _Cursor(body)
    values = {
        "target": cursor.take(16),
        "store": cursor.take(32),
        "source_scope": cursor.take(16),
        "operation_id": cursor.take(16),
        "envelope_request_digest": cursor.take(32),
        "request_digest": cursor.take(32),
        "target_slice_digest": cursor.take(32),
        "assignment_digest": cursor.take(32),
        "result_ref": cursor.take(16),
        "request_mode": cursor.u8(),
    }
    state = {
        "outcome": cursor.u8(),
        "lifecycle_effect": cursor.u8(),
        "phase": cursor.u8(),
        "head_tag": cursor.u8(),
    }
    desired_present = cursor.u8()
    if cursor.u16() != 0:
        raise ContractReject("PXAU v2 reserved u16")
    desired_head_digest = cursor.take(32)
    if desired_present != 1 or desired_head_digest == bytes(32):
        raise ContractReject("PXAU v2 desired head")
    state.update(
        {
            "fabric_generation": _decode_optional_u64(cursor),
            "agent_generation": _decode_optional_u64(cursor),
            "access_generation": _decode_optional_u64(cursor),
            "fabric_session_epoch": _decode_optional_epoch(cursor),
            "proxy_session_epoch": _decode_optional_epoch(cursor),
        }
    )
    evidence = {}
    for key in (
        "retained_s0_current_cas_digest",
        "retained_s0_census_before_digest",
        "retained_s0_census_after_digest",
        "topology_digest",
        "resource_census_digest",
        "raw_outcome_digest",
    ):
        evidence[key] = cursor.take(32)
    for key in (
        "submit_admitted_count",
        "submit_terminalized_count",
        "control_admitted_count",
        "control_terminalized_count",
        "access_generation_high_water",
        "completion_runtime_host_epoch",
        "completion_snapshot_sequence",
        "completion_owner_slot_revision",
    ):
        evidence[key] = cursor.u64()
    evidence["selection_clock_domain"] = cursor.take(16)
    for key in (
        "selection_clock_generation",
        "admitted_at_nanos",
        "absolute_deadline_nanos",
        "selection_observed_at_nanos",
    ):
        evidence[key] = cursor.u64()
    evidence.update(
        {
            "physical_binding_census": cursor.u16(),
            "queryable_declared_bitmap": cursor.u8(),
            "ingress_fenced_bitmap": cursor.u8(),
            "worker_joined_bitmap": cursor.u8(),
            "drain_outcome": cursor.u8(),
            "remote_observation": cursor.u8(),
        }
    )
    if cursor.u8() != 0:
        raise ContractReject("PXAU v2 reserved u8")
    evidence["flags"] = cursor.u16()
    values.update(
        {
            "state": state,
            "desired_head_digest": desired_head_digest,
            "evidence": evidence,
            "runtime_principal": cursor.take(16),
            "runtime_key": cursor.take(16),
            "algorithm": cursor.u16(),
            "algorithm_version": cursor.u16(),
        }
    )
    cursor.finish()
    return values


def _terminal_selection_time_is_valid(
    outcome: int,
    admitted_at_nanos: int,
    absolute_deadline_nanos: int,
    selection_observed_at_nanos: int,
) -> bool:
    if selection_observed_at_nanos < admitted_at_nanos:
        return False
    if outcome == OUTCOME_ACTIVE_READY:
        return selection_observed_at_nanos < absolute_deadline_nanos
    return outcome in {
        OUTCOME_LOCAL_ONLY_READY,
        OUTCOME_NO_EFFECT_REJECTED,
        OUTCOME_UNCERTAIN,
        OUTCOME_QUARANTINED,
    }


def _validate_terminal_values(values: dict[str, Any], request: dict[str, Any]) -> None:
    execution = request["execution"]
    retained = execution["retained_s0_cas"]
    expected_s1 = execution["expected_s1_cas"]
    state = values["state"]
    evidence = values["evidence"]
    correlations = {
        "target": request["envelope"][2],
        "store": request["envelope"][32],
        "source_scope": request["envelope"][3],
        "operation_id": request["envelope"][24],
        "envelope_request_digest": request["envelope_request_digest"],
        "request_digest": request["request_digest"],
        "target_slice_digest": request["envelope"][8],
        "assignment_digest": request["assignment_digest"],
        "result_ref": _terminal_result_ref(request),
        "request_mode": execution["mode"],
        "desired_head_digest": request["envelope"][8],
    }
    if any(values[key] != expected for key, expected in correlations.items()):
        raise ContractReject("PXAU v2 request correlation")
    if (
        state["head_tag"] != HEAD_COMMITTED_INCOMING
        or state["lifecycle_effect"] != LIFECYCLE_MAY_HAVE_STARTED
        or state["fabric_generation"] != retained["fabric_generation"]
        or state["agent_generation"] != retained["agent_generation"]
        or state["fabric_session_epoch"] != retained["fabric_session_epoch"]
    ):
        raise ContractReject("PXAU v2 state correlation")
    digest_keys = (
        "retained_s0_current_cas_digest",
        "retained_s0_census_before_digest",
        "retained_s0_census_after_digest",
        "topology_digest",
        "resource_census_digest",
        "raw_outcome_digest",
    )
    if any(evidence[key] == bytes(32) for key in digest_keys):
        raise ContractReject("PXAU v2 zero evidence")
    if (
        evidence["retained_s0_current_cas_digest"] != retained["digest"]
        or evidence["retained_s0_census_before_digest"]
        != evidence["retained_s0_census_after_digest"]
        or evidence["topology_digest"] != execution["topology_digest"]
        or evidence["submit_terminalized_count"] > evidence["submit_admitted_count"]
        or evidence["control_terminalized_count"] > evidence["control_admitted_count"]
        or evidence["completion_runtime_host_epoch"] == 0
        or evidence["completion_snapshot_sequence"] == 0
        or evidence["completion_owner_slot_revision"] == 0
        or evidence["selection_clock_domain"] == bytes(16)
        or evidence["selection_clock_domain"] != request["envelope"][28]
        or evidence["selection_clock_generation"] != int.from_bytes(request["envelope"][29], "big")
        or evidence["admitted_at_nanos"] == 0
        or evidence["absolute_deadline_nanos"]
        != evidence["admitted_at_nanos"] + execution["profile"]["operation_timeout_nanos"]
        or evidence["selection_observed_at_nanos"] == 0
        or not _terminal_selection_time_is_valid(
            state["outcome"],
            evidence["admitted_at_nanos"],
            evidence["absolute_deadline_nanos"],
            evidence["selection_observed_at_nanos"],
        )
        or evidence["physical_binding_census"] != 2
        or evidence["queryable_declared_bitmap"] & ~EXACT_ROUTE_BITMAP
        or evidence["ingress_fenced_bitmap"] & ~EXACT_ROUTE_BITMAP
        or evidence["worker_joined_bitmap"] & ~EXACT_ROUTE_BITMAP
        or evidence["flags"] & ~KNOWN_FLAGS
        or not evidence["flags"] & FLAG_RETAINED_CENSUS_COMPLETE
        or not evidence["flags"] & FLAG_RETAINED_S0_READY
    ):
        raise ContractReject("PXAU v2 evidence correlation")
    if state["outcome"] == OUTCOME_ACTIVE_READY:
        if (
            execution["mode"] != MODE_REMOTE_ACCESS_ACTIVE
            or state["phase"] != PHASE_READY_OBSERVATION
            or expected_s1["present"] != 0
            or state["access_generation"] != expected_s1["access_generation_high_water"] + 1
            or state["proxy_session_epoch"] is None
            or evidence["access_generation_high_water"]
            != expected_s1["access_generation_high_water"] + 1
            or evidence["completion_owner_slot_revision"] != expected_s1["owner_slot_revision"] + 1
            or evidence["queryable_declared_bitmap"] != EXACT_ROUTE_BITMAP
            or evidence["ingress_fenced_bitmap"] != 0
            or evidence["worker_joined_bitmap"] != 0
            or evidence["drain_outcome"] != DRAIN_NOT_STARTED
            or evidence["remote_observation"] != OBSERVATION_S1_READY
            or evidence["flags"]
            != (
                FLAG_RETAINED_CENSUS_COMPLETE
                | FLAG_RETAINED_S0_READY
                | FLAG_S1_TLS_READY
                | FLAG_S1_ACL_READY
            )
        ):
            raise ContractReject("PXAU v2 ActiveReady shape")
    elif state["outcome"] == OUTCOME_LOCAL_ONLY_READY:
        if (
            execution["mode"] != MODE_LOCAL_AGENT_ONLY_DEACTIVATE
            or state["phase"] != PHASE_LOCAL_ONLY_OBSERVATION
            or expected_s1["present"] != 1
            or state["access_generation"] is not None
            or state["proxy_session_epoch"] is not None
            or evidence["access_generation_high_water"]
            != expected_s1["access_generation_high_water"]
            or evidence["completion_owner_slot_revision"] != expected_s1["owner_slot_revision"] + 1
            or evidence["queryable_declared_bitmap"] != EXACT_ROUTE_BITMAP
            or evidence["ingress_fenced_bitmap"] != EXACT_ROUTE_BITMAP
            or evidence["worker_joined_bitmap"] != EXACT_ROUTE_BITMAP
            or evidence["drain_outcome"] != DRAIN_DRAINED
            or evidence["remote_observation"] != OBSERVATION_S1_ABSENT
            or evidence["submit_admitted_count"] != evidence["submit_terminalized_count"]
            or evidence["control_admitted_count"] != evidence["control_terminalized_count"]
            or evidence["flags"]
            != (
                FLAG_RETAINED_CENSUS_COMPLETE
                | FLAG_RETAINED_S0_READY
                | FLAG_S1_CLOSED
                | FLAG_S1_LISTENER_RELEASED
            )
        ):
            raise ContractReject("PXAU v2 LocalOnlyReady shape")
    else:
        raise ContractReject("oracle admits only frozen success outcomes")
    if (
        values["runtime_principal"] == bytes(16)
        or values["runtime_key"] == bytes(16)
        or values["algorithm"] != 1
        or values["algorithm_version"] != 1
    ):
        raise ContractReject("PXAU v2 authentication")


def _parse_pxau(wire: bytes, request_wire: bytes, runtime_public: bytes) -> dict[str, Any]:
    if len(wire) > MAX_PXAU_CARRIER_BYTES:
        raise ContractReject("PXAU v2 defensive max")
    if len(wire) < PXAU_FIXED_BYTES:
        raise ContractReject("PXAU v2 truncated")
    cursor = _Cursor(wire)
    if cursor.take(4) != PXAU_MAGIC or cursor.u16() != PXAU_VERSION:
        raise ContractReject("PXAU v2 magic/version")
    body = cursor.take(PXAU_FIXED_BYTES - 8)
    values = _parse_terminal_body(body)
    signature_length = cursor.u16()
    if not 0 < signature_length <= MAX_PXAU_SIGNATURE_BYTES:
        raise ContractReject("PXAU v2 signature length")
    signature = cursor.take(signature_length)
    cursor.finish()
    request = _parse_pxar(request_wire)
    _validate_terminal_values(values, request)
    transcript = PXAU_SIGNING_MAGIC + _u16(PXAU_SIGNING_VERSION) + body
    try:
        Ed25519PublicKey.from_public_bytes(runtime_public).verify(signature, transcript)
    except (InvalidSignature, ValueError) as error:
        raise ContractReject("PXAU v2 signature") from error
    return {
        "wire": wire,
        "values": values,
        "transcript": transcript,
        "signature": signature,
        "digest": _digest(PXAU_DOMAIN, [wire]),
    }


def _replace(wire: bytes, offset: int, value: bytes) -> bytes:
    return wire[:offset] + value + wire[offset + len(value) :]


def _resign_pxau(wire: bytes) -> bytes:
    if len(wire) != PXAU_CANONICAL_BYTES or wire[681:683] != _u16(PXAU_SIGNATURE_BYTES):
        raise AssertionError("resign expects canonical 64-byte PXAU v2")
    body = wire[6:681]
    transcript = PXAU_SIGNING_MAGIC + _u16(PXAU_SIGNING_VERSION) + body
    signature = _private(RUNTIME_SEED).sign(transcript)
    return wire[:683] + signature


def _retime_envelope(
    envelope: dict[str, bytes],
    *,
    original_budget_nanos: int = ENVELOPE_ORIGINAL_BUDGET_NANOS,
    remaining_budget_nanos: int = ENVELOPE_REMAINING_BUDGET_NANOS,
) -> dict[str, bytes]:
    legacy = V1.S7.FABRIC.LEGACY
    wire = legacy._rebuild_envelope_after_field_changes(
        envelope["wire"],
        {
            30: _u64(original_budget_nanos),
            31: _u64(remaining_budget_nanos),
        },
    )
    values = legacy._decode_envelope(wire)
    signing_transcript = legacy._signing_transcript(
        2,
        legacy.AUTH_SIGNING_DOMAIN,
        [(tag, values[tag]) for tag in range(1, 38)],
    )
    return {
        **envelope,
        "wire": wire,
        "target_slice_digest": values[8],
        "control_digest": values[25],
        "request_signature": values[38],
        "signing_transcript": signing_transcript,
        "request_digest": _digest(legacy.REQUEST_DIGEST_DOMAIN, [wire]),
    }


def _build_request(
    *,
    mode: int,
    retained_s0_cas: bytes,
    expected_s1_cas: bytes,
    operation_byte: str,
    temporal_byte: str,
    auth_nonce: bytes,
) -> dict[str, Any]:
    predecessor = _predecessor_vectors()
    profile = V1._profile()
    projection = V1._projection(predecessor["projection"])
    pxte = _encode_pxte(
        mode=mode,
        projection=projection,
        predecessor=predecessor["active"]["pxte"],
        retained_s0_cas=retained_s0_cas,
        expected_s1_cas=expected_s1_cas,
        profile=profile["wire"],
    )
    assignment = _assignment_digest(pxte)
    envelope = V1.S7.FABRIC.LEGACY._build_envelope(
        assignment,
        source_revision=223,
        operation_byte=operation_byte,
        temporal_byte=temporal_byte,
        auth_nonce=auth_nonce,
    )
    envelope = _retime_envelope(envelope)
    Ed25519PublicKey.from_public_bytes(envelope["request_public_key"]).verify(
        envelope["request_signature"],
        envelope["signing_transcript"],
    )
    pxar = _encode_pxar(envelope["wire"], pxte)
    parsed = _parse_pxar(pxar)
    return {
        "profile": profile,
        "projection": projection,
        "pxte": pxte,
        "assignment_digest": assignment,
        "envelope": envelope,
        "pxar": pxar,
        "parsed": parsed,
    }


@lru_cache(maxsize=1)
def _vectors() -> dict[str, Any]:
    retained_values = _retained_s0_values()
    retained_wire = _encode_retained_s0_cas(retained_values)
    retained = _parse_retained_s0_cas(retained_wire)
    absent_wire = _encode_active_s1_cas_absent(0, INITIAL_OWNER_SLOT_REVISION)
    active_ready = _build_request(
        mode=MODE_REMOTE_ACCESS_ACTIVE,
        retained_s0_cas=retained_wire,
        expected_s1_cas=absent_wire,
        operation_byte="b4",
        temporal_byte="b5",
        auth_nonce=b"t2-proxy-active-inner",
    )
    active_terminal = _encode_pxau(active_ready["pxar"], OUTCOME_ACTIVE_READY)
    active_snapshot_digest = hashlib.sha256(
        b"PXRS-v2-current-active-slot" + active_terminal["wire"]
    ).digest()
    active_s1_wire = _encode_active_s1_cas_active(
        1,
        ACTIVE_OWNER_SLOT_REVISION,
        pxau_digest=active_terminal["digest"],
        request_digest=active_ready["parsed"]["request_digest"],
        snapshot_digest=active_snapshot_digest,
        snapshot_sequence=73,
        access_generation=1,
        proxy_session_epoch=PROXY_SESSION_EPOCH,
    )
    local_only = _build_request(
        mode=MODE_LOCAL_AGENT_ONLY_DEACTIVATE,
        retained_s0_cas=retained_wire,
        expected_s1_cas=active_s1_wire,
        operation_byte="b6",
        temporal_byte="b7",
        auth_nonce=b"t2-proxy-local-only-inner",
    )
    local_terminal = _encode_pxau(local_only["pxar"], OUTCOME_LOCAL_ONLY_READY)
    return {
        "retained_values": retained_values,
        "retained": retained,
        "active_ready": {
            **active_ready,
            "expected_s1": _parse_active_s1_cas(absent_wire),
            "terminal": active_terminal,
        },
        "local_only_ready": {
            **local_only,
            "expected_s1": _parse_active_s1_cas(active_s1_wire),
            "terminal": local_terminal,
        },
        "active_snapshot_digest": active_snapshot_digest,
    }


def _wire_entry(wire: bytes, digest: bytes) -> dict[str, Any]:
    return {
        "wire_hex": wire.hex(),
        "wire_length": len(wire),
        "digest_hex": digest.hex(),
    }


def _cas_entry(value: dict[str, Any]) -> dict[str, Any]:
    return _wire_entry(value["wire"], value["digest"])


def _envelope_entry(value: dict[str, bytes]) -> dict[str, Any]:
    return {
        "wire_hex": value["wire"].hex(),
        "wire_length": len(value["wire"]),
        "request_digest_hex": value["request_digest"].hex(),
        "target_slice_digest_hex": value["target_slice_digest"].hex(),
        "signing_transcript_hex": value["signing_transcript"].hex(),
        "signing_transcript_sha256_hex": hashlib.sha256(value["signing_transcript"]).hexdigest(),
        "signature_hex": value["request_signature"].hex(),
        "public_key_hex": value["request_public_key"].hex(),
    }


def _terminal_entry(value: dict[str, Any]) -> dict[str, Any]:
    return {
        **_wire_entry(value["wire"], value["digest"]),
        "signing_transcript_hex": value["transcript"].hex(),
        "signing_transcript_length": len(value["transcript"]),
        "signing_transcript_sha256_hex": hashlib.sha256(value["transcript"]).hexdigest(),
        "signature_hex": value["signature"].hex(),
        "public_key_hex": value["public_key"].hex(),
    }


def _scenario_entry(value: dict[str, Any]) -> dict[str, Any]:
    parsed = value["parsed"]
    return {
        "expected_s1_cas": _cas_entry(value["expected_s1"]),
        "pxte_v10": _wire_entry(
            value["pxte"],
            _digest(PXTE_DOMAIN, [value["pxte"]]),
        ),
        "assignment_v11_digest_hex": value["assignment_digest"].hex(),
        "envelope_v2": _envelope_entry(value["envelope"]),
        "pxar_v11": _wire_entry(value["pxar"], parsed["request_digest"]),
        "pxau_v2": _terminal_entry(value["terminal"]),
    }


def _generated_fixture() -> dict[str, Any]:
    vectors = _vectors()
    retained_values = vectors["retained_values"]
    return {
        "format": "paraegox-t2-remote-agent-proxy-data-plane-v2",
        "source": "independent Python struct/hashlib/cryptography T2-B2 oracle",
        "rust_source_freeze": {
            "ref": RUST_SOURCE_REF,
            "commit": RUST_SOURCE_COMMIT,
            "remote_agent_data_plane_plan.rs_sha256": RUST_SOURCE_SHA256,
        },
        "semantic_constants": {
            "retained_s0_cas_bytes": RETAINED_S0_CAS_BYTES,
            "active_s1_cas_bytes": ACTIVE_S1_CAS_BYTES,
            "pxte_v10_prefix_bytes": PXTE_PREFIX_BYTES,
            "max_pxte_v10_bytes": MAX_PXTE_BYTES,
            "pxar_v11_header_bytes": PXAR_HEADER_BYTES,
            "max_pxar_v11_bytes": MAX_PXAR_BYTES,
            "pxau_v2_fixed_bytes": PXAU_FIXED_BYTES,
            "pxau_v2_signature_bytes": PXAU_SIGNATURE_BYTES,
            "pxau_v2_canonical_bytes": PXAU_CANONICAL_BYTES,
            "max_pxau_v2_signature_bytes": MAX_PXAU_SIGNATURE_BYTES,
            "max_canonical_pxau_v2_bytes": MAX_CANONICAL_PXAU_BYTES,
            "max_pxau_v2_carrier_bytes": MAX_PXAU_CARRIER_BYTES,
            "terminal_transcript_bytes": len(
                PXAU_SIGNING_MAGIC + _u16(PXAU_SIGNING_VERSION) + bytes(675)
            ),
            "exact_route_count": EXACT_ROUTE_COUNT,
            "exact_route_bitmap": EXACT_ROUTE_BITMAP,
            "proxy_queue_capacity": QUEUE_CAPACITY,
            "proxy_workers_per_route": WORKERS_PER_ROUTE,
            "proxy_max_pending_sessions": MAX_PENDING_SESSIONS,
            "proxy_max_sessions": MAX_SESSIONS,
            "proxy_max_links": MAX_LINKS,
            "proxy_max_message_bytes": MAX_MESSAGE_BYTES,
            "profile_operation_timeout_nanos": PROFILE_OPERATION_TIMEOUT_NANOS,
            "envelope_original_budget_nanos": ENVELOPE_ORIGINAL_BUDGET_NANOS,
            "envelope_remaining_budget_nanos": ENVELOPE_REMAINING_BUDGET_NANOS,
            "hardening_features": HARDENING_FEATURES,
            "temporal_authority_rule_hex": TEMPORAL_AUTHORITY_RULE.hex(),
            "retained_s0_offsets": {
                "active_pxft_digest": 0,
                "active_pxst_digest": 32,
                "descriptor_record_digest": 64,
                "descriptor_record_sequence": 96,
                "descriptor_receipt_digest": 104,
                "descriptor_payload_digest": 136,
                "fabric_session_epoch": 168,
                "fabric_generation": 184,
                "agent_generation": 192,
            },
            "active_s1_offsets": {
                "present": 0,
                "reserved": 1,
                "access_generation_high_water": 8,
                "owner_slot_revision": 16,
                "active_pxau_digest": 24,
                "active_request_digest": 56,
                "active_snapshot_digest": 88,
                "active_snapshot_sequence": 120,
                "active_access_generation": 128,
                "active_proxy_session_epoch": 136,
            },
            "pxte_v10_offsets": {
                "magic": 0,
                "version": 4,
                "pxae_v1": 6,
                "topology_digest": 276,
                "pxad_version": 308,
                "mode": 310,
                "profile_present": 311,
                "predecessor_length": 312,
                "profile_length": 316,
                "predecessor": 320,
            },
            "pxar_v11_offsets": {
                "magic": 0,
                "version": 4,
                "envelope_length": 6,
                "pxta_length": 10,
                "pxte_length": 14,
                "envelope": 18,
            },
            "pxau_v2_offsets": PXAU_OFFSETS,
        },
        "domains": {
            "topology_compatibility_hex": TOPOLOGY_DOMAIN.hex(),
            "pxte_v10_digest_hex": PXTE_DOMAIN.hex(),
            "assignment_v11_digest_hex": ASSIGNMENT_DOMAIN.hex(),
            "pxar_v11_digest_hex": PXAR_DOMAIN.hex(),
            "retained_s0_cas_digest_hex": RETAINED_S0_CAS_DOMAIN.hex(),
            "active_s1_cas_digest_hex": ACTIVE_S1_CAS_DOMAIN.hex(),
            "pxau_v2_signing_magic_hex": PXAU_SIGNING_MAGIC.hex(),
            "pxau_v2_result_ref_hex": PXAU_RESULT_REF_DOMAIN.hex(),
            "pxau_v2_digest_hex": PXAU_DOMAIN.hex(),
        },
        "topology_compatibility_digest_hex": _topology_compatibility_digest().hex(),
        "runtime_signing_public_key_hex": _public(RUNTIME_SEED).hex(),
        "retained_s0_cas": {
            **_cas_entry(vectors["retained"]),
            "bootstrap_pxap_hex": retained_values["bootstrap_pxap"].hex(),
            "bootstrap_pxah_hex": retained_values["bootstrap_pxah"].hex(),
        },
        "active_ready": _scenario_entry(vectors["active_ready"]),
        "local_only_ready": _scenario_entry(vectors["local_only_ready"]),
    }


def test_semantic_constants_and_r244_source_freeze() -> None:
    assert hashlib.sha256(RUST_SOURCE_PATH.read_bytes()).hexdigest() == RUST_SOURCE_SHA256
    assert PXTE_PREFIX_BYTES == 320
    assert MAX_PXTE_BYTES == 320 + 1_465 + 200 + 152 + 439
    assert MAX_PXAR_BYTES == 18 + 4_096 + 10 + 2_576
    assert PXAU_FIXED_BYTES == PXAU_OFFSETS["signature"]
    assert PXAU_CANONICAL_BYTES == PXAU_FIXED_BYTES + PXAU_SIGNATURE_BYTES
    assert MAX_PXAU_SIGNATURE_BYTES == 512
    assert MAX_CANONICAL_PXAU_BYTES == PXAU_FIXED_BYTES + MAX_PXAU_SIGNATURE_BYTES
    assert MAX_CANONICAL_PXAU_BYTES < MAX_PXAU_CARRIER_BYTES
    assert PROFILE_OPERATION_TIMEOUT_NANOS == 20_000_000_000
    assert ENVELOPE_REMAINING_BUDGET_NANOS >= PROFILE_OPERATION_TIMEOUT_NANOS
    assert len(PXAU_SIGNING_MAGIC + _u16(2) + bytes(675)) == 732
    assert _topology_compatibility_digest().hex() == (
        "b3e79f480ecf52b1bcbde52d53c092a5814b5c9e344eb336b3ca0977e3ab243a"
    )


def test_independent_python_oracle_matches_checked_in_golden() -> None:
    assert json.loads(FIXTURE_PATH.read_text()) == _generated_fixture()


def test_active_ready_and_local_only_ready_round_trip_and_signatures() -> None:
    vectors = _vectors()
    for name, expected_mode, expected_outcome in (
        ("active_ready", MODE_REMOTE_ACCESS_ACTIVE, OUTCOME_ACTIVE_READY),
        (
            "local_only_ready",
            MODE_LOCAL_AGENT_ONLY_DEACTIVATE,
            OUTCOME_LOCAL_ONLY_READY,
        ),
    ):
        value = vectors[name]
        parsed_request = _parse_pxar(value["pxar"])
        parsed_terminal = _parse_pxau(
            value["terminal"]["wire"],
            value["pxar"],
            value["terminal"]["public_key"],
        )
        assert parsed_request["execution"]["mode"] == expected_mode
        assert (
            int.from_bytes(parsed_request["envelope"][30], "big") == ENVELOPE_ORIGINAL_BUDGET_NANOS
        )
        assert (
            int.from_bytes(parsed_request["envelope"][31], "big") == ENVELOPE_REMAINING_BUDGET_NANOS
        )
        assert (
            parsed_request["execution"]["profile"]["operation_timeout_nanos"]
            == PROFILE_OPERATION_TIMEOUT_NANOS
        )
        assert parsed_terminal["values"]["state"]["outcome"] == expected_outcome
        assert len(parsed_terminal["wire"]) == PXAU_CANONICAL_BYTES
        Ed25519PublicKey.from_public_bytes(value["envelope"]["request_public_key"]).verify(
            value["envelope"]["request_signature"],
            value["envelope"]["signing_transcript"],
        )
        Ed25519PublicKey.from_public_bytes(value["terminal"]["public_key"]).verify(
            value["terminal"]["signature"],
            value["terminal"]["transcript"],
        )
    local_evidence = vectors["local_only_ready"]["terminal"]["values"]["evidence"]
    assert (
        local_evidence["selection_observed_at_nanos"]
        == local_evidence["admitted_at_nanos"] + 21_000_000_000
        > local_evidence["absolute_deadline_nanos"]
    )


def test_pxar_v11_remaining_budget_must_cover_operation_timeout() -> None:
    value = _vectors()["active_ready"]
    legacy = V1.S7.FABRIC.LEGACY

    def with_remaining_budget(remaining_budget_nanos: int) -> bytes:
        envelope = legacy._rebuild_envelope_after_field_changes(
            value["envelope"]["wire"],
            {31: _u64(remaining_budget_nanos)},
        )
        assert len(envelope) == len(value["envelope"]["wire"])
        return _replace(value["pxar"], PXAR_HEADER_BYTES, envelope)

    _parse_pxar(with_remaining_budget(PROFILE_OPERATION_TIMEOUT_NANOS))
    short_budget = with_remaining_budget(PROFILE_OPERATION_TIMEOUT_NANOS - 1)
    short_envelope_length = struct.unpack_from(">I", short_budget, 6)[0]
    short_envelope = short_budget[PXAR_HEADER_BYTES : PXAR_HEADER_BYTES + short_envelope_length]
    short_values = legacy._decode_envelope(short_envelope)
    legacy._verify_envelope_signatures(
        short_values,
        value["envelope"]["tenure_public_key"],
        value["envelope"]["request_public_key"],
    )
    with pytest.raises(ContractReject):
        _parse_pxar(short_budget)


def test_cas_width_revision_high_water_and_retained_s0_are_strict() -> None:
    vectors = _vectors()
    retained = vectors["retained"]
    absent = vectors["active_ready"]["expected_s1"]
    active = vectors["local_only_ready"]["expected_s1"]
    assert len(retained["wire"]) == RETAINED_S0_CAS_BYTES
    assert len(absent["wire"]) == len(active["wire"]) == ACTIVE_S1_CAS_BYTES
    assert absent["access_generation_high_water"] == 0
    assert absent["owner_slot_revision"] == INITIAL_OWNER_SLOT_REVISION
    assert active["access_generation_high_water"] == active["active_access_generation"] == 1
    assert active["owner_slot_revision"] == ACTIVE_OWNER_SLOT_REVISION
    assert active["active_pxau_digest"] == vectors["active_ready"]["terminal"]["digest"]
    assert active["active_request_digest"] == vectors["active_ready"]["parsed"]["request_digest"]
    assert (
        vectors["active_ready"]["parsed"]["execution"]["retained_s0_cas"]["wire"]
        == vectors["local_only_ready"]["parsed"]["execution"]["retained_s0_cas"]["wire"]
    )
    with pytest.raises(ContractReject):
        _parse_retained_s0_cas(_replace(retained["wire"], 96, bytes(8)))
    with pytest.raises(ContractReject):
        _parse_active_s1_cas(_replace(absent["wire"], 16, bytes(8)))
    with pytest.raises(ContractReject):
        _parse_active_s1_cas(_replace(active["wire"], 1, b"\x01"))
    with pytest.raises(ContractReject):
        _parse_active_s1_cas(_replace(active["wire"], 128, _u64(2)))


@pytest.mark.parametrize("name", ["active_ready", "local_only_ready"])
def test_terminal_hwm_revision_deadline_and_s0_drift_fail_closed(name: str) -> None:
    value = _vectors()[name]
    wire = value["terminal"]["wire"]
    request = value["pxar"]
    public = value["terminal"]["public_key"]
    mutations = [
        (PXAU_OFFSETS["access_generation_high_water"], _u64(99)),
        (PXAU_OFFSETS["completion_owner_slot_revision"], _u64(99)),
        (PXAU_OFFSETS["absolute_deadline_nanos"], _u64(99)),
        (PXAU_OFFSETS["retained_s0_current_cas_digest"], bytes.fromhex("ed" * 32)),
        (PXAU_OFFSETS["retained_s0_census_after_digest"], bytes.fromhex("ee" * 32)),
    ]
    for offset, replacement in mutations:
        tampered = _resign_pxau(_replace(wire, offset, replacement))
        with pytest.raises(ContractReject):
            _parse_pxau(tampered, request, public)


def test_terminal_temporal_authority_is_exact_and_outcome_sensitive() -> None:
    vectors = _vectors()
    for name in ("active_ready", "local_only_ready"):
        value = vectors[name]
        wire = value["terminal"]["wire"]
        evidence = value["terminal"]["values"]["evidence"]
        request_generation = int.from_bytes(value["parsed"]["envelope"][29], "big")
        assert evidence["selection_clock_generation"] == request_generation
        before_admitted = _resign_pxau(
            _replace(
                wire,
                PXAU_OFFSETS["selection_observed_at_nanos"],
                _u64(evidence["admitted_at_nanos"] - 1),
            )
        )
        later_generation = _resign_pxau(
            _replace(
                wire,
                PXAU_OFFSETS["selection_clock_generation"],
                _u64(request_generation + 1),
            )
        )
        for tampered in (before_admitted, later_generation):
            with pytest.raises(ContractReject):
                _parse_pxau(tampered, value["pxar"], value["terminal"]["public_key"])

    active = vectors["active_ready"]
    active_evidence = active["terminal"]["values"]["evidence"]
    active_at_deadline = _resign_pxau(
        _replace(
            active["terminal"]["wire"],
            PXAU_OFFSETS["selection_observed_at_nanos"],
            _u64(active_evidence["absolute_deadline_nanos"]),
        )
    )
    with pytest.raises(ContractReject):
        _parse_pxau(
            active_at_deadline,
            active["pxar"],
            active["terminal"]["public_key"],
        )

    local = vectors["local_only_ready"]
    local_evidence = local["terminal"]["values"]["evidence"]
    assert local_evidence["selection_observed_at_nanos"] > local_evidence["absolute_deadline_nanos"]
    _parse_pxau(local["terminal"]["wire"], local["pxar"], local["terminal"]["public_key"])
    for cleanup_outcome in (
        OUTCOME_LOCAL_ONLY_READY,
        OUTCOME_NO_EFFECT_REJECTED,
        OUTCOME_UNCERTAIN,
        OUTCOME_QUARANTINED,
    ):
        assert _terminal_selection_time_is_valid(cleanup_outcome, 100, 200, 250)


def test_terminal_selection_exactly_at_admission_is_accepted() -> None:
    value = _vectors()["active_ready"]
    admitted_at_nanos = value["terminal"]["values"]["evidence"]["admitted_at_nanos"]
    at_admission = _resign_pxau(
        _replace(
            value["terminal"]["wire"],
            PXAU_OFFSETS["selection_observed_at_nanos"],
            _u64(admitted_at_nanos),
        )
    )
    transcript = PXAU_SIGNING_MAGIC + _u16(PXAU_SIGNING_VERSION) + at_admission[6:681]
    Ed25519PublicKey.from_public_bytes(value["terminal"]["public_key"]).verify(
        at_admission[683:],
        transcript,
    )
    parsed = _parse_pxau(at_admission, value["pxar"], value["terminal"]["public_key"])
    assert parsed["values"]["evidence"]["selection_observed_at_nanos"] == admitted_at_nanos


def test_local_only_drain_bitmaps_and_counters_fail_closed() -> None:
    value = _vectors()["local_only_ready"]
    wire = value["terminal"]["wire"]
    request = value["pxar"]
    public = value["terminal"]["public_key"]
    mutations = [
        (PXAU_OFFSETS["submit_terminalized_count"], _u64(2)),
        (PXAU_OFFSETS["control_terminalized_count"], _u64(1)),
        (PXAU_OFFSETS["ingress_fenced_bitmap"], _u8(1)),
        (PXAU_OFFSETS["worker_joined_bitmap"], _u8(1)),
        (PXAU_OFFSETS["drain_outcome"], _u8(DRAIN_NOT_STARTED)),
    ]
    for offset, replacement in mutations:
        tampered = _resign_pxau(_replace(wire, offset, replacement))
        with pytest.raises(ContractReject):
            _parse_pxau(tampered, request, public)


@pytest.mark.parametrize(
    ("name", "offset", "replacement"),
    [
        ("active_ready", PXAU_OFFSETS["phase"], _u8(PHASE_LOCAL_ONLY_OBSERVATION)),
        ("active_ready", PXAU_OFFSETS["outcome"], _u8(OUTCOME_LOCAL_ONLY_READY)),
        ("active_ready", PXAU_OFFSETS["flags"], _u16(0x008F)),
        ("local_only_ready", PXAU_OFFSETS["phase"], _u8(PHASE_READY_OBSERVATION)),
        ("local_only_ready", PXAU_OFFSETS["outcome"], _u8(OUTCOME_ACTIVE_READY)),
        ("local_only_ready", PXAU_OFFSETS["flags"], _u16(0x00B1)),
    ],
)
def test_phase_outcome_and_flags_fail_closed(
    name: str,
    offset: int,
    replacement: bytes,
) -> None:
    value = _vectors()[name]
    tampered = _resign_pxau(_replace(value["terminal"]["wire"], offset, replacement))
    with pytest.raises(ContractReject):
        _parse_pxau(tampered, value["pxar"], value["terminal"]["public_key"])


def test_v1_v2_cross_reject_and_wire_maxima() -> None:
    vectors = _vectors()
    old = V1._vectors()
    active = vectors["active_ready"]
    assert len(active["pxte"]) <= MAX_PXTE_BYTES
    assert len(active["pxar"]) <= MAX_PXAR_BYTES
    assert len(active["terminal"]["wire"]) == PXAU_CANONICAL_BYTES < MAX_PXAU_CARRIER_BYTES
    with pytest.raises(ContractReject):
        _parse_pxte(bytes(MAX_PXTE_BYTES + 1))
    with pytest.raises(ContractReject):
        _parse_pxar(bytes(MAX_PXAR_BYTES + 1))
    with pytest.raises(ContractReject):
        _parse_pxau(bytes(MAX_PXAU_CARRIER_BYTES + 1), active["pxar"], _public(RUNTIME_SEED))
    with pytest.raises(ContractReject):
        _parse_pxte(old["pxte"])
    with pytest.raises(ContractReject):
        _parse_pxar(old["pxar"])
    with pytest.raises(ContractReject):
        _parse_pxau(old["pxau"]["wire"], active["pxar"], _public(RUNTIME_SEED))
    with pytest.raises(ValueError):
        V1._parse_pxte(active["pxte"])
    with pytest.raises(ValueError):
        V1._parse_pxar(active["pxar"])
    with pytest.raises(ValueError):
        V1._parse_pxau(
            active["terminal"]["wire"],
            old["pxar"],
            old["pxau"]["public_key"],
        )


@pytest.mark.parametrize("magic", [b"PXRA", b"PXRR", b"PXAG", b"PXAH", b"PXRS"])
def test_cross_protocol_magic_is_rejected(magic: bytes) -> None:
    value = _vectors()["active_ready"]
    with pytest.raises(ContractReject):
        _parse_pxte(_replace(value["pxte"], 0, magic))
    with pytest.raises(ContractReject):
        _parse_pxar(_replace(value["pxar"], 0, magic))
    with pytest.raises(ContractReject):
        _parse_pxau(
            _replace(value["terminal"]["wire"], 0, magic),
            value["pxar"],
            value["terminal"]["public_key"],
        )


def test_terminal_signature_body_and_digest_tamper_are_rejected() -> None:
    value = _vectors()["active_ready"]
    wire = value["terminal"]["wire"]
    signature_tamper = wire[:-1] + bytes([wire[-1] ^ 1])
    with pytest.raises(ContractReject):
        _parse_pxau(signature_tamper, value["pxar"], value["terminal"]["public_key"])
    request_digest_tamper = _resign_pxau(
        _replace(wire, PXAU_OFFSETS["request_digest"], bytes.fromhex("ef" * 32))
    )
    with pytest.raises(ContractReject):
        _parse_pxau(request_digest_tamper, value["pxar"], value["terminal"]["public_key"])
    assert _digest(PXAU_DOMAIN, [wire]) == value["terminal"]["digest"]
    assert _digest(V1.PXAU_DOMAIN, [wire]) != value["terminal"]["digest"]
