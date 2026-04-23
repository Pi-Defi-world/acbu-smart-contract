#![no_std]

use soroban_sdk::{contracttype, Address, String as SorobanString, Vec};

/// Currency code type (e.g., "NGN", "KES", "RWF")
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrencyCode(pub Vec<SorobanString>);

impl CurrencyCode {
    pub fn new(env: &soroban_sdk::Env, code: &str) -> Self {
        let mut v = Vec::new(env);
        v.push_back(SorobanString::from_str(env, code));
        CurrencyCode(v)
    }
}

/// Rate data structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct RateData {
    pub currency: CurrencyCode,
    pub rate_usd: i128, // Rate in 7 decimals (e.g., 0.0012345 = 12345)
    pub timestamp: u64,
    pub sources: soroban_sdk::Vec<i128>, // Source rates for median calculation
}

/// Reserve data structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct ReserveData {
    pub currency: CurrencyCode,
    pub amount: i128,    // Reserve amount in native currency
    pub value_usd: i128, // Value in USD (7 decimals)
    pub timestamp: u64,
}

/// Account details for withdrawals
#[contracttype]
#[derive(Clone, Debug)]
pub struct AccountDetails {
    pub account_number: SorobanString,
    pub bank_code: SorobanString,
    pub account_name: SorobanString,
    pub currency: CurrencyCode,
}

/// Mint event payload emitted by the minting contract.
///
/// **Contract topics (Soroban):** `(Symbol \"mint\", Address recipient)` — the `user` field below
/// is always the mint recipient (same as the topic address).
///
/// **Backend / indexer alignment:** Map XDR or RPC event fields to these names in order. All
/// `i128` amounts use **7 decimal places** (`DECIMALS` = 10_000_000 per whole unit). `rate` is
/// the ACBU/USD rate in the same fixed-point form. `usdc_amount` is USDC in 7 decimals for
/// `mint_from_usdc`; for Afreum S-token mint paths it carries the USD-equivalent notional
/// (still 7-decimal fixed point).
#[contracttype]
#[derive(Clone, Debug)]
pub struct MintEvent {
    pub transaction_id: SorobanString,
    pub user: Address,
    pub usdc_amount: i128,
    pub acbu_amount: i128,
    pub fee: i128,
    pub rate: i128,
    pub timestamp: u64,
}

/// Burn event payload emitted by the burning contract.
///
/// **Contract topics (Soroban):** `(Symbol \"burn\", Address user)` — matches the `user` field.
///
/// **Backend / indexer alignment:** Same field order as XDR struct encoding. Amounts (`acbu_amount`,
/// `local_amount`, `fee`, `rate`) are **7-decimal fixed point** (`DECIMALS`). `currency` is
/// [`CurrencyCode`] (string code such as `\"NGN\"`). For `burn_for_basket`, one event is emitted per
/// recipient slice; `acbu_amount` and `fee` are the portions for that slice, not necessarily the
/// full transaction totals.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BurnEvent {
    pub transaction_id: SorobanString,
    pub user: Address,
    pub acbu_amount: i128,
    pub local_amount: i128,
    pub currency: CurrencyCode,
    pub fee: i128,
    pub rate: i128,
    pub timestamp: u64,
}

/// Rate update event data
#[contracttype]
#[derive(Clone, Debug)]
pub struct RateUpdateEvent {
    pub currency: CurrencyCode,
    pub rate: i128,
    pub timestamp: u64,
    pub validators: soroban_sdk::Vec<Address>,
}

/// Outlier detection event data
#[contracttype]
#[derive(Clone, Debug)]
pub struct OutlierDetectionEvent {
    pub currency: CurrencyCode,
    pub median_rate: i128,
    pub outlier_rate: i128,
    pub deviation_bps: i128,
    pub timestamp: u64,
}

/// Error types
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    Unauthorized,
    Paused,
    InvalidAmount,
    InvalidRate,
    InsufficientReserves,
    RateLimitExceeded,
    InvalidCurrency,
    OracleError,
    ReserveError,
    InsufficientBalance,
}

