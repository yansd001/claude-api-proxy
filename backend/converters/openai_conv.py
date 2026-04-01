"""Convert between Anthropic Messages API format and OpenAI Chat Completions format."""
from __future__ import annotations

import json
import uuid
from typing import Any, AsyncGenerator


# ---------------------------------------------------------------------------
# Request: Anthropic → OpenAI
# ---------------------------------------------------------------------------

def _system_to_str(system: Any) -> str:
    if isinstance(system, str):
        return system
    if isinstance(system, list):
        return "\n\n".join(
            block.get("text", "") for block in system if block.get("type") == "text"
        )
    return ""


def _user_content_blocks_to_openai(blocks: list[dict]) -> list[dict] | str:
    """Convert Anthropic user content blocks to OpenAI content format."""
    parts: list[dict] = []
    for block in blocks:
        btype = block.get("type")
        if btype == "text":
            parts.append({"type": "text", "text": block["text"]})
        elif btype == "image":
            source = block.get("source", {})
            if source.get("type") == "base64":
                data_url = f"data:{source['media_type']};base64,{source['data']}"
                parts.append({"type": "image_url", "image_url": {"url": data_url}})
            elif source.get("type") == "url":
                parts.append({"type": "image_url", "image_url": {"url": source["url"]}})
    if len(parts) == 1 and parts[0]["type"] == "text":
        return parts[0]["text"]
    return parts or ""


def _anthropic_messages_to_openai(messages: list[dict], system: Any = None) -> list[dict]:
    """Convert Anthropic messages array to OpenAI messages array."""
    result: list[dict] = []

    if system:
        result.append({"role": "system", "content": _system_to_str(system)})

    for msg in messages:
        role = msg["role"]
        content = msg["content"]

        if isinstance(content, str):
            result.append({"role": role, "content": content})
            continue

        if role == "user":
            tool_results = [b for b in content if b.get("type") == "tool_result"]
            other = [b for b in content if b.get("type") != "tool_result"]

            # Tool results become role=tool messages
            for tr in tool_results:
                tr_content = tr.get("content", "")
                if isinstance(tr_content, list):
                    tr_content = "\n".join(
                        b.get("text", "") for b in tr_content if b.get("type") == "text"
                    )
                result.append({
                    "role": "tool",
                    "tool_call_id": tr["tool_use_id"],
                    "content": tr_content or "",
                })

            if other:
                converted = _user_content_blocks_to_openai(other)
                result.append({"role": "user", "content": converted})

        elif role == "assistant":
            text_parts = [b["text"] for b in content if b.get("type") == "text"]
            tool_uses = [b for b in content if b.get("type") == "tool_use"]

            msg_obj: dict[str, Any] = {"role": "assistant"}
            msg_obj["content"] = "\n".join(text_parts) if text_parts else None

            if tool_uses:
                msg_obj["tool_calls"] = [
                    {
                        "id": tu["id"],
                        "type": "function",
                        "function": {
                            "name": tu["name"],
                            "arguments": json.dumps(tu.get("input", {})),
                        },
                    }
                    for tu in tool_uses
                ]
            result.append(msg_obj)

    return result


def build_openai_request(anthropic_body: dict, target_model: str) -> dict:
    """Build a complete OpenAI request dict from an Anthropic request body."""
    openai_req: dict[str, Any] = {
        "model": target_model,
        "messages": _anthropic_messages_to_openai(
            anthropic_body["messages"], anthropic_body.get("system")
        ),
        "max_tokens": anthropic_body.get("max_tokens", 4096),
        "stream": anthropic_body.get("stream", False),
    }

    for key in ("temperature", "top_p"):
        if key in anthropic_body:
            openai_req[key] = anthropic_body[key]

    if "stop_sequences" in anthropic_body:
        openai_req["stop"] = anthropic_body["stop_sequences"]

    tools = anthropic_body.get("tools")
    if tools:
        openai_req["tools"] = [
            {
                "type": "function",
                "function": {
                    "name": t["name"],
                    "description": t.get("description", ""),
                    "parameters": t.get("input_schema", {"type": "object", "properties": {}}),
                },
            }
            for t in tools
        ]
        tc = anthropic_body.get("tool_choice")
        if tc:
            tc_type = tc.get("type")
            if tc_type == "auto":
                openai_req["tool_choice"] = "auto"
            elif tc_type == "any":
                openai_req["tool_choice"] = "required"
            elif tc_type == "tool":
                openai_req["tool_choice"] = {
                    "type": "function",
                    "function": {"name": tc["name"]},
                }

    return openai_req


# ---------------------------------------------------------------------------
# Response (non-streaming): OpenAI → Anthropic
# ---------------------------------------------------------------------------

