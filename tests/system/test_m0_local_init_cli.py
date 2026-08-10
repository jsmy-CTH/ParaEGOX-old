from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

_BINARY_ENVIRONMENT = "PARAEGOX_M0_INIT_CLI_BINARY"
_INIT_FIELDS = {
    "schema_version",
    "command",
    "ok",
    "changed",
    "profile",
    "config_relative_path",
    "state_relative_path",
    "diagnostics",
}
_PROFILE = "deterministic-echo-v1"
_CONFIG_RELATIVE_PATH = "paraegox.toml"
_STATE_RELATIVE_PATH = "state"
_SECRET_NAME = "PARAEGOX_INIT_SECRET_CANARY"
_SECRET_VALUE = "init-secret-value-must-not-leak"
_OPENAI_SECRET = "init-openai-secret-value-must-not-leak"
_DEEPSEEK_SECRET = "init-deepseek-secret-value-must-not-leak"


@dataclass(frozen=True)
class CommandResult:
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
    assert stat.S_ISREG(metadata.st_mode), "the exact init binary must be a regular file"
    assert not path.is_symlink(), "the exact init binary must not be a symlink"
    assert metadata.st_mode & 0o111 != 0, "the exact init binary must be executable"
    return path.resolve(strict=True)


def _environment() -> dict[str, str]:
    environment = dict(os.environ)
    environment[_SECRET_NAME] = _SECRET_VALUE
    environment["OPENAI_API_KEY"] = _OPENAI_SECRET
    environment["DEEPSEEK_API_KEY"] = _DEEPSEEK_SECRET
    return environment


def _decode_one_compact_json_object(raw: bytes) -> dict[str, Any]:
    assert raw.endswith(b"\n"), "stdout must end in one LF"
    assert raw.count(b"\n") == 1, "stdout must contain exactly one JSON line"
    value = json.loads(raw)
    assert isinstance(value, dict), "stdout must be one JSON object"
    compact = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    assert raw == compact, "stdout must use compact JSON framing"
    return value


def _assert_diagnostics(diagnostics: object, *, expected_count: int) -> None:
    assert isinstance(diagnostics, list)
    assert len(diagnostics) == expected_count
    for diagnostic in diagnostics:
        assert isinstance(diagnostic, dict)
        assert set(diagnostic) == {"code", "message"}
        assert isinstance(diagnostic["code"], str) and diagnostic["code"]
        assert isinstance(diagnostic["message"], str) and diagnostic["message"]


def _assert_init_envelope(
    envelope: dict[str, Any], *, ok: bool, changed: bool
) -> None:
    assert set(envelope) == _INIT_FIELDS
    assert envelope["schema_version"] == 1
    assert envelope["command"] == "init"
    assert envelope["ok"] is ok
    assert envelope["changed"] is changed
    if ok:
        assert envelope["profile"] == _PROFILE
        assert envelope["config_relative_path"] == _CONFIG_RELATIVE_PATH
        assert envelope["state_relative_path"] == _STATE_RELATIVE_PATH
        _assert_diagnostics(envelope["diagnostics"], expected_count=0)
    else:
        assert envelope["profile"] is None
        assert envelope["config_relative_path"] is None
        assert envelope["state_relative_path"] is None
        _assert_diagnostics(envelope["diagnostics"], expected_count=1)


def _assert_public_output_is_redacted(
    result: CommandResult, *, forbidden: tuple[bytes, ...] = ()
) -> None:
    combined = result.stdout + result.stderr
    for value in (
        _SECRET_NAME.encode(),
        _SECRET_VALUE.encode(),
        b"OPENAI_API_KEY",
        _OPENAI_SECRET.encode(),
        b"DEEPSEEK_API_KEY",
        _DEEPSEEK_SECRET.encode(),
        *forbidden,
    ):
        assert value not in combined


def _finish_command(
    process: subprocess.CompletedProcess[bytes],
    *,
    expected_returncode: int,
    forbidden: tuple[bytes, ...] = (),
) -> CommandResult:
    assert process.returncode == expected_returncode, (
        f"command exited {process.returncode}; stdout={process.stdout!r}; "
        f"stderr={process.stderr!r}"
    )
    assert process.stderr == b"", f"exact JSON grammar must keep stderr empty: {process.stderr!r}"
    envelope = _decode_one_compact_json_object(process.stdout)
    result = CommandResult(process.returncode, envelope, process.stdout, process.stderr)
    _assert_public_output_is_redacted(result, forbidden=forbidden)
    return result


