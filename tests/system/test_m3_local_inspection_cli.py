from __future__ import annotations

import asyncio
import hashlib
import json
import os
import re
import select
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import paraegox_sdk.console_client as console_client

_BINARY_ENVIRONMENT = "PARAEGOX_M3_INSPECTION_CLI_BINARY"
_COMMAND_TIMEOUT_SECONDS = 180.0
_CLEANUP_TIMEOUT_SECONDS = 30.0
_STALE_TIMEOUT_SECONDS = 20.0
_HIDDEN_SUPERVISOR_MODE = b"__local-chat-supervisor-v1"
_CONFIG_COMMITMENT_DOMAIN = b"paraegox.local.managed-chat-config.sha256.v1"
_LOCATOR_RESPONSE_DIGEST_DOMAIN = b"paraegox.local.inspection-locator-response.v1"
_CONTROL_SOCKET_RELATIVE = Path("operator-v1/control-v1.sock")
_LIFECYCLE_RECORD_RELATIVE = Path("operator-v1/lifecycle-v1.json")
_LOCATOR_REQUEST_PREFIX = b"PXLO\x01I"
_LOCATOR_HEADER_BYTES = 160
_MAX_LOCATOR_PATH_BYTES = 4096
_MAX_LOCATOR_FRAME_BYTES = _LOCATOR_HEADER_BYTES + _MAX_LOCATOR_PATH_BYTES
_TOP_LEVEL_FIELDS = (
    "schema_version",
    "command",
    "ok",
    "changed",
    "snapshot",
    "diagnostics",
)
_SNAPSHOT_FIELDS = (
    "snapshot_version",
    "projection_id",
    "observation_clock_ref",
    "projection_revision",
    "projected_at_nanos",
    "overall",
    "projection_digest",
    "sources",
    "node",
)
_SOURCE_FIELDS = (
    "owner",
    "freshness",
    "subject_ref",
    "coordinate",
    "observed_at_nanos",
    "valid_until_nanos",
    "liveness",
    "readiness",
    "health",
    "feature_support",
    "reason",
    "owner_fact_digest",
)
_NODE_FIELDS = (
    "freshness",
    "node_ref",
    "node_incarnation_ref",
    "registration_epoch",
    "status_sequence",
    "observed_at_nanos",
    "valid_until_nanos",
    "liveness",
    "readiness",
    "health",
    "feature_support",
    "reason",
    "node_status_digest",
)
_SOURCE_OWNERS = (
    "authority",
    "deployment_controller",
    "runtime_host",
    "fabric_service",
    "agent_service",
)
_COORDINATE_FIELDS = {
    "authority": ("kind", "tenure_epoch", "fact_sequence"),
    "deployment_controller": ("kind", "revision", "fact_sequence"),
    "runtime_host": ("kind", "runtime_host_epoch", "snapshot_sequence"),
    "fabric_service": ("kind", "service_generation", "observation_sequence"),
    "agent_service": ("kind", "service_generation", "observation_sequence"),
}
_COORDINATE_KINDS = {
    "authority": "authority_tenure",
    "deployment_controller": "deployment_revision",
    "runtime_host": "runtime_host_epoch",
    "fabric_service": "fabric_service_generation",
    "agent_service": "agent_service_generation",
}
_FRESHNESS = {"fresh", "stale", "partitioned", "missing"}
_LIVENESS = {
    "unknown",
    "bootstrapping",
    "live",
    "unresponsive",
    "exited",
    "quarantined",
}
_READINESS = {"unknown", "ready", "not_ready", "degraded", "blocked"}
_HEALTH = {"unknown", "healthy", "degraded", "faulted"}
_FEATURE_SUPPORT = {"unknown", "all_required_supported", "required_unsupported"}
_REASONS = {
    "none",
    "bootstrapping",
    "dependency_unavailable",
    "owner_reported_degraded",
    "owner_reported_failure",
    "feature_unsupported",
    "quarantined",
    "outcome_uncertain",
    "source_unknown",
    "source_missing",
    "source_stale",
    "source_partitioned",
}
_OVERALL = {"ready", "degraded", "unavailable", "unknown"}
_GENERATION_PATTERN = re.compile(r"[0-9a-f]{32}")
_IDENTITY_PATTERN = re.compile(r"[0-9a-f]{32}")
_DIGEST_PATTERN = re.compile(r"[0-9a-f]{64}")
_CANONICAL_DECIMAL_PATTERN = re.compile(r"0|[1-9][0-9]*")
_OPENAI_SENTINEL = "m3-openai-secret-value-must-not-leak"
_DEEPSEEK_SENTINEL = "m3-deepseek-secret-value-must-not-leak"
_EXTRA_ARGUMENT_CANARY = "m3-extra-argument-must-not-leak"


@dataclass(frozen=True)
class InspectionResult:
    returncode: int
    envelope: dict[str, Any] | None
    stdout: bytes | None
    stderr: bytes


def _require_exact_binary() -> Path:
    configured = os.environ.get(_BINARY_ENVIRONMENT)
    assert configured is not None, (
        f"{_BINARY_ENVIRONMENT} must name the already-built binary from the exact "
        "source revision under validation"
    )
    path = Path(configured)
    assert path.is_absolute(), f"{_BINARY_ENVIRONMENT} must be absolute"
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode), "the exact M3a binary must be a regular file"
    assert not path.is_symlink(), "the exact M3a binary must not be a symlink"
    assert metadata.st_mode & 0o111 != 0, "the exact M3a binary must be executable"
    return path.resolve(strict=True)


def _copy_exact_binary(source: Path, target: Path) -> None:
    shutil.copyfile(source, target)
    target.chmod(0o755)
    metadata = target.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not target.is_symlink()
    assert metadata.st_nlink == 1
    assert metadata.st_mode & 0o7777 == 0o755


def _reserve_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def _chat_document(state_root: Path, fabric_port: int) -> str:
    return (
        "schema_version = 1\n"
        f"state_root = {json.dumps(os.fspath(state_root))}\n"
        f'fabric_listen = "tcp/127.0.0.1:{fabric_port}"\n'
        "\n[model]\n"
        'provider = "deterministic-echo-v1"\n'
    )


def _provisioned_chat_document(state_root: Path, fabric_port: int) -> str:
    return (
        "schema_version = 1\n"
        f"state_root = {json.dumps(os.fspath(state_root))}\n"
        f'fabric_listen = "tcp/127.0.0.1:{fabric_port}"\n'
        "\n[model]\n"
        'provider = "deepseek-chat-completions-v1"\n'
        'model = "deepseek-v4-flash"\n'
        'secret_ref = "env:DEEPSEEK_API_KEY"\n'
    )


def _write_config(path: Path, document: str) -> None:
    path.write_text(document, encoding="utf-8")
    path.chmod(0o600)
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_nlink == 1
    assert metadata.st_mode & 0o7777 == 0o600


def _environment(root: Path) -> dict[str, str]:
    temporary = root / "tmp"
    temporary.mkdir(mode=0o700, exist_ok=True)
    return {
        "HOME": os.fspath(root),
        "TMPDIR": os.fspath(temporary),
        "PATH": "/usr/bin:/bin",
        "LANG": "C.UTF-8",
        "OPENAI_API_KEY": _OPENAI_SENTINEL,
        "DEEPSEEK_API_KEY": _DEEPSEEK_SENTINEL,
    }


