"""Independent Python oracle for the Artifact F0/A1 wire contract.

This module intentionally does not import ParaEGOX production encoders.  Checked-in
fixtures are the expected values; the helpers below independently reconstruct and
strictly decode them with only ``struct``, ``hashlib``, and ``json``.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import struct
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

_REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
_FIXTURE_ROOT = _REPOSITORY_ROOT / "tests" / "fixtures" / "wire"
_LEDGER_PATH = _FIXTURE_ROOT / "artifact_f0_semantic_ledger_v1.json"
_CLI_ENVELOPES_PATH = _FIXTURE_ROOT / "artifact_f0_a1_cli_envelopes_v1.jsonl"
_BINARY_ENVIRONMENT = "PARAEGOX_A1_ARTIFACT_CLI_BINARY"

_ZERO32 = bytes(32)
_PROFILE = b"developer-local-echo-prefix-v1"
_RUNTIME_KIND = b"managed_model_data_v1"
_ADAPTER_ABI = b"bounded-text-model-data-v1"
_TARGET_PROFILE = b"developer-local-managed-model-v1"
_ENTRYPOINT = b"literal-prefix-v1"

_DOMAINS = {
    "payload": b"paraegox.artifact.payload.sha256.v1",
    "manifest": b"paraegox.artifact.manifest.sha256.v1",
    "request": b"paraegox.artifact.materialization-request.sha256.v1",
    "admission": b"paraegox.artifact.materialization-admission.sha256.v1",
    "materializing": b"paraegox.artifact.materializing.sha256.v1",
    "object_terminal": b"paraegox.artifact.object-terminal.sha256.v1",
    "operation_terminal": b"paraegox.artifact.materialization-terminal.sha256.v1",
    "receipt": b"paraegox.artifact.materialization-receipt.sha256.v1",
    "snapshot": b"paraegox.artifact.store-snapshot.sha256.v1",
}

_CORE_LENGTHS = {
    "PXAM": 206,
    "PXAK": 72,
    "PXAQ": 176,
    "PXAA": 208,
    "PXMU": 240,
    "PXAV": 192,
    "PXAW": 304,
    "PXAX": 240,
}
_PXOP_LENGTHS = {0: 416, 1: 656, 2: 720, 3: 960, 6: 960, 7: 1200}


@dataclass(frozen=True)
class OperationWire:
    sequence: int
    operation_id: bytes
    request: bytes
    admission: bytes
    materializing: bytes


@dataclass(frozen=True)
class Oracle:
    ledger: dict[str, Any]
    payload: bytes
    store_instance: bytes
    config_commitment: bytes
    primary_operation_id: bytes
    second_operation_id: bytes
    manifest: bytes
    object_key: bytes
    primary: OperationWire
    second: OperationWire
    object_terminal: bytes
    terminals: dict[str, bytes]
    receipts: dict[str, bytes]


def _u16(value: int) -> bytes:
    return struct.pack(">H", value)


def _u32(value: int) -> bytes:
    return struct.pack(">I", value)


def _u64(value: int) -> bytes:
    return struct.pack(">Q", value)


def _digest(domain: bytes, payload: bytes) -> bytes:
    return hashlib.sha256(domain + payload).digest()


def _compact_json(value: dict[str, Any]) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"


def _load_ledger() -> tuple[bytes, dict[str, Any]]:
    raw = _LEDGER_PATH.read_bytes()
    value = json.loads(raw)
    assert isinstance(value, dict)
    return raw, value


def _manifest(payload: bytes) -> bytes:
    payload_digest = _digest(_DOMAINS["payload"], payload)
    header = b"".join(
        (
            b"PXAM",
            _u16(1),
            _u16(80),
            _u32(206),
            _u16(len(_PROFILE)),
            _u16(len(_RUNTIME_KIND)),
            _u16(len(_ADAPTER_ABI)),
            _u16(len(_TARGET_PROFILE)),
            _u16(len(_ENTRYPOINT)),
            _u16(0),
            _u64(len(payload)),
            payload_digest,
            _u32(16_384),
            _u32(32_768),
            _u32(0),
            _u32(0),
        )
    )
    frame = header + _PROFILE + _RUNTIME_KIND + _ADAPTER_ABI + _TARGET_PROFILE + _ENTRYPOINT
    assert len(header) == 80
    assert len(frame) == 206
    return frame


def _object_key(payload: bytes, manifest: bytes) -> bytes:
    return (
        b"PXAK"
        + _u16(1)
        + _u16(72)
        + _digest(_DOMAINS["payload"], payload)
        + _digest(_DOMAINS["manifest"], manifest)
    )


def _request(operation_id: bytes, config_commitment: bytes, object_key: bytes) -> bytes:
    prefix = b"".join(
        (
            b"PXAQ",
            _u16(1),
            b"M\x00",
            _u16(176),
            _u16(0),
            _u32(176),
            operation_id,
            config_commitment,
            object_key,
            _u32(0),
            _u32(0),
        )
    )
    assert len(prefix) == 144
    return prefix + _digest(_DOMAINS["request"], prefix)


def _admission(
    store_instance: bytes,
    sequence: int,
    operation_id: bytes,
    request: bytes,
    object_key: bytes,
) -> bytes:
    prefix = b"".join(
        (
            b"PXAA",
            _u16(1),
            b"MA",
            _u16(208),
            _u16(0),
            _u32(208),
            store_instance,
            _u64(sequence),
            operation_id,
            request[144:176],
            object_key,
        )
    )
    assert len(prefix) == 176
    return prefix + _digest(_DOMAINS["admission"], prefix)


def _materializing(
    store_instance: bytes,
    sequence: int,
    operation_id: bytes,
    request: bytes,
    admission: bytes,
    object_key: bytes,
) -> bytes:
    prefix = b"".join(
        (
            b"PXMU",
            _u16(1),
            b"MP",
            _u16(240),
            _u16(0),
            _u32(240),
            store_instance,
            _u64(sequence),
            operation_id,
            request[144:176],
            admission[176:208],
            object_key,
        )
    )
    assert len(prefix) == 208
    return prefix + _digest(_DOMAINS["materializing"], prefix)


def _object_terminal(
    store_instance: bytes,
    object_sequence: int,
    object_key: bytes,
    payload_len: int,
) -> bytes:
    prefix = b"".join(
        (
            b"PXAV",
            _u16(1),
            b"MR",
            _u16(192),
            _u16(0),
            _u32(192),
            store_instance,
            _u64(object_sequence),
            object_key,
            _u64(payload_len),
            _u32(206),
            bytes(20),
        )
    )
    assert len(prefix) == 160
    return prefix + _digest(_DOMAINS["object_terminal"], prefix)


def _terminal(
    operation: OperationWire,
    store_instance: bytes,
    object_key: bytes,
    state: str,
    object_terminal: bytes | None,
    *,
    include_materializing: bool,
) -> bytes:
    materializing_digest = operation.materializing[208:240] if include_materializing else _ZERO32
    object_terminal_digest = object_terminal[160:192] if object_terminal is not None else _ZERO32
    prefix = b"".join(
        (
            b"PXAW",
            _u16(1),
            b"M",
            state.encode("ascii"),
            _u16(304),
            _u16(0),
            _u32(304),
            store_instance,
            _u64(operation.sequence),
            operation.operation_id,
            operation.request[144:176],
            operation.admission[176:208],
            materializing_digest,
            object_terminal_digest,
            object_key,
        )
    )
    assert len(prefix) == 272
    return prefix + _digest(_DOMAINS["operation_terminal"], prefix)


def _receipt(
    operation: OperationWire,
    store_instance: bytes,
    object_key: bytes,
    terminal: bytes,
) -> bytes:
    prefix = b"".join(
        (
            b"PXAX",
            _u16(1),
            b"M",
            terminal[7:8],
            _u16(240),
            _u16(0),
            _u32(240),
            store_instance,
            _u64(operation.sequence),
            operation.operation_id,
            operation.request[144:176],
            terminal[272:304],
            object_key,
        )
    )
    assert len(prefix) == 208
    return prefix + _digest(_DOMAINS["receipt"], prefix)


def _operation(
    store_instance: bytes,
    config_commitment: bytes,
    object_key: bytes,
    sequence: int,
    operation_id: bytes,
) -> OperationWire:
    request = _request(operation_id, config_commitment, object_key)
    admission = _admission(store_instance, sequence, operation_id, request, object_key)
    materializing = _materializing(
        store_instance, sequence, operation_id, request, admission, object_key
    )
    return OperationWire(sequence, operation_id, request, admission, materializing)


def _oracle() -> Oracle:
    _, ledger = _load_ledger()
    store = ledger["artifact_store"]
    payload = ledger["payload_utf8"].encode()
    store_instance = bytes.fromhex(store["store_instance_hex"])
    config_commitment = bytes.fromhex(store["config_commitment_hex"])
    primary_operation_id = bytes.fromhex(store["primary_operation_id_hex"])
    second_operation_id = bytes.fromhex(store["second_operation_id_hex"])
    manifest = _manifest(payload)
    object_key = _object_key(payload, manifest)
    primary = _operation(
        store_instance,
        config_commitment,
        object_key,
        store["primary_operation_sequence"],
        primary_operation_id,
    )
    second = _operation(
        store_instance,
        config_commitment,
        object_key,
        store["second_operation_sequence"],
        second_operation_id,
    )
    object_terminal = _object_terminal(
        store_instance, store["object_sequence"], object_key, len(payload)
    )
    terminals = {
        "materialized": _terminal(
            primary,
            store_instance,
            object_key,
            "M",
            object_terminal,
            include_materializing=True,
        ),
        "already_materialized": _terminal(
            second,
            store_instance,
            object_key,
            "E",
            object_terminal,
            include_materializing=True,
        ),
        "failed": _terminal(
            primary,
            store_instance,
            object_key,
            "F",
            None,
            include_materializing=False,
        ),
        "failed_after_materializing": _terminal(
            primary,
            store_instance,
            object_key,
            "F",
            None,
            include_materializing=True,
        ),
        "uncertain": _terminal(
            primary,
            store_instance,
            object_key,
            "U",
            None,
            include_materializing=True,
        ),
    }
    receipts = {
        name: _receipt(
            second if name == "already_materialized" else primary,
            store_instance,
            object_key,
            terminal,
        )
        for name, terminal in terminals.items()
    }
    return Oracle(
        ledger,
        payload,
        store_instance,
        config_commitment,
        primary_operation_id,
        second_operation_id,
        manifest,
        object_key,
        primary,
        second,
        object_terminal,
        terminals,
        receipts,
    )


def _pxop(
    operation: OperationWire,
    *,
    terminal: bytes | None = None,
    receipt: bytes | None = None,
    include_materializing: bool = False,
) -> bytes:
    flags = int(include_materializing) | (2 if terminal is not None else 0)
    flags |= 4 if receipt is not None else 0
    nested = operation.request + operation.admission
    if include_materializing:
        nested += operation.materializing
    if terminal is not None:
        nested += terminal
    if receipt is not None:
        nested += receipt
    entry_len = 32 + len(nested)
    assert _PXOP_LENGTHS[flags] == entry_len
    return (
        b"PXOP"
        + _u16(1)
        + _u16(32)
        + _u32(entry_len)
        + _u32(flags)
        + operation.operation_id
        + nested
    )


def _snapshot(
    oracle: Oracle,
    *,
    sequence: int,
    objects: tuple[bytes, ...],
    operations: tuple[bytes, ...],
    state_flags: int = 0,
    accounted_rest_bytes: int,
    quarantine_bytes: int = 0,
) -> bytes:
    object_table = b"".join(objects)
    operation_table = b"".join(operations)
    body = b"".join(
        (
            b"PXAY",
            _u16(1),
            _u16(64),
            _u64(64 + len(object_table) + len(operation_table)),
            _u32(len(objects)),
            _u32(len(operations)),
            _u64(len(object_table)),
            _u64(len(operation_table)),
            bytes(24),
            object_table,
            operation_table,
        )
    )
    prefix = b"".join(
        (
            b"PXAZ",
            _u16(1),
            _u16(192),
            _u64(192 + len(body)),
            _u16(1),
            _u16(1),
            _u16(1),
            _u16(1),
            _u32(state_flags),
            _u32(0),
            oracle.store_instance,
            oracle.config_commitment,
            _u64(sequence),
            _u64(len(operations)),
            _u64(len(objects)),
            _u32(len(operations)),
            _u32(len(objects)),
            _u64(len(body)),
            _u64(accounted_rest_bytes),
            _u64(quarantine_bytes),
            _u64(0),
        )
    )
    assert len(prefix) == 160
    checksum_input = _u64(160) + prefix + _u64(len(body)) + body
    return prefix + _digest(_DOMAINS["snapshot"], checksum_input) + body


def _snapshot_vectors(oracle: Oracle) -> dict[str, bytes]:
    primary = oracle.primary
    second = oracle.second
    pxav = oracle.object_terminal
    materialized = oracle.terminals["materialized"]
    already = oracle.terminals["already_materialized"]
    failed = oracle.terminals["failed"]
    failed_after = oracle.terminals["failed_after_materializing"]
    uncertain = oracle.terminals["uncertain"]
    materialized_receipt = oracle.receipts["materialized"]
    already_receipt = oracle.receipts["already_materialized"]
    failed_receipt = oracle.receipts["failed"]
    failed_after_receipt = oracle.receipts["failed_after_materializing"]
    uncertain_receipt = oracle.receipts["uncertain"]

    specs = {
        "admitted": (1, (), (_pxop(primary),), 0, 672),
        "materializing": (2, (), (_pxop(primary, include_materializing=True),), 0, 912),
        "object_terminal": (
            3,
            (pxav,),
            (_pxop(primary, include_materializing=True),),
            0,
            1_330,
        ),
        "materialized_terminal": (
            4,
            (pxav,),
            (_pxop(primary, terminal=materialized, include_materializing=True),),
            0,
            1_634,
        ),
        "materialized_receipt": (
            5,
            (pxav,),
            (
                _pxop(
                    primary,
                    terminal=materialized,
                    receipt=materialized_receipt,
                    include_materializing=True,
                ),
            ),
            0,
            1_874,
        ),
        "failed_terminal": (2, (), (_pxop(primary, terminal=failed),), 0, 976),
        "failed_receipt": (
            3,
            (),
            (_pxop(primary, terminal=failed, receipt=failed_receipt),),
            0,
            1_216,
        ),
        "failed_after_materializing_terminal": (
            3,
            (),
            (_pxop(primary, terminal=failed_after, include_materializing=True),),
            0,
            1_216,
        ),
        "failed_after_materializing_receipt": (
            4,
            (),
            (
                _pxop(
                    primary,
                    terminal=failed_after,
                    receipt=failed_after_receipt,
                    include_materializing=True,
                ),
            ),
            0,
            1_456,
        ),
        "already_materialized_materializing": (
            7,
            (pxav,),
            (
                _pxop(
                    primary,
                    terminal=materialized,
                    receipt=materialized_receipt,
                    include_materializing=True,
                ),
                _pxop(second, include_materializing=True),
            ),
            0,
            2_530,
        ),
        "already_materialized_terminal": (
            8,
            (pxav,),
            (
                _pxop(
                    primary,
                    terminal=materialized,
                    receipt=materialized_receipt,
                    include_materializing=True,
                ),
                _pxop(second, terminal=already, include_materializing=True),
            ),
            0,
            2_834,
        ),
        "already_materialized_receipt": (
            9,
            (pxav,),
            (
                _pxop(
                    primary,
                    terminal=materialized,
                    receipt=materialized_receipt,
                    include_materializing=True,
                ),
                _pxop(
                    second,
                    terminal=already,
                    receipt=already_receipt,
                    include_materializing=True,
                ),
            ),
            0,
            3_074,
        ),
        "uncertain_blocked": (
            3,
            (),
            (_pxop(primary, terminal=uncertain, include_materializing=True),),
            1,
            1_216,
        ),
        "uncertain_receipt_blocked": (
            4,
            (),
            (
                _pxop(
                    primary,
                    terminal=uncertain,
                    receipt=uncertain_receipt,
                    include_materializing=True,
                ),
            ),
            1,
            1_456,
        ),
    }
    return {
        name: _snapshot(
            oracle,
            sequence=sequence,
            objects=objects,
            operations=operations,
            state_flags=flags,
            accounted_rest_bytes=accounted,
        )
        for name, (sequence, objects, operations, flags, accounted) in specs.items()
    }


def _object_ref(oracle: Oracle) -> str:
    return f"sha256:{oracle.object_key[8:40].hex()}:{oracle.object_key[40:72].hex()}"


def _receipt_ref(oracle: Oracle, name: str) -> str:
    operation = oracle.second if name == "already_materialized" else oracle.primary
    receipt = oracle.receipts[name]
    return (
        f"pxamr1:{oracle.store_instance.hex()}:{operation.sequence}:"
        f"{operation.operation_id.hex()}:{receipt[208:240].hex()}"
    )


def _query_envelope(
    oracle: Oracle,
    *,
    state: str,
    operation_id: bytes,
) -> bytes:
    ok = state not in {"failed", "uncertain"}
    diagnostics: list[dict[str, str]] = []
    if state == "failed":
        diagnostics = [
            {
                "code": "PXLC-ARTIFACT-MATERIALIZATION-FAILED",
                "message": "artifact materialization failed",
            }
        ]
    elif state == "uncertain":
        diagnostics = [
            {
                "code": "PXLC-ARTIFACT-UNCERTAIN",
                "message": "artifact operation outcome is uncertain",
            }
        ]
    return _compact_json(
        {
            "schema_version": 1,
            "command": "artifact.materialization.query",
            "ok": ok,
            "changed": False,
            "operation_id": operation_id.hex(),
            "state": state,
            "artifact_object_ref": _object_ref(oracle),
            "materialization_receipt_ref": None,
            "diagnostics": diagnostics,
        }
    )


def _query_vectors(oracle: Oracle) -> dict[str, bytes]:
    materializing = _query_envelope(
        oracle, state="materializing", operation_id=oracle.primary_operation_id
    )
    return {
        "pxaw_only_materialized": _query_envelope(
            oracle, state="materialized", operation_id=oracle.primary_operation_id
        ),
        "pxaw_only_already_materialized": _query_envelope(
            oracle, state="already_materialized", operation_id=oracle.second_operation_id
        ),
        "pxaw_only_failed": _query_envelope(
            oracle, state="failed", operation_id=oracle.primary_operation_id
        ),
        "pxaw_only_failed_after_materializing": _query_envelope(
            oracle, state="failed", operation_id=oracle.primary_operation_id
        ),
        "pxaw_only_uncertain": _query_envelope(
            oracle, state="uncertain", operation_id=oracle.primary_operation_id
        ),
        "materializing_empty_child": materializing,
        "materializing_temp": materializing,
        "materializing_single_final": materializing,
        "materializing_full_pair": materializing,
        "materializing_object_terminal": materializing,
        "materializing_existing_object": _query_envelope(
            oracle, state="materializing", operation_id=oracle.second_operation_id
        ),
    }


def _hex_fixture(name: str) -> bytes:
    raw = (_FIXTURE_ROOT / name).read_bytes()
    assert raw.endswith(b"\n") and raw.count(b"\n") == 1
    line = raw[:-1]
    assert line == line.lower()
    assert len(line) % 2 == 0
    return bytes.fromhex(line.decode("ascii"))


def _read_u16(frame: bytes, offset: int) -> int:
    return struct.unpack_from(">H", frame, offset)[0]


def _read_u32(frame: bytes, offset: int) -> int:
    return struct.unpack_from(">I", frame, offset)[0]


def _read_u64(frame: bytes, offset: int) -> int:
    return struct.unpack_from(">Q", frame, offset)[0]


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _strict_snapshot_decode(frame: bytes) -> dict[str, Any]:
    _require(len(frame) >= 192, "short PXAZ")
    _require(frame[0:4] == b"PXAZ", "PXAZ magic")
    _require(_read_u16(frame, 4) == 1, "PXAZ version")
    _require(_read_u16(frame, 6) == 192, "PXAZ header")
    _require(_read_u64(frame, 8) == len(frame), "PXAZ frame length")
    _require(frame[16:24] == _u16(1) * 4, "PXAZ body/checksum tags")
    state_flags = _read_u32(frame, 24)
    _require(state_flags in {0, 1}, "PXAZ flags")
    _require(frame[28:32] == bytes(4), "PXAZ reserved")
    _require(frame[152:160] == bytes(8), "PXAZ tail reserved")
    body_len = _read_u64(frame, 128)
    _require(192 + body_len == len(frame), "PXAZ body length")
    body = frame[192:]
    checksum_input = _u64(160) + frame[:160] + _u64(body_len) + body
    _require(frame[160:192] == _digest(_DOMAINS["snapshot"], checksum_input), "checksum")

    _require(body[0:4] == b"PXAY", "PXAY magic")
    _require(_read_u16(body, 4) == 1 and _read_u16(body, 6) == 64, "PXAY version")
    _require(_read_u64(body, 8) == body_len, "PXAY body length")
    object_count = _read_u32(body, 16)
    operation_count = _read_u32(body, 20)
    object_table_len = _read_u64(body, 24)
    operation_table_len = _read_u64(body, 32)
    _require(body[40:64] == bytes(24), "PXAY reserved")
    _require(object_table_len == object_count * 192, "PXAY object table")
    _require(64 + object_table_len + operation_table_len == body_len, "PXAY tables")
    _require(_read_u32(frame, 120) == operation_count, "PXAZ operation count")
    _require(_read_u32(frame, 124) == object_count, "PXAZ object count")
    _require(_read_u64(frame, 104) == operation_count, "operation high-water")
    _require(_read_u64(frame, 112) == object_count, "object high-water")
    _require((state_flags == 1) or _read_u64(frame, 144) == 0, "quarantine")

    cursor = 64
    object_digests: dict[bytes, bytes] = {}
    for expected_sequence in range(1, object_count + 1):
        pxav = body[cursor : cursor + 192]
        _require(len(pxav) == 192 and pxav[0:4] == b"PXAV", "PXAV table")
        _require(pxav[4:16] == _u16(1) + b"MR" + _u16(192) + _u16(0) + _u32(192), "PXAV header")
        _require(pxav[16:48] == frame[32:64], "PXAV store")
        _require(_read_u64(pxav, 48) == expected_sequence, "PXAV order")
        _require(pxav[140:160] == bytes(20), "PXAV reserved")
        _require(pxav[160:192] == _digest(_DOMAINS["object_terminal"], pxav[:160]), "PXAV")
        object_key = pxav[56:128]
        _require(object_key not in object_digests, "duplicate PXAV object")
        object_digests[object_key] = pxav[160:192]
        cursor += 192

    flags_seen: list[int] = []
    operation_ids: list[bytes] = []
    terminal_states: list[str | None] = []
    referenced_object_digests: set[bytes] = set()
    incomplete_entries = 0
    for expected_sequence in range(1, operation_count + 1):
        _require(body[cursor : cursor + 4] == b"PXOP", "PXOP magic")
        entry_len = _read_u32(body, cursor + 8)
        flags = _read_u32(body, cursor + 12)
        _require(_PXOP_LENGTHS.get(flags) == entry_len, "PXOP flags/length")
        entry = body[cursor : cursor + entry_len]
        _require(len(entry) == entry_len, "short PXOP")
        operation_id = entry[16:32]
        request = entry[32:208]
        admission = entry[208:416]
        _require(operation_id not in operation_ids, "duplicate operation id")
        operation_ids.append(operation_id)
        _require(request[0:4] == b"PXAQ" and admission[0:4] == b"PXAA", "nested A")
        _require(
            request[4:16]
            == _u16(1) + b"M\x00" + _u16(176) + _u16(0) + _u32(176),
            "PXAQ header",
        )
        _require(request[16:32] == operation_id, "PXAQ operation id")
        _require(request[32:64] == frame[64:96], "PXAQ config")
        _require(request[136:144] == bytes(8), "PXAQ flags/reserved")
        _require(
            admission[4:16]
            == _u16(1) + b"MA" + _u16(208) + _u16(0) + _u32(208),
            "PXAA header",
        )
        _require(admission[16:48] == frame[32:64], "PXAA store")
        _require(admission[56:72] == operation_id, "PXAA operation id")
        _require(_read_u64(admission, 48) == expected_sequence, "PXAA order")
        _require(request[144:176] == _digest(_DOMAINS["request"], request[:144]), "PXAQ")
        _require(admission[72:104] == request[144:176], "PXAA request")
        _require(admission[104:176] == request[64:136], "PXAA object")
        _require(admission[176:208] == _digest(_DOMAINS["admission"], admission[:176]), "PXAA")
        nested_cursor = 416
        materializing: bytes | None = None
        if flags & 1:
            materializing = entry[nested_cursor : nested_cursor + 240]
            _require(materializing[0:4] == b"PXMU", "nested PXMU")
            _require(
                materializing[4:16]
                == _u16(1) + b"MP" + _u16(240) + _u16(0) + _u32(240),
                "PXMU header",
            )
            _require(materializing[16:48] == frame[32:64], "PXMU store")
            _require(_read_u64(materializing, 48) == expected_sequence, "PXMU sequence")
            _require(materializing[56:72] == operation_id, "PXMU operation")
            _require(materializing[72:104] == request[144:176], "PXMU request")
            _require(materializing[104:136] == admission[176:208], "PXMU admission")
            _require(materializing[136:208] == request[64:136], "PXMU object")
            _require(
                materializing[208:240]
                == _digest(_DOMAINS["materializing"], materializing[:208]),
                "PXMU",
            )
            nested_cursor += 240
        terminal: bytes | None = None
        if flags & 2:
            terminal = entry[nested_cursor : nested_cursor + 304]
            _require(terminal[0:4] == b"PXAW", "nested PXAW")
            terminal_state = terminal[7:8].decode("ascii")
            _require(terminal_state in {"M", "E", "F", "U"}, "PXAW state")
            _require(
                terminal[4:7] == _u16(1) + b"M"
                and terminal[8:16] == _u16(304) + _u16(0) + _u32(304),
                "PXAW header",
            )
            _require(terminal[16:48] == frame[32:64], "PXAW store")
            _require(_read_u64(terminal, 48) == expected_sequence, "PXAW sequence")
            _require(terminal[56:72] == operation_id, "PXAW operation")
            _require(terminal[72:104] == request[144:176], "PXAW request")
            _require(terminal[104:136] == admission[176:208], "PXAW admission")
            _require(terminal[200:272] == request[64:136], "PXAW object")
            expected_materializing_digest = (
                materializing[208:240] if materializing is not None else _ZERO32
            )
            _require(
                terminal[136:168] == expected_materializing_digest,
                "PXAW materializing",
            )
            _require(
                terminal[272:304]
                == _digest(_DOMAINS["operation_terminal"], terminal[:272]),
                "PXAW",
            )
            object_terminal_digest = terminal[168:200]
            if terminal_state in {"M", "E"}:
                _require(materializing is not None, "successful PXAW without PXMU")
                _require(object_terminal_digest != _ZERO32, "successful PXAW without PXAV")
                _require(
                    object_terminal_digest in object_digests.values(),
                    "successful PXAW unknown PXAV",
                )
                if terminal_state == "M":
                    _require(
                        object_terminal_digest not in referenced_object_digests,
                        "M reuses referenced PXAV",
                    )
                else:
                    _require(
                        object_terminal_digest in referenced_object_digests,
                        "E does not reuse an earlier PXAV",
                    )
                referenced_object_digests.add(object_terminal_digest)
            else:
                _require(object_terminal_digest == _ZERO32, "F/U references PXAV")
                if terminal_state == "U":
                    _require(materializing is not None, "U without PXMU")
                    _require(state_flags == 1, "U without blocked flag")
                    _require(expected_sequence == operation_count, "U is not last")
                else:
                    _require(state_flags == 0, "F in blocked snapshot")
            nested_cursor += 304
        else:
            terminal_state = None
        if flags & 4:
            receipt = entry[nested_cursor : nested_cursor + 240]
            _require(receipt[0:4] == b"PXAX", "nested PXAX")
            _require(terminal is not None, "PXAX without PXAW")
            _require(
                receipt[4:7] == _u16(1) + b"M"
                and receipt[8:16] == _u16(240) + _u16(0) + _u32(240),
                "PXAX header",
            )
            _require(receipt[7:8] == terminal[7:8], "PXAX state")
            _require(receipt[16:48] == frame[32:64], "PXAX store")
            _require(_read_u64(receipt, 48) == expected_sequence, "PXAX sequence")
            _require(receipt[56:72] == operation_id, "PXAX operation")
            _require(receipt[72:104] == request[144:176], "PXAX request")
            _require(receipt[104:136] == terminal[272:304], "PXAX terminal")
            _require(receipt[136:208] == request[64:136], "PXAX object")
            _require(receipt[208:240] == _digest(_DOMAINS["receipt"], receipt[:208]), "PXAX")
            nested_cursor += 240
        else:
            incomplete_entries += 1
            _require(expected_sequence == operation_count, "non-last operation lacks PXAX")
        _require(nested_cursor == entry_len, "PXOP trailing")
        flags_seen.append(flags)
        terminal_states.append(terminal_state)
        cursor += entry_len
    _require(cursor == len(body), "PXAY trailing")
    _require(incomplete_entries <= 1, "multiple incomplete operations")
    if state_flags == 1:
        _require(object_count == 0, "blocked snapshot has PXAV")
        _require(terminal_states[-1:] == ["U"], "blocked snapshot lacks last U")
    else:
        _require("U" not in terminal_states, "unblocked snapshot has U")
    return {
        "frame_len": len(frame),
        "snapshot_sequence": _read_u64(frame, 96),
        "state_flags": state_flags,
        "accounted_rest_bytes": _read_u64(frame, 136),
        "quarantine_bytes": _read_u64(frame, 144),
        "object_count": object_count,
        "operation_count": operation_count,
        "presence_flags": flags_seen,
        "operation_ids": operation_ids,
        "terminal_states": terminal_states,
    }


def test_artifact_f0_semantic_ledger_is_exact_canonical_single_source() -> None:
    raw, ledger = _load_ledger()
    assert raw == _compact_json(ledger)
    assert list(ledger) == [
        "format",
        "payload_utf8",
        "artifact_store",
        "deployment",
        "runtime_apply",
        "authority",
        "signing",
        "predecessor",
        "agent_plan",
        "model_plan",
        "runtime_terminal",
        "controller_shapes",
        "runtime_state",
        "text",
    ]
    assert ledger["format"] == "paraegox-artifact-f0-semantic-ledger-v1"
    assert ledger["payload_utf8"] == "artifact-f0-prefix: "


def test_artifact_f0_a1_core_wire_goldens_match_independent_oracle() -> None:
    oracle = _oracle()
    expected = {
        "artifact_f0_pxam_v1.hex": oracle.manifest,
        "artifact_f0_pxak_v1.hex": oracle.object_key,
        "artifact_f0_pxaq_v1.hex": oracle.primary.request,
        "artifact_f0_pxaa_v1.hex": oracle.primary.admission,
        "artifact_f0_pxmu_v1.hex": oracle.primary.materializing,
        "artifact_f0_pxav_v1.hex": oracle.object_terminal,
        "artifact_f0_pxaw_materialized_v1.hex": oracle.terminals["materialized"],
        "artifact_f0_pxaw_already_materialized_v1.hex": oracle.terminals[
            "already_materialized"
        ],
        "artifact_f0_pxaw_failed_v1.hex": oracle.terminals["failed"],
        "artifact_f0_pxaw_uncertain_v1.hex": oracle.terminals["uncertain"],
        "artifact_f0_pxax_materialized_v1.hex": oracle.receipts["materialized"],
        "artifact_f0_pxax_already_materialized_v1.hex": oracle.receipts[
            "already_materialized"
        ],
        "artifact_f0_pxax_failed_v1.hex": oracle.receipts["failed"],
        "artifact_f0_pxax_uncertain_v1.hex": oracle.receipts["uncertain"],
    }
    for fixture_name, expected_bytes in expected.items():
        actual = _hex_fixture(fixture_name)
        assert actual == expected_bytes, fixture_name
        assert len(actual) == _CORE_LENGTHS[actual[:4].decode("ascii")]

    assert oracle.manifest[0:4] == b"PXAM"
    assert oracle.manifest[6:8] == _u16(80)
    assert oracle.manifest[24:32] == _u64(20)
    assert oracle.manifest[32:64] == _digest(_DOMAINS["payload"], oracle.payload)
    assert oracle.object_key[40:72] == _digest(_DOMAINS["manifest"], oracle.manifest)


def test_artifact_f0_a1_snapshot_goldens_match_independent_oracle_and_ledger() -> None:
    oracle = _oracle()
    snapshots = _snapshot_vectors(oracle)
    store = oracle.ledger["artifact_store"]
    for name, expected in snapshots.items():
        fixture_name = f"artifact_f0_pxaz_{name}_v1.hex"
        actual = _hex_fixture(fixture_name)
        assert actual == expected, fixture_name
        decoded = _strict_snapshot_decode(actual)
        assert decoded["frame_len"] == store["snapshot_frame_bytes"][name]
        assert decoded["snapshot_sequence"] == store["snapshot_sequences"][name]
        assert decoded["accounted_rest_bytes"] == store["accounted_logical_bytes"][name]

    assert _strict_snapshot_decode(snapshots["failed_terminal"])["presence_flags"] == [2]
    assert _strict_snapshot_decode(snapshots["failed_receipt"])["presence_flags"] == [6]
    assert _strict_snapshot_decode(snapshots["failed_after_materializing_terminal"])[
        "presence_flags"
    ] == [3]
    assert _strict_snapshot_decode(snapshots["failed_after_materializing_receipt"])[
        "presence_flags"
    ] == [7]
    uncertain = _strict_snapshot_decode(snapshots["uncertain_blocked"])
    assert uncertain["state_flags"] == 1
    assert uncertain["quarantine_bytes"] == 0
    assert uncertain["object_count"] == 0
    assert uncertain["presence_flags"] == [3]


def test_artifact_f0_a1_query_json_goldens_match_independent_oracle() -> None:
    oracle = _oracle()
    for name, expected in _query_vectors(oracle).items():
        raw = (_FIXTURE_ROOT / f"artifact_f0_query_{name}_v1.json").read_bytes()
        assert raw == expected, name
        assert raw == _compact_json(json.loads(raw))

    assert _query_vectors(oracle)["pxaw_only_failed"] == _query_vectors(oracle)[
        "pxaw_only_failed_after_materializing"
    ]


def test_artifact_f0_a1_text_and_capacity_vectors_match_ledger() -> None:
    oracle = _oracle()
    text = oracle.ledger["text"]
    assert (_FIXTURE_ROOT / "artifact_f0_pxop_presence_v1.txt").read_text() == (
        "\n".join(text["pxop_presence_lines"]) + "\n"
    )
    assert (_FIXTURE_ROOT / "artifact_f0_store_capacity_v1.txt").read_text() == (
        "\n".join(text["capacity_lines"]) + "\n"
    )
    assert text["capacity_lines"] == [
        "stable-components=1241344+64*(206+64)+540,result=1259164",
        "transaction-components=1259164+1241344+270,result=2500778",
        "snapshot-frame=1241344,result=accept",
        "snapshot-frame=1241345,result=reject",
        "objects=64,result=accept",
        "objects=65,result=reject",
        "operations=1024,result=accept",
        "operations=1025,result=reject",
        "quarantine=540,result=accept",
        "quarantine=541,result=reject",
        "indexed-object-bytes=270,extra-temp-bytes=270,count=once-each,result=accept",
        "defense-ceiling=8388608,result=accept",
        "defense-ceiling=8388609,result=reject",
        "checked-add=u64-max-plus-one,result=reject",
    ]


def test_artifact_f0_a1_reference_math_is_canonical() -> None:
    oracle = _oracle()
    object_ref = _object_ref(oracle)
    receipt_ref = _receipt_ref(oracle, "materialized")
    assert object_ref.startswith("sha256:") and len(object_ref) == 136
    assert receipt_ref.startswith("pxamr1:")
    assert receipt_ref.split(":")[2] == "1"
    assert receipt_ref.split(":")[3] == oracle.primary_operation_id.hex()
    assert len(receipt_ref.split(":")[4]) == 64
    assert len(oracle.payload) + 16_384 == 16_404
    assert 64 + 16_384 == 16_448
    assert 16_448 <= 32_768


def test_artifact_f0_a1_recovery_and_successor_vectors_are_history_typed() -> None:
    oracle = _oracle()
    snapshots = _snapshot_vectors(oracle)
    object_prefix = _strict_snapshot_decode(snapshots["object_terminal"])
    materialized = _strict_snapshot_decode(snapshots["materialized_terminal"])
    existing_prefix = _strict_snapshot_decode(snapshots["already_materialized_materializing"])
    already = _strict_snapshot_decode(snapshots["already_materialized_terminal"])

    assert object_prefix["snapshot_sequence"] == 3
    assert object_prefix["presence_flags"] == [1]
    assert object_prefix["terminal_states"] == [None]
    assert materialized["snapshot_sequence"] == 4
    assert materialized["terminal_states"] == ["M"]

    assert existing_prefix["snapshot_sequence"] == 7
    assert existing_prefix["presence_flags"] == [7, 1]
    assert existing_prefix["operation_ids"] == [
        oracle.primary_operation_id,
        oracle.second_operation_id,
    ]
    assert already["snapshot_sequence"] == 8
    assert already["terminal_states"] == ["M", "E"]
    assert _read_u64(oracle.primary.admission, 48) == 1
    assert _read_u64(oracle.second.admission, 48) == 2

    with pytest.raises(struct.error):
        _u64(2**64)


@pytest.mark.parametrize(
    ("fixture_name", "offset", "replacement"),
    [
        ("artifact_f0_pxaz_admitted_v1.hex", 0, b"Q"),
        ("artifact_f0_pxaz_admitted_v1.hex", 28, b"\x01"),
        ("artifact_f0_pxaz_admitted_v1.hex", 160, b"\x00"),
        ("artifact_f0_pxaz_uncertain_blocked_v1.hex", 27, b"\x02"),
    ],
)
def test_artifact_f0_a1_strict_snapshot_decoder_rejects_mutations(
    fixture_name: str, offset: int, replacement: bytes
) -> None:
    mutated = bytearray(_hex_fixture(fixture_name))
    mutated[offset : offset + len(replacement)] = replacement
    with pytest.raises(ValueError):
        _strict_snapshot_decode(bytes(mutated))


def test_artifact_f0_a1_snapshot_decoder_rejects_short_and_trailing() -> None:
    frame = _hex_fixture("artifact_f0_pxaz_materialized_receipt_v1.hex")
    with pytest.raises(ValueError):
        _strict_snapshot_decode(frame[:-1])
    with pytest.raises(ValueError):
        _strict_snapshot_decode(frame + b"\x00")


def _require_exact_cli_binary() -> Path:
    configured = os.environ.get(_BINARY_ENVIRONMENT)
    assert configured is not None, (
        f"{_BINARY_ENVIRONMENT} must name the exact-revision binary under validation"
    )
    path = Path(configured)
    assert path.is_absolute()
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode) and not path.is_symlink()
    assert metadata.st_mode & 0o111 != 0
    return path.resolve(strict=True)


def _invoke_artifact(binary: Path, arguments: list[str]) -> tuple[int, bytes, dict[str, Any]]:
    completed = subprocess.run(
        [os.fspath(binary), *arguments],
        cwd=binary.parent,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
        check=False,
    )
    assert completed.stderr == b""
    assert completed.stdout.endswith(b"\n") and completed.stdout.count(b"\n") == 1
    envelope = json.loads(completed.stdout)
    assert isinstance(envelope, dict)
    assert completed.stdout == _compact_json(envelope)
    return completed.returncode, completed.stdout, envelope


def _filesystem_projection(root: Path) -> tuple[tuple[object, ...], ...]:
    projected: list[tuple[object, ...]] = []
    for path in sorted(root.rglob("*")):
        metadata = path.lstat()
        relative = path.relative_to(root).as_posix()
        payload_digest = None
        if stat.S_ISREG(metadata.st_mode):
            payload_digest = hashlib.sha256(path.read_bytes()).hexdigest()
        projected.append(
            (
                relative,
                stat.S_IFMT(metadata.st_mode),
                metadata.st_mode & 0o7777,
                metadata.st_uid,
                metadata.st_gid,
                metadata.st_ino,
                metadata.st_nlink,
                metadata.st_size,
                metadata.st_mtime_ns,
                metadata.st_ctime_ns,
                payload_digest,
            )
        )
    return tuple(projected)


def test_artifact_f0_a1_cli_envelope_fixture_is_canonical() -> None:
    lines = _CLI_ENVELOPES_PATH.read_bytes().splitlines()
    assert len(lines) == 5
    for line in lines:
        value = json.loads(line)
        assert line + b"\n" == _compact_json(value)
    assert json.loads(lines[0])["artifact_object_ref"] == _object_ref(_oracle())
    assert list(json.loads(lines[0])) == [
        "schema_version",
        "command",
        "ok",
        "changed",
        "profile",
        "artifact_object_ref",
        "payload_length",
        "runtime_kind",
        "adapter_abi",
        "target_profile",
        "diagnostics",
    ]
    assert list(json.loads(lines[4])) == [
        "schema_version",
        "command",
        "ok",
        "changed",
        "operation_id",
        "state",
        "artifact_object_ref",
        "materialization_receipt_ref",
        "diagnostics",
    ]


def test_artifact_f0_a1_dispatch_and_authority_source_guards() -> None:
    main_source = (_REPOSITORY_ROOT / "crates/paraegox-local/src/main.rs").read_text()
    artifact_dispatch = main_source.index("config::artifact_json_intent")
    for later_dispatch in (
        "config::tui_attach_intent",
        "config::init_json_intent",
        "config::offline_json_intent",
        "config::lifecycle_json_intent",
    ):
        assert artifact_dispatch < main_source.index(later_dispatch)

    artifact_source = (_REPOSITORY_ROOT / "crates/paraegox-local/src/artifact.rs").read_text()
    build_body = artifact_source[
        artifact_source.index("pub(super) fn run_build") : artifact_source.index(
            "pub(super) fn run_inspect"
        )
    ]
    inspect_body = artifact_source[
        artifact_source.index("pub(super) fn run_inspect") : artifact_source.index(
            "pub(super) fn run_materialize"
        )
    ]
    assert "ArtifactStore" not in build_body
    assert "ArtifactStore" not in inspect_body
    assert "RevalidatingArtifactAuthority::new" in artifact_source
    assert artifact_source.count("drop(authority);") == 2
    assert artifact_source.count("Ok(project_invocation(operation_id, invocation))") == 2


def test_artifact_f0_a1_exact_binary_build_inspect_and_store_sequence() -> None:
    assert os.name == "posix" and os.geteuid() != 0 and os.getegid() != 0
    binary = _require_exact_cli_binary()
    fixtures = [line + b"\n" for line in _CLI_ENVELOPES_PATH.read_bytes().splitlines()]
    base = Path(tempfile.mkdtemp(prefix=".paraegox-a1-", dir=Path.home()))
    os.chmod(base, 0o700)
    try:
        workspace = base / "workspace"
        init = subprocess.run(
            [os.fspath(binary), "init", "--directory", os.fspath(workspace), "--json"],
            cwd=binary.parent,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=30.0,
            check=False,
        )
        assert init.returncode == 0 and init.stderr == b""
        config_path = workspace / "paraegox.toml"
        state_root = workspace / "state"
        assert config_path.is_file() and not state_root.exists()

        source = workspace / "artifact-prefix.txt"
        source.write_bytes(b"artifact-f0-prefix: ")
        os.chmod(source, 0o600)
        output_parent = workspace / "artifact-builds"
        output_parent.mkdir(mode=0o700)
        output = output_parent / "prefix-object"

        build_arguments = [
            "artifact",
            "build",
            "--profile",
            "developer-local-echo-prefix-v1",
            "--source",
            os.fspath(source),
            "--output",
            os.fspath(output),
            "--json",
        ]
        returncode, raw, build = _invoke_artifact(binary, build_arguments)
        assert returncode == 0 and raw == fixtures[0]
        assert not state_root.exists(), "offline build must not construct Store authority"
        assert sorted(path.name for path in output.iterdir()) == [
            "manifest.pxam",
            "payload.bin",
        ]
        assert [path.name for path in output_parent.iterdir()] == ["prefix-object"]

        returncode, raw, replay = _invoke_artifact(binary, build_arguments)
        assert returncode == 0 and raw == fixtures[1]
        assert replay["artifact_object_ref"] == build["artifact_object_ref"]

        manifest = output / "manifest.pxam"
        payload = output / "payload.bin"
        before_inspect = _filesystem_projection(output_parent)
        returncode, raw, inspected = _invoke_artifact(
            binary,
            [
                "artifact",
                "inspect",
                "--manifest",
                os.fspath(manifest),
                "--payload",
                os.fspath(payload),
                "--json",
            ],
        )
        after_inspect = _filesystem_projection(output_parent)
        assert returncode == 0 and raw == fixtures[2]
        assert inspected["artifact_object_ref"] == build["artifact_object_ref"]
        assert after_inspect == before_inspect
        assert not state_root.exists(), "offline inspect must not construct Store authority"

        primary = "a2" * 16
        returncode, raw, _ = _invoke_artifact(
            binary,
            [
                "artifact",
                "materialization",
                "query",
                "--config",
                os.fspath(config_path),
                "--operation-id",
                primary,
                "--json",
            ],
        )
        assert returncode == 1 and raw == fixtures[4]
        assert not state_root.exists(), "query must not create a missing state root"

        returncode, _, materialized = _invoke_artifact(
            binary,
            [
                "artifact",
                "materialize",
                "--config",
                os.fspath(config_path),
                "--manifest",
                os.fspath(manifest),
                "--payload",
                os.fspath(payload),
                "--operation-id",
                primary,
                "--json",
            ],
        )
        assert returncode == 0
        assert materialized["changed"] is True
        assert materialized["state"] == "materialized"
        assert materialized["artifact_object_ref"] == build["artifact_object_ref"]
        receipt = materialized["materialization_receipt_ref"]
        assert isinstance(receipt, str) and receipt.startswith("pxamr1:")

        returncode, _, replayed_materialization = _invoke_artifact(
            binary,
            [
                "artifact",
                "materialize",
                "--config",
                os.fspath(config_path),
                "--manifest",
                os.fspath(manifest),
                "--payload",
                os.fspath(payload),
                "--operation-id",
                primary,
                "--json",
            ],
        )
        assert returncode == 0
        assert replayed_materialization == {**materialized, "changed": False}

        returncode, _, queried = _invoke_artifact(
            binary,
            [
                "artifact",
                "materialization",
                "query",
                "--config",
                os.fspath(config_path),
                "--operation-id",
                primary,
                "--json",
            ],
        )
        assert returncode == 0
        assert queried == {
            **materialized,
            "command": "artifact.materialization.query",
            "changed": False,
        }

        second = "a3" * 16
        returncode, _, already = _invoke_artifact(
            binary,
            [
                "artifact",
                "materialize",
                "--config",
                os.fspath(config_path),
                "--manifest",
                os.fspath(manifest),
                "--payload",
                os.fspath(payload),
                "--operation-id",
                second,
                "--json",
            ],
        )
        assert returncode == 0
        assert already["changed"] is True
        assert already["state"] == "already_materialized"
        assert already["artifact_object_ref"] == build["artifact_object_ref"]
        assert already["materialization_receipt_ref"] != receipt

        returncode, raw, malformed = _invoke_artifact(
            binary,
            build_arguments[:-1],
        )
        assert returncode == 2 and raw == fixtures[3]
        assert malformed["diagnostics"][0]["code"] == "PXLC-ARTIFACT-GRAMMAR"
        assert not (workspace / "operator-v1").exists()
    finally:
        shutil.rmtree(base)
