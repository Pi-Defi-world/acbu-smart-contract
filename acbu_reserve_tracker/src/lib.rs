#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Map, Symbol};


use shared::{CurrencyCode, ReserveData};

mod shared {
    pub use shared::*;
}

#[allow(dead_code)]
pub mod token_contract {
    soroban_sdk::contractimport!(
        file = "../soroban_token_contract.wasm",
        sha256 = "6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d"
    );
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub oracle: Symbol,
    pub reserves: Symbol,
    pub min_reserve_ratio: Symbol,
    pub version: Symbol,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ReserveUpdateEvent {
    pub currency: CurrencyCode,
    pub amount: i128,
    pub value_usd: i128,
    pub timestamp: u64,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserves: symbol_short!("RESERVES"),
    min_reserve_ratio: symbol_short!("MIN_RES"),
    version: symbol_short!("VERSION"),
};

const VERSION: u32 = 1;


#[contract]
pub struct ReserveTrackerContract;

#[contractimpl]
impl ReserveTrackerContract {
    /// Initialize the reserve tracker contract
    pub fn initialize(env: Env, admin: Address, oracle: Address, min_reserve_ratio_bps: i128) {
        // Check if already initialized
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }

        // Store configuration
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.oracle, &oracle);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_reserve_ratio, &min_reserve_ratio_bps);

        // Initialize reserves map
        let reserves: Map<CurrencyCode, ReserveData> = Map::new(&env);
        env.storage().instance().set(&DATA_KEY.reserves, &reserves);
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Update reserve amount for a currency (admin or authorized address)
    pub fn update_reserve(
        env: Env,
        _updater: Address,
        currency: CurrencyCode,
        amount: i128,
        value_usd: i128,
    ) {
        // Authorize admin
        Self::check_admin(&env);

        let current_time = env.ledger().timestamp();

        // Update reserves map
        let mut reserves: Map<CurrencyCode, ReserveData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserves)
            .unwrap_or(Map::new(&env));
        let reserve_data = ReserveData {
            currency: currency.clone(),
            amount,
            value_usd,
            timestamp: current_time,
        };

        reserves.set(currency.clone(), reserve_data);
        env.storage().instance().set(&DATA_KEY.reserves, &reserves);

        // Emit Event
        env.events().publish(
            (symbol_short!("reserve"), currency.clone()),
            ReserveUpdateEvent {
                currency,
                amount,
                value_usd,
                timestamp: current_time,
            },
        );
    }

    /// Get current reserves for all currencies
    pub fn get_all_reserves(env: Env) -> Map<CurrencyCode, ReserveData> {
        env.storage()
            .instance()
            .get(&DATA_KEY.reserves)
            .unwrap_or(Map::new(&env))
    }

    /// Get total reserve value in USD
    pub fn get_total_reserve_value(env: Env) -> i128 {
        let reserves: Map<CurrencyCode, ReserveData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserves)
            .unwrap_or(Map::new(&env));
        let mut total_value = 0i128;

        for entry in reserves.iter() {
            let data = entry.1;
            total_value += data.value_usd;
        }

        total_value
    }

    /// Check if reserves meet the minimum ratio (relative to minted ACBU)
    pub fn is_reserve_sufficient(env: Env, total_acbu_supply: i128) -> bool {
        let total_reserve_value = Self::get_total_reserve_value(env.clone());
        let min_ratio: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_reserve_ratio)
            .unwrap_or(10_000);

        // total_reserve_value / total_acbu_supply >= min_ratio / 10,000
        // total_reserve_value * 10,000 >= total_acbu_supply * min_ratio
        total_reserve_value * 10_000 >= total_acbu_supply * min_ratio
    }

    /// Verify reserves meet the minimum collateral ratio for the given circulating ACBU supply.
    ///
    /// `total_acbu_supply` must be total outstanding ACBU in 7-decimal fixed-point units (1 whole
    /// token = 10_000_000), for example from an indexer or summed balances off-chain.
    /// Do not use this contract's own token balance: the reserve tracker does not custody ACBU.
    pub fn verify_reserves(env: Env, total_acbu_supply: i128) -> bool {
        Self::is_reserve_sufficient(env, total_acbu_supply)
    }

    // Private helper functions
    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }

    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    pub fn migrate(env: Env) {
        Self::check_admin(&env);
        let current_version = VERSION;
        let stored_version: u32 = env.storage().instance().get(&DATA_KEY.version).unwrap_or(0);
        if stored_version < current_version {
            env.storage()
                .instance()
                .set(&DATA_KEY.version, &current_version);
        }
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}

