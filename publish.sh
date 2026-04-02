#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
FRONTEND="$ROOT/frontend"
BACKEND="$ROOT/backend"
PUBLISH="$ROOT/publish-linux"

echo "============================================================"
echo "  Claude API Proxy - Linux Build and Publish"
echo "============================================================"
echo

# ----------------------------------------------------------------
# Step 0: Check dependencies
# ----------------------------------------------------------------
echo "[0/4] Checking dependencies..."

if ! command -v npm &>/dev/null; then
    echo "[Error] npm not found. Please install Node.js."
    exit 1
fi

PYTHON_CMD=""
if command -v python3 &>/dev/null; then
    PYTHON_CMD=python3
elif command -v python &>/dev/null; then
    PYTHON_CMD=python
else
    echo "[Error] Python not found. Please install Python 3.11+"
    exit 1
fi

echo "      npm:    OK"
echo "      python: OK ($PYTHON_CMD)"
echo

# ----------------------------------------------------------------
# Step 1: Build Vue frontend
# ----------------------------------------------------------------
echo "[1/4] Building Vue frontend..."
cd "$FRONTEND"

npm install --frozen-lockfile
npm run build
echo

# ----------------------------------------------------------------
# Step 2: Copy frontend dist to backend/static
# ----------------------------------------------------------------
echo "[2/4] Copying frontend dist to backend/static..."
cd "$ROOT"

rm -rf "$BACKEND/static"
cp -r "$FRONTEND/dist" "$BACKEND/static"
echo "      frontend/dist -> backend/static"
echo

# ----------------------------------------------------------------
# Step 3: Install Python deps and package with PyInstaller
# ----------------------------------------------------------------
echo "[3/4] Installing Python dependencies and packaging binary..."
cd "$BACKEND"

$PYTHON_CMD -m pip install -r requirements.txt -r build-requirements.txt -q

# Clean publish dir before packaging
rm -rf "$PUBLISH"
mkdir -p "$PUBLISH"

$PYTHON_CMD -m PyInstaller \
    --onefile \
    --name claude-api-proxy \
    --add-data "static:static" \
    --collect-all uvicorn \
    --collect-all fastapi \
    --collect-all starlette \
    --collect-all httpx \
    --hidden-import anyio._backends._asyncio \
    --hidden-import anyio._backends._trio \
    --distpath "$PUBLISH" \
    --clean \
    main.py

echo

# ----------------------------------------------------------------
# Step 4: Copy frontend static files to publish-linux/static
# ----------------------------------------------------------------
echo "[4/4] Copying frontend static files to publish-linux/static..."
cd "$ROOT"

cp -r "$BACKEND/static" "$PUBLISH/static"

echo
echo "============================================================"
echo "  Build complete!"
echo "  Output:  $PUBLISH"
echo "  Binary:  $PUBLISH/claude-api-proxy"
echo "  UI:      $PUBLISH/static/  (can be updated independently)"
echo "  config.json will be created next to the binary on first launch."
echo "============================================================"
echo
