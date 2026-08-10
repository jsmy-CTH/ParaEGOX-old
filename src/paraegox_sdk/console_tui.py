"""Minimal Textual console for the typed ParaEGOX conversation client.

This presentation owner receives one Runtime-issued private Agent bootstrap and
may read one separate immutable Inspection startup snapshot before the App
starts. It does not open Zenoh, select a model, read an API key, or own
conversation identity, projection, or retry policy.
"""

from __future__ import annotations

import argparse
import asyncio
import sys
from collections import deque
from collections.abc import Sequence
from pathlib import Path
from typing import Protocol

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import Footer, Input, RichLog, Static
from textual.worker import Worker

from paraegox_sdk.agent_worker.control import AgentConversationCancelOutcomeV1
from paraegox_sdk.agent_worker.protocol import (
    MAX_AGENT_CONVERSATION_INPUT_BYTES,
    AgentConversationTerminalFailureV1,
    AgentConversationTerminalV1,
    TerminalOutcome,
)
from paraegox_sdk.console_client import (
    DeveloperLocalInspectionClientError,
    DeveloperLocalInspectionClientErrorCode,
    DeveloperLocalInspectionClientV2,
    InspectionFreshnessV1,
    InspectionHealthV1,
    InspectionLivenessV1,
    InspectionReadinessV1,
    LocalInspectionOverallV1,
    LocalInspectionRecordV1,
    LocalInspectionSnapshotV2,
    NodeInspectionRecordV2,
    RuntimeAgentConversationCancelResultV1,
    RuntimeAgentConversationClientError,
    RuntimeAgentConversationClientErrorCode,
    RuntimeAgentConversationClientV1,
    _read_tui_attach_handoff_fd,
    _TuiAttachAgentBootstrapIdentityError,
    _TuiAttachAgentSocketError,
    _TuiAttachHandoffError,
    _TuiAttachHandoffErrorCode,
    _TuiAttachInspectionBootstrapIdentityError,
    _TuiAttachInspectionSocketError,
)


class _ConversationClient(Protocol):
    async def open(self) -> object: ...

    async def submit(self, input_text: str) -> AgentConversationTerminalV1: ...

    async def cancel_pending(self) -> RuntimeAgentConversationCancelResultV1: ...

    def close(self) -> None: ...


class _InspectionClient(Protocol):
    async def watch(self, after_revision: int) -> LocalInspectionSnapshotV2 | None: ...

    def close(self) -> None: ...


_MAX_TRANSCRIPT_LINES = 1_000
_INSPECTION_WATCH_FLOOR_SECONDS = 1.0
_TUI_ATTACH_FD = 3
_TUI_ATTACH_HANDOFF_EXIT = 20
_TUI_ATTACH_BOOTSTRAP_EXIT = 21
_TUI_ATTACH_PEER_EXIT = 22
_TUI_ATTACH_PROTOCOL_EXIT = 23
_TUI_ATTACH_IO_EXIT = 24
_TUI_ATTACH_CHILD_EXIT = 1


_FAILURE_MESSAGES = {
    AgentConversationTerminalFailureV1.MODEL_FAILED: "The model request failed.",
    AgentConversationTerminalFailureV1.DEADLINE_EXCEEDED: "The request deadline expired.",
    AgentConversationTerminalFailureV1.REQUEST_CONFLICT: (
        "The request conflicts with an existing request."
    ),
    AgentConversationTerminalFailureV1.CAPACITY_EXHAUSTED: (
        "The conversation service is currently busy."
    ),
    AgentConversationTerminalFailureV1.MODEL_OUTCOME_UNCERTAIN: (
        "The model outcome is uncertain; ParaEGOX did not replay the request."
    ),
    AgentConversationTerminalFailureV1.CANCELLED_BEFORE_MODEL: (
        "The request was cancelled before the model started."
    ),
}


