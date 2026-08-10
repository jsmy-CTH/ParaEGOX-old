from __future__ import annotations

import asyncio
import fcntl
import importlib.util
import os
import struct
import subprocess
import sys
import termios
import tty
from collections.abc import Callable
from pathlib import Path
from types import SimpleNamespace

import pytest
from textual.pilot import Pilot
from textual.widgets import Input, RichLog, Static

import paraegox_sdk.console_client as console_client
import paraegox_sdk.console_tui as console_tui
from paraegox_sdk.agent_worker.control import AgentConversationCancelOutcomeV1
from paraegox_sdk.agent_worker.protocol import (
    MAX_AGENT_CONVERSATION_INPUT_BYTES,
    AgentConversationRequestV1,
    AgentConversationTerminalFailureV1,
    AgentConversationTerminalV1,
)
from paraegox_sdk.console_client import RuntimeAgentConversationCancelResultV1
from paraegox_sdk.console_tui import ParaEGOXConsoleApp, _parse_arguments

_SMOKE_SPEC = importlib.util.spec_from_file_location(
    "paraegox_macos_textual_smoke",
    Path(__file__).parents[2] / "scripts" / "smoke_macos_textual_chat.py",
)
if _SMOKE_SPEC is None or _SMOKE_SPEC.loader is None:
    raise RuntimeError("unable to load the macOS Textual smoke harness")
macos_textual_smoke = importlib.util.module_from_spec(_SMOKE_SPEC)
_SMOKE_SPEC.loader.exec_module(macos_textual_smoke)


def _request(input_text: str, sequence: int) -> AgentConversationRequestV1:
    return AgentConversationRequestV1.create(
        bytes([0x41]) * 16,
        bytes([0x42]) * 16,
        bytes([0x50 + sequence]) * 16,
        bytes([0x60 + sequence]) * 16,
        30_000_000_000,
        input_text,
    )


class FakeConversationClient:
    def __init__(
        self,
        *,
        output: str = "Hello from ParaEGOX",
        terminal_failure: AgentConversationTerminalFailureV1 | None = None,
        open_error: Exception | None = None,
        hold_submit: bool = False,
        cancel_outcome: AgentConversationCancelOutcomeV1 = (
            AgentConversationCancelOutcomeV1.INTENT_RECORDED
        ),
        release_submit_on_cancel: bool = False,
    ) -> None:
        self.output = output
        self.terminal_failure = terminal_failure
        self.open_error = open_error
        self.cancel_outcome = cancel_outcome
        self.release_submit_on_cancel = release_submit_on_cancel
        self.open_calls = 0
        self.submit_calls: list[str] = []
        self.cancel_calls = 0
        self.close_calls = 0
        self.submit_started = asyncio.Event()
        self.submit_release = asyncio.Event()
        if not hold_submit:
            self.submit_release.set()

    async def open(self) -> object:
        self.open_calls += 1
        if self.open_error is not None:
            raise self.open_error
        return object()

    async def submit(self, input_text: str) -> AgentConversationTerminalV1:
        self.submit_calls.append(input_text)
        self.submit_started.set()
        await self.submit_release.wait()
        request = _request(input_text, len(self.submit_calls))
        if self.terminal_failure is not None:
            return AgentConversationTerminalV1.failed(request, self.terminal_failure)
        return AgentConversationTerminalV1.success(request, self.output)

    async def cancel_pending(self) -> RuntimeAgentConversationCancelResultV1:
        self.cancel_calls += 1
        if self.release_submit_on_cancel:
            self.submit_release.set()
        terminal = None
        if self.cancel_outcome is AgentConversationCancelOutcomeV1.TERMINAL:
            input_text = self.submit_calls[-1]
            request = _request(input_text, len(self.submit_calls))
            terminal = (
                AgentConversationTerminalV1.failed(request, self.terminal_failure)
                if self.terminal_failure is not None
                else AgentConversationTerminalV1.success(request, self.output)
            )
        return RuntimeAgentConversationCancelResultV1(self.cancel_outcome, terminal)

    def close(self) -> None:
        self.close_calls += 1


class FakeInspectionClient:
    def __init__(
        self,
        responses: list[console_client.LocalInspectionSnapshotV2 | None | Exception],
    ) -> None:
        self._responses = list(responses)
        self.watch_calls: list[int] = []
        self.watch_start_times: list[float] = []
        self.close_calls = 0
        self.exhausted = asyncio.Event()

    async def watch(
        self,
        after_revision: int,
    ) -> console_client.LocalInspectionSnapshotV2 | None:
        self.watch_calls.append(after_revision)
        self.watch_start_times.append(asyncio.get_running_loop().time())
        if not self._responses:
            self.exhausted.set()
            await asyncio.Event().wait()
            raise AssertionError("unreachable")
        response = self._responses.pop(0)
        if isinstance(response, Exception):
            raise response
        return response

    def close(self) -> None:
        self.close_calls += 1