def _invoke_init(
    binary: Path,
    directory_argument: str,
    *,
    expected_returncode: int,
    extra_arguments: tuple[str, ...] = (),
    command_prefix: tuple[str, ...] = (),
    environment: dict[str, str] | None = None,
) -> CommandResult:
    process = subprocess.run(
        [
            *command_prefix,
            os.fspath(binary),
            "init",
            "--directory",
            directory_argument,
            "--json",
            *extra_arguments,
        ],
        cwd=binary.parent,
        env=_environment() if environment is None else environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
    )
    return _finish_command(
        process,
        expected_returncode=expected_returncode,
        forbidden=(os.fsencode(directory_argument),),
    )


def _invoke_config_check(binary: Path, config_path: Path) -> CommandResult:
    process = subprocess.run(
        [
            os.fspath(binary),
            "config",
            "check",
            "chat",
            "--config",
            os.fspath(config_path),
            "--json",
        ],
        cwd=binary.parent,
        env=_environment(),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
    )
    return _finish_command(
        process,
        expected_returncode=0,
        forbidden=(os.fsencode(config_path),),
    )


def _assert_directory_metadata(path: Path) -> None:
    metadata = path.lstat()
    assert stat.S_ISDIR(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_uid == os.geteuid()
    assert metadata.st_gid == os.getegid()
    assert metadata.st_mode & 0o7777 == 0o700


def _assert_config_metadata(path: Path) -> None:
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode)
    assert not path.is_symlink()
    assert metadata.st_uid == os.geteuid()
    assert metadata.st_gid == os.getegid()
    assert metadata.st_nlink == 1
    assert metadata.st_mode & 0o7777 == 0o600


def _assert_unchanged_error(result: CommandResult, *, expected_returncode: int = 2) -> None:
    assert result.returncode == expected_returncode
    _assert_init_envelope(result.envelope, ok=False, changed=False)


def _require_passwordless_sudo() -> str:
    sudo = shutil.which("sudo")
    assert sudo is not None, "the admitted Ubuntu init harness requires passwordless sudo"
    preflight = subprocess.run(
        [sudo, "-n", "true"],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=10.0,
    )
    assert preflight.returncode == 0, (
        "the admitted Ubuntu init harness requires passwordless sudo"
    )
    return sudo


def _sudo_chown(sudo: str, path: Path, *, uid: int, gid: int) -> None:
    completed = subprocess.run(
        [sudo, "-n", "chown", f"{uid}:{gid}", os.fspath(path)],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=10.0,
    )
    assert completed.returncode == 0, f"fixture ownership change failed: {completed.stderr!r}"


def test_init_creates_only_strict_config_and_repeats_without_mutation(
    tmp_path: Path,
) -> None:
    assert os.name == "posix" and os.geteuid() != 0 and os.getegid() != 0
    binary = _require_exact_binary()
    directory = tmp_path / "first-init-path-canary"

    first = _invoke_init(binary, os.fspath(directory), expected_returncode=0)
    _assert_init_envelope(first.envelope, ok=True, changed=True)
    _assert_directory_metadata(directory)

    config_path = directory / _CONFIG_RELATIVE_PATH
    state_path = directory / _STATE_RELATIVE_PATH
    assert [entry.name for entry in directory.iterdir()] == [_CONFIG_RELATIVE_PATH]
    _assert_config_metadata(config_path)
    assert not state_path.exists() and not state_path.is_symlink()

    document_bytes = config_path.read_bytes()
    document = tomllib.loads(document_bytes.decode("utf-8"))
    assert document == {
        "schema_version": 1,
        "state_root": os.fspath(state_path),
        "fabric_listen": "tcp/127.0.0.1:7447",
        "model": {"provider": _PROFILE},
    }

    checked = _invoke_config_check(binary, config_path)
    assert checked.envelope["schema_version"] == 1
    assert checked.envelope["command"] == "config.check"
    assert checked.envelope["ok"] is True
    assert checked.envelope["kind"] == "chat"
    assert checked.envelope["config_schema_version"] == 1
    assert checked.envelope["profile"] == _PROFILE
    assert checked.envelope["secret_input_reference_present"] is False
    assert not state_path.exists() and not state_path.is_symlink()

    directory_before = directory.stat()
    config_before = config_path.stat()
    repeat = _invoke_init(binary, os.fspath(directory), expected_returncode=0)
    _assert_init_envelope(repeat.envelope, ok=True, changed=False)
    directory_after = directory.stat()
    config_after = config_path.stat()
    assert config_path.read_bytes() == document_bytes
    assert (config_after.st_dev, config_after.st_ino) == (
        config_before.st_dev,
        config_before.st_ino,
    )
    assert config_after.st_mtime_ns == config_before.st_mtime_ns
    assert config_after.st_ctime_ns == config_before.st_ctime_ns
    assert (directory_after.st_dev, directory_after.st_ino) == (
        directory_before.st_dev,
        directory_before.st_ino,
    )
    assert [entry.name for entry in directory.iterdir()] == [_CONFIG_RELATIVE_PATH]
    assert not state_path.exists() and not state_path.is_symlink()


