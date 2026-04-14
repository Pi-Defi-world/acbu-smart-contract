#!/bin/bash

# ACBU Soroban Contracts Deployment Script
# Usage: ./deploy.sh [testnet|mainnet]

set -e

NETWORK=${1:-testnet}
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Deploying ACBU contracts to ${NETWORK}${NC}"

# Check if soroban CLI is installed
if ! command -v soroban &> /dev/null; then
    echo -e "${RED}Error: soroban CLI not found. Please install it first:${NC}"
    echo "cargo install --locked soroban-cli"
    exit 1
fi

# Set network
if [ "$NETWORK" = "testnet" ]; then
    NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
    HORIZON_URL="https://horizon-testnet.stellar.org"
    FRIENDBOT_URL="https://friendbot.stellar.org"
elif [ "$NETWORK" = "mainnet" ]; then
    NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
    HORIZON_URL="https://horizon.stellar.org"
    FRIENDBOT_URL=""
    
    echo -e "${YELLOW}Warning: Deploying to MAINNET. Make sure you have:${NC}"
    echo "1. Tested on testnet"
    echo "2. Security audit completed"
    echo "3. Backup of secret keys"
    read -p "Continue? (yes/no): " confirm
    if [ "$confirm" != "yes" ]; then
        echo "Deployment cancelled"
        exit 1
    fi
else
    echo -e "${RED}Error: Invalid network. Use 'testnet' or 'mainnet'${NC}"
    exit 1
fi

# Check for secret key
if [ -z "$STELLAR_SECRET_KEY" ]; then
    echo -e "${RED}Error: STELLAR_SECRET_KEY environment variable not set${NC}"
    exit 1
fi

# Build contracts
echo -e "${GREEN}Building contracts...${NC}"
cd "$CONTRACTS_DIR"
cargo build --target wasm32-unknown-unknown --release

# Deploy contracts in order: Oracle -> Reserve Tracker -> Minting -> Burning -> Lending Pool -> Escrow
echo -e "${GREEN}Deploying contracts...${NC}"

# Deploy Oracle
echo -e "${YELLOW}Deploying Oracle contract...${NC}"
ORACLE_WASM="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/acbu_oracle.wasm"
ORACLE_ID=$(soroban contract deploy \
    --wasm "$ORACLE_WASM" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    | grep -oP 'Contract ID: \K[^\s]+')

echo -e "${GREEN}Oracle deployed: $ORACLE_ID${NC}"

# Deploy Reserve Tracker
echo -e "${YELLOW}Deploying Reserve Tracker contract...${NC}"
RESERVE_WASM="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/acbu_reserve_tracker.wasm"
RESERVE_ID=$(soroban contract deploy \
    --wasm "$RESERVE_WASM" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    | grep -oP 'Contract ID: \K[^\s]+')

echo -e "${GREEN}Reserve Tracker deployed: $RESERVE_ID${NC}"

# Deploy Minting
echo -e "${YELLOW}Deploying Minting contract...${NC}"
MINTING_WASM="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/acbu_minting.wasm"
MINTING_ID=$(soroban contract deploy \
    --wasm "$MINTING_WASM" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    | grep -oP 'Contract ID: \K[^\s]+')

echo -e "${GREEN}Minting deployed: $MINTING_ID${NC}"

# Deploy Burning
echo -e "${YELLOW}Deploying Burning contract...${NC}"
BURNING_WASM="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/acbu_burning.wasm"
BURNING_ID=$(soroban contract deploy \
    --wasm "$BURNING_WASM" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    | grep -oP 'Contract ID: \K[^\s]+')

echo -e "${GREEN}Burning deployed: $BURNING_ID${NC}"

# Deploy Lending Pool
echo -e "${YELLOW}Deploying Lending Pool contract...${NC}"
LENDING_WASM="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/acbu_lending_pool.wasm"
LENDING_ID=$(soroban contract deploy \
    --wasm "$LENDING_WASM" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    | grep -oP 'Contract ID: \K[^\s]+')