def _inspection_snapshot(
    revision: int = 7,
) -> console_client.LocalInspectionSnapshotV2:
    records = tuple(
        console_client.LocalInspectionRecordV1(
            owner=owner,
            freshness=console_client.InspectionFreshnessV1.MISSING,
            subject_ref=bytes([0x40 + int(owner)]) * 16,
            coordinate=None,
            observed_at_nanos=None,
            valid_until_nanos=None,
            liveness=console_client.InspectionLivenessV1.UNKNOWN,
            readiness=console_client.InspectionReadinessV1.UNKNOWN,
            health=console_client.InspectionHealthV1.UNKNOWN,
            feature_support=console_client.InspectionFeatureSupportV1.UNKNOWN,
            reason=console_client.InspectionReasonV1.SOURCE_MISSING,
            owner_fact_digest=None,
        )
        for owner in console_client.InspectionSourceOwnerV1
    )
    base = console_client.LocalInspectionSnapshotV1(
        projection_id=bytes([0x21]) * 16,
        observation_clock_ref=bytes([0x31]) * 16,
        projection_revision=revision,
        projected_at_nanos=150,
        overall=console_client.LocalInspectionOverallV1.UNKNOWN,
        records=(records[0], records[1], records[2], records[3], records[4]),
        projection_digest=bytes([0x51]) * 32,
        canonical_wire=bytes(592),
    )
    node = console_client.NodeInspectionRecordV2(
        freshness=console_client.InspectionFreshnessV1.FRESH,
        node_ref=bytes([0x61]) * 16,
        node_incarnation_ref=bytes([0x62]) * 16,
        registration_epoch=31,
        status_sequence=41,
        observed_at_nanos=100,
        valid_until_nanos=200,
        liveness=console_client.InspectionLivenessV1.LIVE,
        readiness=console_client.InspectionReadinessV1.READY,
        health=console_client.InspectionHealthV1.HEALTHY,
        feature_support=console_client.InspectionFeatureSupportV1.ALL_REQUIRED_SUPPORTED,
        reason=console_client.InspectionReasonV1.NONE,
        node_status_digest=bytes([0x63]) * 32,
    )
    return console_client.LocalInspectionSnapshotV2(
        base_snapshot=base,
        node=node,
        overall=console_client.LocalInspectionOverallV1.UNKNOWN,
        projection_digest=bytes([0x71]) * 32,
        canonical_wire=bytes(832),
    )


async def _wait_until(
    pilot: Pilot[None],
    predicate: Callable[[], bool],
    *,
    attempts: int = 30,
) -> None:
    for _ in range(attempts):
        if predicate():
            return
        await pilot.pause()
    raise AssertionError("Textual state did not reach the expected condition")


async def _enter(app: ParaEGOXConsoleApp, pilot: Pilot[None], value: str) -> None:
    chat_input = app.query_one("#chat-input", Input)
    chat_input.value = value
    chat_input.focus()
    await pilot.press("enter")
    await pilot.pause()


def test_console_connects_and_submits_one_successful_turn() -> None:
    async def scenario() -> None:
        client = FakeConversationClient(output="A typed reply")
        app = ParaEGOXConsoleApp(
            client,
            inspection_snapshot=_inspection_snapshot(),
        )
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            status = app.query_one("#connection-status", Static)
            inspection = app.query_one("#inspection-status", Static)
            assert str(status.content) == "Connection: connected · Request: idle"
            assert str(inspection.content).splitlines() == [
                (
                    "Node-local startup snapshot UNKNOWN r7 | NodeDaemon ready · "
                    "registration e31 · status s41"
                ),
                "Authority missing | Deployment missing | Runtime missing",
                "Fabric missing | Agent missing | health unreported",
            ]

            await _enter(app, pilot, "hello")
            await _wait_until(pilot, lambda: "Agent: A typed reply" in app.transcript)

            assert client.open_calls == 1
            assert client.submit_calls == ["hello"]
            assert "You: hello" in app.transcript
            assert not app.conversation_pending
        assert client.close_calls == 1

    asyncio.run(scenario())


