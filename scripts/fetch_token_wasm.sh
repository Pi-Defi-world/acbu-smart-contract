#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEST="$PROJECT_ROOT/soroban_token_contract.wasm"

# SHA-256 of the expected artifact — must match contractimport! sha256 fields.
EXPECTED_HASH="eb1a53948744e12a6b00ec891b301ebc78a06deb984d3726c9cbc315392aedec"

# Stellar / soroban-examples release that ships this exact token contract.
# The soroban-examples repo does not publish pre-built WASM binaries;
# we must clone and build the token contract from source.
SOROBAN_EXAMPLES_TAG="v21.6.0"
SOROBAN_EXAMPLES_REPO="https://github.com/stellar/soroban-examples.git"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

force=0
for arg in "$@"; do
  [[ "$arg" == "--force" ]] && force=1
done

# ── Already present? ────────────────────────────────────────────────────────
if [[ -f "$DEST" && "$force" -eq 0 ]]; then
  ACTUAL=$(sha256sum "$DEST" | awk '{print $1}')
  if [[ "$ACTUAL" == "$EXPECTED_HASH" ]]; then
    echo -e "${GREEN}[OK]${NC} soroban_token_contract.wasm already present and verified."
    exit 0
  fi
  echo -e "${YELLOW}[WARN]${NC} Existing file has unexpected hash — re-downloading."
fi

# ── Download ────────────────────────────────────────────────────────────────
echo -e "${YELLOW}[INFO]${NC} Building soroban_token_contract.wasm from source ..."

# Create temporary directory for the build
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Clone the soroban-examples repo at the pinned tag
git clone --depth 1 --branch "$SOROBAN_EXAMPLES_TAG" "$SOROBAN_EXAMPLES_REPO" "$TEMP_DIR/soroban-examples" 2>&1 | grep -v "^Cloning" || true

# Build the token contract
cd "$TEMP_DIR/soroban-examples/contracts/tokens/stellar_asset"
if cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1; then
  :
else
  echo -e "${RED}[FAIL]${NC} Failed to build soroban_token_contract from source."
  exit 1
fi

# Copy the built WASM to the destination
BUILT_WASM="$TEMP_DIR/soroban-examples/target/wasm32-unknown-unknown/release/soroban_token_contract.wasm"
if [[ ! -f "$BUILT_WASM" ]]; then
  echo -e "${RED}[FAIL]${NC} Built WASM file not found at expected location."
  exit 1
fi

cp "$BUILT_WASM" "$DEST"
cd "$PROJECT_ROOT"

# ── Verify ──────────────────────────────────────────────────────────────────
ACTUAL=$(sha256sum "$DEST" | awk '{print $1}')
if [[ "$ACTUAL" != "$EXPECTED_HASH" ]]; then
  rm -f "$DEST"
  echo -e "${RED}[FAIL]${NC} SHA-256 mismatch — built artifact rejected."
  echo "  expected: $EXPECTED_HASH"
  echo "  actual:   $ACTUAL"
  echo ""
  echo "Do NOT use this artifact. The source or build may have changed."
  echo "Update EXPECTED_HASH and SOROBAN_EXAMPLES_TAG if necessary."
  exit 1
fi

echo "[OK] soroban_token_contract.wasm ready at $DEST"