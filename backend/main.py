"""
Claude API Proxy - Main FastAPI Application
Exposes an Anthropic-compatible /v1/messages endpoint and a config REST API.
"""
from __future__ import annotations

import asyncio
import json
import os
import shutil
import sys
import uuid
from pathlib import Path
from typing import Any, AsyncGenerator

import httpx
from fastapi import Depends, FastAPI, HTTPException, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import RedirectResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles

from auth import verify_api_key

# ---------------------------------------------------------------------------
# Shared httpx client – reuse connections across concurrent requests
# ---------------------------------------------------------------------------
_http_client: httpx.AsyncClient | None = None

_MAX_RETRIES = 3
_RETRY_DELAY = 0.5  # seconds, doubled each retry


def _get_http_client() -> httpx.AsyncClient:
    global _http_client
    if _http_client is None or _http_client.is_closed:
        _http_client = httpx.AsyncClient(
            timeout=httpx.Timeout(600, connect=30),
            limits=httpx.Limits(
                max_connections=200,
                max_keepalive_connections=40,
                keepalive_expiry=120,
            ),
            http2=True,
        )
    return _http_client
from config_manager import load_config, new_provider_id, save_config
from converters.gemini_conv import (
    build_gemini_request,
    convert_gemini_response,
    stream_gemini_to_anthropic,
)
from converters.openai_conv import (
    build_openai_request,
    convert_openai_response,
    stream_openai_to_anthropic,
)

app = FastAPI(title="Claude API Proxy", version="1.0.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.on_event("shutdown")
async def _shutdown_http_client():
    global _http_client
    if _http_client is not None:
        await _http_client.aclose()
        _http_client = None


async def _post_with_retry(
    url: str, *, json: dict, headers: dict
) -> httpx.Response:
    """POST with automatic retry on transient connection errors."""
    client = _get_http_client()
    last_exc: Exception | None = None
    for attempt in range(_MAX_RETRIES):
        try:
            return await client.post(url, json=json, headers=headers)
        except (httpx.RemoteProtocolError, httpx.ConnectError, httpx.ReadError) as exc:
            last_exc = exc
            if attempt < _MAX_RETRIES - 1:
                await asyncio.sleep(_RETRY_DELAY * (2 ** attempt))
    raise last_exc  # type: ignore[misc]


def _static_dir() -> Path | None:
    """Locate the Vue frontend dist directory.

    Priority:
    1. <exe_dir>/static  – external folder next to the exe (allows UI update without repackaging)
    2. sys._MEIPASS/static – embedded in the PyInstaller bundle
    3. <backend_dir>/static – development mode
    """
    if getattr(sys, "frozen", False):
        # Check for an external override first
        external = Path(sys.executable).parent / "static"
        if external.exists():
            return external
        # Fall back to embedded bundle
        candidate = Path(sys._MEIPASS) / "static"  # type: ignore[attr-defined]
    else:
        candidate = Path(__file__).parent / "static"
    return candidate if candidate.exists() else None


_ui_dir = _static_dir()

# ---------------------------------------------------------------------------
# Helper: resolve provider + target model from incoming Claude model name
# ---------------------------------------------------------------------------

def _resolve_provider(claude_model: str, config: dict) -> tuple[dict, str]:
    """Return (provider_dict, target_model_name)."""
    for mapping in config.get("model_mappings", []):
        if mapping.get("claude_model") == claude_model:
            provider_id = mapping["provider_id"]
            target_model = mapping["target_model"]
            for p in config.get("providers", []):
                if p["id"] == provider_id and p.get("enabled", True):
                    return p, target_model
            break

    # Fall back to first enabled provider's first model
    for p in config.get("providers", []):
        if p.get("enabled", True):
            models = p.get("models", [])
            target_model = models[0] if models else claude_model
            return p, target_model

    raise HTTPException(
        status_code=400,
        detail={
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": f"No provider configured for model '{claude_model}'. "
                           "Please configure providers and model mappings in the UI.",
            },
        },
    )


def _make_headers(provider: dict) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {provider['api_key']}",
        "Content-Type": "application/json",
    }


async def _aiter_lines(response: httpx.Response) -> AsyncGenerator[str, None]:
    async for line in response.aiter_lines():
        yield line


# ---------------------------------------------------------------------------
# Anthropic direct forwarding – pass-through headers for AWS Bedrock compat
# ---------------------------------------------------------------------------

# Headers from the incoming request that should be forwarded to the upstream
# Anthropic-compatible API (including AWS Bedrock / proxy variants).
_ANTHROPIC_FORWARD_HEADERS = {
    "anthropic-version",
    "anthropic-beta",
    "x-api-key",
    "content-type",
}