def test_console_inspection_watch_is_single_flight_throttled_and_replaces_cache() -> None:
    async def scenario() -> None:
        conversation = FakeConversationClient()
        inspection = FakeInspectionClient([None, _inspection_snapshot(8)])
        app = ParaEGOXConsoleApp(
            conversation,
            inspection_snapshot=_inspection_snapshot(7),
            inspection_client=inspection,
        )
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            await _wait_until(pilot, lambda: len(inspection.watch_calls) == 1)
            assert inspection.watch_calls == [7]
            assert "r7" in str(app.query_one("#inspection-status", Static).content)
            await asyncio.sleep(0.2)
            await pilot.pause()
            assert inspection.watch_calls == [7]

            await asyncio.sleep(0.85)
            await _wait_until(pilot, lambda: len(inspection.watch_calls) == 2)
            assert inspection.watch_calls == [7, 7]
            assert inspection.watch_start_times[1] - inspection.watch_start_times[0] >= 1.0
            await _wait_until(
                pilot,
                lambda: "r8" in str(app.query_one("#inspection-status", Static).content),
            )
            assert app.inspection_available
        assert conversation.close_calls == 1
        assert inspection.close_calls == 1

    asyncio.run(scenario())


def test_console_inspection_failure_latches_unavailable_without_reconnect() -> None:
    async def scenario() -> None:
        conversation = FakeConversationClient(output="conversation remains usable")
        inspection = FakeInspectionClient(
            [
                console_client.DeveloperLocalInspectionClientError(
                    console_client.DeveloperLocalInspectionClientErrorCode.IO,
                    "DeveloperLocal Inspection v2 exchange failed",
                )
            ]
        )
        app = ParaEGOXConsoleApp(
            conversation,
            inspection_snapshot=_inspection_snapshot(),
            inspection_client=inspection,
        )
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            await _wait_until(pilot, lambda: not app.inspection_available)
            assert "Inspection: unavailable" in str(
                app.query_one("#inspection-status", Static).content
            )
            assert inspection.watch_calls == [7]
            await asyncio.sleep(1.05)
            await pilot.pause()
            assert inspection.watch_calls == [7]

            await _enter(app, pilot, "still attached")
            await _wait_until(
                pilot,
                lambda: "Agent: conversation remains usable" in app.transcript,
            )
            assert conversation.submit_calls == ["still attached"]
        assert conversation.close_calls == 1
        assert inspection.close_calls == 1

    asyncio.run(scenario())


def test_console_conversation_failure_does_not_disable_inspection_channel() -> None:
    async def scenario() -> None:
        conversation = FakeConversationClient(
            open_error=console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.GENERATION_RETIRED,
                "Runtime-managed Agent conversation generation is retired",
            )
        )
        inspection = FakeInspectionClient([None])
        app = ParaEGOXConsoleApp(
            conversation,
            inspection_snapshot=_inspection_snapshot(),
            inspection_client=inspection,
        )
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(
                pilot,
                lambda: any("conversation unavailable" in line for line in app.transcript),
            )
            await _wait_until(pilot, lambda: inspection.watch_calls == [7])
            assert not app.connected
            assert app.inspection_available
            assert "Inspection cache" in str(app.query_one("#inspection-status", Static).content)
        assert conversation.close_calls == 1
        assert inspection.close_calls == 1

    asyncio.run(scenario())


def test_console_transcript_is_bounded_and_generic_errors_are_redacted() -> None:
    async def scenario() -> None:
        secret = "/private/bootstrap/API_KEY=secret"
        conversation = FakeConversationClient(open_error=RuntimeError(secret))
        app = ParaEGOXConsoleApp(conversation)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(
                pilot,
                lambda: any("conversation unavailable" in line for line in app.transcript),
            )
            assert secret not in "\n".join(app.transcript)
            for index in range(1_100):
                app._write_line(f"bounded-{index}")
            assert len(app.transcript) == 1_000
            assert app.transcript[0] == "bounded-100"
            assert app.transcript[-1] == "bounded-1099"
        assert conversation.close_calls == 1

    asyncio.run(scenario())


