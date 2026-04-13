use serde_json::{json, Value};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Request: Anthropic → OpenAI
// ---------------------------------------------------------------------------

fn user_content_blocks_to_openai(blocks: &[Value]) -> Value {
    let mut parts: Vec<Value> = Vec::new();
    for block in blocks {
        let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match btype {
            "text" => {
                let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                let mut part = json!({"type": "text", "text": text});
                if let Some(cc) = block.get("cache_control") {
                    part["cache_control"] = cc.clone();
                }
                parts.push(part);
            }
            "image" => {
                if let Some(source) = block.get("source") {
                    let src_type = source.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if src_type == "base64" {
                        let media = source.get("media_type").and_then(|t| t.as_str()).unwrap_or("");
                        let data = source.get("data").and_then(|t| t.as_str()).unwrap_or("");
                        let data_url = format!("data:{};base64,{}", media, data);
                        parts.push(json!({"type": "image_url", "image_url": {"url": data_url}}));
                    } else if src_type == "url" {
                        let url = source.get("url").and_then(|t| t.as_str()).unwrap_or("");
                        parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
                    }
                }
            }
            _ => {}
        }
    }
    if parts.len() == 1 && parts[0].get("type").and_then(|t| t.as_str()) == Some("text") {
        // Keep array format if cache_control is present
        if parts[0].get("cache_control").is_some() {
            json!(parts)
        } else {
            parts[0].get("text").cloned().unwrap_or(json!(""))
        }
    } else if parts.is_empty() {
        json!("")
    } else {
        json!(parts)
    }
}

fn anthropic_messages_to_openai(messages: &[Value], system: Option<&Value>) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();

    if let Some(sys) = system {
        if !sys.is_null() {
            match sys {
                Value::String(s) => {
                    if !s.is_empty() {
                        result.push(json!({"role": "system", "content": s}));
                    }
                }
                Value::Array(arr) => {
                    // Preserve cache_control per system block
                    for block in arr {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            let mut sys_msg = json!({"role": "system", "content": text});
                            if let Some(cc) = block.get("cache_control") {
                                sys_msg["cache_control"] = cc.clone();
                            }
                            result.push(sys_msg);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let content = msg.get("content").unwrap_or(&json!(null));

        if content.is_string() {
            result.push(json!({"role": role, "content": content}));
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
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content_str,
                    }));
                }

                if !other.is_empty() {
                    let other_owned: Vec<Value> = other.into_iter().cloned().collect();
                    let converted = user_content_blocks_to_openai(&other_owned);
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
                // Skip thinking and redacted_thinking blocks

                let mut msg_obj = json!({"role": "assistant"});
                if !text_parts.is_empty() {
                    msg_obj["content"] = json!(text_parts.join("\n"));
                } else {
                    msg_obj["content"] = json!(null);
                }

                if !tool_uses.is_empty() {
                    let calls: Vec<Value> = tool_uses
                        .iter()
                        .map(|tu| {
                            let id = tu.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let name = tu.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let default_input = json!({});
                            let input = tu.get("input").unwrap_or(&default_input);
                            json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(input).unwrap_or_default(),
                                }
                            })
                        })
                        .collect();
                    msg_obj["tool_calls"] = json!(calls);
                }
                result.push(msg_obj);
            }
        }
    }

    result
}

pub fn build_openai_request(anthropic_body: &Value, target_model: &str) -> Value {
    let messages = anthropic_body.get("messages").and_then(|m| m.as_array());
    let system = anthropic_body.get("system");

    let openai_messages = if let Some(msgs) = messages {
        anthropic_messages_to_openai(msgs, system)
    } else {
        Vec::new()
    };

    let max_tokens = anthropic_body.get("max_tokens").and_then(|m| m.as_u64()).unwrap_or(4096);
    let stream = anthropic_body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let mut req = json!({
        "model": target_model,
        "messages": openai_messages,
        "max_tokens": max_tokens,
        "stream": stream,
    });

    // Request usage stats in streaming mode (needed for cache token mapping)
    if stream {
        req["stream_options"] = json!({"include_usage": true});
    }

    for key in &["temperature", "top_p"] {
        if let Some(val) = anthropic_body.get(*key) {
            req[*key] = val.clone();
        }
    }

    if let Some(stop) = anthropic_body.get("stop_sequences") {
        req["stop"] = stop.clone();
    }

    if let Some(tools) = anthropic_body.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let desc = t.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let default_schema = json!({"type": "object", "properties": {}});
                let schema = t.get("input_schema").unwrap_or(&default_schema);
                let mut tool = json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": desc,
                        "parameters": schema,
                    }
                });
                // Preserve cache_control for compatible proxies
                if let Some(cc) = t.get("cache_control") {
                    tool["cache_control"] = cc.clone();
                }
                tool
            })
            .collect();
        req["tools"] = json!(openai_tools);

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
// Response (non-streaming): OpenAI → Anthropic
// ---------------------------------------------------------------------------

fn finish_reason_map(reason: &str) -> &str {
    match reason {
        "stop" => "end_turn",
        "tool_calls" => "tool_use",
        "length" => "max_tokens",
        "content_filter" => "stop_sequence",
        _ => "end_turn",
    }
}

