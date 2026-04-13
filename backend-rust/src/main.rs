mod auth;
mod cache_injector;
mod config;
mod converters;

use axum::{
    Router,
    body::Body,
    extract::{Json, Path, Request},
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Json as JsonResponse, Redirect, Response},
    routing::{get, post, put},
};
use config::{Config, Provider, load_config, new_provider_id, save_config};
use converters::gemini_conv;
use converters::openai_conv;
use futures::StreamExt;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

// ---------------------------------------------------------------------------
// Request logging middleware
// ---------------------------------------------------------------------------

async fn log_request(req: Request, next: middleware::Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = std::time::Instant::now();
    println!("--> {} {}", method, uri);
    let response = next.run(req).await;
    let elapsed = start.elapsed();
    println!("<-- {} {} {} ({:.1}ms)", method, uri, response.status().as_u16(), elapsed.as_secs_f64() * 1000.0);
    response
}

// ---------------------------------------------------------------------------
// Helper: resolve provider + target model
// ---------------------------------------------------------------------------

fn resolve_provider(claude_model: &str, config: &Config) -> Result<(Provider, String), (StatusCode, JsonResponse<Value>)> {
    for mapping in &config.model_mappings {
        if mapping.claude_model == claude_model {
            for p in &config.providers {
                if p.id == mapping.provider_id && p.enabled {
                    return Ok((p.clone(), mapping.target_model.clone()));
                }
            }
            break;
        }
    }

    // Fall back to first enabled provider
    for p in &config.providers {
        if p.enabled {
            let target = p.models.first().map(|s| s.as_str()).unwrap_or(claude_model);
            return Ok((p.clone(), target.to_string()));
        }
    }

    Err((
        StatusCode::BAD_REQUEST,
        JsonResponse(json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": format!("No provider configured for model '{}'. Please configure providers and model mappings in the UI.", claude_model),
            }
        })),
    ))
}

fn make_auth_headers(provider: &Provider) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {}", provider.api_key).parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    headers
}

// ---------------------------------------------------------------------------
// Anthropic direct forwarding
// ---------------------------------------------------------------------------

async fn forward_anthropic_direct(req_headers: &HeaderMap, body: &Value, direct_cfg: &config::AnthropicDirect) -> Response {
    let base_url = direct_cfg.base_url.trim_end_matches('/');
    let url = format!("{}/v1/messages", base_url);
    let is_stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let mut headers = reqwest::header::HeaderMap::new();
    let forward_headers = ["anthropic-version", "anthropic-beta", "x-api-key", "content-type"];
    for name in &forward_headers {
        if let Some(val) = req_headers.get(*name) {
            if let Ok(v) = val.to_str() {
                if let Ok(hname) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                    headers.insert(hname, v.parse().unwrap());
                }
            }
        }
    }
    // Ensure anthropic-version has a default
    headers.entry("anthropic-version").or_insert("2023-06-01".parse().unwrap());
    if !direct_cfg.api_key.is_empty() {
        headers.insert("x-api-key", direct_cfg.api_key.parse().unwrap());
        headers.remove("authorization");
    }
    headers.entry("content-type").or_insert("application/json".parse().unwrap());

    let client = reqwest::Client::new();

    if is_stream {
        let resp = match client.post(&url).headers(headers).json(body).timeout(std::time::Duration::from_secs(600)).send().await {
            Ok(r) => r,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
            }
        };

        if resp.status().is_client_error() || resp.status().is_server_error() {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body = resp.text().await.unwrap_or_default();
            return (status, body).into_response();
        }

        let stream = resp.bytes_stream();
        let body = Body::from_stream(stream);
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(body)
            .unwrap();
    }

    let resp = match client.post(&url).headers(headers).json(body).timeout(std::time::Duration::from_secs(600)).send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body_text = resp.text().await.unwrap_or_default();
    if status.is_client_error() || status.is_server_error() {
        let val: Value = serde_json::from_str(&body_text).unwrap_or(json!({"detail": body_text}));
        return (status, JsonResponse(val)).into_response();
    }
    let val: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
    JsonResponse(val).into_response()
}

