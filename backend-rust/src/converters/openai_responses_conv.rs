use serde_json::{json, Value};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Request: Anthropic Messages → OpenAI Responses API
// ---------------------------------------------------------------------------
// OpenAI Responses API: POST /v1/responses
// Key differences from Chat Completions:
//   - Uses "input" instead of "messages"
//   - Uses "max_output_tokens" instead of "max_tokens"
//   - System prompt goes into a system message inside "input"
//   - Tool results use "function_call_output" type
//   - Streaming events are different (response.output_text.delta, etc.)
//   - Response shape uses "output" array with content items

fn user_content_blocks_to_responses(blocks: &[Value]) -> Value {
    let mut parts: Vec<Value> = Vec::new();
    for block in blocks {
        let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match btype {
            "text" => {
                let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                parts.push(json!({"type": "input_text", "text": text}));
            }
            "image" => {
                if let Some(source) = block.get("source") {
                    let src_type = source.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if src_type == "base64" {
                        let media = source.get("media_type").and_then(|t| t.as_str()).unwrap_or("");
                        let data = source.get("data").and_then(|t| t.as_str()).unwrap_or("");
                        parts.push(json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", media, data),
                        }));
                    } else if src_type == "url" {
                        let url = source.get("url").and_then(|t| t.as_str()).unwrap_or("");
                        parts.push(json!({"type": "input_image", "image_url": url}));
                    }
                }
            }
            _ => {}
        }
    }
    if parts.len() == 1 && parts[0].get("type").and_then(|t| t.as_str()) == Some("input_text") {
        // Simplify to plain string for single text
        parts[0].get("text").cloned().unwrap_or(json!(""))
    } else if parts.is_empty() {
        json!("")
    } else {
        json!(parts)
    }
}

fn anthropic_messages_to_responses_input(messages: &[Value], system: Option<&Value>) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();

    // Add system message as a system role item
    if let Some(sys) = system {
        if !sys.is_null() {
            let sys_text = match sys {
                Value::String(s) if !s.is_empty() => Some(s.clone()),
                Value::Array(arr) => {
                    let text = arr
                        .iter()
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if text.is_empty() { None } else { Some(text) }
                }
                _ => None,
            };
            if let Some(text) = sys_text {
                result.push(json!({
                    "role": "system",
                    "content": text,
                }));
            }
        }
    }

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let content = msg.get("content").unwrap_or(&json!(null));

        if let Some(s) = content.as_str() {
            // Simple string content
            result.push(json!({"role": role, "content": s}));
            continue;
        }

        if let Some(blocks) = content.as_array() {
            if role == "user" {
                // Separate tool results from regular user content
                let tool_results: Vec<&Value> = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                    .collect();
                let other: Vec<&Value> = blocks
                    .iter()
                    .filter(|b| {
                        let t = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        t != "tool_result" && t != "thinking" && t != "redacted_thinking"
                    })
                    .collect();

                for tr in &tool_results {
                    let default_content = json!("");
                    let tr_content = tr.get("content").unwrap_or(&default_content);
                    let content_str = if let Some(arr) = tr_content.as_array() {
                        arr.iter()
                            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else if let Some(s) = tr_content.as_str() {
                        s.to_string()
                    } else {
                        String::new()
                    };

                    let tool_use_id = tr.get("tool_use_id").and_then(|t| t.as_str()).unwrap_or("");
                    result.push(json!({
                        "type": "function_call_output",
                        "call_id": tool_use_id,
                        "output": content_str,
                    }));
                }

                if !other.is_empty() {
                    let other_owned: Vec<Value> = other.into_iter().cloned().collect();
                    let converted = user_content_blocks_to_responses(&other_owned);
                    result.push(json!({"role": "user", "content": converted}));
                }
            } else if role == "assistant" {
                let text_parts: Vec<&str> = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect();
                let tool_uses: Vec<&Value> = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                    .collect();

                // In Responses API, text goes as an assistant message,
                // and tool calls become top-level "function_call" items in the input array.
                if !text_parts.is_empty() {
                    result.push(json!({"role": "assistant", "content": text_parts.join("\n")}));
                }

                for tu in &tool_uses {
                    let id = tu.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = tu.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let default_input = json!({});
                    let input = tu.get("input").unwrap_or(&default_input);
                    result.push(json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": serde_json::to_string(input).unwrap_or_default(),
                    }));
                }
            }
        }
    }

    result
}

