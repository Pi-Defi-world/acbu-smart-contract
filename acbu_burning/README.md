# Burning Contract

The Burning contract handles ACBU token redemption and triggers fiat withdrawals.

## Functions

### `initialize`
Initialize the contract with configuration.

**Parameters:**
- `admin`: Admin address
- `oracle`: Oracle contract address
- `reserve_tracker`: Reserve tracker contract address
- `acbu_token`: ACBU token contract address
- `withdrawal_processor`: Withdrawal processor address
- `fee_rate_bps`: Fee rate in basis points

### `burn_for_currency`
Burn ACBU for single currency redemption.

**Parameters:**
- `acbu_amount`: ACBU amount to burn (7 decimals)
- `currency`: Currency code (NGN, KES, RWF)
- `recipient_account`: Account details for withdrawal

**Returns:** Local currency amount

**Events:** Emits `BurnEvent`

### `burn_for_basket`
Burn ACBU for basket redemption (proportional).

**Parameters:**
- `acbu_amount`: ACBU amount to burn
- `recipient_accounts`: Array of account details per currency

**Returns:** Array of local currency amounts

**Events:** Emits `BurnEvent` for each currency

### `pause` / `unpause`
Pause/unpause the contract (admin only).

### `set_fee_rate`
Update fee rate (admin only).

### `get_fee_rate`
Get current fee rate.

### `is_paused`
Check if contract is paused.

## Access Control

- **Burning:** Any user (with KYC verification)
- **Admin functions:** Admin only
- **Withdrawal limits:** Enforced per currency

## Token transfer model

- `redeem_single` and `redeem_basket` use a pull model for S-token redemption.
- The configured `vault` address must grant this contract allowance via `approve` on the underlying S-token contracts.
- The contract then consumes that allowance with `transfer_from` to move S-tokens from the vault to the recipient.

## Events

### BurnEvent
```rust
pub struct BurnEvent {
    pub transaction_id: String,
    pub user: Address,
    pub acbu_amount: i128,
    pub local_amount: i128,
    pub currency: CurrencyCode,
    pub fee: i128,
    pub rate: i128,
    pub timestamp: u64,
}
```

## Security Features

- Minimum burn amounts
- Reserve availability verification
- Currency rate validation
- Emergency pause mechanism
- Fee calculation

## Integration

The contract integrates with:
- **Oracle Contract:** For currency/USD rates
- **Reserve Tracker:** For reserve verification
- **ACBU Token:** For burning tokens
- **Backend:** For processing withdrawals (via events)