// ---------------------------------------------------------------------------
// POST /v1/messages
// ---------------------------------------------------------------------------

async fn messages_handler(req: Request) -> Response {
    let config = load_config();
    let headers = req.headers().clone();
    let body_bytes = match axum::body::to_bytes(req.into_body(), 100 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": e.to_string()}))).into_response();
        }
    };
    let mut body: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": e.to_string()}))).into_response();
        }
    };

    // Inject cache_control breakpoints for Anthropic direct mode
    if config.anthropic_direct.enabled {
        cache_injector::inject_cache_control(&mut body);
    }

    // Anthropic direct forwarding
    if config.anthropic_direct.enabled {
        return forward_anthropic_direct(&headers, &body, &config.anthropic_direct).await;
    }

    let claude_model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    let message_id = format!("msg_{}", &uuid::Uuid::new_v4().as_simple().to_string()[..24]);
    let is_stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let (provider, target_model) = match resolve_provider(claude_model, &config) {
        Ok(r) => r,
        Err((status, json)) => return (status, json).into_response(),
    };

    let provider_type = &provider.provider_type;

    match provider_type.as_str() {
        "openai" => handle_openai(&provider, &body, &target_model, claude_model, &message_id, is_stream).await,
        "gemini" => handle_gemini(&provider, &body, &target_model, claude_model, &message_id, is_stream).await,
        _ => (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": format!("Unknown provider type: {}", provider_type)}))).into_response(),
    }
}

async fn handle_openai(provider: &Provider, body: &Value, target_model: &str, claude_model: &str, message_id: &str, is_stream: bool) -> Response {
    let mut base_url = provider.base_url.trim_end_matches('/').to_string();
    if !base_url.ends_with("/v1") {
        base_url = format!("{}/v1", base_url);
    }
    let url = format!("{}/chat/completions", base_url);
    let openai_req = openai_conv::build_openai_request(body, target_model);
    let headers = make_auth_headers(provider);
    let client = reqwest::Client::new();

    if is_stream {
        let claude_model = claude_model.to_string();
        let message_id = message_id.to_string();

        let resp = match client.post(&url).headers(headers).json(&openai_req).timeout(std::time::Duration::from_secs(600)).send().await {
            Ok(r) => r,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
            }
        };

        if resp.status().is_client_error() || resp.status().is_server_error() {
            let err_body = resp.text().await.unwrap_or_default();
            let event = format!("data: {}\n\n", err_body);
            return Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(event))
                .unwrap();
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(100);

        tokio::spawn(async move {
            // Send start events
            for event in openai_conv::stream_openai_start(&claude_model, &message_id) {
                if tx.send(Ok(event)).await.is_err() { return; }
            }

            let mut state = openai_conv::OpenAIStreamState::default();
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].to_string();
                            buffer = buffer[pos + 1..].to_string();
                            for event in openai_conv::process_openai_stream_line(&line, &mut state) {
                                if tx.send(Ok(event)).await.is_err() { return; }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Process remaining buffer
            if !buffer.trim().is_empty() {
                for event in openai_conv::process_openai_stream_line(&buffer, &mut state) {
                    let _ = tx.send(Ok(event)).await;
                }
            }

            for event in openai_conv::stream_openai_end(&state) {
                let _ = tx.send(Ok(event)).await;
            }
        });

        let stream = ReceiverStream::new(rx);
        let body = Body::from_stream(stream);
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(body)
            .unwrap();
    }

    // Non-streaming
    let resp = match client.post(&url).headers(headers).json(&openai_req).timeout(std::time::Duration::from_secs(600)).send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
        }
    };

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if status.is_client_error() || status.is_server_error() {
        let val: Value = serde_json::from_str(&body_text).unwrap_or(json!({"detail": body_text}));
        return (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), JsonResponse(val)).into_response();
    }
    let openai_resp: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
    JsonResponse(openai_conv::convert_openai_response(&openai_resp, claude_model, message_id)).into_response()
}