pub fn build_responses_request(anthropic_body: &Value, target_model: &str) -> Value {
    let messages = anthropic_body.get("messages").and_then(|m| m.as_array());
    let system = anthropic_body.get("system");

    let input = if let Some(msgs) = messages {
        anthropic_messages_to_responses_input(msgs, system)
    } else {
        Vec::new()
    };

    let max_tokens = anthropic_body.get("max_tokens").and_then(|m| m.as_u64()).unwrap_or(4096);
    let stream = anthropic_body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let mut req = json!({
        "model": target_model,
        "input": input,
        "max_output_tokens": max_tokens,
        "stream": stream,
    });

    for key in &["temperature", "top_p"] {
        if let Some(val) = anthropic_body.get(*key) {
            req[*key] = val.clone();
        }
    }

    // Tools
    if let Some(tools) = anthropic_body.get("tools").and_then(|t| t.as_array()) {
        let responses_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let desc = t.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let default_schema = json!({"type": "object", "properties": {}});
                let schema = t.get("input_schema").unwrap_or(&default_schema);
                json!({
                    "type": "function",
                    "name": name,
                    "description": desc,
                    "parameters": schema,
                })
            })
            .collect();
        req["tools"] = json!(responses_tools);

        if let Some(tc) = anthropic_body.get("tool_choice") {
            let tc_type = tc.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match tc_type {
                "auto" => { req["tool_choice"] = json!("auto"); }
                "any" => { req["tool_choice"] = json!("required"); }
                "tool" => {
                    let name = tc.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    req["tool_choice"] = json!({"type": "function", "function": {"name": name}});
                }
                _ => {}
            }
        }
    }

    req
}

// ---------------------------------------------------------------------------
// Response (non-streaming): OpenAI Responses → Anthropic
// ---------------------------------------------------------------------------

fn finish_reason_map(reason: &str) -> &str {
    match reason {
        "stop" => "end_turn",
        "tool_calls" => "tool_use",
        "length" | "max_output_tokens" => "max_tokens",
        "content_filter" => "stop_sequence",
        _ => "end_turn",
    }
}

pub fn convert_responses_response(resp: &Value, claude_model: &str, message_id: &str) -> Value {
    let mut content_blocks: Vec<Value> = Vec::new();

    // Responses API: output is an array of output items
    if let Some(output) = resp.get("output").and_then(|o| o.as_array()) {
        for item in output {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                        for c in content_arr {
                            let ctype = c.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            if ctype == "output_text" {
                                if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        content_blocks.push(json!({"type": "text", "text": text}));
                                    }
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let args_str = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                    let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                    content_blocks.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                _ => {}
            }
        }
    }

    let stop_reason_raw = resp.get("stop_reason").and_then(|f| f.as_str()).unwrap_or("stop");
    // Responses API may not include stop_reason; infer from output content
    let has_tool_call = content_blocks.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
    let stop_reason = if has_tool_call {
        "tool_use"
    } else {
        finish_reason_map(stop_reason_raw)
    };

    let default_usage = json!({});
    let usage = resp.get("usage").unwrap_or(&default_usage);

    let input_tokens = usage.get("input_tokens").and_then(|p| p.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|c| c.as_u64()).unwrap_or(0);
    let cached_tokens = usage
        .pointer("/input_tokens_details/cached_tokens")
        .and_then(|c| c.as_u64())
        .unwrap_or(0);

    let cache_read = cached_tokens;
    let cache_creation = if cached_tokens > 0 { 0 } else { input_tokens };

    json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": claude_model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "cache_read_input_tokens": cache_read,
            "cache_creation_input_tokens": cache_creation,
        }
    })
}

