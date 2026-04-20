use serde_json::{json, Value};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Request: Anthropic → OpenAI Responses API
// ---------------------------------------------------------------------------

fn user_content_blocks_to_responses(blocks: &[Value]) -> Vec<Value> {
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
                        let data_url = format!("data:{};base64,{}", media, data);
                        parts.push(json!({"type": "input_image", "image_url": data_url}));
                    } else if src_type == "url" {
                        let url = source.get("url").and_then(|t| t.as_str()).unwrap_or("");
                        parts.push(json!({"type": "input_image", "image_url": url}));
                    }
                }
            }
            _ => {}
        }
    }
    parts
}

fn anthropic_to_responses_input(messages: &[Value], system: Option<&Value>) -> (Option<String>, Vec<Value>) {
    let mut input: Vec<Value> = Vec::new();
    let mut instructions: Option<String> = None;

    // System → instructions
    if let Some(sys) = system {
        if !sys.is_null() {
            let sys_text = match sys {
                Value::String(s) => s.clone(),
                Value::Array(arr) => arr
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                _ => String::new(),
            };
            if !sys_text.is_empty() {
                instructions = Some(sys_text);
            }
        }
    }

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let content = msg.get("content").unwrap_or(&json!(null));

        if content.is_string() {
            let text = content.as_str().unwrap_or("");
            if role == "user" {
                input.push(json!({
                    "role": "user",
                    "content": [{"type": "input_text", "text": text}]
                }));
            } else if role == "assistant" {
                input.push(json!({
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text}]
                }));
            }
            continue;
        }

        if let Some(blocks) = content.as_array() {
            if role == "user" {
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

                // tool_results → function_call_output items
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
                    let call_id = tr.get("tool_use_id").and_then(|t| t.as_str()).unwrap_or("");
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": content_str,
                    }));
                }

                if !other.is_empty() {
                    let other_owned: Vec<Value> = other.into_iter().cloned().collect();
                    let parts = user_content_blocks_to_responses(&other_owned);
                    if !parts.is_empty() {
                        input.push(json!({"role": "user", "content": parts}));
                    }
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

                // Add assistant text as output_text content
                if !text_parts.is_empty() {
                    input.push(json!({
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text_parts.join("\n")}]
                    }));
                }

                // Add tool_use as function_call items
                for tu in &tool_uses {
                    let id = tu.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = tu.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let default_input = json!({});
                    let inp = tu.get("input").unwrap_or(&default_input);
                    input.push(json!({
                        "type": "function_call",
                        "id": id,
                        "call_id": id,
                        "name": name,
                        "arguments": serde_json::to_string(inp).unwrap_or_default(),
                    }));
                }
            }
        }
    }

    (instructions, input)
}