def test_inspection_watch_floor_is_exactly_one_second(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    assert console_tui._inspection_watch_delay(10.0, 10.0) == 1.0
    assert console_tui._inspection_watch_delay(10.0, 10.25) == 0.75
    assert console_tui._inspection_watch_delay(10.0, 11.0) == 0.0
    assert console_tui._inspection_watch_delay(10.0, 12.0) == 0.0

    class EarlyClock:
        def __init__(self) -> None:
            self._times = iter((10.2, 10.7, 11.0))

        def time(self) -> float:
            return next(self._times)

    delays: list[float] = []

    async def early_sleep(delay: float) -> None:
        delays.append(delay)

    monkeypatch.setattr(console_tui.asyncio, "sleep", early_sleep)
    asyncio.run(console_tui._wait_for_inspection_watch_floor(10.0, EarlyClock()))  # type: ignore[arg-type]
    assert delays == pytest.approx([0.8, 0.3])


def test_console_renders_connection_and_terminal_failures_safely() -> None:
    async def connection_failure_scenario() -> None:
        client = FakeConversationClient(
            open_error=console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.OWNER_UNAVAILABLE,
                "Runtime-managed Agent conversation owner is unavailable",
            )
        )
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(
                pilot,
                lambda: any("conversation unavailable" in line for line in app.transcript),
            )
            assert not app.connected
            assert "owner is unavailable" in app.transcript[-1]
            assert str(app.query_one("#connection-status", Static).content).startswith(
                "Connection: unavailable"
            )
            assert app.query_one("#chat-input", Input).disabled
        assert client.close_calls == 1

    async def terminal_failure_scenario() -> None:
        client = FakeConversationClient(
            terminal_failure=AgentConversationTerminalFailureV1.MODEL_OUTCOME_UNCERTAIN
        )
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            await _enter(app, pilot, "do not replay this")
            await _wait_until(
                pilot,
                lambda: any("outcome is uncertain" in line for line in app.transcript),
            )
            assert any("did not replay" in line for line in app.transcript)
            assert not app.conversation_pending
        assert client.close_calls == 1

    asyncio.run(connection_failure_scenario())
    asyncio.run(terminal_failure_scenario())


def test_console_enforces_single_pending_request_and_supports_cancel() -> None:
    async def scenario() -> None:
        client = FakeConversationClient(
            terminal_failure=AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL,
            hold_submit=True,
        )
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            await _enter(app, pilot, "first")
            await _wait_until(pilot, client.submit_started.is_set)
            assert app.conversation_pending

            await _enter(app, pilot, "second")
            assert client.submit_calls == ["first"]
            assert any("one request is already pending" in line for line in app.transcript)

            await _enter(app, pilot, "/cancel")
            await _wait_until(pilot, lambda: client.cancel_calls == 1)
            assert any("cancellation intent was recorded" in line for line in app.transcript)

            client.submit_release.set()
            await _wait_until(pilot, lambda: not app.conversation_pending)
            assert any("cancelled before the model" in line for line in app.transcript)
        assert client.close_calls == 1

    asyncio.run(scenario())


def test_console_consumes_cancel_terminal_once_and_retires_submit() -> None:
    async def scenario() -> None:
        client = FakeConversationClient(
            terminal_failure=AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL,
            hold_submit=True,
            cancel_outcome=AgentConversationCancelOutcomeV1.TERMINAL,
            release_submit_on_cancel=True,
        )
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            await _enter(app, pilot, "cancel race")
            await _wait_until(pilot, client.submit_started.is_set)

            await _enter(app, pilot, "/cancel")
            await _wait_until(pilot, lambda: not app.conversation_pending)
            terminal_lines = [
                line for line in app.transcript if "cancelled before the model" in line
            ]
            assert terminal_lines == [
                "Agent request failed: The request was cancelled before the model started."
            ]
        assert client.close_calls == 1

    asyncio.run(scenario())


def test_console_help_clear_and_quit_are_local_commands() -> None:
    async def scenario() -> None:
        client = FakeConversationClient()
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            await _enter(app, pilot, "/help")
            assert app.transcript[-1] == "System: commands — /help /clear /cancel /quit"

            await _enter(app, pilot, "/clear")
            assert app.transcript == ()
            assert app.query_one("#chat-log", RichLog).lines == []

            chat_input = app.query_one("#chat-input", Input)
            chat_input.value = "/quit"
            chat_input.focus()
            await pilot.press("enter")
            assert client.close_calls == 1
        assert client.close_calls == 1

    asyncio.run(scenario())


def test_console_ctrl_c_binding_closes_client_and_exits() -> None:
    async def scenario() -> None:
        client = FakeConversationClient()
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)

            await pilot.press("ctrl+c")
            await pilot.pause()

            assert client.close_calls == 1
            assert app.return_code == 0
        assert client.close_calls == 1

    asyncio.run(scenario())