def _decode_one_compact_json_object(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n"), "inspection stdout must end in one LF"
    assert raw.count(b"\n") == 1, "inspection stdout must contain exactly one JSON line"
    value = json.loads(raw)
    assert isinstance(value, dict), "inspection stdout must be one JSON object"
    compact = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    assert raw == compact, "inspection stdout must use compact JSON framing"
    return value


def _assert_diagnostics(value: object, *, expected_count: int) -> None:
    assert isinstance(value, list)
    assert len(value) == expected_count
    for diagnostic in value:
        assert isinstance(diagnostic, dict)
        assert list(diagnostic) == ["code", "message"]
        assert isinstance(diagnostic["code"], str) and diagnostic["code"]
        assert isinstance(diagnostic["message"], str) and diagnostic["message"]


def _assert_hex(value: object, pattern: re.Pattern[str]) -> None:
    assert isinstance(value, str)
    assert pattern.fullmatch(value) is not None


def _assert_u64_string(value: object, *, optional: bool = False) -> None:
    if value is None and optional:
        return
    assert isinstance(value, str)
    assert _CANONICAL_DECIMAL_PATTERN.fullmatch(value) is not None
    assert 0 <= int(value) <= (1 << 64) - 1


def _assert_coordinate(coordinate: object, owner: str, *, missing: bool) -> None:
    if missing:
        assert coordinate is None
        return
    assert isinstance(coordinate, dict)
    assert list(coordinate) == list(_COORDINATE_FIELDS[owner])
    assert coordinate["kind"] == _COORDINATE_KINDS[owner]
    for field in _COORDINATE_FIELDS[owner][1:]:
        _assert_u64_string(coordinate[field])
        assert coordinate[field] != "0"


def _assert_source(source: object, owner: str) -> None:
    assert isinstance(source, dict)
    assert list(source) == list(_SOURCE_FIELDS)
    assert source["owner"] == owner
    assert source["freshness"] in _FRESHNESS
    _assert_hex(source["subject_ref"], _IDENTITY_PATTERN)
    missing = source["freshness"] == "missing"
    _assert_coordinate(source["coordinate"], owner, missing=missing)
    _assert_u64_string(source["observed_at_nanos"], optional=True)
    _assert_u64_string(source["valid_until_nanos"], optional=True)
    assert source["liveness"] in _LIVENESS
    assert source["readiness"] in _READINESS
    assert source["health"] in _HEALTH
    assert source["feature_support"] in _FEATURE_SUPPORT
    assert source["reason"] in _REASONS
    if missing:
        assert source["observed_at_nanos"] is None
        assert source["valid_until_nanos"] is None
        assert source["owner_fact_digest"] is None
    else:
        assert source["observed_at_nanos"] != "0"
        assert source["valid_until_nanos"] != "0"
        _assert_hex(source["owner_fact_digest"], _DIGEST_PATTERN)


def _assert_node(node: object) -> None:
    assert isinstance(node, dict)
    assert list(node) == list(_NODE_FIELDS)
    assert node["freshness"] in _FRESHNESS
    _assert_hex(node["node_ref"], _IDENTITY_PATTERN)
    _assert_hex(node["node_incarnation_ref"], _IDENTITY_PATTERN)
    missing = node["freshness"] == "missing"
    for field in (
        "registration_epoch",
        "status_sequence",
        "observed_at_nanos",
        "valid_until_nanos",
    ):
        _assert_u64_string(node[field], optional=True)
        if not missing:
            assert node[field] != "0"
    assert node["liveness"] in _LIVENESS
    assert node["readiness"] in _READINESS
    assert node["health"] in _HEALTH
    assert node["feature_support"] in _FEATURE_SUPPORT
    assert node["reason"] in _REASONS
    if missing:
        assert all(
            node[field] is None
            for field in (
                "registration_epoch",
                "status_sequence",
                "observed_at_nanos",
                "valid_until_nanos",
                "node_status_digest",
            )
        )
    else:
        _assert_hex(node["node_status_digest"], _DIGEST_PATTERN)


def _assert_snapshot(snapshot: object) -> dict[str, Any]:
    assert isinstance(snapshot, dict)
    assert list(snapshot) == list(_SNAPSHOT_FIELDS)
    assert type(snapshot["snapshot_version"]) is int
    assert snapshot["snapshot_version"] == 2
    _assert_hex(snapshot["projection_id"], _IDENTITY_PATTERN)
    _assert_hex(snapshot["observation_clock_ref"], _IDENTITY_PATTERN)
    _assert_u64_string(snapshot["projection_revision"])
    _assert_u64_string(snapshot["projected_at_nanos"])
    assert snapshot["projection_revision"] != "0"
    assert snapshot["projected_at_nanos"] != "0"
    assert snapshot["overall"] in _OVERALL
    _assert_hex(snapshot["projection_digest"], _DIGEST_PATTERN)
    assert isinstance(snapshot["sources"], list)
    assert len(snapshot["sources"]) == len(_SOURCE_OWNERS)
    for source, owner in zip(snapshot["sources"], _SOURCE_OWNERS, strict=True):
        _assert_source(source, owner)
    _assert_node(snapshot["node"])
    return snapshot


def _assert_inspection_envelope(envelope: dict[str, Any], *, ok: bool) -> None:
    assert list(envelope) == list(_TOP_LEVEL_FIELDS)
    assert type(envelope["schema_version"]) is int
    assert envelope["schema_version"] == 1
    assert envelope["command"] == "inspection.snapshot"
    assert type(envelope["ok"]) is bool and envelope["ok"] is ok
    assert envelope["changed"] is False
    if ok:
        _assert_snapshot(envelope["snapshot"])
        _assert_diagnostics(envelope["diagnostics"], expected_count=0)
    else:
        assert envelope["snapshot"] is None
        _assert_diagnostics(envelope["diagnostics"], expected_count=1)


def _assert_public_output_is_redacted(
    result: InspectionResult,
    *,
    config_path: Path | str,
    state_root: Path,
    additional_forbidden: tuple[bytes, ...] = (),
) -> None:
    combined = (result.stdout or b"") + result.stderr
    for forbidden in (
        os.fsencode(config_path),
        os.fsencode(state_root),
        b"OPENAI_API_KEY",
        b"DEEPSEEK_API_KEY",
        b"env:OPENAI_API_KEY",
        b"env:DEEPSEEK_API_KEY",
        _OPENAI_SENTINEL.encode(),
        _DEEPSEEK_SENTINEL.encode(),
        _EXTRA_ARGUMENT_CANARY.encode(),
        *additional_forbidden,
    ):
        assert forbidden not in combined
    sensitive_term = (
        rb"(?i)\b(?:pid|pgid|uid|gid|secretref|credential|seed|private[-_ ]key|"
        rb"capability|token|state_root|config_path)\b"
    )
    assert re.search(sensitive_term, combined) is None


def _spawn_inspection(
    binary: Path,
    config_argument: Path | str,
    environment: dict[str, str],
    *,
    extra_arguments: tuple[str, ...] = (),
    stdout: int | None = subprocess.PIPE,
    pass_fds: tuple[int, ...] = (),
    command_prefix: tuple[str, ...] = (),
) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [
            *command_prefix,
            os.fspath(binary),
            "inspection",
            "snapshot",
            "--config",
            os.fspath(config_argument),
            "--json",
            *extra_arguments,
        ],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=stdout,
        stderr=subprocess.PIPE,
        pass_fds=pass_fds,
    )


