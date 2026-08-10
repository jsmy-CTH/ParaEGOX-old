from __future__ import annotations

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

_BINARY_ENVIRONMENT = "PARAEGOX_D0A_CLI_BINARY"
_COMMAND_TIMEOUT_SECONDS = 180.0
_CLEANUP_TIMEOUT_SECONDS = 30.0
_HIDDEN_SUPERVISOR_MODE = b"__local-chat-supervisor-v1"
_CONFIG_COMMITMENT_DOMAIN = b"paraegox.local.managed-chat-config.sha256.v1"
_LIFECYCLE_RECORD_RELATIVE = Path("operator-v1/lifecycle-v1.json")
_CONTROL_SOCKET_RELATIVE = Path("operator-v1/control-v1.sock")
_DEPLOY_QUERY_PREFIX = b"PXLO\x01P"
_DEPLOY_FIELDS = {
    "schema_version",
    "command",
    "mode",
    "ok",
    "profile",
    "changed",
    "generation",
    "deployment_revision",
    "controller_snapshot_sequence",
    "runtime_apply_request_digest",
    "runtime_terminal_receipt_digest",
    "terminal_outcome",
    "current_health_checked",
    "diagnostics",
}
_GENERATION_PATTERN = re.compile(r"[0-9a-f]{32}")
_DIGEST_PATTERN = re.compile(r"[0-9a-f]{64}")
_CANONICAL_DECIMAL_PATTERN = re.compile(r"0|[1-9][0-9]*")
_OPENAI_SENTINEL = "d0a-openai-secret-value-must-not-leak"
_DEEPSEEK_SENTINEL = "d0a-deepseek-secret-value-must-not-leak"
_ANY_CHANGED = object()


@dataclass(frozen=True)
class DeployResult:
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
    assert stat.S_ISREG(metadata.st_mode), "the exact D0a binary must be a regular file"
    assert not path.is_symlink(), "the exact D0a binary must not be a symlink"
    assert metadata.st_mode & 0o111 != 0, "the exact D0a binary must be executable"
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
    return {
        "HOME": os.fspath(root),
        "TMPDIR": os.fspath(root / "tmp"),
        "PATH": "/usr/bin:/bin",
        "LANG": "C.UTF-8",
        "OPENAI_API_KEY": _OPENAI_SENTINEL,
        "DEEPSEEK_API_KEY": _DEEPSEEK_SENTINEL,
    }


