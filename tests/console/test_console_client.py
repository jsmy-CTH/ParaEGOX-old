from __future__ import annotations

import asyncio
import hashlib
import inspect
import os
import shutil
import socket
import stat
import struct
import tempfile
from collections.abc import Awaitable, Callable, Iterator
from contextlib import contextmanager
from dataclasses import FrozenInstanceError, replace
from pathlib import Path

import pytest

import paraegox_sdk.console_client as console_client
from paraegox_sdk.agent_worker.control import (
    AgentConversationCancelOutcomeV1,
    AgentConversationControlKindV1,
    AgentConversationControlV1,
    AgentConversationOpenOutcomeV1,
    decode_control_v1,
)
from paraegox_sdk.agent_worker.protocol import (
    AgentConversationTerminalFailureV1,
    AgentConversationTerminalV1,
    TerminalOutcome,
    decode_request_v1,
)
from paraegox_sdk.console_client import (
    RuntimeAgentConversationClientError,
    RuntimeAgentConversationClientErrorCode,
    RuntimeAgentConversationClientV1,
)

PXAB_DIGEST_HEX = "9f1801332f8c4b590534fcff9a8ace9105ede142b0f3e60e66bbc009c24a8bc1"
PXAI_WIRE_HEX = (
    "5058414900010070000000f001000000"
    "31313131313131313131313131313131"
    "3232323232323232323232323232323232323232323232323232323232323232"
    "000000003b9aca000000008000000000"
    "bd80a1f59bb8c6801628e282e701d7e009f5a69faa58ee4b95730c9cdf63c835"
    "3333333333333333333333333333333333333333333333333333333333333333"
    "3333333333333333333333333333333333333333333333333333333333333333"
    "3333333333333333333333333333333333333333333333333333333333333333"
    "3333333333333333333333333333333333333333333333333333333333333333"
)

_PXAB_HEADER = struct.Struct(">4sHHIHHII32s16s16sQQHHI32s")
_PXAI_HEADER_BYTES = 112
_MAX_IPC_BODY_BYTES = 65_664
_GENERATION_TOKEN = bytes([0x5A]) * 32
_DECK_RUN_ID = bytes([0x44]) * 16
_SESSION_ID = bytes([0x11]) * 16
_DEADLINE_NANOS = 5_000_000_000
_OPERATION_TIMEOUT_NANOS = 2_000_000_000
_COMMAND_CAPACITY = 8
_INSPECTION_PROJECTION_ID = bytes([0x21]) * 16
_INSPECTION_CLOCK_REF = bytes([0x31]) * 16
_INSPECTION_TOKEN = bytes([0x5B]) * 32
_INSPECTION_REQUEST_SEED = bytes([0x6C]) * 16
_INSPECTION_TIMEOUT_NANOS = 2_000_000_000
_RUST_INSPECTION_FIXTURES = Path(__file__).parents[2] / "crates/paraegox-inspection/tests/fixtures"
_TUI_ATTACH_HANDOFF_GOLDEN = (
    Path(__file__).parents[1] / "fixtures/wire/m5a_tui_attach_handoff_v1.hex"
)


def _inspection_fixture(name: str) -> bytes:
    return bytes.fromhex((_RUST_INSPECTION_FIXTURES / name).read_text(encoding="ascii"))


def _rust_canonical_digest(domain: bytes, fields: tuple[bytes, ...]) -> bytes:
    digest = hashlib.sha256()
    digest.update(b"ParaEGOX\0canonical-digest")
    digest.update((1).to_bytes(2, "big"))
    digest.update(len(domain).to_bytes(4, "big"))
    digest.update(domain)
    for ordinal, value in enumerate(fields, start=1):
        digest.update(b"\x01")
        digest.update(ordinal.to_bytes(4, "big"))
        digest.update(len(value).to_bytes(8, "big"))
        digest.update(value)
    digest.update(b"\xff")
    digest.update(len(fields).to_bytes(4, "big"))
    return digest.digest()


def _rust_bootstrap_wire(socket_path: Path) -> bytes:
    path = os.fsencode(socket_path)
    uid = os.geteuid()
    gid = os.getegid()
    digest = _rust_canonical_digest(
        b"paraegox.runtime.agent.developer-local.bootstrap.sha256.v1",
        (
            (1).to_bytes(2, "big"),
            uid.to_bytes(4, "big"),
            gid.to_bytes(4, "big"),
            _GENERATION_TOKEN,
            _DECK_RUN_ID,
            _SESSION_ID,
            _DEADLINE_NANOS.to_bytes(8, "big"),
            _OPERATION_TIMEOUT_NANOS.to_bytes(8, "big"),
            _COMMAND_CAPACITY.to_bytes(2, "big"),
            _MAX_IPC_BODY_BYTES.to_bytes(4, "big"),
            path,
        ),
    )
    return (
        _PXAB_HEADER.pack(
            b"PXAB",
            1,
            144,
            144 + len(path),
            len(path),
            1,
            uid,
            gid,
            _GENERATION_TOKEN,
            _DECK_RUN_ID,
            _SESSION_ID,
            _DEADLINE_NANOS,
            _OPERATION_TIMEOUT_NANOS,
            _COMMAND_CAPACITY,
            0,
            _MAX_IPC_BODY_BYTES,
            digest,
        )
        + path
    )


def _inspection_bootstrap_wire(socket_path: Path) -> bytes:
    bootstrap = console_client._InspectionBootstrapV2(
        socket_path=os.fsencode(socket_path),
        projection_id=_INSPECTION_PROJECTION_ID,
        generation_token=bytearray(_INSPECTION_TOKEN),
        server_uid=os.geteuid(),
        server_gid=os.getegid(),
        operation_timeout_nanos=_INSPECTION_TIMEOUT_NANOS,
        request_seed=bytearray(_INSPECTION_REQUEST_SEED),
    )
    return console_client._encode_inspection_bootstrap_v2(bootstrap)


def _write_bootstrap(path: Path, socket_path: Path, *, wire: bytes | None = None) -> None:
    path.write_bytes(_rust_bootstrap_wire(socket_path) if wire is None else wire)
    path.chmod(0o600)


def _write_inspection_bootstrap(path: Path, socket_path: Path) -> None:
    path.write_bytes(_inspection_bootstrap_wire(socket_path))
    path.chmod(0o600)


def _tui_attach_pin(
    path: Path,
    kind: bytes,
) -> console_client._TuiAttachBootstrapPinV1:
    metadata = path.stat()
    content = path.read_bytes()
    return console_client._TuiAttachBootstrapPinV1(
        kind=kind,
        path=os.fsencode(path),
        content_length=len(content),
        content_sha256=hashlib.sha256(content).digest(),
        uid=metadata.st_uid,
        gid=metadata.st_gid,
        mode=metadata.st_mode & 0o7777,
        link_count=metadata.st_nlink,
        device=metadata.st_dev,
        inode=metadata.st_ino,
    )


def _inspection_snapshot_with_revision(revision: int) -> bytes:
    wire = bytearray(_inspection_fixture("local_inspection_snapshot_v2.hex"))
    wire[48:56] = revision.to_bytes(8, "big")
    base_start = 112
    wire[base_start + 48 : base_start + 56] = revision.to_bytes(8, "big")
    wire[base_start + 80 : base_start + 112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v1",
        (
            bytes(wire[base_start : base_start + 80]),
            bytes(wire[base_start + 112 : base_start + 592]),
        ),
    )
    wire[80:112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v2",
        (bytes(wire[:80]), bytes(wire[112:])),
    )
    return bytes(wire)