class ParaEGOXConsoleApp(App[None]):
    """One-session chat UI over a caller-provided typed conversation client."""

    TITLE = "ParaEGOX Agent Chat"
    CSS = """
    Screen {
        layout: vertical;
        background: #071015;
        color: #d7e5e9;
    }

    #title {
        height: 3;
        padding: 1 2 0 2;
        color: #72e1c2;
        text-style: bold;
    }

    #connection-status {
        height: 1;
        padding: 0 2;
        color: #9fb8c0;
    }

    #inspection-status {
        height: 3;
        padding: 0 2;
        color: #9fb8c0;
    }

    #chat-log {
        height: 1fr;
        margin: 1 2;
        padding: 1 2;
        border: round #245b64;
        background: #0b171d;
        scrollbar-color: #2c7f7b;
    }

    #chat-input {
        height: 3;
        margin: 0 2 1 2;
        border: round #2c7f7b;
        background: #102229;
    }

    #chat-input:focus {
        border: round #72e1c2;
    }
    """
    BINDINGS = [
        Binding("ctrl+c", "request_exit", "Quit", priority=True),
        Binding("escape", "request_exit", "Quit", show=False, priority=True),
    ]

    def __init__(
        self,
        client: _ConversationClient,
        *,
        inspection_snapshot: LocalInspectionSnapshotV2 | None = None,
        inspection_client: _InspectionClient | None = None,
    ) -> None:
        super().__init__()
        self._client = client
        self._inspection_client = inspection_client
        self._inspection_snapshot = inspection_snapshot
        self._connected = False
        self._conversation_unavailable = False
        self._inspection_unavailable = False
        self._pending = False
        self._cancel_requested = False
        self._clients_closed = False
        self._transcript: deque[str] = deque(maxlen=_MAX_TRANSCRIPT_LINES)
        self._next_request_generation = 1
        self._pending_generation: int | None = None
        self._submit_worker: Worker[None] | None = None
        self._inspection_worker: Worker[None] | None = None
        self._last_rendered_terminal_key: tuple[bytes, bytes, bytes, bytes] | None = None

    @property
    def transcript(self) -> tuple[str, ...]:
        """Return display-safe transcript lines for deterministic UI evidence."""

        return tuple(self._transcript)

    @property
    def conversation_pending(self) -> bool:
        return self._pending

    @property
    def connected(self) -> bool:
        return self._connected

    @property
    def inspection_available(self) -> bool:
        return not self._inspection_unavailable and self._inspection_snapshot is not None

    def compose(self) -> ComposeResult:
        yield Static("ParaEGOX Agent Chat", id="title")
        yield Static("Connection: connecting · Request: idle", id="connection-status")
        inspection_status = (
            "Inspection: no startup snapshot"
            if self._inspection_snapshot is None
            else "\n".join(
                _inspection_status_lines(
                    self._inspection_snapshot,
                    live=self._inspection_client is not None,
                )
            )
        )
        yield Static(inspection_status, id="inspection-status")
        yield RichLog(
            id="chat-log",
            wrap=True,
            markup=False,
            highlight=False,
            max_lines=_MAX_TRANSCRIPT_LINES,
        )
        yield Input(
            placeholder="Message ParaEGOX, or enter /help",
            id="chat-input",
            disabled=True,
        )
        yield Footer()

    def on_mount(self) -> None:
        self._write_line("System: connecting to the Runtime-managed Agent conversation service…")
        self.run_worker(
            self._open_client(),
            name="open typed conversation client",
            group="connection",
            exclusive=True,
            exit_on_error=False,
        )
        if self._inspection_client is not None:
            self._inspection_worker = self.run_worker(
                self._watch_inspection(),
                name="watch typed Inspection client",
                group="inspection-watch",
                exclusive=True,
                exit_on_error=False,
            )

    async def _open_client(self) -> None:
        try:
            await self._client.open()
        except asyncio.CancelledError:
            raise
        except Exception as error:
            self._connected = False
            self._conversation_unavailable = True
            self._write_line(
                f"System: conversation unavailable — {_display_safe_conversation_error(error)}"
            )
        else:
            if not self._clients_closed:
                self._connected = True
                chat_input = self.query_one("#chat-input", Input)
                chat_input.disabled = False
                chat_input.focus()
                self._write_line("System: connected. Enter /help for local console commands.")
        finally:
            self._refresh_status()

    async def _watch_inspection(self) -> None:
        client = self._inspection_client
        snapshot = self._inspection_snapshot
        if client is None or snapshot is None:
            self._mark_inspection_unavailable(
                "the typed Inspection startup snapshot is unavailable"
            )
            return
        previous_start: float | None = None
        loop = asyncio.get_running_loop()
        while not self._clients_closed and not self._inspection_unavailable:
            if previous_start is not None:
                await _wait_for_inspection_watch_floor(previous_start, loop)
                if self._clients_closed:
                    return
            previous_start = loop.time()
            cursor = snapshot.projection_revision
            try:
                updated = await client.watch(cursor)
            except asyncio.CancelledError:
                raise
            except Exception as error:
                self._mark_inspection_unavailable(_display_safe_inspection_operation_error(error))
                return
            if updated is not None:
                snapshot = updated
                self._inspection_snapshot = updated
                self._refresh_inspection_status()

    def _mark_inspection_unavailable(self, message: str) -> None:
        self._inspection_unavailable = True
        try:
            self.query_one("#inspection-status", Static).update(
                f"Inspection: unavailable — {message}"
            )
        except Exception:
            # The App can be unmounting while one final one-shot Watch resolves.
            pass

    def _refresh_inspection_status(self) -> None:
        if self._inspection_unavailable:
            return
        snapshot = self._inspection_snapshot
        if snapshot is None:
            self._mark_inspection_unavailable(
                "the typed Inspection startup snapshot is unavailable"
            )
            return
        self.query_one("#inspection-status", Static).update(
            "\n".join(_inspection_status_lines(snapshot, live=True))
        )

    async def on_input_submitted(self, event: Input.Submitted) -> None:
        entered = event.value
        event.input.value = ""
        command = entered.strip()
        if not command:
            return
        if command.startswith("/"):
            await self._handle_command(command)
            return
        if not self._connected:
            self._write_line("System: the conversation service is not connected.")
            return
        if self._pending:
            self._write_line("System: one request is already pending; wait or enter /cancel.")
            return
        if len(entered.encode("utf-8")) > MAX_AGENT_CONVERSATION_INPUT_BYTES:
            self._write_line(
                "System: message rejected; UTF-8 input exceeds the 16 KiB protocol limit."
            )
            return

        self._pending = True
        self._cancel_requested = False
        generation = self._next_request_generation
        self._next_request_generation += 1
        self._pending_generation = generation
        self._write_line(f"You: {entered}")
        self._refresh_status()
        self._submit_worker = self.run_worker(
            self._submit(entered, generation),
            name="submit conversation turn",
            group="conversation-submit",
            exclusive=True,
            exit_on_error=False,
        )

    async def _handle_command(self, command: str) -> None:
        if command == "/help":
            self._write_line("System: commands — /help /clear /cancel /quit")
        elif command == "/clear":
            self.query_one("#chat-log", RichLog).clear()
            self._transcript.clear()
        elif command == "/cancel":
            self._request_cancel()
        elif command == "/quit":
            self.action_request_exit()
        else:
            self._write_line(f"System: unknown command {command}; enter /help.")

    def _request_cancel(self) -> None:
        if not self._pending:
            self._write_line("System: there is no pending request to cancel.")
            return
        if self._cancel_requested:
            self._write_line("System: cancellation was already requested.")
            return
        self._cancel_requested = True
        generation = self._pending_generation
        if generation is None:
            self._cancel_requested = False
            self._write_line("System: there is no pending request to cancel.")
            return
        self._write_line("System: requesting cancellation…")
        self._refresh_status()
        self.run_worker(
            self._cancel_pending(generation),
            name="cancel pending conversation turn",
            group="conversation-cancel",
            exclusive=True,
            exit_on_error=False,
        )

    async def _cancel_pending(self, generation: int) -> None:
        try:
            result = await self._client.cancel_pending()
        except asyncio.CancelledError:
            raise
        except Exception as error:
            if self._pending_generation == generation:
                self._cancel_requested = False
            self._write_line(
                f"System: cancellation failed — {_display_safe_conversation_error(error)}"
            )
            if _conversation_error_makes_unavailable(error):
                self._mark_conversation_unavailable()
        else:
            if result.outcome is AgentConversationCancelOutcomeV1.INTENT_RECORDED:
                self._write_line(
                    "System: cancellation intent was recorded; awaiting the terminal result."
                )
            elif result.outcome is AgentConversationCancelOutcomeV1.INTENT_ALREADY_RECORDED:
                self._write_line(
                    "System: cancellation intent was already recorded; "
                    "awaiting the terminal result."
                )
            else:
                terminal = result.terminal
                if terminal is None:
                    self._write_line("System: cancellation returned an invalid terminal result.")
                else:
                    self._render_terminal(terminal)
                if self._pending_generation == generation:
                    submit_worker = self._submit_worker
                    self._pending = False
                    self._cancel_requested = False
                    self._pending_generation = None
                    self._submit_worker = None
                    if submit_worker is not None and not submit_worker.is_finished:
                        submit_worker.cancel()
        finally:
            self._refresh_status()

    async def _submit(self, input_text: str, generation: int) -> None:
        try:
            terminal = await self._client.submit(input_text)
        except asyncio.CancelledError:
            raise
        except Exception as error:
            self._write_line(f"System: request failed — {_display_safe_conversation_error(error)}")
            if _conversation_error_makes_unavailable(error):
                self._mark_conversation_unavailable()
        else:
            self._render_terminal(terminal)
        finally:
            if self._pending_generation == generation:
                self._pending = False
                self._cancel_requested = False
                self._pending_generation = None
                self._submit_worker = None
                self._refresh_status()

    def _render_terminal(self, terminal: AgentConversationTerminalV1) -> bool:
        terminal_key = (
            terminal.deck_run_id,
            terminal.session_id,
            terminal.request_id,
            terminal.request_digest,
        )
        if terminal_key == self._last_rendered_terminal_key:
            return False
        self._last_rendered_terminal_key = terminal_key
        if terminal.outcome is TerminalOutcome.SUCCESS and terminal.output is not None:
            self._write_line(f"Agent: {terminal.output}")
        elif terminal.outcome is TerminalOutcome.FAILURE and terminal.failure is not None:
            message = _FAILURE_MESSAGES.get(
                terminal.failure,
                "The request ended with an unknown terminal failure.",
            )
            self._write_line(f"Agent request failed: {message}")
        else:
            self._write_line("Agent request failed: the terminal response was inconsistent.")
        return True

    def _write_line(self, line: str) -> None:
        self._transcript.append(line)
        self.query_one("#chat-log", RichLog).write(line)

    def _refresh_status(self) -> None:
        if self._conversation_unavailable:
            connection = "unavailable"
        elif self._connected:
            connection = "connected"
        else:
            connection = "disconnected"
        if self._pending and self._cancel_requested:
            request = "cancelling"
        elif self._pending:
            request = "pending"
        else:
            request = "idle"
        self.query_one("#connection-status", Static).update(
            f"Connection: {connection} · Request: {request}"
        )

    def _mark_conversation_unavailable(self) -> None:
        self._conversation_unavailable = True
        self._connected = False
        try:
            self.query_one("#chat-input", Input).disabled = True
        except Exception:
            pass
        self._refresh_status()

    def action_request_exit(self) -> None:
        self._close_client()
        self.exit()

    def on_unmount(self) -> None:
        self._close_client()

    def _close_client(self) -> None:
        if self._clients_closed:
            return
        self._clients_closed = True
        if self._submit_worker is not None and not self._submit_worker.is_finished:
            self._submit_worker.cancel()
        if self._inspection_worker is not None and not self._inspection_worker.is_finished:
            self._inspection_worker.cancel()
        try:
            self._client.close()
        except Exception:
            # Shutdown is best-effort at the presentation boundary. The typed
            # client owns transport cleanup and is required to make close
            # idempotent; the TUI neither retries nor exposes private details.
            pass
        if self._inspection_client is not None:
            try:
                self._inspection_client.close()
            except Exception:
                pass


