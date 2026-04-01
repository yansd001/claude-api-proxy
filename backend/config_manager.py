"""Configuration management for claude-api-proxy."""
from __future__ import annotations

import json
import secrets
import sys
import uuid
from pathlib import Path
from typing import Any


def _get_config_path() -> Path:
    """Return a writable config.json path regardless of frozen/dev mode."""
    if getattr(sys, "frozen", False):
        # Running as PyInstaller bundle: store config next to the .exe
        return Path(sys.executable).parent / "config.json"
    return Path(__file__).parent / "config.json"


CONFIG_PATH = _get_config_path()

DEFAULT_CONFIG: dict[str, Any] = {
    "server": {
        "port": 8000,
        "host": "0.0.0.0",
        "api_key": secrets.token_urlsafe(32),
    },
    "providers": [],
    "model_mappings": [],
    "default_provider_id": "",
    "default_model": "",
}


def load_config() -> dict[str, Any]:
    if not CONFIG_PATH.exists():
        save_config(DEFAULT_CONFIG)
        return DEFAULT_CONFIG.copy()
    with CONFIG_PATH.open(encoding="utf-8") as f:
        return json.load(f)


def save_config(config: dict[str, Any]) -> None:
    with CONFIG_PATH.open("w", encoding="utf-8") as f:
        json.dump(config, f, ensure_ascii=False, indent=2)


def new_provider_id() -> str:
    return uuid.uuid4().hex