/// Constants
pub const BASIS_POINTS: i128 = 10_000;
pub const DECIMALS: i128 = 10_000_000; // 7 decimals
pub const MIN_MINT_AMOUNT: i128 = 10_000_000; // 10 USDC (7 decimals)
pub const MAX_MINT_AMOUNT: i128 = 1_000_000_000_000; // 1M USDC (7 decimals)
pub const MIN_BURN_AMOUNT: i128 = 10_000_000; // 10 ACBU (7 decimals)
pub const UPDATE_INTERVAL_SECONDS: u64 = 21_600; // 6 hours
pub const EMERGENCY_THRESHOLD_BPS: i128 = 500; // 5% deviation threshold
pub const OUTLIER_THRESHOLD_BPS: i128 = 300; // 3% deviation for outlier detection

/// Utility functions
pub fn calculate_fee(amount: i128, fee_rate_bps: i128) -> i128 {
    (amount * fee_rate_bps) / BASIS_POINTS
}

pub fn calculate_amount_after_fee(amount: i128, fee_rate_bps: i128) -> i128 {
    amount - calculate_fee(amount, fee_rate_bps)
}

/// Calculate median using in-place quickselect algorithm
/// This avoids unnecessary allocations (clone) and reduces gas consumption
pub fn median(mut values: soroban_sdk::Vec<i128>) -> Option<i128> {
    if values.is_empty() {
        return None;
    }

    let n = values.len();
    let mid = n / 2;

    if n % 2 == 0 {
        // For even count, find two middle elements and average them
        let _ = quickselect_inplace(&mut values, 0, (n - 1) as i32, (mid - 1) as i32);
        let val1 = values.get((mid - 1) as u32)?;
        let _ = quickselect_inplace(&mut values, 0, (n - 1) as i32, mid as i32);
        let val2 = values.get(mid as u32)?;
        Some((val1 + val2) / 2)
    } else {
        // For odd count, find the middle element
        let _ = quickselect_inplace(&mut values, 0, (n - 1) as i32, mid as i32);
        Some(values.get(mid as u32)?)
    }
}

/// In-place quickselect to find the k-th smallest element
/// Based on Hoare's selection algorithm for O(n) average performance without cloning
fn quickselect_inplace(values: &mut soroban_sdk::Vec<i128>, mut left: i32, mut right: i32, k: i32) {
    while left < right {
        let pivot_index = partition_inplace(values, left, right);
        if k == pivot_index {
            return;
        } else if k < pivot_index {
            right = pivot_index - 1;
        } else {
            left = pivot_index + 1;
        }
    }
}

/// Partition array in-place for quickselect using Lomuto partition scheme
fn partition_inplace(values: &mut soroban_sdk::Vec<i128>, left: i32, right: i32) -> i32 {
    let pivot_value = values.get(right as u32).unwrap_or(0);
    let mut i = left - 1;

    for j in left..right {
        let val_j = values.get(j as u32).unwrap_or(0);
        if val_j < pivot_value {
            i += 1;
            let idx_i = i as u32;
            let idx_j = j as u32;
            let val_i = values.get(idx_i).unwrap_or(0);
            values.set(idx_i, val_j);
            values.set(idx_j, val_i);
        }
    }

    let idx_i_plus_1 = (i + 1) as u32;
    let idx_right = right as u32;
    let val_i_plus_1 = values.get(idx_i_plus_1).unwrap_or(0);
    values.set(idx_i_plus_1, pivot_value);
    values.set(idx_right, val_i_plus_1);

    i + 1
}

/// Calculate percentage deviation
pub fn calculate_deviation(value1: i128, value2: i128) -> i128 {
    if value2 == 0 {
        return i128::MAX;
    }
    let diff = if value1 > value2 {
        value1 - value2
    } else {
        value2 - value1
    };
    (diff * BASIS_POINTS) / value2
}

