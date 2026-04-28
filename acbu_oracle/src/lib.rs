#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Map,
    Symbol, Vec,
};

use shared::{
    calculate_deviation, median, CurrencyCode, DataKey as SharedDataKey, OutlierDetectionEvent,
    RateData, RateUpdateEvent, BASIS_POINTS, CONTRACT_VERSION, DECIMALS, EMERGENCY_THRESHOLD_BPS,
    MAX_VALIDATORS, OUTLIER_THRESHOLD_BPS, STALE_RATE_MAX_LEDGERS, UPDATE_INTERVAL_SECONDS,
    calculate_deviation, median, CONTRACT_VERSION, CurrencyCode, DataKey as SharedDataKey, OutlierDetectionEvent, RateData, RateUpdateEvent,
    BASIS_POINTS, DECIMALS, EMERGENCY_THRESHOLD_BPS, OUTLIER_THRESHOLD_BPS, STALE_RATE_MAX_LEDGERS,
    UPDATE_INTERVAL_SECONDS,
};

mod shared {
    pub use shared::*;
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum OracleError {
    AlreadyInitialized = 7001,
    InvalidMinSignatures = 7002,
    MinSignaturesZero = 7003,
    NoPendingAdmin = 7004,
    AdminTimelockNotElapsed = 7005,
    NoPendingAdminToCancel = 7006,
    UnauthorizedValidator = 7007,
    UpdateIntervalNotMet = 7008,
    InsufficientOracleSources = 7009,
    InvalidRate = 7010,
    RateNotFound = 7011,
    STokenNotConfigured = 7012,
    ValidatorAlreadyExists = 7013,
    CannotRemoveValidator = 7014,
    InvalidVersion = 7015,
    RateStaleLedger = 7016,
}

// ─── Admin rotation timelock (seconds) ───────────────────────────────────────
/// How long the pending admin must wait before they can claim ownership.
/// 24 hours gives the current admin time to cancel a mistaken/malicious transfer.
const ADMIN_TIMELOCK_SECONDS: u64 = 86_400;

/// Minimum number of oracle source feeds required to derive a quorum-based rate.
const MIN_ORACLE_SOURCE_FEEDS: u32 = 3;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub validators: Symbol,
    pub min_signatures: Symbol,
    pub currencies: Symbol,
    pub rates: Symbol,
    pub last_update: Symbol,
    pub update_interval: Symbol,
    pub basket_weights: Symbol,
    pub s_tokens: Symbol,
    pub version: Symbol,
    // ── New keys for two-step admin rotation ──────────────────────────────
    pub pending_admin: Symbol,
    pub pending_admin_eligible_at: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    validators: symbol_short!("VALIDTRS"),
    min_signatures: symbol_short!("MIN_SIG"),
    currencies: symbol_short!("CURRNCYS"),
    rates: symbol_short!("RATES"),
    last_update: symbol_short!("LAST_UPD"),
    update_interval: symbol_short!("UPD_INT"),
    basket_weights: symbol_short!("BSK_WTS"),
    s_tokens: symbol_short!("S_TOKNS"),
    version: symbol_short!("VERSION"),
    pending_admin: symbol_short!("PEND_ADM"),
    pending_admin_eligible_at: symbol_short!("PEND_ETA"),
};

const VERSION: u32 = 9; // bumped from 8 → 9 for admin rotation feature

// ─── Admin rotation event payloads ───────────────────────────────────────────

/// Emitted when the current admin nominates a successor.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferInitiatedEvent {
    pub current_admin: Address,
    pub pending_admin: Address,
    /// Ledger timestamp after which `accept_admin` is callable.
    pub eligible_at: u64,
}

/// Emitted when the pending admin successfully claims ownership.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferCompletedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
    pub timestamp: u64,
}

/// Emitted when the current admin cancels a pending transfer.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferCancelledEvent {
    pub admin: Address,
    pub cancelled_pending: Address,
    pub timestamp: u64,
}

