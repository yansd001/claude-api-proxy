"""
Entry point for PyInstaller bundle.
Uvicorn must receive the *app object* directly (not a string like "main:app")
because the frozen environment cannot import modules by string name.
"""
from __future__ import annotations

import sys

import uvicorn

# Import the app object at module level so it exists in the frozen package graph
from main import app
from config_manager import load_config


def main() -> None:
    cfg = load_config()
    server_cfg = cfg.get("server", {})
    host = server_cfg.get("host", "0.0.0.0")
    port = server_cfg.get("port", 8000)
    print(f"Claude API Proxy starting on http://{host}:{port}", flush=True)
    print(f"Open the config UI at: http://{'localhost' if host == '0.0.0.0' else host}:{port}/ui/", flush=True)
    uvicorn.run(app, host=host, port=port)


if __name__ == "__main__":
    main()