async fn handle_gemini(provider: &Provider, body: &Value, target_model: &str, claude_model: &str, message_id: &str, is_stream: bool) -> Response {
    let base_url = if provider.base_url.is_empty() {
        "https://generativelanguage.googleapis.com".to_string()
    } else {
        provider.base_url.trim_end_matches('/').to_string()
    };
    let api_key = &provider.api_key;
    let (gemini_model, gemini_body) = gemini_conv::build_gemini_request(body, target_model);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());

    let client = reqwest::Client::new();

    if is_stream {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
            base_url, gemini_model, api_key
        );
        let claude_model = claude_model.to_string();
        let message_id = message_id.to_string();

        let resp = match client.post(&url).headers(headers).json(&gemini_body).timeout(std::time::Duration::from_secs(600)).send().await {
            Ok(r) => r,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
            }
        };

        if resp.status().is_client_error() || resp.status().is_server_error() {
            let err_body = resp.text().await.unwrap_or_default();
            let event = format!("data: {}\n\n", err_body);
            return Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(event))
                .unwrap();
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(100);

        tokio::spawn(async move {
            for event in gemini_conv::stream_gemini_start(&claude_model, &message_id) {
                if tx.send(Ok(event)).await.is_err() { return; }
            }

            let mut state = gemini_conv::GeminiStreamState::default();
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].to_string();
                            buffer = buffer[pos + 1..].to_string();
                            for event in gemini_conv::process_gemini_stream_line(&line, &mut state) {
                                if tx.send(Ok(event)).await.is_err() { return; }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if !buffer.trim().is_empty() {
                for event in gemini_conv::process_gemini_stream_line(&buffer, &mut state) {
                    let _ = tx.send(Ok(event)).await;
                }
            }

            for event in gemini_conv::stream_gemini_end(&state) {
                let _ = tx.send(Ok(event)).await;
            }
        });

        let stream = ReceiverStream::new(rx);
        let body = Body::from_stream(stream);
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(body)
            .unwrap();
    }

    // Non-streaming
    let url = format!(
        "{}/v1beta/models/{}:generateContent?key={}",
        base_url, gemini_model, api_key
    );
    let resp = match client.post(&url).headers(headers).json(&gemini_body).timeout(std::time::Duration::from_secs(600)).send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
        }
    };

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if status.is_client_error() || status.is_server_error() {
        let val: Value = serde_json::from_str(&body_text).unwrap_or(json!({"detail": body_text}));
        return (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), JsonResponse(val)).into_response();
    }
    let gemini_resp: Value = serde_json::from_str(&body_text).unwrap_or(json!({}));
    JsonResponse(gemini_conv::convert_gemini_response(&gemini_resp, claude_model, message_id)).into_response()
}

// ---------------------------------------------------------------------------
// Config API handlers
// ---------------------------------------------------------------------------

async fn get_config_handler() -> JsonResponse<Value> {
    JsonResponse(serde_json::to_value(load_config()).unwrap())
}

async fn put_config_handler(Json(body): Json<Value>) -> JsonResponse<Value> {
    let config: Config = serde_json::from_value(body).unwrap_or_default();
    save_config(&config);
    JsonResponse(json!({"success": true}))
}

// --- Providers ---

async fn list_providers() -> JsonResponse<Value> {
    JsonResponse(serde_json::to_value(&load_config().providers).unwrap())
}