def _decode_one_compact_json_object(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n"), "deploy stdout must end in one LF"
    assert raw.count(b"\n") == 1, "deploy stdout must contain exactly one JSON line"
    value = json.loads(raw)
    assert isinstance(value, dict), "deploy stdout must be one JSON object"
    compact = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    assert raw == compact, "deploy stdout must use compact JSON framing"
    return value


def _assert_diagnostics(value: object, *, expected_count: int) -> None:
    assert isinstance(value, list)
    assert len(value) == expected_count
    for diagnostic in value:
        assert isinstance(diagnostic, dict)
        assert set(diagnostic) == {"code", "message"}
        assert isinstance(diagnostic["code"], str) and diagnostic["code"]
        assert isinstance(diagnostic["message"], str) and diagnostic["message"]


def _assert_deploy_envelope(
    envelope: dict[str, Any],
    *,
    ok: bool,
    expected_changed: bool | None | object,
) -> None:
    assert set(envelope) == _DEPLOY_FIELDS
    assert type(envelope["schema_version"]) is int and envelope["schema_version"] == 1
    assert envelope["command"] == "deploy"
    assert envelope["mode"] == "local"
    assert type(envelope["ok"]) is bool and envelope["ok"] is ok
    if expected_changed is not _ANY_CHANGED:
        assert envelope["changed"] is expected_changed
    assert envelope["current_health_checked"] is False

    if ok:
        assert type(envelope["changed"]) is bool
        assert envelope["profile"] == "deterministic-echo-v1"
        assert isinstance(envelope["generation"], str)
        assert _GENERATION_PATTERN.fullmatch(envelope["generation"]) is not None
        for field in ("deployment_revision", "controller_snapshot_sequence"):
            assert isinstance(envelope[field], str)
            assert _CANONICAL_DECIMAL_PATTERN.fullmatch(envelope[field]) is not None
        for field in ("runtime_apply_request_digest", "runtime_terminal_receipt_digest"):
            assert isinstance(envelope[field], str)
            assert _DIGEST_PATTERN.fullmatch(envelope[field]) is not None
        assert envelope["terminal_outcome"] == "active_ready"
        _assert_diagnostics(envelope["diagnostics"], expected_count=0)
    else:
        assert envelope["changed"] is False or envelope["changed"] is None
        for field in (
            "profile",
            "generation",
            "deployment_revision",
            "controller_snapshot_sequence",
            "runtime_apply_request_digest",
            "runtime_terminal_receipt_digest",
            "terminal_outcome",
        ):
            assert envelope[field] is None
        _assert_diagnostics(envelope["diagnostics"], expected_count=1)


def _assert_public_output_is_redacted(
    result: DeployResult, *, config_path: Path, state_root: Path
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
    sensitive_term = (
        rb"(?i)\b(?:pid|pgid|uid|gid|secretref|credential|seed|private[-_ ]key|"
        rb"capability|token|endpoint|route)\b"
    )
    assert re.search(sensitive_term, combined) is None


def _spawn_deploy(
    binary: Path,
    config_path: Path,
    environment: dict[str, str],
    *,
    stdout: int | None = subprocess.PIPE,
    pass_fds: tuple[int, ...] = (),
) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
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
        stdout=stdout,
        stderr=subprocess.PIPE,
        pass_fds=pass_fds,
    )


def _finish_deploy(
    process: subprocess.Popen[bytes],
    config_path: Path,
    state_root: Path,
    *,
    expected_returncode: int,
    expected_changed: bool | None | object,
) -> DeployResult:
    try:
        stdout, stderr = process.communicate(timeout=_COMMAND_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        stdout, stderr = process.communicate()
        raise AssertionError(
            f"deploy exceeded {_COMMAND_TIMEOUT_SECONDS}s; stdout={stdout!r}; stderr={stderr!r}"
        ) from error
    assert process.returncode == expected_returncode, (
        f"deploy exited {process.returncode}; stdout={stdout!r}; stderr={stderr!r}"
    )
    assert stdout is not None
    assert stderr == b"", f"exact deploy grammar must keep stderr empty: {stderr!r}"
    envelope = _decode_one_compact_json_object(stdout)
    _assert_deploy_envelope(
        envelope,
        ok=expected_returncode == 0,
        expected_changed=expected_changed,
    )
    result = DeployResult(process.returncode, envelope, stdout, stderr)
    _assert_public_output_is_redacted(result, config_path=config_path, state_root=state_root)
    return result


def _invoke_deploy(
    binary: Path,
    config_path: Path,
    state_root: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int,
    expected_changed: bool | None | object,
) -> DeployResult:
    return _finish_deploy(
        _spawn_deploy(binary, config_path, environment),
        config_path,
        state_root,
        expected_returncode=expected_returncode,
        expected_changed=expected_changed,
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
    )
    assert process.returncode == expected_returncode, (
        f"{command} exited {process.returncode}; stdout={process.stdout!r}; "
        f"stderr={process.stderr!r}"
    )
    assert process.stderr == b""
    return _decode_one_compact_json_object(process.stdout)


def _invoke_init(binary: Path, directory: Path, environment: dict[str, str]) -> dict[str, Any]:
    process = subprocess.run(
        [os.fspath(binary), "init", "--directory", os.fspath(directory), "--json"],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
    )
    assert process.returncode == 0, (
        f"init exited {process.returncode}; stdout={process.stdout!r}; stderr={process.stderr!r}"
    )
    assert process.stderr == b""
    return _decode_one_compact_json_object(process.stdout)


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
    assert len(processes) >= 2, "deploy success lacks the supervisor and real Node child"
    supervisors = {
        process_id
        for process_id in processes
        if _HIDDEN_SUPERVISOR_MODE in _process_command_line(process_id)
    }
    assert len(supervisors) == 1, "one D0a generation must have one supervisor"
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


def _terminate_private_processes(binary: Path) -> None:
    # Harness-only reclamation after a failed assertion or SIGKILL scenario.
    # It is not product orphan-recovery evidence.
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
    paths: list[Path] = []
    for path in root.rglob("*"):
        try:
            metadata = path.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISSOCK(metadata.st_mode):
            paths.append(path)
    return paths


def _assert_secret_material_absent_from_files(
    root: Path, *, exact_binary: Path, public_input_files: tuple[Path, ...]
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


def _deployment_identity(envelope: dict[str, Any]) -> tuple[object, ...]:
    return tuple(
        envelope[field]
        for field in (
            "generation",
            "deployment_revision",
            "controller_snapshot_sequence",
            "runtime_apply_request_digest",
            "runtime_terminal_receipt_digest",
            "terminal_outcome",
        )
    )


def _managed_chat_config_commitment(document: str) -> bytes:
    payload = document.encode()
    digest = hashlib.sha256()
    digest.update(_CONFIG_COMMITMENT_DOMAIN)
    digest.update(len(payload).to_bytes(8, "big"))
    digest.update(payload)
    return digest.digest()


def _raw_deploy_query(
    state_root: Path,
    document: str,
    expected_generation: str,
) -> bytes:
    request = (
        _DEPLOY_QUERY_PREFIX
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
            assert len(response) <= 4096, "private DeployQuery response exceeded its bound"
    return bytes(response)


def _assert_private_projection_matches_public(
    raw: bytes,
    public: dict[str, Any],
) -> dict[str, Any]:
    projection = json.loads(raw)
    assert isinstance(projection, dict)
    assert set(projection) == {
        "generation",
        "deployment_revision",
        "controller_snapshot_sequence",
        "runtime_apply_request_digest",
        "runtime_terminal_receipt_digest",
        "terminal_outcome",
        "fabric_replayed",
        "model_agent_replayed",
    }
    assert projection["generation"] == public["generation"]
    assert type(projection["deployment_revision"]) is int
    assert str(projection["deployment_revision"]) == public["deployment_revision"]
    assert type(projection["controller_snapshot_sequence"]) is int
    assert str(projection["controller_snapshot_sequence"]) == public[
        "controller_snapshot_sequence"
    ]
    assert projection["runtime_apply_request_digest"] == public[
        "runtime_apply_request_digest"
    ]
    assert projection["runtime_terminal_receipt_digest"] == public[
        "runtime_terminal_receipt_digest"
    ]
    assert projection["terminal_outcome"] == "active_ready"
    assert type(projection["fabric_replayed"]) is bool
    assert type(projection["model_agent_replayed"]) is bool
    return projection


_DEPLOY_QUERY_INTERPOSER_SOURCE = r"""
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/socket.h>
#include <unistd.h>

static const unsigned char deploy_prefix[6] = {'P', 'X', 'L', 'O', 1, 'P'};
static __thread int inside_interposer = 0;

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

static int is_deploy_query(const void *buffer, size_t count) {
    return count >= sizeof(deploy_prefix)
        && memcmp(buffer, deploy_prefix, sizeof(deploy_prefix)) == 0;
}

static int is_deploy_query_iov(const struct iovec *iov, size_t iovcnt) {
    unsigned char observed[sizeof(deploy_prefix)];
    size_t copied = 0;
    for (size_t index = 0; index < iovcnt && copied < sizeof(observed); ++index) {
        size_t available = iov[index].iov_len;
        size_t wanted = sizeof(observed) - copied;
        size_t take = available < wanted ? available : wanted;
        memcpy(observed + copied, iov[index].iov_base, take);
        copied += take;
    }
    return copied == sizeof(observed)
        && memcmp(observed, deploy_prefix, sizeof(deploy_prefix)) == 0;
}

static int before_deploy_query(void) {
    const char *mode = getenv("PARAEGOX_D0A_INTERPOSE_MODE");
    if (mode == NULL || inside_interposer) {
        return 0;
    }
    inside_interposer = 1;
    int audit_fd = configured_fd("PARAEGOX_D0A_AUDIT_FD");
    if (strcmp(mode, "fail") == 0) {
        const char marker = 'F';
        if (audit_fd >= 0) {
            (void)syscall(SYS_write, audit_fd, &marker, 1);
        }
        inside_interposer = 0;
        errno = EIO;
        return -1;
    }
    if (strcmp(mode, "barrier") == 0) {
        const char marker = 'B';
        if (audit_fd >= 0) {
            (void)syscall(SYS_write, audit_fd, &marker, 1);
        }
        int release_fd = configured_fd("PARAEGOX_D0A_RELEASE_FD");
        char release = 0;
        if (release_fd < 0 || syscall(SYS_read, release_fd, &release, 1) != 1) {
            inside_interposer = 0;
            errno = EIO;
            return -1;
        }
    }
    inside_interposer = 0;
    return 0;
}

ssize_t write(int fd, const void *buffer, size_t count) {
    static ssize_t (*real_write)(int, const void *, size_t) = NULL;
    if (real_write == NULL) {
        real_write = dlsym(RTLD_NEXT, "write");
    }
    if (is_deploy_query(buffer, count) && before_deploy_query() != 0) {
        return -1;
    }
    return real_write(fd, buffer, count);
}

ssize_t send(int fd, const void *buffer, size_t count, int flags) {
    static ssize_t (*real_send)(int, const void *, size_t, int) = NULL;
    if (real_send == NULL) {
        real_send = dlsym(RTLD_NEXT, "send");
    }
    if (is_deploy_query(buffer, count) && before_deploy_query() != 0) {
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
    if (is_deploy_query(buffer, count) && before_deploy_query() != 0) {
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
        && is_deploy_query_iov(message->msg_iov, message->msg_iovlen)
        && before_deploy_query() != 0
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
    if (iovcnt > 0 && is_deploy_query_iov(iov, (size_t)iovcnt)
        && before_deploy_query() != 0) {
        return -1;
    }
    return real_writev(fd, iov, iovcnt);
}
"""


def _compile_deploy_query_interposer(root: Path) -> Path:
    source = root / "deploy-query-interposer.c"
    library = root / "deploy-query-interposer.so"
    source.write_text(_DEPLOY_QUERY_INTERPOSER_SOURCE, encoding="utf-8")
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
        f"deploy-query interposer compilation failed: {completed.stderr!r}"
    )
    return library


def _spawn_interposed_deploy(
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
            "PARAEGOX_D0A_INTERPOSE_MODE": mode,
            "PARAEGOX_D0A_AUDIT_FD": str(audit_write),
            "PARAEGOX_D0A_RELEASE_FD": str(release_read),
        }
    )
    try:
        process = _spawn_deploy(
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
    assert expected in observed, f"DeployQuery interposer did not report {expected!r}: {observed!r}"
    return bytes(observed)


def _read_available_audit(audit_fd: int, observed: bytes) -> bytes:
    result = bytearray(observed)
    while True:
        readable, _, _ = select.select([audit_fd], [], [], 0.2)
        if not readable:
            return bytes(result)
        chunk = os.read(audit_fd, 64)
        if not chunk:
            return bytes(result)
        result.extend(chunk)


def test_init_deploy_repeat_and_concurrent_followers_use_one_real_owner() -> None:
    assert sys.platform.startswith("linux"), "D0a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()

    with tempfile.TemporaryDirectory(prefix="paraegox-d0a-golden-", dir="/tmp") as raw:
        root = Path(raw).resolve(strict=True)
        root.chmod(0o700)
        (root / "tmp").mkdir(mode=0o700)
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        binary = binary_directory / "paraegox"
        _copy_exact_binary(source_binary, binary)
        environment = _environment(root)
        workspace = root / "workspace"
        config_path = workspace / "paraegox.toml"
        state_root = workspace / "state"

        try:
            initialized = _invoke_init(binary, workspace, environment)
            assert initialized["command"] == "init"
            assert initialized["ok"] is True
            assert initialized["changed"] is True
            assert config_path.is_file()
            assert not state_root.exists()

            first = _spawn_deploy(binary, config_path, environment)
            second = _spawn_deploy(binary, config_path, environment)
            overlapped = first.poll() is None and second.poll() is None
            results = [
                _finish_deploy(
                    process,
                    config_path,
                    state_root,
                    expected_returncode=0,
                    expected_changed=_ANY_CHANGED,
                )
                for process in (first, second)
            ]
            assert sorted(result.envelope["changed"] for result in results) == [False, True]
            assert overlapped, "both fresh deploy clients must overlap as real processes"
            assert len({_deployment_identity(result.envelope) for result in results}) == 1

            ready_processes, _ = _assert_single_owner_graph(binary)
            repeated = _invoke_deploy(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=0,
                expected_changed=False,
            )
            assert _deployment_identity(repeated.envelope) == _deployment_identity(
                results[0].envelope
            )
            assert _matching_processes(binary) == ready_processes
            generation = repeated.envelope["generation"]
            assert isinstance(generation, str)
            private_projection = _assert_private_projection_matches_public(
                _raw_deploy_query(
                    state_root,
                    config_path.read_text(encoding="utf-8"),
                    generation,
                ),
                repeated.envelope,
            )
            leader = next(result for result in results if result.envelope["changed"] is True)
            assert leader.envelope["changed"] is (
                True and not private_projection["model_agent_replayed"]
            )

            down = _invoke_lifecycle(binary, "down", config_path, environment)
            assert down["ok"] is True and down["state"] == "stopped"
            _wait_for_no_matching_processes(binary)
            assert _socket_paths(root) == []
        finally:
            if config_path.exists():
                try:
                    _invoke_lifecycle(binary, "down", config_path, environment)
                except (AssertionError, OSError, subprocess.SubprocessError):
                    pass
            _terminate_private_processes(binary)
            assert not _matching_processes(binary)
            _assert_secret_material_absent_from_files(
                root,
                exact_binary=binary,
                public_input_files=(config_path,),
            )


def test_deploy_query_fencing_drift_down_race_and_output_failure_are_fail_closed() -> None:
    assert sys.platform.startswith("linux"), "D0a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()

    with tempfile.TemporaryDirectory(prefix="paraegox-d0a-query-", dir="/tmp") as raw:
        root = Path(raw).resolve(strict=True)
        root.chmod(0o700)
        (root / "tmp").mkdir(mode=0o700)
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        binary = binary_directory / "paraegox"
        _copy_exact_binary(source_binary, binary)
        interposer = _compile_deploy_query_interposer(root)
        state_root = root / "state"
        config_path = root / "paraegox.toml"
        original_document = _chat_document(state_root, _reserve_loopback_port())
        _write_config(config_path, original_document)
        environment = _environment(root)

        try:
            # run_up accepts and starts a fresh generation, then the real client write to
            # DeployQuery is failed. The public result must preserve uncertainty instead
            # of reporting changed=false, while the lifecycle remains actually Running.
            failed_query, audit_fd, release_fd = _spawn_interposed_deploy(
                binary,
                config_path,
                environment,
                interposer,
                mode="fail",
            )
            try:
                marker = _wait_for_audit_marker(audit_fd, b"F")
                query_failure = _finish_deploy(
                    failed_query,
                    config_path,
                    state_root,
                    expected_returncode=1,
                    expected_changed=None,
                )
                assert query_failure.envelope["diagnostics"][0]["code"] == "PXLC-DEPLOY-QUERY"
                assert _read_available_audit(audit_fd, marker) == b"F", (
                    "one public deploy request retried its private DeployQuery"
                )
            finally:
                os.close(audit_fd)
                os.close(release_fd)

            running_after_query_failure = _invoke_lifecycle(
                binary,
                "status",
                config_path,
                environment,
            )
            assert running_after_query_failure["ok"] is True
            assert running_after_query_failure["state"] == "running"
            assert running_after_query_failure["owner_readiness_observed"] is True
            first_generation = running_after_query_failure["generation"]
            assert isinstance(first_generation, str)
            _assert_single_owner_graph(binary)

            first_down = _invoke_lifecycle(binary, "down", config_path, environment)
            assert first_down["state"] == "stopped"
            _wait_for_no_matching_processes(binary)

            up = _invoke_lifecycle(binary, "up", config_path, environment)
            assert up["ok"] is True and up["state"] == "running"
            assert up["changed"] is True
            generation = up["generation"]
            assert isinstance(generation, str) and generation != first_generation
            ready_processes, _ = _assert_single_owner_graph(binary)

            after_up = _invoke_deploy(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=0,
                expected_changed=False,
            )
            assert after_up.envelope["generation"] == generation
            current_projection = _raw_deploy_query(state_root, original_document, generation)
            _assert_private_projection_matches_public(current_projection, after_up.envelope)

            mismatched_generation = "11" * 16 if generation != "11" * 16 else "22" * 16
            assert _raw_deploy_query(
                state_root,
                original_document,
                mismatched_generation,
            ) == b"", "a mismatched expected generation obtained a DeployQuery projection"
            assert _matching_processes(binary) == ready_processes
            after_mismatch = _invoke_deploy(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=0,
                expected_changed=False,
            )
            assert _deployment_identity(after_mismatch.envelope) == _deployment_identity(
                after_up.envelope
            )

            drift_document = _chat_document(state_root, _reserve_loopback_port())
            assert drift_document != original_document
            _write_config(config_path, drift_document)
            drift = _invoke_deploy(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=2,
                expected_changed=False,
            )
            assert drift.envelope["diagnostics"][0]["code"] == (
                "PXLC-LIFECYCLE-CONFIGURATION"
            )
            assert _matching_processes(binary) == ready_processes
            _write_config(config_path, original_document)

            with Path("/dev/full").open("wb", buffering=0) as unwritable_stdout:
                output_failure = _spawn_deploy(
                    binary,
                    config_path,
                    environment,
                    stdout=unwritable_stdout.fileno(),
                )
                try:
                    stdout, stderr = output_failure.communicate(
                        timeout=_COMMAND_TIMEOUT_SECONDS
                    )
                except subprocess.TimeoutExpired as error:
                    output_failure.kill()
                    output_failure.communicate()
                    raise AssertionError("deploy output-failure case timed out") from error
            assert stdout is None
            assert output_failure.returncode == 1
            assert stderr == b""
            assert _matching_processes(binary) == ready_processes, (
                "stdout failure changed the already-running owner graph"
            )

            raced, race_audit_fd, race_release_fd = _spawn_interposed_deploy(
                binary,
                config_path,
                environment,
                interposer,
                mode="barrier",
            )
            try:
                race_marker = _wait_for_audit_marker(race_audit_fd, b"B")
                raced_down = _invoke_lifecycle(binary, "down", config_path, environment)
                assert raced_down["ok"] is True and raced_down["state"] == "stopped"
                os.write(race_release_fd, b"R")
                os.close(race_release_fd)
                race_release_fd = -1
                raced_result = _finish_deploy(
                    raced,
                    config_path,
                    state_root,
                    expected_returncode=1,
                    expected_changed=False,
                )
                assert raced_result.envelope["diagnostics"][0]["code"] == "PXLC-DEPLOY-QUERY"
                assert _read_available_audit(race_audit_fd, race_marker) == b"B", (
                    "down-raced DeployQuery was retried"
                )
            finally:
                os.close(race_audit_fd)
                if race_release_fd >= 0:
                    os.close(race_release_fd)
            _wait_for_no_matching_processes(binary)

            lifecycle_record = state_root / _LIFECYCLE_RECORD_RELATIVE
            lifecycle_record.write_bytes(b'{"schema_version":1}\n')
            lifecycle_record.chmod(0o600)
            tampered = _invoke_deploy(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=1,
                expected_changed=None,
            )
            assert tampered.envelope["diagnostics"][0]["code"] == "PXLC-DEPLOY-LIFECYCLE"
            assert not _matching_processes(binary)
        finally:
            if config_path.exists():
                _write_config(config_path, original_document)
                try:
                    _invoke_lifecycle(binary, "down", config_path, environment)
                except (AssertionError, OSError, subprocess.SubprocessError):
                    pass
            _terminate_private_processes(binary)
            assert not _matching_processes(binary)
            _assert_secret_material_absent_from_files(
                root,
                exact_binary=binary,
                public_input_files=(config_path,),
            )


def test_unsupported_profile_and_crashed_supervisor_never_fabricate_deployment() -> None:
    assert sys.platform.startswith("linux"), "D0a process evidence runs on Ubuntu"
    assert os.geteuid() != 0 and os.getegid() != 0
    source_binary = _require_exact_binary()

    with tempfile.TemporaryDirectory(prefix="paraegox-d0a-failure-", dir="/tmp") as raw:
        root = Path(raw).resolve(strict=True)
        root.chmod(0o700)
        (root / "tmp").mkdir(mode=0o700)
        binary_directory = root / "bin"
        binary_directory.mkdir(mode=0o700)
        binary = binary_directory / "paraegox"
        _copy_exact_binary(source_binary, binary)
        environment = _environment(root)
        provisioned_state = root / "provisioned-state"
        provisioned_config = root / "provisioned.toml"
        _write_config(
            provisioned_config,
            _provisioned_chat_document(provisioned_state, _reserve_loopback_port()),
        )
        state_root = root / "state"
        config_path = root / "paraegox.toml"
        document = _chat_document(state_root, _reserve_loopback_port())
        _write_config(config_path, document)

        try:
            unsupported = _invoke_deploy(
                binary,
                provisioned_config,
                provisioned_state,
                environment,
                expected_returncode=2,
                expected_changed=False,
            )
            assert unsupported.envelope["diagnostics"][0]["code"] == (
                "PXLC-DEPLOY-PROFILE-UNSUPPORTED"
            )
            assert not provisioned_state.exists(), (
                "unsupported profile reached lifecycle or domain state"
            )
            assert not _matching_processes(binary)
            assert _socket_paths(root) == []

            up = _invoke_lifecycle(binary, "up", config_path, environment)
            assert up["ok"] is True and up["state"] == "running"
            _, supervisor = _assert_single_owner_graph(binary)
            os.kill(supervisor, signal.SIGKILL)
            deadline = time.monotonic() + 10.0
            while time.monotonic() < deadline and Path("/proc", str(supervisor)).exists():
                time.sleep(0.05)
            assert not Path("/proc", str(supervisor)).exists()

            crashed = _invoke_deploy(
                binary,
                config_path,
                state_root,
                environment,
                expected_returncode=1,
                expected_changed=None,
            )
            assert crashed.envelope["diagnostics"][0]["code"] == "PXLC-DEPLOY-LIFECYCLE"
            assert crashed.envelope["terminal_outcome"] is None
        finally:
            # SIGKILL orphan recovery remains outside D0a. Reclaim only this copied
            # exact-binary fixture so the test cannot leak a child into later jobs.
            _terminate_private_processes(binary)
            assert not _matching_processes(binary)
            _assert_secret_material_absent_from_files(
                root,
                exact_binary=binary,
                public_input_files=(config_path, provisioned_config),
            )