def _build_anthropic_direct_headers(
    request: Request, direct_cfg: dict
) -> dict[str, str]:
    """Build headers for the upstream Anthropic API call.

    Forwards recognised Anthropic headers from the incoming request and
    overwrites the auth key with the configured direct API key.
    """
    headers: dict[str, str] = {}
    for name in _ANTHROPIC_FORWARD_HEADERS:
        value = request.headers.get(name)
        if value:
            headers[name] = value

    # Always set auth from config – never leak proxy key to upstream
    api_key = direct_cfg.get("api_key", "")
    if api_key:
        headers["x-api-key"] = api_key
        headers.pop("Authorization", None)  # prefer x-api-key for Anthropic

    # Ensure content-type
    headers.setdefault("content-type", "application/json")
    return headers


async def _forward_anthropic_direct(
    request: Request, body: dict, direct_cfg: dict
) -> StreamingResponse | dict:
    """Forward the request as-is to the upstream Anthropic API."""
    base_url: str = direct_cfg.get("base_url", "https://yansd666.com").rstrip("/")
    url = f"{base_url}/v1/messages"
    headers = _build_anthropic_direct_headers(request, direct_cfg)
    is_stream = body.get("stream", False)

    if is_stream:
        async def generate() -> AsyncGenerator[bytes, None]:
            client = _get_http_client()
            async with client.stream(
                "POST", url, json=body, headers=headers
            ) as resp:
                if resp.status_code >= 400:
                    err_body = await resp.aread()
                    yield err_body
                    return
                async for chunk in resp.aiter_bytes():
                    yield chunk

        return StreamingResponse(generate(), media_type="text/event-stream")

    resp = await _post_with_retry(url, json=body, headers=headers)
    if resp.status_code >= 400:
        raise HTTPException(status_code=resp.status_code, detail=resp.json())
    return resp.json()


# ---------------------------------------------------------------------------
# /v1/messages  – main Claude-compatible endpoint
# ---------------------------------------------------------------------------

@app.post("/v1/messages", dependencies=[Depends(verify_api_key)])
async def messages(request: Request):
    config = load_config()
    body: dict = await request.json()

    # ---- Anthropic direct forwarding (bypass provider conversion) ----
    direct_cfg = config.get("anthropic_direct", {})
    if direct_cfg.get("enabled"):
        return await _forward_anthropic_direct(request, body, direct_cfg)

    claude_model: str = body.get("model", "")
    message_id = f"msg_{uuid.uuid4().hex[:24]}"
    is_stream = body.get("stream", False)

    provider, target_model = _resolve_provider(claude_model, config)
    provider_type: str = provider.get("type", "openai")

    # ------------------------------------------------------------------ OpenAI
    if provider_type == "openai":
        base_url: str = provider.get("base_url", "https://yansd666.com").rstrip("/")
        if not base_url.endswith("/v1"):
            base_url = f"{base_url}/v1"
        url = f"{base_url}/chat/completions"
        openai_req = build_openai_request(body, target_model)
        headers = _make_headers(provider)

        if is_stream:
            async def generate() -> AsyncGenerator[bytes, None]:
                client = _get_http_client()
                async with client.stream("POST", url, json=openai_req, headers=headers) as resp:
                    if resp.status_code >= 400:
                        err_body = await resp.aread()
                        yield f"data: {err_body.decode()}\n\n".encode()
                        return
                    async for chunk in stream_openai_to_anthropic(
                        _aiter_lines(resp), claude_model, message_id
                    ):
                        yield chunk.encode()

            return StreamingResponse(generate(), media_type="text/event-stream")

        resp = await _post_with_retry(url, json=openai_req, headers=headers)
        if resp.status_code >= 400:
            raise HTTPException(status_code=resp.status_code, detail=resp.json())
        return convert_openai_response(resp.json(), claude_model, message_id)

    # ------------------------------------------------------------------ Gemini
    if provider_type == "gemini":
        base_url = provider.get("base_url", "https://generativelanguage.googleapis.com").rstrip("/")
        api_key = provider.get("api_key", "")
        gemini_model, gemini_body = build_gemini_request(body, target_model)
        headers = {"Content-Type": "application/json"}

        if is_stream:
            url = (
                f"{base_url}/v1beta/models/{gemini_model}"
                f":streamGenerateContent?key={api_key}&alt=sse"
            )

            async def generate_gemini() -> AsyncGenerator[bytes, None]:
                client = _get_http_client()
                async with client.stream("POST", url, json=gemini_body, headers=headers) as resp:
                    if resp.status_code >= 400:
                        err_body = await resp.aread()
                        yield f"data: {err_body.decode()}\n\n".encode()
                        return
                    async for chunk in stream_gemini_to_anthropic(
                        _aiter_lines(resp), claude_model, message_id
                    ):
                        yield chunk.encode()

            return StreamingResponse(generate_gemini(), media_type="text/event-stream")

        url = (
            f"{base_url}/v1beta/models/{gemini_model}"
            f":generateContent?key={api_key}"
        )
        resp = await _post_with_retry(url, json=gemini_body, headers=headers)
        if resp.status_code >= 400:
            raise HTTPException(status_code=resp.status_code, detail=resp.json())
        return convert_gemini_response(resp.json(), claude_model, message_id)

    raise HTTPException(status_code=400, detail=f"Unknown provider type: {provider_type}")


