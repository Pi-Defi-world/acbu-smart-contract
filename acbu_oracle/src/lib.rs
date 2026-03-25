#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Map, Symbol, Vec,
};


use shared::{
    calculate_deviation, median, CurrencyCode, OutlierDetectionEvent, RateData, RateUpdateEvent,
    EMERGENCY_THRESHOLD_BPS, OUTLIER_THRESHOLD_BPS, UPDATE_INTERVAL_SECONDS,
};

mod shared {
    pub use shared::*;
}

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
};

const VERSION: u32 = 1;


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
    /// Initialize the oracle contract
    pub fn initialize(
        env: Env,
        admin: Address,
        validators: Vec<Address>,
        min_signatures: u32,
        currencies: Vec<CurrencyCode>,
        basket_weights: Map<CurrencyCode, i128>,
    ) {
        // Check if already initialized
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }

        // Validate inputs
        if !((1..=validators.len()).contains(&min_signatures)) {
            panic!("Invalid min_signatures configuration");
        }

        if min_signatures == 0 {
            panic!("Minimum signatures must be > 0");
        }

        // Store configuration
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
        env.storage()
            .instance()
            .set(&DATA_KEY.update_interval, &UPDATE_INTERVAL_SECONDS);

        // Initialize rates map
        let rates: Map<CurrencyCode, RateData> = Map::new(&env);
        env.storage().instance().set(&DATA_KEY.rates, &rates);
        env.storage().instance().set(&DATA_KEY.last_update, &0u64);
    }

    /// Update rate for a currency (validator function)
    pub fn update_rate(
        env: Env,
        validator: Address,
        currency: CurrencyCode,
        rate: i128,
        sources: Vec<i128>,
        _timestamp: u64,
    ) {
        // Authorize validator
        validator.require_auth();

        // Check if caller is a validator
        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();
        let mut is_validator = false;
        for v in validators.iter() {
            if v == validator {
                is_validator = true;
                break;
            }
        }
        if !is_validator {
            panic!("Unauthorized: validator only");
        }

        // Check update interval
        let last_update: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.last_update)
            .unwrap_or(0);
        let update_interval: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.update_interval)
            .unwrap_or(UPDATE_INTERVAL_SECONDS);
        let current_time = env.ledger().timestamp();

        // Allow emergency updates if rate moved >5%
        let mut allow_update = false;
        if let Some(existing_rate) = Self::get_rate_internal(&env, &currency) {
            let deviation = calculate_deviation(rate, existing_rate.rate_usd);
            if deviation > EMERGENCY_THRESHOLD_BPS {
                allow_update = true; // Emergency update
            }
        }

        if !allow_update && current_time < last_update + update_interval {
            panic!("Update interval not met");
        }

        // Calculate median from sources
        let median_rate = median(sources.clone()).unwrap_or(rate);

        // Detect outliers in the source rates
        for i in 0..sources.len() {
            let source_rate = sources.get(i).unwrap();
            let deviation_bps = calculate_deviation(source_rate, median_rate);
            
            if deviation_bps > OUTLIER_THRESHOLD_BPS {
                // Emit outlier detection event
                let outlier_event = OutlierDetectionEvent {
                    currency: currency.clone(),
                    median_rate,
                    outlier_rate: source_rate,
                    deviation_bps,
                    timestamp: current_time,
                };
                env.events()
                    .publish((symbol_short!("outlier"), currency.clone()), outlier_event);
            }
        }

        // Create rate data
        let rate_data = RateData {
            currency: currency.clone(),
            rate_usd: median_rate,
            timestamp: current_time,
            sources,
        };

        // Update rates map
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

        // Emit RateUpdateEvent
        let event = RateUpdateEvent {
            currency: currency.clone(),
            rate: median_rate,
            timestamp: current_time,
            validators,
        };
        env.events()
            .publish((symbol_short!("rate_upd"), currency.clone()), event);
    }

    /// Get current rate for a currency
    pub fn get_rate(env: Env, currency: CurrencyCode) -> i128 {
        if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
            rate_data.rate_usd
        } else {
            panic!("Rate not found for currency");
        }
    }

    /// Get ACBU/USD rate (basket-weighted)
    pub fn get_acbu_usd_rate(env: Env) -> i128 {
        let basket_weights: Map<CurrencyCode, i128> = env
            .storage()
            .instance()
            .get(&DATA_KEY.basket_weights)
            .unwrap();
        let currencies: Vec<CurrencyCode> =
            env.storage().instance().get(&DATA_KEY.currencies).unwrap();

        let mut weighted_sum = 0i128;
        let mut total_weight = 0i128;

        for currency in currencies.iter() {
            if let Some(weight) = basket_weights.get(currency.clone()) {
                if let Some(rate_data) = Self::get_rate_internal(&env, &currency) {
                    // Weight is in basis points (e.g., 1800 = 18%)
                    let contribution = (rate_data.rate_usd * weight) / 10_000;
                    weighted_sum += contribution;
                    total_weight += weight;
                }
            }
        }

        if total_weight == 0 {
            panic!("No valid rates available");
        }

        // Normalize to ensure weights sum to 100%
        (weighted_sum * 10_000) / total_weight
    }

    /// Add validator (admin only)
    pub fn add_validator(env: Env, validator: Address) {
        Self::check_admin(&env);
        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();

        // Check if already exists
        for v in validators.iter() {
            if v == validator {
                panic!("Validator already exists");
            }
        }

        let mut new_validators = validators.clone();
        new_validators.push_back(validator);
        env.storage()
            .instance()
            .set(&DATA_KEY.validators, &new_validators);
    }

    /// Remove validator (admin only)
    pub fn remove_validator(env: Env, validator: Address) {
        Self::check_admin(&env);
        let validators: Vec<Address> = env.storage().instance().get(&DATA_KEY.validators).unwrap();
        let min_sigs: u32 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_signatures)
            .unwrap();

        // Can't remove if it would make validators < min_signatures
        if validators.len() <= min_sigs {
            panic!("Cannot remove validator: would violate minimum signatures");
        }

        // Remove validator
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

    /// Get all validators
    pub fn get_validators(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DATA_KEY.validators).unwrap()
    }

    /// Get minimum signatures required
    pub fn get_min_signatures(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DATA_KEY.min_signatures)
            .unwrap()
    }

    // Private helper functions
    fn get_rate_internal(env: &Env, currency: &CurrencyCode) -> Option<RateData> {
        let rates: Map<CurrencyCode, RateData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.rates)
            .unwrap_or(Map::new(env));
        rates.get(currency.clone())
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }

    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

