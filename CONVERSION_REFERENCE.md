# 参数转换参考文档

本文档描述代理将 **Anthropic Messages API** 格式转换为三种目标格式时所有字段的映射关系。

---

## 目录

1. [OpenAI Chat Completions](#1-openai-chat-completions)
2. [OpenAI Responses API](#2-openai-responses-api)
3. [Gemini generateContent](#3-gemini-generatecontent)
4. [响应回转换（三者通用）](#4-响应回转换三者通用)
5. [通用清洗规则](#5-通用清洗规则)

---

## 1. OpenAI Chat Completions

**端点**：`POST /v1/chat/completions`

### 1.1 请求字段映射

| Anthropic 字段 | OpenAI 字段 | 转换说明 |
|---|---|---|
| `model` | `model` | 替换为配置的 `target_model` |
| `messages` + `system` | `messages[]` | 见下方消息转换规则 |
| `max_tokens` | `max_tokens` | 直接映射 |
| `stream` | `stream` | 直接映射；流式时自动追加 `stream_options.include_usage: true` |
| `temperature` | `temperature` | 直接映射（可选） |
| `top_p` | `top_p` | 直接映射（可选） |
| `stop_sequences` | `stop` | 直接映射 |
| `tools[].name` | `tools[].function.name` | 嵌套在 `function` 下 |
| `tools[].description` | `tools[].function.description` | 嵌套在 `function` 下 |
| `tools[].input_schema` | `tools[].function.parameters` | 直接映射 JSON Schema |
| `tools[].cache_control` | `tools[].cache_control` | 透传（兼容部分代理） |
| `tool_choice.type: "auto"` | `tool_choice: "auto"` | 字符串形式 |
| `tool_choice.type: "any"` | `tool_choice: "required"` | 映射为 required |
| `tool_choice.type: "tool"` | `tool_choice: {"type":"function","function":{"name":"..."}}` | 强制指定函数 |

### 1.2 消息（messages）转换规则

#### system 字段

| Anthropic | OpenAI |
|---|---|
| `system: "string"` | `{"role":"system","content":"string"}` 插入首位 |
| `system: [{type:"text", text:"...", cache_control:{...}}]` | 每个 block 生成独立的 `{"role":"system","content":"...","cache_control":{...}}` |

#### user 消息

| Anthropic content block | OpenAI 格式 |
|---|---|
| `{type:"text", text:"..."}` 单个且无 cache_control | `content: "string"` 直接展开为字符串 |
| `{type:"text", text:"...", cache_control:{...}}` | `content: [{type:"text", text:"...", cache_control:{...}}]` 保留数组 |
| `{type:"image", source:{type:"base64", media_type, data}}` | `{type:"image_url", image_url:{url:"data:{media};base64,{data}"}}` |
| `{type:"image", source:{type:"url", url:"..."}}` | `{type:"image_url", image_url:{url:"..."}}` |
| `{type:"tool_result", tool_use_id, content}` | 独立的 `{"role":"tool","tool_call_id":"...","content":"..."}` 消息 |
| `{type:"thinking"}` / `{type:"redacted_thinking"}` | **丢弃**（不转发） |

#### assistant 消息

| Anthropic content block | OpenAI 格式 |
|---|---|
| `{type:"text", text:"..."}` | `message.content: "..."` |
| 无 text block | `message.content: null` |
| `{type:"tool_use", id, name, input}` | `message.tool_calls: [{id, type:"function", function:{name, arguments: JSON.stringify(input)}}]` |
| `{type:"thinking"}` / `{type:"redacted_thinking"}` | **丢弃** |

---

## 2. OpenAI Responses API

**端点**：`POST /v1/responses`

与 Chat Completions 的主要差异：使用 `input` 代替 `messages`，`max_output_tokens` 代替 `max_tokens`，工具定义结构不同，工具结果格式不同。

### 2.1 请求字段映射

| Anthropic 字段 | Responses API 字段 | 转换说明 |
|---|---|---|
| `model` | `model` | 替换为配置的 `target_model` |
| `messages` + `system` | `input[]` | 见下方 input 转换规则 |
| `max_tokens` | `max_output_tokens` | **字段名不同** |
| `stream` | `stream` | 直接映射 |
| `temperature` | `temperature` | 直接映射（可选） |
| `top_p` | `top_p` | 直接映射（可选） |
| `tools[].name` | `tools[].name` | 与 Chat Completions 不同，**不嵌套** |
| `tools[].description` | `tools[].description` | 直接映射 |
| `tools[].input_schema` | `tools[].parameters` | 直接映射 JSON Schema |
| `tools[]` 固定追加 | `tools[].type: "function"` | 必填 |
| `tool_choice.type: "auto"` | `tool_choice: "auto"` | 字符串形式 |
| `tool_choice.type: "any"` | `tool_choice: "required"` | 映射为 required |
| `tool_choice.type: "tool"` | `tool_choice: {"type":"function","name":"..."}` | **注意**：无 `function` 嵌套层 |

### 2.2 input 数组转换规则

#### system 字段

| Anthropic | Responses API input 条目 |
|---|---|
| `system: "string"` | `{"role":"system","content":"string"}` |
| `system: [{type:"text", text:"..."}]` | 合并所有 text block，`{"role":"system","content":"合并文本"}` |

#### user 消息 content block

| Anthropic block | Responses API 格式 |
|---|---|
| `{type:"text", text:"..."}` 单个 | `content: "string"` 直接展开 |
| 多个 text/image block | `content: [{type:"input_text", text:"..."}, ...]` |
| `{type:"image", source:{type:"base64"}}` | `{type:"input_image", image_url:"data:{media};base64,{data}"}` |
| `{type:"image", source:{type:"url"}}` | `{type:"input_image", image_url:"..."}` |
| `{type:"tool_result", tool_use_id, content}` | 顶层条目 `{type:"function_call_output", call_id:"...", output:"..."}` |
| `{type:"thinking"}` / `{type:"redacted_thinking"}` | **丢弃** |

#### assistant 消息 content block

| Anthropic block | Responses API input 条目 |
|---|---|
| `{type:"text", text:"..."}` | `{"role":"assistant","content":"..."}` |
| `{type:"tool_use", id, name, input}` | 顶层条目 `{type:"function_call", call_id:id, name, arguments: JSON.stringify(input)}` |
| `{type:"thinking"}` / `{type:"redacted_thinking"}` | **丢弃** |

---

## 3. Gemini generateContent

**端点**：
- 非流式：`POST /v1beta/models/{model}:generateContent?key={api_key}`
- 流式：`POST /v1beta/models/{model}:streamGenerateContent?key={api_key}&alt=sse`

### 3.1 请求字段映射

| Anthropic 字段 | Gemini 字段 | 转换说明 |
|---|---|---|
| `model` | URL 路径中的 `{model}` | 替换为配置的 `target_model` |
| `messages` | `contents[]` | 见下方 contents 转换规则 |
| `system` | `system_instruction.parts[0].text` | 提取纯文本后放入 system_instruction |
| `max_tokens` | `generationConfig.maxOutputTokens` | 嵌套在 generationConfig 下 |
| `temperature` | `generationConfig.temperature` | 嵌套在 generationConfig 下 |
| `top_p` | `generationConfig.topP` | 注意大写 P |
| `stop_sequences` | `generationConfig.stopSequences` | 直接映射 |
| `tools[].name` | `tools[0].function_declarations[].name` | 展平到 function_declarations 数组 |
| `tools[].description` | `tools[0].function_declarations[].description` | 同上 |
| `tools[].input_schema` | `tools[0].function_declarations[].parameters` | 直接映射 JSON Schema |
| `tool_choice.type: "auto"` | `tool_config.function_calling_config.mode: "AUTO"` | |
| `tool_choice.type: "any"` | `tool_config.function_calling_config.mode: "ANY"` | |
| `tool_choice.type: "tool"` | `tool_config.function_calling_config.mode: "ANY"` + `allowed_function_names: [name]` | |

### 3.2 消息（contents）转换规则

| Anthropic role | Gemini role |
|---|---|
| `user` | `user` |
| `assistant` | `model` |

#### content block 映射

| Anthropic block | Gemini parts 条目 |
|---|---|
| `{type:"text", text:"..."}` | `{text: "..."}` |
| `{type:"image", source:{type:"base64"}}` | `{inline_data:{mime_type, data}}` |
| `{type:"image", source:{type:"url"}}` | `{text: "[Image URL: ...]"}`（降级处理，Gemini 不支持直接 URL 图片） |
| `{type:"tool_use", name, input}` | `{functionCall:{name, args: input}}` |
| `{type:"tool_result", tool_use_id, content}` | `{functionResponse:{name: 反查工具名, response:{result:"..."}}}` |
| `{type:"thinking"}` / `{type:"redacted_thinking"}` | **丢弃** |

> **注意**：Gemini 的 `functionResponse` 需要工具名称，代理会通过遍历历史消息中的 `tool_use` block 反查 `tool_use_id` 对应的名称。

---

## 4. 响应回转换（三者通用）

### 4.1 stop_reason 映射

| 上游 finish_reason | Anthropic stop_reason |
|---|---|
| `stop` | `end_turn` |
| `tool_calls` | `tool_use` |
| `length` | `max_tokens` |
| `content_filter` | `stop_sequence` |
| `max_output_tokens`（Responses API） | `max_tokens` |
| `STOP`（Gemini） | `end_turn` |
| `MAX_TOKENS`（Gemini） | `max_tokens` |
| `SAFETY` / `RECITATION`（Gemini） | `stop_sequence` |
| 有 tool_use block（任意后端） | 强制覆盖为 `tool_use` |

### 4.2 usage token 映射

| 上游字段 | Anthropic usage 字段 | 说明 |
|---|---|---|
| OpenAI `prompt_tokens` | `input_tokens` | |
| OpenAI `completion_tokens` | `output_tokens` | |
| OpenAI `prompt_tokens_details.cached_tokens` | `cache_read_input_tokens` | |
| Responses API `input_tokens` | `input_tokens` | |
| Responses API `output_tokens` | `output_tokens` | |
| Responses API `input_tokens_details.cached_tokens` | `cache_read_input_tokens` | |
| Gemini `promptTokenCount` | `input_tokens` | |
| Gemini `candidatesTokenCount` | `output_tokens` | |
| Gemini `cachedContentTokenCount` | `cache_read_input_tokens` | |
| 所有后端 | `cache_creation_input_tokens: 0` | OpenAI/Gemini 无显式缓存写入计费 |

### 4.3 工具调用响应（tool_use block）

| 上游格式 | Anthropic block |
|---|---|
| OpenAI `message.tool_calls[].function.{name,arguments}` | `{type:"tool_use", id, name, input: JSON.parse(arguments)}` |
| Responses API `output[].type=="function_call"` `.{call_id,name,arguments}` | `{type:"tool_use", id:call_id, name, input: JSON.parse(arguments)}` |
| Gemini `parts[].functionCall.{name,args}` | `{type:"tool_use", id:"toolu_{random16}", name, input: args}` |

### 4.4 流式事件映射

所有后端均转换为 Anthropic SSE 事件流格式：

| Anthropic SSE 事件 | 含义 |
|---|---|
| `message_start` | 流开始，带初始 usage |
| `ping` | 心跳 |
| `content_block_start` | 新 block 开始（text 或 tool_use） |
| `content_block_delta` | block 内容增量（text_delta 或 input_json_delta） |
| `content_block_stop` | block 结束 |
| `message_delta` | 流结束前的 stop_reason + 最终 usage |
| `message_stop` | 流结束 |

**OpenAI Chat Completions 流式特殊处理**：工具调用参数 (`arguments` delta) 在流式过程中**缓冲**，在流结束时统一清洗后作为单个 `input_json_delta` 发出，避免中间片段触发客户端校验。

**OpenAI Responses API 流式事件映射**：

| Responses API 事件 | 映射到 |
|---|---|
| `response.output_text.delta` | `content_block_delta` (text_delta) |
| `response.output_item.added` (function_call) | `content_block_start` (tool_use) |
| `response.function_call_arguments.delta` | 缓冲到内存 |
| `response.completed` | 提取 usage 和 stop_reason |
| 流结束时 | 清洗参数 → `content_block_delta` (input_json_delta) → `content_block_stop` |

---

## 5. 通用清洗规则

工具调用参数在转发给客户端前会经过 `strip_empty_values` 递归清洗，移除以下值（避免 LLM 生成的空可选参数导致客户端 JSON Schema 校验失败）：

| 类型 | 示例 | 处理 |
|---|---|---|
| `null` 字段 | `"pages": null` | 移除 |
| 空字符串字段 | `"pages": ""` | 移除 |
| 空数组字段 | `"pages": []` | 移除 |
| 嵌套对象递归 | `"opts": {"x": ""}` | 递归清洗内层 |

该清洗应用于：
- OpenAI Chat Completions 非流式响应（`convert_openai_response`）
- OpenAI Chat Completions 流式末尾（`stream_openai_end`）
- OpenAI Responses API 非流式响应（`convert_responses_response`）
- OpenAI Responses API 流式末尾（`stream_responses_end`）
- Gemini 非流式响应（`convert_gemini_response`）
- Gemini 流式处理（`process_gemini_stream_line`）