# ---------------------------------------------------------------------------
# Config API (used by Vue frontend, no auth required for local use)
# ---------------------------------------------------------------------------

@app.get("/api/config")
def get_config():
    config = load_config()
    # Strip sensitive data from provider list for display
    return config


@app.put("/api/config")
def put_config(request_body: dict):
    save_config(request_body)
    return {"success": True}


# --- Providers ---

@app.get("/api/providers")
def list_providers():
    return load_config().get("providers", [])


@app.post("/api/providers")
def add_provider(body: dict):
    config = load_config()
    body["id"] = new_provider_id()
    body.setdefault("enabled", True)
    config.setdefault("providers", []).append(body)
    if not config.get("default_provider_id"):
        config["default_provider_id"] = body["id"]
    save_config(config)
    return body


@app.put("/api/providers/{provider_id}")
def update_provider(provider_id: str, body: dict):
    config = load_config()
    providers = config.get("providers", [])
    for i, p in enumerate(providers):
        if p["id"] == provider_id:
            body["id"] = provider_id
            providers[i] = body
            save_config(config)
            return body
    raise HTTPException(status_code=404, detail="Provider not found")


@app.delete("/api/providers/{provider_id}")
def delete_provider(provider_id: str):
    config = load_config()
    config["providers"] = [p for p in config.get("providers", []) if p["id"] != provider_id]
    # Clean up orphaned mappings
    config["model_mappings"] = [
        m for m in config.get("model_mappings", []) if m.get("provider_id") != provider_id
    ]
    if config.get("default_provider_id") == provider_id:
        remaining = config["providers"]
        config["default_provider_id"] = remaining[0]["id"] if remaining else ""
    save_config(config)
    return {"success": True}


# --- Model Mappings ---

@app.get("/api/model-mappings")
def list_mappings():
    return load_config().get("model_mappings", [])


@app.post("/api/model-mappings")
def add_mapping(body: dict):
    config = load_config()
    config.setdefault("model_mappings", []).append(body)
    save_config(config)
    return body


@app.put("/api/model-mappings/{idx}")
def update_mapping(idx: int, body: dict):
    config = load_config()
    mappings = config.get("model_mappings", [])
    if idx < 0 or idx >= len(mappings):
        raise HTTPException(status_code=404, detail="Mapping not found")
    mappings[idx] = body
    save_config(config)
    return body


@app.delete("/api/model-mappings/{idx}")
def delete_mapping(idx: int):
    config = load_config()
    mappings = config.get("model_mappings", [])
    if idx < 0 or idx >= len(mappings):
        raise HTTPException(status_code=404, detail="Mapping not found")
    mappings.pop(idx)
    save_config(config)
    return {"success": True}


# ---------------------------------------------------------------------------
# Claude Code Management API
# ---------------------------------------------------------------------------

def _get_claude_settings_path() -> Path:
    return Path.home() / ".claude" / "settings.json"


def _update_claude_settings(env_updates: dict) -> None:
    settings_path = _get_claude_settings_path()
    settings_path.parent.mkdir(parents=True, exist_ok=True)
    if settings_path.exists():
        with settings_path.open(encoding="utf-8") as f:
            settings = json.load(f)
    else:
        settings = {}
    settings.setdefault("env", {})
    settings["env"].update(env_updates)
    with settings_path.open("w", encoding="utf-8") as f:
        json.dump(settings, f, ensure_ascii=False, indent=2)


def _is_claude_installed() -> bool:
    for name in ["claude", "claude.cmd"]:
        if shutil.which(name):
            return True
    return False


def _find_npm() -> str | None:
    for name in ["npm", "npm.cmd"]:
        result = shutil.which(name)
        if result:
            return result
    return None


