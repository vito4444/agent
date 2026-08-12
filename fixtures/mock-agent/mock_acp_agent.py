#!/usr/bin/env python3
"""Minimal ACP agent over stdio JSON-RPC for CI contract tests.

Mirrors the subset OpenCode exposes that our client must handle.
Not a full ACP implementation — unknown methods return -32601.
"""

from __future__ import annotations

import json
import sys
from typing import Any


def send(msg: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(msg, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def notify_update(session_id: str, update: dict[str, Any]) -> None:
    send(
        {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": session_id, "update": update},
        }
    )


CONFIG = [
    {
        "id": "model",
        "name": "Model",
        "category": "model",
        "type": "select",
        "currentValue": "default",
        "options": [
            {"value": "default", "name": "Default"},
            {"value": "fast", "name": "Fast"},
        ],
    },
    {
        "id": "thought",
        "name": "Thought",
        "category": "thought_level",
        "type": "select",
        "currentValue": "medium",
        "options": [
            {"value": "low", "name": "Low"},
            {"value": "medium", "name": "Medium"},
            {"value": "high", "name": "High"},
        ],
    },
]


def main() -> None:
    session_id = "sess_mock_1"
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        method = msg.get("method")
        msg_id = msg.get("id")
        params = msg.get("params") or {}

        if method == "initialize":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "protocolVersion": params.get("protocolVersion", 1),
                        "agentCapabilities": {
                            "loadSession": False,
                            "promptCapabilities": {"image": False},
                        },
                        "agentInfo": {
                            "name": "mock-acp-agent",
                            "title": "Mock ACP Agent",
                            "version": "0.1.0",
                        },
                        "authMethods": [],
                    },
                }
            )
        elif method == "session/new":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "sessionId": session_id,
                        "configOptions": CONFIG,
                    },
                }
            )
        elif method == "session/prompt":
            notify_update(
                session_id,
                {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": {"type": "text", "text": "Thinking about the request..."},
                },
            )
            notify_update(
                session_id,
                {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "Mock agent acknowledges: "},
                },
            )
            # request permission once
            send(
                {
                    "jsonrpc": "2.0",
                    "id": 900001,
                    "method": "session/request_permission",
                    "params": {
                        "sessionId": session_id,
                        "opType": "edit",
                        "toolCall": {
                            "toolCallId": "call_mock_1",
                            "title": "Edit file",
                            "kind": "edit",
                        },
                    },
                }
            )
            # wait for permission response
            perm_line = sys.stdin.readline()
            if perm_line:
                pass
            notify_update(
                session_id,
                {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call_mock_1",
                    "title": "Edit src/lib.rs",
                    "kind": "edit",
                    "status": "completed",
                },
            )
            notify_update(
                session_id,
                {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call_mock_1",
                    "status": "completed",
                    "content": [
                        {
                            "type": "diff",
                            "path": "src/lib.rs",
                            "diff": "@@\n-a-b\n+a+b\n",
                        },
                        {"type": "terminal", "output": "$ echo ok\nok\n"},
                    ],
                },
            )
            # unknown extension — client must ignore
            notify_update(
                session_id,
                {"sessionUpdate": "_vendor_noise", "_meta": {"x": 1}},
            )
            text = ""
            for block in params.get("prompt") or []:
                if isinstance(block, dict) and block.get("type") == "text":
                    text += block.get("text") or ""
            notify_update(
                session_id,
                {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": text or "done"},
                },
            )
            send(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {"stopReason": "end_turn"},
                }
            )
        elif method == "session/set_config_option":
            cid = params.get("configId")
            val = params.get("value")
            for opt in CONFIG:
                if opt["id"] == cid:
                    opt["currentValue"] = val
            send(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {"configOptions": CONFIG},
                }
            )
        elif method == "session/cancel":
            # notification — no response
            continue
        elif method == "session/close":
            send({"jsonrpc": "2.0", "id": msg_id, "result": {}})
        elif msg_id is not None:
            send(
                {
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "error": {"code": -32601, "message": f"method not found: {method}"},
                }
            )


if __name__ == "__main__":
    main()