async fn add_provider(Json(mut body): Json<Value>) -> JsonResponse<Value> {
    let mut config = load_config();
    body["id"] = json!(new_provider_id());
    if body.get("enabled").is_none() {
        body["enabled"] = json!(true);
    }
    let provider: Provider = serde_json::from_value(body.clone()).unwrap_or_else(|_| {
        Provider {
            id: new_provider_id(),
            provider_type: "openai".to_string(),
            name: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            models: Vec::new(),
            enabled: true,
        }
    });
    if config.default_provider_id.is_empty() {
        config.default_provider_id = provider.id.clone();
    }
    config.providers.push(provider);
    save_config(&config);
    JsonResponse(body)
}

async fn update_provider(Path(provider_id): Path<String>, Json(mut body): Json<Value>) -> Response {
    let mut config = load_config();
    body["id"] = json!(&provider_id);
    for (i, p) in config.providers.iter().enumerate() {
        if p.id == provider_id {
            let updated: Provider = match serde_json::from_value(body.clone()) {
                Ok(p) => p,
                Err(e) => return (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": e.to_string()}))).into_response(),
            };
            config.providers[i] = updated;
            save_config(&config);
            return JsonResponse(body).into_response();
        }
    }
    (StatusCode::NOT_FOUND, JsonResponse(json!({"detail": "Provider not found"}))).into_response()
}

async fn delete_provider(Path(provider_id): Path<String>) -> JsonResponse<Value> {
    let mut config = load_config();
    config.providers.retain(|p| p.id != provider_id);
    config.model_mappings.retain(|m| m.provider_id != provider_id);
    if config.default_provider_id == provider_id {
        config.default_provider_id = config.providers.first().map(|p| p.id.clone()).unwrap_or_default();
    }
    save_config(&config);
    JsonResponse(json!({"success": true}))
}

// --- Model Mappings ---

async fn list_mappings() -> JsonResponse<Value> {
    JsonResponse(serde_json::to_value(&load_config().model_mappings).unwrap())
}

async fn add_mapping(Json(body): Json<Value>) -> JsonResponse<Value> {
    let mut config = load_config();
    let mapping: config::ModelMapping = match serde_json::from_value(body.clone()) {
        Ok(m) => m,
        Err(_) => return JsonResponse(json!({"detail": "Invalid mapping"})),
    };
    config.model_mappings.push(mapping);
    save_config(&config);
    JsonResponse(body)
}

async fn update_mapping(Path(idx): Path<usize>, Json(body): Json<Value>) -> Response {
    let mut config = load_config();
    if idx >= config.model_mappings.len() {
        return (StatusCode::NOT_FOUND, JsonResponse(json!({"detail": "Mapping not found"}))).into_response();
    }
    let mapping: config::ModelMapping = match serde_json::from_value(body.clone()) {
        Ok(m) => m,
        Err(e) => return (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": e.to_string()}))).into_response(),
    };
    config.model_mappings[idx] = mapping;
    save_config(&config);
    JsonResponse(body).into_response()
}

async fn delete_mapping(Path(idx): Path<usize>) -> Response {
    let mut config = load_config();
    if idx >= config.model_mappings.len() {
        return (StatusCode::NOT_FOUND, JsonResponse(json!({"detail": "Mapping not found"}))).into_response();
    }
    config.model_mappings.remove(idx);
    save_config(&config);
    JsonResponse(json!({"success": true})).into_response()
}

// ---------------------------------------------------------------------------
// Claude Code Management API
// ---------------------------------------------------------------------------

fn home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
}

fn get_claude_settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

fn update_claude_settings(env_updates: &Value) -> Result<(), String> {
    let path = get_claude_settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut settings: Value = if path.exists() {
        let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&data).unwrap_or(json!({}))
    } else {
        json!({})
    };

    if settings.get("env").is_none() {
        settings["env"] = json!({});
    }
    if let (Some(env_obj), Some(updates)) = (settings["env"].as_object_mut(), env_updates.as_object()) {
        for (k, v) in updates {
            env_obj.insert(k.clone(), v.clone());
        }
    }

    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}

fn is_cmd_available(name: &str) -> bool {
    std::process::Command::new(name).arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok()
}

