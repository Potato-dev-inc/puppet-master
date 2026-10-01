"""Button-only operator flows; no real workers are launched."""

import json
import os
import unittest
from unittest.mock import patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QObject, Signal
from PySide6.QtWidgets import QApplication, QLineEdit, QComboBox, QPlainTextEdit, QTableWidget
import gui


class FakeTransport(QObject):
    connected = Signal()
    disconnected = Signal()
    tools_received = Signal(list)
    result_received = Signal(int, str, object)
    request_failed = Signal(int, str, str)
    progress_received = Signal(object)
    log_received = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.calls = []
        self.cancelled = []
        self.disconnect_count = 0
        self.started = None
        self.refresh_count = 0

    def start(self, program, args, cwd):
        self.started = (program, args, cwd)

    def disconnect(self):
        self.disconnect_count += 1
        self.disconnected.emit()

    def call_tool(self, name, arguments, timeout_ms=35000):
        self.calls.append((name, arguments, timeout_ms))
        return len(self.calls)

    def cancel_request(self, request_id):
        self.cancelled.append(request_id)
        self.request_failed.emit(request_id, "CANCELLED", "Request cancelled")

    def refresh_tools(self):
        self.refresh_count += 1


TOOLS = [
    {"name": "set_mode", "inputSchema": {"type": "object", "properties": {"mode": {"type": "string", "enum": ["agent", "shell", "both"]}}, "required": ["mode"]}},
    {"name": "run_agent", "inputSchema": {"type": "object", "properties": {
        "task": {"type": "string"}, "project_path": {"type": "string"},
        "agent_type": {"type": "string", "enum": ["claude", "codex", "cursor_agent"]},
        "acceptance_criteria": {"type": "array", "items": {"type": "string"}}}, "required": ["task"]}},
    {"name": "list_agents", "inputSchema": {"type": "object", "properties": {"project_path": {"type": "string"}}}},
    {"name": "wait_agents", "inputSchema": {"type": "object", "properties": {
        "handles": {"type": "array", "items": {"type": "string"}},
        "after_revisions": {"type": "object"}, "timeout_ms": {"type": "integer"}}, "required": ["handles"]}},
    *[{"name": name, "inputSchema": {"type": "object", "properties": {"handle": {"type": "string"}}, "required": ["handle"]}}
      for name in ("cancel_agent", "agent_transcript", "take_over")],
    {"name": "shell_exec", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "bridge_health", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "list_panes", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "delegate_work", "inputSchema": {"type": "object", "properties": {
        "task": {"type": "string"}, "acceptance_criteria": {"type": "array", "items": {"type": "string"}},
        "idempotency_key": {"type": "string"}, "project_path": {"type": "string"}},
        "required": ["task", "acceptance_criteria", "idempotency_key", "project_path"]}},
    *[{"name": name, "inputSchema": {"type": "object", "properties": {
        "operation_id": {"type": "string"}}, "required": ["operation_id"]}}
      for name in ("get_operation", "wait_for_operation", "cancel_operation")],
]


class GuiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.patches = [patch.object(gui, "McpTransport", FakeTransport),
                        patch.object(gui.QMessageBox, "warning"),
                        patch.object(gui.QMessageBox, "information")]
        for item in self.patches:
            item.start()
        self.window = gui.MainWindow()
        self.window.transport.connected.emit()
        self.window.transport.tools_received.emit(TOOLS)

    def tearDown(self):
        self.window.close()
        self.window.deleteLater()
        self.app.processEvents()
        for item in reversed(self.patches):
            item.stop()

    def test_main_surface_has_buttons_and_no_editable_inputs(self):
        self.assertEqual(self.window.findChildren(QLineEdit), [])
        self.assertEqual(self.window.findChildren(QComboBox), [])
        self.assertEqual(self.window.findChildren(QTableWidget), [])
        self.assertTrue(all(w.isReadOnly() for w in self.window.findChildren(QPlainTextEdit)))
        self.assertEqual(set(self.window.tool_buttons), {t["name"] for t in TOOLS if t["name"] != "set_mode"})
        self.assertEqual(set(self.window.mode_buttons), {"agent", "shell", "both"})
        self.assertEqual(self.window._mode, "both")

    def test_agent_quick_buttons_are_available_in_agent_mode_and_shell_tools_disable(self):
        agent_tools = [tool for tool in TOOLS if tool["name"] not in {"shell_exec", "delegate_work", "bridge_health", "list_panes", "get_operation", "wait_for_operation", "cancel_operation"}]
        self.window._on_tools(agent_tools)
        self.assertTrue(self.window.run_agent_button.isEnabled())
        self.assertTrue(self.window.list_agents_button.isEnabled())
        self.assertFalse(self.window.delegate_button.isEnabled())
        self.assertFalse(self.window.panes_button.isEnabled())
        self.assertEqual(self.window._mode, "agent")

    def test_discovery_does_not_submit_work(self):
        self.assertEqual(self.window.transport.calls, [])

    def test_parameterless_tool_button_calls_mcp(self):
        self.window.tool_buttons["bridge_health"].click()
        self.assertEqual(self.window.transport.calls[-1][:2], ("bridge_health", {}))

    def test_parameter_dialog_cancel_submits_nothing(self):
        with patch.object(gui, "ask_tool_arguments", return_value=None):
            self.window.tool_buttons["delegate_work"].click()
        self.assertEqual(self.window.transport.calls, [])

    def test_parameter_dialog_submit_delegates_without_json_editor(self):
        args = {"task": "Inspect", "acceptance_criteria": ["Summarize"],
                "project_path": str(gui.REPO_ROOT), "idempotency_key": "key-1"}
        with patch.object(gui, "ask_tool_arguments", return_value=args):
            self.window.tool_buttons["delegate_work"].click()
        self.assertEqual(self.window.transport.calls[-1][0], "delegate_work")
        self.assertEqual(self.window.transport.calls[-1][1]["task"], "Inspect")

    def test_agent_buttons_submit_typed_forms_and_track_handle(self):
        self.window._set_mode_visual("agent")
        args = {"task": "Inspect", "agent_type": "codex", "project_path": str(gui.REPO_ROOT)}
        with patch.object(gui, "ask_tool_arguments", return_value=args):
            self.window.tool_buttons["run_agent"].click()
        self.assertEqual(self.window.transport.calls[-1][:2], ("run_agent", args))
        self.window._on_result(1, "run_agent", {"structuredContent": {
            "handle": "agent-1", "operation_id": "op-1", "status": "running", "revision": 2}})
        self.assertEqual(self.window._agent_handle, "agent-1")
        self.assertEqual(self.window._agent_revision, 2)
        defaults = self.window._defaults_for_tool("wait_agents")
        self.assertEqual(defaults["handles"], ["agent-1"])
        self.assertEqual(defaults["after_revisions"], {"agent-1": 2})

    def test_agent_list_and_wait_results_update_current_record(self):
        self.window._on_result(1, "list_agents", {"structuredContent": {"agents": [
            {"handle": "agent-1", "status": "running", "revision": 4},
        ]}})
        self.assertEqual(self.window._agent_handle, "agent-1")
        self.window._on_result(2, "wait_agents", {"structuredContent": {"agents": [
            {"handle": "agent-1", "status": "completed", "revision": 5, "result": "done"},
        ]}})
        self.assertEqual(self.window._agent_cache["agent-1"]["status"], "completed")
        self.assertIn("done", self.window.result_view.toPlainText())

    def test_explicit_mode_buttons_change_catalog_and_refresh(self):
        self.window.mode_buttons["agent"].click()
        self.assertEqual(self.window.transport.calls[-1][:2], ("set_mode", {"mode": "agent"}))
        self.window._on_result(1, "set_mode", {"structuredContent": {"mode": "agent", "tools": ["run_agent"]}})
        self.assertEqual(self.window._mode, "agent")
        self.assertEqual(self.window.transport.refresh_count, 1)

    def test_tool_refresh_rebuilds_buttons_for_new_mode_catalog(self):
        self.window._on_tools([
            {"name": "set_mode", "inputSchema": {"type": "object", "properties": {}}},
            {"name": "shell_exec", "inputSchema": {"type": "object", "properties": {}}},
        ])
        self.assertEqual(self.window._mode, "shell")
        self.assertIn("shell_exec", self.window.tool_buttons)
        self.assertNotIn("run_agent", self.window.tool_buttons)

    def test_snapshot_tracking_and_human_result(self):
        snapshot = {"operation_id": "op-1", "revision": 8, "status": "running", "stage": "testing"}
        self.window._on_result(1, "delegate_work", {"structuredContent": snapshot})
        self.assertEqual(self.window._operation_id, "op-1")
        self.assertEqual(self.window._revision, 8)
        self.assertIn("testing", self.window.result_view.toPlainText().casefold())
        self.assertNotIn('"operation_id"', self.window.result_view.toPlainText())

    def test_progress_updates_without_polling(self):
        self.window._on_progress({"message": {"operation_id": "op-1", "revision": 9,
            "status": "waiting_input", "required_action": {"kind": "manual_approval_required"}}})
        self.assertEqual(self.window._revision, 9)
        self.assertEqual(self.window._operation_id, "op-1")
        self.assertEqual(self.window.transport.calls, [])

    def test_stale_response_does_not_lower_revision(self):
        self.window._show_operation({"operation_id": "op-1", "revision": 9, "status": "completed"})
        self.window._show_operation({"operation_id": "op-1", "revision": 8, "status": "running"})
        self.assertEqual(self.window._revision, 9)

    def test_new_delegation_selects_new_operation(self):
        self.window._show_operation({"operation_id": "old", "revision": 9, "status": "running"})
        self.window._on_result(2, "delegate_work", {"structuredContent": {
            "operation_id": "new", "revision": 1, "status": "queued"}})
        self.window._on_progress({"message": {"operation_id": "old", "revision": 10, "status": "completed"}})
        self.assertEqual(self.window._operation_id, "new")

    def test_cancel_request_does_not_cancel_operation(self):
        self.window._call("wait_for_operation", {"operation_id": "op-1"})
        self.window._cancel_request()
        self.assertEqual(self.window.transport.cancelled, [1])
        self.assertEqual([c[0] for c in self.window.transport.calls], ["wait_for_operation"])

    def test_concurrent_cancel_operation_and_wait(self):
        self.window._call("wait_for_operation", {"operation_id": "op-1"})
        self.window._call("cancel_operation", {"operation_id": "op-1"})
        self.assertEqual(len(self.window._active_requests), 2)

    def test_cancel_wait_ignores_other_pending_requests(self):
        self.window._call("wait_for_operation", {"operation_id": "op-1"})
        self.window._call("cancel_operation", {"operation_id": "op-1"})
        self.window._cancel_request()
        self.assertEqual(self.window.transport.cancelled, [1])
        self.assertIn(2, self.window._active_requests)

    def test_operation_buttons_use_context_without_another_form(self):
        self.window._show_operation({"operation_id": "op-1", "revision": 9, "status": "running"})
        with patch.object(gui, "ask_tool_arguments", return_value=None) as form:
            self.window.get_button.click()
            self.window.wait_button.click()
            self.window.cancel_op_button.click()
        form.assert_not_called()
        self.assertEqual([c[0] for c in self.window.transport.calls],
                         ["get_operation", "wait_for_operation", "cancel_operation"])
        wait_args = self.window.transport.calls[1][1]
        self.assertEqual(wait_args["operation_id"], "op-1")
        self.assertEqual(wait_args["after_revision"], 9)
        self.assertEqual(wait_args["timeout_ms"], 30000)

    def test_same_delegation_reuses_key_but_new_task_gets_new_key(self):
        args = {"task": "Inspect", "acceptance_criteria": ["Summarize"],
                "project_path": str(gui.REPO_ROOT)}
        with patch.object(gui, "ask_tool_arguments", side_effect=[dict(args), dict(args),
                    {**args, "task": "Review"}]):
            for _ in range(3):
                self.window.tool_buttons["delegate_work"].click()
        keys = [call[1]["idempotency_key"] for call in self.window.transport.calls]
        self.assertEqual(keys[0], keys[1])
        self.assertNotEqual(keys[1], keys[2])

    def test_error_does_not_overwrite_operation(self):
        self.window._show_operation({"operation_id": "op-1", "revision": 2, "status": "running"})
        self.window._on_result(3, "get_operation", {"isError": True,
            "structuredContent": {"code": "BRIDGE_DOWN", "message": "Disconnected"}})
        self.assertEqual(self.window._operation_id, "op-1")
        self.assertIn("Disconnected", self.window.result_view.toPlainText())

    def test_disconnected_actions_do_not_dispatch(self):
        self.window.transport.disconnected.emit()
        self.window._call("bridge_health", {})
        self.assertEqual(self.window.transport.calls, [])

    def test_connect_button_uses_automatic_program_and_arguments(self):
        self.window.transport.disconnected.emit()
        self.window.connect_button.click()
        self.assertEqual(self.window.transport.started, (*gui.default_command(), str(gui.REPO_ROOT)))

    def test_choose_project_clears_old_context_without_cancelling_work(self):
        self.window._pane_id = "worker-1"
        self.window._show_operation({"operation_id": "op-1", "revision": 7, "status": "running"})
        with patch.object(gui.QFileDialog, "getExistingDirectory", return_value="C:/work/another"):
            self.window.project_button.click()
        self.assertEqual(self.window._project_path, "C:/work/another")
        self.assertEqual(self.window._operation_id, "")
        self.assertEqual(self.window._pane_id, "")
        self.assertEqual(self.window.transport.calls, [])

    def test_worker_selector_preserves_actual_agent_type(self):
        self.window._on_result(1, "list_panes", {"structuredContent": {"data": [
            {"id": "worker-1", "agent_type": "claude", "cwd": "C:/work"}]}})
        with patch.object(gui, "ask_worker_pane",
                          return_value={"id": "worker-1", "agent_type": "claude", "cwd": "C:/work"}):
            self.window.worker_button.click()
        self.assertEqual(self.window._pane_id, "worker-1")
        self.assertEqual(self.window._defaults_for_tool("delegate_work")["agent_type"], "claude")
        self.assertIn("worker-1"[:8], self.window.worker_label.text())

    def test_list_panes_loads_every_worker_from_bridge_payload(self):
        panes = [
            {"id": "aaaa1111-1111-1111-1111-111111111111", "agent_type": "powershell",
             "status": "idle", "cwd": "C:/one"},
            {"id": "bbbb2222-2222-2222-2222-222222222222", "agent_type": "powershell",
             "status": "waiting_input", "cwd": "C:/two"},
            {"id": "puppet-master-orchestrator-1", "agent_type": "codex",
             "status": "idle", "cwd": "C:/orch"},
        ]
        self.window._on_result(1, "list_panes", {
            "content": [{"type": "text", "text": json.dumps(panes)}],
            "structuredContent": {"ok": True, "snapshot": panes},
        })
        self.assertEqual(
            [pane["id"] for pane in self.window._pane_cache],
            [panes[0]["id"], panes[1]["id"]],
        )

    def test_new_task_resets_retry_cache_without_mutating_running_work(self):
        self.window._idempotency_keys["task-fingerprint"] = "key-1"
        self.window._show_operation({"operation_id": "op-1", "revision": 7, "status": "running"})
        self.window.new_task_button.click()
        self.assertEqual(self.window._idempotency_keys, {})
        self.assertEqual(self.window._operation_id, "")
        self.assertEqual(self.window.transport.calls, [])

    def test_missing_tool_and_empty_worker_selection_do_not_dispatch(self):
        self.window._run_tool("missing_tool")
        self.window.worker_button.click()
        self.assertEqual(self.window.transport.calls, [])

    def test_failure_and_invalid_progress_are_displayed_without_dispatch(self):
        self.window._on_failure(1, "get_operation", "Bridge unavailable")
        self.assertIn("Bridge unavailable", self.window.result_view.toPlainText())
        self.window._on_progress({"message": "Thinking"})
        self.window._show_operation({"revision": 5})
        self.assertEqual(self.window.transport.calls, [])

    def test_transport_deadline_exceeds_requested_wait_and_send_failure_is_reported(self):
        self.window._call("wait_for_operation", {"operation_id": "op-1", "timeout_ms": 60000})
        self.assertEqual(self.window.transport.calls[-1][2], 65000)
        with patch.object(self.window.transport, "call_tool", side_effect=RuntimeError("Pipe closed")):
            self.window._call("get_operation", {"operation_id": "op-1"})
        self.assertIn("Pipe closed", self.window.result_view.toPlainText())

    def test_close_disconnects_only_owned_mcp_child(self):
        self.window.close()
        self.assertGreaterEqual(self.window.transport.disconnect_count, 1)
        self.assertEqual(self.window.transport.calls, [])


if __name__ == "__main__":
    unittest.main()