def test_console_real_pty_ctrl_c_drains_teardown_and_exits() -> None:
    child_code = (
        "from paraegox_sdk.console_tui import ParaEGOXConsoleApp\n"
        "class Client:\n"
        "    def __init__(self): self.close_calls = 0\n"
        "    async def open(self): return object()\n"
        "    async def submit(self, input_text): raise AssertionError(input_text)\n"
        "    async def cancel_pending(self): raise AssertionError('unexpected cancel')\n"
        "    def close(self): self.close_calls += 1\n"
        "client = Client()\n"
        "app = ParaEGOXConsoleApp(client)\n"
        "app.run()\n"
        "print(f'PX_REAL_PTY_EXIT:{app.return_code}:{client.close_calls}', flush=True)\n"
    )
    master_fd, slave_fd = os.openpty()
    fcntl.ioctl(slave_fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    environment = os.environ.copy()
    environment["TERM"] = "xterm-256color"
    environment.pop("TEXTUAL_ALLOW_SIGNALS", None)
    process = subprocess.Popen(
        [sys.executable, "-c", child_code],
        cwd=Path(__file__).parents[2],
        env=environment,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        start_new_session=True,
    )
    os.close(slave_fd)
    capture = bytearray()
    try:
        macos_textual_smoke._read_until(
            master_fd,
            process,
            capture,
            b"System: connected",
            5.0,
        )
        assert termios.tcgetattr(master_fd)[tty.LFLAG] & termios.ISIG == 0

        os.write(master_fd, b"\x03")
        macos_textual_smoke._wait_for_exit(master_fd, process, capture, 5.0)

        assert b"PX_REAL_PTY_EXIT:0:1" in capture
        assert macos_textual_smoke._TEXTUAL_TERMINAL_RESTORE in capture
    finally:
        macos_textual_smoke._stop_process(process)
        os.close(master_fd)


def test_console_smoke_exit_wait_keeps_timeout_and_nonzero_fail_closed() -> None:
    class RunningProcess:
        returncode = None

        @staticmethod
        def poll() -> None:
            return None

    capture = bytearray()
    reader_fd, writer_fd = os.pipe()
    try:
        os.write(writer_fd, b"PX_TEXTUAL_TEARDOWN_STARTED")
        with pytest.raises(TimeoutError, match="Textual did not restore"):
            macos_textual_smoke._wait_for_exit(
                reader_fd,
                RunningProcess(),
                capture,
                0.05,
            )
    finally:
        os.close(writer_fd)
        os.close(reader_fd)
    assert b"PX_TEXTUAL_TEARDOWN_STARTED" in capture

    reader_fd, writer_fd = os.pipe()
    try:
        with pytest.raises(TimeoutError, match="Rust parent did not join"):
            macos_textual_smoke._wait_for_exit(
                reader_fd,
                RunningProcess(),
                bytearray(macos_textual_smoke._TEXTUAL_TERMINAL_RESTORE),
                0.05,
            )
    finally:
        os.close(writer_fd)
        os.close(reader_fd)

    class FailedProcess:
        returncode = 7

        @staticmethod
        def poll() -> int:
            return 7

    reader_fd, writer_fd = os.pipe()
    os.close(writer_fd)
    try:
        with pytest.raises(RuntimeError, match="unsuccessfully with code 7"):
            macos_textual_smoke._wait_for_exit(
                reader_fd,
                FailedProcess(),
                bytearray(),
                0.05,
            )
    finally:
        os.close(reader_fd)


def test_console_rejects_utf8_input_above_protocol_limit() -> None:
    async def scenario() -> None:
        client = FakeConversationClient()
        app = ParaEGOXConsoleApp(client)
        async with app.run_test(size=(100, 30)) as pilot:
            await _wait_until(pilot, lambda: app.connected)
            oversized = "界" * (MAX_AGENT_CONVERSATION_INPUT_BYTES // 3 + 1)
            assert len(oversized.encode("utf-8")) > MAX_AGENT_CONVERSATION_INPUT_BYTES
            await _enter(app, pilot, oversized)
            assert client.submit_calls == []
            assert "exceeds the 16 KiB protocol limit" in app.transcript[-1]
        assert client.close_calls == 1

    asyncio.run(scenario())


def test_console_cli_accepts_only_explicit_absolute_bootstrap_paths(tmp_path: Path) -> None:
    runtime = tmp_path / "runtime.pxab"
    inspection = tmp_path / "inspection.pxib"
    parsed = _parse_arguments(
        [
            "--runtime-bootstrap-file",
            str(runtime),
            "--inspection-bootstrap-file",
            str(inspection),
        ]
    )
    assert parsed.runtime_bootstrap_file == runtime
    assert parsed.inspection_bootstrap_file == inspection

    invalid_arguments = [
        [],
        ["--runtime-bootstrap-file", "relative.pxab"],
        ["--runtime-bootstrap-file", str(runtime), "unexpected"],
        [
            "--runtime-bootstrap-file",
            str(runtime),
            "--runtime-bootstrap-file",
            str(runtime),
        ],
        [
            "--runtime-bootstrap-file",
            str(runtime),
            "--inspection-bootstrap-file",
            str(runtime),
        ],
    ]
    for arguments in invalid_arguments:
        with pytest.raises(SystemExit) as captured:
            _parse_arguments(arguments)
        assert captured.value.code == 2


def test_hidden_tui_attach_mode_is_exact_silent_and_loads_latest_before_ui(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    events: list[str] = []
    terminal_output = SimpleNamespace(isatty=lambda: True)
    original_driver_output = object()
    conversation_pin = object()
    inspection_pin = object()
    handoff = SimpleNamespace(
        conversation=conversation_pin,
        inspection=inspection_pin,
    )

    class ConversationClient:
        def __init__(self) -> None:
            self.close_calls = 0

        def close(self) -> None:
            self.close_calls += 1

    class ConversationFactory:
        @staticmethod
        def _from_tui_attach_pin(pin: object) -> ConversationClient:
            assert pin is conversation_pin
            events.append("conversation-pin")
            return conversation

    class InspectionClient:
        def __init__(self) -> None:
            self.latest_calls = 0
            self.close_calls = 0

        async def latest(self) -> console_client.LocalInspectionSnapshotV2:
            self.latest_calls += 1
            events.append("latest")
            return snapshot

        def close(self) -> None:
            self.close_calls += 1

    class InspectionFactory:
        @staticmethod
        def _from_tui_attach_pin(pin: object) -> InspectionClient:
            assert pin is inspection_pin
            events.append("inspection-pin")
            return inspection

    class App:
        def __init__(
            self,
            client: ConversationClient,
            *,
            inspection_snapshot: console_client.LocalInspectionSnapshotV2,
            inspection_client: InspectionClient,
        ) -> None:
            assert client is conversation
            assert inspection_snapshot is snapshot
            assert inspection_client is inspection
            events.append("app-init")

        def run(self) -> None:
            assert sys.__stderr__ is terminal_output
            assert sys.__stderr__ is sys.__stdout__
            events.append("app-run")

        def _close_client(self) -> None:
            events.append("app-close")

    conversation = ConversationClient()
    inspection = InspectionClient()
    snapshot = _inspection_snapshot()

    def read_handoff(fd: int) -> SimpleNamespace:
        assert fd == 3
        events.append("handoff")
        return handoff

    monkeypatch.setattr(console_tui, "_read_tui_attach_handoff_fd", read_handoff)
    monkeypatch.setattr(console_tui, "RuntimeAgentConversationClientV1", ConversationFactory)
    monkeypatch.setattr(console_tui, "DeveloperLocalInspectionClientV2", InspectionFactory)
    monkeypatch.setattr(console_tui, "ParaEGOXConsoleApp", App)
    monkeypatch.setattr(sys, "__stdout__", terminal_output)
    monkeypatch.setattr(sys, "__stderr__", original_driver_output)

    invalid = [
        ["--tui-attach-fd"],
        ["--tui-attach-fd", "4"],
        ["--tui-attach-fd=3"],
        ["--tui-attach-fd", "3", "extra"],
        ["--runtime-bootstrap-file", "/tmp/x", "--tui-attach-fd", "3"],
    ]
    for arguments in invalid:
        assert console_tui.main(arguments) == 20
    assert events == []

    assert console_tui.main(["--tui-attach-fd", "3"]) == 0
    assert events == [
        "handoff",
        "conversation-pin",
        "inspection-pin",
        "latest",
        "app-init",
        "app-run",
        "app-close",
    ]
    assert inspection.latest_calls == 1
    assert conversation.close_calls == 1
    assert inspection.close_calls == 1
    assert sys.__stderr__ is original_driver_output
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == ""


def test_hidden_tui_attach_restores_driver_output_after_app_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    terminal_output = SimpleNamespace(isatty=lambda: True)
    original_driver_output = object()

    class FailingApp:
        @staticmethod
        def run() -> None:
            assert sys.__stderr__ is terminal_output
            assert sys.__stderr__ is sys.__stdout__
            raise RuntimeError("private app failure")

    monkeypatch.setattr(sys, "__stdout__", terminal_output)
    monkeypatch.setattr(sys, "__stderr__", original_driver_output)
    with pytest.raises(RuntimeError, match="private app failure"):
        console_tui._run_attached_tui_app(FailingApp())
    assert sys.__stderr__ is original_driver_output


def test_hidden_tui_attach_rejects_missing_or_non_terminal_driver_output(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    terminal_output = SimpleNamespace(isatty=lambda: True)
    non_terminal_output = SimpleNamespace(isatty=lambda: False)
    original_driver_output = object()

    class App:
        @staticmethod
        def run() -> None:
            pytest.fail("the app must not run without both terminal streams")

    for stdout, stderr in (
        (None, original_driver_output),
        (non_terminal_output, original_driver_output),
        (terminal_output, None),
    ):
        with monkeypatch.context() as patch:
            patch.setattr(sys, "__stdout__", stdout)
            patch.setattr(sys, "__stderr__", stderr)
            with pytest.raises(RuntimeError, match="terminal output is unavailable"):
                console_tui._run_attached_tui_app(App())
            assert sys.__stderr__ is stderr


@pytest.mark.parametrize(
    ("error", "expected_exit"),
    [
        (
            console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.INVALID_BOOTSTRAP,
                "safe",
            ),
            21,
        ),
        (
            console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.PEER_CREDENTIALS_MISMATCH,
                "safe",
            ),
            22,
        ),
        (
            console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.INVALID_SOCKET,
                "safe",
            ),
            22,
        ),
        (
            console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.INVALID_FRAME,
                "safe",
            ),
            23,
        ),
        (
            console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.OPERATION_TIMED_OUT,
                "safe",
            ),
            24,
        ),
        (
            console_client.RuntimeAgentConversationClientError(
                console_client.RuntimeAgentConversationClientErrorCode.IO,
                "safe",
            ),
            24,
        ),
    ],
)
def test_hidden_tui_attach_private_exit_mapping_is_silent(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    error: Exception,
    expected_exit: int,
) -> None:
    handoff = SimpleNamespace(conversation=object(), inspection=object())

    class FailingConversationFactory:
        @staticmethod
        def _from_tui_attach_pin(_pin: object) -> object:
            raise error

    monkeypatch.setattr(console_tui, "_read_tui_attach_handoff_fd", lambda _fd: handoff)
    monkeypatch.setattr(
        console_tui,
        "RuntimeAgentConversationClientV1",
        FailingConversationFactory,
    )
    assert console_tui.main(["--tui-attach-fd", "3"]) == expected_exit
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == ""


