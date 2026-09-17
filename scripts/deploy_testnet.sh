#!/bin/bash

# Deploy to testnet
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/deploy.sh" testnet

echo ""
echo "Next: seed oracle rates + s_token map, reserve USD values, mint demo SAC supply to the"
echo "minting contract address, then wire CONTRACT_* env vars. See:"
echo "  ACBU-DOCUMENTATION/TESTNET_CUSTODIAL_BOOTSTRAP.md"