def _finish_inspection(
    process: subprocess.Popen[bytes],
    config_path: Path | str,
    state_root: Path,
    *,
    expected_returncode: int,
    additional_forbidden: tuple[bytes, ...] = (),
) -> InspectionResult:
    try:
        stdout, stderr = process.communicate(timeout=_COMMAND_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        stdout, stderr = process.communicate()
        raise AssertionError(
            f"inspection exceeded {_COMMAND_TIMEOUT_SECONDS}s; "
            f"stdout={stdout!r}; stderr={stderr!r}"
        ) from error
    assert process.returncode == expected_returncode, (
        f"inspection exited {process.returncode}; stdout={stdout!r}; stderr={stderr!r}"
    )
    assert stderr == b"", f"exact inspection grammar must keep stderr empty: {stderr!r}"
    envelope = None if stdout is None else _decode_one_compact_json_object(stdout)
    if envelope is not None:
        _assert_inspection_envelope(envelope, ok=expected_returncode == 0)
    result = InspectionResult(process.returncode, envelope, stdout, stderr)
    _assert_public_output_is_redacted(
        result,
        config_path=config_path,
        state_root=state_root,
        additional_forbidden=additional_forbidden,
    )
    return result


def _invoke_inspection(
    binary: Path,
    config_path: Path | str,
    state_root: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int,
    extra_arguments: tuple[str, ...] = (),
    command_prefix: tuple[str, ...] = (),
    additional_forbidden: tuple[bytes, ...] = (),
) -> InspectionResult:
    return _finish_inspection(
        _spawn_inspection(
            binary,
            config_path,
            environment,
            extra_arguments=extra_arguments,
            command_prefix=command_prefix,
        ),
        config_path,
        state_root,
        expected_returncode=expected_returncode,
        additional_forbidden=additional_forbidden,
    )


def _invoke_lifecycle(
    binary: Path,
    command: str,
    config_path: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int = 0,
) -> dict[str, Any]:
    process = subprocess.run(
        [os.fspath(binary), command, "--config", os.fspath(config_path), "--json"],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=_COMMAND_TIMEOUT_SECONDS,
        check=False,
    )
    assert process.returncode == expected_returncode, (
        f"{command} exited {process.returncode}; stdout={process.stdout!r}; "
        f"stderr={process.stderr!r}"
    )
    assert process.stderr == b""
    return _decode_one_compact_json_object(process.stdout)


def _invoke_deploy_projection(
    binary: Path,
    config_path: Path,
    environment: dict[str, str],
) -> dict[str, Any]:
    process = subprocess.run(
        [
            os.fspath(binary),
            "deploy",
            "--local",
            "--config",
            os.fspath(config_path),
            "--json",
        ],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=_COMMAND_TIMEOUT_SECONDS,
        check=False,
    )
    assert process.returncode == 0, (
        f"deploy correlation query exited {process.returncode}; "
        f"stdout={process.stdout!r}; stderr={process.stderr!r}"
    )
    assert process.stderr == b""
    envelope = _decode_one_compact_json_object(process.stdout)
    assert envelope["command"] == "deploy"
    assert envelope["ok"] is True
    assert envelope["generation"] is not None
    return envelope


def _diagnostic_code(result: InspectionResult) -> str:
    assert result.envelope is not None
    diagnostics = result.envelope["diagnostics"]
    assert isinstance(diagnostics, list) and len(diagnostics) == 1
    code = diagnostics[0]["code"]
    assert isinstance(code, str)
    return code


def _matching_processes(binary: Path) -> set[int]:
    expected = binary.stat()
    matches: set[int] = set()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            observed = (entry / "exe").stat()
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        if (observed.st_dev, observed.st_ino) == (expected.st_dev, expected.st_ino):
            matches.add(int(entry.name))
    return matches


def _process_command_line(process_id: int) -> tuple[bytes, ...]:
    raw = (Path("/proc") / str(process_id) / "cmdline").read_bytes()
    return tuple(argument for argument in raw.split(b"\0") if argument)


def _assert_single_owner_graph(
    binary: Path,
    *,
    ignored_process_ids: frozenset[int] = frozenset(),
) -> tuple[set[int], int]:
    all_processes = _matching_processes(binary)
    assert ignored_process_ids <= all_processes, (
        "an explicitly ignored harness client is no longer live"
    )
    processes = all_processes - ignored_process_ids
    assert len(processes) >= 2, "Inspection success lacks the supervisor and real Node child"
    supervisors = {
        process_id
        for process_id in processes
        if _HIDDEN_SUPERVISOR_MODE in _process_command_line(process_id)
    }
    assert len(supervisors) == 1, "one M3a generation must have one supervisor"
    supervisor = next(iter(supervisors))
    assert os.getsid(supervisor) == supervisor
    assert {os.getsid(process_id) for process_id in processes} == {supervisor}
    return processes, supervisor


def _wait_for_no_matching_processes(binary: Path) -> None:
    deadline = time.monotonic() + _CLEANUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if not _matching_processes(binary):
            return
        time.sleep(0.05)
    assert not _matching_processes(binary), "joined down left a ParaEGOX process alive"


def _wait_for_exact_matching_processes(
    binary: Path,
    expected_process_ids: frozenset[int],
) -> None:
    assert expected_process_ids, "exact process waiting is only for a known blocked client"
    deadline = time.monotonic() + _CLEANUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if _matching_processes(binary) == expected_process_ids:
            return
        time.sleep(0.05)
    assert _matching_processes(binary) == expected_process_ids, (
        "joined down did not leave exactly the intentionally blocked harness client"
    )


def _terminate_private_processes(binary: Path) -> None:
    # Harness-only reclamation after a failed assertion. This is not product
    # orphan-recovery evidence.
    for requested_signal in (signal.SIGTERM, signal.SIGKILL):
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            matches = _matching_processes(binary)
            if not matches:
                return
            for process_id in matches:
                try:
                    os.kill(process_id, requested_signal)
                except ProcessLookupError:
                    pass
            time.sleep(0.05)


def _tree_fingerprint(root: Path) -> tuple[tuple[object, ...], ...]:
    fingerprint: list[tuple[object, ...]] = []
    for path in sorted(root.rglob("*"), key=lambda item: os.fsencode(item)):
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            raise AssertionError(f"state entry disappeared while fingerprinting: {path.name}")
        if stat.S_ISREG(metadata.st_mode):
            kind = "regular"
            content = hashlib.sha256(path.read_bytes()).hexdigest()
        elif stat.S_ISDIR(metadata.st_mode):
            kind = "directory"
            content = None
        elif stat.S_ISSOCK(metadata.st_mode):
            kind = "socket"
            content = None
        elif stat.S_ISLNK(metadata.st_mode):
            kind = "symlink"
            content = os.readlink(path)
        else:
            kind = "other"
            content = None
        fingerprint.append(
            (
                os.fspath(path.relative_to(root)),
                kind,
                metadata.st_dev,
                metadata.st_ino,
                stat.S_IMODE(metadata.st_mode),
                metadata.st_uid,
                metadata.st_gid,
                metadata.st_nlink,
                metadata.st_size,
                metadata.st_mtime_ns,
                metadata.st_ctime_ns,
                content,
            )
        )
    return tuple(fingerprint)


def _assert_secret_material_absent_from_files(
    root: Path,
    *,
    exact_binary: Path,
    public_input_files: tuple[Path, ...],
) -> None:
    forbidden = (
        _OPENAI_SENTINEL.encode(),
        _DEEPSEEK_SENTINEL.encode(),
        b"env:OPENAI_API_KEY",
        b"env:DEEPSEEK_API_KEY",
    )
    overlap = max(map(len, forbidden)) - 1
    for path in root.rglob("*"):
        if path == exact_binary or path in public_input_files:
            continue
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            continue
        if not stat.S_ISREG(metadata.st_mode):
            continue
        carry = b""
        with path.open("rb") as source:
            while chunk := source.read(64 * 1024):
                payload = carry + chunk
                for fragment in forbidden:
                    assert fragment not in payload, f"Secret material leaked into {path.name}"
                carry = payload[-overlap:]


def _managed_chat_config_commitment(document: str) -> bytes:
    payload = document.encode()
    digest = hashlib.sha256()
    digest.update(_CONFIG_COMMITMENT_DOMAIN)
    digest.update(len(payload).to_bytes(8, "big"))
    digest.update(payload)
    return digest.digest()


def _raw_locator_query(
    state_root: Path,
    document: str,
    expected_generation: str,
) -> bytes:
    request = (
        _LOCATOR_REQUEST_PREFIX
        + _managed_chat_config_commitment(document)
        + bytes.fromhex(expected_generation)
    )
    assert len(request) == 54
    response = bytearray()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(10.0)
        client.connect(os.fspath(state_root / _CONTROL_SOCKET_RELATIVE))
        client.sendall(request)
        client.shutdown(socket.SHUT_WR)
        while True:
            try:
                chunk = client.recv(4096)
            except ConnectionResetError:
                break
            if not chunk:
                break
            response.extend(chunk)
            assert len(response) <= _MAX_LOCATOR_FRAME_BYTES + 1
    return bytes(response)


def _decode_locator_response(
    raw: bytes,
    *,
    expected_generation: str,
    expected_commitment: bytes,
) -> Path:
    assert _LOCATOR_HEADER_BYTES < len(raw) <= _MAX_LOCATOR_FRAME_BYTES
    assert raw[:4] == b"PXIL"
    assert raw[4:6] == (1).to_bytes(2, "big")
    assert raw[6:8] == b"IR"
    assert raw[8:10] == _LOCATOR_HEADER_BYTES.to_bytes(2, "big")
    assert raw[10:12] == bytes(2)
    assert int.from_bytes(raw[12:16], "big") == len(raw)
    path_length = int.from_bytes(raw[16:20], "big")
    content_length = int.from_bytes(raw[20:24], "big")
    assert 1 <= path_length <= _MAX_LOCATOR_PATH_BYTES
    assert _LOCATOR_HEADER_BYTES + path_length == len(raw)
    assert content_length > 0
    assert raw[24:40] == bytes.fromhex(expected_generation)
    assert raw[40:72] == expected_commitment
    assert any(raw[72:104]), "PXIB content digest must be nonzero"
    assert int.from_bytes(raw[104:112], "big") > 0
    assert int.from_bytes(raw[112:120], "big") > 0
    assert raw[120:128] == bytes(8)
    path_bytes = raw[_LOCATOR_HEADER_BYTES:]
    expected_digest = hashlib.sha256(
        _LOCATOR_RESPONSE_DIGEST_DOMAIN + raw[:128] + path_bytes
    ).digest()
    assert raw[128:160] == expected_digest
    path_text = path_bytes.decode("utf-8")
    path = Path(path_text)
    assert path.is_absolute()
    assert os.path.normpath(path_text) == path_text
    assert os.fsencode(path) == path_bytes
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_dev == int.from_bytes(raw[104:112], "big")
    assert metadata.st_ino == int.from_bytes(raw[112:120], "big")
    assert metadata.st_size == content_length
    assert hashlib.sha256(path.read_bytes()).digest() == raw[72:104]
    return path


def _load_private_snapshot_once(path: Path) -> console_client.LocalInspectionSnapshotV2:
    client = console_client.DeveloperLocalInspectionClientV2.from_private_bootstrap_file(path)
    try:
        return asyncio.run(client.latest())
    finally:
        client.close()


def _enum_name(value: object) -> str:
    name = getattr(value, "name", None)
    assert isinstance(name, str)
    return name.lower()


def _private_coordinate_projection(
    coordinate: console_client.InspectionSourceCoordinateV1 | None,
) -> dict[str, str] | None:
    if coordinate is None:
        return None
    owner = _enum_name(coordinate.owner)
    fields = _COORDINATE_FIELDS[owner]
    return {
        "kind": _COORDINATE_KINDS[owner],
        fields[1]: str(coordinate.value),
        fields[2]: str(coordinate.sequence),
    }


def _private_snapshot_projection(
    value: console_client.LocalInspectionSnapshotV2,
) -> dict[str, Any]:
    sources = []
    for record in value.base_snapshot.records:
        sources.append(
            {
                "owner": _enum_name(record.owner),
                "freshness": _enum_name(record.freshness),
                "subject_ref": record.subject_ref.hex(),
                "coordinate": _private_coordinate_projection(record.coordinate),
                "observed_at_nanos": (
                    None if record.observed_at_nanos is None else str(record.observed_at_nanos)
                ),
                "valid_until_nanos": (
                    None if record.valid_until_nanos is None else str(record.valid_until_nanos)
                ),
                "liveness": _enum_name(record.liveness),
                "readiness": _enum_name(record.readiness),
                "health": _enum_name(record.health),
                "feature_support": _enum_name(record.feature_support),
                "reason": _enum_name(record.reason),
                "owner_fact_digest": (
                    None if record.owner_fact_digest is None else record.owner_fact_digest.hex()
                ),
            }
        )
    node = value.node
    return {
        "snapshot_version": 2,
        "projection_id": value.projection_id.hex(),
        "observation_clock_ref": value.observation_clock_ref.hex(),
        "projection_revision": str(value.projection_revision),
        "projected_at_nanos": str(value.projected_at_nanos),
        "overall": _enum_name(value.overall),
        "projection_digest": value.projection_digest.hex(),
        "sources": sources,
        "node": {
            "freshness": _enum_name(node.freshness),
            "node_ref": node.node_ref.hex(),
            "node_incarnation_ref": node.node_incarnation_ref.hex(),
            "registration_epoch": (
                None if node.registration_epoch is None else str(node.registration_epoch)
            ),
            "status_sequence": (
                None if node.status_sequence is None else str(node.status_sequence)
            ),
            "observed_at_nanos": (
                None if node.observed_at_nanos is None else str(node.observed_at_nanos)
            ),
            "valid_until_nanos": (
                None if node.valid_until_nanos is None else str(node.valid_until_nanos)
            ),
            "liveness": _enum_name(node.liveness),
            "readiness": _enum_name(node.readiness),
            "health": _enum_name(node.health),
            "feature_support": _enum_name(node.feature_support),
            "reason": _enum_name(node.reason),
            "node_status_digest": (
                None if node.node_status_digest is None else node.node_status_digest.hex()
            ),
        },
    }


def _assert_initial_owner_semantics(snapshot: dict[str, Any]) -> None:
    assert snapshot["projection_revision"] == "1"
    assert snapshot["overall"] == "unknown"
    sources = snapshot["sources"]
    for source in sources:
        assert source["freshness"] == "fresh"
        assert source["health"] == "unknown"
        assert source["feature_support"] == "all_required_supported"
        assert source["reason"] == "source_unknown"
    assert [source["liveness"] for source in sources] == ["unknown"] * 5
    assert [source["readiness"] for source in sources] == [
        "unknown",
        "unknown",
        "ready",
        "ready",
        "ready",
    ]
    node = snapshot["node"]
    assert node["freshness"] == "fresh"
    assert node["liveness"] == "live"
    assert node["readiness"] == "unknown"
    assert node["health"] == "unknown"
    assert node["feature_support"] == "all_required_supported"
    assert node["reason"] == "source_unknown"


def _assert_stale_projection(
    initial: dict[str, Any],
    stale: dict[str, Any],
) -> None:
    assert stale["projection_id"] == initial["projection_id"]
    assert stale["observation_clock_ref"] == initial["observation_clock_ref"]
    assert stale["projection_revision"] == "2"
    assert int(stale["projected_at_nanos"]) > int(initial["projected_at_nanos"])
    assert stale["overall"] == "unknown"
    for first, later in zip(initial["sources"], stale["sources"], strict=True):
        for retained in (
            "owner",
            "subject_ref",
            "coordinate",
            "observed_at_nanos",
            "valid_until_nanos",
            "owner_fact_digest",
        ):
            assert later[retained] == first[retained]
        assert later["freshness"] == "stale"
        assert later["liveness"] == "unknown"
        assert later["readiness"] == "unknown"
        assert later["health"] == "unknown"
        assert later["feature_support"] == "unknown"
        assert later["reason"] == "source_stale"
        assert int(stale["projected_at_nanos"]) > int(later["valid_until_nanos"])
    for retained in (
        "node_ref",
        "node_incarnation_ref",
        "registration_epoch",
        "status_sequence",
        "observed_at_nanos",
        "valid_until_nanos",
        "node_status_digest",
    ):
        assert stale["node"][retained] == initial["node"][retained]
    assert stale["node"]["freshness"] == "stale"
    assert stale["node"]["liveness"] == "unknown"
    assert stale["node"]["readiness"] == "unknown"
    assert stale["node"]["health"] == "unknown"
    assert stale["node"]["feature_support"] == "unknown"
    assert stale["node"]["reason"] == "source_stale"
    assert int(stale["projected_at_nanos"]) > int(stale["node"]["valid_until_nanos"])


def _wait_for_stale_snapshot(
    binary: Path,
    config_path: Path,
    state_root: Path,
    environment: dict[str, str],
    initial: dict[str, Any],
) -> dict[str, Any]:
    latest_valid_until = max(
        int(source["valid_until_nanos"]) for source in initial["sources"]
    )
    latest_valid_until = max(latest_valid_until, int(initial["node"]["valid_until_nanos"]))
    remaining_nanos = max(0, latest_valid_until - int(initial["projected_at_nanos"]))
    deadline = time.monotonic() + min(
        _STALE_TIMEOUT_SECONDS,
        remaining_nanos / 1_000_000_000 + 5.0,
    )
    while time.monotonic() < deadline:
        result = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert result.envelope is not None
        snapshot = result.envelope["snapshot"]
        assert isinstance(snapshot, dict)
        if snapshot["projection_revision"] == "2":
            return snapshot
        assert snapshot["projection_revision"] == "1"
        time.sleep(0.1)
    raise AssertionError("real Inspection owner did not publish stale revision 2 in time")


def _require_passwordless_sudo() -> str:
    sudo = shutil.which("sudo")
    assert sudo is not None, "the admitted Ubuntu M3a harness requires passwordless sudo"
    preflight = subprocess.run(
        [sudo, "-n", "true"],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=10.0,
        check=False,
    )
    assert preflight.returncode == 0, (
        "the admitted Ubuntu M3a harness requires passwordless sudo"
    )
    return sudo


_INSPECTION_INTERPOSER_SOURCE = r"""
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <unistd.h>

static const unsigned char locator_prefix[6] = {'P', 'X', 'L', 'O', 1, 'I'};
static __thread int inside_interposer = 0;
static int bootstrap_open_observed = 0;

static int configured_fd(const char *name) {
    const char *raw = getenv(name);
    if (raw == NULL || *raw == '\0') {
        return -1;
    }
    char *end = NULL;
    long value = strtol(raw, &end, 10);
    if (end == raw || *end != '\0' || value < 0 || value > 1048576) {
        return -1;
    }
    return (int)value;
}

static void report_marker(char marker) {
    int audit_fd = configured_fd("PARAEGOX_M3_AUDIT_FD");
    if (audit_fd >= 0) {
        (void)syscall(SYS_write, audit_fd, &marker, 1);
    }
}

static int await_release(void) {
    int release_fd = configured_fd("PARAEGOX_M3_RELEASE_FD");
    char release = 0;
    if (release_fd < 0 || syscall(SYS_read, release_fd, &release, 1) != 1) {
        errno = EIO;
        return -1;
    }
    return 0;
}

static int is_locator_request(const void *buffer, size_t count) {
    return count >= sizeof(locator_prefix)
        && memcmp(buffer, locator_prefix, sizeof(locator_prefix)) == 0;
}

static int is_latest_request(const void *buffer, size_t count) {
    if (count < 45) {
        return 0;
    }
    const unsigned char *bytes = buffer;
    return memcmp(bytes + 32, "PXIQ", 4) == 0
        && bytes[36] == 0
        && bytes[37] == 2
        && bytes[44] == 1;
}

static size_t copy_iov_prefix(
    unsigned char *output,
    size_t capacity,
    const struct iovec *iov,
    size_t iovcnt
) {
    size_t copied = 0;
    for (size_t index = 0; index < iovcnt && copied < capacity; ++index) {
        size_t available = iov[index].iov_len;
        size_t wanted = capacity - copied;
        size_t take = available < wanted ? available : wanted;
        memcpy(output + copied, iov[index].iov_base, take);
        copied += take;
    }
    return copied;
}

static int before_request(const void *buffer, size_t count) {
    if (inside_interposer) {
        return 0;
    }
    int locator = is_locator_request(buffer, count);
    int latest = is_latest_request(buffer, count);
    if (!locator && !latest) {
        return 0;
    }
    inside_interposer = 1;
    report_marker(locator ? 'L' : 'Q');
    const char *mode = getenv("PARAEGOX_M3_INTERPOSE_MODE");
    int failure = 0;
    if (mode != NULL && locator && strcmp(mode, "fail_locator") == 0) {
        errno = EIO;
        failure = -1;
    } else if (mode != NULL && latest && strcmp(mode, "fail_latest") == 0) {
        errno = EIO;
        failure = -1;
    } else if (mode != NULL && latest && strcmp(mode, "barrier_latest") == 0) {
        failure = await_release();
    }
    inside_interposer = 0;
    return failure;
}

static int before_request_iov(const struct iovec *iov, size_t iovcnt) {
    unsigned char observed[45];
    size_t copied = copy_iov_prefix(observed, sizeof(observed), iov, iovcnt);
    return before_request(observed, copied);
}

static int is_pxib_path(const char *path) {
    if (path == NULL) {
        return 0;
    }
    size_t length = strlen(path);
    return length >= 5 && strcmp(path + length - 5, ".pxib") == 0;
}

static int before_open(const char *path, int flags) {
    const char *mode = getenv("PARAEGOX_M3_INTERPOSE_MODE");
    if (
        inside_interposer
        || mode == NULL
        || strcmp(mode, "barrier_bootstrap_open") != 0
        || !is_pxib_path(path)
        || (flags & O_CREAT) != 0
        || (flags & O_ACCMODE) != O_RDONLY
        || bootstrap_open_observed
    ) {
        return 0;
    }
    inside_interposer = 1;
    bootstrap_open_observed = 1;
    report_marker('O');
    int result = await_release();
    inside_interposer = 0;
    return result;
}

ssize_t write(int fd, const void *buffer, size_t count) {
    static ssize_t (*real_write)(int, const void *, size_t) = NULL;
    if (real_write == NULL) {
        real_write = dlsym(RTLD_NEXT, "write");
    }
    if (before_request(buffer, count) != 0) {
        return -1;
    }
    return real_write(fd, buffer, count);
}

ssize_t send(int fd, const void *buffer, size_t count, int flags) {
    static ssize_t (*real_send)(int, const void *, size_t, int) = NULL;
    if (real_send == NULL) {
        real_send = dlsym(RTLD_NEXT, "send");
    }
    if (before_request(buffer, count) != 0) {
        return -1;
    }
    return real_send(fd, buffer, count, flags);
}

ssize_t sendto(
    int fd,
    const void *buffer,
    size_t count,
    int flags,
    const struct sockaddr *address,
    socklen_t address_length
) {
    static ssize_t (*real_sendto)(
        int, const void *, size_t, int, const struct sockaddr *, socklen_t
    ) = NULL;
    if (real_sendto == NULL) {
        real_sendto = dlsym(RTLD_NEXT, "sendto");
    }
    if (before_request(buffer, count) != 0) {
        return -1;
    }
    return real_sendto(fd, buffer, count, flags, address, address_length);
}

ssize_t sendmsg(int fd, const struct msghdr *message, int flags) {
    static ssize_t (*real_sendmsg)(int, const struct msghdr *, int) = NULL;
    if (real_sendmsg == NULL) {
        real_sendmsg = dlsym(RTLD_NEXT, "sendmsg");
    }
    if (
        message != NULL
        && before_request_iov(message->msg_iov, message->msg_iovlen) != 0
    ) {
        return -1;
    }
    return real_sendmsg(fd, message, flags);
}

ssize_t writev(int fd, const struct iovec *iov, int iovcnt) {
    static ssize_t (*real_writev)(int, const struct iovec *, int) = NULL;
    if (real_writev == NULL) {
        real_writev = dlsym(RTLD_NEXT, "writev");
    }
    if (iovcnt > 0 && before_request_iov(iov, (size_t)iovcnt) != 0) {
        return -1;
    }
    return real_writev(fd, iov, iovcnt);
}

int open(const char *path, int flags, ...) {
    static int (*real_open)(const char *, int, ...) = NULL;
    if (real_open == NULL) {
        real_open = dlsym(RTLD_NEXT, "open");
    }
    mode_t creation_mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list arguments;
        va_start(arguments, flags);
        creation_mode = va_arg(arguments, mode_t);
        va_end(arguments);
    }
    if (before_open(path, flags) != 0) {
        return -1;
    }
    if ((flags & O_CREAT) != 0) {
        return real_open(path, flags, creation_mode);
    }
    return real_open(path, flags);
}

int open64(const char *path, int flags, ...) {
    static int (*real_open64)(const char *, int, ...) = NULL;
    if (real_open64 == NULL) {
        real_open64 = dlsym(RTLD_NEXT, "open64");
    }
    mode_t creation_mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list arguments;
        va_start(arguments, flags);
        creation_mode = va_arg(arguments, mode_t);
        va_end(arguments);
    }
    if (before_open(path, flags) != 0) {
        return -1;
    }
    if ((flags & O_CREAT) != 0) {
        return real_open64(path, flags, creation_mode);
    }
    return real_open64(path, flags);
}

int openat(int directory_fd, const char *path, int flags, ...) {
    static int (*real_openat)(int, const char *, int, ...) = NULL;
    if (real_openat == NULL) {
        real_openat = dlsym(RTLD_NEXT, "openat");
    }
    mode_t creation_mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list arguments;
        va_start(arguments, flags);
        creation_mode = va_arg(arguments, mode_t);
        va_end(arguments);
    }
    if (before_open(path, flags) != 0) {
        return -1;
    }
    if ((flags & O_CREAT) != 0) {
        return real_openat(directory_fd, path, flags, creation_mode);
    }
    return real_openat(directory_fd, path, flags);
}

int openat64(int directory_fd, const char *path, int flags, ...) {
    static int (*real_openat64)(int, const char *, int, ...) = NULL;
    if (real_openat64 == NULL) {
        real_openat64 = dlsym(RTLD_NEXT, "openat64");
    }
    mode_t creation_mode = 0;
    if ((flags & O_CREAT) != 0) {
        va_list arguments;
        va_start(arguments, flags);
        creation_mode = va_arg(arguments, mode_t);
        va_end(arguments);
    }
    if (before_open(path, flags) != 0) {
        return -1;
    }
    if ((flags & O_CREAT) != 0) {
        return real_openat64(directory_fd, path, flags, creation_mode);
    }
    return real_openat64(directory_fd, path, flags);
}
"""


def _compile_inspection_interposer(root: Path) -> Path:
    source = root / "inspection-interposer.c"
    library = root / "inspection-interposer.so"
    source.write_text(_INSPECTION_INTERPOSER_SOURCE, encoding="utf-8")
    completed = subprocess.run(
        [
            "cc",
            "-shared",
            "-fPIC",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-o",
            os.fspath(library),
            os.fspath(source),
            "-ldl",
        ],
        capture_output=True,
        timeout=30.0,
        check=False,
    )
    assert completed.returncode == 0, (
        f"Inspection interposer compilation failed: {completed.stderr!r}"
    )
    return library


def _spawn_interposed_inspection(
    binary: Path,
    config_path: Path,
    environment: dict[str, str],
    library: Path,
    *,
    mode: str,
) -> tuple[subprocess.Popen[bytes], int, int]:
    audit_read, audit_write = os.pipe()
    release_read, release_write = os.pipe()
    injected = environment.copy()
    injected.update(
        {
            "LD_PRELOAD": os.fspath(library),
            "PARAEGOX_M3_INTERPOSE_MODE": mode,
            "PARAEGOX_M3_AUDIT_FD": str(audit_write),
            "PARAEGOX_M3_RELEASE_FD": str(release_read),
        }
    )
    try:
        process = _spawn_inspection(
            binary,
            config_path,
            injected,
            pass_fds=(audit_write, release_read),
        )
    except BaseException:
        os.close(audit_read)
        os.close(audit_write)
        os.close(release_read)
        os.close(release_write)
        raise
    os.close(audit_write)
    os.close(release_read)
    return process, audit_read, release_write


def _spawn_interposed_lifecycle_up(
    binary: Path,
    config_path: Path,
    environment: dict[str, str],
    library: Path,
) -> tuple[subprocess.Popen[bytes], int, int]:
    audit_read, audit_write = os.pipe()
    release_read, release_write = os.pipe()
    injected = environment.copy()
    injected.update(
        {
            "LD_PRELOAD": os.fspath(library),
            "PARAEGOX_M3_INTERPOSE_MODE": "barrier_bootstrap_open",
            "PARAEGOX_M3_AUDIT_FD": str(audit_write),
            "PARAEGOX_M3_RELEASE_FD": str(release_read),
        }
    )
    try:
        process = subprocess.Popen(
            [os.fspath(binary), "up", "--config", os.fspath(config_path), "--json"],
            cwd=binary.parent,
            env=injected,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            pass_fds=(audit_write, release_read),
        )
    except BaseException:
        os.close(audit_read)
        os.close(audit_write)
        os.close(release_read)
        os.close(release_write)
        raise
    os.close(audit_write)
    os.close(release_read)
    return process, audit_read, release_write


def _finish_lifecycle_process(
    process: subprocess.Popen[bytes],
    *,
    allowed_returncodes: set[int],
) -> dict[str, Any]:
    try:
        stdout, stderr = process.communicate(timeout=_COMMAND_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        stdout, stderr = process.communicate()
        raise AssertionError(
            f"lifecycle race process timed out; stdout={stdout!r}; stderr={stderr!r}"
        ) from error
    assert process.returncode in allowed_returncodes, (
        f"lifecycle race exited {process.returncode}; stdout={stdout!r}; stderr={stderr!r}"
    )
    assert stderr == b""
    assert stdout is not None
    return _decode_one_compact_json_object(stdout)


def _wait_for_lifecycle_state(state_root: Path, expected: str) -> None:
    record_path = state_root / _LIFECYCLE_RECORD_RELATIVE
    deadline = time.monotonic() + 30.0
    while time.monotonic() < deadline:
        try:
            record = json.loads(record_path.read_bytes())
        except (FileNotFoundError, json.JSONDecodeError):
            time.sleep(0.05)
            continue
        if isinstance(record, dict) and record.get("state") == expected:
            return
        time.sleep(0.05)
    raise AssertionError(f"lifecycle record did not reach {expected!r}")


def _wait_for_audit_marker(audit_fd: int, expected: bytes) -> bytes:
    observed = bytearray()
    deadline = time.monotonic() + 60.0
    while expected not in observed and time.monotonic() < deadline:
        readable, _, _ = select.select([audit_fd], [], [], 0.25)
        if readable:
            chunk = os.read(audit_fd, 64)
            if not chunk:
                break
            observed.extend(chunk)
    assert expected in observed, (
        f"Inspection interposer did not report {expected!r}: {observed!r}"
    )
    return bytes(observed)


def _read_available_audit(audit_fd: int, observed: bytes = b"") -> bytes:
    result = bytearray(observed)
    while True:
        readable, _, _ = select.select([audit_fd], [], [], 0.2)
        if not readable:
            return bytes(result)
        chunk = os.read(audit_fd, 64)
        if not chunk:
            return bytes(result)
        result.extend(chunk)


def test_inspection_never_started_grammar_config_and_root_are_pre_effect() -> None:
    assert sys.platform.startswith("linux"), "M3a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m3-pre-"))
    created.chmod(0o700)
    binary = created / "paraegox"
    _copy_exact_binary(source_binary, binary)
    state_root = created / "state"
    config_path = created / "paraegox.toml"
    document = _chat_document(state_root, _reserve_loopback_port())
    _write_config(config_path, document)
    environment = _environment(created)
    try:
        assert not state_root.exists()
        assert _matching_processes(binary) == set()

        never_started = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=1,
        )
        assert _diagnostic_code(never_started) == "PXLC-INSPECTION-NOT-RUNNING"
        assert not state_root.exists()
        assert _matching_processes(binary) == set()

        relative_canary = "relative-m3-config-path-must-not-leak.toml"
        relative = _invoke_inspection(
            binary,
            relative_canary,
            state_root,
            environment,
            expected_returncode=2,
            additional_forbidden=(relative_canary.encode(),),
        )
        assert _diagnostic_code(relative) == "PXLC-CONFIG-PATH-INVALID"
        assert not (binary.parent / relative_canary).exists()

        malformed_path = created / "malformed-secret-canary.toml"
        _write_config(
            malformed_path,
            'schema_version = 1\nunknown_secret_field = "do-not-echo-this"\n',
        )
        malformed = _invoke_inspection(
            binary,
            malformed_path,
            state_root,
            environment,
            expected_returncode=2,
            additional_forbidden=(b"do-not-echo-this",),
        )
        assert _diagnostic_code(malformed) == "PXLC-CONFIG-DOCUMENT-INVALID"

        symlink_path = created / "config-symlink-canary.toml"
        symlink_path.symlink_to(config_path)
        symlink = _invoke_inspection(
            binary,
            symlink_path,
            state_root,
            environment,
            expected_returncode=2,
        )
        assert _diagnostic_code(symlink) == "PXLC-CONFIG-PATH-INVALID"
        assert symlink_path.is_symlink()

        extra = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=2,
            extra_arguments=(f"--{_EXTRA_ARGUMENT_CANARY}",),
        )
        assert _diagnostic_code(extra) == "PXLC-INSPECTION-GRAMMAR"

        sudo = _require_passwordless_sudo()
        root_environment = environment.copy()
        root_environment["PATH"] = os.environ.get("PATH", "/usr/bin:/bin")
        root_rejected = _invoke_inspection(
            binary,
            config_path,
            state_root,
            root_environment,
            expected_returncode=1,
            command_prefix=(sudo, "-n", "--"),
        )
        assert _diagnostic_code(root_rejected) == "PXLC-EXECUTION-IDENTITY"

        assert not state_root.exists(), (
            "never-started Inspection rejection created lifecycle or domain state"
        )
        assert _matching_processes(binary) == set(), (
            "never-started Inspection rejection started an owner"
        )
    finally:
        _terminate_private_processes(binary)
        shutil.rmtree(created)


