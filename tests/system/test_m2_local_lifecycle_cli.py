from __future__ import annotations

import hashlib
import json
import os
import re
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

_BINARY_ENVIRONMENT = "PARAEGOX_M2_CLI_BINARY"
_COMMAND_TIMEOUT_SECONDS = 150.0
_PROCESS_CLEANUP_TIMEOUT_SECONDS = 30.0
_HIDDEN_SUPERVISOR_MODE = b"__local-chat-supervisor-v1"
_MANAGED_CHAT_CONFIG_COMMITMENT_DOMAIN = b"paraegox.local.managed-chat-config.sha256.v1"
_LIFECYCLE_RECORD_RELATIVE = Path("operator-v1/lifecycle-v1.json")
_CONTROL_SOCKET_RELATIVE = Path("operator-v1/control-v1.sock")
_INTERNAL_STATUS_PREFIX = b"PXLO\x01S"
_LIFECYCLE_FIELDS = {
    "schema_version",
    "command",
    "ok",
    "state",
    "generation",
    "changed",
    "owner_readiness_observed",
    "inspection_checked",
    "diagnostics",
}
_LIFECYCLE_STATES = {
    "never_started",
    "starting",
    "running",
    "stopping",
    "stopped",
    "failed",
    "unknown",
}
_GENERATION_PATTERN = re.compile(r"[0-9a-f]+")
_OPENAI_SENTINEL = "m2-openai-secret-value-must-not-leak"
_DEEPSEEK_SENTINEL = "m2-deepseek-secret-value-must-not-leak"


@dataclass(frozen=True)
class LifecycleResult:
    returncode: int
    envelope: dict[str, Any]
    stdout: bytes
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
    assert stat.S_ISREG(metadata.st_mode), "the exact lifecycle binary must be a regular file"
    assert not path.is_symlink(), "the exact lifecycle binary must not be a symlink"
    assert metadata.st_mode & 0o111 != 0, "the exact lifecycle binary must be executable"
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


def _managed_chat_config_commitment(document: str) -> str:
    payload = document.encode()
    digest = hashlib.sha256()
    digest.update(_MANAGED_CHAT_CONFIG_COMMITMENT_DOMAIN)
    digest.update(len(payload).to_bytes(8, "big"))
    digest.update(payload)
    return digest.hexdigest()


def _invoke_hidden_supervisor(
    binary: Path,
    config_path: Path,
    document: str,
    generation: str,
    environment: dict[str, str],
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [
            os.fspath(binary),
            _HIDDEN_SUPERVISOR_MODE.decode("ascii"),
            "--config",
            os.fspath(config_path),
            "--expected-config-commitment",
            _managed_chat_config_commitment(document),
            "--expected-generation",
            generation,
        ],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
    )


def _write_config(path: Path, document: str) -> None:
    path.write_text(document, encoding="utf-8")
    path.chmod(0o600)
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_nlink == 1
    assert metadata.st_mode & 0o7777 == 0o600