_FINISH_REASON_MAP = {
    "stop": "end_turn",
    "tool_calls": "tool_use",
    "length": "max_tokens",
    "content_filter": "stop_sequence",
}


def convert_openai_response(openai_resp: dict, claude_model: str, message_id: str) -> dict:
    """Convert a non-streaming OpenAI response to Anthropic response format."""
    choice = openai_resp["choices"][0]
    message = choice["message"]
    content_blocks: list[dict] = []

    if message.get("content"):
        content_blocks.append({"type": "text", "text": message["content"]})

    for tc in message.get("tool_calls") or []:
        try:
            input_data = json.loads(tc["function"]["arguments"])
        except (json.JSONDecodeError, KeyError, TypeError):
            input_data = {}
        content_blocks.append({
            "type": "tool_use",
            "id": tc["id"],
            "name": tc["function"]["name"],
            "input": input_data,
        })

    finish_reason = choice.get("finish_reason", "stop")
    stop_reason = _FINISH_REASON_MAP.get(finish_reason, "end_turn")
    usage = openai_resp.get("usage", {})

    return {
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": claude_model,
        "stop_reason": stop_reason,
        "stop_sequence": None,
        "usage": {
            "input_tokens": usage.get("prompt_tokens", 0),
            "output_tokens": usage.get("completion_tokens", 0),
        },
    }


# ---------------------------------------------------------------------------
# Streaming: OpenAI SSE → Anthropic SSE
# ---------------------------------------------------------------------------

def _sse(event: str, data: dict) -> str:
    return f"event: {event}\ndata: {json.dumps(data)}\n\n"


async def stream_openai_to_anthropic(
    response_lines: AsyncGenerator[str, None],
    claude_model: str,
    message_id: str,
) -> AsyncGenerator[str, None]:
    """Read OpenAI SSE lines, yield Anthropic SSE event strings."""

    # --- Send opening events ---
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
    text_block_index = -1          # -1 = no open text block
    tool_block_map: dict[int, int] = {}  # openai tc index → anthropic block index
    stop_reason = "end_turn"
    output_tokens = 0

    async for line in response_lines:
        line = line.strip()
        if not line.startswith("data:"):
            continue
        raw = line[5:].strip()
        if raw == "[DONE]":
            break
        try:
            chunk = json.loads(raw)
        except json.JSONDecodeError:
            continue

        # Usage (sometimes appears in last chunk)
        if chunk.get("usage"):
            output_tokens = chunk["usage"].get("completion_tokens", output_tokens)

        choices = chunk.get("choices")
        if not choices:
            continue

        choice = choices[0]
        delta = choice.get("delta", {})
        finish_reason = choice.get("finish_reason")

        # --- Text content ---
        text_content = delta.get("content")
        if text_content:
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
                "delta": {"type": "text_delta", "text": text_content},
            })

        # --- Tool calls ---
        for tc_delta in delta.get("tool_calls") or []:
            tc_idx = tc_delta.get("index", 0)
            tc_id = tc_delta.get("id")

            if tc_id is not None:
                # New tool call: close text block first
                if text_block_index != -1:
                    yield _sse("content_block_stop", {
                        "type": "content_block_stop",
                        "index": text_block_index,
                    })
                    text_block_index = -1

                block_idx = next_index
                next_index += 1
                tool_block_map[tc_idx] = block_idx
                fn = tc_delta.get("function", {})
                yield _sse("content_block_start", {
                    "type": "content_block_start",
                    "index": block_idx,
                    "content_block": {
                        "type": "tool_use",
                        "id": tc_id,
                        "name": fn.get("name", ""),
                        "input": {},
                    },
                })

            fn_delta = (tc_delta.get("function") or {}).get("arguments")
            if fn_delta:
                block_idx = tool_block_map.get(tc_idx, next_index - 1)
                yield _sse("content_block_delta", {
                    "type": "content_block_delta",
                    "index": block_idx,
                    "delta": {"type": "input_json_delta", "partial_json": fn_delta},
                })

        if finish_reason:
            stop_reason = _FINISH_REASON_MAP.get(finish_reason, "end_turn")

    # --- Close open blocks ---
    if text_block_index != -1:
        yield _sse("content_block_stop", {
            "type": "content_block_stop",
            "index": text_block_index,
        })
    for block_idx in tool_block_map.values():
        yield _sse("content_block_stop", {
            "type": "content_block_stop",
            "index": block_idx,
        })

    # --- Closing events ---
    yield _sse("message_delta", {
        "type": "message_delta",
        "delta": {"stop_reason": stop_reason, "stop_sequence": None},
        "usage": {"output_tokens": output_tokens},
    })
    yield _sse("message_stop", {"type": "message_stop"})