def _inspection_watch_delay(previous_start: float, current_time: float) -> float:
    elapsed = max(0.0, current_time - previous_start)
    return max(0.0, _INSPECTION_WATCH_FLOOR_SECONDS - elapsed)


async def _wait_for_inspection_watch_floor(
    previous_start: float,
    loop: asyncio.AbstractEventLoop,
) -> None:
    while (delay := _inspection_watch_delay(previous_start, loop.time())) > 0:
        await asyncio.sleep(delay)


def _inspection_status_lines(
    snapshot: LocalInspectionSnapshotV2,
    *,
    live: bool = False,
) -> tuple[str, str, str]:
    node = snapshot.node
    coordinate = (
        ""
        if node.registration_epoch is None or node.status_sequence is None
        else (f" · registration e{node.registration_epoch} · status s{node.status_sequence}")
    )
    records = snapshot.base_snapshot.records
    snapshot_label = "Inspection cache" if live else "Node-local startup snapshot"
    return (
        (
            f"{snapshot_label} {_overall_label(snapshot.overall)} "
            f"r{snapshot.projection_revision} | NodeDaemon {_projected_node_label(node)}"
            f"{coordinate}"
        ),
        (
            f"Authority {_projected_source_label(records[0])} | "
            f"Deployment {_projected_source_label(records[1])} | "
            f"Runtime {_projected_source_label(records[2])}"
        ),
        (
            f"Fabric {_projected_source_label(records[3])} | "
            f"Agent {_projected_source_label(records[4])} | "
            f"{_five_owner_health_label(records)}"
        ),
    )