async fn claude_code_status() -> JsonResponse<Value> {
    let installed = is_cmd_available("claude") || is_cmd_available("claude.cmd");
    JsonResponse(json!({"installed": installed}))
}

async fn install_claude_code() -> Response {
    let npm_cmd = if is_cmd_available("npm") {
        "npm"
    } else if is_cmd_available("npm.cmd") {
        "npm.cmd"
    } else {
        return (StatusCode::INTERNAL_SERVER_ERROR, JsonResponse(json!({"detail": "未找到 npm，请先安装 Node.js"}))).into_response();
    };

    let output = match tokio::process::Command::new(npm_cmd)
        .args(["install", "-g", "@anthropic-ai/claude-code"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, JsonResponse(json!({"detail": e.to_string()}))).into_response(),
    };

    if output.status.success() {
        JsonResponse(json!({"success": true, "output": String::from_utf8_lossy(&output.stdout).to_string()})).into_response()
    } else {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        (StatusCode::INTERNAL_SERVER_ERROR, JsonResponse(json!({"detail": err}))).into_response()
    }
}

async fn configure_claude_proxy() -> Response {
    let config = load_config();
    let host = if config.server.host == "0.0.0.0" { "localhost" } else { &config.server.host };
    let port = config.server.port;
    let api_key = &config.server.api_key;
    let base_url = format!("http://{}:{}", host, port);

    match update_claude_settings(&json!({
        "ANTHROPIC_AUTH_TOKEN": api_key,
        "ANTHROPIC_BASE_URL": base_url,
        "API_TIMEOUT_MS": "300000",
    })) {
        Ok(_) => JsonResponse(json!({"success": true})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, JsonResponse(json!({"detail": e}))).into_response(),
    }
}

async fn configure_claude_external(Json(body): Json<Value>) -> Response {
    let base_url = body.get("base_url").and_then(|u| u.as_str()).unwrap_or("https://api.anthropic.com").trim_end_matches('/').to_string();
    let api_key = body.get("api_key").and_then(|k| k.as_str()).unwrap_or("");

    if api_key.is_empty() {
        return (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": "API Key 不能为空"}))).into_response();
    }

    match update_claude_settings(&json!({
        "ANTHROPIC_AUTH_TOKEN": api_key,
        "ANTHROPIC_BASE_URL": base_url,
        "API_TIMEOUT_MS": "300000",
    })) {
        Ok(_) => JsonResponse(json!({"success": true})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, JsonResponse(json!({"detail": e}))).into_response(),
    }
}

async fn runtime_info() -> JsonResponse<Value> {
    JsonResponse(json!({"docker": std::env::var("CONFIG_PATH").is_ok()}))
}

async fn fetch_models_handler(Json(body): Json<Value>) -> Response {
    let base_url = body.get("base_url").and_then(|u| u.as_str()).unwrap_or("").trim_end_matches('/');
    let api_key = body.get("api_key").and_then(|k| k.as_str()).unwrap_or("");

    if base_url.is_empty() || api_key.is_empty() {
        return (StatusCode::BAD_REQUEST, JsonResponse(json!({"detail": "base_url and api_key are required"}))).into_response();
    }

    let url = if base_url.contains("generativelanguage.googleapis.com") {
        format!("{}/v1beta/models?key={}", base_url, api_key)
    } else {
        let base = if base_url.ends_with("/v1") { base_url.to_string() } else { format!("{}/v1", base_url) };
        format!("{}/models", base)
    };

    let client = reqwest::Client::new();
    let mut req = client.get(&url).timeout(std::time::Duration::from_secs(30));
    if !base_url.contains("generativelanguage.googleapis.com") {
        req = req.header("authorization", format!("Bearer {}", api_key));
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, JsonResponse(json!({"detail": e.to_string()}))).into_response();
        }
    };

    if !resp.status().is_success() {
        let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let text = resp.text().await.unwrap_or_default();
        return (status, JsonResponse(json!({"detail": text}))).into_response();
    }

    let data: Value = resp.json().await.unwrap_or(json!({}));

    let models: Vec<String> = if let Some(arr) = data.get("models").and_then(|m| m.as_array()) {
        // Gemini format: { models: [{ name: "models/gemini-pro" }] }
        arr.iter()
            .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
            .map(|n| n.strip_prefix("models/").unwrap_or(n).to_string())
            .collect()
    } else if let Some(arr) = data.get("data").and_then(|d| d.as_array()) {
        // OpenAI format: { data: [{ id: "gpt-4" }] }
        arr.iter()
            .filter_map(|m| m.get("id").and_then(|id| id.as_str()))
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };

    JsonResponse(json!({"models": models})).into_response()
}

