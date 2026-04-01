"""Convert between Anthropic Messages API format and Google Gemini API format."""
from __future__ import annotations

import json
import uuid
from typing import Any, AsyncGenerator


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _get_tool_name(messages: list[dict], tool_use_id: str) -> str:
    """Scan previous messages to find the function name for a tool_use_id."""
    for msg in messages:
        content = msg.get("content", [])
        if isinstance(content, list):
            for block in content:
                if block.get("type") == "tool_use" and block.get("id") == tool_use_id:
                    return block["name"]
    return "unknown_tool"


def _system_to_str(system: Any) -> str:
    if isinstance(system, str):
        return system
    if isinstance(system, list):
        return "\n\n".join(
            b.get("text", "") for b in system if b.get("type") == "text"
        )
    return ""


# ---------------------------------------------------------------------------
# Request: Anthropic → Gemini
# ---------------------------------------------------------------------------

def _content_to_gemini_parts(
    content: Any,
    role: str,
    all_messages: list[dict],
) -> list[dict]:
    """Convert Anthropic content (str or list of blocks) to Gemini parts."""
    if isinstance(content, str):
        return [{"text": content}]

    parts: list[dict] = []
    for block in content:
        btype = block.get("type")
        if btype == "text":
            parts.append({"text": block["text"]})
        elif btype == "image":
            source = block.get("source", {})
            if source.get("type") == "base64":
                parts.append({
                    "inline_data": {
                        "mime_type": source["media_type"],
                        "data": source["data"],
                    }
                })
            elif source.get("type") == "url":
                # Gemini doesn't natively support URL images; embed by reference
                parts.append({"text": f"[Image URL: {source['url']}]"})
        elif btype == "tool_use":
            parts.append({
                "functionCall": {
                    "name": block["name"],
                    "args": block.get("input", {}),
                }
            })
        elif btype == "tool_result":
            tr_content = block.get("content", "")
            if isinstance(tr_content, list):
                tr_content = "\n".join(
                    b.get("text", "") for b in tr_content if b.get("type") == "text"
                )
            fn_name = _get_tool_name(all_messages, block["tool_use_id"])
            parts.append({
                "functionResponse": {
                    "name": fn_name,
                    "response": {"result": tr_content or ""},
                }
            })
    return parts


def build_gemini_request(anthropic_body: dict, target_model: str) -> tuple[str, dict]:
    """
    Build Gemini API request.
    Returns (model_name, request_body).
    """
    messages = anthropic_body["messages"]
    contents: list[dict] = []

    for msg in messages:
        role = msg["role"]
        gemini_role = "model" if role == "assistant" else "user"
        parts = _content_to_gemini_parts(msg["content"], role, messages)
        if parts:
            contents.append({"role": gemini_role, "parts": parts})

    body: dict[str, Any] = {"contents": contents}

    system = anthropic_body.get("system")
    if system:
        body["system_instruction"] = {"parts": [{"text": _system_to_str(system)}]}

    gen_config: dict[str, Any] = {}
    if "max_tokens" in anthropic_body:
        gen_config["maxOutputTokens"] = anthropic_body["max_tokens"]
    if "temperature" in anthropic_body:
        gen_config["temperature"] = anthropic_body["temperature"]
    if "top_p" in anthropic_body:
        gen_config["topP"] = anthropic_body["top_p"]
    if "stop_sequences" in anthropic_body:
        gen_config["stopSequences"] = anthropic_body["stop_sequences"]
    if gen_config:
        body["generationConfig"] = gen_config

    tools = anthropic_body.get("tools")
    if tools:
        fn_decls = []
        for t in tools:
            fn_decl: dict[str, Any] = {
                "name": t["name"],
                "description": t.get("description", ""),
            }
            schema = t.get("input_schema")
            if schema:
                fn_decl["parameters"] = schema
            fn_decls.append(fn_decl)
        body["tools"] = [{"function_declarations": fn_decls}]

    return target_model, body


# ---------------------------------------------------------------------------
# Response (non-streaming): Gemini → Anthropic
# ---------------------------------------------------------------------------

_FINISH_REASON_MAP = {
    "STOP": "end_turn",
    "MAX_TOKENS": "max_tokens",
    "SAFETY": "stop_sequence",
    "RECITATION": "stop_sequence",
    "OTHER": "end_turn",
    "FINISH_REASON_UNSPECIFIED": "end_turn",
}