def _overall_label(overall: LocalInspectionOverallV1) -> str:
    return {
        LocalInspectionOverallV1.READY: "READY",
        LocalInspectionOverallV1.DEGRADED: "DEGRADED",
        LocalInspectionOverallV1.UNAVAILABLE: "UNAVAILABLE",
        LocalInspectionOverallV1.UNKNOWN: "UNKNOWN",
    }[overall]


def _projected_source_label(record: LocalInspectionRecordV1) -> str:
    if record.freshness is InspectionFreshnessV1.STALE:
        return "stale"
    if record.freshness is InspectionFreshnessV1.PARTITIONED:
        return "partitioned"
    if record.freshness is InspectionFreshnessV1.MISSING:
        return "missing"
    return _current_state_label(record.readiness, record.liveness)


def _projected_node_label(record: NodeInspectionRecordV2) -> str:
    if record.freshness is InspectionFreshnessV1.STALE:
        return "stale"
    if record.freshness is InspectionFreshnessV1.PARTITIONED:
        return "partitioned"
    if record.freshness is InspectionFreshnessV1.MISSING:
        return "missing"
    return _current_state_label(record.readiness, record.liveness)


def _current_state_label(
    readiness: InspectionReadinessV1,
    liveness: InspectionLivenessV1,
) -> str:
    if readiness is InspectionReadinessV1.READY and liveness is InspectionLivenessV1.LIVE:
        return "ready"
    if readiness is InspectionReadinessV1.READY and liveness is InspectionLivenessV1.UNKNOWN:
        return "recorded-ready"
    return {
        InspectionReadinessV1.NOT_READY: "not-ready",
        InspectionReadinessV1.DEGRADED: "degraded",
        InspectionReadinessV1.BLOCKED: "blocked",
        InspectionReadinessV1.UNKNOWN: "unknown",
        InspectionReadinessV1.READY: "invalid-ready",
    }[readiness]