def _decode_one_json_object(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n"), "lifecycle stdout must end in one LF"
    assert raw.count(b"\n") == 1, "lifecycle stdout must contain exactly one JSON line"
    value = json.loads(raw)
    assert isinstance(value, dict), "lifecycle stdout must be a JSON object"
    compact = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    assert raw == compact, "lifecycle stdout must use compact JSON framing"
    return value


def _assert_generation(value: object) -> None:
    if value is None:
        return
    assert isinstance(value, str)
    assert _GENERATION_PATTERN.fullmatch(value) is not None


def _assert_public_envelope(envelope: dict[str, Any], command: str) -> None:
    assert set(envelope) == _LIFECYCLE_FIELDS
    assert envelope["schema_version"] == 1
    assert envelope["command"] == command
    assert type(envelope["ok"]) is bool
    assert envelope["state"] in _LIFECYCLE_STATES
    _assert_generation(envelope["generation"])
    assert type(envelope["changed"]) is bool
    assert type(envelope["owner_readiness_observed"]) is bool
    assert envelope["inspection_checked"] is False
    assert isinstance(envelope["diagnostics"], list)
    for diagnostic in envelope["diagnostics"]:
        assert isinstance(diagnostic, dict)
        assert set(diagnostic) == {"code", "message"}
        assert isinstance(diagnostic["code"], str) and diagnostic["code"]
        assert isinstance(diagnostic["message"], str) and diagnostic["message"]


def _assert_public_output_is_redacted(
    result: LifecycleResult, *, config_path: Path, state_root: Path
) -> None:
    combined = result.stdout + result.stderr
    for forbidden in (
        os.fsencode(config_path),
        os.fsencode(state_root),
        b"OPENAI_API_KEY",
        b"DEEPSEEK_API_KEY",
        b"env:OPENAI_API_KEY",
        b"env:DEEPSEEK_API_KEY",
        _OPENAI_SENTINEL.encode(),
        _DEEPSEEK_SENTINEL.encode(),
    ):
        assert forbidden not in combined
    assert re.search(rb"(?i)\b(?:pid|pgid|secretref)\b", combined) is None


def _invoke_lifecycle(
    binary: Path,
    command: str,
    config_path: Path,
    state_root: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int,
) -> LifecycleResult:
    process = _spawn_lifecycle(
        binary,
        command,
        config_path,
        environment,
    )
    return _finish_lifecycle(
        process,
        command,
        config_path,
        state_root,
        expected_returncode=expected_returncode,
    )


def _spawn_lifecycle(
    binary: Path,
    command: str,
    config_path: Path,
    environment: dict[str, str],
) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [os.fspath(binary), command, "--config", os.fspath(config_path), "--json"],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def _finish_lifecycle(
    process: subprocess.Popen[bytes],
    command: str,
    config_path: Path,
    state_root: Path,
    *,
    expected_returncode: int,
) -> LifecycleResult:
    try:
        stdout, stderr = process.communicate(timeout=_COMMAND_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        stdout, stderr = process.communicate()
        raise AssertionError(
            f"{command} exceeded {_COMMAND_TIMEOUT_SECONDS}s; stdout={stdout!r}; stderr={stderr!r}"
        ) from error
    assert process.returncode == expected_returncode, (
        f"{command} exited {process.returncode}; stdout={stdout!r}; stderr={stderr!r}"
    )
    assert stderr == b"", f"exact lifecycle grammar must keep stderr empty: {stderr!r}"
    envelope = _decode_one_json_object(stdout)
    _assert_public_envelope(envelope, command)
    result = LifecycleResult(
        returncode=process.returncode,
        envelope=envelope,
        stdout=stdout,
        stderr=stderr,
    )
    _assert_public_output_is_redacted(result, config_path=config_path, state_root=state_root)
    return result


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


def _assert_single_owner_graph(binary: Path) -> tuple[set[int], int]:
    processes = _matching_processes(binary)
    assert len(processes) >= 2, (
        "running lifecycle lacks the managed supervisor and its real Node child"
    )
    supervisors = {
        process_id
        for process_id in processes
        if _HIDDEN_SUPERVISOR_MODE in _process_command_line(process_id)
    }
    assert len(supervisors) == 1, "one lifecycle generation must have one supervisor"
    supervisor = next(iter(supervisors))
    assert os.getsid(supervisor) == supervisor, "the supervisor must own its detached session"
    assert {os.getsid(process_id) for process_id in processes} == {supervisor}, (
        "concurrent up created more than one owner graph"
    )
    return processes, supervisor


def _disconnect_after_private_status_request(state_root: Path, generation: str) -> None:
    record_path = state_root / _LIFECYCLE_RECORD_RELATIVE
    record = json.loads(record_path.read_bytes())
    assert isinstance(record, dict)
    assert record["schema_version"] == 1
    assert record["state"] == "running"
    assert record["generation"] == generation
    assert record["owner_readiness_observed"] is True
    commitment = record["config_commitment"]
    assert isinstance(commitment, str)
    assert re.fullmatch(r"[0-9a-f]{64}", commitment) is not None
    request = _INTERNAL_STATUS_PREFIX + bytes.fromhex(commitment)
    assert len(request) == 38

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(5.0)
        client.connect(os.fspath(state_root / _CONTROL_SOCKET_RELATIVE))
        client.sendall(request)
        try:
            client.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass


def _wait_for_no_matching_processes(binary: Path) -> None:
    deadline = time.monotonic() + _PROCESS_CLEANUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if not _matching_processes(binary):
            return
        time.sleep(0.05)
    assert not _matching_processes(binary), "joined down left a ParaEGOX process alive"


def _terminate_private_processes(binary: Path) -> None:
    # Harness-only reclamation after a failed assertion. No command is invoked
    # after SIGKILL, so this is not M2b orphan/recovery evidence.
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


def _private_file_identity_and_bytes(path: Path) -> tuple[int, int, int, int, bytes]:
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode & 0o7777,
        metadata.st_nlink,
        path.read_bytes(),
    )


def _socket_identity(path: Path) -> tuple[int, int, int]:
    metadata = path.lstat()
    assert stat.S_ISSOCK(metadata.st_mode)
    return metadata.st_dev, metadata.st_ino, metadata.st_mode & 0o7777


def _assert_secret_material_absent_from_files(
    root: Path,
    *,
    exact_binary: Path,
    public_input_files: tuple[Path, ...],
) -> None:
    if not root.exists():
        return
    forbidden_fragments = (
        _OPENAI_SENTINEL.encode(),
        _DEEPSEEK_SENTINEL.encode(),
        b"OPENAI_API_KEY",
        b"DEEPSEEK_API_KEY",
        b"env:OPENAI_API_KEY",
        b"env:DEEPSEEK_API_KEY",
    )
    overlap = max(map(len, forbidden_fragments)) - 1
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
                for forbidden in forbidden_fragments:
                    assert forbidden not in payload, f"Secret material leaked into {path.name}"
                carry = payload[-overlap:]


def test_hidden_supervisor_contention_is_distinct_from_prestate_failure() -> None:
    import fcntl

    assert sys.platform.startswith("linux"), "M2 lifecycle process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()

    temporary_parent = Path("/tmp").resolve(strict=True)
    with tempfile.TemporaryDirectory(
        prefix="paraegox-m2-hidden-contention-", dir=temporary_parent
    ) as raw:
        root = Path(raw).resolve(strict=True)
        root.chmod(0o700)
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        binary = binary_directory / "paraegox"
        _copy_exact_binary(source_binary, binary)

        state_root = root / "state"
        operator_root = state_root / "operator-v1"
        operator_root.mkdir(parents=True, mode=0o700)
        os.chown(state_root, -1, os.getegid())
        os.chown(operator_root, -1, os.getegid())
        state_root.chmod(0o700)
        operator_root.chmod(0o700)
        owner_lock = operator_root / "owner.lock"
        owner_lock.touch(mode=0o600)
        os.chown(owner_lock, -1, os.getegid())
        owner_lock.chmod(0o600)

        fixture_document = _chat_document(state_root, _reserve_loopback_port())
        fixture_config = root / "fixture.toml"
        _write_config(fixture_config, fixture_document)
        provisioned_document = _provisioned_chat_document(state_root, _reserve_loopback_port())
        provisioned_config = root / "provisioned.toml"
        _write_config(provisioned_config, provisioned_document)

        environment = os.environ.copy()
        environment.pop("PYTHONPATH", None)
        environment.pop("VIRTUAL_ENV", None)
        environment.update(
            {
                "HOME": os.fspath(root),
                "PATH": "/usr/bin:/bin",
                "DEEPSEEK_API_KEY": _DEEPSEEK_SENTINEL,
            }
        )
        initial_lock = _private_file_identity_and_bytes(owner_lock)
        with owner_lock.open("r+b", buffering=0) as locked:
            fcntl.flock(locked.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)

            contended = _invoke_hidden_supervisor(
                binary,
                fixture_config,
                fixture_document,
                "11" * 16,
                environment,
            )
            assert contended.returncode == 3
            assert contended.stdout == b""
            assert contended.stderr == b""

            missing_secret_environment = environment.copy()
            missing_secret_environment.pop("DEEPSEEK_API_KEY")
            prestate_failure = _invoke_hidden_supervisor(
                binary,
                provisioned_config,
                provisioned_document,
                "22" * 16,
                missing_secret_environment,
            )
            assert prestate_failure.returncode == 1
            assert prestate_failure.stdout == b""
            assert b"PXLC-PROVIDER-SECRET" in prestate_failure.stderr
            assert _DEEPSEEK_SENTINEL.encode() not in prestate_failure.stderr

        assert _private_file_identity_and_bytes(owner_lock) == initial_lock
        assert sorted(path.name for path in operator_root.iterdir()) == ["owner.lock"]
        assert not _matching_processes(binary)


def test_managed_local_lifecycle_up_status_idempotent_down_and_drift() -> None:
    assert sys.platform.startswith("linux"), "M2 lifecycle process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0, (
        "DeveloperLocal lifecycle evidence requires a non-root uid and gid"
    )
    source_binary = _require_exact_binary()

    temporary_parent = Path("/tmp").resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="paraegox-m2-lifecycle-", dir=temporary_parent) as raw:
        root = Path(raw).resolve(strict=True)
        root.chmod(0o700)
        binary_directory = root / "bin"
        temporary_directory = root / "tmp"
        binary_directory.mkdir(mode=0o700)
        temporary_directory.mkdir(mode=0o700)
        binary = binary_directory / "paraegox"
        _copy_exact_binary(source_binary, binary)

        state_root = root / "state"
        config_path = root / "paraegox.toml"
        missing_secret_state_root = root / "missing-secret-state"
        missing_secret_config_path = root / "missing-secret.toml"
        failed_state_root = root / "occupied-port-state"
        failed_config_path = root / "occupied-port.toml"
        original_document = _chat_document(state_root, _reserve_loopback_port())
        _write_config(config_path, original_document)
        _write_config(
            missing_secret_config_path,
            _provisioned_chat_document(
                missing_secret_state_root,
                _reserve_loopback_port(),
            ),
        )
        environment = os.environ.copy()
        environment.pop("PYTHONPATH", None)
        environment.pop("VIRTUAL_ENV", None)
        environment.update(
            {
                "HOME": os.fspath(root),
                "TMPDIR": os.fspath(temporary_directory),
                "PATH": "/usr/bin:/bin",
                "OPENAI_API_KEY": _OPENAI_SENTINEL,
                "DEEPSEEK_API_KEY": _DEEPSEEK_SENTINEL,
            }
        )

        try:
            missing_secret_environment = environment.copy()
            missing_secret_environment.pop("DEEPSEEK_API_KEY")
            missing_secret = _invoke_lifecycle(
                binary,
                "up",
                missing_secret_config_path,
                missing_secret_state_root,
                missing_secret_environment,
                expected_returncode=1,
            )
            assert missing_secret.envelope["ok"] is False
            assert missing_secret.envelope["state"] == "failed"
            assert missing_secret.envelope["generation"] is None
            assert missing_secret.envelope["changed"] is False
            assert missing_secret.envelope["owner_readiness_observed"] is False
            assert missing_secret.envelope["diagnostics"]
            assert not missing_secret_state_root.exists(), (
                "missing Secret created lifecycle or domain state"
            )
            assert not _matching_processes(binary), (
                "missing Secret left a lifecycle or owner process"
            )
            assert _socket_paths(root) == [], "missing Secret left a Unix socket"

            never_started = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert never_started.envelope == {
                "schema_version": 1,
                "command": "status",
                "ok": True,
                "state": "never_started",
                "generation": None,
                "changed": False,
                "owner_readiness_observed": False,
                "inspection_checked": False,
                "diagnostics": [],
            }
            assert not state_root.exists(), "status mutated a never-started state root"

            first_up_process = _spawn_lifecycle(
                binary,
                "up",
                config_path,
                environment,
            )
            second_up_process = _spawn_lifecycle(
                binary,
                "up",
                config_path,
                environment,
            )
            overlapped = first_up_process.poll() is None and second_up_process.poll() is None
            concurrent_ups = [
                _finish_lifecycle(
                    process,
                    "up",
                    config_path,
                    state_root,
                    expected_returncode=0,
                )
                for process in (first_up_process, second_up_process)
            ]
            assert overlapped, "both never-started up clients must overlap as real processes"
            for concurrent_up in concurrent_ups:
                assert concurrent_up.envelope["ok"] is True
                assert concurrent_up.envelope["state"] == "running"
                assert concurrent_up.envelope["owner_readiness_observed"] is True
                assert concurrent_up.envelope["diagnostics"] == []
            assert sorted(result.envelope["changed"] for result in concurrent_ups) == [
                False,
                True,
            ]
            generations = {result.envelope["generation"] for result in concurrent_ups}
            assert len(generations) == 1
            generation = generations.pop()
            assert isinstance(generation, str)

            ready_processes, _ = _assert_single_owner_graph(binary)
            running_record = _private_file_identity_and_bytes(
                state_root / _LIFECYCLE_RECORD_RELATIVE
            )
            running_socket = _socket_identity(state_root / _CONTROL_SOCKET_RELATIVE)

            running = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert running.envelope["ok"] is True
            assert running.envelope["state"] == "running"
            assert running.envelope["generation"] == generation
            assert running.envelope["changed"] is False
            assert running.envelope["owner_readiness_observed"] is True
            assert running.envelope["diagnostics"] == []

            _disconnect_after_private_status_request(state_root, generation)
            after_private_disconnect = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert after_private_disconnect.envelope["ok"] is True
            assert after_private_disconnect.envelope["state"] == "running"
            assert after_private_disconnect.envelope["generation"] == generation
            assert after_private_disconnect.envelope["changed"] is False
            assert after_private_disconnect.envelope["owner_readiness_observed"] is True
            assert after_private_disconnect.envelope["diagnostics"] == []
            assert _matching_processes(binary) == ready_processes
            assert (
                _private_file_identity_and_bytes(state_root / _LIFECYCLE_RECORD_RELATIVE)
                == running_record
            ), "status must not rewrite the lifecycle record"
            assert _socket_identity(state_root / _CONTROL_SOCKET_RELATIVE) == running_socket, (
                "status must not replace the control socket"
            )

            duplicate = _invoke_lifecycle(
                binary,
                "up",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert duplicate.envelope["ok"] is True
            assert duplicate.envelope["state"] == "running"
            assert duplicate.envelope["generation"] == generation
            assert duplicate.envelope["changed"] is False
            assert duplicate.envelope["owner_readiness_observed"] is True
            assert duplicate.envelope["diagnostics"] == []
            assert _matching_processes(binary) == ready_processes, (
                "idempotent up created or replaced a managed process"
            )

            drift_document = _chat_document(state_root, _reserve_loopback_port())
            assert drift_document != original_document
            _write_config(config_path, drift_document)
            drift = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=2,
            )
            assert drift.envelope["ok"] is False
            assert drift.envelope["state"] == "unknown"
            assert drift.envelope["generation"] == generation
            assert drift.envelope["changed"] is False
            assert drift.envelope["owner_readiness_observed"] is True
            assert drift.envelope["inspection_checked"] is False
            assert drift.envelope["diagnostics"]
            assert _matching_processes(binary) == ready_processes

            _write_config(config_path, original_document)
            after_drift = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert after_drift.envelope["ok"] is True
            assert after_drift.envelope["state"] == "running"
            assert after_drift.envelope["generation"] == generation
            assert after_drift.envelope["changed"] is False
            assert after_drift.envelope["owner_readiness_observed"] is True
            assert after_drift.envelope["diagnostics"] == []

            down = _invoke_lifecycle(
                binary,
                "down",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert down.envelope["ok"] is True
            assert down.envelope["state"] == "stopped"
            assert down.envelope["generation"] == generation
            assert down.envelope["changed"] is True
            assert down.envelope["owner_readiness_observed"] is True
            assert down.envelope["diagnostics"] == []
            assert len(_matching_processes(binary)) <= 1, (
                "down replied stopped before its composition children had joined"
            )
            _wait_for_no_matching_processes(binary)
            assert _socket_paths(root) == [], "joined down left a Unix socket behind"

            stopped = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert stopped.envelope["ok"] is True
            assert stopped.envelope["state"] == "stopped"
            assert stopped.envelope["generation"] == generation
            assert stopped.envelope["changed"] is False
            assert stopped.envelope["owner_readiness_observed"] is True
            assert stopped.envelope["diagnostics"] == []

            duplicate_down = _invoke_lifecycle(
                binary,
                "down",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert duplicate_down.envelope["ok"] is True
            assert duplicate_down.envelope["state"] == "stopped"
            assert duplicate_down.envelope["generation"] == generation
            assert duplicate_down.envelope["changed"] is False
            assert duplicate_down.envelope["owner_readiness_observed"] is True
            assert duplicate_down.envelope["diagnostics"] == []
            _wait_for_no_matching_processes(binary)

            restart_first_process = _spawn_lifecycle(
                binary,
                "up",
                config_path,
                environment,
            )
            restart_second_process = _spawn_lifecycle(
                binary,
                "up",
                config_path,
                environment,
            )
            restart_overlapped = (
                restart_first_process.poll() is None and restart_second_process.poll() is None
            )
            restarted_ups = [
                _finish_lifecycle(
                    process,
                    "up",
                    config_path,
                    state_root,
                    expected_returncode=0,
                )
                for process in (restart_first_process, restart_second_process)
            ]
            assert restart_overlapped, (
                "both stopped-generation up clients must overlap as real processes"
            )
            for restarted_up in restarted_ups:
                assert restarted_up.envelope["ok"] is True
                assert restarted_up.envelope["state"] == "running"
                assert restarted_up.envelope["owner_readiness_observed"] is True
                assert restarted_up.envelope["diagnostics"] == []
            assert sorted(result.envelope["changed"] for result in restarted_ups) == [
                False,
                True,
            ]
            restart_generations = {result.envelope["generation"] for result in restarted_ups}
            assert len(restart_generations) == 1
            second_generation = restart_generations.pop()
            assert isinstance(second_generation, str)
            assert second_generation != generation
            _, second_supervisor = _assert_single_owner_graph(binary)
            assert _HIDDEN_SUPERVISOR_MODE in _process_command_line(second_supervisor)

            os.kill(second_supervisor, signal.SIGTERM)
            _wait_for_no_matching_processes(binary)
            signaled_stopped = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                state_root,
                environment,
                expected_returncode=0,
            )
            assert signaled_stopped.envelope["ok"] is True
            assert signaled_stopped.envelope["state"] == "stopped"
            assert signaled_stopped.envelope["generation"] == second_generation
            assert signaled_stopped.envelope["changed"] is False
            assert signaled_stopped.envelope["owner_readiness_observed"] is True
            assert signaled_stopped.envelope["diagnostics"] == []
            assert not _matching_processes(binary)
            assert _socket_paths(root) == [], (
                "SIGTERM-triggered joined shutdown left a Unix socket behind"
            )

            provisioned_up = _invoke_lifecycle(
                binary,
                "up",
                missing_secret_config_path,
                missing_secret_state_root,
                environment,
                expected_returncode=0,
            )
            assert provisioned_up.envelope["ok"] is True
            assert provisioned_up.envelope["state"] == "running"
            assert provisioned_up.envelope["changed"] is True
            assert provisioned_up.envelope["owner_readiness_observed"] is True
            provisioned_generation = provisioned_up.envelope["generation"]
            assert isinstance(provisioned_generation, str)
            _assert_single_owner_graph(binary)

            provisioned_down = _invoke_lifecycle(
                binary,
                "down",
                missing_secret_config_path,
                missing_secret_state_root,
                environment,
                expected_returncode=0,
            )
            assert provisioned_down.envelope["ok"] is True
            assert provisioned_down.envelope["state"] == "stopped"
            assert provisioned_down.envelope["generation"] == provisioned_generation
            assert provisioned_down.envelope["changed"] is True
            assert provisioned_down.envelope["owner_readiness_observed"] is True
            _wait_for_no_matching_processes(binary)
            provisioned_terminal_record = _private_file_identity_and_bytes(
                missing_secret_state_root / _LIFECYCLE_RECORD_RELATIVE
            )

            stopped_missing_secret = _invoke_lifecycle(
                binary,
                "up",
                missing_secret_config_path,
                missing_secret_state_root,
                missing_secret_environment,
                expected_returncode=1,
            )
            assert stopped_missing_secret.envelope["ok"] is False
            assert stopped_missing_secret.envelope["state"] == "failed"
            assert stopped_missing_secret.envelope["generation"] == provisioned_generation
            assert stopped_missing_secret.envelope["changed"] is False
            assert stopped_missing_secret.envelope["owner_readiness_observed"] is True
            assert stopped_missing_secret.envelope["diagnostics"]
            assert (
                _private_file_identity_and_bytes(
                    missing_secret_state_root / _LIFECYCLE_RECORD_RELATIVE
                )
                == provisioned_terminal_record
            ), "pre-state Secret failure rewrote the prior terminal generation"
            assert not _matching_processes(binary)
            assert _socket_paths(root) == []

            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied_listener:
                occupied_listener.bind(("127.0.0.1", 0))
                occupied_listener.listen(1)
                occupied_port = int(occupied_listener.getsockname()[1])
                _write_config(
                    failed_config_path,
                    _chat_document(failed_state_root, occupied_port),
                )
                failed_up = _invoke_lifecycle(
                    binary,
                    "up",
                    failed_config_path,
                    failed_state_root,
                    environment,
                    expected_returncode=1,
                )
            assert failed_up.envelope["ok"] is False
            assert failed_up.envelope["state"] == "unknown"
            assert isinstance(failed_up.envelope["generation"], str)
            assert failed_up.envelope["changed"] is True
            assert failed_up.envelope["owner_readiness_observed"] is False
            _wait_for_no_matching_processes(binary)
            assert _socket_paths(root) == [], "failed owner startup left a Unix socket behind"
        finally:
            if config_path.exists():
                _write_config(config_path, original_document)
                try:
                    _invoke_lifecycle(
                        binary,
                        "down",
                        config_path,
                        state_root,
                        environment,
                        expected_returncode=0,
                    )
                except (AssertionError, OSError, subprocess.SubprocessError):
                    pass
            _terminate_private_processes(binary)
            assert not _matching_processes(binary), "test cleanup could not reclaim ParaEGOX"
            _assert_secret_material_absent_from_files(
                root,
                exact_binary=binary,
                public_input_files=(
                    config_path,
                    missing_secret_config_path,
                    failed_config_path,
                ),
            )
