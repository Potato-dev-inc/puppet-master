"""Tests for schema-driven MCP tool forms; tests never invoke a worker."""

import os
import unittest
from PySide6.QtCore import QTimer

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication, QDialog, QDialogButtonBox, QDoubleSpinBox, QPlainTextEdit, QSpinBox, QWidget

try:
    from .forms import ToolArgumentsDialog, ask_tool_arguments, render_result
except ImportError:
    from forms import ToolArgumentsDialog, ask_tool_arguments, render_result


class ToolFormTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_form_collects_typed_values_and_omits_empty_optionals(self):
        tool = {"name": "delegate_work", "inputSchema": {
            "type": "object",
            "required": ["task", "agent_type", "acceptance_criteria"],
            "properties": {
                "project_path": {"type": "string"},
                "task": {"type": "string", "minLength": 1},
                "agent_type": {"type": "string", "enum": ["codex", "cursor"]},
                "acceptance_criteria": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                "exclusive": {"type": "boolean"},
                "task_id": {"type": "string"},
            },
        }}
        dialog = ToolArgumentsDialog(None, tool, {"project_path": "C:/repo", "task_id": "existing"})
        self.assertNotIn("project_path", dialog.fields)
        self.assertEqual(dialog.fields["task_id"][0].text(), "existing")
        dialog.fields["task"][0].setPlainText("Inspect the MCP")
        dialog.fields["agent_type"][0].setCurrentIndex(2)
        dialog.fields["acceptance_criteria"][0].setPlainText("Find issues\nLeave files unchanged")
        dialog.fields["exclusive"][0].setChecked(True)
        values = dialog.values()
        self.assertEqual(values, {
            "project_path": "C:/repo", "task_id": "existing", "task": "Inspect the MCP",
            "agent_type": "cursor", "acceptance_criteria": ["Find issues", "Leave files unchanged"],
            "exclusive": True,
        })
        self.assertNotIn("idempotency_key", values)

    def test_required_fields_are_validated_and_optional_numbers_are_omitted(self):
        tool = {"name": "wait_for_operation", "inputSchema": {
            "required": ["operation_id"], "properties": {
                "operation_id": {"type": "string", "minLength": 1},
                "after_revision": {"type": "integer", "minimum": 0},
                "timeout_ms": {"type": "integer", "default": 120000},
            },
        }}
        dialog = ToolArgumentsDialog(None, tool)
        with self.assertRaisesRegex(ValueError, "required"):
            dialog.values()
        dialog.fields["operation_id"][0].setText("op-8")
        result = dialog.values()
        self.assertEqual(result["operation_id"], "op-8")
        self.assertNotIn("after_revision", result)
        self.assertEqual(result["timeout_ms"], 120000)

    def test_nested_object_schema_renders_fields_recursively(self):
        tool = {"name": "wait_for_panes", "inputSchema": {
            "required": ["pane_ids"], "properties": {
                "pane_ids": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                "match": {"type": "object", "properties": {"model_id": {"type": "string"}}},
            },
        }}
        dialog = ToolArgumentsDialog(None, tool)
        self.assertIn("match.model_id", dialog.fields)
        dialog.fields["pane_ids"][0].setPlainText("pane-1")
        dialog.fields["match.model_id"][0].setText("gpt-test")
        self.assertEqual(dialog.values(), {"pane_ids": ["pane-1"], "match": {"model_id": "gpt-test"}})

    def test_results_render_human_readable_labels_without_json_dump(self):
        result = render_result({"structuredContent": {
            "snapshot": {"operation_id": "op-1", "status": "waiting_input",
                         "required_action": {"kind": "approval", "reason": "Review the command"}},
            "reason": "revision_changed",
        }, "content": [{"type": "text", "text": "Operation updated"}]})
        self.assertIn("Operation id: op-1", result)
        self.assertIn("Status: Waiting input", result)
        self.assertIn("Revision changed", result)
        self.assertNotIn('{"', result)

    def test_native_text_result_and_error_are_preserved(self):
        self.assertEqual(render_result({"isError": True, "content": [{"type": "text", "text": "PANE_NOT_FOUND: pane gone"}]}),
                         "Tool error\nPANE_NOT_FOUND: pane gone")

    def test_direct_operation_snapshot_is_rendered_for_status_panel(self):
        rendered = render_result({"operation_id": "op-2", "revision": 4, "status": "running", "stage": "testing"})
        self.assertIn("Operation id: op-2", rendered)
        self.assertIn("Status: Running", rendered)
        self.assertIn("Stage: Testing", rendered)

    def test_rust_json_text_prefers_structured_content_and_never_shows_json(self):
        snapshot = {"operation_id": "op-3", "revision": 5, "status": "completed", "stage": "done"}
        result = {"structuredContent": snapshot,
                  "content": [{"type": "text", "text": '{"operation_id":"op-3","status":"completed"}'}]}
        rendered = render_result(result)
        self.assertEqual(rendered.count("Operation id: op-3"), 1)
        self.assertNotIn('{"', rendered)

    def test_json_text_only_is_decoded_but_prose_is_preserved(self):
        decoded = render_result({"content": [{"type": "text", "text": '{"operation_id":"op-4","status":"waiting_input"}'}]})
        prose = render_result({"content": [{"type": "text", "text": "The worker needs approval."}]})
        self.assertIn("Status: Waiting input", decoded)
        self.assertNotIn('{"', decoded)
        self.assertEqual(prose, "The worker needs approval.")

    def test_enum_arrays_use_checked_choices_and_validate_free_text(self):
        tool = {"name": "wait_for_operation", "inputSchema": {
            "required": ["until"], "properties": {
                "until": {"type": "array", "items": {"type": "string", "enum": ["completed", "failed"]}}
            },
        }}
        dialog = ToolArgumentsDialog(None, tool)
        choices = dialog.fields["until"][0]
        choices.item(0).setCheckState(Qt.CheckState.Checked)
        self.assertEqual(dialog.values(), {"until": ["completed"]})

    def test_enum_array_and_required_text_reject_invalid_input(self):
        tool = {"name": "delegate_work", "inputSchema": {
            "required": ["task", "criteria", "state"], "properties": {
                "task": {"type": "string", "minLength": 3},
                "criteria": {"type": "array", "minItems": 1,
                              "items": {"type": "string", "minLength": 2}},
                "state": {"type": "string", "enum": ["ready", "done"]},
            },
        }}
        dialog = ToolArgumentsDialog(None, tool)
        with self.assertRaisesRegex(ValueError, "required"):
            dialog.values()
        dialog.fields["task"][0].setPlainText("ok")
        dialog.fields["criteria"][0].setPlainText("x")
        dialog.fields["state"][0].setCurrentIndex(1)
        with self.assertRaisesRegex(ValueError, "required"):
            dialog.values()
        dialog.fields["task"][0].setPlainText("task")
        dialog.fields["criteria"][0].setPlainText("x")
        with self.assertRaisesRegex(ValueError, "at least 2"):
            dialog.values()
        dialog.fields["criteria"][0].setPlainText("ok")
        dialog.fields["state"][0].setCurrentIndex(0)
        with self.assertRaisesRegex(ValueError, "Choose an option"):
            dialog.values()

    def test_required_hidden_context_is_carried_and_missing_context_remains_visible(self):
        tool = {"name": "get_operation", "inputSchema": {
            "required": ["operation_id", "project_path"], "properties": {
                "operation_id": {"type": "string"}, "project_path": {"type": "string"}
            },
        }}
        with_context = ToolArgumentsDialog(None, tool, {"operation_id": "op-1", "project_path": "C:/repo"})
        self.assertEqual(with_context.values(), {"operation_id": "op-1", "project_path": "C:/repo"})
        missing_context = ToolArgumentsDialog(None, tool, {"operation_id": "op-1"})
        self.assertIn("project_path", missing_context.fields)
        with self.assertRaisesRegex(ValueError, "required"):
            missing_context.values()

    def test_optional_numeric_boolean_and_empty_nested_objects_are_safe(self):
        tool = {"name": "options", "inputSchema": {"properties": {
            "optional_int": {"type": "integer", "minimum": 0, "maximum": 20},
            "optional_number": {"type": "number", "minimum": 0, "maximum": 10},
            "enabled": {"type": "boolean"},
            "match": {"type": "object", "properties": {"model_id": {"type": "string"}}},
        }}}
        dialog = ToolArgumentsDialog(None, tool)
        self.assertIsInstance(dialog.fields["optional_int"][0], QSpinBox)
        self.assertIsInstance(dialog.fields["optional_number"][0], QDoubleSpinBox)
        self.assertEqual(dialog.values(), {"enabled": False})
        dialog.fields["optional_int"][0].setValue(7)
        dialog.fields["optional_number"][0].setValue(2.5)
        dialog.fields["enabled"][0].setChecked(True)
        dialog.fields["match.model_id"][0].setText("model-a")
        self.assertEqual(dialog.values(), {
            "optional_int": 7, "optional_number": 2.5, "enabled": True,
            "match": {"model_id": "model-a"},
        })

    def test_multiline_minimum_and_password_widget(self):
        tool = {"name": "complete_task", "inputSchema": {"required": ["evidence", "token"], "properties": {
            "evidence": {"type": "string", "minLength": 5},
            "token": {"type": "string", "format": "password"},
            "notes": {"type": "string", "maxLength": 300},
        }}}
        dialog = ToolArgumentsDialog(None, tool)
        self.assertIsInstance(dialog.fields["evidence"][0], QPlainTextEdit)
        self.assertIsInstance(dialog.fields["notes"][0], QPlainTextEdit)
        with self.assertRaisesRegex(ValueError, "required"):
            dialog.values()
        dialog.fields["evidence"][0].setPlainText("done")
        dialog.fields["token"][0].setText("secret")
        with self.assertRaisesRegex(ValueError, "required"):
            dialog.values()
        dialog.fields["evidence"][0].setPlainText("verified")
        self.assertEqual(dialog.values(), {"evidence": "verified", "token": "secret"})

    def test_modal_argument_api_returns_values_or_none_on_cancel(self):
        tool = {"name": "bridge_health", "description": "Check bridge status", "inputSchema": {"properties": {}}}

        def accept_dialog():
            dialog = next(widget for widget in QApplication.topLevelWidgets() if isinstance(widget, QDialog))
            self.assertIn("Bridge health", dialog.windowTitle())
            dialog.submit_button.click()

        QTimer.singleShot(0, accept_dialog)
        self.assertEqual(ask_tool_arguments(None, tool), {})

        def cancel_dialog():
            dialog = next(widget for widget in QApplication.topLevelWidgets() if isinstance(widget, QDialog))
            dialog.reject()

        QTimer.singleShot(0, cancel_dialog)
        self.assertIsNone(ask_tool_arguments(None, tool))

    def test_invalid_submit_displays_validation_instead_of_closing(self):
        dialog = ToolArgumentsDialog(None, {"name": "required", "inputSchema": {
            "required": ["task"], "properties": {"task": {"type": "string"}}
        }})
        dialog.show()
        dialog.submit_button.click()
        self.assertEqual(dialog.result(), 0)
        self.assertIn("required", dialog.error_label.text())
        dialog.close()

    def test_json_list_resource_image_unknown_and_error_results(self):
        self.assertIn("• One", render_result(["One", "Two"]))
        resources = render_result({"content": [
            {"type": "resource", "resource": {"text": "Resource text"}},
            {"type": "image"}, {"type": "audio"}, {"type": "other", "name": "blob"},
            "plain item",
        ]})
        self.assertIn("Resource text", resources)
        self.assertIn("Image result attached", resources)
        self.assertIn("Audio result attached", resources)
        self.assertIn("Name: blob", resources)
        self.assertIn("plain item", resources)
        self.assertEqual(render_result({"error": {"code": "BAD_INPUT", "message": "Missing field"}}),
                         "Error\nCode: BAD_INPUT\nMessage: Missing field")
        self.assertEqual(render_result({"content": []}), "No result content.")


if __name__ == "__main__":
    unittest.main()