def _five_owner_health_label(
    records: tuple[
        LocalInspectionRecordV1,
        LocalInspectionRecordV1,
        LocalInspectionRecordV1,
        LocalInspectionRecordV1,
        LocalInspectionRecordV1,
    ],
) -> str:
    if all(record.health is InspectionHealthV1.UNKNOWN for record in records):
        return "health unreported"
    healthy = sum(record.health is InspectionHealthV1.HEALTHY for record in records)
    degraded = sum(record.health is InspectionHealthV1.DEGRADED for record in records)
    faulted = sum(record.health is InspectionHealthV1.FAULTED for record in records)
    return f"health {healthy} healthy/{degraded} degraded/{faulted} faulted"


def _display_safe_conversation_error(error: Exception) -> str:
    if isinstance(error, RuntimeAgentConversationClientError):
        return str(error)
    return "the typed conversation operation failed"


def _display_safe_inspection_error(error: Exception) -> str:
    if isinstance(error, DeveloperLocalInspectionClientError):
        return str(error)
    return "the typed Inspection startup read failed"


def _display_safe_inspection_operation_error(error: Exception) -> str:
    if isinstance(error, DeveloperLocalInspectionClientError):
        return str(error)
    return "the typed Inspection operation failed"


def _conversation_error_makes_unavailable(error: Exception) -> bool:
    if not isinstance(error, RuntimeAgentConversationClientError):
        return True
    return error.code not in {
        RuntimeAgentConversationClientErrorCode.OPERATION_REJECTED,
        RuntimeAgentConversationClientErrorCode.OVERLOADED,
        RuntimeAgentConversationClientErrorCode.REQUEST_PENDING,
        RuntimeAgentConversationClientErrorCode.NO_PENDING_REQUEST,
    }


