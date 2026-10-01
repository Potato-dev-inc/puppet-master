"""Asynchronous MCP stdio client backed by Qt's QProcess."""

from __future__ import annotations

import json
from typing import Any

from PySide6.QtCore import QObject, QProcess, QTimer, Signal

try:
    from .protocol import ProtocolError, decode_line, encode_line, notification, request
except ImportError:  # Support running gui.py directly from this directory.
    from protocol import ProtocolError, decode_line, encode_line, notification, request


class McpTransport(QObject):
    """Launch and communicate with one local MCP server over JSON-lines stdio.

    The child process is the MCP server only. Disconnect never touches any
    worker or desktop application that the server itself may manage.
    """

    connected = Signal()
    disconnected = Signal()
    tools_received = Signal(list)
    result_received = Signal(int, str, object)
    request_failed = Signal(int, str, str)
    progress_received = Signal(object)
    log_received = Signal(str)

    PROTOCOL_VERSION = "2025-06-18"
    MAX_LINE_BYTES = 4 * 1024 * 1024
    HANDSHAKE_TIMEOUT_MS = 10000

    def __init__(self, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._process: QProcess | None = None
        self._stdout = bytearray()
        self._stderr = bytearray()
        self._next_id = 1
        self._pending: dict[int, dict[str, Any]] = {}
        self._connected = False
        self._closing = False
        self._closed_emitted = False
        self._kill_timer: QTimer | None = None
        self._tools_refresh_pending = False
        self._tools_refresh_dirty = False

    def start(self, program: str, args: list[str] | tuple[str, ...], cwd: str) -> None:
        if self._process is not None and self._process.state() != QProcess.ProcessState.NotRunning:
            raise RuntimeError("MCP process is already running")
        if not isinstance(program, str) or not program.strip():
            raise ValueError("program must be a non-empty path")
        if not all(isinstance(arg, str) for arg in args):
            raise ValueError("all process arguments must be strings")
        if not isinstance(cwd, str) or not cwd.strip():
            raise ValueError("cwd must be a non-empty path")

        self._stdout.clear()
        self._stderr.clear()
        self._closing = False
        self._closed_emitted = False
        self._connected = False
        self._tools_refresh_pending = False
        self._tools_refresh_dirty = False
        proc = QProcess(self)
        proc.setProgram(program)
        proc.setArguments(list(args))
        proc.setWorkingDirectory(cwd)
        proc.setProcessChannelMode(QProcess.ProcessChannelMode.SeparateChannels)
        proc.started.connect(self._on_started)
        proc.readyReadStandardOutput.connect(self._read_stdout)
        proc.readyReadStandardError.connect(self._read_stderr)
        proc.finished.connect(self._on_finished)
        proc.errorOccurred.connect(self._on_process_error)
        self._process = proc
        proc.start()

    def disconnect(self) -> None:
        """Gracefully stop the MCP child, then kill it if it ignores terminate."""
        proc = self._process
        if proc is None or proc.state() == QProcess.ProcessState.NotRunning:
            self._emit_disconnected()
            return
        if self._closing:
            return
        self._closing = True
        self._fail_all("DISCONNECTED", "MCP client disconnected")
        proc.terminate()
        timer = QTimer(self)
        timer.setSingleShot(True)
        timer.timeout.connect(self._kill_if_running)
        self._kill_timer = timer
        timer.start(1500)

    def call_tool(self, name: str, arguments: dict[str, Any], timeout_ms: int = 35000) -> int:
        if not self._connected or self._process is None or self._process.state() == QProcess.ProcessState.NotRunning:
            raise RuntimeError("MCP server is not connected")
        if not isinstance(name, str) or not name.strip():
            raise ValueError("tool name must be a non-empty string")
        if not isinstance(arguments, dict):
            raise ValueError("tool arguments must be an object")
        if not isinstance(timeout_ms, int) or isinstance(timeout_ms, bool) or timeout_ms < 1:
            raise ValueError("timeout_ms must be a positive integer")

        request_id = self._allocate_id()
        timer = QTimer(self)
        timer.setSingleShot(True)
        timer.timeout.connect(lambda rid=request_id: self._on_timeout(rid))
        self._pending[request_id] = {"method": "tools/call", "tool": name, "timer": timer}
        timer.start(timeout_ms)
        try:
            self._send(request(request_id, "tools/call", {
                "name": name,
                "arguments": arguments,
                "_meta": {"progressToken": request_id},
            }))
        except Exception:
            self._take_pending(request_id)
            raise
        return request_id

    def cancel_request(self, request_id: int) -> None:
        pending = self._take_pending(request_id)
        if pending is None:
            return
        message = "Request cancelled"
        try:
            self._send(notification("notifications/cancelled", {
                "requestId": request_id,
                "reason": "Cancelled by user",
            }))
        except RuntimeError as exc:
            message += f"; could not notify MCP server: {exc}"
        self.request_failed.emit(request_id, "CANCELLED", message)

    def refresh_tools(self) -> None:
        """Refresh the button catalog, coalescing duplicate list-changed events."""
        if not self._connected or self._tools_refresh_pending:
            if self._connected and self._tools_refresh_pending:
                self._tools_refresh_dirty = True
            return
        request_id = self._allocate_id()
        self._start_internal_request(request_id, "tools/list", "tools/list", disconnect_on_timeout=False)
        self._tools_refresh_pending = True
        self._tools_refresh_dirty = False
        try:
            self._send(request(request_id, "tools/list"))
        except RuntimeError as exc:
            self._take_pending(request_id)
            self._tools_refresh_pending = False
            self.request_failed.emit(request_id, "PROCESS_UNAVAILABLE", str(exc))

    def _allocate_id(self) -> int:
        result = self._next_id
        self._next_id += 1
        return result

    def _on_started(self) -> None:
        init_id = self._allocate_id()
        self._start_internal_request(init_id, "initialize", "initialize", disconnect_on_timeout=True)
        try:
            self._send(request(init_id, "initialize", {
                "protocolVersion": self.PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": True}},
                "clientInfo": {"name": "puppet-master-qt", "version": "0.1.0"},
            }))
        except RuntimeError as exc:
            self._take_pending(init_id)
            self.request_failed.emit(init_id, "PROCESS_UNAVAILABLE", str(exc))
            self.disconnect()

    def _read_stdout(self) -> None:
        if self._process is None:
            return
        self._stdout.extend(bytes(self._process.readAllStandardOutput()))
        if len(self._stdout) > self.MAX_LINE_BYTES and b"\n" not in self._stdout:
            self._stdout.clear()
            self.log_received.emit("MCP protocol error: response line exceeded 4 MiB")
            self.disconnect()
            return
        while b"\n" in self._stdout:
            line, _, remaining = self._stdout.partition(b"\n")
            self._stdout = bytearray(remaining)
            if len(line) > self.MAX_LINE_BYTES:
                self.log_received.emit("MCP protocol error: response line exceeded 4 MiB")
                self.disconnect()
                return
            self._handle_line(line.rstrip(b"\r"))

    def _read_stderr(self) -> None:
        if self._process is None:
            return
        self._stderr.extend(bytes(self._process.readAllStandardError()))
        while b"\n" in self._stderr:
            line, _, remaining = self._stderr.partition(b"\n")
            self._stderr = bytearray(remaining)
            if len(line) > self.MAX_LINE_BYTES:
                line = line[:self.MAX_LINE_BYTES] + b" [stderr line truncated]"
            message = line.decode("utf-8", errors="replace").rstrip("\r")
            if message:
                self.log_received.emit(message)
        if len(self._stderr) > self.MAX_LINE_BYTES:
            message = self._stderr[:self.MAX_LINE_BYTES].decode("utf-8", errors="replace")
            self._stderr.clear()
            self.log_received.emit(message + " [stderr line truncated]")

    def _handle_line(self, line: bytes) -> None:
        if not line.strip():
            return
        try:
            message = decode_line(line)
        except ProtocolError as exc:
            self.log_received.emit(f"MCP protocol error: {exc}")
            return
        if "method" in message:
            self._handle_notification(message)
        else:
            self._handle_response(message)

    def _handle_notification(self, message: dict[str, Any]) -> None:
        method = message["method"]
        params = message.get("params", {})
        if method == "notifications/progress":
            self.progress_received.emit(params)
        elif method == "notifications/tools/list_changed":
            self.log_received.emit("MCP tool catalog changed; refreshing buttons")
            self.refresh_tools()
        elif method == "notifications/message":
            self.log_received.emit(str(params.get("data", params.get("message", params))))
        else:
            self.log_received.emit(f"MCP notification {method}: {json.dumps(params, ensure_ascii=False)}")

    def _handle_response(self, message: dict[str, Any]) -> None:
        request_id = message["id"]
        if not isinstance(request_id, int) or isinstance(request_id, bool):
            self.log_received.emit(f"MCP protocol error: unexpected response id {request_id!r}")
            return
        pending = self._take_pending(request_id)
        if pending is None:
            self.log_received.emit(f"MCP response for unknown request {request_id}")
            return
        method = pending["method"]
        if "error" in message:
            error = message["error"]
            code = str(error.get("code", "RPC_ERROR"))
            text = str(error.get("message", "MCP request failed"))
            if method == "tools/list":
                self._tools_refresh_pending = False
                self._tools_refresh_dirty = False
            if method == "initialize":
                self.request_failed.emit(request_id, code, text)
                self.disconnect()
            elif method == "tools/list":
                self.request_failed.emit(request_id, code, text)
            else:
                self.request_failed.emit(request_id, code, text)
            return

        result = message["result"]
        if method == "initialize":
            if not isinstance(result, dict) or not isinstance(result.get("protocolVersion"), str):
                self.request_failed.emit(request_id, "INVALID_INITIALIZE_RESULT", "MCP initialize response has no protocolVersion")
                self.disconnect()
                return
            self._send(notification("notifications/initialized"))
            self._connected = True
            self.connected.emit()
            list_id = self._allocate_id()
            self._start_internal_request(list_id, "tools/list", "tools/list", disconnect_on_timeout=True)
            self._tools_refresh_pending = True
            try:
                self._send(request(list_id, "tools/list"))
            except RuntimeError as exc:
                self._take_pending(list_id)
                self.request_failed.emit(list_id, "PROCESS_UNAVAILABLE", str(exc))
                self.disconnect()
        elif method == "tools/list":
            self._tools_refresh_pending = False
            refresh_again = self._tools_refresh_dirty
            self._tools_refresh_dirty = False
            tools = result.get("tools") if isinstance(result, dict) else None
            if not isinstance(tools, list):
                self.request_failed.emit(request_id, "INVALID_TOOLS_RESULT", "MCP tools/list response has no tools array")
                if refresh_again:
                    self.refresh_tools()
                return
            self.tools_received.emit(tools)
            if refresh_again:
                self.refresh_tools()
        else:
            self.result_received.emit(request_id, pending["tool"], result)

    def _on_timeout(self, request_id: int) -> None:
        pending = self._take_pending(request_id)
        if pending is None:
            return
        method = pending["method"]
        if pending.get("disconnect_on_timeout", method == "initialize"):
            message = f"MCP {method} handshake deadline exceeded"
            self.log_received.emit(message)
            self.request_failed.emit(request_id, "TIMEOUT", message)
            self.disconnect()
            return
        if method == "tools/list":
            self._tools_refresh_pending = False
            self._tools_refresh_dirty = False
            self.request_failed.emit(request_id, "TIMEOUT", "MCP tools/list refresh deadline exceeded")
            return
        message = f"{pending['tool']} exceeded its deadline"
        try:
            self._send(notification("notifications/cancelled", {
                "requestId": request_id,
                "reason": "Client deadline exceeded",
            }))
        except RuntimeError as exc:
            message += f"; could not notify MCP server: {exc}"
        self.request_failed.emit(request_id, "TIMEOUT", message)

    def _start_internal_request(self, request_id: int, method: str, tool_name: str,
                                disconnect_on_timeout: bool = False) -> None:
        timer = QTimer(self)
        timer.setSingleShot(True)
        timer.timeout.connect(lambda rid=request_id: self._on_timeout(rid))
        self._pending[request_id] = {"method": method, "tool": tool_name, "timer": timer,
                                     "disconnect_on_timeout": disconnect_on_timeout}
        timer.start(self.HANDSHAKE_TIMEOUT_MS)

    def _take_pending(self, request_id: int) -> dict[str, Any] | None:
        pending = self._pending.pop(request_id, None)
        if pending is not None and pending["timer"] is not None:
            pending["timer"].stop()
            pending["timer"].deleteLater()
        return pending

    def _send(self, message: dict[str, Any]) -> None:
        proc = self._process
        if proc is None or proc.state() == QProcess.ProcessState.NotRunning:
            raise RuntimeError("MCP server process is not running")
        payload = encode_line(message)
        written = proc.write(payload)
        if written < 0:
            raise RuntimeError("failed to write to MCP server stdin")

    def _on_process_error(self, error: QProcess.ProcessError) -> None:
        proc = self._process
        if proc is None or error != QProcess.ProcessError.FailedToStart:
            return
        message = proc.errorString()
        self.log_received.emit(f"Could not start MCP server: {message}")
        self._fail_all("PROCESS_START_FAILED", message)
        self._emit_disconnected()

    def _on_finished(self, exit_code: int, _exit_status: QProcess.ExitStatus) -> None:
        self._read_stdout()
        self._read_stderr()
        if self._stdout.strip():
            tail = bytes(self._stdout)
            self._stdout.clear()
            self._handle_line(tail)
        if self._stderr:
            tail_text = self._stderr.decode("utf-8", errors="replace").strip()
            self._stderr.clear()
            if tail_text:
                self.log_received.emit(tail_text)
        self._fail_all("PROCESS_EXITED", f"MCP server exited with code {exit_code}")
        if self._kill_timer is not None:
            self._kill_timer.stop()
            self._kill_timer.deleteLater()
            self._kill_timer = None
        self._connected = False
        self._emit_disconnected()

    def _kill_if_running(self) -> None:
        if self._process is not None and self._process.state() != QProcess.ProcessState.NotRunning:
            self._process.kill()

    def _fail_all(self, code: str, message: str) -> None:
        self._tools_refresh_pending = False
        self._tools_refresh_dirty = False
        for request_id, pending in list(self._pending.items()):
            self._take_pending(request_id)
            if pending["method"] not in ("initialize", "tools/list"):
                self.request_failed.emit(request_id, code, message)
        self._pending.clear()

    def _emit_disconnected(self) -> None:
        if not self._closed_emitted:
            self._closed_emitted = True
            self._connected = False
            self.disconnected.emit()
