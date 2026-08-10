from __future__ import annotations

import errno
import fcntl
import hashlib
import json
import os
import re
import select
import shlex
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager, suppress
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

import paraegox_sdk.console_client as console_client

_BINARY_ENVIRONMENT = "PARAEGOX_M5A_TUI_CLI_BINARY"
_COMMAND_TIMEOUT_SECONDS = 180.0
_TUI_START_TIMEOUT_SECONDS = 45.0
_TUI_REPLY_TIMEOUT_SECONDS = 45.0
_TUI_EXIT_TIMEOUT_SECONDS = 12.0
_OWNER_CLEANUP_TIMEOUT_SECONDS = 30.0
_MAX_CAPTURE_BYTES = 2 * 1024 * 1024
_HIDDEN_SUPERVISOR_MODE = b"__local-chat-supervisor-v1"
_TUI_HELP_LINE = "       paraegox tui --config <absolute-paraegox.toml>"
_TUI_ATTACH_HEADER_BYTES = 288
_TUI_ATTACH_MAX_FRAME_BYTES = 8_480
_TEXTUAL_TERMINAL_RESTORE = b"\x1b[?1049l"
_OPENAI_SENTINEL = "m5a-openai-secret-value-must-not-leak"
_DEEPSEEK_SENTINEL = "m5a-deepseek-secret-value-must-not-leak"
_ENVIRONMENT_CANARY = "m5a-unlisted-environment-must-not-reach-child"
_GENERATION_PATTERN = re.compile(r"[0-9a-f]{32}")
_ANSI_ESCAPE = re.compile(rb"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))")


pytestmark = pytest.mark.skipif(  # GOV-WAIVER-0015
    sys.platform != "linux",
    reason=(
        "GOV-WAIVER-0015: the exact M5a harness requires Linux /proc, "
        "Unix peer credentials, hard-linked Unix-socket pins, and PTY "
        "controlling-terminal semantics"
    ),
)


@dataclass(frozen=True)
class TuiDiagnostic:
    returncode: int
    stdout: bytes
    stderr: bytes


@dataclass
class PtyInvocation:
    process: subprocess.Popen[bytes]
    master_fd: int
    observer_fd: int
    original_termios: list[Any]
    capture: bytearray


@dataclass(frozen=True)
class LauncherLayout:
    binary: Path
    mode_file: Path
    marker_file: Path
    release_file: Path


def _require_exact_binary() -> Path:
    configured = os.environ.get(_BINARY_ENVIRONMENT)
    assert configured is not None, (
        f"{_BINARY_ENVIRONMENT} must name the already-built binary from the exact "
        "source revision under validation"
    )
    path = Path(configured)
    assert path.is_absolute(), f"{_BINARY_ENVIRONMENT} must be absolute"
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode), "the exact M5a binary must be a regular file"
    assert not path.is_symlink(), "the exact M5a binary must not be a symlink"
    assert metadata.st_mode & 0o111 != 0, "the exact M5a binary must be executable"
    return path.resolve(strict=True)


def _require_console_program() -> Path:
    configured = shutil.which("paraegox-console")
    assert configured is not None, (
        "the locked Python environment must expose the internal paraegox-console entrypoint"
    )
    path = Path(configured).resolve(strict=True)
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_mode & 0o111 != 0
    return path


