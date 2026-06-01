#!/usr/bin/env bash
# fetch_token_wasm.sh — download the pinned soroban_token_contract.wasm
#
# The WASM artifact is NOT stored in git (see .gitignore).  Run this script
# once after cloning — or whenever you need to rebuild — to place the verified
# artifact at the project root where contractimport! expects it.
#
# Verification: the script checks the downloaded file against the pinned
# SHA-256 hash before it is usable.  If the hash does not match, the file
# is deleted and the script exits non-zero.
#
# Usage:
#   ./scripts/fetch_token_wasm.sh
#   ./scripts/fetch_token_wasm.sh --force   # overwrite an existing file

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DEST="$PROJECT_ROOT/soroban_token_contract.wasm"

# SHA-256 of the expected artifact — must match contractimport! sha256 fields.
EXPECTED_HASH="6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d"

# Stellar / soroban-examples release that ships this exact token contract.
# If the upstream URL changes, update this variable and the EXPECTED_HASH above.
RELEASE_URL="https://github.com/stellar/soroban-examples/releases/download/v21.7.1/soroban_token_contract.wasm"

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
echo -e "${YELLOW}[INFO]${NC} Downloading soroban_token_contract.wasm ..."
if command -v curl &>/dev/null; then
  curl -fsSL "$RELEASE_URL" -o "$DEST"
elif command -v wget &>/dev/null; then
  wget -q "$RELEASE_URL" -O "$DEST"
else
  echo -e "${RED}[FAIL]${NC} Neither curl nor wget found. Install one and retry."
  exit 1
fi

# ── Verify ──────────────────────────────────────────────────────────────────
ACTUAL=$(sha256sum "$DEST" | awk '{print $1}')
if [[ "$ACTUAL" != "$EXPECTED_HASH" ]]; then
  rm -f "$DEST"
  echo -e "${RED}[FAIL]${NC} SHA-256 mismatch — downloaded file rejected."
  echo "  expected: $EXPECTED_HASH"
  echo "  actual:   $ACTUAL"
  echo ""
  echo "Do NOT use this artifact.  Check whether the release URL is correct"
  echo "or whether the pinned hash needs updating."
  exit 1
fi

echo -e "${GREEN}[OK]${NC} soroban_token_contract.wasm verified ($ACTUAL)"
echo "Artifact is ready for use."
