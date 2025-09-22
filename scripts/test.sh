#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

RUST_ONLY=false
TS_ONLY=false
VERBOSE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --rust)
            RUST_ONLY=true
            shift
            ;;
        --ts)
            TS_ONLY=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --help)
            echo "Usage: test.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --rust       Run only Rust tests"
            echo "  --ts         Run only TypeScript tests"
            echo "  --verbose    Enable verbose output"
            echo "  --help       Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo "========================================="
echo "  iVZA Test Suite"
echo "========================================="

FAILED=0

# Rust tests
if [ "$TS_ONLY" = false ]; then
    echo ""
    echo "[1/3] Running Rust formatting check..."
    cd "$PROJECT_DIR"
    if cargo fmt --all -- --check; then
        echo "  Formatting: OK"
    else
        echo "  Formatting: FAILED"
        FAILED=1
    fi

    echo ""
    echo "[2/3] Running Rust clippy..."
    if cargo clippy --all-targets --all-features -- -D warnings; then
        echo "  Clippy: OK"
    else
        echo "  Clippy: FAILED"
        FAILED=1
    fi

    echo ""
    echo "[3/3] Running Rust tests..."
    CARGO_ARGS="--all-features"
    if [ "$VERBOSE" = true ]; then
        CARGO_ARGS="$CARGO_ARGS --verbose"
    fi
    if cargo test $CARGO_ARGS; then
        echo "  Tests: OK"
    else
        echo "  Tests: FAILED"
        FAILED=1
    fi
fi

# TypeScript tests
if [ "$RUST_ONLY" = false ] && [ -d "$PROJECT_DIR/sdk" ]; then
    echo ""
    echo "[TS] Running TypeScript SDK tests..."
    cd "$PROJECT_DIR/sdk"

    if [ ! -d "node_modules" ]; then
        echo "  Installing dependencies..."
        npm install
    fi

    echo "  Building..."
    if npm run build; then
        echo "  Build: OK"
    else
        echo "  Build: FAILED"
        FAILED=1
    fi

    echo "  Linting..."
    if npm run lint 2>/dev/null; then
        echo "  Lint: OK"
    else
        echo "  Lint: FAILED (or no lint script)"
    fi

    echo "  Testing..."
    if npm test; then
        echo "  Tests: OK"
    else
        echo "  Tests: FAILED"
        FAILED=1
    fi
fi

echo ""
echo "========================================="
if [ "$FAILED" -eq 0 ]; then
    echo "  All checks passed."
else
    echo "  Some checks failed."
    exit 1
fi
echo "========================================="
