from __future__ import annotations

import contextlib
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
import threading
import time
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

_BINARY_ENVIRONMENT = "PARAEGOX_M4A_RECEIPT_CLI_BINARY"
_COMMAND_TIMEOUT_SECONDS = 180.0
_CLEANUP_TIMEOUT_SECONDS = 30.0
_WIRE_TIMEOUT_SECONDS = 10.0
_HIDDEN_SUPERVISOR_MODE = b"__local-chat-supervisor-v1"
_CONFIG_COMMITMENT_DOMAIN = b"paraegox.local.managed-chat-config.sha256.v1"
_LOCATOR_RESPONSE_DIGEST_DOMAIN = b"paraegox.local.receipt-locator-response.v1"
_BOOTSTRAP_DIGEST_DOMAIN = b"paraegox.local.receipt-bootstrap.v1"
_REQUEST_ID_DOMAIN = b"paraegox.local.receipt-request-id.v1"
_REQUEST_DIGEST_DOMAIN = b"paraegox.local.receipt-latest-request.v1"
_RESPONSE_DIGEST_DOMAIN = b"paraegox.local.receipt-latest-response.v1"
_CONTROL_SOCKET_RELATIVE = Path("operator-v1/control-v1.sock")
_LIFECYCLE_RECORD_RELATIVE = Path("operator-v1/lifecycle-v1.json")
_LOCATOR_REQUEST_PREFIX = b"PXLO\x01R"
_LOCATOR_HEADER_BYTES = 160
_MAX_LOCATOR_PATH_BYTES = 4096
_MAX_LOCATOR_FRAME_BYTES = _LOCATOR_HEADER_BYTES + _MAX_LOCATOR_PATH_BYTES
_BOOTSTRAP_HEADER_BYTES = 320
_MIN_BOOTSTRAP_BYTES = 321
_MAX_BOOTSTRAP_BYTES = 832
_REQUEST_FRAME_BYTES = 176
_REQUEST_TRANSPORT_BYTES = 208
_RESPONSE_HEADER_BYTES = 224
_MAX_RESPONSE_FRAME_BYTES = 2272
_MAX_RESPONSE_TRANSPORT_BYTES = 2276
_OPERATION_TIMEOUT_NANOS = 5_000_000_000
_MAX_IN_FLIGHT = 8
_PXMT_MAGIC_OFFSET = 0
_PXMT_RUNTIME_TARGET_OFFSET = 6
_PXMT_RUNTIME_STORE_OFFSET = 22
_PXMT_REQUEST_DIGEST_OFFSET = 86
_PXMT_RESPONSE_KEY_REF_OFFSET = 505
_TOP_LEVEL_FIELDS = (
    "schema_version",
    "command",
    "ok",
    "changed",
    "generation",
    "snapshot",
    "diagnostics",
)
_SNAPSHOT_FIELDS = (
    "snapshot_version",
    "query_scope",
    "source_owner",
    "record_kind",
    "receipt_version",
    "request_digest",
    "receipt_digest",
    "request_mode",
    "terminal_outcome",
    "lifecycle_effect",
    "desired_head",
    "desired_head_digest",
    "fabric_generation",
    "model_generation",
    "agent_generation",
    "physical_binding_census",
    "census_complete",
    "fabric_ready",
    "model_ready",
    "agent_ready",
    "fabric_to_agent_dependency_ready",
    "model_to_agent_dependency_ready",
    "exact_zero",
    "quarantined",
    "resource_census_digest",
    "raw_outcome_digest",
    "completion_runtime_host_epoch",
    "completion_snapshot_sequence",
    "selection_clock_generation",
    "selection_observed_at_nanos",
    "current_health_checked",
)
_GENERATION_PATTERN = re.compile(r"[0-9a-f]{32}")
_DIGEST_PATTERN = re.compile(r"[0-9a-f]{64}")
_CANONICAL_DECIMAL_PATTERN = re.compile(r"0|[1-9][0-9]*")
_OPENAI_SENTINEL = "m4a-openai-secret-value-must-not-leak"
_DEEPSEEK_SENTINEL = "m4a-deepseek-secret-value-must-not-leak"
_EXTRA_ARGUMENT_CANARY = "m4a-extra-argument-must-not-leak"
_LOW_LEVEL_CANARY = "m4a-low-level-error-must-not-leak"
_RECEIPT_DIAGNOSTIC_MESSAGES = {
    "PXLC-RECEIPT-GRAMMAR": (
        "receipt snapshot requires exactly --config "
        "<absolute-paraegox.toml> --json"
    ),
    "PXLC-RECEIPT-NOT-RUNNING": (
        "local Receipt snapshot requires the current owner generation to be running"
    ),
    "PXLC-RECEIPT-LOCATOR": "local Receipt owner locator query failed closed",
    "PXLC-RECEIPT-BOOTSTRAP": "local Receipt bootstrap failed strict validation",
    "PXLC-RECEIPT-PEER": "local Receipt endpoint identity failed strict validation",
    "PXLC-RECEIPT-PROTOCOL": (
        "local Receipt response failed strict protocol validation"
    ),
    "PXLC-RECEIPT-NOT-FOUND": (
        "local Receipt is retiring and unavailable for this generation"
    ),
    "PXLC-RECEIPT-IO": "local Receipt one-shot exchange failed closed",
    "PXLC-RECEIPT-JSON-OUTPUT": "local Receipt machine-readable output failed",
}


@dataclass(frozen=True)
class ReceiptResult:
    returncode: int
    envelope: dict[str, Any] | None
    stdout: bytes | None
    stderr: bytes


@dataclass(frozen=True)
class ReceiptLocator:
    generation: bytes
    config_commitment: bytes
    bootstrap_path: Path
    bootstrap_length: int
    bootstrap_sha256: bytes
    bootstrap_device: int
    bootstrap_inode: int


@dataclass(frozen=True)
class ReceiptBootstrap:
    generation: bytes
    config_commitment: bytes
    token: bytes
    server_uid: int
    server_gid: int
    request_id_seed: bytes
    runtime_target: bytes
    runtime_store_instance: bytes
    runtime_response_key_ref: bytes
    runtime_response_public_key: bytes
    expected_request_digest: bytes
    expected_receipt_digest: bytes
    socket_path: Path


@dataclass(frozen=True)
class ReceiptResponse:
    outcome: str
    request_id: bytes
    generation: bytes
    config_commitment: bytes
    expected_request_digest: bytes
    expected_receipt_digest: bytes
    payload: bytes
    transport: bytes


@dataclass
class FakePeer:
    path: Path
    requests: list[bytes]
    errors: list[BaseException]
    thread: threading.Thread


def _bootstrap_private_material(bootstrap: ReceiptBootstrap) -> tuple[bytes, ...]:
    return (
        os.fsencode(bootstrap.socket_path),
        bootstrap.token,
        bootstrap.request_id_seed,
        bootstrap.runtime_target,
        bootstrap.runtime_store_instance,
        bootstrap.runtime_response_key_ref,
        bootstrap.runtime_response_public_key,
    )


def _require_exact_binary() -> Path:
    configured = os.environ.get(_BINARY_ENVIRONMENT)
    assert configured is not None, (
        f"{_BINARY_ENVIRONMENT} must name the already-built binary from the exact "
        "source revision under validation"
    )
    path = Path(configured)
    assert path.is_absolute(), f"{_BINARY_ENVIRONMENT} must be absolute"
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode), "the exact M4a binary must be a regular file"
    assert not path.is_symlink(), "the exact M4a binary must not be a symlink"
    assert metadata.st_mode & 0o111 != 0, "the exact M4a binary must be executable"
    return path.resolve(strict=True)


def _sha256_file(path: Path) -> bytes:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(128 * 1024):
            digest.update(chunk)
    return digest.digest()


def _copy_exact_binary(source: Path, target: Path) -> None:
    expected_digest = _sha256_file(source)
    shutil.copyfile(source, target)
    target.chmod(0o755)
    metadata = target.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not target.is_symlink()
    assert metadata.st_nlink == 1
    assert metadata.st_mode & 0o7777 == 0o755
    assert _sha256_file(target) == expected_digest


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
        "PARAEGOX_M4A_LOW_LEVEL_CANARY": _LOW_LEVEL_CANARY,
    }


