#![no_std]
use core::fmt::{self, Display};
use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, Address,
    BytesN, Env, Map, Symbol, Vec,
};

use shared::{
    calculate_deviation, median, CurrencyCode, DataKey as SharedDataKey, OutlierDetectionEvent,
    RateData, RateUpdateEvent, BASIS_POINTS, CONTRACT_VERSION, DECIMALS, EMERGENCY_THRESHOLD_BPS,
    MAX_VALIDATORS, OUTLIER_THRESHOLD_BPS, STALE_RATE_MAX_LEDGERS, UPDATE_INTERVAL_SECONDS,
};

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
    NoPendingUpgrade = 7017,
    UpgradeTimelockNotElapsed = 7018,
    NoPendingValidatorChange = 7019,
    ValidatorTimelockNotElapsed = 7020,
    MaxValidatorsReached = 7021,
    TimestampRollback = 7022,
    RateNotInitialized = 7023,
    Unknown = 7999,
}

impl Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AlreadyInitialized => "oracle already initialized",
            Self::InvalidMinSignatures => "invalid minimum signatures",
            Self::MinSignaturesZero => "minimum signatures cannot be zero",
            Self::NoPendingAdmin => "no pending admin",
            Self::AdminTimelockNotElapsed => "admin timelock has not elapsed",
            Self::NoPendingAdminToCancel => "no pending admin to cancel",
            Self::UnauthorizedValidator => "unauthorized validator",
            Self::UpdateIntervalNotMet => "update interval not met",
            Self::InsufficientOracleSources => "insufficient oracle sources",
            Self::InvalidRate => "invalid rate",
            Self::RateNotFound => "rate not found",
            Self::STokenNotConfigured => "s-token not configured",
            Self::ValidatorAlreadyExists => "validator already exists",
            Self::CannotRemoveValidator => "cannot remove validator",
            Self::InvalidVersion => "invalid contract version",
            Self::RateStaleLedger => "rate is stale",
            Self::NoPendingUpgrade => "no pending upgrade",
            Self::UpgradeTimelockNotElapsed => "upgrade timelock has not elapsed",
            Self::NoPendingValidatorChange => "no pending validator change",
            Self::ValidatorTimelockNotElapsed => "validator timelock has not elapsed",
            Self::MaxValidatorsReached => "maximum validators reached",
            Self::TimestampRollback => "timestamp rollback",
            Self::RateNotInitialized => "rate not initialized - no submissions yet",
            Self::Unknown => "unknown oracle error",
        };
        f.write_str(message)
    }
}

const ADMIN_TIMELOCK_SECONDS: u64 = 86_400;
const MIN_ORACLE_SOURCE_FEEDS: u32 = 3;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub validators: Symbol,
    pub validator_set: Symbol,
    pub min_signatures: Symbol,
    pub currencies: Symbol,
    pub rates: Symbol,
    pub last_update: Symbol,
    pub update_interval: Symbol,
    pub basket_weights: Symbol,
    pub s_tokens: Symbol,
    pub version: Symbol,
    pub pending_admin: Symbol,
    pub pending_admin_eligible_at: Symbol,
    pub pending_upgrade_wasm: Symbol,
    pub pending_upgrade_version: Symbol,
    pub pending_upgrade_eligible_at: Symbol,
    pub pending_validator: Symbol,
    pub pending_validator_is_add: Symbol,
    pub pending_validator_eligible_at: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    validators: symbol_short!("VALIDTRS"),
    validator_set: symbol_short!("VAL_SET"),
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
    pending_upgrade_wasm: symbol_short!("PEND_UPG"),
    pending_upgrade_version: symbol_short!("PU_VER"),
    pending_upgrade_eligible_at: symbol_short!("PU_ETA"),
    pending_validator: symbol_short!("PEND_VAL"),
    pending_validator_is_add: symbol_short!("PV_ADD"),
    pending_validator_eligible_at: symbol_short!("PV_ETA"),
};

const VERSION: u32 = 9;

#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferInitiatedEvent {
    pub current_admin: Address,
    pub pending_admin: Address,
    pub eligible_at: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferCompletedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct AdminTransferCancelledEvent {
    pub admin: Address,
    pub cancelled_pending: Address,
    pub timestamp: u64,
}

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

