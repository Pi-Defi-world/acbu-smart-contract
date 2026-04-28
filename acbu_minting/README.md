# Minting Contract

The Minting contract handles conversion of USDC and fiat deposits into ACBU tokens.

## Functions

### `initialize`
Initialize the contract with configuration.

**Parameters:**
- `admin`: Admin address (multisig)
- `oracle`: Oracle contract address
- `reserve_tracker`: Reserve tracker contract address
- `acbu_token`: ACBU token contract address
- `usdc_token`: USDC token contract address
- `fee_rate_bps`: Fee rate in basis points (e.g., 300 = 0.3%)

### `mint_from_usdc`
Mint ACBU from USDC deposit.

**Parameters:**
- `usdc_amount`: USDC amount (7 decimals)
- `recipient`: Recipient Stellar address

**Returns:** ACBU amount minted

**Events:** Emits `MintEvent`

### `mint_from_fiat`
Mint ACBU from fiat deposit (via fintech partner).

**Parameters:**
- `currency`: Currency code (NGN, KES, RWF)
- `amount`: Fiat amount (7 decimals)
- `recipient`: Recipient Stellar address
- `fintech_tx_id`: Fintech transaction ID

**Returns:** ACBU amount minted

**Events:** Emits `MintEvent`

### `pause` / `unpause`
Pause/unpause the contract (admin only).

### `set_fee_rate`
Update fee rate (admin only).

**Parameters:**
- `fee_rate_bps`: New fee rate in basis points

### `get_fee_rate`
Get current fee rate.

**Returns:** Fee rate in basis points

### `is_paused`
Check if contract is paused.

**Returns:** Boolean

## Access Control

- **Minting:** Any user (with KYC verification)
- **Admin functions:** Admin only (multisig in production)
- **Pause/Unpause:** Admin only

## Token transfer model

- `mint_from_usdc` uses a push model: users transfer USDC into the minting contract.
- `mint_from_single` and `mint_from_basket` also use push semantics: users transfer underlying S-tokens into the configured vault.
- These minting flows do not rely on `approve`/`transfer_from` allowances from the user side.

## Events

### MintEvent
```rust
pub struct MintEvent {
    pub transaction_id: String,
    pub user: Address,
    pub usdc_amount: i128,
    pub acbu_amount: i128,
    pub fee: i128,
    pub rate: i128,
    pub timestamp: u64,
}
```

## Security Features

- Minimum/maximum mint amounts
- Reserve verification before minting
- Oracle rate validation
- Emergency pause mechanism
- Fee calculation and collection

## Integration

The contract integrates with:
- **Oracle Contract:** For ACBU/USD and currency/USD rates
- **Reserve Tracker:** For reserve verification
- **ACBU Token:** For minting tokens
- **USDC Token:** For receiving USDC deposits
