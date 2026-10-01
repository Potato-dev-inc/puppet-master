"""End-to-end QProcess transport checks with a local mock JSON-RPC child."""

import json
import os
import sys
import threading
import unittest
from pathlib import Path

from PySide6.QtCore import QCoreApplication, QEventLoop, QTimer

from transport import McpTransport


MOCK_SERVER = r'''
import json, os, sys, threading, time
lock = threading.Lock()
def send(obj, split=False):
    data = (json.dumps(obj, separators=(",", ":")) + "\n").encode()
    with lock:
        if split:
            middle = max(1, len(data) // 2)
            os.write(1, data[:middle]); time.sleep(.02); os.write(1, data[middle:])
        else:
            os.write(1, data)
def response(i, result): send({"jsonrpc":"2.0", "id":i, "result":result}, split=True)
for line in sys.stdin:
    try: msg = json.loads(line)
    except Exception: continue
    method = msg.get("method")
    if method == "initialize":
        response(msg["id"], {"protocolVersion":"2025-06-18", "capabilities":{}, "serverInfo":{"name":"mock","version":"1"}})
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        response(msg["id"], {"tools":[{"name":"progress"}, {"name":"rpc_error"}, {"name":"slow"}, {"name":"exit_worker"}]})
    elif method == "notifications/cancelled":
        print("CANCELLED:" + str(msg.get("params", {}).get("requestId")), file=sys.stderr, flush=True)
    elif method == "tools/call":
        name = msg["params"]["name"]
        if name == "progress":
            send({"jsonrpc":"2.0", "method":"notifications/progress", "params":{"progressToken":msg["params"]["_meta"]["progressToken"], "progress":3, "message":"working"}})
            response(msg["id"], {"structuredContent":{"operation_id":"op-7", "revision":3}, "isError":False})
        elif name == "rpc_error":
            send({"jsonrpc":"2.0", "id":msg["id"], "error":{"code":-32000, "message":"mock failure"}})
        elif name == "slow":
            threading.Timer(.8, lambda: response(msg["id"], {"ok":True})).start()
        elif name == "exit_worker":
            os._exit(7)
'''


class QProcessIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QCoreApplication.instance() or QCoreApplication([])

    def wait_until(self, predicate, timeout_ms=2500):
        if predicate():
            return True
        loop = QEventLoop()
        timer = QTimer()
        timer.setSingleShot(True)
        timer.timeout.connect(loop.quit)
        timer.start(timeout_ms)
        poll = QTimer()
        poll.timeout.connect(lambda: loop.quit() if predicate() else None)
        poll.start(5)
        loop.exec()
        poll.stop()
        timer.stop()
        return predicate()

    def test_full_mcp_stdio_lifecycle(self):
        transport = McpTransport()
        self.addCleanup(lambda: self._stop(transport))
        events = {name: [] for name in (
            "connected", "disconnected", "tools", "results", "failures", "progress", "logs"
        )}
        signal_map = {
            "connected": transport.connected,
            "disconnected": transport.disconnected,
            "tools": transport.tools_received,
            "results": transport.result_received,
            "failures": transport.request_failed,
            "progress": transport.progress_received,
            "logs": transport.log_received,
        }
        for name, signal in signal_map.items():
            signal.connect(lambda *args, target=events[name]: target.append(args))
        transport.start(sys.executable, ["-u", "-c", MOCK_SERVER], str(Path.cwd()))
        self.assertTrue(self.wait_until(lambda: bool(events["tools"])))
        self.assertTrue(events["connected"])
        self.assertEqual(events["tools"][-1][0][0]["name"], "progress")

        successful_id = transport.call_tool("progress", {})
        self.assertTrue(self.wait_until(lambda: any(row[0] == successful_id for row in events["results"])))
        result = next(row[2] for row in events["results"] if row[0] == successful_id)
        self.assertEqual(result["structuredContent"]["operation_id"], "op-7")
        self.assertTrue(any(row[0].get("progress") == 3 for row in events["progress"]))

        error_id = transport.call_tool("rpc_error", {})
        self.assertTrue(self.wait_until(lambda: any(row[0] == error_id for row in events["failures"])))
        self.assertEqual(next(row[1:] for row in events["failures"] if row[0] == error_id), ("-32000", "mock failure"))

        cancel_id = transport.call_tool("slow", {})
        transport.cancel_request(cancel_id)
        self.assertTrue(any(row[:2] == (cancel_id, "CANCELLED") for row in events["failures"]))
        self.assertTrue(self.wait_until(lambda: any("CANCELLED:" + str(cancel_id) in row[0] for row in events["logs"])))

        timeout_id = transport.call_tool("slow", {}, timeout_ms=60)
        self.assertTrue(self.wait_until(lambda: any(row[:2] == (timeout_id, "TIMEOUT") for row in events["failures"])))
        self.assertTrue(self.wait_until(lambda: any("CANCELLED:" + str(timeout_id) in row[0] for row in events["logs"])))

        transport.call_tool("exit_worker", {})
        self.assertTrue(self.wait_until(lambda: bool(events["disconnected"])))
        self.assertIn("PROCESS_EXITED", [row[1] for row in events["failures"]])

    @staticmethod
    def _stop(transport):
        transport.disconnect()
        proc = transport._process
        if proc is not None and proc.state() != proc.ProcessState.NotRunning:
            loop = QEventLoop()
            proc.finished.connect(loop.quit)
            timer = QTimer()
            timer.setSingleShot(True)
            timer.timeout.connect(loop.quit)
            timer.start(2500)
            loop.exec()


if __name__ == "__main__":
    unittest.main()