pub fn convert_openai_response(openai_resp: &Value, claude_model: &str, message_id: &str) -> Value {
    let choice = &openai_resp["choices"][0];
    let message = &choice["message"];
    let mut content_blocks: Vec<Value> = Vec::new();

    if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
        if !text.is_empty() {
            content_blocks.push(json!({"type": "text", "text": text}));
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let func = &tc["function"];
            let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args_str = func.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
            content_blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }));
        }
    }

    let finish_reason = choice.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("stop");
    let stop_reason = finish_reason_map(finish_reason);
    let default_usage = json!({});
    let usage = openai_resp.get("usage").unwrap_or(&default_usage);

    // Extract cache tokens: OpenAI uses prompt_tokens_details.cached_tokens
    let input_tokens = usage.get("prompt_tokens").and_then(|p| p.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("completion_tokens").and_then(|c| c.as_u64()).unwrap_or(0);
    let cached_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(|c| c.as_u64())
        .unwrap_or(0);

    // Map to Anthropic format: cached portion → cache_read, remainder → input
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
// Streaming: OpenAI SSE → Anthropic SSE
// ---------------------------------------------------------------------------

fn sse(event: &str, data: &Value) -> String {
    format!("event: {}\ndata: {}\n\n", event, serde_json::to_string(data).unwrap())
}

pub fn stream_openai_start(claude_model: &str, message_id: &str) -> Vec<String> {
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

pub struct OpenAIStreamState {
    pub next_index: usize,
    pub text_block_index: i32,
    pub tool_block_map: HashMap<usize, usize>,
    pub stop_reason: String,
    pub output_tokens: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl Default for OpenAIStreamState {
    fn default() -> Self {
        Self {
            next_index: 0,
            text_block_index: -1,
            tool_block_map: HashMap::new(),
            stop_reason: "end_turn".to_string(),
            output_tokens: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        }
    }
}

pub fn process_openai_stream_line(line: &str, state: &mut OpenAIStreamState) -> Vec<String> {
    let mut events = Vec::new();
    let trimmed = line.trim();
    if !trimmed.starts_with("data:") {
        return events;
    }
    let raw = trimmed[5..].trim();
    if raw == "[DONE]" {
        return events;
    }

    let chunk: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return events,
    };

    if let Some(usage) = chunk.get("usage") {
        if let Some(ct) = usage.get("completion_tokens").and_then(|c| c.as_u64()) {
            state.output_tokens = ct;
        }
        if let Some(pt) = usage.get("prompt_tokens").and_then(|p| p.as_u64()) {
            state.input_tokens = pt;
        }
        // Extract cached tokens from OpenAI's prompt_tokens_details
        if let Some(cached) = usage.pointer("/prompt_tokens_details/cached_tokens").and_then(|c| c.as_u64()) {
            state.cache_read_tokens = cached;
            // If we have cached tokens, no creation; otherwise all input is "created"
            state.cache_creation_tokens = if cached > 0 { 0 } else { state.input_tokens };
        }
    }

    let choices = match chunk.get("choices").and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return events,
    };
    if choices.is_empty() {
        return events;
    }

    let choice = &choices[0];
    let default_delta = json!({});
    let delta = choice.get("delta").unwrap_or(&default_delta);

    // Text content
    if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
        if !text.is_empty() {
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
        }
    }

    // Tool calls
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
        for tc_delta in tool_calls {
            let tc_idx = tc_delta.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let tc_id = tc_delta.get("id").and_then(|i| i.as_str());

            if let Some(id) = tc_id {
                // Close text block
                if state.text_block_index != -1 {
                    events.push(sse("content_block_stop", &json!({
                        "type": "content_block_stop",
                        "index": state.text_block_index,
                    })));
                    state.text_block_index = -1;
                }

                let block_idx = state.next_index;
                state.next_index += 1;
                state.tool_block_map.insert(tc_idx, block_idx);
                let default_func = json!({});
                let func = tc_delta.get("function").unwrap_or(&default_func);
                let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                events.push(sse("content_block_start", &json!({
                    "type": "content_block_start",
                    "index": block_idx,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": {},
                    }
                })));
            }

            if let Some(args) = tc_delta.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()) {
                if !args.is_empty() {
                    let block_idx = state.tool_block_map.get(&tc_idx).copied().unwrap_or(state.next_index.saturating_sub(1));
                    events.push(sse("content_block_delta", &json!({
                        "type": "content_block_delta",
                        "index": block_idx,
                        "delta": {"type": "input_json_delta", "partial_json": args},
                    })));
                }
            }
        }
    }

    if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
        state.stop_reason = finish_reason_map(fr).to_string();
    }

    events
}

pub fn stream_openai_end(state: &OpenAIStreamState) -> Vec<String> {
    let mut events = Vec::new();

    if state.text_block_index != -1 {
        events.push(sse("content_block_stop", &json!({
            "type": "content_block_stop",
            "index": state.text_block_index,
        })));
    }
    for block_idx in state.tool_block_map.values() {
        events.push(sse("content_block_stop", &json!({
            "type": "content_block_stop",
            "index": block_idx,
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