def _decode_one_compact_json_object(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n"), "Receipt stdout must end in one LF"
    assert raw.count(b"\n") == 1, "Receipt stdout must contain exactly one JSON line"
    value = json.loads(raw)
    assert isinstance(value, dict), "Receipt stdout must be one JSON object"
    compact = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    assert raw == compact, "Receipt stdout must use compact JSON framing"
    return value


def _assert_diagnostics(value: object, *, expected_count: int) -> None:
    assert isinstance(value, list)
    assert len(value) == expected_count
    for diagnostic in value:
        assert isinstance(diagnostic, dict)
        assert list(diagnostic) == ["code", "message"]
        assert isinstance(diagnostic["code"], str) and diagnostic["code"]
        assert isinstance(diagnostic["message"], str) and diagnostic["message"]
        if diagnostic["code"].startswith("PXLC-RECEIPT-"):
            assert _RECEIPT_DIAGNOSTIC_MESSAGES[diagnostic["code"]] == diagnostic[
                "message"
            ]


def _assert_hex(value: object, pattern: re.Pattern[str]) -> None:
    assert isinstance(value, str)
    assert pattern.fullmatch(value) is not None
    assert any(character != "0" for character in value)


def _assert_u64_string(value: object) -> None:
    assert isinstance(value, str)
    assert _CANONICAL_DECIMAL_PATTERN.fullmatch(value) is not None
    assert 0 <= int(value) <= (1 << 64) - 1


def _assert_snapshot(snapshot: object) -> dict[str, Any]:
    assert isinstance(snapshot, dict)
    assert list(snapshot) == list(_SNAPSHOT_FIELDS)
    assert type(snapshot["snapshot_version"]) is int
    assert snapshot["snapshot_version"] == 1
    assert snapshot["query_scope"] == "current_running_generation"
    assert snapshot["source_owner"] == "runtime_host"
    assert snapshot["record_kind"] == "managed_model_agent_stack_terminal_receipt"
    assert type(snapshot["receipt_version"]) is int
    assert snapshot["receipt_version"] == 1
    for field in (
        "request_digest",
        "receipt_digest",
        "desired_head_digest",
        "resource_census_digest",
        "raw_outcome_digest",
    ):
        _assert_hex(snapshot[field], _DIGEST_PATTERN)
    assert snapshot["request_mode"] == "fabric_model_and_agent"
    assert snapshot["terminal_outcome"] == "active_ready"
    assert snapshot["lifecycle_effect"] == "may_have_started"
    assert snapshot["desired_head"] == "committed_incoming"
    for field in (
        "fabric_generation",
        "model_generation",
        "agent_generation",
        "completion_runtime_host_epoch",
        "completion_snapshot_sequence",
        "selection_clock_generation",
        "selection_observed_at_nanos",
    ):
        _assert_u64_string(snapshot[field])
    assert type(snapshot["physical_binding_census"]) is int
    assert snapshot["physical_binding_census"] == 2
    for field in (
        "census_complete",
        "fabric_ready",
        "model_ready",
        "agent_ready",
        "fabric_to_agent_dependency_ready",
        "model_to_agent_dependency_ready",
    ):
        assert snapshot[field] is True
    assert snapshot["exact_zero"] is False
    assert snapshot["quarantined"] is False
    assert snapshot["current_health_checked"] is False
    return snapshot


def _assert_receipt_envelope(envelope: dict[str, Any], *, ok: bool) -> None:
    assert list(envelope) == list(_TOP_LEVEL_FIELDS)
    assert type(envelope["schema_version"]) is int
    assert envelope["schema_version"] == 1
    assert envelope["command"] == "receipt.snapshot"
    assert type(envelope["ok"]) is bool and envelope["ok"] is ok
    assert envelope["changed"] is False
    if ok:
        _assert_hex(envelope["generation"], _GENERATION_PATTERN)
        _assert_snapshot(envelope["snapshot"])
        _assert_diagnostics(envelope["diagnostics"], expected_count=0)
    else:
        assert envelope["generation"] is None
        assert envelope["snapshot"] is None
        _assert_diagnostics(envelope["diagnostics"], expected_count=1)


def _assert_public_output_is_redacted(
    result: ReceiptResult,
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
        _LOW_LEVEL_CANARY.encode(),
        *additional_forbidden,
    ):
        assert forbidden not in combined
    sensitive_term = (
        rb"(?i)\b(?:pid|pgid|uid|gid|secretref|credential|seed|private[-_ ]key|"
        rb"capability|token|state_root|config_path)\b"
    )
    assert re.search(sensitive_term, combined) is None


def _spawn_receipt(
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
            "receipt",
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


def _finish_receipt(
    process: subprocess.Popen[bytes],
    config_path: Path | str,
    state_root: Path,
    *,
    expected_returncode: int,
    expected_code: str | None = None,
    additional_forbidden: tuple[bytes, ...] = (),
) -> ReceiptResult:
    try:
        stdout, stderr = process.communicate(timeout=_COMMAND_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        stdout, stderr = process.communicate()
        raise AssertionError(
            f"receipt exceeded {_COMMAND_TIMEOUT_SECONDS}s; "
            f"stdout={stdout!r}; stderr={stderr!r}"
        ) from error
    assert process.returncode == expected_returncode, (
        f"receipt exited {process.returncode}; stdout={stdout!r}; stderr={stderr!r}"
    )
    assert stderr == b"", f"exact Receipt grammar must keep stderr empty: {stderr!r}"
    envelope = None if stdout is None else _decode_one_compact_json_object(stdout)
    if envelope is not None:
        _assert_receipt_envelope(envelope, ok=expected_returncode == 0)
        if expected_code is not None:
            assert _diagnostic_code_from_envelope(envelope) == expected_code
    result = ReceiptResult(process.returncode, envelope, stdout, stderr)
    _assert_public_output_is_redacted(
        result,
        config_path=config_path,
        state_root=state_root,
        additional_forbidden=additional_forbidden,
    )
    return result


def _invoke_receipt(
    binary: Path,
    config_path: Path | str,
    state_root: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int,
    expected_code: str | None = None,
    extra_arguments: tuple[str, ...] = (),
    command_prefix: tuple[str, ...] = (),
    additional_forbidden: tuple[bytes, ...] = (),
) -> ReceiptResult:
    return _finish_receipt(
        _spawn_receipt(
            binary,
            config_path,
            environment,
            extra_arguments=extra_arguments,
            command_prefix=command_prefix,
        ),
        config_path,
        state_root,
        expected_returncode=expected_returncode,
        expected_code=expected_code,
        additional_forbidden=additional_forbidden,
    )


def _invoke_receipt_arguments(
    binary: Path,
    arguments: list[str],
    config_path: Path | str,
    state_root: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int,
    expected_code: str,
) -> ReceiptResult:
    process = subprocess.Popen(
        [os.fspath(binary), *arguments],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return _finish_receipt(
        process,
        config_path,
        state_root,
        expected_returncode=expected_returncode,
        expected_code=expected_code,
    )


def _diagnostic_code_from_envelope(envelope: dict[str, Any]) -> str:
    diagnostics = envelope["diagnostics"]
    assert isinstance(diagnostics, list) and len(diagnostics) == 1
    code = diagnostics[0]["code"]
    assert isinstance(code, str)
    return code


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
    return envelope


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
    assert ignored_process_ids <= all_processes
    processes = all_processes - ignored_process_ids
    assert len(processes) >= 2, "Receipt success lacks the supervisor and real Node child"
    supervisors = {
        process_id
        for process_id in processes
        if _HIDDEN_SUPERVISOR_MODE in _process_command_line(process_id)
    }
    assert len(supervisors) == 1, "one M4a generation must have one supervisor"
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
    assert expected_process_ids
    deadline = time.monotonic() + _CLEANUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if _matching_processes(binary) == expected_process_ids:
            return
        time.sleep(0.05)
    assert _matching_processes(binary) == expected_process_ids


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


def _socket_paths(root: Path) -> list[Path]:
    if not root.exists():
        return []
    sockets: list[Path] = []
    for path in root.rglob("*"):
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISSOCK(metadata.st_mode):
            sockets.append(path)
    return sockets


def _local_runtime_directories() -> frozenset[Path]:
    directories: set[Path] = set()
    for path in Path("/tmp").glob("pxl-*"):
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISDIR(metadata.st_mode):
            directories.add(path)
    return frozenset(directories)


def _assert_inert_failed_runtime_directory(root: Path) -> None:
    metadata = root.lstat()
    assert stat.S_ISDIR(metadata.st_mode)
    entries = sorted(root.rglob("*"), key=lambda item: os.fsencode(item))
    assert [entry.relative_to(root) for entry in entries] == [Path("node")]
    node_metadata = entries[0].lstat()
    assert stat.S_ISDIR(node_metadata.st_mode)
    assert _socket_paths(root) == []
    assert not any(
        stat.S_ISREG(entry.lstat().st_mode) or entry.suffix == ".pxrb"
        for entry in entries
    )


def _tree_fingerprint(root: Path) -> tuple[tuple[object, ...], ...]:
    fingerprint: list[tuple[object, ...]] = []
    for path in sorted(root.rglob("*"), key=lambda item: os.fsencode(item)):
        metadata = path.lstat()
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


def _read_to_eof(client: socket.socket, maximum: int) -> bytes:
    response = bytearray()
    while True:
        try:
            chunk = client.recv(4096)
        except ConnectionResetError:
            break
        if not chunk:
            break
        response.extend(chunk)
        assert len(response) <= maximum
    return bytes(response)


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
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(_WIRE_TIMEOUT_SECONDS)
        client.connect(os.fspath(state_root / _CONTROL_SOCKET_RELATIVE))
        client.sendall(request)
        client.shutdown(socket.SHUT_WR)
        return _read_to_eof(client, _MAX_LOCATOR_FRAME_BYTES + 1)


def _decode_locator_response(
    raw: bytes,
    *,
    expected_generation: str,
    expected_commitment: bytes,
) -> ReceiptLocator:
    assert _LOCATOR_HEADER_BYTES < len(raw) <= _MAX_LOCATOR_FRAME_BYTES
    assert raw[:4] == b"PXRL"
    assert raw[4:6] == (1).to_bytes(2, "big")
    assert raw[6:8] == b"RR"
    assert raw[8:10] == _LOCATOR_HEADER_BYTES.to_bytes(2, "big")
    assert raw[10:12] == bytes(2)
    assert int.from_bytes(raw[12:16], "big") == len(raw)
    path_length = int.from_bytes(raw[16:20], "big")
    content_length = int.from_bytes(raw[20:24], "big")
    assert 1 <= path_length <= _MAX_LOCATOR_PATH_BYTES
    assert _LOCATOR_HEADER_BYTES + path_length == len(raw)
    assert _MIN_BOOTSTRAP_BYTES <= content_length <= _MAX_BOOTSTRAP_BYTES
    generation = raw[24:40]
    commitment = raw[40:72]
    assert generation == bytes.fromhex(expected_generation)
    assert commitment == expected_commitment
    bootstrap_sha256 = raw[72:104]
    bootstrap_device = int.from_bytes(raw[104:112], "big")
    bootstrap_inode = int.from_bytes(raw[112:120], "big")
    assert any(bootstrap_sha256)
    assert bootstrap_device > 0 and bootstrap_inode > 0
    assert raw[120:128] == bytes(8)
    path_bytes = raw[_LOCATOR_HEADER_BYTES:]
    assert raw[128:160] == hashlib.sha256(
        _LOCATOR_RESPONSE_DIGEST_DOMAIN + raw[:128] + path_bytes
    ).digest()
    path_text = path_bytes.decode("utf-8")
    path = Path(path_text)
    assert path.is_absolute()
    assert os.path.normpath(path_text) == path_text
    assert os.fsencode(path) == path_bytes
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_uid == os.geteuid() and metadata.st_gid == os.getegid()
    assert stat.S_IMODE(metadata.st_mode) == 0o600
    assert metadata.st_nlink == 1
    assert metadata.st_dev == bootstrap_device
    assert metadata.st_ino == bootstrap_inode
    assert metadata.st_size == content_length
    assert _sha256_file(path) == bootstrap_sha256
    return ReceiptLocator(
        generation,
        commitment,
        path,
        content_length,
        bootstrap_sha256,
        bootstrap_device,
        bootstrap_inode,
    )


def _decode_bootstrap(locator: ReceiptLocator) -> ReceiptBootstrap:
    raw = locator.bootstrap_path.read_bytes()
    assert len(raw) == locator.bootstrap_length
    assert _sha256_file(locator.bootstrap_path) == locator.bootstrap_sha256
    assert _MIN_BOOTSTRAP_BYTES <= len(raw) <= _MAX_BOOTSTRAP_BYTES
    assert raw[:4] == b"PXRB"
    assert raw[4:6] == (1).to_bytes(2, "big")
    assert int.from_bytes(raw[6:8], "big") == _BOOTSTRAP_HEADER_BYTES
    assert int.from_bytes(raw[8:12], "big") == len(raw)
    path_length = int.from_bytes(raw[12:16], "big")
    assert 1 <= path_length <= 512
    assert _BOOTSTRAP_HEADER_BYTES + path_length == len(raw)
    assert raw[16:32] == locator.generation
    assert raw[32:64] == locator.config_commitment
    assert int.from_bytes(raw[104:112], "big") == _OPERATION_TIMEOUT_NANOS
    socket_bytes = raw[_BOOTSTRAP_HEADER_BYTES:]
    assert raw[288:320] == hashlib.sha256(
        _BOOTSTRAP_DIGEST_DOMAIN + raw[:288] + socket_bytes
    ).digest()
    socket_text = socket_bytes.decode("utf-8")
    socket_path = Path(socket_text)
    assert socket_path.is_absolute()
    assert os.path.normpath(socket_text) == socket_text
    assert os.fsencode(socket_path) == socket_bytes
    server_uid = int.from_bytes(raw[96:100], "big")
    server_gid = int.from_bytes(raw[100:104], "big")
    assert server_uid == os.geteuid() and server_gid == os.getegid()
    for value in (
        raw[64:96],
        raw[112:128],
        raw[128:144],
        raw[144:176],
        raw[176:192],
        raw[192:224],
        raw[224:256],
        raw[256:288],
    ):
        assert any(value)
    socket_metadata = socket_path.lstat()
    assert stat.S_ISSOCK(socket_metadata.st_mode)
    assert socket_metadata.st_uid == server_uid
    assert socket_metadata.st_gid == server_gid
    assert stat.S_IMODE(socket_metadata.st_mode) == 0o600
    return ReceiptBootstrap(
        generation=raw[16:32],
        config_commitment=raw[32:64],
        token=raw[64:96],
        server_uid=server_uid,
        server_gid=server_gid,
        request_id_seed=raw[112:128],
        runtime_target=raw[128:144],
        runtime_store_instance=raw[144:176],
        runtime_response_key_ref=raw[176:192],
        runtime_response_public_key=raw[192:224],
        expected_request_digest=raw[224:256],
        expected_receipt_digest=raw[256:288],
        socket_path=socket_path,
    )


def _request_id(bootstrap: ReceiptBootstrap) -> bytes:
    value = hashlib.sha256(
        _REQUEST_ID_DOMAIN + bootstrap.request_id_seed + (1).to_bytes(8, "big")
    ).digest()[:16]
    assert any(value)
    return value


def _build_request_transport(bootstrap: ReceiptBootstrap) -> bytes:
    frame = bytearray(_REQUEST_FRAME_BYTES)
    frame[:4] = b"PXRQ"
    frame[4:6] = (1).to_bytes(2, "big")
    frame[6] = ord("L")
    frame[8:10] = _REQUEST_FRAME_BYTES.to_bytes(2, "big")
    frame[12:16] = _REQUEST_FRAME_BYTES.to_bytes(4, "big")
    frame[16:32] = _request_id(bootstrap)
    frame[32:48] = bootstrap.generation
    frame[48:80] = bootstrap.config_commitment
    frame[80:112] = bootstrap.expected_request_digest
    frame[112:144] = bootstrap.expected_receipt_digest
    frame[144:176] = hashlib.sha256(_REQUEST_DIGEST_DOMAIN + frame[:144]).digest()
    transport = bootstrap.token + bytes(frame)
    assert len(transport) == _REQUEST_TRANSPORT_BYTES
    return transport


def _raw_exchange(
    socket_path: Path,
    transport: bytes,
    *,
    timeout_seconds: float = _WIRE_TIMEOUT_SECONDS,
    maximum: int = _MAX_RESPONSE_TRANSPORT_BYTES + 1,
) -> bytes:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(timeout_seconds)
        client.connect(os.fspath(socket_path))
        client.sendall(transport)
        client.shutdown(socket.SHUT_WR)
        return _read_to_eof(client, maximum)


def _decode_response_transport(
    raw: bytes,
    *,
    expected_request: bytes,
) -> ReceiptResponse:
    assert 4 + _RESPONSE_HEADER_BYTES <= len(raw) <= _MAX_RESPONSE_TRANSPORT_BYTES
    frame_length = int.from_bytes(raw[:4], "big")
    assert frame_length == len(raw) - 4
    frame = raw[4:]
    assert frame[:4] == b"PXRO"
    assert frame[4:6] == (1).to_bytes(2, "big")
    assert frame[6] == ord("L")
    outcome = chr(frame[7])
    assert outcome in {"R", "N"}
    assert int.from_bytes(frame[8:10], "big") == _RESPONSE_HEADER_BYTES
    assert frame[10:12] == bytes(2)
    assert int.from_bytes(frame[12:16], "big") == frame_length
    payload_length = int.from_bytes(frame[16:20], "big")
    assert frame[20:24] == bytes(4)
    assert frame_length == _RESPONSE_HEADER_BYTES + payload_length
    request_frame = expected_request[32:]
    assert frame[24:40] == request_frame[16:32]
    assert frame[40:56] == request_frame[32:48]
    assert frame[56:88] == request_frame[48:80]
    assert frame[88:120] == request_frame[80:112]
    assert frame[120:152] == request_frame[112:144]
    assert frame[184:192] == bytes(8)
    payload = frame[_RESPONSE_HEADER_BYTES:]
    if outcome == "R":
        assert 1 <= payload_length <= 2048
        assert frame[152:184] == hashlib.sha256(payload).digest()
    else:
        assert payload_length == 0 and payload == b""
        assert frame[152:184] == bytes(32)
    assert frame[192:224] == hashlib.sha256(
        _RESPONSE_DIGEST_DOMAIN + frame[:192] + payload
    ).digest()
    return ReceiptResponse(
        outcome=outcome,
        request_id=frame[24:40],
        generation=frame[40:56],
        config_commitment=frame[56:88],
        expected_request_digest=frame[88:120],
        expected_receipt_digest=frame[120:152],
        payload=payload,
        transport=raw,
    )


def _not_found_transport(request_transport: bytes) -> bytes:
    request = request_transport[32:]
    assert len(request) == _REQUEST_FRAME_BYTES
    frame = bytearray(_RESPONSE_HEADER_BYTES)
    frame[:4] = b"PXRO"
    frame[4:6] = (1).to_bytes(2, "big")
    frame[6:8] = b"LN"
    frame[8:10] = _RESPONSE_HEADER_BYTES.to_bytes(2, "big")
    frame[12:16] = _RESPONSE_HEADER_BYTES.to_bytes(4, "big")
    frame[24:40] = request[16:32]
    frame[40:56] = request[32:48]
    frame[56:88] = request[48:80]
    frame[88:120] = request[80:112]
    frame[120:152] = request[112:144]
    frame[192:224] = hashlib.sha256(_RESPONSE_DIGEST_DOMAIN + frame[:192]).digest()
    return _RESPONSE_HEADER_BYTES.to_bytes(4, "big") + bytes(frame)


def _mutate_request(
    transport: bytes,
    *,
    offset: int,
    recompute_digest: bool = True,
) -> bytes:
    mutated = bytearray(transport)
    mutated[32 + offset] ^= 1
    if recompute_digest:
        frame = mutated[32:]
        frame[144:176] = hashlib.sha256(_REQUEST_DIGEST_DOMAIN + frame[:144]).digest()
        mutated[32:] = frame
    return bytes(mutated)


def _mutate_ready_payload(transport: bytes, *, payload_offset: int = -1) -> bytes:
    frame = bytearray(transport[4:])
    assert frame[7] == ord("R")
    assert len(frame) > _RESPONSE_HEADER_BYTES
    payload = frame[_RESPONSE_HEADER_BYTES:]
    assert -len(payload) <= payload_offset < len(payload)
    resolved_offset = (
        payload_offset if payload_offset >= 0 else len(payload) + payload_offset
    )
    frame[_RESPONSE_HEADER_BYTES + resolved_offset] ^= 1
    payload = frame[_RESPONSE_HEADER_BYTES:]
    frame[152:184] = hashlib.sha256(payload).digest()
    frame[192:224] = hashlib.sha256(
        _RESPONSE_DIGEST_DOMAIN + frame[:192] + payload
    ).digest()
    return len(frame).to_bytes(4, "big") + bytes(frame)


_RECEIPT_INTERPOSER_SOURCE = r"""
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/un.h>
#include <unistd.h>

static const unsigned char status_prefix[6] = {'P', 'X', 'L', 'O', 1, 'S'};
static const unsigned char locator_prefix[6] = {'P', 'X', 'L', 'O', 1, 'R'};
static __thread int inside_interposer = 0;
static int bootstrap_open_observed = 0;
static int bootstrap_close_observed = 0;
static int tracked_bootstrap_fd = -1;

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
    int audit_fd = configured_fd("PARAEGOX_M4A_AUDIT_FD");
    if (audit_fd >= 0) {
        (void)syscall(SYS_write, audit_fd, &marker, 1);
    }
}

static int await_release(void) {
    int release_fd = configured_fd("PARAEGOX_M4A_RELEASE_FD");
    char release = 0;
    if (release_fd < 0 || syscall(SYS_read, release_fd, &release, 1) != 1) {
        errno = EIO;
        return -1;
    }
    return 0;
}

static int is_status_request(const void *buffer, size_t count) {
    return count >= sizeof(status_prefix)
        && memcmp(buffer, status_prefix, sizeof(status_prefix)) == 0;
}

static int is_locator_request(const void *buffer, size_t count) {
    return count >= sizeof(locator_prefix)
        && memcmp(buffer, locator_prefix, sizeof(locator_prefix)) == 0;
}

static int is_latest_request(const void *buffer, size_t count) {
    if (count < 39) {
        return 0;
    }
    const unsigned char *bytes = buffer;
    return memcmp(bytes + 32, "PXRQ", 4) == 0
        && bytes[36] == 0
        && bytes[37] == 1
        && bytes[38] == 'L';
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
    const char *mode = getenv("PARAEGOX_M4A_INTERPOSE_MODE");
    if (mode != NULL && strcmp(mode, "barrier_startup_bootstrap_open") == 0) {
        return 0;
    }
    int status = is_status_request(buffer, count);
    int locator = is_locator_request(buffer, count);
    int latest = is_latest_request(buffer, count);
    if (!status && !locator && !latest) {
        return 0;
    }
    inside_interposer = 1;
    report_marker(status ? 'S' : (locator ? 'L' : 'Q'));
    int failure = 0;
    if (mode != NULL && status && strcmp(mode, "fail_status") == 0) {
        errno = EIO;
        failure = -1;
    } else if (mode != NULL && locator && strcmp(mode, "fail_locator") == 0) {
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
    unsigned char observed[64];
    size_t copied = copy_iov_prefix(observed, sizeof(observed), iov, iovcnt);
    return before_request(observed, copied);
}

static int after_open(int fd, int flags) {
    const char *mode = getenv("PARAEGOX_M4A_INTERPOSE_MODE");
    if (
        fd < 0
        || inside_interposer
        || mode == NULL
        || (
            strcmp(mode, "barrier_bootstrap_open") != 0
            && strcmp(mode, "barrier_startup_bootstrap_open") != 0
            && strcmp(mode, "barrier_bootstrap_close") != 0
        )
        || (flags & O_CREAT) != 0
        || (flags & O_ACCMODE) != O_RDONLY
        || bootstrap_open_observed
    ) {
        return fd;
    }
    unsigned char magic[4] = {0, 0, 0, 0};
    long observed = syscall(SYS_pread64, fd, magic, sizeof(magic), 0);
    if (observed != 4 || memcmp(magic, "PXRB", 4) != 0) {
        return fd;
    }
    inside_interposer = 1;
    bootstrap_open_observed = 1;
    if (strcmp(mode, "barrier_bootstrap_close") == 0) {
        tracked_bootstrap_fd = fd;
        inside_interposer = 0;
        return fd;
    }
    report_marker('O');
    if (await_release() != 0) {
        int saved = errno;
        (void)syscall(SYS_close, fd);
        errno = saved;
        fd = -1;
    }
    inside_interposer = 0;
    return fd;
}

int close(int fd) {
    static int (*real_close)(int) = NULL;
    if (real_close == NULL) {
        real_close = dlsym(RTLD_NEXT, "close");
    }
    if (
        fd != tracked_bootstrap_fd
        || bootstrap_close_observed
        || inside_interposer
    ) {
        return real_close(fd);
    }
    tracked_bootstrap_fd = -1;
    bootstrap_close_observed = 1;
    int result = real_close(fd);
    int close_errno = errno;
    inside_interposer = 1;
    report_marker('B');
    int release_result = await_release();
    int release_errno = errno;
    inside_interposer = 0;
    if (release_result != 0) {
        errno = release_errno;
        return -1;
    }
    errno = close_errno;
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

int connect(int fd, const struct sockaddr *address, socklen_t address_length) {
    static int (*real_connect)(int, const struct sockaddr *, socklen_t) = NULL;
    if (real_connect == NULL) {
        real_connect = dlsym(RTLD_NEXT, "connect");
    }
    if (
        address != NULL
        && address->sa_family == AF_UNIX
        && !inside_interposer
        && bootstrap_close_observed
    ) {
        const char *mode = getenv("PARAEGOX_M4A_INTERPOSE_MODE");
        if (mode != NULL && strcmp(mode, "barrier_bootstrap_close") == 0) {
            inside_interposer = 1;
            report_marker('C');
            inside_interposer = 0;
        }
    }
    const char *from = getenv("PARAEGOX_M4A_REDIRECT_FROM");
    const char *to = getenv("PARAEGOX_M4A_REDIRECT_TO");
    if (
        address == NULL
        || address->sa_family != AF_UNIX
        || from == NULL
        || to == NULL
    ) {
        return real_connect(fd, address, address_length);
    }
    const struct sockaddr_un *original = (const struct sockaddr_un *)address;
    if (strcmp(original->sun_path, from) != 0) {
        return real_connect(fd, address, address_length);
    }
    size_t length = strlen(to);
    if (length == 0 || length >= sizeof(original->sun_path)) {
        errno = ENAMETOOLONG;
        return -1;
    }
    struct sockaddr_un redirected;
    memset(&redirected, 0, sizeof(redirected));
    redirected.sun_family = AF_UNIX;
    memcpy(redirected.sun_path, to, length + 1);
    socklen_t redirected_length = (socklen_t)(
        offsetof(struct sockaddr_un, sun_path) + length + 1
    );
    return real_connect(fd, (const struct sockaddr *)&redirected, redirected_length);
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
        return after_open(real_open(path, flags, creation_mode), flags);
    }
    return after_open(real_open(path, flags), flags);
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
        return after_open(real_open64(path, flags, creation_mode), flags);
    }
    return after_open(real_open64(path, flags), flags);
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
        return after_open(real_openat(directory_fd, path, flags, creation_mode), flags);
    }
    return after_open(real_openat(directory_fd, path, flags), flags);
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
        return after_open(
            real_openat64(directory_fd, path, flags, creation_mode), flags
        );
    }
    return after_open(real_openat64(directory_fd, path, flags), flags);
}
"""


def _compile_receipt_interposer(root: Path) -> Path:
    source = root / "receipt-interposer.c"
    library = root / "receipt-interposer.so"
    source.write_text(_RECEIPT_INTERPOSER_SOURCE, encoding="utf-8")
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
        f"Receipt interposer compilation failed: {completed.stderr!r}"
    )
    return library


def _spawn_interposed_receipt(
    binary: Path,
    config_path: Path,
    environment: dict[str, str],
    library: Path,
    *,
    mode: str,
    redirect_from: Path | None = None,
    redirect_to: Path | None = None,
) -> tuple[subprocess.Popen[bytes], int, int]:
    audit_read, audit_write = os.pipe()
    release_read, release_write = os.pipe()
    injected = environment.copy()
    injected.update(
        {
            "LD_PRELOAD": os.fspath(library),
            "PARAEGOX_M4A_INTERPOSE_MODE": mode,
            "PARAEGOX_M4A_AUDIT_FD": str(audit_write),
            "PARAEGOX_M4A_RELEASE_FD": str(release_read),
        }
    )
    if redirect_from is not None or redirect_to is not None:
        assert redirect_from is not None and redirect_to is not None
        injected["PARAEGOX_M4A_REDIRECT_FROM"] = os.fspath(redirect_from)
        injected["PARAEGOX_M4A_REDIRECT_TO"] = os.fspath(redirect_to)
    try:
        process = _spawn_receipt(
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
            "PARAEGOX_M4A_INTERPOSE_MODE": "barrier_startup_bootstrap_open",
            "PARAEGOX_M4A_AUDIT_FD": str(audit_write),
            "PARAEGOX_M4A_RELEASE_FD": str(release_read),
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
            f"lifecycle race timed out; stdout={stdout!r}; stderr={stderr!r}"
        ) from error
    assert process.returncode in allowed_returncodes, (
        f"lifecycle race exited {process.returncode}; stdout={stdout!r}; "
        f"stderr={stderr!r}"
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
        f"Receipt interposer did not report {expected!r}: {observed!r}"
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


@contextlib.contextmanager
def _fake_peer(
    root: Path,
    responder: Callable[[socket.socket, bytes], None],
) -> Iterator[FakePeer]:
    path = root / f"fake-receipt-{time.monotonic_ns():x}.sock"
    requests: list[bytes] = []
    errors: list[BaseException] = []
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(os.fspath(path))
    path.chmod(0o600)
    listener.listen(1)
    listener.settimeout(30.0)

    def serve() -> None:
        try:
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(15.0)
                request = _read_to_eof(connection, _REQUEST_TRANSPORT_BYTES + 1)
                requests.append(request)
                responder(connection, request)
        except BaseException as error:  # surfaced on the harness thread below
            errors.append(error)
        finally:
            listener.close()

    thread = threading.Thread(target=serve, name="m4a-fake-peer", daemon=True)
    thread.start()
    peer = FakePeer(path, requests, errors, thread)
    try:
        yield peer
    finally:
        thread.join(timeout=30.0)
        assert not thread.is_alive(), "fake Receipt peer did not terminate"
        assert not errors, f"fake Receipt peer failed: {errors!r}"
        with contextlib.suppress(FileNotFoundError):
            path.unlink()


def _write_response(connection: socket.socket, response: bytes) -> None:
    connection.sendall(response)
    with contextlib.suppress(OSError):
        connection.shutdown(socket.SHUT_WR)


def _require_passwordless_sudo() -> str:
    sudo = shutil.which("sudo")
    assert sudo is not None, "the admitted Ubuntu M4a harness requires passwordless sudo"
    preflight = subprocess.run(
        [sudo, "-n", "true"],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=10.0,
        check=False,
    )
    assert preflight.returncode == 0, (
        "the admitted Ubuntu M4a harness requires passwordless sudo"
    )
    return sudo


def _socket_fd_count(process_id: int) -> int:
    count = 0
    directory = Path("/proc") / str(process_id) / "fd"
    for entry in directory.iterdir():
        try:
            target = os.readlink(entry)
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        if target.startswith("socket:["):
            count += 1
    return count


def _wait_for_socket_fd_count(process_id: int, minimum: int) -> None:
    deadline = time.monotonic() + 10.0
    while time.monotonic() < deadline:
        if _socket_fd_count(process_id) >= minimum:
            return
        time.sleep(0.02)
    assert _socket_fd_count(process_id) >= minimum, (
        "Receipt endpoint did not accept the expected bounded exchanges"
    )


def _wait_for_socket_fd_count_at_most(process_id: int, maximum: int) -> None:
    deadline = time.monotonic() + 10.0
    while time.monotonic() < deadline:
        if _socket_fd_count(process_id) <= maximum:
            return
        time.sleep(0.02)
    assert _socket_fd_count(process_id) <= maximum, (
        "Receipt endpoint did not release completed exchanges"
    )


def _silent_raw_exchange(
    socket_path: Path,
    transport: bytes,
    *,
    timeout_seconds: float = _WIRE_TIMEOUT_SECONDS,
) -> bytes:
    try:
        return _raw_exchange(
            socket_path,
            transport,
            timeout_seconds=timeout_seconds,
        )
    except (BrokenPipeError, ConnectionResetError):
        return b""


def _run_redirected_peer(
    binary: Path,
    config_path: Path,
    state_root: Path,
    environment: dict[str, str],
    interposer: Path,
    bootstrap: ReceiptBootstrap,
    responder: Callable[[socket.socket, bytes], None],
    *,
    expected_returncode: int,
    expected_code: str | None = None,
) -> tuple[ReceiptResult, bytes, bytes]:
    with _fake_peer(binary.parent, responder) as peer:
        process, audit_fd, release_fd = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="audit",
            redirect_from=bootstrap.socket_path,
            redirect_to=peer.path,
        )
        os.close(release_fd)
        result = _finish_receipt(
            process,
            config_path,
            state_root,
            expected_returncode=expected_returncode,
            expected_code=expected_code,
            additional_forbidden=(
                os.fsencode(peer.path),
                *_bootstrap_private_material(bootstrap),
            ),
        )
        markers = _read_available_audit(audit_fd)
        os.close(audit_fd)
    assert len(peer.requests) == 1, "fake peer did not receive exactly one exchange"
    return result, markers, peer.requests[0]


def _spawn_root_peer(
    sudo: str,
    path: Path,
    owner_uid: int,
    owner_gid: int,
) -> subprocess.Popen[bytes]:
    process = subprocess.Popen(
        [
            sudo,
            "-n",
            sys.executable,
            os.fspath(Path(__file__).resolve()),
            "--root-peer",
            os.fspath(path),
            str(owner_uid),
            str(owner_gid),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert process.stdout is not None
    readable, _, _ = select.select([process.stdout], [], [], 10.0)
    assert readable, "root fake peer did not become ready"
    assert process.stdout.readline() == b"READY\n"
    return process


def test_receipt_never_started_grammar_config_and_root_are_pre_effect() -> None:
    assert sys.platform.startswith("linux"), "M4a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m4-pre-"))
    created.chmod(0o700)
    binary = created / "paraegox"
    _copy_exact_binary(source_binary, binary)
    state_root = created / "state"
    config_path = created / "paraegox.toml"
    document = _chat_document(state_root, _reserve_loopback_port())
    _write_config(config_path, document)
    environment = _environment(created)
    try:
        help_result = subprocess.run(
            [os.fspath(binary), "--help"],
            cwd=binary.parent,
            env=environment,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=30.0,
            check=False,
        )
        assert help_result.returncode == 0
        assert help_result.stderr == b""
        help_lines = help_result.stdout.decode("utf-8").splitlines()
        expected_help = (
            "       paraegox receipt snapshot --config "
            "<absolute-paraegox.toml> --json"
        )
        assert help_lines.count(expected_help) == 1
        assert sum(
            line.startswith(("Usage: paraegox", "       paraegox"))
            for line in help_lines
        ) == 15

        never_started = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-NOT-RUNNING",
        )
        assert never_started.envelope is not None

        missing_config = _invoke_receipt_arguments(
            binary,
            ["receipt", "snapshot", "--json"],
            config_path,
            state_root,
            environment,
            expected_returncode=2,
            expected_code="PXLC-RECEIPT-GRAMMAR",
        )
        assert missing_config.envelope is not None

        relative = _invoke_receipt(
            binary,
            "paraegox.toml",
            state_root,
            environment,
            expected_returncode=2,
            expected_code="PXLC-CONFIG-PATH-INVALID",
        )
        assert relative.envelope is not None

        extra = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=2,
            expected_code="PXLC-RECEIPT-GRAMMAR",
            extra_arguments=(f"--{_EXTRA_ARGUMENT_CANARY}",),
        )
        assert extra.envelope is not None

        malformed_path = created / "malformed.toml"
        _write_config(malformed_path, "schema_version = 1\ndo-not-echo-this\n")
        malformed = _invoke_receipt(
            binary,
            malformed_path,
            state_root,
            environment,
            expected_returncode=2,
            expected_code="PXLC-CONFIG-DOCUMENT-INVALID",
            additional_forbidden=(b"do-not-echo-this",),
        )
        assert malformed.envelope is not None

        symlink_path = created / "config-symlink-canary.toml"
        symlink_path.symlink_to(config_path)
        symlink = _invoke_receipt(
            binary,
            symlink_path,
            state_root,
            environment,
            expected_returncode=2,
            expected_code="PXLC-CONFIG-PATH-INVALID",
        )
        assert symlink.envelope is not None
        assert symlink_path.is_symlink()

        sudo = _require_passwordless_sudo()
        root_environment = environment.copy()
        root_environment["PATH"] = os.environ.get("PATH", "/usr/bin:/bin")
        root_rejected = _invoke_receipt(
            binary,
            config_path,
            state_root,
            root_environment,
            expected_returncode=1,
            expected_code="PXLC-EXECUTION-IDENTITY",
            command_prefix=(sudo, "-n", "--"),
        )
        assert root_rejected.envelope is not None

        assert not state_root.exists(), (
            "never-started Receipt rejection created lifecycle or domain state"
        )
        assert _matching_processes(binary) == set(), (
            "never-started Receipt rejection started an owner"
        )
    finally:
        _terminate_private_processes(binary)
        shutil.rmtree(created)


def test_running_receipt_matches_real_pxmt_and_is_read_only_secret_free() -> None:
    assert sys.platform.startswith("linux"), "M4a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m4-run-"))
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
    try:
        up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert up["ok"] is True and up["state"] == "running"
        assert up["changed"] is True and up["owner_readiness_observed"] is True
        generation = up["generation"]
        assert isinstance(generation, str)
        assert _GENERATION_PATTERN.fullmatch(generation) is not None
        ready_processes, _ = _assert_single_owner_graph(binary)

        inaccessible_evidence_decoy = created / "paraegox-evidence"
        inaccessible_evidence_decoy.mkdir(mode=0o000)
        before = _tree_fingerprint(state_root)
        first = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert first.envelope is not None
        assert first.envelope["generation"] == generation
        snapshot = _assert_snapshot(first.envelope["snapshot"])
        assert _tree_fingerprint(state_root) == before
        assert _matching_processes(binary) == ready_processes

        deployment = _invoke_deploy_projection(binary, config_path, environment)
        assert deployment["generation"] == generation
        assert snapshot["request_digest"] == deployment["runtime_apply_request_digest"]
        assert snapshot["receipt_digest"] == deployment["runtime_terminal_receipt_digest"]

        locator = _decode_locator_response(
            _raw_locator_query(state_root, document, generation),
            expected_generation=generation,
            expected_commitment=_managed_chat_config_commitment(document),
        )
        bootstrap = _decode_bootstrap(locator)
        request = _build_request_transport(bootstrap)
        raw_ready = _raw_exchange(bootstrap.socket_path, request)
        response = _decode_response_transport(raw_ready, expected_request=request)
        assert response.outcome == "R"
        assert response.generation.hex() == generation
        assert response.expected_request_digest.hex() == snapshot["request_digest"]
        assert response.expected_receipt_digest.hex() == snapshot["receipt_digest"]
        assert len(response.payload) <= 2048
        assert first.stdout is not None
        for private_material in (
            os.fsencode(locator.bootstrap_path),
            *_bootstrap_private_material(bootstrap),
        ):
            assert private_material not in first.stdout

        repeated = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert repeated.envelope == first.envelope
        assert _tree_fingerprint(state_root) == before
        assert _matching_processes(binary) == ready_processes
        inaccessible_evidence_decoy.chmod(0o700)
        inaccessible_evidence_decoy.rmdir()

        down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert down["ok"] is True and down["state"] == "stopped"
        _wait_for_no_matching_processes(binary)
        after_down = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-NOT-RUNNING",
        )
        assert after_down.envelope is not None
        assert after_down.envelope["snapshot"] is None

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
        provisioned_receipt = _invoke_receipt(
            binary,
            provisioned_config,
            provisioned_state,
            no_secret_environment,
            expected_returncode=0,
        )
        assert provisioned_receipt.envelope is not None
        assert _tree_fingerprint(provisioned_state) == provisioned_before
        assert _matching_processes(binary) == provisioned_processes
        provisioned_down = _invoke_lifecycle(
            binary,
            "down",
            provisioned_config,
            no_secret_environment,
        )
        assert provisioned_down["ok"] is True
        _wait_for_no_matching_processes(binary)
        _assert_secret_material_absent_from_files(
            created,
            exact_binary=binary,
            public_input_files=(config_path, provisioned_config),
        )
    finally:
        with contextlib.suppress(OSError):
            inaccessible_evidence_decoy = created / "paraegox-evidence"
            inaccessible_evidence_decoy.chmod(0o700)
            inaccessible_evidence_decoy.rmdir()
        for candidate in (config_path, provisioned_config):
            with contextlib.suppress(
                AssertionError,
                FileNotFoundError,
                subprocess.SubprocessError,
            ):
                _invoke_lifecycle(binary, "down", candidate, environment)
        _terminate_private_processes(binary)
        shutil.rmtree(created)