async fn get_server() -> JsonResponse<Value> {
    JsonResponse(serde_json::to_value(&load_config().server).unwrap())
}

async fn update_server(Json(body): Json<Value>) -> JsonResponse<Value> {
    let mut config = load_config();
    if let Ok(server) = serde_json::from_value(body.clone()) {
        config.server = server;
        save_config(&config);
    }
    JsonResponse(body)
}

// ---------------------------------------------------------------------------
// Static files & UI redirect
// ---------------------------------------------------------------------------

fn static_dir() -> Option<PathBuf> {
    // Check next to executable first
    if let Ok(exe) = std::env::current_exe() {
        let external = exe.parent().unwrap_or(std::path::Path::new(".")).join("static");
        if external.exists() {
            return Some(external);
        }
    }
    // Development mode
    let dev = std::path::Path::new("static");
    if dev.exists() {
        return Some(dev.to_path_buf());
    }
    None
}

async fn ui_redirect() -> Redirect {
    Redirect::permanent("/ui/index.html")
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let config = load_config();
    let host = config.server.host.clone();
    let port = config.server.port;

    // Build authed router for /v1/messages
    let messages_route = Router::new()
        .route("/v1/messages", post(messages_handler))
        .layer(middleware::from_fn(auth::verify_api_key));

    // Config API (no auth for local use)
    let config_api = Router::new()
        .route("/api/config", get(get_config_handler).put(put_config_handler))
        .route("/api/providers", get(list_providers).post(add_provider))
        .route("/api/providers/{provider_id}", put(update_provider).delete(delete_provider))
        .route("/api/model-mappings", get(list_mappings).post(add_mapping))
        .route("/api/model-mappings/{idx}", put(update_mapping).delete(delete_mapping))
        .route("/api/claude-code/status", get(claude_code_status))
        .route("/api/claude-code/install", post(install_claude_code))
        .route("/api/claude-code/configure-proxy", post(configure_claude_proxy))
        .route("/api/claude-code/configure-external", post(configure_claude_external))
        .route("/api/runtime-info", get(runtime_info))
        .route("/api/server", get(get_server).put(update_server))
        .route("/api/fetch-models", post(fetch_models_handler));

    let mut app = Router::new()
        .merge(messages_route)
        .merge(config_api)
        .route("/", get(|| async { Redirect::permanent("/ui/") }));

    // Serve static files if directory exists
    if let Some(ui_dir) = static_dir() {
        app = app.nest_service(
            "/ui",
            tower_http::services::ServeDir::new(&ui_dir)
                .fallback(tower_http::services::ServeFile::new(ui_dir.join("index.html"))),
        );
    } else {
        app = app.route("/ui/", get(ui_redirect));
    }

    app = app
        .layer(middleware::from_fn(log_request))
        .layer(CorsLayer::very_permissive());

    let addr = format!("{}:{}", host, port);
    println!("Claude API Proxy starting on http://{}", addr);
    let display_host = if host == "0.0.0.0" { "localhost" } else { &host };
    println!("Open the config UI at: http://{}:{}/ui/", display_host, port);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