@pytest.mark.parametrize(
    ("code", "expected_exit"),
    [
        (console_client.DeveloperLocalInspectionClientErrorCode.INVALID_BOOTSTRAP, 21),
        (console_client.DeveloperLocalInspectionClientErrorCode.PEER_CREDENTIALS_MISMATCH, 22),
        (console_client.DeveloperLocalInspectionClientErrorCode.INVALID_SOCKET, 22),
        (console_client.DeveloperLocalInspectionClientErrorCode.CORRELATION_MISMATCH, 23),
        (console_client.DeveloperLocalInspectionClientErrorCode.IO, 24),
    ],
)
def test_hidden_tui_attach_inspection_exit_mapping(
    code: console_client.DeveloperLocalInspectionClientErrorCode,
    expected_exit: int,
) -> None:
    error = console_client.DeveloperLocalInspectionClientError(code, "safe")
    assert console_tui._inspection_private_exit(error) == expected_exit
    assert console_tui._inspection_private_exit(RuntimeError("unclassified")) == 1
    assert console_tui._runtime_private_exit(RuntimeError("unclassified")) == 1

    runtime_bootstrap_race = console_client._TuiAttachAgentBootstrapIdentityError(
        console_client.RuntimeAgentConversationClientErrorCode.ENDPOINT_IDENTITY_CHANGED,
        "safe",
    )
    runtime_socket_race = console_client._TuiAttachAgentSocketError(
        console_client.RuntimeAgentConversationClientErrorCode.IO,
        "safe",
    )
    inspection_bootstrap_race = console_client._TuiAttachInspectionBootstrapIdentityError(
        console_client.DeveloperLocalInspectionClientErrorCode.ENDPOINT_IDENTITY_CHANGED,
        "safe",
    )
    inspection_socket_race = console_client._TuiAttachInspectionSocketError(
        console_client.DeveloperLocalInspectionClientErrorCode.INSECURE_PERMISSIONS,
        "safe",
    )
    assert console_tui._runtime_private_exit(runtime_bootstrap_race) == 21
    assert console_tui._runtime_private_exit(runtime_socket_race) == 22
    assert console_tui._inspection_private_exit(inspection_bootstrap_race) == 21
    assert console_tui._inspection_private_exit(inspection_socket_race) == 22


