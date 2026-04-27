# Oracle Contract

The Oracle contract aggregates exchange rates from multiple validators and provides rate data to other contracts.

## Functions

### `initialize`
Initialize the contract with validators and currencies.

**Parameters:**
- `admin`: Admin address
- `validators`: Array of validator addresses (5 validators)
- `min_signatures`: Minimum signatures required (3 of 5)
- `currencies`: Supported currencies
- `basket_weights`: Currency weights in basket (basis points)

### `update_rate`
Update exchange rate for a currency (validator function).

**Parameters:**
- `currency`: Currency code
- `rate`: Rate in 7 decimals
- `sources`: Array of source rates for median calculation
- `timestamp`: Unix timestamp

**Access:** Validators only

**Events:** Emits `RateUpdateEvent`

### `get_rate`
Get current rate for a currency.

**Parameters:**
- `currency`: Currency code

**Returns:** Rate in 7 decimals

### `get_acbu_usd_rate`
Get ACBU/USD rate (basket-weighted).

**Returns:** ACBU/USD rate in 7 decimals

### `add_validator` / `remove_validator`
Manage validators (admin only).

### `get_validators`
Get all validator addresses.

### `get_min_signatures`
Get minimum signatures required.

## Rate Calculation

1. **Median Calculation:** Takes median of 3 source rates
2. **Outlier Detection:** Flags rates with >3% deviation
3. **Emergency Updates:** Allows updates if rate moves >5%
4. **ACBU/USD Rate:** Weighted sum of basket currencies

## Access Control

- **Update rates:** Validators only (multisig)
- **Read rates:** Public
- **Manage validators:** Admin only

## Events

### RateUpdateEvent
```rust
pub struct RateUpdateEvent {
    pub currency: CurrencyCode,
    pub rate: i128,
    pub timestamp: u64,
    pub validator: Address,
}
```

## Security Features

- Multi-validator consensus (3 of 5)
- Outlier detection
- Update interval enforcement (6 hours)
- Emergency update mechanism (>5% moves)
- Rate deviation limits

## Integration

The contract is used by:
- **Minting Contract:** For ACBU/USD and currency/USD rates
- **Burning Contract:** For currency/USD rates
- **Backend:** For rate updates and queries
