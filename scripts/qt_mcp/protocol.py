"""Small, strict JSON-RPC helpers for MCP's newline-delimited stdio transport."""

from __future__ import annotations

import json
from typing import Any


class ProtocolError(ValueError):
    """Raised when a peer sends an invalid JSON-RPC message."""


def request(request_id: int, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
    if not isinstance(request_id, int) or isinstance(request_id, bool) or request_id < 1:
        raise ValueError("request_id must be a positive integer")
    if not isinstance(method, str) or not method:
        raise ValueError("method must be a non-empty string")
    message: dict[str, Any] = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        if not isinstance(params, dict):
            raise ValueError("params must be an object")
        message["params"] = params
    return message


def notification(method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
    if not isinstance(method, str) or not method:
        raise ValueError("method must be a non-empty string")
    message: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        if not isinstance(params, dict):
            raise ValueError("params must be an object")
        message["params"] = params
    return message


def encode_line(message: dict[str, Any]) -> bytes:
    if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
        raise ValueError("message must be a JSON-RPC 2.0 object")
    return (json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")


def decode_line(line: bytes | bytearray | str) -> dict[str, Any]:
    try:
        if isinstance(line, (bytes, bytearray)):
            line = bytes(line).decode("utf-8")
        value = json.loads(line)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolError(f"invalid JSON-RPC line: {exc}") from exc
    if not isinstance(value, dict) or value.get("jsonrpc") != "2.0":
        raise ProtocolError("message must be a JSON-RPC 2.0 object")
    if "method" in value:
        if not isinstance(value["method"], str) or not value["method"]:
            raise ProtocolError("notification method must be a non-empty string")
        if "params" in value and not isinstance(value["params"], dict):
            raise ProtocolError("notification params must be an object")
        return value
    if "id" not in value or not ("result" in value or "error" in value):
        raise ProtocolError("response must include an id and result or error")
    response_id = value["id"]
    if not ((isinstance(response_id, int) and not isinstance(response_id, bool)) or isinstance(response_id, str)):
        raise ProtocolError("response id must be an integer or string")
    if "error" in value and (
        not isinstance(value["error"], dict)
        or not isinstance(value["error"].get("code"), int)
        or isinstance(value["error"].get("code"), bool)
    ):
        raise ProtocolError("response error must include an integer code")
    if "result" in value and "error" in value:
        raise ProtocolError("response cannot include both result and error")
    return value