def _inspection_response(
    request: bytes,
    outcome: console_client._InspectionResponseOutcomeV2,
    *,
    current_revision: int,
    snapshot_wire: bytes = b"",
) -> bytes:
    frame = bytearray(144 + len(snapshot_wire))
    frame[:4] = b"PXIP"
    frame[4:6] = (2).to_bytes(2, "big")
    frame[6:8] = (144).to_bytes(2, "big")
    frame[8:12] = len(frame).to_bytes(4, "big")
    frame[12:16] = len(snapshot_wire).to_bytes(4, "big")
    frame[16] = int(outcome)
    frame[17] = request[12]
    frame[24:40] = request[16:32]
    frame[40:56] = request[32:48]
    frame[56:64] = request[48:56]
    frame[64:72] = current_revision.to_bytes(8, "big")
    frame[72:104] = request[64:96]
    frame[144:] = snapshot_wire
    frame[112:144] = _rust_canonical_digest(
        b"paraegox.inspection.protocol-response.v2",
        (bytes(frame[:112]), snapshot_wire),
    )
    return bytes(frame)


@contextmanager
def _private_directory() -> Iterator[Path]:
    created = Path(tempfile.mkdtemp(prefix="px-console-"))
    resolved = Path(os.path.realpath(created))
    resolved.chmod(0o700)
    try:
        yield resolved
    finally:
        shutil.rmtree(created)


@contextmanager
def _bound_private_socket(directory: Path) -> Iterator[Path]:
    socket_path = directory / "agent.sock"
    endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    endpoint.bind(os.fspath(socket_path))
    socket_path.chmod(0o600)
    try:
        yield socket_path
    finally:
        endpoint.close()
        with contextlib_suppress(FileNotFoundError):
            socket_path.unlink()


@contextmanager
def contextlib_suppress(*exceptions: type[BaseException]) -> Iterator[None]:
    try:
        yield
    except exceptions:
        pass


def test_bootstrap_and_ipc_codecs_match_rust_golden_vectors() -> None:
    fixed_bootstrap_digest = console_client._bootstrap_digest(
        server_uid=501,
        server_gid=20,
        generation_token=bytes([0x5A]) * 32,
        deck_run_id=_DECK_RUN_ID,
        session_id=_SESSION_ID,
        request_deadline_budget_nanos=5_000_000_000,
        operation_timeout_nanos=1_000_000_000,
        command_capacity=8,
        socket_path=b"/tmp/paraegox-agent.sock",
    )
    assert fixed_bootstrap_digest.hex() == PXAB_DIGEST_HEX

    frame = console_client._IpcFrame(
        kind=console_client._OperationKind.OPEN,
        status=console_client._ResponseStatus.OK,
        correlation=bytes([0x31]) * 16,
        generation_token=bytes([0x32]) * 32,
        operation_timeout_nanos=1_000_000_000,
        body=bytes([0x33]) * 128,
    )
    wire = console_client._encode_ipc_frame(b"PXAI", frame)
    assert wire.hex() == PXAI_WIRE_HEX
    assert console_client._decode_ipc_frame(b"PXAI", wire) == frame


def _fixed_tui_handoff() -> console_client._TuiAttachHandoffV1:
    return console_client._TuiAttachHandoffV1(
        generation=bytes([0x41]) * 16,
        config_commitment=bytes([0x42]) * 32,
        conversation=console_client._TuiAttachBootstrapPinV1(
            kind=b"C",
            path=b"/private/run/conversation.pxab",
            content_length=144,
            content_sha256=bytes([0x43]) * 32,
            uid=501,
            gid=20,
            mode=0o600,
            link_count=1,
            device=7,
            inode=11,
        ),
        inspection=console_client._TuiAttachBootstrapPinV1(
            kind=b"I",
            path=b"/private/run/inspection.pxib",
            content_length=128,
            content_sha256=bytes([0x44]) * 32,
            uid=501,
            gid=20,
            mode=0o600,
            link_count=1,
            device=7,
            inode=12,
        ),
    )


def test_tui_attach_handoff_v1_is_canonical_bounded_and_token_free() -> None:
    handoff = _fixed_tui_handoff()
    wire = console_client._encode_tui_attach_handoff_v1(handoff)
    golden = bytes.fromhex(_TUI_ATTACH_HANDOFF_GOLDEN.read_text(encoding="ascii"))

    assert len(golden) == 346
    assert hashlib.sha256(golden).hexdigest() == (
        "b2290af8d07d94ccbef67bf05e115181afa330d57aa66b68690322d846548621"
    )
    assert wire == golden
    assert wire[:4] == b"PXTH"
    assert wire[4:6] == (1).to_bytes(2, "big")
    assert wire[6:8] == b"TR"
    assert wire[8:10] == (288).to_bytes(2, "big")
    assert wire[10:12] == (2).to_bytes(2, "big")
    assert int.from_bytes(wire[12:16], "big") == len(wire)
    assert wire[64] == ord("C")
    assert wire[160] == ord("I")
    assert len(wire) <= 8_480
    assert _GENERATION_TOKEN not in wire
    expected_digest = hashlib.sha256(
        b"paraegox.local.tui-attach-handoff.v1" + wire[:256] + wire[288:]
    ).digest()
    assert wire[256:288] == expected_digest
    assert (
        console_client._decode_tui_attach_handoff_v1(
            wire,
            expected_uid=501,
            expected_gid=20,
        )
        == handoff
    )

    corruptions = [0, 4, 6, 7, 8, 10, 12, 16, 32, 64, 65, 68, 72, 76, 80, 84, 88, 112, 144]
    for offset in corruptions:
        corrupted = bytearray(wire)
        corrupted[offset] ^= 1
        with pytest.raises(console_client._TuiAttachHandoffError):
            console_client._decode_tui_attach_handoff_v1(
                bytes(corrupted),
                expected_uid=501,
                expected_gid=20,
            )

    with pytest.raises(console_client._TuiAttachHandoffError):
        console_client._decode_tui_attach_handoff_v1(
            wire + b"x",
            expected_uid=501,
            expected_gid=20,
        )

    invalid_handoffs = [
        replace(
            handoff,
            conversation=replace(handoff.conversation, path=b"/private//conversation.pxab"),
        ),
        replace(
            handoff,
            conversation=replace(
                handoff.conversation,
                path=handoff.inspection.path,
            ),
        ),
        replace(
            handoff,
            inspection=replace(handoff.inspection, content_sha256=bytes(32)),
        ),
        replace(
            handoff,
            inspection=replace(handoff.inspection, link_count=2),
        ),
        replace(
            handoff,
            inspection=replace(handoff.inspection, path=b"/" + b"a" * 4_096),
        ),
    ]
    for invalid in invalid_handoffs:
        with pytest.raises(console_client._TuiAttachHandoffError):
            console_client._encode_tui_attach_handoff_v1(invalid)