pub fn build_responses_request(anthropic_body: &Value, target_model: &str) -> Value {
    let messages = anthropic_body.get("messages").and_then(|m| m.as_array());
    let system = anthropic_body.get("system");

    let (instructions, input) = if let Some(msgs) = messages {
        anthropic_to_responses_input(msgs, system)
    } else {
        (None, Vec::new())
    };

    let max_tokens = anthropic_body.get("max_tokens").and_then(|m| m.as_u64()).unwrap_or(4096);
    let stream = anthropic_body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let mut req = json!({
        "model": target_model,
        "input": input,
        "max_output_tokens": max_tokens,
        "stream": stream,
        "store": false,
    });

    if let Some(inst) = instructions {
        req["instructions"] = json!(inst);
    }

    for key in &["temperature", "top_p"] {
        if let Some(val) = anthropic_body.get(*key) {
            req[*key] = val.clone();
        }
    }

    // Convert Anthropic tools to Responses function tools
    if let Some(tools) = anthropic_body.get("tools").and_then(|t| t.as_array()) {
        let response_tools: Vec<Value> = tools
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
        req["tools"] = json!(response_tools);

        // Forward tool_choice from Anthropic format
        if let Some(tc) = anthropic_body.get("tool_choice") {
            let tc_type = tc.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match tc_type {
                "auto" => { req["tool_choice"] = json!("auto"); }
                "any" => { req["tool_choice"] = json!("required"); }
                "tool" => {
                    let name = tc.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    req["tool_choice"] = json!({"type": "function", "name": name});
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

pub fn convert_responses_response(resp: &Value, claude_model: &str, message_id: &str) -> Value {
    let mut content_blocks: Vec<Value> = Vec::new();
    let mut stop_reason = "end_turn".to_string();

    if let Some(output) = resp.get("output").and_then(|o| o.as_array()) {
        for item in output {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for part in content {
                            let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            if part_type == "output_text" {
                                let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                if !text.is_empty() {
                                    content_blocks.push(json!({"type": "text", "text": text}));
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or(
                        item.get("id").and_then(|i| i.as_str()).unwrap_or("")
                    );
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let args_str = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                    let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                    content_blocks.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                    stop_reason = "tool_use".to_string();
                }
                _ => {}
            }
        }
    }

    // Map status to stop_reason
    if let Some(status) = resp.get("status").and_then(|s| s.as_str()) {
        match status {
            "completed" => { if stop_reason != "tool_use" { stop_reason = "end_turn".to_string(); } }
            "incomplete" => { stop_reason = "max_tokens".to_string(); }
            _ => {}
        }
    }

    let default_usage = json!({});
    let usage = resp.get("usage").unwrap_or(&default_usage);
    let input_tokens = usage.get("input_tokens").and_then(|p| p.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|c| c.as_u64()).unwrap_or(0);
    let cached_tokens = usage
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|c| c.as_u64())
        .unwrap_or(0);

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
            "cache_read_input_tokens": cached_tokens,
            "cache_creation_input_tokens": if cached_tokens > 0 { 0 } else { input_tokens },
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
    /// item.id → block_idx  (delta events use item_id = item.id)
    pub tool_block_map: HashMap<String, usize>,
    /// item.id → call_id  (call_id is used as Anthropic tool_use.id)
    pub item_id_to_call_id: HashMap<String, String>,
    /// item.id → name
    pub item_id_to_name: HashMap<String, String>,
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
            item_id_to_call_id: HashMap::new(),
            item_id_to_name: HashMap::new(),
            stop_reason: "end_turn".to_string(),
            output_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        }
    }
}

pub fn process_responses_stream_line(line: &str, state: &mut ResponsesStreamState) -> Vec<String> {
    let mut events = Vec::new();
    let trimmed = line.trim();

    // Responses API uses "event: <type>\ndata: <json>" format
    // We receive lines one at a time, so we look for "data:" lines
    if !trimmed.starts_with("data:") {
        return events;
    }
    let raw = trimmed[5..].trim();
    if raw.is_empty() || raw == "[DONE]" {
        return events;
    }

    let chunk: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return events,
    };

    let event_type = chunk.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        // Text output delta
        "response.output_text.delta" => {
            let delta_text = chunk.get("delta").and_then(|d| d.as_str()).unwrap_or("");
            if !delta_text.is_empty() {
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
                    "delta": {"type": "text_delta", "text": delta_text},
                })));
            }
        }
        // Text output done
        "response.output_text.done" => {
            if state.text_block_index != -1 {
                events.push(sse("content_block_stop", &json!({
                    "type": "content_block_stop",
                    "index": state.text_block_index,
                })));
                state.text_block_index = -1;
            }
        }
        // Function call arguments streaming
        "response.function_call_arguments.delta" => {
            // delta events carry item_id = item.id (NOT call_id)
            let item_id = chunk.get("item_id").and_then(|i| i.as_str())
                .or_else(|| chunk.get("call_id").and_then(|i| i.as_str()))
                .unwrap_or("");
            let delta_args = chunk.get("delta").and_then(|d| d.as_str()).unwrap_or("");

            if !state.tool_block_map.contains_key(item_id) {
                // Fallback: response.output_item.added may not have fired yet
                if state.text_block_index != -1 {
                    events.push(sse("content_block_stop", &json!({
                        "type": "content_block_stop",
                        "index": state.text_block_index,
                    })));
                    state.text_block_index = -1;
                }

                let block_idx = state.next_index;
                state.next_index += 1;
                state.tool_block_map.insert(item_id.to_string(), block_idx);

                let call_id = state.item_id_to_call_id.get(item_id).map(|s| s.as_str()).unwrap_or(item_id);
                let name = state.item_id_to_name.get(item_id).map(|s| s.as_str()).unwrap_or("");
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

            if !delta_args.is_empty() {
                let block_idx = state.tool_block_map.get(item_id).copied().unwrap_or(0);
                events.push(sse("content_block_delta", &json!({
                    "type": "content_block_delta",
                    "index": block_idx,
                    "delta": {"type": "input_json_delta", "partial_json": delta_args},
                })));
            }
        }
        // Function call done
        "response.function_call_arguments.done" => {
            let item_id = chunk.get("item_id").and_then(|i| i.as_str())
                .or_else(|| chunk.get("call_id").and_then(|i| i.as_str()))
                .unwrap_or("");
            if let Some(&block_idx) = state.tool_block_map.get(item_id) {
                events.push(sse("content_block_stop", &json!({
                    "type": "content_block_stop",
                    "index": block_idx,
                })));
            }
            state.stop_reason = "tool_use".to_string();
        }
        // Output item added — captures name and call_id before arguments start streaming
        "response.output_item.added" => {
            if let Some(item) = chunk.get("item") {
                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if item_type == "function_call" {
                    // item.id is what delta events reference via item_id
                    let item_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    // call_id is the Anthropic tool_use.id; if absent fall back to item.id
                    let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or(item_id);
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");

                    // Store mappings for use in delta/done events
                    state.item_id_to_call_id.insert(item_id.to_string(), call_id.to_string());
                    state.item_id_to_name.insert(item_id.to_string(), name.to_string());

                    // Close text block if open
                    if state.text_block_index != -1 {
                        events.push(sse("content_block_stop", &json!({
                            "type": "content_block_stop",
                            "index": state.text_block_index,
                        })));
                        state.text_block_index = -1;
                    }

                    if !state.tool_block_map.contains_key(item_id) {
                        let block_idx = state.next_index;
                        state.next_index += 1;
                        // Key by item.id so delta events (which use item_id) can find it
                        state.tool_block_map.insert(item_id.to_string(), block_idx);
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
        }
        // Response completed with usage
        "response.completed" => {
            if let Some(response) = chunk.get("response") {
                if let Some(status) = response.get("status").and_then(|s| s.as_str()) {
                    match status {
                        "completed" => { if state.tool_block_map.is_empty() { state.stop_reason = "end_turn".to_string(); } }
                        "incomplete" => { state.stop_reason = "max_tokens".to_string(); }
                        _ => {}
                    }
                }
                if let Some(usage) = response.get("usage") {
                    if let Some(it) = usage.get("input_tokens").and_then(|p| p.as_u64()) {
                        state.input_tokens = it;
                    }
                    if let Some(ot) = usage.get("output_tokens").and_then(|c| c.as_u64()) {
                        state.output_tokens = ot;
                    }
                    if let Some(cached) = usage.get("input_tokens_details").and_then(|d| d.get("cached_tokens")).and_then(|c| c.as_u64()) {
                        state.cache_read_tokens = cached;
                        state.cache_creation_tokens = if cached > 0 { 0 } else { state.input_tokens };
                    }
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

    events.push(sse("message_delta", &json!({
        "type": "message_delta",
        "delta": {"stop_reason": &state.stop_reason, "stop_sequence": null},
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
