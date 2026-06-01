# ACBU Soroban Smart Contracts

Soroban (Stellar) smart contracts for the ACBU (African Currency Basket Unit) stablecoin platform.

## Contracts

- **Minting Contract** - Handles USDC → ACBU conversions
- **Burning Contract** - Handles ACBU → Fiat redemptions
- **Oracle Contract** - Aggregates exchange rates from multiple validators
- **Reserve Tracker Contract** - Tracks and verifies reserve balances

## Prerequisites

- Rust 1.70 or higher
- Soroban CLI (`cargo install --locked soroban-cli`)
- Stellar account with XLM for deployment fees

## Building

```bash
# Build all contracts in the workspace
cargo build --target wasm32-unknown-unknown --release

# Build specific contract
cd acbu_minting
cargo build --target wasm32-unknown-unknown --release
```

## Testing

```bash
# Run all tests in the workspace
cargo test

# Run tests for specific contract
cd acbu_minting
cargo test
```

## Deployment

### Testnet

```bash
export STELLAR_SECRET_KEY="your-secret-key"
./scripts/deploy_testnet.sh
```

### Mainnet

```bash
export STELLAR_SECRET_KEY="your-secret-key"
./scripts/deploy_mainnet.sh
```

**Warning:** Only deploy to mainnet after:
1. Testing on testnet
2. Security audit completion
3. Backup of secret keys

## Contract Addresses

After deployment, contract addresses are saved to `.soroban/deployment_{network}.json`

## Development

### Project Structure

```
.
├── acbu_minting/           # Minting contract
├── acbu_burning/           # Burning contract
├── acbu_oracle/            # Oracle contract
├── acbu_reserve_tracker/   # Reserve tracker contract
├── acbu_savings_vault/     # Savings vault contract
├── acbu_lending_pool/      # Lending pool contract
├── acbu_escrow/            # Escrow contract
├── acbu_multisig/          # Multisig shared contract
├── shared/                 # Shared types and utilities
├── scripts/                # Deployment scripts
├── docs/                   # Documentation
└── tests/                  # Integration tests
```

### Adding a New Contract

1. Create contract directory: `mkdir new_contract`
2. Add to workspace `Cargo.toml` members
3. Create `Cargo.toml` and `src/lib.rs`
4. Update deployment scripts

## Security

- All admin functions require multisig (3 of 5)
- Rate limits on transactions
- Circuit breakers for anomalies
- Time locks for critical operations

## Documentation

See individual contract README files for detailed documentation.
