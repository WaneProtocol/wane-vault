#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "========================================="
echo "  iVZA Build Script"
echo "========================================="

# Build Rust workspace
echo ""
echo "[1/3] Building Rust workspace..."
cd "$PROJECT_DIR"

if ! command -v cargo &> /dev/null; then
    echo "ERROR: cargo not found. Install Rust via https://rustup.rs"
    exit 1
fi

cargo build --release
echo "  Rust build complete."

# Build Anchor program (if Anchor is available)
echo ""
echo "[2/3] Building Anchor program..."
if command -v anchor &> /dev/null; then
    if [ -d "$PROJECT_DIR/programs" ]; then
        cd "$PROJECT_DIR"
        anchor build
        echo "  Anchor build complete."
    else
        echo "  No programs/ directory found. Skipping."
    fi
else
    echo "  Anchor CLI not found. Skipping on-chain program build."
fi

# Build TypeScript SDK
echo ""
echo "[3/3] Building TypeScript SDK..."
if [ -d "$PROJECT_DIR/sdk" ]; then
    cd "$PROJECT_DIR/sdk"

    if ! command -v npm &> /dev/null; then
        echo "ERROR: npm not found. Install Node.js 18+."
        exit 1
    fi

    npm install
    npm run build
    echo "  TypeScript SDK build complete."
else
    echo "  No sdk/ directory found. Skipping."
fi

echo ""
echo "========================================="
echo "  Build complete."
echo "========================================="