def test_init_rejects_existing_conflicts_without_overwrite(tmp_path: Path) -> None:
    assert os.name == "posix" and os.geteuid() != 0 and os.getegid() != 0
    binary = _require_exact_binary()

    conflict_directory = tmp_path / "different-content-path-canary"
    created = _invoke_init(binary, os.fspath(conflict_directory), expected_returncode=0)
    _assert_init_envelope(created.envelope, ok=True, changed=True)
    conflict_config = conflict_directory / _CONFIG_RELATIVE_PATH
    conflict_bytes = b"different-content-secret-canary\n"
    conflict_config.write_bytes(conflict_bytes)
    conflict_config.chmod(0o600)
    conflict_before = conflict_config.stat()
    conflict = _invoke_init(binary, os.fspath(conflict_directory), expected_returncode=2)
    _assert_unchanged_error(conflict)
    conflict_after = conflict_config.stat()
    assert conflict_config.read_bytes() == conflict_bytes
    assert (conflict_after.st_dev, conflict_after.st_ino) == (
        conflict_before.st_dev,
        conflict_before.st_ino,
    )
    assert conflict_after.st_mtime_ns == conflict_before.st_mtime_ns
    assert conflict_bytes.strip() not in conflict.stdout

    wrong_directory_mode = tmp_path / "wrong-directory-mode-path-canary"
    wrong_directory_mode.mkdir(mode=0o700)
    wrong_directory_mode.chmod(0o755)
    wrong_directory = _invoke_init(
        binary, os.fspath(wrong_directory_mode), expected_returncode=2
    )
    _assert_unchanged_error(wrong_directory)
    assert list(wrong_directory_mode.iterdir()) == []
    assert wrong_directory_mode.stat().st_mode & 0o7777 == 0o755

    wrong_file_mode = tmp_path / "wrong-file-mode-path-canary"
    created = _invoke_init(binary, os.fspath(wrong_file_mode), expected_returncode=0)
    _assert_init_envelope(created.envelope, ok=True, changed=True)
    wrong_mode_config = wrong_file_mode / _CONFIG_RELATIVE_PATH
    wrong_mode_bytes = wrong_mode_config.read_bytes()
    wrong_mode_config.chmod(0o644)
    rejected_mode = _invoke_init(binary, os.fspath(wrong_file_mode), expected_returncode=2)
    _assert_unchanged_error(rejected_mode)
    assert wrong_mode_config.read_bytes() == wrong_mode_bytes
    assert wrong_mode_config.stat().st_mode & 0o7777 == 0o644

    hardlink_directory = tmp_path / "hardlink-path-canary"
    created = _invoke_init(binary, os.fspath(hardlink_directory), expected_returncode=0)
    _assert_init_envelope(created.envelope, ok=True, changed=True)
    hardlink_config = hardlink_directory / _CONFIG_RELATIVE_PATH
    hardlink_bytes = hardlink_config.read_bytes()
    hardlink_peer = tmp_path / "hardlink-peer-secret-canary"
    os.link(hardlink_config, hardlink_peer)
    rejected_link = _invoke_init(
        binary, os.fspath(hardlink_directory), expected_returncode=2
    )
    _assert_unchanged_error(rejected_link)
    assert hardlink_config.read_bytes() == hardlink_bytes
    assert hardlink_peer.read_bytes() == hardlink_bytes
    assert hardlink_config.stat().st_nlink == 2

    temporary_conflict_directory = tmp_path / "temporary-conflict-path-canary"
    temporary_conflict_directory.mkdir(mode=0o700)
    reserved_temporary = temporary_conflict_directory / ".paraegox.toml.tmp"
    temporary_bytes = b"reserved-temporary-content-secret-canary\n"
    reserved_temporary.write_bytes(temporary_bytes)
    reserved_temporary.chmod(0o600)
    temporary_before = reserved_temporary.stat()
    temporary_conflict = _invoke_init(
        binary,
        os.fspath(temporary_conflict_directory),
        expected_returncode=2,
    )
    _assert_unchanged_error(temporary_conflict)
    temporary_after = reserved_temporary.stat()
    assert reserved_temporary.read_bytes() == temporary_bytes
    assert (temporary_after.st_dev, temporary_after.st_ino) == (
        temporary_before.st_dev,
        temporary_before.st_ino,
    )
    assert temporary_after.st_mtime_ns == temporary_before.st_mtime_ns
    assert temporary_bytes.strip() not in temporary_conflict.stdout
    assert not (temporary_conflict_directory / _CONFIG_RELATIVE_PATH).exists()
    assert [entry.name for entry in temporary_conflict_directory.iterdir()] == [
        ".paraegox.toml.tmp"
    ]

    sudo = _require_passwordless_sudo()
    invoking_uid = os.geteuid()
    invoking_gid = os.getegid()

    wrong_owner_directory = tmp_path / "wrong-directory-owner-path-canary"
    created = _invoke_init(binary, os.fspath(wrong_owner_directory), expected_returncode=0)
    _assert_init_envelope(created.envelope, ok=True, changed=True)
    _sudo_chown(sudo, wrong_owner_directory, uid=0, gid=0)
    try:
        rejected_owner = _invoke_init(
            binary, os.fspath(wrong_owner_directory), expected_returncode=2
        )
        _assert_unchanged_error(rejected_owner)
        assert wrong_owner_directory.stat().st_uid == 0
        assert wrong_owner_directory.stat().st_gid == 0
    finally:
        _sudo_chown(
            sudo,
            wrong_owner_directory,
            uid=invoking_uid,
            gid=invoking_gid,
        )

    wrong_file_owner = tmp_path / "wrong-file-owner-path-canary"
    created = _invoke_init(binary, os.fspath(wrong_file_owner), expected_returncode=0)
    _assert_init_envelope(created.envelope, ok=True, changed=True)
    wrong_owner_config = wrong_file_owner / _CONFIG_RELATIVE_PATH
    wrong_owner_bytes = wrong_owner_config.read_bytes()
    _sudo_chown(sudo, wrong_owner_config, uid=0, gid=0)
    try:
        rejected_owner = _invoke_init(binary, os.fspath(wrong_file_owner), expected_returncode=2)
        _assert_unchanged_error(rejected_owner)
        assert wrong_owner_config.stat().st_uid == 0
        assert wrong_owner_config.stat().st_gid == 0
    finally:
        _sudo_chown(
            sudo,
            wrong_owner_config,
            uid=invoking_uid,
            gid=invoking_gid,
        )
    assert wrong_owner_config.read_bytes() == wrong_owner_bytes

    unwritable_parent = tmp_path / "unwritable-parent-path-canary"
    unwritable_parent.mkdir(mode=0o700)
    unwritable_parent.chmod(0o500)
    _sudo_chown(sudo, unwritable_parent, uid=0, gid=0)
    io_failure_directory = unwritable_parent / "io-failure-path-canary"
    try:
        io_failure = _invoke_init(
            binary, os.fspath(io_failure_directory), expected_returncode=1
        )
        _assert_unchanged_error(io_failure, expected_returncode=1)
    finally:
        _sudo_chown(
            sudo,
            unwritable_parent,
            uid=invoking_uid,
            gid=invoking_gid,
        )
        unwritable_parent.chmod(0o700)
    assert not io_failure_directory.exists() and not io_failure_directory.is_symlink()


