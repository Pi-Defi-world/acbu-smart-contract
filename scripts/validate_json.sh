#!/usr/bin/env bash
# validate_json.sh – validate config JSON files against their schemas.
# Requires: ajv-cli  (npm install -g ajv-cli)
# Usage: ./scripts/validate_json.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

check_dep() {
    if ! command -v ajv &>/dev/null; then
        echo "ERROR: ajv not found. Install with: npm install -g ajv-cli"
        exit 1
    fi
}

validate() {
    local schema="$1"
    local data="$2"
    echo "Validating $(basename "$data") ..."
    if ajv validate -s "$schema" -d "$data" --spec=draft7 2>&1; then
        echo "  ✓ $(basename "$data") is valid"
    else
        echo "  ✗ $(basename "$data") FAILED validation"
        exit 1
    fi
}

check_dep

validate "$REPO_ROOT/schemas/validators.schema.json"              "$REPO_ROOT/validators.json"
validate "$REPO_ROOT/schemas/weights.schema.json"                 "$REPO_ROOT/weights.json"
validate "$REPO_ROOT/schemas/oracle_basket_currencies.schema.json" "$REPO_ROOT/scripts/oracle_basket_currencies.json"
validate "$REPO_ROOT/schemas/oracle_basket_weights_bps.schema.json" "$REPO_ROOT/scripts/oracle_basket_weights_bps.json"

echo ""
echo "All JSON files passed schema validation."