/// Emitted when a rate read is rejected because the stored rate is too old.
/// Consumers (e.g. monitoring bots) can subscribe to this to alert on stale feeds.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StaleRateEvent {
    pub currency: CurrencyCode,
    pub stored_ledger: u32,
    pub current_ledger: u32,
    pub max_stale_ledgers: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ValidatorSignature {
    pub validator: Address,
    pub timestamp: u64,
}

#[contract]
pub struct OracleContract;

#[contractimpl]
impl OracleContract {
    // ─────────────────────────────────────────────────────────────────────────
    // Initialisation
    // ─────────────────────────────────────────────────────────────────────────

    pub fn initialize(
        env: Env,
        admin: Address,
        validators: Vec<Address>,
        min_signatures: u32,
        currencies: Vec<CurrencyCode>,
        basket_weights: Map<CurrencyCode, i128>,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(OracleError::AlreadyInitialized);
        }

        if !((1..=validators.len()).contains(&min_signatures)) {
            env.panic_with_error(OracleError::InvalidMinSignatures);
        }
        if min_signatures == 0 {
            env.panic_with_error(OracleError::MinSignaturesZero);
        }
        if validators.len() > MAX_VALIDATORS {
            panic!("Too many validators: maximum allowed is {}", MAX_VALIDATORS);
        }

        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.validators, &validators);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_signatures, &min_signatures);
        env.storage()
            .instance()
            .set(&DATA_KEY.currencies, &currencies);
        env.storage()
            .instance()
            .set(&DATA_KEY.basket_weights, &basket_weights);

        let s_tokens_empty: Map<CurrencyCode, Address> = Map::new(&env);
        env.storage()
            .instance()
            .set(&DATA_KEY.s_tokens, &s_tokens_empty);
        env.storage()
            .instance()
            .set(&DATA_KEY.update_interval, &UPDATE_INTERVAL_SECONDS);

        let rates: Map<CurrencyCode, RateData> = Map::new(&env);
        env.storage().instance().set(&DATA_KEY.rates, &rates);
        env.storage().instance().set(&DATA_KEY.last_update, &0u64);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Two-step admin rotation
    // ─────────────────────────────────────────────────────────────────────────

    /// **Step 1 — Initiate transfer (current admin only)**
    ///
    /// Records `new_admin` as the pending successor and starts the timelock.
    /// The current admin retains full authority until `accept_admin` completes.
    /// Calling this again while a transfer is pending *replaces* the previous
    /// nomination (allows correction of a typo'd address before the timelock
    /// expires, provided the current admin's key is still accessible).
    pub fn transfer_admin(env: Env, new_admin: Address) {
        Self::check_admin(&env);

        let eligible_at = env.ledger().timestamp() + ADMIN_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_admin, &new_admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_admin_eligible_at, &eligible_at);

        let current_admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        env.events().publish(
            (symbol_short!("adm_init"),),
            AdminTransferInitiatedEvent {
                current_admin,
                pending_admin: new_admin,
                eligible_at,
            },
        );
    }

    /// **Step 2 — Accept transfer (pending admin only)**
    ///
    /// The nominated address calls this after the timelock has elapsed.
    /// On success the pending state is cleared and the new admin is stored.
    pub fn accept_admin(env: Env) {
        let pending_admin: Address = match env.storage().instance().get(&DATA_KEY.pending_admin) {
            Some(a) => a,
            None => env.panic_with_error(OracleError::NoPendingAdmin),
        };

        // Require signature from the incoming admin
        pending_admin.require_auth();

        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
            .unwrap_or(u64::MAX);

        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(OracleError::AdminTimelockNotElapsed);
        }

        let old_admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();

        // Commit the new admin
        env.storage()
            .instance()
            .set(&DATA_KEY.admin, &pending_admin);

        // Clear pending state
        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);

        env.events().publish(
            (symbol_short!("adm_done"),),
            AdminTransferCompletedEvent {
                old_admin,
                new_admin: pending_admin,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    /// **Cancel a pending transfer (current admin only)**
    ///
    /// Allows the current admin to revoke a pending nomination at any time
    /// before it is accepted, e.g. if the nominated address was incorrect or
    /// the key was later found to be compromised.
    pub fn cancel_admin_transfer(env: Env) {
        Self::check_admin(&env);

        let pending_admin: Address = match env.storage().instance().get(&DATA_KEY.pending_admin) {
            Some(a) => a,
            None => env.panic_with_error(OracleError::NoPendingAdminToCancel),
        };

        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);

        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        env.events().publish(
            (symbol_short!("adm_cncl"),),
            AdminTransferCancelledEvent {
                admin,
                cancelled_pending: pending_admin,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    /// Read current admin address (public)
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DATA_KEY.admin).unwrap()
    }

    /// Read pending admin address, if any (public — for monitoring)
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DATA_KEY.pending_admin)
    }

    /// Ledger timestamp at which the pending admin may call `accept_admin`
    pub fn get_pending_admin_eligible_at(env: Env) -> Option<u64> {
        env.storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rate management (unchanged)
    // ─────────────────────────────────────────────────────────────────────────

    pub fn update_rate(
        env: Env,
        validator: Address,
        currency: CurrencyCode,
        rate: i128,
        sources: Vec<i128>,
        _timestamp: u64,
    ) {
        validator.require_auth();

        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();
        let mut is_validator = false;
        for v in validators.iter() {
            if v == validator {
                is_validator = true;
                break;
            }
        }
        if !is_validator {
            env.panic_with_error(OracleError::UnauthorizedValidator);
        }

        let update_interval: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.update_interval)
            .unwrap_or(UPDATE_INTERVAL_SECONDS);
        let current_time = env.ledger().timestamp();

        let existing_rate = Self::get_rate_internal(&env, &currency);
        let mut allow_update = false;
        if let Some(existing_rate) = existing_rate.clone() {
            let deviation = calculate_deviation(rate, existing_rate.rate_usd);
            if deviation > EMERGENCY_THRESHOLD_BPS {
                allow_update = true;
            }
        }

        if let Some(existing_rate) = existing_rate {
            if !allow_update && current_time < existing_rate.timestamp + update_interval {
                env.panic_with_error(OracleError::UpdateIntervalNotMet);
            }
        }

        if sources.len() > 0 && sources.len() < MIN_ORACLE_SOURCE_FEEDS {
            env.panic_with_error(OracleError::InsufficientOracleSources);
        }

        // Pass 1: compute reference median from all sources to establish a baseline.
        let raw_median = median(sources.clone()).unwrap_or(rate);

        // Pass 2: reject sources that deviate beyond OUTLIER_THRESHOLD_BPS and emit alert events.
        // Outliers are quarantined so they cannot influence the final stored rate.
        //
        // NOTE: Some Stellar CLI / RPC stacks are sensitive to complex contracttype values in
        // event topics; keep oracle rate updates functional even if event topic conversion would
        // otherwise fail. We still compute deviation, but we avoid publishing per-currency topics.
        let mut clean_sources: Vec<i128> = Vec::new(&env);
        for i in 0..sources.len() {
            let source_rate = sources.get(i).unwrap();
            let deviation_bps = calculate_deviation(source_rate, raw_median);

            if deviation_bps > OUTLIER_THRESHOLD_BPS {
                let outlier_event = OutlierDetectionEvent {
                    currency: currency.clone(),
                    median_rate: raw_median,
                    outlier_rate: source_rate,
                    deviation_bps,
                    timestamp: current_time,
                };
                env.events()
                    .publish((symbol_short!("outlier"),), outlier_event);
            } else {
                clean_sources.push_back(source_rate);
            }
        }

        // Final rate: median of clean sources only.
        // If every source was an outlier (extreme disagreement), fall back to raw_median so the
        // update is never silently dropped; this edge case should be investigated off-chain.
        let median_rate = if clean_sources.is_empty() {
            raw_median
        } else {
            median(clean_sources).unwrap_or(raw_median)
        };

        // Create rate data (original sources retained for audit trail).
        let rate_data = RateData {
            currency: currency.clone(),
            rate_usd: median_rate,
            timestamp: current_time,
            sources,
            ledger: env.ledger().sequence(),
        };

        let mut rates: Map<CurrencyCode, RateData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.rates)
            .unwrap_or(Map::new(&env));
        rates.set(currency.clone(), rate_data);
        env.storage().instance().set(&DATA_KEY.rates, &rates);
        env.storage()
            .instance()
            .set(&DATA_KEY.last_update, &current_time);

        let event = RateUpdateEvent {
            currency: currency.clone(),
            rate: median_rate,
            timestamp: current_time,
            validator: validator.clone(),
        };
        env.events().publish((symbol_short!("rate_upd"),), event);
    }

    pub fn set_rate_admin(env: Env, currency: CurrencyCode, rate: i128) {
        Self::check_admin(&env);
        if rate <= 0 {
            env.panic_with_error(OracleError::InvalidRate);
        }
        let current_time = env.ledger().timestamp();
        let rate_data = RateData {
            currency: currency.clone(),
            rate_usd: rate,
            timestamp: current_time,
            sources: Vec::new(&env),
            // Admin override: stamp with current ledger so the rate is immediately fresh.
            ledger: env.ledger().sequence(),
        };
        let mut rates: Map<CurrencyCode, RateData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.rates)
            .unwrap_or(Map::new(&env));
        rates.set(currency, rate_data);
        env.storage().instance().set(&DATA_KEY.rates, &rates);
        env.storage()
            .instance()
            .set(&DATA_KEY.last_update, &current_time);
    }

    pub fn get_rate(env: Env, currency: CurrencyCode) -> i128 {
        if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
            Self::assert_rate_fresh(&env, &rate_data, &currency);
            rate_data.rate_usd
        } else {
            env.panic_with_error(OracleError::RateNotFound);
        }
    }

    /// Get rate data with timestamp for staleness validation
    pub fn get_rate_with_timestamp(env: Env, currency: CurrencyCode) -> (i128, u64) {
        if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
            Self::assert_rate_fresh(&env, &rate_data, &currency);
            (rate_data.rate_usd, rate_data.timestamp)
        } else {
            env.panic_with_error(OracleError::RateNotFound);
        }
    }

    /// Get ACBU/USD rate with timestamp
    pub fn get_acbu_usd_rate_with_timestamp(env: Env) -> (i128, u64) {
        let basket_weights: Map<CurrencyCode, i128> = env
            .storage()
            .instance()
            .get(&DATA_KEY.basket_weights)
            .unwrap_or(Map::new(&env));
        let currencies: Vec<CurrencyCode> = env
            .storage()
            .instance()
            .get(&DATA_KEY.currencies)
            .unwrap_or(Vec::new(&env));

        let mut weighted_sum = 0i128;
        let mut total_weight = 0i128;
        let mut oldest_timestamp = u64::MAX;

        for currency in currencies.iter() {
            if let Some(weight) = basket_weights.get(currency.clone()) {
                if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
                    // Enforce staleness: a stale basket component blocks the whole basket rate.
                    Self::assert_rate_fresh(&env, &rate_data, &currency);
                    let contribution = (rate_data.rate_usd * weight) / BASIS_POINTS;
                    weighted_sum += contribution;
                    total_weight += weight;
                    if rate_data.timestamp < oldest_timestamp {
                        oldest_timestamp = rate_data.timestamp;
                    }
                }
            }
        }

        let rate = if total_weight > 0 {
            weighted_sum / total_weight
        } else {
            DECIMALS // Neutral rate if no weights
        };

        (
            rate,
            if oldest_timestamp == u64::MAX {
                0
            } else {
                oldest_timestamp
            },
        )
    }

    /// Get ACBU/USD rate (basket-weighted)
    pub fn get_acbu_usd_rate(env: Env) -> i128 {
        let basket_weights: Map<CurrencyCode, i128> = env
            .storage()
            .instance()
            .get(&DATA_KEY.basket_weights)
            .unwrap_or(Map::new(&env));
        let currencies: Vec<CurrencyCode> = env
            .storage()
            .instance()
            .get(&DATA_KEY.currencies)
            .unwrap_or(Vec::new(&env));

        let mut weighted_sum = 0i128;
        let mut total_weight = 0i128;

        for currency in currencies.iter() {
            if let Some(weight) = basket_weights.get(currency.clone()) {
                if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
                    // Enforce staleness: a stale basket component blocks the whole basket rate.
                    Self::assert_rate_fresh(&env, &rate_data, &currency);
                    let contribution = (rate_data.rate_usd * weight) / 10_000;
                    weighted_sum += contribution;
                    total_weight += weight;
                }
            }
        }

        if total_weight == 0 {
            return DECIMALS;
        }

        (weighted_sum * 10_000) / total_weight
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Basket / token config (unchanged)
    // ─────────────────────────────────────────────────────────────────────────

    pub fn get_currencies(env: Env) -> Vec<CurrencyCode> {
        env.storage()
            .instance()
            .get(&DATA_KEY.currencies)
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_basket_weight(env: Env, currency: CurrencyCode) -> i128 {
        let basket_weights: Map<CurrencyCode, i128> = env
            .storage()
            .instance()
            .get(&DATA_KEY.basket_weights)
            .unwrap_or(Map::new(&env));
        basket_weights.get(currency).unwrap_or(0)
    }

    pub fn set_basket_config(
        env: Env,
        currencies: Vec<CurrencyCode>,
        basket_weights: Map<CurrencyCode, i128>,
    ) {
        Self::check_admin(&env);
        env.storage()
            .instance()
            .set(&DATA_KEY.currencies, &currencies);
        env.storage()
            .instance()
            .set(&DATA_KEY.basket_weights, &basket_weights);
    }

    pub fn set_s_token_address(env: Env, currency: CurrencyCode, token_address: Address) {
        Self::check_admin(&env);
        let mut m: Map<CurrencyCode, Address> = env
            .storage()
            .instance()
            .get(&DATA_KEY.s_tokens)
            .unwrap_or(Map::new(&env));
        m.set(currency, token_address);
        env.storage().instance().set(&DATA_KEY.s_tokens, &m);
    }

    pub fn get_s_token_address(env: Env, currency: CurrencyCode) -> Address {
        let m: Map<CurrencyCode, Address> = env
            .storage()
            .instance()
            .get(&DATA_KEY.s_tokens)
            .unwrap_or(Map::new(&env));
        if let Some(addr) = m.get(currency.clone()) {
            addr
        } else {
            env.panic_with_error(OracleError::STokenNotConfigured);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Validator management (unchanged)
    // ─────────────────────────────────────────────────────────────────────────

    pub fn add_validator(env: Env, validator: Address) {
        Self::check_admin(&env);
        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();
        for v in validators.iter() {
            if v == validator {
                env.panic_with_error(OracleError::ValidatorAlreadyExists);
            }
        }
        if validators.len() >= MAX_VALIDATORS {
            panic!(
                "Cannot add validator: maximum number of validators ({}) reached",
                MAX_VALIDATORS
            );
        }
        let mut new_validators = validators.clone();
        new_validators.push_back(validator);
        env.storage()
            .instance()
            .set(&DATA_KEY.validators, &new_validators);
    }

    pub fn remove_validator(env: Env, validator: Address) {
        Self::check_admin(&env);
        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();
        let min_sigs: u32 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_signatures)
            .unwrap();
        if validators.len() <= min_sigs {
            env.panic_with_error(OracleError::CannotRemoveValidator);
        }
        let mut new_validators = Vec::new(&env);
        for v in validators.iter() {
            if v != validator {
                new_validators.push_back(v.clone());
            }
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.validators, &new_validators);
    }

    pub fn get_validators(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DATA_KEY.validators).unwrap()
    }

    pub fn get_min_signatures(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DATA_KEY.min_signatures)
            .unwrap()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Upgrade / migration
    // ─────────────────────────────────────────────────────────────────────────

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn migrate(env: Env) {
        Self::check_admin(&env);
        let current_version = VERSION;
        let stored_version: u32 = env.storage().instance().get(&DATA_KEY.version).unwrap_or(0);
        if stored_version < current_version {
            if stored_version < 2 {
                let s_tokens_empty: Map<CurrencyCode, Address> = Map::new(&env);
                env.storage()
                    .instance()
                    .set(&DATA_KEY.s_tokens, &s_tokens_empty);
            }
            if stored_version < 3 {
                let rates_empty: Map<CurrencyCode, RateData> = Map::new(&env);
                env.storage().instance().set(&DATA_KEY.rates, &rates_empty);
                env.storage().instance().set(&DATA_KEY.last_update, &0u64);
            }
            if stored_version < 6 {
                let currencies_empty: Vec<CurrencyCode> = Vec::new(&env);
                let basket_weights_empty: Map<CurrencyCode, i128> = Map::new(&env);
                env.storage()
                    .instance()
                    .set(&DATA_KEY.currencies, &currencies_empty);
                env.storage()
                    .instance()
                    .set(&DATA_KEY.basket_weights, &basket_weights_empty);

                let rates_empty: Map<CurrencyCode, RateData> = Map::new(&env);
                env.storage().instance().set(&DATA_KEY.rates, &rates_empty);
                env.storage().instance().set(&DATA_KEY.last_update, &0u64);

                let s_tokens_empty: Map<CurrencyCode, Address> = Map::new(&env);
                env.storage()
                    .instance()
                    .set(&DATA_KEY.s_tokens, &s_tokens_empty);
            }
            // v9 migration: no data backfill needed — pending_admin keys
            // simply don't exist on upgraded contracts until a transfer is initiated.
            env.storage()
                .instance()
                .set(&DATA_KEY.version, &current_version);
        }
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();

        let current_version = Self::get_version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(OracleError::InvalidVersion);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Run migrations
        for v in current_version..new_version {
            match v {
                0 => migrate_v0_to_v1(env.clone()),
                _ => {}
            }
        }

        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &new_version);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Private helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn get_rate_internal(env: &Env, currency: &CurrencyCode) -> Option<RateData> {
        let rates: Map<CurrencyCode, RateData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.rates)
            .unwrap_or(Map::new(env));
        rates.get(currency.clone())
    }

    /// Panics if the stored rate is older than `STALE_RATE_MAX_LEDGERS` ledgers.
    ///
    /// Using ledger sequence (not timestamp) as the staleness signal is intentional:
    /// ledger sequence is set by the network and cannot be forged by a validator.
    /// This is the oracle-side enforcement; the minting contract adds a second,
    /// timestamp-based layer via `check_oracle_freshness`.
    ///
    /// Admin override path: call `set_rate_admin` to refresh the stored rate with
    /// the current ledger sequence, which immediately unblocks reads.
    fn assert_rate_fresh(env: &Env, rate_data: &RateData, currency: &CurrencyCode) {
        let current_ledger = env.ledger().sequence();
        let age = current_ledger.saturating_sub(rate_data.ledger);
        if age > STALE_RATE_MAX_LEDGERS {
            // Emit an observable event before panicking so monitoring bots can alert.
            env.events().publish(
                (symbol_short!("stale_rt"),),
                StaleRateEvent {
                    currency: currency.clone(),
                    stored_ledger: rate_data.ledger,
                    current_ledger,
                    max_stale_ledgers: STALE_RATE_MAX_LEDGERS,
                },
            );
            env.panic_with_error(OracleError::RateStaleLedger);
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}
mod tests;
fn migrate_v0_to_v1(_env: Env) {}