def test_init_rejects_symlinks_relative_extra_and_root_without_leaks(
    tmp_path: Path,
) -> None:
    assert os.name == "posix" and os.geteuid() != 0 and os.getegid() != 0
    binary = _require_exact_binary()

    real_directory = tmp_path / "real-directory-secret-canary"
    real_directory.mkdir(mode=0o700)
    directory_symlink = tmp_path / "directory-symlink-path-canary"
    directory_symlink.symlink_to(real_directory, target_is_directory=True)
    rejected_directory_symlink = _invoke_init(
        binary, os.fspath(directory_symlink), expected_returncode=2
    )
    _assert_unchanged_error(rejected_directory_symlink)
    assert list(real_directory.iterdir()) == []

    config_symlink_directory = tmp_path / "config-symlink-path-canary"
    config_symlink_directory.mkdir(mode=0o700)
    symlink_target = tmp_path / "config-symlink-target-secret-canary"
    symlink_bytes = b"symlink-target-content-secret-canary\n"
    symlink_target.write_bytes(symlink_bytes)
    symlink_target.chmod(0o600)
    (config_symlink_directory / _CONFIG_RELATIVE_PATH).symlink_to(symlink_target)
    rejected_config_symlink = _invoke_init(
        binary, os.fspath(config_symlink_directory), expected_returncode=2
    )
    _assert_unchanged_error(rejected_config_symlink)
    assert symlink_target.read_bytes() == symlink_bytes
    assert symlink_bytes.strip() not in rejected_config_symlink.stdout

    rejected_filesystem_root = _invoke_init(binary, "/", expected_returncode=2)
    _assert_unchanged_error(rejected_filesystem_root)

    relative = "relative-init-path-secret-canary"
    rejected_relative = _invoke_init(binary, relative, expected_returncode=2)
    _assert_unchanged_error(rejected_relative)
    assert not (binary.parent / relative).exists()

    extra_directory = tmp_path / "extra-argument-path-canary"
    rejected_extra = _invoke_init(
        binary,
        os.fspath(extra_directory),
        expected_returncode=2,
        extra_arguments=("--unexpected-secret-canary",),
    )
    _assert_unchanged_error(rejected_extra)
    assert not extra_directory.exists() and not extra_directory.is_symlink()
    assert b"unexpected-secret-canary" not in rejected_extra.stdout

    sudo = _require_passwordless_sudo()
    root_directory = tmp_path / "root-execution-path-canary"
    root_environment = _environment()
    root_environment["PATH"] = os.environ.get("PATH", "/usr/bin:/bin")
    rejected_root = _invoke_init(
        binary,
        os.fspath(root_directory),
        expected_returncode=2,
        command_prefix=(sudo, "-n", "--"),
        environment=root_environment,
    )
    _assert_unchanged_error(rejected_root)
    assert not root_directory.exists() and not root_directory.is_symlink()