@app.get("/api/claude-code/status")
def claude_code_status():
    return {"installed": _is_claude_installed()}


@app.post("/api/claude-code/install")
async def install_claude_code():
    npm_cmd = _find_npm()
    if not npm_cmd:
        raise HTTPException(status_code=500, detail="未找到 npm，请先安装 Node.js")
    try:
        proc = await asyncio.create_subprocess_exec(
            npm_cmd, "install", "-g", "@anthropic-ai/claude-code",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=300)
        if proc.returncode == 0:
            return {"success": True, "output": stdout.decode(errors="replace")}
        raise HTTPException(
            status_code=500,
            detail=stderr.decode(errors="replace") or stdout.decode(errors="replace"),
        )
    except asyncio.TimeoutError:
        raise HTTPException(
            status_code=500,
            detail="安装超时，请手动运行：npm install -g @anthropic-ai/claude-code",
        )
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.post("/api/claude-code/configure-proxy")
def configure_claude_proxy():
    config = load_config()
    server = config.get("server", {})
    host = server.get("host", "0.0.0.0")
    if host == "0.0.0.0":
        host = "localhost"
    port = server.get("port", 8000)
    api_key = server.get("api_key", "")
    base_url = f"http://{host}:{port}"
    _update_claude_settings({
        "ANTHROPIC_AUTH_TOKEN": api_key,
        "ANTHROPIC_BASE_URL": base_url,
        "API_TIMEOUT_MS": "300000",
    })
    return {"success": True}


@app.post("/api/claude-code/configure-external")
def configure_claude_external(body: dict):
    base_url = body.get("base_url", "https://yansd666.com").rstrip("/")
    api_key = body.get("api_key", "")
    if not api_key:
        raise HTTPException(status_code=400, detail="API Key 不能为空")
    _update_claude_settings({
        "ANTHROPIC_AUTH_TOKEN": api_key,
        "ANTHROPIC_BASE_URL": base_url,
        "API_TIMEOUT_MS": "300000",
    })
    return {"success": True}


# --- Runtime info ---

@app.get("/api/runtime-info")
def runtime_info():
    return {"docker": bool(os.environ.get("CONFIG_PATH"))}


# --- Server settings ---

@app.get("/api/server")
def get_server():
    return load_config().get("server", {})


@app.put("/api/server")
def update_server(body: dict):
    config = load_config()
    config["server"] = body
    save_config(config)
    return body


# --- Anthropic Direct settings ---

@app.get("/api/anthropic-direct")
def get_anthropic_direct():
    return load_config().get("anthropic_direct", {
        "enabled": False,
        "base_url": "https://yansd666.com",
        "api_key": "",
    })


@app.put("/api/anthropic-direct")
def update_anthropic_direct(body: dict):
    config = load_config()
    config["anthropic_direct"] = body
    save_config(config)
    return body


# --- Fetch remote models for a provider ---

@app.post("/api/fetch-models")
async def fetch_models(body: dict):
    """Proxy request to fetch available models from a provider's /v1/models endpoint."""
    base_url: str = body.get("base_url", "").rstrip("/")
    api_key: str = body.get("api_key", "")
    if not base_url or not api_key:
        raise HTTPException(status_code=400, detail="base_url and api_key are required")
    if not base_url.endswith("/v1"):
        url = f"{base_url}/v1/models"
    else:
        url = f"{base_url}/models"
    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }
    try:
        async with httpx.AsyncClient(timeout=30) as client:
            resp = await client.get(url, headers=headers)
        if resp.status_code >= 400:
            raise HTTPException(status_code=resp.status_code, detail=resp.text)
        data = resp.json()
        # Extract model ids from OpenAI-compatible response
        models = [m["id"] for m in data.get("data", []) if "id" in m]
        return {"models": models}
    except httpx.TimeoutException:
        raise HTTPException(status_code=504, detail="请求超时，请检查 Base URL 是否正确")
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


# ---------------------------------------------------------------------------
# Serve Vue frontend (registered after all API routes so API always wins)
# ---------------------------------------------------------------------------

@app.get("/", include_in_schema=False)
def root_redirect():
    return RedirectResponse(url="/ui/")


if _ui_dir:
    app.mount("/ui", StaticFiles(directory=str(_ui_dir), html=True), name="ui")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import uvicorn
    from config_manager import load_config as _lc

    cfg = _lc()
    s = cfg.get("server", {})
    uvicorn.run(app, host=s.get("host", "0.0.0.0"), port=s.get("port", 8000))