def test_receipt_single_status_locator_latest_and_generation_races_fail_closed() -> None:
    assert sys.platform.startswith("linux"), "M4a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m4-fence-"))
    created.chmod(0o700)
    binary = created / "paraegox"
    _copy_exact_binary(source_binary, binary)
    state_root = created / "state"
    config_path = created / "paraegox.toml"
    document = _chat_document(state_root, _reserve_loopback_port())
    _write_config(config_path, document)
    environment = _environment(created)
    startup_state = created / "startup-state"
    startup_config = created / "startup.toml"
    startup_document = _chat_document(startup_state, _reserve_loopback_port())
    _write_config(startup_config, startup_document)
    failed_state = created / "occupied-port-state"
    failed_config = created / "occupied-port.toml"
    interposer = _compile_receipt_interposer(created)
    replacement_socket_fd: int | None = None
    try:
        up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert up["ok"] is True and up["state"] == "running"
        generation = up["generation"]
        assert isinstance(generation, str)
        ready_processes, _ = _assert_single_owner_graph(binary)
        before = _tree_fingerprint(state_root)

        audit_process, audit_fd, audit_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="audit",
        )
        os.close(audit_release)
        audit = _finish_receipt(
            audit_process,
            config_path,
            state_root,
            expected_returncode=0,
        )
        assert audit.envelope is not None
        audit_markers = _read_available_audit(audit_fd)
        os.close(audit_fd)
        assert audit_markers == b"SLQ", (
            "one Receipt snapshot must perform one Status, locator, and Latest"
        )

        status_process, status_fd, status_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="fail_status",
        )
        os.close(status_release)
        status_failure = _finish_receipt(
            status_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-LOCATOR",
        )
        status_markers = _read_available_audit(status_fd)
        os.close(status_fd)
        assert status_failure.envelope is not None
        assert status_markers == b"S", "Status failure continued or retried"

        locator_process, locator_fd, locator_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="fail_locator",
        )
        os.close(locator_release)
        locator_failure = _finish_receipt(
            locator_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-LOCATOR",
        )
        locator_markers = _read_available_audit(locator_fd)
        os.close(locator_fd)
        assert locator_failure.envelope is not None
        assert locator_markers == b"SL", "locator failure continued or retried"

        latest_process, latest_fd, latest_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="fail_latest",
        )
        os.close(latest_release)
        latest_failure = _finish_receipt(
            latest_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-IO",
        )
        latest_markers = _read_available_audit(latest_fd)
        os.close(latest_fd)
        assert latest_failure.envelope is not None
        assert latest_markers == b"SLQ", "Latest failure reconnected or retried"
        assert _tree_fingerprint(state_root) == before
        assert _matching_processes(binary) == ready_processes

        replacement_locator = _decode_locator_response(
            _raw_locator_query(state_root, document, generation),
            expected_generation=generation,
            expected_commitment=_managed_chat_config_commitment(document),
        )
        replacement_bootstrap = _decode_bootstrap(replacement_locator)
        replacement_socket_fd = os.open(
            replacement_bootstrap.socket_path,
            os.O_PATH | os.O_NOFOLLOW | os.O_CLOEXEC,
        )
        old_socket_metadata = os.fstat(replacement_socket_fd)
        assert stat.S_ISSOCK(old_socket_metadata.st_mode)
        old_socket_path_metadata = replacement_bootstrap.socket_path.lstat()
        assert (
            old_socket_metadata.st_dev,
            old_socket_metadata.st_ino,
        ) == (
            old_socket_path_metadata.st_dev,
            old_socket_path_metadata.st_ino,
        )
        replacement_process, replacement_fd, replacement_release = (
            _spawn_interposed_receipt(
                binary,
                config_path,
                environment,
                interposer,
                mode="barrier_bootstrap_close",
            )
        )
        replacement_observed = _wait_for_audit_marker(replacement_fd, b"B")
        assert replacement_observed == b"SLB"
        replacement_client = frozenset({replacement_process.pid})
        old_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert old_down["ok"] is True and old_down["state"] == "stopped"
        _wait_for_exact_matching_processes(binary, replacement_client)
        replacement_up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert replacement_up["ok"] is True and replacement_up["state"] == "running"
        assert replacement_up["generation"] != generation
        replacement_processes, _ = _assert_single_owner_graph(
            binary,
            ignored_process_ids=replacement_client,
        )
        successor_locator = _decode_locator_response(
            _raw_locator_query(
                state_root,
                document,
                replacement_up["generation"],
            ),
            expected_generation=replacement_up["generation"],
            expected_commitment=_managed_chat_config_commitment(document),
        )
        successor_bootstrap = _decode_bootstrap(successor_locator)
        assert successor_bootstrap.socket_path == replacement_bootstrap.socket_path
        successor_socket_metadata = successor_bootstrap.socket_path.lstat()
        assert stat.S_ISSOCK(successor_socket_metadata.st_mode)
        assert (
            old_socket_metadata.st_dev,
            old_socket_metadata.st_ino,
        ) != (
            successor_socket_metadata.st_dev,
            successor_socket_metadata.st_ino,
        )
        os.write(replacement_release, b"1")
        os.close(replacement_release)
        replacement_failure = _finish_receipt(
            replacement_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-BOOTSTRAP",
        )
        replacement_markers = _read_available_audit(
            replacement_fd,
            replacement_observed,
        )
        os.close(replacement_fd)
        assert replacement_failure.envelope is not None
        assert replacement_markers == b"SLBC", (
            "bootstrap-pinned old query attempted Latest against the successor socket"
        )
        os.close(replacement_socket_fd)
        replacement_socket_fd = None
        assert _matching_processes(binary) == replacement_processes
        successor = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert successor.envelope is not None
        assert successor.envelope["generation"] == replacement_up["generation"]

        race_process, race_fd, race_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="barrier_latest",
        )
        race_observed = _wait_for_audit_marker(race_fd, b"Q")
        assert race_observed == b"SLQ"
        race_client = frozenset({race_process.pid})
        raced_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert raced_down["ok"] is True and raced_down["state"] == "stopped"
        _wait_for_exact_matching_processes(binary, race_client)
        os.write(race_release, b"1")
        os.close(race_release)
        race_failure = _finish_receipt(
            race_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-IO",
        )
        race_markers = _read_available_audit(race_fd, race_observed)
        os.close(race_fd)
        assert race_failure.envelope is not None
        assert race_markers == b"SLQ"
        _wait_for_no_matching_processes(binary)

        output_up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert output_up["ok"] is True and output_up["state"] == "running"
        output_processes, _ = _assert_single_owner_graph(binary)
        output_before = _tree_fingerprint(state_root)
        output_fd = os.open("/dev/full", os.O_WRONLY)
        try:
            output_process = _spawn_receipt(
                binary,
                config_path,
                environment,
                stdout=output_fd,
            )
        finally:
            os.close(output_fd)
        output_failure = _finish_receipt(
            output_process,
            config_path,
            state_root,
            expected_returncode=1,
        )
        assert output_failure.stdout is None and output_failure.envelope is None
        assert _tree_fingerprint(state_root) == output_before
        assert _matching_processes(binary) == output_processes

        drift_document = _chat_document(state_root, _reserve_loopback_port())
        _write_config(config_path, drift_document)
        drift = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=2,
            expected_code="PXLC-LIFECYCLE-CONFIGURATION",
        )
        assert drift.envelope is not None
        assert _matching_processes(binary) == output_processes
        _write_config(config_path, document)
        output_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert output_down["ok"] is True
        _wait_for_no_matching_processes(binary)

        startup_process, startup_fd, startup_release = _spawn_interposed_lifecycle_up(
            binary,
            startup_config,
            environment,
            interposer,
        )
        startup_observed = _wait_for_audit_marker(startup_fd, b"O")
        assert startup_observed == b"O"
        _wait_for_lifecycle_state(startup_state, "starting")
        startup_receipt_process, startup_receipt_fd, startup_receipt_release = (
            _spawn_interposed_receipt(
                binary,
                startup_config,
                environment,
                interposer,
                mode="audit",
            )
        )
        os.close(startup_receipt_release)
        startup_receipt = _finish_receipt(
            startup_receipt_process,
            startup_config,
            startup_state,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-NOT-RUNNING",
        )
        startup_receipt_markers = _read_available_audit(startup_receipt_fd)
        os.close(startup_receipt_fd)
        assert startup_receipt.envelope is not None
        assert startup_receipt.envelope["snapshot"] is None
        assert startup_receipt_markers == b"S", (
            "a starting generation advanced beyond its single Status observation"
        )
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
        down_result = _finish_lifecycle_process(down_process, allowed_returncodes={0})
        assert down_result["state"] == "stopped"
        startup_result = _finish_lifecycle_process(
            startup_process,
            allowed_returncodes={0, 1},
        )
        assert startup_result["state"] in {"stopped", "failed", "unknown"}
        startup_markers = _read_available_audit(startup_fd, startup_observed)
        os.close(startup_fd)
        assert startup_markers == b"O"
        _wait_for_no_matching_processes(binary)
        startup_receipt = _invoke_receipt(
            binary,
            startup_config,
            startup_state,
            environment,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-NOT-RUNNING",
        )
        assert startup_receipt.envelope is not None

        local_runtime_before = _local_runtime_directories()
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied_listener:
            occupied_listener.bind(("127.0.0.1", 0))
            occupied_listener.listen(1)
            occupied_port = int(occupied_listener.getsockname()[1])
            failed_document = _chat_document(failed_state, occupied_port)
            _write_config(failed_config, failed_document)
            failed_up = _invoke_lifecycle(
                binary,
                "up",
                failed_config,
                environment,
                expected_returncode=1,
            )
            assert failed_up["ok"] is False
            assert failed_up["state"] == "unknown"
            _assert_hex(failed_up["generation"], _GENERATION_PATTERN)
            assert failed_up["changed"] is True
            assert failed_up["owner_readiness_observed"] is False
            _wait_for_no_matching_processes(binary)
            assert _socket_paths(failed_state) == []
            failed_runtime_directories = _local_runtime_directories()
            created_runtime_directories = (
                failed_runtime_directories - local_runtime_before
            )
            assert len(created_runtime_directories) == 1
            failed_runtime_directory = next(iter(created_runtime_directories))
            _assert_inert_failed_runtime_directory(failed_runtime_directory)
            failed_before = _tree_fingerprint(failed_state)
            failed_runtime_before = _tree_fingerprint(failed_runtime_directory)
            failed_receipt = _invoke_receipt(
                binary,
                failed_config,
                failed_state,
                environment,
                expected_returncode=1,
                expected_code="PXLC-RECEIPT-NOT-RUNNING",
            )
            assert failed_receipt.envelope is not None
            assert failed_receipt.envelope["snapshot"] is None
            assert _tree_fingerprint(failed_state) == failed_before
            assert _socket_paths(failed_state) == []
            assert _local_runtime_directories() == failed_runtime_directories
            assert (
                _tree_fingerprint(failed_runtime_directory)
                == failed_runtime_before
            )
            _assert_inert_failed_runtime_directory(failed_runtime_directory)
            assert _matching_processes(binary) == set()
    finally:
        if replacement_socket_fd is not None:
            os.close(replacement_socket_fd)
        for candidate in (config_path, startup_config, failed_config):
            with contextlib.suppress(
                AssertionError,
                FileNotFoundError,
                subprocess.SubprocessError,
            ):
                _invoke_lifecycle(binary, "down", candidate, environment)
        _terminate_private_processes(binary)
        shutil.rmtree(created)