def test_hidden_tui_attach_handoff_and_initial_not_found_map_without_ui(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    peer_error = console_client._TuiAttachHandoffError(
        console_client._TuiAttachHandoffErrorCode.PEER_CREDENTIALS_MISMATCH,
        "safe",
    )
    monkeypatch.setattr(
        console_tui,
        "_read_tui_attach_handoff_fd",
        lambda _fd: (_ for _ in ()).throw(peer_error),
    )
    assert console_tui.main(["--tui-attach-fd", "3"]) == 22
    io_error = console_client._TuiAttachHandoffError(
        console_client._TuiAttachHandoffErrorCode.IO,
        "safe",
    )
    monkeypatch.setattr(
        console_tui,
        "_read_tui_attach_handoff_fd",
        lambda _fd: (_ for _ in ()).throw(io_error),
    )
    assert console_tui.main(["--tui-attach-fd", "3"]) == 24

    class Conversation:
        def close(self) -> None:
            return None

    class ConversationFactory:
        @staticmethod
        def _from_tui_attach_pin(_pin: object) -> Conversation:
            return Conversation()

    class Inspection:
        async def latest(self) -> console_client.LocalInspectionSnapshotV2:
            raise console_client.DeveloperLocalInspectionClientError(
                console_client.DeveloperLocalInspectionClientErrorCode.SNAPSHOT_UNAVAILABLE,
                "safe",
            )

        def close(self) -> None:
            return None

    class InspectionFactory:
        @staticmethod
        def _from_tui_attach_pin(_pin: object) -> Inspection:
            return Inspection()

    monkeypatch.setattr(
        console_tui,
        "_read_tui_attach_handoff_fd",
        lambda _fd: SimpleNamespace(conversation=object(), inspection=object()),
    )
    monkeypatch.setattr(console_tui, "RuntimeAgentConversationClientV1", ConversationFactory)
    monkeypatch.setattr(console_tui, "DeveloperLocalInspectionClientV2", InspectionFactory)
    monkeypatch.setattr(
        console_tui,
        "ParaEGOXConsoleApp",
        lambda *_args, **_kwargs: pytest.fail("UI must not start before initial Latest"),
    )
    assert console_tui.main(["--tui-attach-fd", "3"]) == 23
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == ""


def test_startup_inspection_loader_reads_once_and_closes(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    snapshot = _inspection_snapshot()

    class FakeInspectionClient:
        def __init__(self) -> None:
            self.latest_calls = 0
            self.close_calls = 0

        async def latest(self) -> console_client.LocalInspectionSnapshotV2:
            self.latest_calls += 1
            return snapshot

        def close(self) -> None:
            self.close_calls += 1

    client = FakeInspectionClient()
    monkeypatch.setattr(
        console_tui.DeveloperLocalInspectionClientV2,
        "from_private_bootstrap_file",
        lambda _path: client,
    )
    loaded = console_tui._load_inspection_snapshot_once(tmp_path / "inspection.pxib")
    assert loaded is snapshot
    assert client.latest_calls == 1
    assert client.close_calls == 1


def test_inspection_startup_failure_closes_agent_client_before_ui(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    agent_client = FakeConversationClient()

    class RuntimeClientFactory:
        @staticmethod
        def from_private_bootstrap_file(_path: Path) -> FakeConversationClient:
            return agent_client

    def fail_inspection(_path: Path) -> console_client.LocalInspectionSnapshotV2:
        raise console_client.DeveloperLocalInspectionClientError(
            console_client.DeveloperLocalInspectionClientErrorCode.SNAPSHOT_UNAVAILABLE,
            "DeveloperLocal Inspection v2 startup snapshot is unavailable",
        )

    monkeypatch.setattr(
        console_tui,
        "RuntimeAgentConversationClientV1",
        RuntimeClientFactory,
    )
    monkeypatch.setattr(console_tui, "_load_inspection_snapshot_once", fail_inspection)
    with pytest.raises(SystemExit) as captured:
        console_tui.main(
            [
                "--runtime-bootstrap-file",
                str(tmp_path / "runtime.pxab"),
                "--inspection-bootstrap-file",
                str(tmp_path / "inspection.pxib"),
            ]
        )
    assert agent_client.close_calls == 1
    assert "startup snapshot is unavailable" in str(captured.value)
    assert str(tmp_path) not in str(captured.value)
