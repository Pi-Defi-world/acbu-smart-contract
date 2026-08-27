# Contract Deployment Guide

This guide walks you through deploying ACBU Soroban smart contracts to Stellar testnet and mainnet.

## Automated deployment (preferred)

Deployment is scripted and recorded — you do not have to run the steps below by hand:

| Path | What it does |
| --- | --- |
| **CI:** `.github/workflows/deploy.yml` | `Actions → Deploy Contracts → Run workflow`, pick `testnet`/`mainnet` (mainnet also needs `confirm: deploy`). Builds with `--locked`, runs `scripts/deploy.sh`, and uploads `.soroban/deployment_<network>.json` as a run artifact plus a job-summary record. Needs the `STELLAR_SECRET_KEY` secret on the matching GitHub Environment. |
| **Local:** `./scripts/deploy.sh testnet` / `mainnet` | Same script the workflow calls. Wrappers: `scripts/deploy_testnet.sh`, `scripts/deploy_mainnet.sh`. Verify afterwards with `scripts/verify_deployment.sh`. |

The recorded `.soroban/deployment_<network>.json` (network, timestamp, per-contract IDs) is the source of truth for every deploy.

## Reproducible builds

`Cargo.lock` is committed and every dependency resolves from a registry with a
recorded checksum — no crate is pulled from a git URL. Always build with
`cargo build --locked` so the pinned graph is used, and the build resolves
without live GitHub access. `.github/workflows/deps-guard.yml` enforces this.

## Prerequisites

1. **Rust Toolchain** (1.70+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Soroban CLI**
   ```bash
   cargo install --locked soroban-cli
   ```

3. **Stellar Account**
   - For testnet: Use [Stellar Laboratory](https://laboratory.stellar.org/#account-creator?network=test) to create a test account
   - For mainnet: Create an account with XLM for deployment fees

4. **Environment Variables**
   ```bash
   export STELLAR_SECRET_KEY="your-secret-key-here"
   ```

## Building Contracts

Before deployment, build all contracts:

```bash
cd contracts
cargo build --target wasm32-unknown-unknown --release
```

This will create WASM files in `target/wasm32-unknown-unknown/release/`:
- `minting.wasm`
- `burning.wasm`
- `oracle.wasm`
- `reserve_tracker.wasm`

## Testnet Deployment

1. **Set your secret key:**
   ```bash
   export STELLAR_SECRET_KEY="your-testnet-secret-key"
   ```

2. **Run deployment script:**
   ```bash
   chmod +x scripts/deploy_testnet.sh
   ./scripts/deploy_testnet.sh
   ```

3. **Save contract addresses:**
   After deployment, contract addresses are saved to `.soroban/deployment_testnet.json`

4. **Set environment variables for backend:**
   ```bash
   export CONTRACT_ORACLE_TESTNET="<oracle-contract-id>"
   export CONTRACT_RESERVE_TRACKER_TESTNET="<reserve-tracker-contract-id>"
   export CONTRACT_MINTING_TESTNET="<minting-contract-id>"
   export CONTRACT_BURNING_TESTNET="<burning-contract-id>"
   ```

## Mainnet Deployment

**⚠️ WARNING: Only deploy to mainnet after:**
- ✅ Testing on testnet
- ✅ Security audit completion
- ✅ Backup of secret keys
- ✅ Team approval

1. **Set your secret key:**
   ```bash
   export STELLAR_SECRET_KEY="your-mainnet-secret-key"
   ```

2. **Run deployment script:**
   ```bash
   chmod +x scripts/deploy_mainnet.sh
   ./scripts/deploy_mainnet.sh
   ```

3. **Save contract addresses:**
   Contract addresses are saved to `.soroban/deployment_mainnet.json`

4. **Set environment variables for backend:**
   ```bash
   export CONTRACT_ORACLE_MAINNET="<oracle-contract-id>"
   export CONTRACT_RESERVE_TRACKER_MAINNET="<reserve-tracker-contract-id>"
   export CONTRACT_MINTING_MAINNET="<minting-contract-id>"
   export CONTRACT_BURNING_MAINNET="<burning-contract-id>"
   ```

## Contract Initialization

After deployment, contracts need to be initialized. This is done through the backend services or directly via Soroban CLI.

### Oracle Contract

```bash
soroban contract invoke \
  --id <oracle-contract-id> \
  --network testnet \
  --source <admin-secret-key> \
  -- initialize \
  --admin <admin-address> \
  --validators '[<validator1>, <validator2>, ...]' \
  --min_signatures 3 \
  --currencies '["NGN", "KES", "RWF"]' \
  --basket_weights '{"NGN": 1800, "KES": 1200, "RWF": 800}'
```

### Reserve Tracker Contract

```bash
soroban contract invoke \
  --id <reserve-tracker-contract-id> \
  --network testnet \
  --source <admin-secret-key> \
  -- initialize \
  --admin <admin-address> \
  --acbu_token <acbu-token-contract-id> \
  --min_ratio_bps 10200 \
  --target_ratio_bps 10500
```

### Minting Contract

```bash
soroban contract invoke \
  --id <minting-contract-id> \
  --network testnet \
  --source <admin-secret-key> \
  -- initialize \
  --admin <admin-address> \
  --oracle <oracle-contract-id> \
  --reserve_tracker <reserve-tracker-contract-id> \
  --acbu_token <acbu-token-contract-id> \
  --usdc_token <usdc-token-contract-id> \
  --fee_rate_bps 300
```

### Burning Contract

```bash
soroban contract invoke \
  --id <burning-contract-id> \
  --network testnet \
  --source <admin-secret-key> \
  -- initialize \
  --admin <admin-address> \
  --oracle <oracle-contract-id> \
  --reserve_tracker <reserve-tracker-contract-id> \
  --acbu_token <acbu-token-contract-id> \
  --withdrawal_processor <withdrawal-processor-address> \
  --fee_rate_bps 300
```

## Verification

After deployment and initialization, verify contracts:

1. **Check contract state:**
   ```bash
   soroban contract invoke \
     --id <contract-id> \
     --network testnet \
     -- get_fee_rate
   ```

2. **Verify on Stellar Explorer:**
   - Testnet: https://stellar.expert/explorer/testnet
   - Mainnet: https://stellar.expert/explorer/public

## Troubleshooting

### "Insufficient balance"
- Ensure your account has enough XLM for deployment fees
- Testnet: Use friendbot to fund your account

### "Contract already initialized"
- Contract can only be initialized once
- Deploy a new contract instance if needed

### "Unauthorized"
- Ensure you're using the correct admin secret key
- Check that the contract is initialized with your address as admin

## Security Notes

1. **Never commit secret keys** to version control
2. **Use environment variables** for all sensitive data
3. **Backup contract addresses** after deployment
4. **Test thoroughly** on testnet before mainnet
5. **Use multisig** for admin operations in production

## Next Steps

After deployment:
1. Update backend environment variables
2. Initialize contracts
3. Test contract interactions
4. Set up event listeners
5. Monitor contract activity
