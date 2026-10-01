"""Transport state-machine tests use tiny Qt doubles, so unittest needs no GUI install."""

import json
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch


class BoundSignal:
    def __init__(self):
        self.handlers = []
        self.values = []

    def connect(self, fn):
        self.handlers.append(fn)

    def emit(self, *args):
        self.values.append(args)
        for handler in list(self.handlers):
            handler(*args)


class SignalDescriptor:
    def __set_name__(self, owner, name):
        self.name = "_signal_" + name

    def __get__(self, obj, owner):
        if obj is None:
            return self
        if not hasattr(obj, self.name):
            setattr(obj, self.name, BoundSignal())
        return getattr(obj, self.name)


class FakeQObject:
    def __init__(self, parent=None):
        self.parent = parent


class FakeTimer(FakeQObject):
    instances = []

    def __init__(self, parent=None):
        super().__init__(parent)
        self.timeout = BoundSignal()
        self.active = False
        self.__class__.instances.append(self)

    def setSingleShot(self, _value): pass
    def start(self, _milliseconds): self.active = True
    def stop(self): self.active = False
    def deleteLater(self): pass
    def fire(self):
        if self.active:
            self.active = False
            self.timeout.emit()


class FakeProcess(FakeQObject):
    class ProcessState:
        NotRunning = 0
        Running = 1

    class ProcessChannelMode:
        SeparateChannels = 0

    class ProcessError:
        FailedToStart = 0

    class ExitStatus:
        NormalExit = 0

    def __init__(self, parent=None):
        super().__init__(parent)
        self.started = BoundSignal()
        self.readyReadStandardOutput = BoundSignal()
        self.readyReadStandardError = BoundSignal()
        self.finished = BoundSignal()
        self.errorOccurred = BoundSignal()
        self.stdout = bytearray()
        self.stderr = bytearray()
        self.writes = []
        self._state = self.ProcessState.NotRunning
        self.terminated = False
        self.killed = False

    def setProgram(self, program): self.program = program
    def setArguments(self, args): self.args = args
    def setWorkingDirectory(self, cwd): self.cwd = cwd
    def setProcessChannelMode(self, mode): self.channel_mode = mode
    def start(self):
        self._state = self.ProcessState.Running
        self.started.emit()
    def state(self): return self._state
    def write(self, data):
        self.writes.append(bytes(data))
        return len(data)
    def readAllStandardOutput(self):
        data = bytes(self.stdout)
        self.stdout.clear()
        return data
    def readAllStandardError(self):
        data = bytes(self.stderr)
        self.stderr.clear()
        return data
    def errorString(self): return "fake process error"
    def terminate(self): self.terminated = True
    def kill(self):
        self.killed = True
        self._state = self.ProcessState.NotRunning

    def feed_stdout(self, data):
        self.stdout.extend(data)
        self.readyReadStandardOutput.emit()

    def feed_stderr(self, data):
        self.stderr.extend(data)
        self.readyReadStandardError.emit()


def load_transport():
    qtcore = types.ModuleType("PySide6.QtCore")
    qtcore.QObject = FakeQObject
    qtcore.QProcess = FakeProcess
    qtcore.QTimer = FakeTimer
    qtcore.Signal = lambda *_args: SignalDescriptor()
    pyside = types.ModuleType("PySide6")
    pyside.QtCore = qtcore
    qtcore.__package__ = "PySide6"
    original = sys.modules.pop("transport", None)
    with patch.dict(sys.modules, {"PySide6": pyside, "PySide6.QtCore": qtcore}):
        sys.path.insert(0, str(Path(__file__).parent))
        try:
            import transport
            return transport
        finally:
            sys.path.pop(0)
            sys.modules.pop("transport", None)
            if original is not None:
                sys.modules["transport"] = original


class TransportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.transport_module = load_transport()
        cls.McpTransport = cls.transport_module.McpTransport

    def setUp(self):
        FakeTimer.instances.clear()
        self.client = self.McpTransport()
        self.client.start("python", ["server.py"], ".")
        self.process = self.client._process
        self._respond(self.process.writes[-1], {"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {}})
        self.assertTrue(self.client._connected)
        self._respond(self.process.writes[-1], {"tools": [{"name": "delegate_work"}]})

    @staticmethod
    def _decode_written(line):
        return json.loads(line)

    def _respond(self, request_line, result):
        request_obj = self._decode_written(request_line)
        reply = {"jsonrpc": "2.0", "id": request_obj["id"], "result": result}
        self.process.feed_stdout(json.dumps(reply).encode() + b"\n")

    def test_initializes_lists_tools_and_accepts_partial_lines(self):
        init = self._decode_written(self.process.writes[0])
        self.assertEqual(init["method"], "initialize")
        self.assertEqual(init["params"]["protocolVersion"], "2025-06-18")
        self.assertTrue(init["params"]["capabilities"]["tools"]["listChanged"])
        initialized = self._decode_written(self.process.writes[1])
        self.assertEqual(initialized["method"], "notifications/initialized")
        self.assertEqual(self.client.tools_received.values[-1][0][0]["name"], "delegate_work")

        request_id = self.client.call_tool("get_operation", {"operation_id": "op-1"})
        data = json.dumps({"jsonrpc": "2.0", "id": request_id, "result": {"ok": True}}).encode() + b"\n"
        self.process.feed_stdout(data[:5])
        self.assertFalse(self.client.result_received.values)
        self.process.feed_stdout(data[5:])
        self.assertEqual(self.client.result_received.values[-1], (request_id, "get_operation", {"ok": True}))

    def test_progress_and_stderr_are_forwarded_as_events(self):
        progress = {"progressToken": 44, "progress": 2, "message": "running"}
        self.process.feed_stdout(json.dumps({"jsonrpc": "2.0", "method": "notifications/progress", "params": progress}).encode() + b"\n")
        self.assertEqual(self.client.progress_received.values[-1][0], progress)
        self.process.feed_stderr(b"bridge log\n")
        self.assertEqual(self.client.log_received.values[-1], ("bridge log",))

    def test_tool_list_changed_refreshes_and_publishes_new_catalog(self):
        event = {"jsonrpc": "2.0", "method": "notifications/tools/list_changed", "params": {}}
        self.process.feed_stdout(json.dumps(event).encode() + b"\n")
        first = self._decode_written(self.process.writes[-1])
        self.assertEqual(first["method"], "tools/list")
        self.assertTrue(self.client._tools_refresh_pending)
        self.process.feed_stdout(json.dumps(event).encode() + b"\n")
        self.assertEqual(sum(self._decode_written(row).get("method") == "tools/list" for row in self.process.writes), 2)
        self._respond(self.process.writes[-1], {"tools": [{"name": "run_agent"}, {"name": "set_mode"}]})
        self.assertEqual(self._decode_written(self.process.writes[-1])["method"], "tools/list")
        self._respond(self.process.writes[-1], {"tools": [{"name": "run_agent"}, {"name": "set_mode"}]})
        self.assertEqual([tool["name"] for tool in self.client.tools_received.values[-1][0]], ["run_agent", "set_mode"])
        self.assertFalse(self.client._tools_refresh_pending)

    def test_tool_catalog_refresh_timeout_keeps_connection_and_can_retry(self):
        self.client.refresh_tools()
        first_request = self._decode_written(self.process.writes[-1])
        self.client._pending[first_request["id"]]["timer"].fire()
        self.assertTrue(self.client._connected)
        self.assertEqual(self.client.request_failed.values[-1][1], "TIMEOUT")
        self.client.refresh_tools()
        second_request = self._decode_written(self.process.writes[-1])
        self.assertEqual(second_request["method"], "tools/list")
        self.assertNotEqual(first_request["id"], second_request["id"])

    def test_cancel_sends_cancellation_notification_and_clears_pending(self):
        request_id = self.client.call_tool("wait_for_operation", {"operation_id": "op-1"})
        self.client.cancel_request(request_id)
        cancel = self._decode_written(self.process.writes[-1])
        self.assertEqual(cancel["method"], "notifications/cancelled")
        self.assertEqual(cancel["params"]["requestId"], request_id)
        self.assertEqual(self.client.request_failed.values[-1], (request_id, "CANCELLED", "Request cancelled"))
        self.assertNotIn(request_id, self.client._pending)

    def test_deadline_notifies_server_and_reports_timeout(self):
        request_id = self.client.call_tool("wait_for_operation", {}, timeout_ms=15)
        timer = self.client._pending[request_id]["timer"]
        timer.fire()
        self.assertEqual(self._decode_written(self.process.writes[-1])["method"], "notifications/cancelled")
        self.assertEqual(self.client.request_failed.values[-1][1], "TIMEOUT")
        self.assertNotIn(request_id, self.client._pending)

    def test_disconnect_terminates_only_mcp_child(self):
        self.client.disconnect()
        self.assertTrue(self.process.terminated)
        self.assertFalse(self.process.killed)
        self.process._state = FakeProcess.ProcessState.NotRunning
        self.process.finished.emit(0, FakeProcess.ExitStatus.NormalExit)
        self.assertEqual(len(self.client.disconnected.values), 1)

    def test_validates_process_and_tool_arguments(self):
        for args in (("", [], "."), ("python", [1], "."), ("python", [], "")):
            client = self.McpTransport()
            with self.subTest(args=args), self.assertRaises(ValueError):
                client.start(*args)
        for args in (("", {}, 10), ("tool", [], 10), ("tool", {}, 0), ("tool", {}, True)):
            with self.subTest(args=args), self.assertRaises(ValueError):
                self.client.call_tool(*args)
        client = self.McpTransport()
        with self.assertRaises(RuntimeError):
            client.call_tool("tool", {})
        with self.assertRaises(RuntimeError):
            self.client.start("python", [], ".")

    def test_logs_protocol_messages_and_ignores_unknown_responses(self):
        self.client._handle_line(b"not-json")
        self.client._handle_line(b"\n")
        self.client._handle_line(json.dumps({
            "jsonrpc": "2.0", "method": "notifications/message", "params": {"data": "server says hi"}
        }).encode())
        self.client._handle_line(json.dumps({
            "jsonrpc": "2.0", "method": "notifications/custom", "params": {"x": 1}
        }).encode())
        self.client._handle_line(json.dumps({"jsonrpc": "2.0", "id": 999, "result": {}}).encode())
        self.assertTrue(any("invalid JSON-RPC" in row[0] for row in self.client.log_received.values))
        self.assertIn(("server says hi",), self.client.log_received.values)
        self.assertTrue(any("notifications/custom" in row[0] for row in self.client.log_received.values))
        self.assertTrue(any("unknown request 999" in row[0] for row in self.client.log_received.values))

    def test_invalid_initialize_result_and_failed_start_emit_errors(self):
        client = self.McpTransport()
        client.start("python", [], ".")
        client._handle_response({"jsonrpc": "2.0", "id": 1, "result": {}})
        self.assertEqual(client.request_failed.values[-1][1], "INVALID_INITIALIZE_RESULT")
        self.assertTrue(client._process.terminated)

        failed = self.McpTransport()
        failed.start("missing-executable", [], ".")
        failed._process.errorOccurred.emit(FakeProcess.ProcessError.FailedToStart)
        self.assertEqual(failed.request_failed.values, [])
        self.assertTrue(failed.disconnected.values)
        self.assertTrue(any("Could not start MCP server" in row[0] for row in failed.log_received.values))

    def test_handshake_deadlines_disconnect_and_report(self):
        for respond_to_init in (False, True):
            with self.subTest(respond_to_init=respond_to_init):
                client = self.McpTransport()
                client.start("python", [], ".")
                if respond_to_init:
                    client._handle_response({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": "2025-06-18"}})
                    request_id = 2
                    expected = "tools/list"
                else:
                    request_id = 1
                    expected = "initialize"
                timer = client._pending[request_id]["timer"]
                timer.fire()
                self.assertEqual(client.request_failed.values[-1][1:], ("TIMEOUT", f"MCP {expected} handshake deadline exceeded"))
                self.assertTrue(client._process.terminated)

    def test_cancel_after_child_exit_reports_without_raising(self):
        request_id = self.client.call_tool("wait_for_operation", {})
        self.process._state = FakeProcess.ProcessState.NotRunning
        self.client.cancel_request(request_id)
        self.assertEqual(self.client.request_failed.values[-1][0:2], (request_id, "CANCELLED"))
        self.assertIn("could not notify MCP server", self.client.request_failed.values[-1][2])

    def test_oversize_protocol_line_disconnects_cleanly(self):
        self.process.feed_stdout(b"x" * (self.client.MAX_LINE_BYTES + 1) + b"\n")
        self.assertTrue(self.process.terminated)
        self.assertTrue(any("response line exceeded" in row[0] for row in self.client.log_received.values))

    def test_failed_write_clears_pending_call(self):
        original_write = self.process.write
        self.process.write = lambda _data: -1
        with self.assertRaises(RuntimeError):
            self.client.call_tool("delegate_work", {})
        self.assertFalse(self.client._pending)
        self.process.write = original_write


if __name__ == "__main__":
    unittest.main()
