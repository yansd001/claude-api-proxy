use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_tool_name(messages: &[Value], tool_use_id: &str) -> String {
    for msg in messages {
        if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    && block.get("id").and_then(|i| i.as_str()) == Some(tool_use_id)
                {
                    return block.get("name").and_then(|n| n.as_str()).unwrap_or("unknown_tool").to_string();
                }
            }
        }
    }
    "unknown_tool".to_string()
}

fn system_to_str(system: &Value) -> String {
    match system {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Request: Anthropic → Gemini
// ---------------------------------------------------------------------------

fn content_to_gemini_parts(content: &Value, all_messages: &[Value]) -> Vec<Value> {
    if let Some(s) = content.as_str() {
        return vec![json!({"text": s})];
    }

    let blocks = match content.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut parts = Vec::new();
    for block in blocks {
        let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match btype {
            "text" => {
                let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                parts.push(json!({"text": text}));
            }
            "image" => {
                if let Some(source) = block.get("source") {
                    let src_type = source.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if src_type == "base64" {
                        let media = source.get("media_type").and_then(|t| t.as_str()).unwrap_or("");
                        let data = source.get("data").and_then(|t| t.as_str()).unwrap_or("");
                        parts.push(json!({
                            "inline_data": {
                                "mime_type": media,
                                "data": data,
                            }
                        }));
                    } else if src_type == "url" {
                        let url = source.get("url").and_then(|t| t.as_str()).unwrap_or("");
                        parts.push(json!({"text": format!("[Image URL: {}]", url)}));
                    }
                }
            }
            "tool_use" => {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let default_input = json!({});
                let input = block.get("input").unwrap_or(&default_input);
                parts.push(json!({
                    "functionCall": {
                        "name": name,
                        "args": input,
                    }
                }));
            }
            "tool_result" => {
                let default_content = json!("");
                let tr_content = block.get("content").unwrap_or(&default_content);
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

                let tool_use_id = block.get("tool_use_id").and_then(|t| t.as_str()).unwrap_or("");
                let fn_name = get_tool_name(all_messages, tool_use_id);
                parts.push(json!({
                    "functionResponse": {
                        "name": fn_name,
                        "response": {"result": content_str},
                    }
                }));
            }
            _ => {}
        }
    }
    parts
}

pub fn build_gemini_request(anthropic_body: &Value, target_model: &str) -> (String, Value) {
    let messages = anthropic_body.get("messages").and_then(|m| m.as_array());
    let mut contents: Vec<Value> = Vec::new();

    if let Some(msgs) = messages {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let gemini_role = if role == "assistant" { "model" } else { "user" };
            let content = msg.get("content").unwrap_or(&json!(null));
            let msgs_arr: Vec<Value> = messages.map(|m| m.to_vec()).unwrap_or_default();
            let parts = content_to_gemini_parts(content, &msgs_arr);
            if !parts.is_empty() {
                contents.push(json!({"role": gemini_role, "parts": parts}));
            }
        }
    }

    let mut body = json!({"contents": contents});

    if let Some(system) = anthropic_body.get("system") {
        if !system.is_null() {
            let s = system_to_str(system);
            if !s.is_empty() {
                body["system_instruction"] = json!({"parts": [{"text": s}]});
            }
        }
    }

    let mut gen_config = json!({});
    if let Some(mt) = anthropic_body.get("max_tokens") {
        gen_config["maxOutputTokens"] = mt.clone();
    }
    if let Some(temp) = anthropic_body.get("temperature") {
        gen_config["temperature"] = temp.clone();
    }
    if let Some(tp) = anthropic_body.get("top_p") {
        gen_config["topP"] = tp.clone();
    }
    if let Some(stop) = anthropic_body.get("stop_sequences") {
        gen_config["stopSequences"] = stop.clone();
    }
    if gen_config != json!({}) {
        body["generationConfig"] = gen_config;
    }

    if let Some(tools) = anthropic_body.get("tools").and_then(|t| t.as_array()) {
        let fn_decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                let mut decl = json!({
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": t.get("description").and_then(|d| d.as_str()).unwrap_or(""),
                });
                if let Some(schema) = t.get("input_schema") {
                    decl["parameters"] = schema.clone();
                }
                decl
            })
            .collect();
        body["tools"] = json!([{"function_declarations": fn_decls}]);
    }

    (target_model.to_string(), body)
}

// ---------------------------------------------------------------------------
// Response (non-streaming): Gemini → Anthropic
// ---------------------------------------------------------------------------

fn gemini_finish_reason_map(reason: &str) -> &str {
    match reason {
        "STOP" => "end_turn",
        "MAX_TOKENS" => "max_tokens",
        "SAFETY" | "RECITATION" => "stop_sequence",
        _ => "end_turn",
    }
}