_EFFECT_INTERPOSER_SOURCE = r"""
#define _GNU_SOURCE
#include <errno.h>
#include <netdb.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <unistd.h>

static void report_effect(char effect) {
    const char *raw_fd = getenv("PARAEGOX_INIT_EFFECT_AUDIT_FD");
    if (raw_fd == NULL) {
        return;
    }
    char *end = NULL;
    long fd = strtol(raw_fd, &end, 10);
    if (end == raw_fd || *end != '\0' || fd < 0) {
        return;
    }
    (void)syscall(SYS_write, (int)fd, &effect, 1);
}

int socket(int domain, int type, int protocol) {
    (void)domain; (void)type; (void)protocol;
    report_effect('s'); errno = EPERM; return -1;
}

int socketpair(int domain, int type, int protocol, int descriptors[2]) {
    (void)domain; (void)type; (void)protocol; (void)descriptors;
    report_effect('S'); errno = EPERM; return -1;
}

int connect(int descriptor, const struct sockaddr *address, socklen_t length) {
    (void)descriptor; (void)address; (void)length;
    report_effect('c'); errno = EPERM; return -1;
}

int bind(int descriptor, const struct sockaddr *address, socklen_t length) {
    (void)descriptor; (void)address; (void)length;
    report_effect('b'); errno = EPERM; return -1;
}

int listen(int descriptor, int backlog) {
    (void)descriptor; (void)backlog;
    report_effect('l'); errno = EPERM; return -1;
}

int getaddrinfo(
    const char *node,
    const char *service,
    const struct addrinfo *hints,
    struct addrinfo **result
) {
    (void)node; (void)service; (void)hints; (void)result;
    report_effect('g'); return EAI_FAIL;
}

pid_t fork(void) {
    report_effect('f'); errno = EPERM; return -1;
}

pid_t vfork(void) {
    report_effect('v'); errno = EPERM; return -1;
}

int posix_spawn(
    pid_t *pid,
    const char *path,
    const posix_spawn_file_actions_t *actions,
    const posix_spawnattr_t *attributes,
    char *const argv[],
    char *const envp[]
) {
    (void)pid; (void)path; (void)actions; (void)attributes; (void)argv; (void)envp;
    report_effect('p'); return EPERM;
}

int posix_spawnp(
    pid_t *pid,
    const char *file,
    const posix_spawn_file_actions_t *actions,
    const posix_spawnattr_t *attributes,
    char *const argv[],
    char *const envp[]
) {
    (void)pid; (void)file; (void)actions; (void)attributes; (void)argv; (void)envp;
    report_effect('P'); return EPERM;
}

int system(const char *command) {
    (void)command; report_effect('y'); errno = EPERM; return -1;
}

FILE *popen(const char *command, const char *mode) {
    (void)command; (void)mode; report_effect('o'); errno = EPERM; return NULL;
}

int execve(const char *path, char *const argv[], char *const envp[]) {
    (void)path; (void)argv; (void)envp;
    report_effect('e'); errno = EPERM; return -1;
}

int execveat(
    int directory_fd,
    const char *path,
    char *const argv[],
    char *const envp[],
    int flags
) {
    (void)directory_fd; (void)path; (void)argv; (void)envp; (void)flags;
    report_effect('E'); errno = EPERM; return -1;
}
"""