def _require_passwordless_sudo() -> Path:
    sudo = shutil.which("sudo")
    if sudo is None:
        pytest.skip(
            "GOV-WAIVER-0015: wrong-peer and root-execution evidence require the "
            "admitted Ubuntu passwordless-sudo profile"
        )
    result = subprocess.run(
        [sudo, "-n", "true"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=5.0,
        check=False,
    )
    if result.returncode != 0:
        pytest.skip(
            "GOV-WAIVER-0015: wrong-peer and root-execution evidence require the "
            "admitted Ubuntu passwordless-sudo profile"
        )
    return Path(sudo).resolve(strict=True)


def _copy_exact_binary(source: Path, target: Path) -> None:
    shutil.copyfile(source, target)
    target.chmod(0o755)
    source_digest = hashlib.sha256(source.read_bytes()).digest()
    target_digest = hashlib.sha256(target.read_bytes()).digest()
    assert target_digest == source_digest
    metadata = target.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not target.is_symlink()
    assert metadata.st_nlink == 1
    assert stat.S_IMODE(metadata.st_mode) == 0o755


def _environment(root: Path, console: Path) -> dict[str, str]:
    temporary = root / "tmp"
    temporary.mkdir(mode=0o700, exist_ok=True)
    return {
        "HOME": os.fspath(root),
        "TMPDIR": os.fspath(temporary),
        "PATH": os.pathsep.join((os.fspath(console.parent), "/usr/bin", "/bin")),
        "TERM": "xterm-256color",
        "LANG": "C.UTF-8",
        "OPENAI_API_KEY": _OPENAI_SENTINEL,
        "DEEPSEEK_API_KEY": _DEEPSEEK_SENTINEL,
        "PARAEGOX_M5A_UNLISTED_CANARY": _ENVIRONMENT_CANARY,
    }


def _decode_json_line(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n") and raw.count(b"\n") == 1
    value = json.loads(raw)
    assert isinstance(value, dict)
    assert raw == json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    return value


def _invoke_init(binary: Path, workspace: Path, environment: dict[str, str]) -> Path:
    process = subprocess.run(
        [os.fspath(binary), "init", "--directory", os.fspath(workspace), "--json"],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
        check=False,
    )
    assert process.returncode == 0, (process.stdout, process.stderr)
    assert process.stderr == b""
    envelope = _decode_json_line(process.stdout)
    assert envelope["ok"] is True
    assert envelope["profile"] == "deterministic-echo-v1"
    config = workspace / "paraegox.toml"
    assert config.is_file() and stat.S_IMODE(config.stat().st_mode) == 0o600
    assert not (workspace / "state").exists()
    return config


def _invoke_lifecycle(
    binary: Path,
    command: str,
    config: Path,
    environment: dict[str, str],
    *,
    expected_returncode: int = 0,
) -> dict[str, Any]:
    process = subprocess.run(
        [os.fspath(binary), command, "--config", os.fspath(config), "--json"],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=_COMMAND_TIMEOUT_SECONDS,
        check=False,
    )
    assert process.returncode == expected_returncode, (
        command,
        process.returncode,
        process.stdout,
        process.stderr,
    )
    assert process.stderr == b""
    return _decode_json_line(process.stdout)


def _invoke_tui_without_pty(
    binary: Path,
    arguments: list[str],
    environment: dict[str, str],
) -> TuiDiagnostic:
    process = subprocess.run(
        [os.fspath(binary), *arguments],
        cwd=binary.parent,
        env=environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
        check=False,
    )
    return TuiDiagnostic(process.returncode, process.stdout, process.stderr)


def _one_public_diagnostic(result: TuiDiagnostic, code: str) -> None:
    assert result.stdout == b""
    expected_prefix = f"paraegox: code={code} message=".encode()
    assert result.stderr.startswith(expected_prefix), result.stderr
    assert result.stderr.endswith(b"\n")
    assert result.stderr.count(b"\n") == 1
    assert result.stderr[len(expected_prefix) : -1]
    for forbidden in (
        _OPENAI_SENTINEL.encode(),
        _DEEPSEEK_SENTINEL.encode(),
        _ENVIRONMENT_CANARY.encode(),
        b"Traceback",
    ):
        assert forbidden not in result.stderr


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


def _process_arguments(process_id: int) -> tuple[bytes, ...]:
    raw = (Path("/proc") / str(process_id) / "cmdline").read_bytes()
    return tuple(value for value in raw.split(b"\0") if value)


def _owner_processes(binary: Path) -> set[int]:
    processes = _matching_processes(binary)
    supervisors = {
        process_id
        for process_id in processes
        if _HIDDEN_SUPERVISOR_MODE in _process_arguments(process_id)
    }
    assert len(supervisors) == 1, "one Running generation must retain one supervisor"
    supervisor = next(iter(supervisors))
    assert os.getsid(supervisor) == supervisor
    owners = {process_id for process_id in processes if os.getsid(process_id) == supervisor}
    assert len(owners) >= 2, "the real owner graph must include the supervisor and Node child"
    return owners


def _wait_for_no_matching_processes(binary: Path) -> None:
    deadline = time.monotonic() + _OWNER_CLEANUP_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if not _matching_processes(binary):
            return
        time.sleep(0.05)
    assert not _matching_processes(binary), "joined down left an exact-binary process alive"


def _reserve_loopback_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def _assign_unique_fabric_listener(config: Path) -> None:
    before = config.lstat()
    assert stat.S_ISREG(before.st_mode)
    assert not config.is_symlink()
    assert before.st_uid == os.geteuid() and before.st_gid == os.getegid()
    assert before.st_nlink == 1 and stat.S_IMODE(before.st_mode) == 0o600
    document = config.read_text(encoding="utf-8")
    default = 'fabric_listen = "tcp/127.0.0.1:7447"\n'
    assert document.count(default) == 1
    replacement = f'fabric_listen = "tcp/127.0.0.1:{_reserve_loopback_port()}"\n'
    config.write_text(document.replace(default, replacement), encoding="utf-8")
    after = config.lstat()
    assert (after.st_dev, after.st_ino) == (before.st_dev, before.st_ino)
    assert after.st_uid == before.st_uid and after.st_gid == before.st_gid
    assert after.st_nlink == 1 and stat.S_IMODE(after.st_mode) == 0o600


def _terminate_test_processes(binary: Path) -> None:
    for requested_signal in (signal.SIGTERM, signal.SIGKILL):
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            matches = _matching_processes(binary)
            if not matches:
                return
            for process_id in matches:
                with suppress(ProcessLookupError):
                    os.kill(process_id, requested_signal)
            time.sleep(0.05)


def _set_controlling_terminal() -> None:
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


def _spawn_tui_pty(
    binary: Path,
    config: Path,
    environment: dict[str, str],
) -> PtyInvocation:
    master_fd, slave_fd = os.openpty()
    fcntl.ioctl(slave_fd, termios.TIOCSWINSZ, struct.pack("HHHH", 34, 120, 0, 0))
    observer_fd = os.dup(slave_fd)
    original_termios = termios.tcgetattr(observer_fd)
    process = subprocess.Popen(
        [os.fspath(binary), "tui", "--config", os.fspath(config)],
        cwd=binary.parent,
        env=environment,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        preexec_fn=_set_controlling_terminal,
    )
    os.close(slave_fd)
    return PtyInvocation(process, master_fd, observer_fd, original_termios, bytearray())


def _bounded_append(capture: bytearray, chunk: bytes) -> None:
    capture.extend(chunk)
    if len(capture) > _MAX_CAPTURE_BYTES:
        del capture[: len(capture) - _MAX_CAPTURE_BYTES]


def _read_pty_once(invocation: PtyInvocation, timeout: float) -> bool:
    ready, _, _ = select.select([invocation.master_fd], [], [], timeout)
    if not ready:
        return False
    try:
        chunk = os.read(invocation.master_fd, 65_536)
    except OSError as error:
        if error.errno == errno.EIO:
            return False
        raise
    if chunk:
        _bounded_append(invocation.capture, chunk)
        return True
    return False


def _read_until(invocation: PtyInvocation, marker: bytes, timeout_seconds: float) -> None:
    deadline = time.monotonic() + timeout_seconds
    while marker not in invocation.capture:
        if time.monotonic() >= deadline:
            raise TimeoutError(
                f"timed out waiting for {marker!r}; "
                f"tail={_safe_terminal_tail(invocation.capture)!r}"
            )
        _read_pty_once(invocation, 0.2)
        if invocation.process.poll() is not None and marker not in invocation.capture:
            raise AssertionError(
                f"TUI exited {invocation.process.returncode} before {marker!r}; "
                f"tail={_safe_terminal_tail(invocation.capture)!r}"
            )


def _read_until_plain_line(
    invocation: PtyInvocation,
    line: bytes,
    timeout_seconds: float,
) -> int:
    deadline = time.monotonic() + timeout_seconds
    while True:
        plain = _ANSI_ESCAPE.sub(b"", invocation.capture).replace(b"\r", b"")
        position = plain.find(line)
        if position >= 0:
            return position
        if time.monotonic() >= deadline:
            raise TimeoutError(
                f"timed out waiting for terminal line {line!r}; "
                f"tail={_safe_terminal_tail(invocation.capture)!r}"
            )
        _read_pty_once(invocation, 0.2)
        if invocation.process.poll() is not None:
            raise AssertionError(
                f"TUI exited {invocation.process.returncode} before {line!r}; "
                f"tail={_safe_terminal_tail(invocation.capture)!r}"
            )


def _wait_for_pty_exit(
    invocation: PtyInvocation,
    *,
    expected_returncode: int,
    timeout_seconds: float = _TUI_EXIT_TIMEOUT_SECONDS,
) -> bytes:
    deadline = time.monotonic() + timeout_seconds
    while invocation.process.poll() is None:
        if time.monotonic() >= deadline:
            raise TimeoutError(
                f"TUI did not exit; tail={_safe_terminal_tail(invocation.capture)!r}"
            )
        _read_pty_once(invocation, 0.2)
    while _read_pty_once(invocation, 0):
        pass
    assert invocation.process.returncode == expected_returncode, (
        invocation.process.returncode,
        _safe_terminal_tail(invocation.capture),
    )
    assert termios.tcgetattr(invocation.observer_fd) == invocation.original_termios
    return bytes(invocation.capture)


def _close_pty(invocation: PtyInvocation) -> None:
    if invocation.process.poll() is None:
        with suppress(ProcessLookupError):
            os.killpg(invocation.process.pid, signal.SIGTERM)
        try:
            invocation.process.wait(timeout=3.0)
        except subprocess.TimeoutExpired:
            with suppress(ProcessLookupError):
                os.killpg(invocation.process.pid, signal.SIGKILL)
            invocation.process.wait(timeout=3.0)
    os.close(invocation.master_fd)
    os.close(invocation.observer_fd)


def _safe_terminal_tail(capture: bytearray | bytes) -> str:
    text = bytes(capture[-8_192:]).decode("utf-8", errors="replace")
    return "".join(value for value in text if value in "\n\r\t" or value >= " ")


def _assert_capture_redacted(capture: bytes, config: Path, state_root: Path) -> None:
    for forbidden in (
        os.fsencode(config),
        os.fsencode(state_root),
        _OPENAI_SENTINEL.encode(),
        _DEEPSEEK_SENTINEL.encode(),
        _ENVIRONMENT_CANARY.encode(),
        b"OPENAI_API_KEY",
        b"DEEPSEEK_API_KEY",
        b"SecretRef",
        b"Traceback",
    ):
        assert forbidden not in capture


def _assert_single_pty_diagnostic(capture: bytes, code: str) -> None:
    plain = _ANSI_ESCAPE.sub(b"", capture).replace(b"\r", b"")
    marker = b"paraegox: code="
    assert plain.count(marker) == 1, _safe_terminal_tail(capture)
    start = plain.index(marker)
    end = plain.find(b"\n", start)
    assert end >= 0, _safe_terminal_tail(capture)
    diagnostic = plain[start:end]
    expected_prefix = f"paraegox: code={code} message=".encode()
    assert diagnostic.startswith(expected_prefix)
    assert diagnostic[len(expected_prefix) :]
    assert b"Traceback" not in capture


def _write_mode(layout: LauncherLayout, mode: str) -> None:
    layout.mode_file.write_text(mode + "\n", encoding="utf-8")


def _install_launcher(
    exact_binary: Path,
    root: Path,
    real_console: Path,
) -> LauncherLayout:
    binary_directory = root / "bin"
    binary_directory.mkdir(mode=0o700)
    binary = binary_directory / "paraegox"
    _copy_exact_binary(exact_binary, binary)
    mode_file = root / "launcher.mode"
    marker_file = root / "launcher.marker"
    release_file = root / "launcher.release"
    test_file = Path(__file__).resolve(strict=True)
    launcher = binary_directory / "paraegox-console"
    command = " ".join(
        shlex.quote(value)
        for value in (
            # Keep the active virtual-environment interpreter identity. Resolving
            # this symlink selects uv's base interpreter and loses the installed
            # pytest/ParaEGOX environment before the private fault runs.
            os.fspath(Path(sys.executable).absolute()),
            os.fspath(test_file),
            "--m5a-launcher",
            os.fspath(mode_file),
            os.fspath(marker_file),
            os.fspath(release_file),
            os.fspath(real_console),
        )
    )
    launcher.write_text(f'#!/bin/sh\nexec {command} "$@"\n', encoding="utf-8")
    launcher.chmod(0o700)
    layout = LauncherLayout(binary, mode_file, marker_file, release_file)
    _write_mode(layout, "passthrough")
    return layout


def _wait_for_file(path: Path, timeout_seconds: float = 10.0) -> bytes:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            content = path.read_bytes()
        except FileNotFoundError:
            content = b""
        if content:
            return content
        time.sleep(0.02)
    raise TimeoutError(f"timed out waiting for harness marker {path.name}")


def _clear_launcher_files(layout: LauncherLayout) -> None:
    for path in (layout.marker_file, layout.release_file):
        with suppress(FileNotFoundError):
            path.unlink()


@contextmanager
def _running_owner(
    binary: Path,
    environment: dict[str, str],
    root: Path,
) -> Iterator[tuple[Path, Path, str, set[int]]]:
    _wait_for_no_matching_processes(binary)
    workspace = root / "workspace"
    config = _invoke_init(binary, workspace, environment)
    _assign_unique_fabric_listener(config)
    state_root = workspace / "state"
    try:
        up = _invoke_lifecycle(binary, "up", config, environment)
        assert up["ok"] is True and up["state"] == "running"
        generation = up["generation"]
        assert isinstance(generation, str) and _GENERATION_PATTERN.fullmatch(generation)
        owners = _owner_processes(binary)
    except BaseException:
        if _matching_processes(binary):
            with suppress(Exception):
                _invoke_lifecycle(binary, "down", config, environment)
        _terminate_test_processes(binary)
        raise
    try:
        yield config, state_root, generation, owners
    finally:
        if _matching_processes(binary):
            with suppress(Exception):
                _invoke_lifecycle(binary, "down", config, environment)
        _terminate_test_processes(binary)


def test_m5a_exact_grammar_help_and_never_started_are_pre_effect() -> None:
    binary = _require_exact_binary()
    console = _require_console_program()
    with tempfile.TemporaryDirectory(prefix="px-m5a-grammar-") as temporary:
        root = Path(temporary).resolve(strict=True)
        environment = _environment(root, console)
        workspace = root / "workspace"
        config = _invoke_init(binary, workspace, environment)
        state_root = workspace / "state"

        help_result = subprocess.run(
            [os.fspath(binary), "--help"],
            cwd=binary.parent,
            env=environment,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=10.0,
            check=False,
        )
        assert help_result.returncode == 0 and help_result.stderr == b""
        help_lines = help_result.stdout.decode().splitlines()
        assert help_lines.count(_TUI_HELP_LINE) == 1

        malformed = (
            (["tui"], "PXLC-TUI-GRAMMAR"),
            (["tui", "--help"], "PXLC-TUI-GRAMMAR"),
            (["tui", "--config", os.fspath(config), "--json"], "PXLC-TUI-GRAMMAR"),
            (["tui", "--config", os.fspath(config), "--retry"], "PXLC-TUI-GRAMMAR"),
            (["tui", "--config", "paraegox.toml"], "PXLC-CONFIG-PATH-INVALID"),
            (["tui", "--config", os.fspath(root / "missing.toml")], "PXLC-CONFIG-FILE-READ"),
        )
        before = frozenset(path.relative_to(root) for path in root.rglob("*"))
        for arguments, expected_code in malformed:
            result = _invoke_tui_without_pty(binary, list(arguments), environment)
            assert result.returncode == 2
            _one_public_diagnostic(result, expected_code)
        assert frozenset(path.relative_to(root) for path in root.rglob("*")) == before
        assert not state_root.exists()

        never_started = _invoke_tui_without_pty(
            binary,
            ["tui", "--config", os.fspath(config)],
            environment,
        )
        assert never_started.returncode == 1
        _one_public_diagnostic(never_started, "PXLC-TUI-NOT-RUNNING")
        assert not state_root.exists()


def test_m5a_real_pty_echo_watch_detach_and_signals_preserve_owner() -> None:
    exact_binary = _require_exact_binary()
    console = _require_console_program()
    with tempfile.TemporaryDirectory(prefix="px-m5a-live-") as temporary:
        root = Path(temporary).resolve(strict=True)
        binary = root / "paraegox"
        _copy_exact_binary(exact_binary, binary)
        environment = _environment(root, console)
        with _running_owner(binary, environment, root) as (
            config,
            state_root,
            generation,
            owners,
        ):
            invocation = _spawn_tui_pty(binary, config, environment)
            try:
                _read_until(invocation, b"System: connected", _TUI_START_TIMEOUT_SECONDS)
                initial_position = _read_until_plain_line(
                    invocation,
                    b"Inspection cache UNKNOWN r1 | NodeDaemon unknown",
                    _TUI_START_TIMEOUT_SECONDS,
                )
                stale_position = _read_until_plain_line(
                    invocation,
                    b"Inspection cache UNKNOWN r2 | NodeDaemon stale",
                    _TUI_START_TIMEOUT_SECONDS,
                )
                assert stale_position > initial_position
                os.write(invocation.master_fd, b"m5a-exact-binary-echo\r")
                _read_until(
                    invocation,
                    b"echo: m5a-exact-binary-echo",
                    _TUI_REPLY_TIMEOUT_SECONDS,
                )
                os.write(invocation.master_fd, b"\x1b")
                capture = _wait_for_pty_exit(invocation, expected_returncode=0)
                assert _TEXTUAL_TERMINAL_RESTORE in capture
                _assert_capture_redacted(capture, config, state_root)
            finally:
                _close_pty(invocation)

            status = _invoke_lifecycle(binary, "status", config, environment)
            assert status["state"] == "running" and status["generation"] == generation
            assert _owner_processes(binary) == owners

            for requested_signal in (signal.SIGINT, signal.SIGTERM):
                signaled = _spawn_tui_pty(binary, config, environment)
                try:
                    _read_until(signaled, b"System: connected", _TUI_START_TIMEOUT_SECONDS)
                    os.kill(signaled.process.pid, requested_signal)
                    capture = _wait_for_pty_exit(signaled, expected_returncode=1)
                    _assert_single_pty_diagnostic(capture, "PXLC-TUI-CHILD")
                    _assert_capture_redacted(capture, config, state_root)
                finally:
                    _close_pty(signaled)
                status = _invoke_lifecycle(binary, "status", config, environment)
                assert status["state"] == "running" and status["generation"] == generation
                assert _owner_processes(binary) == owners

        _wait_for_no_matching_processes(binary)
        assert not any(path.is_socket() for path in state_root.rglob("*") if path.exists())


def test_m5a_private_20_21_23_24_child_and_generation_race_are_single_line() -> None:
    exact_binary = _require_exact_binary()
    real_console = _require_console_program()
    with tempfile.TemporaryDirectory(prefix="px-m5a-fault-") as temporary:
        root = Path(temporary).resolve(strict=True)
        layout = _install_launcher(exact_binary, root, real_console)
        environment = _environment(root, real_console)
        with _running_owner(layout.binary, environment, root) as (
            config,
            state_root,
            generation,
            owners,
        ):
            scenarios = (
                ("partial-handoff", "PXLC-TUI-HANDOFF", 8.0),
                ("replace-bootstrap", "PXLC-TUI-BOOTSTRAP", 8.0),
                ("inspection-not-found", "PXLC-TUI-PROTOCOL", 8.0),
                ("handoff-eof-timeout", "PXLC-TUI-IO", 8.0),
                ("child-25", "PXLC-TUI-CHILD", 8.0),
            )
            for mode, code, timeout_seconds in scenarios:
                _clear_launcher_files(layout)
                _write_mode(layout, mode)
                invocation = _spawn_tui_pty(layout.binary, config, environment)
                try:
                    assert _wait_for_file(layout.marker_file) == b"handoff-read"
                    capture = _wait_for_pty_exit(
                        invocation,
                        expected_returncode=1,
                        timeout_seconds=timeout_seconds,
                    )
                    _assert_single_pty_diagnostic(capture, code)
                    _assert_capture_redacted(capture, config, state_root)
                finally:
                    _close_pty(invocation)
                assert _owner_processes(layout.binary) == owners

            _clear_launcher_files(layout)
            _write_mode(layout, "hang-ignore-signals")
            hanging = _spawn_tui_pty(layout.binary, config, environment)
            try:
                child_pid = int(_wait_for_file(layout.marker_file).decode())
                assert Path("/proc", str(child_pid)).exists()
                started = time.monotonic()
                os.kill(hanging.process.pid, signal.SIGTERM)
                capture = _wait_for_pty_exit(
                    hanging,
                    expected_returncode=1,
                    timeout_seconds=8.0,
                )
                elapsed = time.monotonic() - started
                assert 4.5 <= elapsed < 8.0
                assert not Path("/proc", str(child_pid)).exists()
                _assert_single_pty_diagnostic(capture, "PXLC-TUI-CHILD")
            finally:
                _close_pty(hanging)
            assert _owner_processes(layout.binary) == owners

            _clear_launcher_files(layout)
            _write_mode(layout, "pause-before-console")
            old_attach = _spawn_tui_pty(layout.binary, config, environment)
            try:
                _wait_for_file(layout.marker_file)
                down = _invoke_lifecycle(layout.binary, "down", config, environment)
                assert down["state"] == "stopped"
                replacement = _invoke_lifecycle(layout.binary, "up", config, environment)
                new_generation = replacement["generation"]
                assert new_generation != generation
                layout.release_file.write_text("release\n", encoding="utf-8")
                capture = _wait_for_pty_exit(old_attach, expected_returncode=1)
                _assert_single_pty_diagnostic(capture, "PXLC-TUI-BOOTSTRAP")
            finally:
                _close_pty(old_attach)

            _clear_launcher_files(layout)
            _write_mode(layout, "passthrough")
            new_attach = _spawn_tui_pty(layout.binary, config, environment)
            try:
                _read_until(new_attach, b"System: connected", _TUI_START_TIMEOUT_SECONDS)
                os.write(new_attach.master_fd, b"\x1b")
                _wait_for_pty_exit(new_attach, expected_returncode=0)
            finally:
                _close_pty(new_attach)
            status = _invoke_lifecycle(layout.binary, "status", config, environment)
            assert status["state"] == "running"
            assert status["generation"] == new_generation

        _wait_for_no_matching_processes(layout.binary)


def test_m5a_wrong_peer_private_22_and_root_execution_fail_closed() -> None:
    sudo = _require_passwordless_sudo()
    exact_binary = _require_exact_binary()
    real_console = _require_console_program()
    with tempfile.TemporaryDirectory(prefix="px-m5a-peer-") as temporary:
        root = Path(temporary).resolve(strict=True)
        layout = _install_launcher(exact_binary, root, real_console)
        environment = _environment(root, real_console)
        workspace = root / "workspace"
        config = _invoke_init(layout.binary, workspace, environment)
        _assign_unique_fabric_listener(config)
        state_root = workspace / "state"

        root_process = subprocess.run(
            [
                os.fspath(sudo),
                "-n",
                "env",
                "-i",
                f"PATH={environment['PATH']}",
                f"TERM={environment['TERM']}",
                os.fspath(layout.binary),
                "tui",
                "--config",
                os.fspath(config),
            ],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=15.0,
            check=False,
        )
        assert root_process.returncode == 1
        _one_public_diagnostic(
            TuiDiagnostic(
                root_process.returncode,
                root_process.stdout,
                root_process.stderr,
            ),
            "PXLC-EXECUTION-IDENTITY",
        )
        assert not state_root.exists()

        try:
            up = _invoke_lifecycle(layout.binary, "up", config, environment)
            generation = up["generation"]
            owners = _owner_processes(layout.binary)
            _clear_launcher_files(layout)
            _write_mode(layout, "inspection-wrong-peer")
            invocation = _spawn_tui_pty(layout.binary, config, environment)
            try:
                assert _wait_for_file(layout.marker_file) == b"handoff-read"
                capture = _wait_for_pty_exit(invocation, expected_returncode=1)
                _assert_single_pty_diagnostic(capture, "PXLC-TUI-PEER")
                _assert_capture_redacted(capture, config, state_root)
            finally:
                _close_pty(invocation)
            status = _invoke_lifecycle(layout.binary, "status", config, environment)
            assert status["state"] == "running" and status["generation"] == generation
            assert _owner_processes(layout.binary) == owners
            down = _invoke_lifecycle(layout.binary, "down", config, environment)
            assert down["state"] == "stopped"
        finally:
            _terminate_test_processes(layout.binary)
        _wait_for_no_matching_processes(layout.binary)


def _read_handoff_from_fd_three() -> bytes:
    stream = socket.socket(fileno=os.dup(3))
    stream.settimeout(6.0)
    chunks: list[bytes] = []
    try:
        while True:
            chunk = stream.recv(65_536)
            if not chunk:
                break
            chunks.append(chunk)
            if sum(map(len, chunks)) > _TUI_ATTACH_MAX_FRAME_BYTES:
                return b""
    finally:
        stream.close()
        with suppress(OSError):
            os.close(3)
    frame = b"".join(chunks)
    if not _TUI_ATTACH_HEADER_BYTES <= len(frame) <= _TUI_ATTACH_MAX_FRAME_BYTES:
        return b""
    return frame


def _handoff_paths(frame: bytes) -> tuple[Path, Path]:
    if frame[:4] != b"PXTH" or len(frame) < _TUI_ATTACH_HEADER_BYTES:
        raise RuntimeError("invalid handoff fixture")
    conversation_length = int.from_bytes(frame[68:72], "big")
    inspection_length = int.from_bytes(frame[164:168], "big")
    conversation_start = _TUI_ATTACH_HEADER_BYTES
    conversation_end = conversation_start + conversation_length
    inspection_end = conversation_end + inspection_length
    if inspection_end != len(frame):
        raise RuntimeError("invalid handoff fixture lengths")
    return (
        Path(os.fsdecode(frame[conversation_start:conversation_end])),
        Path(os.fsdecode(frame[conversation_end:inspection_end])),
    )


def _run_real_console(
    real_console: Path,
    frame: bytes,
    *,
    exact_eof: bool = True,
) -> int:
    writer, child_endpoint = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    writer_fd = os.dup(writer.fileno())
    writer.close()
    writer = socket.socket(fileno=writer_fd)
    os.dup2(child_endpoint.fileno(), 3)
    os.set_inheritable(3, True)
    child_endpoint.close()
    process = subprocess.Popen(
        [os.fspath(real_console), "--tui-attach-fd", "3"],
        pass_fds=(3,),
        close_fds=True,
    )
    os.close(3)
    try:
        writer.sendall(frame)
        if exact_eof:
            writer.shutdown(socket.SHUT_WR)
        return process.wait(timeout=15.0)
    finally:
        writer.close()
        if process.poll() is None:
            process.kill()
            process.wait(timeout=3.0)


@contextmanager
def _temporarily_replaced_file(path: Path) -> Iterator[None]:
    backup = path.with_name(path.name + ".m5a-owner-backup")
    original = path.read_bytes()
    path.rename(backup)
    path.write_bytes(original)
    path.chmod(0o600)
    try:
        yield
    finally:
        with suppress(FileNotFoundError):
            path.unlink()
        backup.rename(path)


@dataclass(frozen=True)
class InspectionEndpointReplacement:
    socket_path: Path
    pin_path: Path
    socket_backup: Path
    pin_backup: Path


@contextmanager
def _replace_inspection_endpoint(bootstrap_path: Path) -> Iterator[InspectionEndpointReplacement]:
    bootstrap = console_client._decode_inspection_bootstrap_v2(bootstrap_path.read_bytes())
    socket_path = Path(os.fsdecode(bootstrap.socket_path))
    pins = list(socket_path.parent.glob(".pxi-*-socket.pin"))
    if len(pins) != 1:
        raise RuntimeError("Inspection fixture must expose one canonical socket pin")
    pin_path = pins[0]
    socket_backup = socket_path.with_name(socket_path.name + ".m5a-owner-backup")
    pin_backup = pin_path.with_name(pin_path.name + ".m5a-owner-backup")
    socket_path.rename(socket_backup)
    pin_path.rename(pin_backup)
    replacement = InspectionEndpointReplacement(
        socket_path,
        pin_path,
        socket_backup,
        pin_backup,
    )
    try:
        yield replacement
    finally:
        for path in (pin_path, socket_path):
            with suppress(FileNotFoundError):
                path.unlink()
        pin_backup.rename(pin_path)
        socket_backup.rename(socket_path)


def _read_one_request(connection: socket.socket) -> bytes:
    connection.settimeout(5.0)
    chunks: list[bytes] = []
    while True:
        chunk = connection.recv(65_536)
        if not chunk:
            break
        chunks.append(chunk)
    authenticated = b"".join(chunks)
    if len(authenticated) < 33:
        raise RuntimeError("Inspection request was incomplete")
    return authenticated[32:]


def _not_found_response(request: bytes) -> bytes:
    frame = bytearray(144)
    frame[:4] = b"PXIP"
    frame[4:6] = (2).to_bytes(2, "big")
    frame[6:8] = (144).to_bytes(2, "big")
    frame[8:12] = (144).to_bytes(4, "big")
    frame[16] = int(console_client._InspectionResponseOutcomeV2.NOT_FOUND)
    frame[17] = request[12]
    frame[24:40] = request[16:32]
    frame[40:56] = request[32:48]
    frame[56:64] = request[48:56]
    frame[72:104] = request[64:96]
    frame[112:144] = console_client._canonical_digest(
        b"paraegox.inspection.protocol-response.v2",
        (bytes(frame[:112]), b""),
    )
    return bytes(frame)


def _serve_inspection_once(
    replacement: InspectionEndpointReplacement,
    *,
    outcome: str,
) -> tuple[socket.socket, threading.Thread]:
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(os.fspath(replacement.socket_path))
    replacement.socket_path.chmod(0o600)
    os.link(replacement.socket_path, replacement.pin_path)
    listener.listen(1)

    def serve() -> None:
        with listener:
            connection, _ = listener.accept()
            with connection:
                request = _read_one_request(connection)
                if outcome == "not-found":
                    response = _not_found_response(request)
                    connection.sendall(len(response).to_bytes(4, "big") + response)
                else:
                    time.sleep(6.0)

    thread = threading.Thread(target=serve, name="m5a-inspection-fixture", daemon=True)
    thread.start()
    return listener, thread


def _launcher_main(arguments: list[str]) -> int:
    if len(arguments) != 6 or arguments[4:] != ["--tui-attach-fd", "3"]:
        return 125
    mode_file, marker_file, release_file, real_console = map(Path, arguments[:4])
    frame = _read_handoff_from_fd_three()
    if not frame:
        return 125
    mode = mode_file.read_text(encoding="utf-8").strip()
    if mode in {
        "partial-handoff",
        "replace-bootstrap",
        "inspection-not-found",
        "handoff-eof-timeout",
        "child-25",
        "inspection-wrong-peer",
    }:
        marker_file.write_text("handoff-read", encoding="utf-8")
    if mode == "child-25":
        return 25
    if mode == "hang-ignore-signals":
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        marker_file.write_text(str(os.getpid()), encoding="utf-8")
        while True:
            signal.pause()
    if mode == "pause-before-console":
        marker_file.write_text("located", encoding="utf-8")
        deadline = time.monotonic() + 30.0
        while not release_file.exists():
            if time.monotonic() >= deadline:
                return 25
            time.sleep(0.02)
    if mode == "partial-handoff":
        return _run_real_console(real_console, frame[:-1])
    if mode == "handoff-eof-timeout":
        return _run_real_console(real_console, frame, exact_eof=False)

    conversation_bootstrap, inspection_bootstrap = _handoff_paths(frame)
    if mode == "replace-bootstrap":
        with _temporarily_replaced_file(conversation_bootstrap):
            return _run_real_console(real_console, frame)
    if mode in {"inspection-not-found", "inspection-timeout"}:
        with _replace_inspection_endpoint(inspection_bootstrap) as replacement:
            _, thread = _serve_inspection_once(
                replacement,
                outcome="not-found" if mode == "inspection-not-found" else "timeout",
            )
            result = _run_real_console(real_console, frame)
            thread.join(timeout=0.5)
            return result
    if mode == "inspection-wrong-peer":
        with _replace_inspection_endpoint(inspection_bootstrap) as replacement:
            sudo = Path(shutil.which("sudo") or "/usr/bin/sudo")
            root_marker = marker_file.with_name(marker_file.name + ".root")
            command = [
                os.fspath(sudo),
                "-n",
                # Preserve the active virtual-environment launcher. Resolving it
                # selects uv's base interpreter and loses the installed package.
                os.fspath(Path(sys.executable).absolute()),
                os.fspath(Path(__file__).resolve(strict=True)),
                "--m5a-root-peer",
                os.fspath(replacement.socket_path),
                os.fspath(replacement.pin_path),
                str(os.geteuid()),
                str(os.getegid()),
                os.fspath(root_marker),
            ]
            server = subprocess.Popen(
                command,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            try:
                _wait_for_file(root_marker, 5.0)
                result = _run_real_console(real_console, frame)
                server.wait(timeout=5.0)
                return result
            finally:
                with suppress(FileNotFoundError):
                    root_marker.unlink()
                if server.poll() is None:
                    server.terminate()
                    with suppress(subprocess.TimeoutExpired):
                        server.wait(timeout=2.0)
    return _run_real_console(real_console, frame)


def _root_peer_main(arguments: list[str]) -> int:
    if len(arguments) != 5 or os.geteuid() != 0:
        return 125
    socket_path = Path(arguments[0])
    pin_path = Path(arguments[1])
    uid = int(arguments[2])
    gid = int(arguments[3])
    marker = Path(arguments[4])
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        listener.bind(os.fspath(socket_path))
        os.chown(socket_path, uid, gid)
        socket_path.chmod(0o600)
        os.link(socket_path, pin_path)
        listener.listen(1)
        marker.write_text(str(os.getpid()), encoding="utf-8")
        listener.settimeout(8.0)
        connection, _ = listener.accept()
        with connection:
            connection.settimeout(2.0)
            with suppress(TimeoutError, OSError):
                while connection.recv(4096):
                    pass
        return 0
    finally:
        listener.close()


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "--m5a-launcher":
        raise SystemExit(_launcher_main(sys.argv[2:]))
    if len(sys.argv) >= 2 and sys.argv[1] == "--m5a-root-peer":
        raise SystemExit(_root_peer_main(sys.argv[2:]))
    raise SystemExit(125)