def test_running_snapshot_matches_owner_projection_and_stale_revision_two() -> None:
    assert sys.platform.startswith("linux"), "M3a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m3-run-"))
    created.chmod(0o700)
    binary = created / "paraegox"
    _copy_exact_binary(source_binary, binary)
    state_root = created / "state"
    config_path = created / "paraegox.toml"
    document = _chat_document(state_root, _reserve_loopback_port())
    _write_config(config_path, document)
    environment = _environment(created)
    try:
        up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert up["ok"] is True
        assert up["state"] == "running"
        assert up["changed"] is True
        assert up["owner_readiness_observed"] is True
        generation = up["generation"]
        assert isinstance(generation, str)
        assert _GENERATION_PATTERN.fullmatch(generation) is not None
        ready_processes, _ = _assert_single_owner_graph(binary)

        before_snapshot = _tree_fingerprint(state_root)
        initial_result = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert initial_result.envelope is not None
        initial = initial_result.envelope["snapshot"]
        assert isinstance(initial, dict)
        _assert_initial_owner_semantics(initial)
        assert _tree_fingerprint(state_root) == before_snapshot, (
            "read-only public snapshot modified lifecycle or domain files"
        )
        assert _matching_processes(binary) == ready_processes, (
            "read-only public snapshot changed the owner process graph"
        )

        locator_wire = _raw_locator_query(state_root, document, generation)
        locator_path = _decode_locator_response(
            locator_wire,
            expected_generation=generation,
            expected_commitment=_managed_chat_config_commitment(document),
        )
        assert initial_result.stdout is not None
        assert os.fsencode(locator_path) not in initial_result.stdout
        private_snapshot = _load_private_snapshot_once(locator_path)
        assert initial == _private_snapshot_projection(private_snapshot), (
            "public serializer did not exactly preserve the strict PXIS-v2 projection"
        )

        deployment = _invoke_deploy_projection(binary, config_path, environment)
        deployment_source = initial["sources"][1]
        assert deployment_source["coordinate"] == {
            "kind": "deployment_revision",
            "revision": deployment["deployment_revision"],
            "fact_sequence": deployment["controller_snapshot_sequence"],
        }
        assert (
            deployment_source["owner_fact_digest"]
            == deployment["runtime_apply_request_digest"]
        )
        assert deployment["generation"] == generation

        stale = _wait_for_stale_snapshot(
            binary,
            config_path,
            state_root,
            environment,
            initial,
        )
        _assert_stale_projection(initial, stale)
        stable_result = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert stable_result.envelope is not None
        assert stable_result.envelope["snapshot"] == stale, (
            "repeated stale Latest grew the immutable revision or changed its projection"
        )
        assert _matching_processes(binary) == ready_processes

        down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert down["ok"] is True and down["state"] == "stopped"
        _wait_for_no_matching_processes(binary)
        after_down = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=1,
        )
        assert _diagnostic_code(after_down) == "PXLC-INSPECTION-NOT-RUNNING"
        assert after_down.envelope is not None
        assert after_down.envelope["snapshot"] is None, (
            "down returned a cached Inspection success"
        )
        _assert_secret_material_absent_from_files(
            created,
            exact_binary=binary,
            public_input_files=(config_path,),
        )
    finally:
        try:
            _invoke_lifecycle(binary, "down", config_path, environment)
        except (AssertionError, FileNotFoundError, subprocess.SubprocessError):
            pass
        _terminate_private_processes(binary)
        shutil.rmtree(created)


