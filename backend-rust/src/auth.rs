use axum::{extract::Request, middleware::Next, response::Response};

use crate::config::load_config;

pub async fn verify_api_key(req: Request, next: Next) -> Result<Response, Response> {
    let config = load_config();
    let expected_key = &config.server.api_key;

    if expected_key.is_empty() {
        return Ok(next.run(req).await);
    }

    let provided = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            if v.to_lowercase().starts_with("bearer ") {
                Some(v[7..].to_string())
            } else {
                None
            }
        })
        .or_else(|| {
            req.headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    if provided != *expected_key {
        let body = serde_json::json!({"detail": "Invalid API key"});
        return Err(axum::response::IntoResponse::into_response((
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(body),
        )));
    }

    Ok(next.run(req).await)
}
