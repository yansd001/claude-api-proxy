"""Auth middleware for claude-api-proxy."""
from __future__ import annotations

from fastapi import HTTPException, Request

from config_manager import load_config


async def verify_api_key(request: Request) -> None:
    """Validate the incoming API key against the configured proxy key."""
    config = load_config()
    expected_key: str = config.get("server", {}).get("api_key", "")

    # Skip auth if no key is configured
    if not expected_key:
        return

    auth_header = request.headers.get("authorization", "")
    x_api_key = request.headers.get("x-api-key", "")

    provided = ""
    if auth_header.lower().startswith("bearer "):
        provided = auth_header[7:]
    elif x_api_key:
        provided = x_api_key

    if provided != expected_key:
        raise HTTPException(status_code=401, detail="Invalid API key")