def test_tui_attach_fd_requires_one_same_peer_frame_and_exact_eof() -> None:
    wire = console_client._encode_tui_attach_handoff_v1(
        console_client._TuiAttachHandoffV1(
            generation=bytes([0x41]) * 16,
            config_commitment=bytes([0x42]) * 32,
            conversation=replace(
                _fixed_tui_handoff().conversation,
                uid=os.geteuid(),
                gid=os.getegid(),
            ),
            inspection=replace(
                _fixed_tui_handoff().inspection,
                uid=os.geteuid(),
                gid=os.getegid(),
            ),
        )
    )
    reader, writer = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        writer.sendall(wire)
        writer.shutdown(socket.SHUT_WR)
        decoded = console_client._read_tui_attach_handoff_fd(os.dup(reader.fileno()))
        assert decoded.generation == bytes([0x41]) * 16
    finally:
        reader.close()
        writer.close()

    for payload in (wire[:-1], wire + b"trailing"):
        reader, writer = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            writer.sendall(payload)
            writer.shutdown(socket.SHUT_WR)
            with pytest.raises(console_client._TuiAttachHandoffError):
                console_client._read_tui_attach_handoff_fd(os.dup(reader.fileno()))
        finally:
            reader.close()
            writer.close()

    class TimedOutSocket:
        @staticmethod
        def recv(_length: int) -> bytes:
            raise TimeoutError

    with pytest.raises(console_client._TuiAttachHandoffError) as timed_out:
        console_client._receive_exact(TimedOutSocket(), 1)  # type: ignore[arg-type]
    assert timed_out.value.code is console_client._TuiAttachHandoffErrorCode.IO


def test_private_bootstrap_reader_checks_permissions_digest_and_socket() -> None:
    with _private_directory() as directory, _bound_private_socket(directory) as socket_path:
        bootstrap_path = directory / "agent.bootstrap"
        _write_bootstrap(bootstrap_path, socket_path)

        client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        client.close()
        client.close()

        bootstrap_path.chmod(0o644)
        with pytest.raises(RuntimeAgentConversationClientError) as permissions:
            RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        assert (
            permissions.value.code is RuntimeAgentConversationClientErrorCode.INSECURE_PERMISSIONS
        )

        bootstrap_path.chmod(0o600)
        tampered = bytearray(_rust_bootstrap_wire(socket_path))
        tampered[112] ^= 1
        _write_bootstrap(bootstrap_path, socket_path, wire=bytes(tampered))
        with pytest.raises(RuntimeAgentConversationClientError) as digest:
            RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        assert digest.value.code is RuntimeAgentConversationClientErrorCode.DIGEST_MISMATCH

        _write_bootstrap(bootstrap_path, socket_path)
        socket_path.chmod(0o666)
        with pytest.raises(RuntimeAgentConversationClientError) as insecure_socket:
            RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        assert insecure_socket.value.code is RuntimeAgentConversationClientErrorCode.INVALID_SOCKET


def test_bootstrap_reader_rejects_insecure_parent() -> None:
    with _private_directory() as directory, _bound_private_socket(directory) as socket_path:
        bootstrap_path = directory / "agent.bootstrap"
        _write_bootstrap(bootstrap_path, socket_path)
        directory.chmod(0o755)
        with pytest.raises(RuntimeAgentConversationClientError) as permissions:
            RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        assert (
            permissions.value.code is RuntimeAgentConversationClientErrorCode.INSECURE_PERMISSIONS
        )
        directory.chmod(0o700)


