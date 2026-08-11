from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import socket
import stat
import subprocess
import tempfile
from pathlib import Path
from typing import Any

import pytest

_BINARY_ENVIRONMENT = "PARAEGOX_D0B_EXTERNAL_ARTIFACT_CLI_BINARY"
_COMMAND_TIMEOUT_SECONDS = 180.0
_GENERATION_PATTERN = re.compile(r"[0-9a-f]{32}")
_DIGEST_PATTERN = re.compile(r"[0-9a-f]{64}")
_DECIMAL_PATTERN = re.compile(r"[1-9][0-9]*")
_DEPLOYMENT_FIELDS = {
    "schema_version",
    "command",
    "mode",
    "ok",
    "changed",
    "operation_id",
    "state",
    "profile",
    "artifact_object_ref",
    "materialization_receipt_ref",
    "generation",
    "deployment_revision",
    "controller_snapshot_sequence",
    "deployment_receipt_ref",
    "runtime_apply_request_digest",
    "runtime_terminal_receipt_digest",
    "terminal_outcome",
    "current_health_checked",
    "diagnostics",
}


def _require_exact_binary() -> Path:
    configured = os.environ.get(_BINARY_ENVIRONMENT)
    assert configured is not None, (
        f"{_BINARY_ENVIRONMENT} must name the exact-revision CLI binary"
    )
    binary = Path(configured)
    assert binary.is_absolute()
    metadata = binary.lstat()
    assert stat.S_ISREG(metadata.st_mode) and not binary.is_symlink()
    assert metadata.st_mode & 0o111
    return binary.resolve(strict=True)


def _copy_exact_binary(source: Path, target: Path) -> None:
    shutil.copyfile(source, target)
    target.chmod(0o755)
    assert hashlib.sha256(target.read_bytes()).digest() == hashlib.sha256(
        source.read_bytes()
    ).digest()
    metadata = target.lstat()
    assert stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1
    assert stat.S_IMODE(metadata.st_mode) == 0o755


def _reserve_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def _write_config(path: Path, state_root: Path, fabric_port: int) -> None:
    path.write_text(
        "schema_version = 1\n"
        f"state_root = {json.dumps(os.fspath(state_root))}\n"
        f'fabric_listen = "tcp/127.0.0.1:{fabric_port}"\n'
        "\n[model]\n"
        'provider = "deterministic-echo-v1"\n',
        encoding="utf-8",
    )
    path.chmod(0o600)
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1
    assert stat.S_IMODE(metadata.st_mode) == 0o600


def _environment(root: Path) -> dict[str, str]:
    temporary = root / "tmp"
    temporary.mkdir(mode=0o700)
    return {
        "HOME": os.fspath(root),
        "TMPDIR": os.fspath(temporary),
        "PATH": "/usr/bin:/bin",
        "LANG": "C.UTF-8",
        "PARAEGOX_D0B_RUNTIME_DIAGNOSTIC_PATH": os.fspath(
            root / "d0b-runtime-diagnostic.txt"
        ),
    }