def convert_gemini_response(gemini_resp: dict, claude_model: str, message_id: str) -> dict:
    """Convert a non-streaming Gemini response to Anthropic response format."""
    candidates = gemini_resp.get("candidates", [])
    content_blocks: list[dict] = []
    finish_reason = "end_turn"

    if candidates:
        candidate = candidates[0]
        finish_reason = _FINISH_REASON_MAP.get(
            candidate.get("finishReason", "STOP"), "end_turn"
        )
        gemini_content = candidate.get("content", {})
        parts = gemini_content.get("parts", [])

        for part in parts:
            if "text" in part:
                content_blocks.append({"type": "text", "text": part["text"]})
            elif "functionCall" in part:
                fc = part["functionCall"]
                content_blocks.append({
                    "type": "tool_use",
                    "id": f"toolu_{uuid.uuid4().hex[:16]}",
                    "name": fc["name"],
                    "input": fc.get("args", {}),
                })

    if any(b["type"] == "tool_use" for b in content_blocks):
        finish_reason = "tool_use"

    usage_meta = gemini_resp.get("usageMetadata", {})
    return {
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": claude_model,
        "stop_reason": finish_reason,
        "stop_sequence": None,
        "usage": {
            "input_tokens": usage_meta.get("promptTokenCount", 0),
            "output_tokens": usage_meta.get("candidatesTokenCount", 0),
        },
    }


# ---------------------------------------------------------------------------
# Streaming: Gemini SSE → Anthropic SSE
# ---------------------------------------------------------------------------

def _sse(event: str, data: dict) -> str:
    return f"event: {event}\ndata: {json.dumps(data)}\n\n"


async def stream_gemini_to_anthropic(
    response_lines: AsyncGenerator[str, None],
    claude_model: str,
    message_id: str,
) -> AsyncGenerator[str, None]:
    """Read Gemini SSE lines, yield Anthropic SSE event strings."""

    yield _sse("message_start", {
        "type": "message_start",
        "message": {
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": claude_model,
            "stop_reason": None,
            "stop_sequence": None,
            "usage": {"input_tokens": 0, "output_tokens": 1},
        },
    })
    yield _sse("ping", {"type": "ping"})

    next_index = 0
    text_block_index = -1
    # tool_use blocks: name → block_index (Gemini returns complete fn call at once)
    stop_reason = "end_turn"
    output_tokens = 0
    input_tokens = 0

    async for line in response_lines:
        line = line.strip()
        if not line.startswith("data:"):
            continue
        raw = line[5:].strip()
        if not raw:
            continue
        try:
            chunk = json.loads(raw)
        except json.JSONDecodeError:
            continue

        usage_meta = chunk.get("usageMetadata", {})
        if usage_meta:
            input_tokens = usage_meta.get("promptTokenCount", input_tokens)
            output_tokens = usage_meta.get("candidatesTokenCount", output_tokens)

        candidates = chunk.get("candidates", [])
        if not candidates:
            continue

        candidate = candidates[0]
        finish = candidate.get("finishReason", "")
        if finish and finish not in ("", "FINISH_REASON_UNSPECIFIED"):
            stop_reason = _FINISH_REASON_MAP.get(finish, "end_turn")

        parts = candidate.get("content", {}).get("parts", [])
        for part in parts:
            if "text" in part:
                if text_block_index == -1:
                    text_block_index = next_index
                    next_index += 1
                    yield _sse("content_block_start", {
                        "type": "content_block_start",
                        "index": text_block_index,
                        "content_block": {"type": "text", "text": ""},
                    })
                yield _sse("content_block_delta", {
                    "type": "content_block_delta",
                    "index": text_block_index,
                    "delta": {"type": "text_delta", "text": part["text"]},
                })

            elif "functionCall" in part:
                # Gemini returns complete function call – emit as a single block
                fc = part["functionCall"]
                # Close text block if open
                if text_block_index != -1:
                    yield _sse("content_block_stop", {
                        "type": "content_block_stop",
                        "index": text_block_index,
                    })
                    text_block_index = -1

                block_idx = next_index
                next_index += 1
                tool_id = f"toolu_{uuid.uuid4().hex[:16]}"
                args_str = json.dumps(fc.get("args", {}))

                yield _sse("content_block_start", {
                    "type": "content_block_start",
                    "index": block_idx,
                    "content_block": {
                        "type": "tool_use",
                        "id": tool_id,
                        "name": fc["name"],
                        "input": {},
                    },
                })
                yield _sse("content_block_delta", {
                    "type": "content_block_delta",
                    "index": block_idx,
                    "delta": {"type": "input_json_delta", "partial_json": args_str},
                })
                yield _sse("content_block_stop", {
                    "type": "content_block_stop",
                    "index": block_idx,
                })
                stop_reason = "tool_use"

    # Close text block if still open
    if text_block_index != -1:
        yield _sse("content_block_stop", {
            "type": "content_block_stop",
            "index": text_block_index,
        })

    yield _sse("message_delta", {
        "type": "message_delta",
        "delta": {"stop_reason": stop_reason, "stop_sequence": None},
        "usage": {"output_tokens": output_tokens},
    })
    yield _sse("message_stop", {"type": "message_stop"})