pub fn convert_gemini_response(gemini_resp: &Value, claude_model: &str, message_id: &str) -> Value {
    let candidates = gemini_resp.get("candidates").and_then(|c| c.as_array());
    let mut content_blocks: Vec<Value> = Vec::new();
    let mut finish_reason = "end_turn".to_string();

    if let Some(cands) = candidates {
        if let Some(candidate) = cands.first() {
            finish_reason = gemini_finish_reason_map(
                candidate.get("finishReason").and_then(|f| f.as_str()).unwrap_or("STOP"),
            ).to_string();

            if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        content_blocks.push(json!({"type": "text", "text": text}));
                    } else if let Some(fc) = part.get("functionCall") {
                        let tool_id = format!("toolu_{}", &uuid::Uuid::new_v4().as_simple().to_string()[..16]);
                        content_blocks.push(json!({
                            "type": "tool_use",
                            "id": tool_id,
                            "name": fc.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                            "input": fc.get("args").unwrap_or(&json!({})),
                        }));
                    }
                }
            }
        }
    }

    if content_blocks.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use")) {
        finish_reason = "tool_use".to_string();
    }

    let default_usage = json!({});
    let usage_meta = gemini_resp.get("usageMetadata").unwrap_or(&default_usage);
    let input_tokens = usage_meta.get("promptTokenCount").and_then(|p| p.as_u64()).unwrap_or(0);
    let output_tokens = usage_meta.get("candidatesTokenCount").and_then(|c| c.as_u64()).unwrap_or(0);
    let cached_tokens = usage_meta.get("cachedContentTokenCount").and_then(|c| c.as_u64()).unwrap_or(0);

    json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": claude_model,
        "stop_reason": finish_reason,
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
// Streaming: Gemini SSE → Anthropic SSE
// ---------------------------------------------------------------------------

fn sse(event: &str, data: &Value) -> String {
    format!("event: {}\ndata: {}\n\n", event, serde_json::to_string(data).unwrap())
}

pub fn stream_gemini_start(claude_model: &str, message_id: &str) -> Vec<String> {
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

pub struct GeminiStreamState {
    pub next_index: usize,
    pub text_block_index: i32,
    pub stop_reason: String,
    pub output_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
}

impl Default for GeminiStreamState {
    fn default() -> Self {
        Self {
            next_index: 0,
            text_block_index: -1,
            stop_reason: "end_turn".to_string(),
            output_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
        }
    }
}

pub fn process_gemini_stream_line(line: &str, state: &mut GeminiStreamState) -> Vec<String> {
    let mut events = Vec::new();
    let trimmed = line.trim();
    if !trimmed.starts_with("data:") {
        return events;
    }
    let raw = trimmed[5..].trim();
    if raw.is_empty() {
        return events;
    }

    let chunk: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return events,
    };

    if let Some(usage_meta) = chunk.get("usageMetadata") {
        if let Some(ot) = usage_meta.get("candidatesTokenCount").and_then(|c| c.as_u64()) {
            state.output_tokens = ot;
        }
        if let Some(pt) = usage_meta.get("promptTokenCount").and_then(|p| p.as_u64()) {
            state.input_tokens = pt;
        }
        if let Some(ct) = usage_meta.get("cachedContentTokenCount").and_then(|c| c.as_u64()) {
            state.cache_read_tokens = ct;
        }
    }

    let candidates = match chunk.get("candidates").and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return events,
    };
    if candidates.is_empty() {
        return events;
    }

    let candidate = &candidates[0];

    if let Some(finish) = candidate.get("finishReason").and_then(|f| f.as_str()) {
        if !finish.is_empty() && finish != "FINISH_REASON_UNSPECIFIED" {
            state.stop_reason = gemini_finish_reason_map(finish).to_string();
        }
    }

    if let Some(parts) = candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
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
                    "delta": {"type": "text_delta", "text": text},
                })));
            } else if let Some(fc) = part.get("functionCall") {
                // Close text block if open
                if state.text_block_index != -1 {
                    events.push(sse("content_block_stop", &json!({
                        "type": "content_block_stop",
                        "index": state.text_block_index,
                    })));
                    state.text_block_index = -1;
                }

                let block_idx = state.next_index;
                state.next_index += 1;
                let tool_id = format!("toolu_{}", &uuid::Uuid::new_v4().as_simple().to_string()[..16]);
                let args_str = serde_json::to_string(fc.get("args").unwrap_or(&json!({}))).unwrap_or_default();

                events.push(sse("content_block_start", &json!({
                    "type": "content_block_start",
                    "index": block_idx,
                    "content_block": {
                        "type": "tool_use",
                        "id": tool_id,
                        "name": fc.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "input": {},
                    }
                })));
                events.push(sse("content_block_delta", &json!({
                    "type": "content_block_delta",
                    "index": block_idx,
                    "delta": {"type": "input_json_delta", "partial_json": args_str},
                })));
                events.push(sse("content_block_stop", &json!({
                    "type": "content_block_stop",
                    "index": block_idx,
                })));
                state.stop_reason = "tool_use".to_string();
            }
        }
    }

    events
}

pub fn stream_gemini_end(state: &GeminiStreamState) -> Vec<String> {
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
            "cache_creation_input_tokens": if state.cache_read_tokens > 0 { 0 } else { state.input_tokens },
        },
    })));
    events.push(sse("message_stop", &json!({"type": "message_stop"})));

    events
}