echo -e "${GREEN}Lending Pool deployed: $LENDING_ID${NC}"

# Deploy Escrow
echo -e "${YELLOW}Deploying Escrow contract...${NC}"
ESCROW_WASM="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/acbu_escrow.wasm"
ESCROW_ID=$(soroban contract deploy \
    --wasm "$ESCROW_WASM" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    | grep -oP 'Contract ID: \K[^\s]+')

echo -e "${GREEN}Escrow deployed: $ESCROW_ID${NC}"

# Save contract addresses
DEPLOYMENT_FILE="$CONTRACTS_DIR/.soroban/deployment_${NETWORK}.json"
mkdir -p "$(dirname "$DEPLOYMENT_FILE")"
cat > "$DEPLOYMENT_FILE" << EOF
{
  "network": "$NETWORK",
  "deployed_at": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "contracts": {
    "oracle": "$ORACLE_ID",
    "reserve_tracker": "$RESERVE_ID",
    "minting": "$MINTING_ID",
    "burning": "$BURNING_ID",
    "lending_pool": "$LENDING_ID",
    "escrow": "$ESCROW_ID"
  }
}
EOF

# Initialize contracts (Note: Placeholder addresses for tokens and admin)
ADMIN_ADDRESS=$(soroban keys address "$STELLAR_SECRET_KEY")
ACBU_TOKEN_ID="TODO_ACBU_TOKEN_ID"
USDC_TOKEN_ID="TODO_USDC_TOKEN_ID"
VAULT_ADDRESS="TODO_VAULT_ADDRESS"
TREASURY_ADDRESS="TODO_TREASURY_ADDRESS"
WD_PROC_ADDRESS="TODO_WD_PROC_ADDRESS"

echo -e "${YELLOW}Initializing contracts...${NC}"

# Initialize Oracle
soroban contract invoke \
    --id "$ORACLE_ID" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDRESS"

# Initialize Reserve Tracker
soroban contract invoke \
    --id "$RESERVE_ID" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDRESS" \
    --oracle "$ORACLE_ID" \
    --acbu_token "$ACBU_TOKEN_ID" \
    --min_ratio 10200

# Initialize Minting
soroban contract invoke \
    --id "$MINTING_ID" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDRESS" \
    --oracle "$ORACLE_ID" \
    --reserve_tracker "$RESERVE_ID" \
    --acbu_token "$ACBU_TOKEN_ID" \
    --usdc_token "$USDC_TOKEN_ID" \
    --vault "$VAULT_ADDRESS" \
    --treasury "$TREASURY_ADDRESS" \
    --fee_rate_bps 30 \
    --fee_single_bps 50

# Initialize Burning
soroban contract invoke \
    --id "$BURNING_ID" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDRESS" \
    --oracle "$ORACLE_ID" \
    --reserve_tracker "$RESERVE_ID" \
    --acbu_token "$ACBU_TOKEN_ID" \
    --withdrawal_processor "$WD_PROC_ADDRESS" \
    --vault "$VAULT_ADDRESS" \
    --fee_rate_bps 30 \
    --fee_single_redeem_bps 100

# Initialize Lending Pool
soroban contract invoke \
    --id "$LENDING_ID" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDRESS" \
    --acbu_token "$ACBU_TOKEN_ID"

# Initialize Escrow
soroban contract invoke \
    --id "$ESCROW_ID" \
    --network "$NETWORK" \
    --source "$STELLAR_SECRET_KEY" \
    -- \
    initialize \
    --admin "$ADMIN_ADDRESS" \
    --acbu_token "$ACBU_TOKEN_ID"

echo -e "${GREEN}Deployment and initialization complete!${NC}"
echo -e "${GREEN}Contract addresses saved to: $DEPLOYMENT_FILE${NC}"
echo ""
echo "Contract Addresses:"
echo "  Oracle: $ORACLE_ID"
echo "  Reserve Tracker: $RESERVE_ID"
echo "  Minting: $MINTING_ID"
echo "  Burning: $BURNING_ID"