contractmeta!(key = "version", val = "9");

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
            env.panic_with_error(OracleError::MaxValidatorsReached);
        }

        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.validators, &validators);
        let mut validator_set: Map<Address, bool> = Map::new(&env);
        for v in validators.iter() {
            validator_set.set(v, true);
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.validator_set, &validator_set);
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

    pub fn accept_admin(env: Env) {
        let pending_admin: Address = match env.storage().instance().get(&DATA_KEY.pending_admin) {
            Some(a) => a,
            None => env.panic_with_error(OracleError::NoPendingAdmin),
        };

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

        env.storage()
            .instance()
            .set(&DATA_KEY.admin, &pending_admin);
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

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DATA_KEY.admin).unwrap()
    }

    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DATA_KEY.pending_admin)
    }

    pub fn get_pending_admin_eligible_at(env: Env) -> Option<u64> {
        env.storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rate management
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

        let validator_set: Map<Address, bool> =
            match env.storage().instance().get(&DATA_KEY.validator_set) {
                Some(set) => set,
                None => {
                    let validators: Vec<Address> =
                        env.storage().instance().get(&DATA_KEY.validators).unwrap();
                    let mut set: Map<Address, bool> = Map::new(&env);
                    for v in validators.iter() {
                        set.set(v, true);
                    }
                    env.storage().instance().set(&DATA_KEY.validator_set, &set);
                    set
                }
            };
        if !validator_set.contains_key(validator.clone()) {
            env.panic_with_error(OracleError::UnauthorizedValidator);
        }

        let update_interval: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.update_interval)
            .unwrap_or(UPDATE_INTERVAL_SECONDS);
        let current_time = env.ledger().timestamp();

        let existing_rate = Self::get_rate_internal(&env, &currency);
        if let Some(ref existing) = existing_rate {
            if current_time < existing.timestamp {
                env.panic_with_error(OracleError::TimestampRollback);
            }
        }
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

        // Bypass median and outlier calculation workflows if 0 or 1 submissions exist
        let median_rate = if sources.is_empty() {
            rate
        } else if sources.len() == 1 {
            sources.get(0).unwrap()
        } else {
            let raw_median = median(sources.clone()).unwrap_or(rate);

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

            if clean_sources.is_empty() {
                raw_median
            } else if clean_sources.len() == 1 {
                clean_sources.get(0).unwrap()
            } else {
                median(clean_sources).unwrap_or(raw_median)
            }
        };

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

        let existing_rate = Self::get_rate_internal(&env, &currency);
        if let Some(ref existing) = existing_rate {
            if current_time < existing.timestamp {
                env.panic_with_error(OracleError::TimestampRollback);
            }
        }
        let rate_data = RateData {
            currency: currency.clone(),
            rate_usd: rate,
            timestamp: current_time,
            sources: Vec::new(&env),
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

        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        let event = RateUpdateEvent {
            currency,
            rate,
            timestamp: current_time,
            validator: admin,
        };
        env.events().publish((symbol_short!("rate_upd"),), event);
    }

    pub fn get_rate(env: Env, currency: CurrencyCode) -> i128 {
        Self::assert_currency_registered(&env, &currency);
        if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
            Self::assert_rate_fresh(&env, &rate_data, &currency);
            rate_data.rate_usd
        } else {
            env.panic_with_error(OracleError::RateNotInitialized);
        }
    }

    pub fn get_rate_with_timestamp(env: Env, currency: CurrencyCode) -> (i128, u64) {
        Self::assert_currency_registered(&env, &currency);
        if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
            Self::assert_rate_fresh(&env, &rate_data, &currency);
            (rate_data.rate_usd, rate_data.timestamp)
        } else {
            env.panic_with_error(OracleError::RateNotInitialized);
        }
    }

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
        if currencies.is_empty() {
            env.panic_with_error(OracleError::RateNotInitialized);
        }

        let mut weighted_sum = 0i128;
        let mut total_weight = 0i128;
        let mut oldest_timestamp = u64::MAX;

        for currency in currencies.iter() {
            if let Some(weight) = basket_weights.get(currency.clone()) {
                if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
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

        if total_weight == 0 {
            env.panic_with_error(OracleError::RateNotInitialized);
        }

        let rate = weighted_sum / total_weight;

        (
            rate,
            if oldest_timestamp == u64::MAX {
                0
            } else {
                oldest_timestamp
            },
        )
    }

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
        if currencies.is_empty() {
            env.panic_with_error(OracleError::RateNotInitialized);
        }

        let mut weighted_sum = 0i128;
        let mut total_weight = 0i128;

        for currency in currencies.iter() {
            if let Some(weight) = basket_weights.get(currency.clone()) {
                if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
                    Self::assert_rate_fresh(&env, &rate_data, &currency);
                    let contribution = (rate_data.rate_usd * weight) / 10_000;
                    weighted_sum += contribution;
                    total_weight += weight;
                }
            }
        }

        if total_weight == 0 {
            env.panic_with_error(OracleError::RateNotInitialized);
        }

        (weighted_sum * 10_000) / total_weight
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Basket / token config
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
    // Validator management
    // ─────────────────────────────────────────────────────────────────────────

    pub fn schedule_validator_change(env: Env, validator: Address, add: bool) {
        Self::check_admin(&env);
        let eligible_at = env.ledger().timestamp() + ADMIN_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_validator, &validator);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_validator_is_add, &add);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_validator_eligible_at, &eligible_at);
    }

    pub fn execute_validator_change(env: Env) {
        Self::check_admin(&env);
        let validator: Address = match env.storage().instance().get(&DATA_KEY.pending_validator) {
            Some(v) => v,
            None => env.panic_with_error(OracleError::NoPendingValidatorChange),
        };
        let is_add: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_validator_is_add)
            .unwrap_or(false);
        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_validator_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(OracleError::ValidatorTimelockNotElapsed);
        }
        env.storage().instance().remove(&DATA_KEY.pending_validator);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_validator_is_add);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_validator_eligible_at);

        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();
        if is_add {
            for v in validators.iter() {
                if v == validator {
                    env.panic_with_error(OracleError::ValidatorAlreadyExists);
                }
            }
            if validators.len() >= MAX_VALIDATORS {
                env.panic_with_error(OracleError::MaxValidatorsReached);
            }
            let mut new_validators = validators.clone();
            new_validators.push_back(validator.clone());
            env.storage()
                .instance()
                .set(&DATA_KEY.validators, &new_validators);
            Self::index_validator(&env, &validator, true);
        } else {
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
            Self::index_validator(&env, &validator, false);

            // FIX #342: clear all stored rates so no submission from the
            // removed validator can persist into subsequent reads.
            let empty_rates: Map<CurrencyCode, RateData> = Map::new(&env);
            env.storage().instance().set(&DATA_KEY.rates, &empty_rates);
            env.storage().instance().set(&DATA_KEY.last_update, &0u64);
        }
    }

    fn index_validator(env: &Env, validator: &Address, add: bool) {
        let mut validator_set: Map<Address, bool> = env
            .storage()
            .instance()
            .get(&DATA_KEY.validator_set)
            .unwrap_or_else(|| Map::new(env));
        if add {
            validator_set.set(validator.clone(), true);
        } else {
            validator_set.remove(validator.clone());
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.validator_set, &validator_set);
    }

    pub fn cancel_validator_change(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().remove(&DATA_KEY.pending_validator);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_validator_is_add);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_validator_eligible_at);
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

    pub fn get_rate_decimals(env: Env) -> u32 {
        7
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
            env.storage()
                .instance()
                .set(&DATA_KEY.version, &current_version);
        }
    }

    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);
        let current_version = Self::get_version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(OracleError::InvalidVersion);
        }
        let eligible_at = env.ledger().timestamp() + ADMIN_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_wasm, &new_wasm_hash);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_version, &new_version);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_eligible_at, &eligible_at);
    }

    pub fn execute_upgrade(env: Env) {
        Self::check_admin(&env);
        let wasm_hash: BytesN<32> =
            match env.storage().instance().get(&DATA_KEY.pending_upgrade_wasm) {
                Some(h) => h,
                None => env.panic_with_error(OracleError::NoPendingUpgrade),
            };
        let new_version: u32 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_version)
            .unwrap_or_else(|| env.panic_with_error(OracleError::NoPendingUpgrade));
        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(OracleError::UpgradeTimelockNotElapsed);
        }
        let current_version = Self::get_version(env.clone());
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_version);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
        env.deployer().update_current_contract_wasm(wasm_hash);
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

    pub fn cancel_upgrade(env: Env) {
        Self::check_admin(&env);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_version);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
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

    fn assert_rate_fresh(env: &Env, rate_data: &RateData, currency: &CurrencyCode) {
        let current_ledger = env.ledger().sequence();
        let age = current_ledger.saturating_sub(rate_data.ledger);
        if age > STALE_RATE_MAX_LEDGERS {
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

    fn assert_currency_registered(env: &Env, currency: &CurrencyCode) {
        let currencies: Vec<CurrencyCode> = env
            .storage()
            .instance()
            .get(&DATA_KEY.currencies)
            .unwrap_or(Vec::new(env));
        if currencies.is_empty() {
            env.panic_with_error(OracleError::CurrencyNotRegistered);
        }
        if !currencies.contains(currency.clone()) {
            env.panic_with_error(OracleError::CurrencyNotRegistered);
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}
mod tests;
fn migrate_v0_to_v1(_env: Env) {}