def test_agent_client_entropy_failure_is_typed_and_zeroizes_token(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    bootstrap = console_client._BootstrapV1(
        socket_path=b"/private/agent.sock",
        generation_token=bytearray(_GENERATION_TOKEN),
        deck_run_id=_DECK_RUN_ID,
        session_id=_SESSION_ID,
        request_deadline_budget_nanos=_DEADLINE_NANOS,
        operation_timeout_nanos=_OPERATION_TIMEOUT_NANOS,
        command_capacity=_COMMAND_CAPACITY,
        server_uid=os.geteuid(),
        server_gid=os.getegid(),
    )
    monkeypatch.setattr(
        console_client.secrets,
        "token_bytes",
        lambda _length: (_ for _ in ()).throw(OSError()),
    )
    with pytest.raises(RuntimeAgentConversationClientError) as unavailable:
        RuntimeAgentConversationClientV1._from_bootstrap(bootstrap)
    assert unavailable.value.code is RuntimeAgentConversationClientErrorCode.ENTROPY_UNAVAILABLE
    assert not any(bootstrap.generation_token)


def test_tui_attach_pinned_bootstrap_loaders_bind_exact_file_bytes_and_identity(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with _private_directory() as directory:
        agent_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        inspection_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        agent_socket_path = directory / "agent.sock"
        inspection_socket_path = directory / "inspection.sock"
        agent_socket.bind(os.fspath(agent_socket_path))
        inspection_socket.bind(os.fspath(inspection_socket_path))
        agent_socket_path.chmod(0o600)
        inspection_socket_path.chmod(0o600)
        os.link(
            inspection_socket_path,
            directory / ".pxi-0123456789abcdef0123456789abcdef-socket.pin",
        )
        agent_bootstrap = directory / "agent.pxab"
        inspection_bootstrap = directory / "inspection.pxib"
        _write_bootstrap(agent_bootstrap, agent_socket_path)
        _write_inspection_bootstrap(inspection_bootstrap, inspection_socket_path)
        agent_pin = _tui_attach_pin(agent_bootstrap, b"C")
        inspection_pin = _tui_attach_pin(inspection_bootstrap, b"I")
        try:
            RuntimeAgentConversationClientV1._from_tui_attach_pin(agent_pin).close()
            console_client.DeveloperLocalInspectionClientV2._from_tui_attach_pin(
                inspection_pin
            ).close()

            real_read = console_client.os.read

            def fail_read(_descriptor: int, _remaining: int) -> bytes:
                raise OSError

            monkeypatch.setattr(console_client.os, "read", fail_read)
            with pytest.raises(RuntimeAgentConversationClientError) as read_failed:
                RuntimeAgentConversationClientV1._from_tui_attach_pin(agent_pin)
            assert read_failed.value.code is RuntimeAgentConversationClientErrorCode.IO
            monkeypatch.setattr(console_client.os, "read", real_read)

            agent_wire = agent_bootstrap.read_bytes()
            tampered = bytearray(agent_wire)
            tampered[112] ^= 1
            agent_bootstrap.write_bytes(tampered)
            with pytest.raises(RuntimeAgentConversationClientError) as digest:
                RuntimeAgentConversationClientV1._from_tui_attach_pin(agent_pin)
            assert digest.value.code is RuntimeAgentConversationClientErrorCode.DIGEST_MISMATCH
            agent_bootstrap.write_bytes(agent_wire)

            replacement = directory / "replacement.pxab"
            replacement.write_bytes(agent_wire)
            replacement.chmod(0o600)
            os.replace(replacement, agent_bootstrap)
            with pytest.raises(RuntimeAgentConversationClientError) as replaced:
                RuntimeAgentConversationClientV1._from_tui_attach_pin(agent_pin)
            assert isinstance(
                replaced.value,
                console_client._TuiAttachAgentBootstrapIdentityError,
            )
            assert replaced.value.code is (
                RuntimeAgentConversationClientErrorCode.ENDPOINT_IDENTITY_CHANGED
            )

            inspection_wire = inspection_bootstrap.read_bytes()
            tampered_inspection = bytearray(inspection_wire)
            tampered_inspection[96] ^= 1
            inspection_bootstrap.write_bytes(tampered_inspection)
            with pytest.raises(
                console_client.DeveloperLocalInspectionClientError
            ) as inspection_digest:
                console_client.DeveloperLocalInspectionClientV2._from_tui_attach_pin(inspection_pin)
            assert inspection_digest.value.code is (
                console_client.DeveloperLocalInspectionClientErrorCode.DIGEST_MISMATCH
            )
        finally:
            agent_socket.close()
            inspection_socket.close()


def test_tui_attach_pinned_loaders_reject_a_symlinked_path_component() -> None:
    with _private_directory() as directory:
        actual = directory / "actual"
        actual.mkdir(mode=0o700)
        alias = directory / "alias"
        alias.symlink_to(actual, target_is_directory=True)
        agent_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        inspection_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        agent_socket_path = actual / "agent.sock"
        inspection_socket_path = actual / "inspection.sock"
        agent_socket.bind(os.fspath(agent_socket_path))
        inspection_socket.bind(os.fspath(inspection_socket_path))
        agent_socket_path.chmod(0o600)
        inspection_socket_path.chmod(0o600)
        os.link(
            inspection_socket_path,
            actual / ".pxi-0123456789abcdef0123456789abcdef-socket.pin",
        )
        agent_bootstrap = actual / "agent.pxab"
        inspection_bootstrap = actual / "inspection.pxib"
        _write_bootstrap(agent_bootstrap, agent_socket_path)
        _write_inspection_bootstrap(inspection_bootstrap, inspection_socket_path)
        agent_pin = replace(
            _tui_attach_pin(agent_bootstrap, b"C"),
            path=os.fsencode(alias / agent_bootstrap.name),
        )
        inspection_pin = replace(
            _tui_attach_pin(inspection_bootstrap, b"I"),
            path=os.fsencode(alias / inspection_bootstrap.name),
        )
        try:
            with pytest.raises(RuntimeAgentConversationClientError) as agent_rejected:
                RuntimeAgentConversationClientV1._from_tui_attach_pin(agent_pin)
            assert agent_rejected.value.code is (
                RuntimeAgentConversationClientErrorCode.SYMLINK_REJECTED
            )
            with pytest.raises(
                console_client.DeveloperLocalInspectionClientError
            ) as inspection_rejected:
                console_client.DeveloperLocalInspectionClientV2._from_tui_attach_pin(inspection_pin)
            assert inspection_rejected.value.code is (
                console_client.DeveloperLocalInspectionClientErrorCode.SYMLINK_REJECTED
            )
        finally:
            agent_socket.close()
            inspection_socket.close()


@pytest.mark.parametrize("socket_pin_case", ["missing", "noncanonical", "duplicate", "wrong"])
def test_inspection_socket_pin_is_unique_canonical_and_same_inode(
    socket_pin_case: str,
) -> None:
    with _private_directory() as directory:
        endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        other = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        socket_path = directory / "inspection.sock"
        other_path = directory / "other.sock"
        endpoint.bind(os.fspath(socket_path))
        other.bind(os.fspath(other_path))
        socket_path.chmod(0o600)
        other_path.chmod(0o600)
        canonical = directory / ".pxi-0123456789abcdef0123456789abcdef-socket.pin"
        try:
            if socket_pin_case == "missing":
                os.link(socket_path, directory / "inspection.anchor")
            elif socket_pin_case == "noncanonical":
                os.link(socket_path, directory / ".pxi-NOTHEX-socket.pin")
            elif socket_pin_case == "duplicate":
                os.link(socket_path, canonical)
                os.link(other_path, directory / ".pxi-fedcba9876543210fedcba9876543210-socket.pin")
            else:
                os.link(socket_path, directory / "inspection.anchor")
                os.link(other_path, canonical)
            bootstrap_path = directory / "inspection.pxib"
            _write_inspection_bootstrap(bootstrap_path, socket_path)
            with pytest.raises(console_client.DeveloperLocalInspectionClientError) as rejected:
                console_client.DeveloperLocalInspectionClientV2.from_private_bootstrap_file(
                    bootstrap_path
                )
            assert rejected.value.code is (
                console_client.DeveloperLocalInspectionClientErrorCode.INVALID_SOCKET
            )
        finally:
            endpoint.close()
            other.close()


def test_unknown_response_status_is_rejected_even_with_a_valid_digest() -> None:
    wire = bytearray(bytes.fromhex(PXAI_WIRE_HEX))
    wire[13] = 0xFF
    digest = _rust_canonical_digest(
        b"paraegox.runtime.agent.developer-local.ipc-frame.sha256.v1",
        (
            (1).to_bytes(2, "big"),
            (0xFF).to_bytes(2, "big"),
            bytes([0x31]) * 16,
            bytes([0x32]) * 32,
            (1_000_000_000).to_bytes(8, "big"),
            bytes([0x33]) * 128,
        ),
    )
    wire[80:112] = digest
    with pytest.raises(RuntimeAgentConversationClientError) as unknown:
        console_client._decode_ipc_frame(b"PXAI", bytes(wire))
    assert unknown.value.code is RuntimeAgentConversationClientErrorCode.UNKNOWN_RESPONSE_STATUS


async def _read_request(reader: asyncio.StreamReader) -> console_client._IpcFrame:
    header = await reader.readexactly(_PXAI_HEADER_BYTES)
    frame_length = int.from_bytes(header[8:12], "big")
    body = await reader.readexactly(frame_length - _PXAI_HEADER_BYTES)
    assert await reader.read(1) == b""
    return console_client._decode_ipc_frame(b"PXAI", header + body)


async def _write_response(
    writer: asyncio.StreamWriter,
    request: console_client._IpcFrame,
    body: bytes,
    *,
    correlation: bytes | None = None,
) -> None:
    response = console_client._IpcFrame(
        kind=request.kind,
        status=console_client._ResponseStatus.OK,
        correlation=request.correlation if correlation is None else correlation,
        generation_token=request.generation_token,
        operation_timeout_nanos=request.operation_timeout_nanos,
        body=body,
    )
    writer.write(console_client._encode_ipc_frame(b"PXAO", response))
    await writer.drain()
    if writer.can_write_eof():
        writer.write_eof()


async def _with_fake_server(
    scenario: Callable[[Path, Path, list[BaseException]], Awaitable[None]],
) -> None:
    with _private_directory() as directory:
        socket_path = directory / "agent.sock"
        bootstrap_path = directory / "agent.bootstrap"
        errors: list[BaseException] = []
        tasks: set[asyncio.Task[None]] = set()

        def start_handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            async def invoke() -> None:
                try:
                    await handler(reader, writer)
                except BaseException as cause:
                    errors.append(cause)
                finally:
                    writer.close()
                    with contextlib_suppress(Exception):
                        await writer.wait_closed()

            task = asyncio.create_task(invoke())
            tasks.add(task)
            task.add_done_callback(tasks.discard)

        handler: Callable[[asyncio.StreamReader, asyncio.StreamWriter], Awaitable[None]]
        handler = scenario.handler  # type: ignore[attr-defined]
        server = await asyncio.start_unix_server(start_handler, path=socket_path)
        socket_path.chmod(0o600)
        _write_bootstrap(bootstrap_path, socket_path)
        try:
            await scenario(bootstrap_path, socket_path, errors)
        finally:
            server.close()
            await server.wait_closed()
            if tasks:
                await asyncio.gather(*tuple(tasks), return_exceptions=True)
        assert not errors


async def _with_fake_inspection_server(
    response_wire: bytes | Callable[[bytes, int], bytes | Awaitable[bytes]],
    scenario: Callable[
        [console_client.DeveloperLocalInspectionClientV2, list[bytes]], Awaitable[None]
    ],
) -> None:
    with _private_directory() as directory:
        socket_path = directory / "inspection.sock"
        bootstrap_path = directory / "inspection.pxib"
        requests: list[bytes] = []
        errors: list[BaseException] = []
        tasks: set[asyncio.Task[None]] = set()

        def start_handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            async def invoke() -> None:
                try:
                    authenticated_request = await reader.readexactly(128)
                    assert await reader.read(1) == b""
                    assert authenticated_request[:32] == _INSPECTION_TOKEN
                    request = authenticated_request[32:]
                    requests.append(request)
                    produced = (
                        response_wire(request, len(requests))
                        if callable(response_wire)
                        else response_wire
                    )
                    response = await produced if inspect.isawaitable(produced) else produced
                    writer.write(len(response).to_bytes(4, "big") + response)
                    await writer.drain()
                    if writer.can_write_eof():
                        writer.write_eof()
                except BaseException as cause:
                    errors.append(cause)
                finally:
                    writer.close()
                    with contextlib_suppress(Exception):
                        await writer.wait_closed()

            task = asyncio.create_task(invoke())
            tasks.add(task)
            task.add_done_callback(tasks.discard)

        server = await asyncio.start_unix_server(start_handler, path=socket_path)
        socket_path.chmod(0o600)
        os.link(
            socket_path,
            directory / ".pxi-0123456789abcdef0123456789abcdef-socket.pin",
        )
        _write_inspection_bootstrap(bootstrap_path, socket_path)
        client = console_client.DeveloperLocalInspectionClientV2.from_private_bootstrap_file(
            bootstrap_path
        )
        try:
            await scenario(client, requests)
        finally:
            client.close()
            server.close()
            await server.wait_closed()
            if tasks:
                await asyncio.gather(*tuple(tasks), return_exceptions=True)
        assert not errors


def test_successful_fake_uds_open_and_submit() -> None:
    async def scenario(
        bootstrap_path: Path,
        _socket_path: Path,
        errors: list[BaseException],
    ) -> None:
        client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        assert await client.open() is AgentConversationOpenOutcomeV1.OPENED
        terminal = await client.submit("hello from Textual")
        assert terminal.outcome is TerminalOutcome.SUCCESS
        assert terminal.output == "echo: hello from Textual"
        client.close()
        assert not errors

    async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        request = await _read_request(reader)
        if request.kind is console_client._OperationKind.OPEN:
            control = decode_control_v1(request.body)
            assert control.kind is AgentConversationControlKindV1.OPEN_REQUEST
            body = AgentConversationControlV1.open_result(
                _DECK_RUN_ID,
                _SESSION_ID,
                AgentConversationOpenOutcomeV1.OPENED,
            ).canonical_wire()
        else:
            assert request.kind is console_client._OperationKind.SUBMIT
            semantic = decode_request_v1(request.body)
            assert semantic.request_id == request.correlation
            body = AgentConversationTerminalV1.success(
                semantic,
                f"echo: {semantic.input}",
            ).canonical_wire()
        await _write_response(writer, request, body)

    scenario.handler = handler  # type: ignore[attr-defined]
    asyncio.run(_with_fake_server(scenario))


def test_exchange_rejects_response_correlation_mismatch() -> None:
    async def scenario(
        bootstrap_path: Path,
        _socket_path: Path,
        _errors: list[BaseException],
    ) -> None:
        client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        with pytest.raises(RuntimeAgentConversationClientError) as mismatch:
            await client.open()
        assert mismatch.value.code is RuntimeAgentConversationClientErrorCode.CORRELATION_MISMATCH
        client.close()

    async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        request = await _read_request(reader)
        body = AgentConversationControlV1.open_result(
            _DECK_RUN_ID,
            _SESSION_ID,
            AgentConversationOpenOutcomeV1.OPENED,
        ).canonical_wire()
        wrong = bytes([request.correlation[0] ^ 1]) + request.correlation[1:]
        await _write_response(writer, request, body, correlation=wrong)

    scenario.handler = handler  # type: ignore[attr-defined]
    asyncio.run(_with_fake_server(scenario))


def test_agent_exchange_revalidates_socket_identity_after_exact_response(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    async def scenario(
        bootstrap_path: Path,
        _socket_path: Path,
        _errors: list[BaseException],
    ) -> None:
        client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        real_validate = console_client._validate_socket_path
        calls = 0

        def changing_identity(
            bootstrap: console_client._BootstrapV1,
        ) -> console_client._FileIdentity:
            nonlocal calls
            calls += 1
            identity = real_validate(bootstrap)
            if calls == 3:
                return replace(identity, inode=identity.inode + 1)
            return identity

        monkeypatch.setattr(console_client, "_validate_socket_path", changing_identity)
        with pytest.raises(RuntimeAgentConversationClientError) as changed:
            await client.open()
        assert changed.value.code is (
            RuntimeAgentConversationClientErrorCode.ENDPOINT_IDENTITY_CHANGED
        )
        assert calls == 3
        client.close()

    async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        request = await _read_request(reader)
        body = AgentConversationControlV1.open_result(
            _DECK_RUN_ID,
            _SESSION_ID,
            AgentConversationOpenOutcomeV1.OPENED,
        ).canonical_wire()
        await _write_response(writer, request, body)

    scenario.handler = handler  # type: ignore[attr-defined]
    asyncio.run(_with_fake_server(scenario))


def test_open_accepts_only_opened_or_existing() -> None:
    async def scenario(
        bootstrap_path: Path,
        _socket_path: Path,
        _errors: list[BaseException],
    ) -> None:
        client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
        with pytest.raises(RuntimeAgentConversationClientError) as rejected:
            await client.open()
        assert rejected.value.code is RuntimeAgentConversationClientErrorCode.OPERATION_REJECTED
        client.close()

    async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        request = await _read_request(reader)
        body = AgentConversationControlV1.open_result(
            _DECK_RUN_ID,
            _SESSION_ID,
            AgentConversationOpenOutcomeV1.DECK_RUN_SEALED,
        ).canonical_wire()
        await _write_response(writer, request, body)

    scenario.handler = handler  # type: ignore[attr-defined]
    asyncio.run(_with_fake_server(scenario))


@pytest.mark.parametrize(
    "cancel_outcome",
    [
        AgentConversationCancelOutcomeV1.INTENT_RECORDED,
        AgentConversationCancelOutcomeV1.INTENT_ALREADY_RECORDED,
    ],
)
def test_cancel_pending_uses_the_exact_active_request(
    cancel_outcome: AgentConversationCancelOutcomeV1,
) -> None:
    async def run() -> None:
        submit_seen = asyncio.Event()
        cancel_seen = asyncio.Event()

        async def scenario(
            bootstrap_path: Path,
            _socket_path: Path,
            _errors: list[BaseException],
        ) -> None:
            client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
            submit = asyncio.create_task(client.submit("cancel this request"))
            await submit_seen.wait()
            result = await client.cancel_pending()
            assert result.outcome is cancel_outcome
            assert result.terminal is None
            terminal = await submit
            assert terminal.outcome is TerminalOutcome.FAILURE
            assert terminal.failure is AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL
            client.close()

        async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            request = await _read_request(reader)
            if request.kind is console_client._OperationKind.SUBMIT:
                semantic = decode_request_v1(request.body)
                submit_seen.set()
                await cancel_seen.wait()
                body = AgentConversationTerminalV1.failed(
                    semantic,
                    AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL,
                ).canonical_wire()
            else:
                assert request.kind is console_client._OperationKind.CANCEL
                control = decode_control_v1(request.body)
                assert control.kind is AgentConversationControlKindV1.CANCEL_REQUEST
                assert control.request_id is not None
                body = AgentConversationControlV1.cancel_result(
                    _DECK_RUN_ID,
                    _SESSION_ID,
                    control.request_id,
                    cancel_outcome,
                ).canonical_wire()
                cancel_seen.set()
            await _write_response(writer, request, body)

        scenario.handler = handler  # type: ignore[attr-defined]
        await _with_fake_server(scenario)

    asyncio.run(run())


def test_cancel_terminal_retires_the_original_submit() -> None:
    async def run() -> None:
        submit_seen = asyncio.Event()
        release_submit_handler = asyncio.Event()
        submitted: list[console_client.AgentConversationRequestV1] = []

        async def scenario(
            bootstrap_path: Path,
            _socket_path: Path,
            _errors: list[BaseException],
        ) -> None:
            client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
            submit = asyncio.create_task(client.submit("cancel before handoff"))
            await submit_seen.wait()

            result = await client.cancel_pending()
            assert result.outcome is AgentConversationCancelOutcomeV1.TERMINAL
            assert result.terminal is not None
            assert result.terminal.failure is (
                AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL
            )
            assert submit.done()
            with pytest.raises(asyncio.CancelledError):
                await submit
            with pytest.raises(RuntimeAgentConversationClientError) as no_pending:
                await client.cancel_pending()
            assert no_pending.value.code is (
                RuntimeAgentConversationClientErrorCode.NO_PENDING_REQUEST
            )
            release_submit_handler.set()
            client.close()

        async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            request = await _read_request(reader)
            if request.kind is console_client._OperationKind.SUBMIT:
                submitted.append(decode_request_v1(request.body))
                submit_seen.set()
                await release_submit_handler.wait()
                return

            assert request.kind is console_client._OperationKind.CANCEL
            control = decode_control_v1(request.body)
            assert control.request_id == submitted[0].request_id
            terminal = AgentConversationTerminalV1.failed(
                submitted[0],
                AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL,
            )
            body = AgentConversationControlV1.cancel_result(
                _DECK_RUN_ID,
                _SESSION_ID,
                submitted[0].request_id,
                AgentConversationCancelOutcomeV1.TERMINAL,
                terminal,
            ).canonical_wire()
            await _write_response(writer, request, body)

        scenario.handler = handler  # type: ignore[attr-defined]
        await _with_fake_server(scenario)

    asyncio.run(run())


@pytest.mark.parametrize(
    ("cancel_outcome", "safe_fragment"),
    [
        (AgentConversationCancelOutcomeV1.NOT_FOUND, "was not found"),
        (AgentConversationCancelOutcomeV1.SESSION_SEALED, "Session is sealed"),
    ],
)
def test_cancel_rejections_are_safe_and_retire_the_original_submit(
    cancel_outcome: AgentConversationCancelOutcomeV1,
    safe_fragment: str,
) -> None:
    async def run() -> None:
        submit_seen = asyncio.Event()
        release_submit_handler = asyncio.Event()
        submitted_request_ids: list[bytes] = []

        async def scenario(
            bootstrap_path: Path,
            socket_path: Path,
            _errors: list[BaseException],
        ) -> None:
            client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(bootstrap_path)
            submit = asyncio.create_task(client.submit("reject cancellation"))
            await submit_seen.wait()

            with pytest.raises(RuntimeAgentConversationClientError) as rejected:
                await client.cancel_pending()
            assert rejected.value.code is RuntimeAgentConversationClientErrorCode.OPERATION_REJECTED
            assert safe_fragment in str(rejected.value)
            assert str(bootstrap_path) not in str(rejected.value)
            assert str(socket_path) not in str(rejected.value)
            assert _GENERATION_TOKEN.hex() not in str(rejected.value)
            assert submit.done()
            with pytest.raises(asyncio.CancelledError):
                await submit
            release_submit_handler.set()
            client.close()

        async def handler(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
            request = await _read_request(reader)
            if request.kind is console_client._OperationKind.SUBMIT:
                submitted_request_ids.append(decode_request_v1(request.body).request_id)
                submit_seen.set()
                await release_submit_handler.wait()
                return

            control = decode_control_v1(request.body)
            assert request.kind is console_client._OperationKind.CANCEL
            assert control.request_id == submitted_request_ids[0]
            body = AgentConversationControlV1.cancel_result(
                _DECK_RUN_ID,
                _SESSION_ID,
                submitted_request_ids[0],
                cancel_outcome,
            ).canonical_wire()
            await _write_response(writer, request, body)

        scenario.handler = handler  # type: ignore[attr-defined]
        await _with_fake_server(scenario)

    asyncio.run(run())


def test_operation_timeout_does_not_wait_for_hung_writer_cleanup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NeverRespondingReader:
        async def readexactly(self, _count: int) -> bytes:
            await asyncio.Event().wait()
            raise AssertionError("unreachable")

    class HangingCleanupWriter:
        def __init__(self) -> None:
            self.closed = False
            self.wait_closed_called = False

        def get_extra_info(self, name: str) -> object | None:
            return object() if name == "socket" else None

        def write(self, _wire: bytes) -> None:
            return None

        async def drain(self) -> None:
            return None

        def can_write_eof(self) -> bool:
            return True

        def write_eof(self) -> None:
            return None

        def close(self) -> None:
            self.closed = True

        async def wait_closed(self) -> None:
            self.wait_closed_called = True
            await asyncio.Event().wait()

    async def run() -> None:
        writer = HangingCleanupWriter()

        async def open_connection(*, path: bytes) -> tuple[NeverRespondingReader, object]:
            assert path == b"/private/runtime-agent.sock"
            return NeverRespondingReader(), writer

        bootstrap = console_client._BootstrapV1(
            socket_path=b"/private/runtime-agent.sock",
            generation_token=bytearray(_GENERATION_TOKEN),
            deck_run_id=_DECK_RUN_ID,
            session_id=_SESSION_ID,
            request_deadline_budget_nanos=_DEADLINE_NANOS,
            operation_timeout_nanos=5_000_000,
            command_capacity=_COMMAND_CAPACITY,
            server_uid=os.geteuid(),
            server_gid=os.getegid(),
        )
        client = RuntimeAgentConversationClientV1(bootstrap, bytes([0x6A]) * 32)
        monkeypatch.setattr(console_client.asyncio, "open_unix_connection", open_connection)
        monkeypatch.setattr(
            console_client,
            "_validate_socket_path",
            lambda _bootstrap: console_client._FileIdentity(1, 2, stat.S_IFSOCK | 0o600),
        )
        monkeypatch.setattr(
            console_client,
            "_peer_credentials",
            lambda _socket: (os.geteuid(), os.getegid()),
        )

        with pytest.raises(RuntimeAgentConversationClientError) as timed_out:
            await asyncio.wait_for(
                client._exchange(
                    console_client._OperationKind.OPEN,
                    bytes([0x31]) * 16,
                    AgentConversationControlV1.open_request(
                        _DECK_RUN_ID, _SESSION_ID
                    ).canonical_wire(),
                ),
                timeout=0.5,
            )
        assert timed_out.value.code is RuntimeAgentConversationClientErrorCode.OPERATION_TIMED_OUT
        assert writer.closed
        assert not writer.wait_closed_called
        client.close()

    asyncio.run(run())


def test_inspection_v2_decodes_all_rust_generated_fixtures(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    expected_bootstrap = console_client._InspectionBootstrapV2(
        socket_path=b"/tmp/inspection.sock",
        projection_id=_INSPECTION_PROJECTION_ID,
        generation_token=bytearray(_INSPECTION_TOKEN),
        server_uid=501,
        server_gid=20,
        operation_timeout_nanos=_INSPECTION_TIMEOUT_NANOS,
        request_seed=bytearray(_INSPECTION_REQUEST_SEED),
    )
    bootstrap_wire = _inspection_fixture("developer_local_inspection_bootstrap_v2.hex")
    assert console_client._encode_inspection_bootstrap_v2(expected_bootstrap) == bootstrap_wire
    monkeypatch.setattr(console_client.os, "geteuid", lambda: 501)
    monkeypatch.setattr(console_client.os, "getegid", lambda: 20)
    bootstrap = console_client._decode_inspection_bootstrap_v2(bootstrap_wire)
    assert bootstrap.socket_path == b"/tmp/inspection.sock"
    assert bootstrap.projection_id == _INSPECTION_PROJECTION_ID

    request_id = console_client._inspection_request_id_v2(bootstrap, 1)
    request = console_client._encode_inspection_latest_request_v2(
        request_id,
        _INSPECTION_PROJECTION_ID,
    )
    request_wire = _inspection_fixture("inspection_latest_request_v2.hex")
    assert request.canonical_wire == request_wire
    assert bytes(bootstrap.generation_token) + request.canonical_wire == _inspection_fixture(
        "developer_local_inspection_authenticated_request_v2.hex"
    )

    snapshot_wire = _inspection_fixture("local_inspection_snapshot_v2.hex")
    snapshot = console_client._decode_local_inspection_snapshot_v2(snapshot_wire)
    assert snapshot.canonical_wire == snapshot_wire
    assert snapshot.projection_revision == 7
    assert snapshot.overall is console_client.LocalInspectionOverallV1.UNKNOWN
    assert tuple(record.owner for record in snapshot.base_snapshot.records) == tuple(
        console_client.InspectionSourceOwnerV1
    )
    assert all(
        record.freshness is console_client.InspectionFreshnessV1.MISSING
        for record in snapshot.base_snapshot.records
    )
    assert snapshot.node.registration_epoch == 31
    assert snapshot.node.status_sequence == 41
    response_snapshot = console_client._decode_inspection_response_v2(
        _inspection_fixture("inspection_snapshot_response_v2.hex"),
        request,
    )
    assert response_snapshot == snapshot
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as unavailable:
        console_client._decode_inspection_response_v2(
            _inspection_fixture("inspection_not_found_response_v2.hex"),
            request,
        )
    assert (
        unavailable.value.code
        is console_client.DeveloperLocalInspectionClientErrorCode.SNAPSHOT_UNAVAILABLE
    )
    with pytest.raises(FrozenInstanceError):
        setattr(snapshot, "overall", console_client.LocalInspectionOverallV1.READY)
    console_client.DeveloperLocalInspectionClientV2(bootstrap).close()


def test_inspection_v2_rejects_digest_owner_order_and_aggregate_corruption() -> None:
    request_wire = _inspection_fixture("inspection_latest_request_v2.hex")
    request = console_client._InspectionRequestV2(
        request_id=request_wire[16:32],
        projection_id=request_wire[32:48],
        kind=console_client._InspectionRequestKindV2.LATEST,
        after_revision=0,
        request_digest=request_wire[64:96],
        canonical_wire=request_wire,
    )
    response = bytearray(_inspection_fixture("inspection_snapshot_response_v2.hex"))
    response[-1] ^= 1
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as digest:
        console_client._decode_inspection_response_v2(bytes(response), request)
    assert (
        digest.value.code is console_client.DeveloperLocalInspectionClientErrorCode.DIGEST_MISMATCH
    )

    correlation = bytearray(_inspection_fixture("inspection_snapshot_response_v2.hex"))
    correlation[24] ^= 1
    correlation[112:144] = _rust_canonical_digest(
        b"paraegox.inspection.protocol-response.v2",
        (bytes(correlation[:112]), bytes(correlation[144:])),
    )
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as mismatch:
        console_client._decode_inspection_response_v2(bytes(correlation), request)
    assert (
        mismatch.value.code
        is console_client.DeveloperLocalInspectionClientErrorCode.CORRELATION_MISMATCH
    )

    reserved = bytearray(_inspection_fixture("local_inspection_snapshot_v2.hex"))
    reserved[71] = 1
    reserved[80:112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v2",
        (bytes(reserved[:80]), bytes(reserved[112:])),
    )
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as noncanonical:
        console_client._decode_local_inspection_snapshot_v2(bytes(reserved))
    assert (
        noncanonical.value.code is console_client.DeveloperLocalInspectionClientErrorCode.PROTOCOL
    )

    aggregate = bytearray(_inspection_fixture("local_inspection_snapshot_v2.hex"))
    aggregate[70] = 1
    aggregate[80:112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v2",
        (bytes(aggregate[:80]), bytes(aggregate[112:])),
    )
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as invalid_aggregate:
        console_client._decode_local_inspection_snapshot_v2(bytes(aggregate))
    assert (
        invalid_aggregate.value.code
        is console_client.DeveloperLocalInspectionClientErrorCode.PROTOCOL
    )

    owner_order = bytearray(_inspection_fixture("local_inspection_snapshot_v2.hex"))
    base_start = 112
    owner_order[base_start + 112] = 2
    owner_order[base_start + 80 : base_start + 112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v1",
        (
            bytes(owner_order[base_start : base_start + 80]),
            bytes(owner_order[base_start + 112 : base_start + 592]),
        ),
    )
    owner_order[80:112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v2",
        (bytes(owner_order[:80]), bytes(owner_order[112:])),
    )
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as invalid_order:
        console_client._decode_local_inspection_snapshot_v2(bytes(owner_order))
    assert (
        invalid_order.value.code is console_client.DeveloperLocalInspectionClientErrorCode.PROTOCOL
    )

    node_coordinate = bytearray(_inspection_fixture("local_inspection_snapshot_v2.hex"))
    node_start = 112 + 592
    node_coordinate[node_start + 40 : node_start + 48] = bytes(8)
    node_coordinate[80:112] = _rust_canonical_digest(
        b"paraegox.inspection.local-snapshot.v2",
        (bytes(node_coordinate[:80]), bytes(node_coordinate[112:])),
    )
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as invalid_node:
        console_client._decode_local_inspection_snapshot_v2(bytes(node_coordinate))
    assert (
        invalid_node.value.code is console_client.DeveloperLocalInspectionClientErrorCode.PROTOCOL
    )


def test_inspection_v2_real_uds_reads_latest_exactly_once_and_closes() -> None:
    async def scenario(
        client: console_client.DeveloperLocalInspectionClientV2,
        requests: list[bytes],
    ) -> None:
        snapshot = await client.latest()
        assert snapshot.projection_revision == 7
        assert snapshot.node.registration_epoch == 31
        assert len(requests) == 1
        assert requests[0][:4] == b"PXIQ"
        assert requests[0][4:6] == (2).to_bytes(2, "big")
        assert requests[0][12] == 1
        assert not any(requests[0][48:64])

        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as reused:
            await client.latest()
        assert (
            reused.value.code is console_client.DeveloperLocalInspectionClientErrorCode.ALREADY_USED
        )
        client.close()
        client.close()
        assert not any(client._bootstrap.generation_token)
        assert not any(client._bootstrap.request_seed)

    asyncio.run(
        _with_fake_inspection_server(
            _inspection_fixture("inspection_snapshot_response_v2.hex"),
            scenario,
        )
    )


def test_inspection_v2_latest_then_watch_tracks_cursor_without_retry_or_queue() -> None:
    revision_eight = _inspection_snapshot_with_revision(8)

    def response_for(request: bytes, sequence: int) -> bytes:
        if sequence == 1:
            assert request[12] == int(console_client._InspectionRequestKindV2.LATEST)
            return _inspection_response(
                request,
                console_client._InspectionResponseOutcomeV2.SNAPSHOT,
                current_revision=7,
                snapshot_wire=_inspection_fixture("local_inspection_snapshot_v2.hex"),
            )
        if sequence == 2:
            assert request[12] == int(console_client._InspectionRequestKindV2.WATCH)
            assert int.from_bytes(request[48:56], "big") == 7
            return _inspection_response(
                request,
                console_client._InspectionResponseOutcomeV2.NOT_MODIFIED,
                current_revision=7,
            )
        assert sequence == 3
        assert int.from_bytes(request[48:56], "big") == 7
        return _inspection_response(
            request,
            console_client._InspectionResponseOutcomeV2.SNAPSHOT,
            current_revision=8,
            snapshot_wire=revision_eight,
        )

    async def scenario(
        client: console_client.DeveloperLocalInspectionClientV2,
        requests: list[bytes],
    ) -> None:
        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as first_watch:
            await client.watch(7)
        assert first_watch.value.code is (
            console_client.DeveloperLocalInspectionClientErrorCode.LATEST_REQUIRED
        )

        latest = await client.latest()
        assert latest.projection_revision == 7
        assert await client.watch(7) is None
        assert client._cursor_revision == 7
        updated = await client.watch(7)
        assert updated is not None
        assert updated.projection_revision == 8
        assert client._cursor_revision == 8
        assert len(requests) == 3
        assert len({request[16:32] for request in requests}) == 3

        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as stale_cursor:
            await client.watch(7)
        assert stale_cursor.value.code is (
            console_client.DeveloperLocalInspectionClientErrorCode.CORRELATION_MISMATCH
        )
        assert len(requests) == 3

    asyncio.run(_with_fake_inspection_server(response_for, scenario))


def test_inspection_v2_rejects_concurrent_watch_and_sends_no_second_exchange() -> None:
    watch_started = asyncio.Event()
    release_watch = asyncio.Event()

    async def response_for(request: bytes, sequence: int) -> bytes:
        if sequence == 1:
            return _inspection_response(
                request,
                console_client._InspectionResponseOutcomeV2.SNAPSHOT,
                current_revision=7,
                snapshot_wire=_inspection_fixture("local_inspection_snapshot_v2.hex"),
            )
        assert sequence == 2
        watch_started.set()
        await release_watch.wait()
        return _inspection_response(
            request,
            console_client._InspectionResponseOutcomeV2.NOT_MODIFIED,
            current_revision=7,
        )

    async def scenario(
        client: console_client.DeveloperLocalInspectionClientV2,
        requests: list[bytes],
    ) -> None:
        await client.latest()
        pending = asyncio.create_task(client.watch(7))
        await watch_started.wait()
        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as concurrent:
            await client.watch(7)
        assert concurrent.value.code is (
            console_client.DeveloperLocalInspectionClientErrorCode.REQUEST_PENDING
        )
        assert len(requests) == 2
        release_watch.set()
        assert await pending is None
        assert len(requests) == 2

    asyncio.run(_with_fake_inspection_server(response_for, scenario))


def test_inspection_v2_watch_not_found_is_terminal_for_that_single_exchange() -> None:
    def response_for(request: bytes, sequence: int) -> bytes:
        if sequence == 1:
            return _inspection_response(
                request,
                console_client._InspectionResponseOutcomeV2.SNAPSHOT,
                current_revision=7,
                snapshot_wire=_inspection_fixture("local_inspection_snapshot_v2.hex"),
            )
        assert sequence == 2
        return _inspection_response(
            request,
            console_client._InspectionResponseOutcomeV2.NOT_FOUND,
            current_revision=0,
        )

    async def scenario(
        client: console_client.DeveloperLocalInspectionClientV2,
        requests: list[bytes],
    ) -> None:
        await client.latest()
        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as unavailable:
            await client.watch(7)
        assert unavailable.value.code is (
            console_client.DeveloperLocalInspectionClientErrorCode.SNAPSHOT_UNAVAILABLE
        )
        assert len(requests) == 2

    asyncio.run(_with_fake_inspection_server(response_for, scenario))


def test_inspection_v2_timeout_attempts_exactly_one_exchange(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class NeverRespondingReader:
        async def readexactly(self, _count: int) -> bytes:
            await asyncio.Event().wait()
            raise AssertionError("unreachable")

    class Writer:
        def __init__(self) -> None:
            self.closed = False
            self.writes = 0

        def get_extra_info(self, name: str) -> object | None:
            return object() if name == "socket" else None

        def write(self, _wire: bytes | bytearray) -> None:
            self.writes += 1

        async def drain(self) -> None:
            return None

        def can_write_eof(self) -> bool:
            return True

        def write_eof(self) -> None:
            return None

        def close(self) -> None:
            self.closed = True

    async def scenario() -> None:
        writer = Writer()
        connection_calls = 0

        async def open_connection(*, path: bytes) -> tuple[NeverRespondingReader, Writer]:
            nonlocal connection_calls
            assert path == b"/private/inspection.sock"
            connection_calls += 1
            return NeverRespondingReader(), writer

        bootstrap = console_client._InspectionBootstrapV2(
            socket_path=b"/private/inspection.sock",
            projection_id=_INSPECTION_PROJECTION_ID,
            generation_token=bytearray(_INSPECTION_TOKEN),
            server_uid=os.geteuid(),
            server_gid=os.getegid(),
            operation_timeout_nanos=5_000_000,
            request_seed=bytearray(_INSPECTION_REQUEST_SEED),
        )
        identity = console_client._FileIdentity(1, 2, stat.S_IFSOCK | 0o600)
        monkeypatch.setattr(console_client.asyncio, "open_unix_connection", open_connection)
        monkeypatch.setattr(
            console_client,
            "_validate_inspection_socket_path",
            lambda _bootstrap: identity,
        )
        monkeypatch.setattr(
            console_client,
            "_peer_credentials",
            lambda _socket: (os.geteuid(), os.getegid()),
        )
        client = console_client.DeveloperLocalInspectionClientV2(bootstrap)
        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as timed_out:
            await client.latest()
        assert timed_out.value.code is (
            console_client.DeveloperLocalInspectionClientErrorCode.OPERATION_TIMED_OUT
        )
        assert connection_calls == 1
        assert writer.writes == 1
        assert writer.closed
        client.close()

    asyncio.run(scenario())


def test_inspection_v2_not_found_and_closed_client_fail_closed() -> None:
    async def not_found_scenario(
        client: console_client.DeveloperLocalInspectionClientV2,
        requests: list[bytes],
    ) -> None:
        with pytest.raises(console_client.DeveloperLocalInspectionClientError) as unavailable:
            await client.latest()
        assert (
            unavailable.value.code
            is console_client.DeveloperLocalInspectionClientErrorCode.SNAPSHOT_UNAVAILABLE
        )
        assert len(requests) == 1

    asyncio.run(
        _with_fake_inspection_server(
            _inspection_fixture("inspection_not_found_response_v2.hex"),
            not_found_scenario,
        )
    )

    bootstrap = console_client._InspectionBootstrapV2(
        socket_path=b"/private/inspection.sock",
        projection_id=_INSPECTION_PROJECTION_ID,
        generation_token=bytearray(_INSPECTION_TOKEN),
        server_uid=os.geteuid(),
        server_gid=os.getegid(),
        operation_timeout_nanos=_INSPECTION_TIMEOUT_NANOS,
        request_seed=bytearray(_INSPECTION_REQUEST_SEED),
    )
    closed = console_client.DeveloperLocalInspectionClientV2(bootstrap)
    closed.close()
    closed.close()
    with pytest.raises(console_client.DeveloperLocalInspectionClientError) as error:
        asyncio.run(closed.latest())
    assert error.value.code is console_client.DeveloperLocalInspectionClientErrorCode.CLOSED
