import json
import unittest

from protocol import ProtocolError, decode_line, encode_line, notification, request


class ProtocolTests(unittest.TestCase):
    def test_encodes_compact_utf8_json_line(self):
        data = encode_line(request(3, "tools/call", {"arguments": {"task": "你好"}}))
        self.assertTrue(data.endswith(b"\n"))
        self.assertEqual(json.loads(data), {
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"arguments": {"task": "你好"}},
        })

    def test_notification_has_no_id(self):
        value = notification("notifications/initialized")
        self.assertNotIn("id", value)
        self.assertEqual(decode_line(encode_line(value)), value)

    def test_rejects_invalid_request_inputs(self):
        for args in ((0, "x", None), (True, "x", None), (1, "", None), (1, "x", [])):
            with self.subTest(args=args), self.assertRaises(ValueError):
                request(*args)

    def test_rejects_malformed_json_and_invalid_envelopes(self):
        for line in (b"{", b"[]", b'{"jsonrpc":"1.0","method":"x"}',
                     b'{"jsonrpc":"2.0","id":{},"result":null}',
                     b'{"jsonrpc":"2.0","id":1,"error":{"code":true}}'):
            with self.subTest(line=line), self.assertRaises(ProtocolError):
                decode_line(line)

    def test_preserves_structured_tool_result(self):
        result = {"content": [{"type": "text", "text": "ok"}], "isError": False,
                  "structuredContent": {"operation_id": "op-1", "revision": 2}}
        parsed = decode_line(encode_line({"jsonrpc": "2.0", "id": 8, "result": result}))
        self.assertEqual(parsed["result"], result)


if __name__ == "__main__":
    unittest.main()