def _compile_effect_interposer(tmp_path: Path) -> Path:
    compiler = shutil.which("cc")
    assert compiler is not None, "the admitted Ubuntu init harness requires a C compiler"
    source = tmp_path / "init_effect_interposer.c"
    library = tmp_path / "init_effect_interposer.so"
    source.write_text(_EFFECT_INTERPOSER_SOURCE, encoding="utf-8")
    completed = subprocess.run(
        [
            compiler,
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-shared",
            "-fPIC",
            os.fspath(source),
            "-o",
            os.fspath(library),
        ],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=30.0,
    )
    assert completed.returncode == 0, (
        f"effect interposer compilation failed: {completed.stderr!r}"
    )
    return library


def _matching_binary_processes(binary: Path) -> set[int]:
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


def test_init_has_no_child_socket_or_network_effect(tmp_path: Path) -> None:
    assert (
        os.name == "posix"
        and Path("/proc/self").is_dir()
        and os.geteuid() != 0
        and os.getegid() != 0
    )
    binary = _require_exact_binary()
    interposer = _compile_effect_interposer(tmp_path)
    directory = tmp_path / "effect-free-init-path-canary"
    before_processes = _matching_binary_processes(binary)
    read_fd, write_fd = os.pipe()
    try:
        environment = _environment()
        environment["LD_PRELOAD"] = os.fspath(interposer)
        environment["PARAEGOX_INIT_EFFECT_AUDIT_FD"] = str(write_fd)
        process = subprocess.Popen(
            [
                os.fspath(binary),
                "init",
                "--directory",
                os.fspath(directory),
                "--json",
            ],
            cwd=binary.parent,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            pass_fds=(write_fd,),
        )
        os.close(write_fd)
        write_fd = -1
        try:
            stdout, stderr = process.communicate(timeout=30.0)
        except subprocess.TimeoutExpired as error:
            process.kill()
            stdout, stderr = process.communicate()
            raise AssertionError(
                f"init effect audit timed out; stdout={stdout!r}; stderr={stderr!r}"
            ) from error
        os.set_blocking(read_fd, False)
        try:
            audit = os.read(read_fd, 4096)
        except BlockingIOError:
            audit = b""
    finally:
        os.close(read_fd)
        if write_fd >= 0:
            os.close(write_fd)

    result = _finish_command(
        subprocess.CompletedProcess(process.args, process.returncode, stdout, stderr),
        expected_returncode=0,
        forbidden=(os.fsencode(directory),),
    )
    _assert_init_envelope(result.envelope, ok=True, changed=True)
    assert audit == b"", f"init attempted a child/socket/network effect: {audit!r}"
    assert _matching_binary_processes(binary) == before_processes
    assert [entry.name for entry in directory.iterdir()] == [_CONFIG_RELATIVE_PATH]
    assert not (directory / _STATE_RELATIVE_PATH).exists()