def test_receipt_raw_auth_retirement_deadline_capacity_and_fault_peers() -> None:
    assert sys.platform.startswith("linux"), "M4a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()
    created = Path(tempfile.mkdtemp(prefix="px-m4-wire-"))
    created.chmod(0o700)
    binary = created / "paraegox"
    _copy_exact_binary(source_binary, binary)
    state_root = created / "state"
    config_path = created / "paraegox.toml"
    document = _chat_document(state_root, _reserve_loopback_port())
    _write_config(config_path, document)
    environment = _environment(created)
    interposer = _compile_receipt_interposer(created)
    root_peer_process: subprocess.Popen[bytes] | None = None
    holders: list[socket.socket] = []
    deadline_client: socket.socket | None = None
    retiring_client: socket.socket | None = None
    try:
        up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert up["ok"] is True and up["state"] == "running"
        generation = up["generation"]
        assert isinstance(generation, str)
        _, supervisor = _assert_single_owner_graph(binary)
        baseline_socket_count = _socket_fd_count(supervisor)

        locator = _decode_locator_response(
            _raw_locator_query(state_root, document, generation),
            expected_generation=generation,
            expected_commitment=_managed_chat_config_commitment(document),
        )
        bootstrap = _decode_bootstrap(locator)
        request = _build_request_transport(bootstrap)
        ready_transport = _raw_exchange(bootstrap.socket_path, request)
        ready_response = _decode_response_transport(
            ready_transport,
            expected_request=request,
        )
        assert ready_response.outcome == "R"
        _wait_for_socket_fd_count_at_most(supervisor, baseline_socket_count)

        wrong_token = bytearray(request)
        wrong_token[0] ^= 1
        rejected_transports = (
            bytes(wrong_token),
            _mutate_request(request, offset=0),
            _mutate_request(request, offset=4),
            _mutate_request(request, offset=6),
            _mutate_request(request, offset=7),
            _mutate_request(request, offset=16),
            _mutate_request(request, offset=32),
            _mutate_request(request, offset=48),
            _mutate_request(request, offset=80),
            _mutate_request(request, offset=112),
            _mutate_request(request, offset=144, recompute_digest=False),
            request + b"trailing-request-byte",
        )
        for rejected in rejected_transports:
            assert _silent_raw_exchange(
                bootstrap.socket_path,
                rejected,
                timeout_seconds=2.0,
            ) == b"", "unauthenticated or non-canonical PXRQ received a response"
        _wait_for_socket_fd_count_at_most(supervisor, baseline_socket_count)

        for _ in range(_MAX_IN_FLIGHT - 1):
            client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            client.settimeout(2.0)
            client.connect(os.fspath(bootstrap.socket_path))
            client.sendall(request[:40])
            holders.append(client)
        _wait_for_socket_fd_count(
            supervisor,
            baseline_socket_count + _MAX_IN_FLIGHT - 1,
        )
        admitted_at_capacity = _decode_response_transport(
            _raw_exchange(bootstrap.socket_path, request),
            expected_request=request,
        )
        assert admitted_at_capacity.outcome == "R"
        _wait_for_socket_fd_count_at_most(
            supervisor,
            baseline_socket_count + _MAX_IN_FLIGHT - 1,
        )

        last_holder = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        last_holder.settimeout(2.0)
        last_holder.connect(os.fspath(bootstrap.socket_path))
        last_holder.sendall(request[:40])
        holders.append(last_holder)
        _wait_for_socket_fd_count(
            supervisor,
            baseline_socket_count + _MAX_IN_FLIGHT,
        )
        overload_started = time.monotonic()
        assert _silent_raw_exchange(
            bootstrap.socket_path,
            request,
            timeout_seconds=2.0,
        ) == b""
        assert time.monotonic() - overload_started < 2.0, (
            "the ninth Receipt exchange was queued instead of silently rejected"
        )
        for client in holders:
            client.close()
        holders.clear()
        _wait_for_socket_fd_count_at_most(supervisor, baseline_socket_count)

        deadline_client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        deadline_client.settimeout(1.0)
        deadline_started = time.monotonic()
        deadline_client.connect(os.fspath(bootstrap.socket_path))
        deadline_client.sendall(request[:32])
        _wait_for_socket_fd_count(supervisor, baseline_socket_count + 1)
        next_byte = 32
        deadline_closed = False
        while time.monotonic() - deadline_started < 8.0:
            time.sleep(0.4)
            try:
                deadline_client.sendall(request[next_byte : next_byte + 1])
                next_byte += 1
            except (BrokenPipeError, ConnectionResetError):
                deadline_closed = True
                break
            readable, _, _ = select.select([deadline_client], [], [], 0.0)
            if readable:
                try:
                    deadline_closed = deadline_client.recv(1, socket.MSG_PEEK) == b""
                except ConnectionResetError:
                    deadline_closed = True
                if deadline_closed:
                    break
        deadline_elapsed = time.monotonic() - deadline_started
        assert deadline_closed, "progressing partial PXRQ escaped the absolute deadline"
        assert 4.0 <= deadline_elapsed <= 7.0
        deadline_client.close()
        deadline_client = None
        _wait_for_socket_fd_count_at_most(supervisor, baseline_socket_count)

        original_bootstrap = locator.bootstrap_path.read_bytes()
        tampered_bootstrap = bytearray(original_bootstrap)
        tampered_bootstrap[192] ^= 1
        try:
            locator.bootstrap_path.write_bytes(tampered_bootstrap)
            locator.bootstrap_path.chmod(0o600)
            public_key_tamper = _invoke_receipt(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=1,
                expected_code="PXLC-RECEIPT-BOOTSTRAP",
            )
            assert public_key_tamper.envelope is not None
        finally:
            locator.bootstrap_path.write_bytes(original_bootstrap)
            locator.bootstrap_path.chmod(0o600)
        restored_bootstrap = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert restored_bootstrap.envelope is not None

        public_ready, ready_markers, observed_ready_request = _run_redirected_peer(
            binary,
            config_path,
            state_root,
            environment,
            interposer,
            bootstrap,
            lambda connection, _: _write_response(connection, ready_transport),
            expected_returncode=0,
        )
        assert public_ready.envelope is not None
        assert public_ready.envelope["generation"] == generation
        assert ready_markers == b"SLQ"
        assert observed_ready_request == request

        response_cases: tuple[
            tuple[str, Callable[[socket.socket, bytes], None]], ...
        ] = (
            (
                "PXLC-RECEIPT-NOT-FOUND",
                lambda connection, observed: _write_response(
                    connection,
                    _not_found_transport(observed),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, observed: _write_response(
                    connection,
                    _not_found_transport(_mutate_request(observed, offset=16)),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    _mutate_ready_payload(
                        ready_transport,
                        payload_offset=_PXMT_MAGIC_OFFSET,
                    ),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    _mutate_ready_payload(
                        ready_transport,
                        payload_offset=_PXMT_RUNTIME_TARGET_OFFSET,
                    ),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    _mutate_ready_payload(
                        ready_transport,
                        payload_offset=_PXMT_RUNTIME_STORE_OFFSET,
                    ),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    _mutate_ready_payload(
                        ready_transport,
                        payload_offset=_PXMT_REQUEST_DIGEST_OFFSET,
                    ),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    _mutate_ready_payload(
                        ready_transport,
                        payload_offset=_PXMT_RESPONSE_KEY_REF_OFFSET,
                    ),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    _mutate_ready_payload(ready_transport),
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    ready_transport + b"trailing-response-byte",
                ),
            ),
            (
                "PXLC-RECEIPT-PROTOCOL",
                lambda connection, _: _write_response(
                    connection,
                    (_MAX_RESPONSE_FRAME_BYTES + 1).to_bytes(4, "big"),
                ),
            ),
            (
                "PXLC-RECEIPT-IO",
                lambda connection, _: _write_response(
                    connection,
                    ready_transport[: 4 + (_RESPONSE_HEADER_BYTES // 2)],
                ),
            ),
        )
        for expected_code, responder in response_cases:
            result, markers, observed_request = _run_redirected_peer(
                binary,
                config_path,
                state_root,
                environment,
                interposer,
                bootstrap,
                responder,
                expected_returncode=1,
                expected_code=expected_code,
            )
            assert result.envelope is not None
            assert markers == b"SLQ"
            assert observed_request == request

        def hold_response_open(_: socket.socket, observed: bytes) -> None:
            assert observed == request
            time.sleep(6.0)

        with _fake_peer(binary.parent, hold_response_open) as stalled_peer:
            stalled_process, stalled_fd, stalled_release = _spawn_interposed_receipt(
                binary,
                config_path,
                environment,
                interposer,
                mode="audit",
                redirect_from=bootstrap.socket_path,
                redirect_to=stalled_peer.path,
            )
            os.close(stalled_release)
            stalled_started = time.monotonic()
            stalled = _finish_receipt(
                stalled_process,
                config_path,
                state_root,
                expected_returncode=1,
                expected_code="PXLC-RECEIPT-IO",
                additional_forbidden=(
                    os.fsencode(stalled_peer.path),
                    *_bootstrap_private_material(bootstrap),
                ),
            )
            stalled_elapsed = time.monotonic() - stalled_started
            stalled_markers = _read_available_audit(stalled_fd)
            os.close(stalled_fd)
        assert stalled.envelope is not None
        assert stalled_markers == b"SLQ"
        assert stalled_peer.requests == [request]
        assert 4.0 <= stalled_elapsed <= 6.5

        sudo = _require_passwordless_sudo()
        root_peer_path = created / "root-peer.sock"
        root_peer_process = _spawn_root_peer(
            sudo,
            root_peer_path,
            os.geteuid(),
            os.getegid(),
        )
        peer_process, peer_fd, peer_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="audit",
            redirect_from=bootstrap.socket_path,
            redirect_to=root_peer_path,
        )
        os.close(peer_release)
        peer_failure = _finish_receipt(
            peer_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-PEER",
            additional_forbidden=(
                os.fsencode(root_peer_path),
                *_bootstrap_private_material(bootstrap),
            ),
        )
        peer_markers = _read_available_audit(peer_fd)
        os.close(peer_fd)
        root_stdout, root_stderr = root_peer_process.communicate(timeout=15.0)
        assert root_peer_process.returncode == 0
        assert root_stdout == b"" and root_stderr == b""
        root_peer_process = None
        assert peer_failure.envelope is not None
        assert peer_markers == b"SL", "peer rejection sent a PXRQ"

        retiring_client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        retiring_client.settimeout(10.0)
        retiring_client.connect(os.fspath(bootstrap.socket_path))
        retiring_client.sendall(request)
        _wait_for_socket_fd_count(supervisor, baseline_socket_count + 1)
        down_process = subprocess.Popen(
            [
                os.fspath(binary),
                "down",
                "--config",
                os.fspath(config_path),
                "--json",
            ],
            cwd=binary.parent,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        _wait_for_lifecycle_state(state_root, "stopping")
        stopping_process, stopping_fd, stopping_release = _spawn_interposed_receipt(
            binary,
            config_path,
            environment,
            interposer,
            mode="audit",
        )
        os.close(stopping_release)
        stopping_receipt = _finish_receipt(
            stopping_process,
            config_path,
            state_root,
            expected_returncode=1,
            expected_code="PXLC-RECEIPT-NOT-RUNNING",
        )
        stopping_markers = _read_available_audit(stopping_fd)
        os.close(stopping_fd)
        assert stopping_receipt.envelope is not None
        assert stopping_receipt.envelope["snapshot"] is None
        assert stopping_markers == b"S", (
            "a stopping generation advanced beyond its single Status observation"
        )
        retiring_client.shutdown(socket.SHUT_WR)
        retiring_transport = _read_to_eof(
            retiring_client,
            _MAX_RESPONSE_TRANSPORT_BYTES + 1,
        )
        retiring_response = _decode_response_transport(
            retiring_transport,
            expected_request=request,
        )
        assert retiring_response.outcome == "N"
        retiring_client.close()
        retiring_client = None
        down = _finish_lifecycle_process(down_process, allowed_returncodes={0})
        assert down["ok"] is True and down["state"] == "stopped"
        _wait_for_no_matching_processes(binary)

        replacement_up = _invoke_lifecycle(binary, "up", config_path, environment)
        assert replacement_up["ok"] is True
        assert replacement_up["generation"] != generation
        replacement = _invoke_receipt(
            binary,
            config_path,
            state_root,
            environment,
            expected_returncode=0,
        )
        assert replacement.envelope is not None
        assert replacement.envelope["generation"] == replacement_up["generation"]
        replacement_down = _invoke_lifecycle(binary, "down", config_path, environment)
        assert replacement_down["ok"] is True
        _wait_for_no_matching_processes(binary)
    finally:
        for client in holders:
            with contextlib.suppress(OSError):
                client.close()
        if deadline_client is not None:
            with contextlib.suppress(OSError):
                deadline_client.close()
        if retiring_client is not None:
            with contextlib.suppress(OSError):
                retiring_client.close()
        if root_peer_process is not None:
            with contextlib.suppress(ProcessLookupError):
                root_peer_process.terminate()
            with contextlib.suppress(subprocess.TimeoutExpired):
                root_peer_process.communicate(timeout=5.0)
            if root_peer_process.poll() is None:
                root_peer_process.kill()
                root_peer_process.communicate()
        with contextlib.suppress(
            AssertionError,
            FileNotFoundError,
            subprocess.SubprocessError,
        ):
            _invoke_lifecycle(binary, "down", config_path, environment)
        _terminate_private_processes(binary)
        shutil.rmtree(created)


def _root_peer_main(arguments: list[str]) -> int:
    if len(arguments) != 3:
        return 2
    path = Path(arguments[0])
    owner_uid = int(arguments[1])
    owner_gid = int(arguments[2])
    with contextlib.suppress(FileNotFoundError):
        path.unlink()
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        listener.bind(os.fspath(path))
        os.chown(path, owner_uid, owner_gid)
        path.chmod(0o600)
        listener.listen(1)
        listener.settimeout(15.0)
        sys.stdout.write("READY\n")
        sys.stdout.flush()
        connection, _ = listener.accept()
        with connection:
            connection.settimeout(15.0)
            request = _read_to_eof(connection, _REQUEST_TRANSPORT_BYTES + 1)
        return 0 if request == b"" else 3
    finally:
        listener.close()
        with contextlib.suppress(FileNotFoundError):
            path.unlink()


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "--root-peer":
        raise SystemExit(_root_peer_main(sys.argv[2:]))
    raise SystemExit(2)