def _decode_json_line(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n") and raw.count(b"\n") == 1
    value = json.loads(raw)
    assert isinstance(value, dict)
    assert raw == json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    return value


def _invoke_json(
    binary: Path,
    arguments: list[str],
    environment: dict[str, str],
    *,
    expected_returncode: int = 0,
) -> dict[str, Any]:
    process = subprocess.run(
        [os.fspath(binary), *arguments],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=_COMMAND_TIMEOUT_SECONDS,
        check=False,
    )
    if process.returncode != expected_returncode:
        print(f"D0B-STDOUT:{process.stdout.decode(errors='backslashreplace')}")
        print(f"D0B-STDERR:{process.stderr.decode(errors='backslashreplace')}")
        _print_d0b_failure_state(environment)
    assert process.returncode == expected_returncode, (
        arguments,
        process.returncode,
        process.stdout,
        process.stderr,
    )
    assert process.stderr == b""
    return _decode_json_line(process.stdout)


def _print_d0b_failure_state(environment: dict[str, str]) -> None:
    diagnostic_path = Path(environment["PARAEGOX_D0B_RUNTIME_DIAGNOSTIC_PATH"])
    if diagnostic_path.is_file():
        print(
            "D0B-RUNTIME-DIAGNOSTIC:"
            f"{diagnostic_path.read_text(encoding='utf-8', errors='backslashreplace').strip()}"
        )
    state_root = Path(environment["HOME"]) / "state"
    if not state_root.is_dir():
        print("D0B-STATE:<absent>")
        return
    for path in sorted(state_root.rglob("*")):
        relative = path.relative_to(state_root)
        try:
            metadata = path.lstat()
        except OSError as error:
            print(f"D0B-STATE:{relative}:lstat={error!r}")
            continue
        print(
            f"D0B-STATE:{relative}:mode={stat.S_IMODE(metadata.st_mode):04o}:"
            f"size={metadata.st_size}"
        )
        if path.name != "managed-model-agent-stack.snapshot-v1":
            continue
        try:
            header = path.read_bytes()[:208]
        except OSError as error:
            print(f"D0B-PXMA:{relative}:read={error!r}")
            continue
        if len(header) < 208 or header[:4] != b"PXMA":
            print(f"D0B-PXMA:{relative}:invalid-header={header.hex()}")
            continue
        print(
            f"D0B-PXMA:{relative}:version={int.from_bytes(header[4:6], 'big')}:"
            f"sequence={int.from_bytes(header[12:20], 'big')}:phase={header[140]}:"
            f"fabric={int.from_bytes(header[116:124], 'big')}:"
            f"model={int.from_bytes(header[124:132], 'big')}:"
            f"agent={int.from_bytes(header[132:140], 'big')}:"
            f"census={int.from_bytes(header[141:143], 'big')}:"
            f"complete={header[143]}:ready={header[144:147].hex()}"
        )


def _matching_processes(binary: Path) -> list[int]:
    matches = []
    for candidate in Path("/proc").iterdir():
        if not candidate.name.isdecimal():
            continue
        try:
            executable = (candidate / "exe").resolve(strict=True)
        except (FileNotFoundError, PermissionError, OSError):
            continue
        if executable == binary:
            matches.append(int(candidate.name))
    return sorted(matches)


def _terminate_fixture_processes(binary: Path) -> None:
    for process_id in _matching_processes(binary):
        try:
            os.kill(process_id, 15)
        except ProcessLookupError:
            pass


def _assert_active_ready(
    envelope: dict[str, Any],
    *,
    command: str,
    changed: bool,
    operation_id: str,
    object_ref: str,
    receipt_ref: str,
) -> None:
    assert set(envelope) == _DEPLOYMENT_FIELDS
    assert envelope["schema_version"] == 1
    assert envelope["command"] == command
    assert envelope["mode"] == "local"
    assert envelope["ok"] is True
    assert envelope["changed"] is changed
    assert envelope["operation_id"] == operation_id
    assert envelope["state"] == "active_ready"
    assert envelope["profile"] == "developer-local-echo-prefix-v1"
    assert envelope["artifact_object_ref"] == object_ref
    assert envelope["materialization_receipt_ref"] == receipt_ref
    assert _GENERATION_PATTERN.fullmatch(envelope["generation"])
    assert _DECIMAL_PATTERN.fullmatch(envelope["deployment_revision"])
    assert envelope["controller_snapshot_sequence"] == "2"
    assert isinstance(envelope["deployment_receipt_ref"], str)
    assert envelope["deployment_receipt_ref"].startswith("pxdor1:")
    assert _DIGEST_PATTERN.fullmatch(envelope["runtime_apply_request_digest"])
    assert _DIGEST_PATTERN.fullmatch(envelope["runtime_terminal_receipt_digest"])
    assert envelope["terminal_outcome"] == "active_ready"
    assert envelope["current_health_checked"] is False
    assert envelope["diagnostics"] == []


@pytest.mark.parametrize("_attempt", range(30))
def test_d0b_external_artifact_reaches_active_ready_replays_queries_and_joins(
    _attempt: int,
) -> None:
    assert os.name == "posix" and Path("/proc").is_dir()
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()

    with tempfile.TemporaryDirectory(prefix=".paraegox-d0b-", dir=Path.home()) as raw:
        root = Path(raw).resolve(strict=True)
        root.chmod(0o700)
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        binary = binary_directory / "paraegox"
        _copy_exact_binary(source_binary, binary)
        environment = _environment(root)
        state_root = root / "state"
        config_path = root / "paraegox.toml"
        _write_config(config_path, state_root, _reserve_loopback_port())
        source = root / "prefix.txt"
        source.write_bytes(b"artifact-d0b-prefix: ")
        source.chmod(0o600)
        output_parent = root / "artifact-builds"
        output_parent.mkdir(mode=0o700)
        output = output_parent / "prefix-object"
        operation_id = "d1" * 16
        materialization_operation_id = "a2" * 16

        try:
            built = _invoke_json(
                binary,
                [
                    "artifact",
                    "build",
                    "--profile",
                    "developer-local-echo-prefix-v1",
                    "--source",
                    os.fspath(source),
                    "--output",
                    os.fspath(output),
                    "--json",
                ],
                environment,
            )
            object_ref = built["artifact_object_ref"]
            assert isinstance(object_ref, str) and object_ref.startswith("sha256:")
            materialized = _invoke_json(
                binary,
                [
                    "artifact",
                    "materialize",
                    "--config",
                    os.fspath(config_path),
                    "--manifest",
                    os.fspath(output / "manifest.pxam"),
                    "--payload",
                    os.fspath(output / "payload.bin"),
                    "--operation-id",
                    materialization_operation_id,
                    "--json",
                ],
                environment,
            )
            receipt_ref = materialized["materialization_receipt_ref"]
            assert isinstance(receipt_ref, str) and receipt_ref.startswith("pxamr1:")
            assert not (state_root / "operator-v1").exists()

            deploy_arguments = [
                "deploy",
                "--local",
                "--config",
                os.fspath(config_path),
                "--artifact-object-ref",
                object_ref,
                "--materialization-receipt-ref",
                receipt_ref,
                "--operation-id",
                operation_id,
                "--json",
            ]
            deployed = _invoke_json(binary, deploy_arguments, environment)
            _assert_active_ready(
                deployed,
                command="deploy",
                changed=True,
                operation_id=operation_id,
                object_ref=object_ref,
                receipt_ref=receipt_ref,
            )
            assert sorted(
                path.name for path in (state_root / "artifact-external-controller-v1").iterdir()
            ) == ["artifact-external.lock", "artifact-external.pxmj"]

            queried = _invoke_json(
                binary,
                [
                    "deployment",
                    "operation",
                    "query",
                    "--config",
                    os.fspath(config_path),
                    "--operation-id",
                    operation_id,
                    "--json",
                ],
                environment,
            )
            _assert_active_ready(
                queried,
                command="deployment.operation.query",
                changed=False,
                operation_id=operation_id,
                object_ref=object_ref,
                receipt_ref=receipt_ref,
            )
            replayed = _invoke_json(binary, deploy_arguments, environment)
            _assert_active_ready(
                replayed,
                command="deploy",
                changed=False,
                operation_id=operation_id,
                object_ref=object_ref,
                receipt_ref=receipt_ref,
            )
            assert replayed["generation"] == deployed["generation"]

            status = _invoke_json(
                binary,
                ["status", "--config", os.fspath(config_path), "--json"],
                environment,
            )
            assert status["ok"] is True and status["state"] == "running"
            assert status["generation"] == deployed["generation"]
            down = _invoke_json(
                binary,
                ["down", "--config", os.fspath(config_path), "--json"],
                environment,
            )
            assert down["ok"] is True and down["state"] == "stopped"
            assert down["changed"] is True
            assert _matching_processes(binary) == []
        finally:
            if config_path.exists() and _matching_processes(binary):
                try:
                    _invoke_json(
                        binary,
                        ["down", "--config", os.fspath(config_path), "--json"],
                        environment,
                    )
                except (AssertionError, OSError, subprocess.SubprocessError):
                    pass
            _terminate_fixture_processes(binary)
            assert _matching_processes(binary) == []