class _StorePathOnce(argparse.Action):
    def __call__(
        self,
        parser: argparse.ArgumentParser,
        namespace: argparse.Namespace,
        values: Path,
        option_string: str | None = None,
    ) -> None:
        if getattr(namespace, self.dest, None) is not None:
            parser.error(f"{option_string} may be provided only once")
        setattr(namespace, self.dest, values)


def _absolute_path(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute() or ".." in path.parts:
        raise argparse.ArgumentTypeError("bootstrap file path must be absolute and normalized")
    return path


def _parse_arguments(arguments: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="paraegox-console",
        description="Run the ParaEGOX typed local Agent chat console.",
        allow_abbrev=False,
    )
    parser.add_argument(
        "--runtime-bootstrap-file",
        required=True,
        type=_absolute_path,
        action=_StorePathOnce,
        help="absolute owner-private Runtime Agent IPC bootstrap path",
    )
    parser.add_argument(
        "--inspection-bootstrap-file",
        type=_absolute_path,
        action=_StorePathOnce,
        help="optional absolute owner-private Inspection v2 bootstrap path",
    )
    parsed = parser.parse_args(arguments)
    if (
        parsed.inspection_bootstrap_file is not None
        and parsed.inspection_bootstrap_file == parsed.runtime_bootstrap_file
    ):
        parser.error("Runtime and Inspection bootstrap paths must differ")
    return parsed


def _load_inspection_snapshot_once(path: Path) -> LocalInspectionSnapshotV2:
    inspection_client = DeveloperLocalInspectionClientV2.from_private_bootstrap_file(path)
    try:
        return asyncio.run(inspection_client.latest())
    finally:
        inspection_client.close()


_RUNTIME_BOOTSTRAP_FAILURES = frozenset(
    {
        RuntimeAgentConversationClientErrorCode.INVALID_PATH,
        RuntimeAgentConversationClientErrorCode.SYMLINK_REJECTED,
        RuntimeAgentConversationClientErrorCode.INSECURE_PERMISSIONS,
        RuntimeAgentConversationClientErrorCode.BOOTSTRAP_OPEN_FAILED,
        RuntimeAgentConversationClientErrorCode.INVALID_BOOTSTRAP,
        RuntimeAgentConversationClientErrorCode.DIGEST_MISMATCH,
    }
)
_INSPECTION_BOOTSTRAP_FAILURES = frozenset(
    {
        DeveloperLocalInspectionClientErrorCode.INVALID_PATH,
        DeveloperLocalInspectionClientErrorCode.SYMLINK_REJECTED,
        DeveloperLocalInspectionClientErrorCode.INSECURE_PERMISSIONS,
        DeveloperLocalInspectionClientErrorCode.BOOTSTRAP_OPEN_FAILED,
        DeveloperLocalInspectionClientErrorCode.INVALID_BOOTSTRAP,
        DeveloperLocalInspectionClientErrorCode.DIGEST_MISMATCH,
    }
)


def _runtime_private_exit(error: Exception) -> int:
    if isinstance(error, _TuiAttachAgentBootstrapIdentityError):
        return _TUI_ATTACH_BOOTSTRAP_EXIT
    if isinstance(error, _TuiAttachAgentSocketError):
        return _TUI_ATTACH_PEER_EXIT
    if not isinstance(error, RuntimeAgentConversationClientError):
        return _TUI_ATTACH_CHILD_EXIT
    if error.code is RuntimeAgentConversationClientErrorCode.PEER_CREDENTIALS_MISMATCH:
        return _TUI_ATTACH_PEER_EXIT
    if error.code in {
        RuntimeAgentConversationClientErrorCode.INVALID_SOCKET,
        RuntimeAgentConversationClientErrorCode.ENDPOINT_IDENTITY_CHANGED,
    }:
        return _TUI_ATTACH_PEER_EXIT
    if error.code in _RUNTIME_BOOTSTRAP_FAILURES:
        return _TUI_ATTACH_BOOTSTRAP_EXIT
    if error.code in {
        RuntimeAgentConversationClientErrorCode.IO,
        RuntimeAgentConversationClientErrorCode.OPERATION_TIMED_OUT,
        RuntimeAgentConversationClientErrorCode.ENTROPY_UNAVAILABLE,
    }:
        return _TUI_ATTACH_IO_EXIT
    return _TUI_ATTACH_PROTOCOL_EXIT


def _inspection_private_exit(error: Exception) -> int:
    if isinstance(error, _TuiAttachInspectionBootstrapIdentityError):
        return _TUI_ATTACH_BOOTSTRAP_EXIT
    if isinstance(error, _TuiAttachInspectionSocketError):
        return _TUI_ATTACH_PEER_EXIT
    if not isinstance(error, DeveloperLocalInspectionClientError):
        return _TUI_ATTACH_CHILD_EXIT
    if error.code is DeveloperLocalInspectionClientErrorCode.PEER_CREDENTIALS_MISMATCH:
        return _TUI_ATTACH_PEER_EXIT
    if error.code in {
        DeveloperLocalInspectionClientErrorCode.INVALID_SOCKET,
        DeveloperLocalInspectionClientErrorCode.ENDPOINT_IDENTITY_CHANGED,
    }:
        return _TUI_ATTACH_PEER_EXIT
    if error.code in _INSPECTION_BOOTSTRAP_FAILURES:
        return _TUI_ATTACH_BOOTSTRAP_EXIT
    if error.code in {
        DeveloperLocalInspectionClientErrorCode.IO,
        DeveloperLocalInspectionClientErrorCode.OPERATION_TIMED_OUT,
    }:
        return _TUI_ATTACH_IO_EXIT
    return _TUI_ATTACH_PROTOCOL_EXIT


def _run_tui_attach() -> int:
    try:
        handoff = _read_tui_attach_handoff_fd(_TUI_ATTACH_FD)
    except _TuiAttachHandoffError as error:
        if error.code is _TuiAttachHandoffErrorCode.PEER_CREDENTIALS_MISMATCH:
            return _TUI_ATTACH_PEER_EXIT
        if error.code is _TuiAttachHandoffErrorCode.IO:
            return _TUI_ATTACH_IO_EXIT
        return _TUI_ATTACH_HANDOFF_EXIT
    except Exception:
        return _TUI_ATTACH_CHILD_EXIT

    conversation_pin = handoff.conversation
    inspection_pin = handoff.inspection
    del handoff
    try:
        conversation_client = RuntimeAgentConversationClientV1._from_tui_attach_pin(
            conversation_pin
        )
    except Exception as error:
        return _runtime_private_exit(error)
    del conversation_pin
    try:
        inspection_client = DeveloperLocalInspectionClientV2._from_tui_attach_pin(inspection_pin)
    except Exception as error:
        conversation_client.close()
        return _inspection_private_exit(error)
    del inspection_pin
    try:
        try:
            inspection_snapshot = asyncio.run(inspection_client.latest())
        except Exception as error:
            return _inspection_private_exit(error)
        app = ParaEGOXConsoleApp(
            conversation_client,
            inspection_snapshot=inspection_snapshot,
            inspection_client=inspection_client,
        )
        try:
            app.run()
        except Exception:
            return _TUI_ATTACH_CHILD_EXIT
        finally:
            app._close_client()
        return 0
    finally:
        conversation_client.close()
        inspection_client.close()


def main(arguments: Sequence[str] | None = None) -> int:
    argv = tuple(sys.argv[1:] if arguments is None else arguments)
    hidden_intent = any(
        argument == "--tui-attach-fd" or argument.startswith("--tui-attach-fd=")
        for argument in argv
    )
    if hidden_intent:
        if argv != ("--tui-attach-fd", str(_TUI_ATTACH_FD)):
            return _TUI_ATTACH_HANDOFF_EXIT
        return _run_tui_attach()

    parsed = _parse_arguments(argv)
    try:
        client = RuntimeAgentConversationClientV1.from_private_bootstrap_file(
            parsed.runtime_bootstrap_file
        )
    except Exception as error:
        raise SystemExit(
            "paraegox-console: unable to load Runtime bootstrap — "
            f"{_display_safe_conversation_error(error)}"
        ) from None

    inspection_snapshot = None
    if parsed.inspection_bootstrap_file is not None:
        try:
            inspection_snapshot = _load_inspection_snapshot_once(parsed.inspection_bootstrap_file)
        except Exception as error:
            client.close()
            raise SystemExit(
                "paraegox-console: unable to load Inspection startup snapshot — "
                f"{_display_safe_inspection_error(error)}"
            ) from None

    app = ParaEGOXConsoleApp(
        client,
        inspection_snapshot=inspection_snapshot,
    )
    try:
        app.run()
    finally:
        app._close_client()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
