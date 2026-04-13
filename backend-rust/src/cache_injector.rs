//! Cache 断点注入器
//!
//! 在请求转发前自动注入 cache_control 标记，提升 Claude Code 缓存命中率。
//! 参考 cc-switch 实现，最多注入 4 个断点：tools 末尾、system 末尾、最后 assistant 消息。

use serde_json::{json, Value};

/// 在请求体关键位置注入 cache_control 断点
pub fn inject_cache_control(body: &mut Value) {
    let existing = count_existing(body);
    let mut budget = 4_usize.saturating_sub(existing);
    if budget == 0 {
        return;
    }

    // (a) tools 数组末尾
    if budget > 0 {
        if let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) {
            if let Some(last) = tools.last_mut() {
                if last.get("cache_control").is_none() {
                    if let Some(o) = last.as_object_mut() {
                        o.insert("cache_control".to_string(), make_cache_control());
                    }
                    budget -= 1;
                }
            }
        }
    }

    // (b) system 末尾 — 如果是字符串先转为数组
    if budget > 0 {
        if body.get("system").and_then(|s| s.as_str()).is_some() {
            let text = body["system"].as_str().unwrap().to_string();
            body["system"] = json!([{"type": "text", "text": text}]);
        }

        if let Some(system) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
            if let Some(last) = system.last_mut() {
                if last.get("cache_control").is_none() {
                    if let Some(o) = last.as_object_mut() {
                        o.insert("cache_control".to_string(), make_cache_control());
                    }
                    budget -= 1;
                }
            }
        }
    }

    // (c) 最后一条 assistant 消息的最后一个非 thinking block
    if budget > 0 {
        if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            if let Some(assistant_msg) = messages
                .iter_mut()
                .rev()
                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
            {
                if let Some(content) = assistant_msg
                    .get_mut("content")
                    .and_then(|c| c.as_array_mut())
                {
                    if let Some(block) = content.iter_mut().rev().find(|b| {
                        let bt = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        bt != "thinking" && bt != "redacted_thinking"
                    }) {
                        if block.get("cache_control").is_none() {
                            if let Some(o) = block.as_object_mut() {
                                o.insert("cache_control".to_string(), make_cache_control());
                            }
                        }
                    }
                }
            }
        }
    }
}

fn make_cache_control() -> Value {
    json!({"type": "ephemeral"})
}

fn count_existing(body: &Value) -> usize {
    let mut count = 0;

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        count += tools.iter().filter(|t| t.get("cache_control").is_some()).count();
    }

    if let Some(system) = body.get("system").and_then(|s| s.as_array()) {
        count += system.iter().filter(|b| b.get("cache_control").is_some()).count();
    }

    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                count += content.iter().filter(|b| b.get("cache_control").is_some()).count();
            }
        }
    }

    count
}