// ---------------------------------------------------------------------------
// Streaming: OpenAI Responses SSE → Anthropic SSE
// ---------------------------------------------------------------------------

fn sse(event: &str, data: &Value) -> String {
    format!("event: {}\ndata: {}\n\n", event, serde_json::to_string(data).unwrap())
}

pub fn stream_responses_start(claude_model: &str, message_id: &str) -> Vec<String> {
    vec![
        sse("message_start", &json!({
            "type": "message_start",
            "message": {
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": claude_model,
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 1},
            }
        })),
        sse("ping", &json!({"type": "ping"})),
    ]
}

pub struct ResponsesStreamState {
    pub next_index: usize,
    pub text_block_index: i32,
    // Map from item_id/call_id to block index
    pub tool_block_map: HashMap<String, usize>,
    // Buffered argument JSON fragments per block index (emitted at stream end after cleanup)
    pub tool_block_args: HashMap<usize, String>,
    pub stop_reason: String,
    pub output_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl Default for ResponsesStreamState {
    fn default() -> Self {
        Self {
            next_index: 0,
            text_block_index: -1,
            tool_block_map: HashMap::new(),
            tool_block_args: HashMap::new(),
            stop_reason: "end_turn".to_string(),
            output_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        }
    }
}

/// Stub kept for API symmetry; actual parsing uses process_responses_stream_event.
/// Remove keys whose value is an empty string from a JSON object string.
/// This prevents GPT-generated empty optional params (e.g. pages:"") from
/// reaching the client and failing validation.
fn strip_empty_string_values(json_str: &str) -> String {
    if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str::<serde_json::Value>(json_str) {
        map.retain(|_, v| v != &serde_json::Value::String(String::new()));
        serde_json::to_string(&serde_json::Value::Object(map))
            .unwrap_or_else(|_| json_str.to_string())
    } else {
        json_str.to_string()
    }
}

#[allow(dead_code)]
pub fn process_responses_stream_line(_line: &str, _state: &mut ResponsesStreamState) -> Vec<String> {
    Vec::new()
}

/// Process a complete event (event_type + parsed JSON data).
pub fn process_responses_stream_event(
    event_type: &str,
    data: &Value,
    state: &mut ResponsesStreamState,
) -> Vec<String> {
    let mut events = Vec::new();

    match event_type {
        "response.output_text.delta" => {
            let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
            if !delta.is_empty() {
                if state.text_block_index == -1 {
                    state.text_block_index = state.next_index as i32;
                    state.next_index += 1;
                    events.push(sse("content_block_start", &json!({
                        "type": "content_block_start",
                        "index": state.text_block_index,
                        "content_block": {"type": "text", "text": ""},
                    })));
                }
                events.push(sse("content_block_delta", &json!({
                    "type": "content_block_delta",
                    "index": state.text_block_index,
                    "delta": {"type": "text_delta", "text": delta},
                })));
            }
        }

        "response.output_item.added" => {
            // A new output item is starting; if it's a function_call, open a tool_use block
            if let Some(item) = data.get("item") {
                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if item_type == "function_call" {
                    // Close text block if open
                    if state.text_block_index != -1 {
                        events.push(sse("content_block_stop", &json!({
                            "type": "content_block_stop",
                            "index": state.text_block_index,
                        })));
                        state.text_block_index = -1;
                    }
                    // item.id = "fc_xxx" (used in delta events as item_id)
                    // item.call_id = "call_xxx" (used as tool_use.id for Claude Code)
                    let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let block_idx = state.next_index;
                    state.next_index += 1;
                    // Store both item_id and call_id so we can find the block from either
                    if !item_id.is_empty() {
                        state.tool_block_map.insert(item_id, block_idx);
                    }
                    if !call_id.is_empty() {
                        state.tool_block_map.insert(call_id.clone(), block_idx);
                    }
                    events.push(sse("content_block_start", &json!({
                        "type": "content_block_start",
                        "index": block_idx,
                        "content_block": {
                            "type": "tool_use",
                            "id": call_id,
                            "name": name,
                            "input": {},
                        }
                    })));
                }
            }
        }

        "response.function_call_arguments.delta" => {
            let delta = data.get("delta").and_then(|d| d.as_str()).unwrap_or("");
            if !delta.is_empty() {
                // Responses API delta events carry item_id ("fc_xxx"), not call_id
                let block_idx = if let Some(iid) = data.get("item_id").and_then(|i| i.as_str()) {
                    state.tool_block_map.get(iid).copied()
                } else if let Some(cid) = data.get("call_id").and_then(|i| i.as_str()) {
                    state.tool_block_map.get(cid).copied()
                } else {
                    None
                };
                // Buffer instead of emitting immediately; will be cleaned and emitted in stream_responses_end
                if let Some(idx) = block_idx {
                    state.tool_block_args.entry(idx).or_insert_with(String::new).push_str(delta);
                }
            }
        }

        "response.completed" => {
            if let Some(response) = data.get("response") {
                if let Some(usage) = response.get("usage") {
                    if let Some(ot) = usage.get("output_tokens").and_then(|c| c.as_u64()) {
                        state.output_tokens = ot;
                    }
                    if let Some(it) = usage.get("input_tokens").and_then(|p| p.as_u64()) {
                        state.input_tokens = it;
                    }
                    if let Some(cached) = usage.pointer("/input_tokens_details/cached_tokens").and_then(|c| c.as_u64()) {
                        state.cache_read_tokens = cached;
                        state.cache_creation_tokens = if cached > 0 { 0 } else { state.input_tokens };
                    }
                }
                if let Some(sr) = response.get("stop_reason").and_then(|s| s.as_str()) {
                    state.stop_reason = finish_reason_map(sr).to_string();
                }
            }
        }

        _ => {}
    }

    events
}

pub fn stream_responses_end(state: &ResponsesStreamState) -> Vec<String> {
    let mut events = Vec::new();

    if state.text_block_index != -1 {
        events.push(sse("content_block_stop", &json!({
            "type": "content_block_stop",
            "index": state.text_block_index,
        })));
    }
    // Emit tool block stops in index order, deduplicating since we store both item_id and call_id
    let mut seen_blocks = std::collections::HashSet::new();
    let mut tool_stops: Vec<usize> = state.tool_block_map.values()
        .filter(|&&idx| seen_blocks.insert(idx))
        .copied()
        .collect();
    tool_stops.sort_unstable();
    for block_idx in tool_stops {
        // Emit buffered arguments after stripping empty-string values (e.g. GPT generates pages: "")
        if let Some(args) = state.tool_block_args.get(&block_idx) {
            let cleaned = strip_empty_string_values(args);
            if !cleaned.is_empty() {
                events.push(sse("content_block_delta", &json!({
                    "type": "content_block_delta",
                    "index": block_idx,
                    "delta": {"type": "input_json_delta", "partial_json": cleaned},
                })));
            }
        }
        events.push(sse("content_block_stop", &json!({
            "type": "content_block_stop",
            "index": block_idx,
        })));
    }

    // Infer stop_reason: if any tool blocks were opened during streaming → tool_use
    let final_stop_reason = if !seen_blocks.is_empty() {
        "tool_use".to_string()
    } else {
        state.stop_reason.clone()
    };

    events.push(sse("message_delta", &json!({
        "type": "message_delta",
        "delta": {"stop_reason": final_stop_reason, "stop_sequence": null},
        "usage": {
            "output_tokens": state.output_tokens,
            "input_tokens": state.input_tokens,
            "cache_read_input_tokens": state.cache_read_tokens,
            "cache_creation_input_tokens": state.cache_creation_tokens,
        },
    })));
    events.push(sse("message_stop", &json!({"type": "message_stop"})));

    events
}