def test_generation_fencing_single_query_races_output_and_secret_are_bounded() -> None:
    assert sys.platform.startswith("linux"), "M3a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m3-fence-"))
    created.chmod(0o700)
    binary = created / "paraegox"
    _copy_exact_binary(source_binary, binary)
    state_root = created / "state"
    config_path = created / "paraegox.toml"
    document = _chat_document(state_root, _reserve_loopback_port())
    _write_config(config_path, document)
    environment = _environment(created)
    provisioned_state = created / "provisioned-state"
    provisioned_config = created / "provisioned.toml"
    provisioned_document = _provisioned_chat_document(
        provisioned_state,
        _reserve_loopback_port(),
    )
    _write_config(provisioned_config, provisioned_document)
    startup_state = created / "startup-race-state"
    startup_config = created / "startup-race.toml"
    _write_config(startup_config, _chat_document(startup_state, _reserve_loopback_port()))
    interposer = _compile_inspection_interposer(created)
    try:
        up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert up["ok"] is True and up["state"] == "running"
        generation = up["generation"]
        assert isinstance(generation, str)
        ready_processes, _ = _assert_single_owner_graph(binary)
        before_queries = _tree_fingerprint(state_root)

        valid_locator = _raw_locator_query(state_root, document, generation)
        _decode_locator_response(
            valid_locator,
            expected_generation=generation,
            expected_commitment=_managed_chat_config_commitment(document),
        )
        mismatched_generation = bytearray.fromhex(generation)
        mismatched_generation[0] ^= 1
        mismatched = _raw_locator_query(
            state_root,
            document,
            mismatched_generation.hex(),
        )
        assert mismatched == b"", "locator answered a mismatched expected generation"
        assert _tree_fingerprint(state_root) == before_queries
        assert _matching_processes(binary) == ready_processes

        audit_process, audit_fd, audit_release = _spawn_interposed_inspection(
            binary,
            config_path,
            environment,
            interposer,
            mode="audit",
        )
        os.close(audit_release)
        audit_success = _finish_inspection(
            audit_process,
            config_path,
            state_root,
            expected_returncode=0,
        )
        assert audit_success.envelope is not None
        audit_markers = _read_available_audit(audit_fd)
        os.close(audit_fd)
        assert audit_markers == b"LQ", (
            "one snapshot must perform one locator and one authenticated Latest"
        )

        locator_process, locator_fd, locator_release = _spawn_interposed_inspection(
            binary,
            config_path,
            environment,
            interposer,
            mode="fail_locator",
        )
        os.close(locator_release)
        locator_failure = _finish_inspection(
            locator_process,
            config_path,
            state_root,
            expected_returncode=1,
        )
        locator_markers = _read_available_audit(locator_fd)
        os.close(locator_fd)
        assert _diagnostic_code(locator_failure) == "PXLC-INSPECTION-LOCATOR"
        assert locator_markers == b"L", "locator failure reconnected or retried"

        latest_process, latest_fd, latest_release = _spawn_interposed_inspection(
            binary,
            config_path,
            environment,
            interposer,
            mode="fail_latest",
        )
        os.close(latest_release)
        latest_failure = _finish_inspection(
            latest_process,
            config_path,
            state_root,
            expected_returncode=1,
        )
        latest_markers = _read_available_audit(latest_fd)
        os.close(latest_fd)
        assert _diagnostic_code(latest_failure) == "PXLC-INSPECTION-IO"
        assert latest_markers == b"LQ", "Latest failure reconnected or retried"
        assert _matching_processes(binary) == ready_processes

        replacement_process, replacement_fd, replacement_release = (
            _spawn_interposed_inspection(
                binary,
                config_path,
                environment,
                interposer,
                mode="barrier_bootstrap_open",
            )
        )
        replacement_observed = _wait_for_audit_marker(replacement_fd, b"O")
        assert replacement_observed == b"LO"
        replacement_client = frozenset({replacement_process.pid})
        assert replacement_process.poll() is None
        old_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert old_down["ok"] is True and old_down["state"] == "stopped"
        _wait_for_exact_matching_processes(binary, replacement_client)
        replacement_up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert replacement_up["ok"] is True and replacement_up["state"] == "running"
        replacement_generation = replacement_up["generation"]
        assert isinstance(replacement_generation, str)
        assert replacement_generation != generation
        replacement_processes, _ = _assert_single_owner_graph(
            binary,
            ignored_process_ids=replacement_client,
        )
        os.write(replacement_release, b"1")
        os.close(replacement_release)
        replacement_failure = _finish_inspection(
            replacement_process,
            config_path,
            state_root,
            expected_returncode=1,
        )
        replacement_markers = _read_available_audit(
            replacement_fd,
            replacement_observed,
        )
        os.close(replacement_fd)
        assert _diagnostic_code(replacement_failure) == "PXLC-INSPECTION-BOOTSTRAP"
        assert replacement_markers == b"LO", (
            "old locator accepted a successor-generation PXIB or attempted Latest"
        )
        assert _matching_processes(binary) == replacement_processes, (
            "completed old-generation client remained in the successor owner graph"
        )
        successor_snapshot = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert successor_snapshot.envelope is not None
        assert _matching_processes(binary) == replacement_processes

        race_process, race_fd, race_release = _spawn_interposed_inspection(
            binary,
            config_path,
            environment,
            interposer,
            mode="barrier_latest",
        )
        race_observed = _wait_for_audit_marker(race_fd, b"Q")
        assert race_observed == b"LQ"
        race_client = frozenset({race_process.pid})
        assert race_process.poll() is None
        raced_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert raced_down["ok"] is True and raced_down["state"] == "stopped"
        _wait_for_exact_matching_processes(binary, race_client)
        os.write(race_release, b"1")
        os.close(race_release)
        race_failure = _finish_inspection(
            race_process,
            config_path,
            state_root,
            expected_returncode=1,
        )
        race_markers = _read_available_audit(race_fd, race_observed)
        os.close(race_fd)
        assert _diagnostic_code(race_failure) == "PXLC-INSPECTION-IO"
        assert race_markers == b"LQ", "down-raced Latest retried or returned cache"
        _wait_for_no_matching_processes(binary)
        after_race = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=1,
        )
        assert _diagnostic_code(after_race) == "PXLC-INSPECTION-NOT-RUNNING"

        output_up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert output_up["ok"] is True and output_up["state"] == "running"
        output_processes, _ = _assert_single_owner_graph(binary)
        output_fingerprint = _tree_fingerprint(state_root)
        output_fd = os.open("/dev/full", os.O_WRONLY)
        try:
            output_process = _spawn_inspection(
                binary,
                config_path,
                environment,
                stdout=output_fd,
            )
        finally:
            os.close(output_fd)
        output_failure = _finish_inspection(
            output_process,
            config_path,
            state_root,
            expected_returncode=1,
        )
        assert output_failure.stdout is None
        assert output_failure.envelope is None
        assert _tree_fingerprint(state_root) == output_fingerprint
        assert _matching_processes(binary) == output_processes

        original_document = document
        drift_document = _chat_document(state_root, _reserve_loopback_port())
        assert drift_document != original_document
        _write_config(config_path, drift_document)
        drift = _invoke_inspection(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=2,
        )
        assert _diagnostic_code(drift) == "PXLC-LIFECYCLE-CONFIGURATION"
        assert _matching_processes(binary) == output_processes
        _write_config(config_path, original_document)
        output_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert output_down["ok"] is True and output_down["state"] == "stopped"
        _wait_for_no_matching_processes(binary)

        provisioned_up = _invoke_lifecycle(
            binary,
            "up",
            provisioned_config,
            environment,
        )
        assert provisioned_up["ok"] is True and provisioned_up["state"] == "running"
        provisioned_processes, _ = _assert_single_owner_graph(binary)
        no_secret_environment = environment.copy()
        no_secret_environment.pop("DEEPSEEK_API_KEY")
        provisioned_before = _tree_fingerprint(provisioned_state)
        provisioned_snapshot = _invoke_inspection(
            binary,
            provisioned_config,
            provisioned_state,
            no_secret_environment,
            expected_returncode=0,
        )
        assert provisioned_snapshot.envelope is not None
        assert _tree_fingerprint(provisioned_state) == provisioned_before
        assert _matching_processes(binary) == provisioned_processes
        provisioned_down = _invoke_lifecycle(
            binary,
            "down",
            provisioned_config,
            no_secret_environment,
        )
        assert provisioned_down["ok"] is True and provisioned_down["state"] == "stopped"
        _wait_for_no_matching_processes(binary)
        _assert_secret_material_absent_from_files(
            created,
            exact_binary=binary,
            public_input_files=(config_path, provisioned_config, startup_config),
        )

        startup_process, startup_fd, startup_release = _spawn_interposed_lifecycle_up(
            binary,
            startup_config,
            environment,
            interposer,
        )
        startup_observed = _wait_for_audit_marker(startup_fd, b"O")
        assert startup_observed == b"O"
        down_process = subprocess.Popen(
            [
                os.fspath(binary),
                "down",
                "--config",
                os.fspath(startup_config),
                "--json",
            ],
            cwd=binary.parent,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        _wait_for_lifecycle_state(startup_state, "stopping")
        os.write(startup_release, b"1")
        os.close(startup_release)
        down_race_result = _finish_lifecycle_process(
            down_process,
            allowed_returncodes={0},
        )
        assert down_race_result["state"] == "stopped"
        startup_result = _finish_lifecycle_process(
            startup_process,
            allowed_returncodes={0, 1},
        )
        assert startup_result["state"] in {"stopped", "failed", "unknown"}
        startup_markers = _read_available_audit(startup_fd, startup_observed)
        os.close(startup_fd)
        assert startup_markers == b"O", (
            "startup/down race reopened or retried the PXIB publication"
        )
        _wait_for_no_matching_processes(binary)
        startup_inspection = _invoke_inspection(
            binary,
            startup_config,
            startup_state,
            environment,
            expected_returncode=1,
        )
        assert _diagnostic_code(startup_inspection) == "PXLC-INSPECTION-NOT-RUNNING"
    finally:
        for candidate_config in (config_path, provisioned_config, startup_config):
            try:
                _invoke_lifecycle(binary, "down", candidate_config, environment)
            except (AssertionError, FileNotFoundError, subprocess.SubprocessError):
                pass
        _terminate_private_processes(binary)
        shutil.rmtree(created)
