#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Default values
CLUSTER="devnet"
KEYPAIR="$HOME/.config/solana/id.json"
SKIP_BUILD=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --cluster)
            CLUSTER="$2"
            shift 2
            ;;
        --keypair)
            KEYPAIR="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --help)
            echo "Usage: deploy.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --cluster <CLUSTER>   Solana cluster: devnet, testnet, mainnet-beta (default: devnet)"
            echo "  --keypair <PATH>      Path to deployer keypair (default: ~/.config/solana/id.json)"
            echo "  --skip-build          Skip the build step before deploying"
            echo "  --help                Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo "========================================="
echo "  IVZA Deploy Script"
echo "========================================="
echo ""
echo "  Cluster:  $CLUSTER"
echo "  Keypair:  $KEYPAIR"
echo ""

# Validate tools
if ! command -v solana &> /dev/null; then
    echo "ERROR: solana CLI not found."
    exit 1
fi

if ! command -v anchor &> /dev/null; then
    echo "ERROR: anchor CLI not found."
    exit 1
fi

# Validate keypair
if [ ! -f "$KEYPAIR" ]; then
    echo "ERROR: Keypair file not found at $KEYPAIR"
    exit 1
fi

# Set Solana config
echo "[1/4] Configuring Solana CLI..."
solana config set --url "$CLUSTER" --keypair "$KEYPAIR"

# Show deployer balance
echo ""
echo "[2/4] Checking deployer balance..."
BALANCE=$(solana balance --keypair "$KEYPAIR" | tr -d ' ')
echo "  Balance: $BALANCE"

# Build
if [ "$SKIP_BUILD" = false ]; then
    echo ""
    echo "[3/4] Building Anchor program..."
    cd "$PROJECT_DIR"
    anchor build
    echo "  Build complete."
else
    echo ""
    echo "[3/4] Skipping build (--skip-build flag set)."
fi

# Deploy
echo ""
echo "[4/4] Deploying to $CLUSTER..."
cd "$PROJECT_DIR"
anchor deploy --provider.cluster "$CLUSTER" --provider.wallet "$KEYPAIR"

echo ""
echo "========================================="
echo "  Deployment complete."
echo "========================================="

# Show program IDs
echo ""
echo "Deployed program IDs:"
if [ -d "$PROJECT_DIR/target/deploy" ]; then
    for keyfile in "$PROJECT_DIR"/target/deploy/*-keypair.json; do
        if [ -f "$keyfile" ]; then
            PROGRAM_NAME=$(basename "$keyfile" | sed 's/-keypair.json//')
            PROGRAM_ID=$(solana-keygen pubkey "$keyfile")
            echo "  $PROGRAM_NAME: $PROGRAM_ID"
        fi
    done
fi
