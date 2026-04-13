#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
FRONTEND="$ROOT/frontend"
BACKEND_RUST="$ROOT/backend-rust"
PUBLISH="$ROOT/publish-linux"

echo "============================================================"
echo "  Claude API Proxy (Rust) - Linux Build and Publish"
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

if ! command -v cargo &>/dev/null; then
    echo "[Error] cargo not found. Please install Rust (https://rustup.rs)."
    exit 1
fi

echo "      npm:    OK"
echo "      cargo:  OK"
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
# Step 2: Prepare output directory
# ----------------------------------------------------------------
echo "[2/4] Preparing output directory..."
cd "$ROOT"

rm -rf "$PUBLISH"
mkdir -p "$PUBLISH"
cp -r "$FRONTEND/dist" "$PUBLISH/static"
echo "      frontend/dist -> publish-linux/static"
echo

# ----------------------------------------------------------------
# Step 3: Build Rust backend
# ----------------------------------------------------------------
echo "[3/4] Building Rust backend (release)..."
cd "$BACKEND_RUST"

cargo build --release
echo

# ----------------------------------------------------------------
# Step 4: Copy binary to publish
# ----------------------------------------------------------------
echo "[4/4] Copying binary to publish-linux..."
cd "$ROOT"

cp "$BACKEND_RUST/target/release/claude-api-proxy" "$PUBLISH/claude-api-proxy"

echo
echo "============================================================"
echo "  Build complete!"
echo "  Output:  $PUBLISH"
echo "  Binary:  $PUBLISH/claude-api-proxy"
echo "  UI:      $PUBLISH/static/  (can be updated independently)"
echo "  config.json will be created next to the binary on first launch."
echo "============================================================"
echo "  config.json will be created next to the binary on first launch."
echo "============================================================"
echo
