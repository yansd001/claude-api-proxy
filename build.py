#!/usr/bin/env python3
"""
Build script: Vue frontend → Python binary via PyInstaller.

Usage:
    python build.py

Output:
    dist/claude-api-proxy.exe  (Windows)
    dist/claude-api-proxy      (Linux / macOS)

The resulting executable is self-contained:
- Serves the Vue configuration UI at http://localhost:8000/ui/
- Provides the Anthropic-compatible /v1/messages proxy endpoint
- Writes config.json next to the executable (writable by default)
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parent
BACKEND = ROOT / "backend"
FRONTEND = ROOT / "frontend"
DIST_VUE = FRONTEND / "dist"
STATIC = BACKEND / "static"      # embedded into the bundle
DIST_EXE = ROOT / "dist"         # where the final exe lands


def run(cmd: list, cwd: Path | None = None) -> None:
    display = " ".join(str(c) for c in cmd)
    print(f"\n>>> {display}")
    subprocess.run(cmd, cwd=cwd, check=True)


def main() -> None:
    npm = "npm.cmd" if sys.platform == "win32" else "npm"

    # ------------------------------------------------------------------ Step 1
    print("\n[1/3] Building Vue frontend …")
    run([npm, "install", "--frozen-lockfile"], cwd=FRONTEND)
    run([npm, "run", "build"], cwd=FRONTEND)

    # ------------------------------------------------------------------ Step 2
    print("\n[2/3] Copying Vue dist → backend/static …")
    if STATIC.exists():
        shutil.rmtree(STATIC)
    shutil.copytree(DIST_VUE, STATIC)
    print(f"      {DIST_VUE}  →  {STATIC}")

    # ------------------------------------------------------------------ Step 3
    print("\n[3/3] Packaging with PyInstaller …")

    # On Windows the path separator in --add-data is ';', on Unix ':'
    sep = os.pathsep
    add_data = f"{STATIC}{sep}static"

    run(
        [
            sys.executable, "-m", "PyInstaller",
            "--onefile",
            "--name", "claude-api-proxy",
            # Embed the Vue static files
            "--add-data", add_data,
            # Collect dynamic-import heavy packages to avoid missing module errors
            "--collect-all", "uvicorn",
            "--collect-all", "fastapi",
            "--collect-all", "starlette",
            "--collect-all", "httpx",
            # Common hidden imports for anyio / asyncio backend
            "--hidden-import", "anyio._backends._asyncio",
            "--hidden-import", "anyio._backends._trio",
            # Output directory
            "--distpath", str(DIST_EXE),
            # Clean previous build artefacts
            "--clean",
            "main.py",
        ],
        cwd=BACKEND,
    )

    suffix = ".exe" if sys.platform == "win32" else ""
    exe = DIST_EXE / f"claude-api-proxy{suffix}"
    print(f"\n✅  Build complete → {exe}")
    print("   Run the exe directly; config.json will be created next to it on first launch.")


if __name__ == "__main__":
    main()
