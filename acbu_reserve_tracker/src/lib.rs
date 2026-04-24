#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Map, Symbol, Vec,
};

use shared::{CurrencyCode, DataKey as SharedDataKey, ReserveData, BASIS_POINTS, CONTRACT_VERSION};

mod shared {
    pub use shared::*;
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub oracle: Symbol,
    pub reserves: Symbol,
    pub min_reserve_ratio: Symbol,
    pub acbu_token: Symbol,
    pub version: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserves: symbol_short!("RESERVES"),
    min_reserve_ratio: symbol_short!("MIN_RES"),
    acbu_token: symbol_short!("ACBU_TKN"),
    version: symbol_short!("VERSION"),
};

#[contract]
pub struct ReserveTrackerContract;

#[contractimpl]
impl ReserveTrackerContract {
    /// Initialize the reserve tracker contract
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        acbu_token: Address,
        min_reserve_ratio_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }

        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.oracle, &oracle);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_reserve_ratio, &min_reserve_ratio_bps);

        let reserves: Map<CurrencyCode, ReserveData> = Map::new(&env);
        env.storage().instance().set(&DATA_KEY.reserves, &reserves);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    pub fn get_total_supply_from_token(env: &Env) -> i128 {
        let acbu_token_addr: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        // Use invoke_contract to avoid dependency on a specific token client implementation
        env.invoke_contract(
            &acbu_token_addr,
            &Symbol::new(env, "get_total_supply"),
            Vec::new(env),
        )
    }

    pub fn verify_reserves(env: Env) -> bool {
        let total_acbu_supply = Self::get_total_supply_from_token(&env);
        Self::is_reserve_sufficient(env, total_acbu_supply)
    }

    pub fn verify_reserves_manual(env: Env, total_acbu_supply: i128) -> bool {
        Self::is_reserve_sufficient(env, total_acbu_supply)
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

        reserves.set(currency.clone(), reserve_data.clone());
        env.storage().instance().set(&DATA_KEY.reserves, &reserves);

        env.events()
            .publish((symbol_short!("reserve"), currency.clone()), reserve_data);
    }

    /// Get current reserves for all currencies
    pub fn get_all_reserves(env: Env) -> Map<CurrencyCode, ReserveData> {
        env.storage()
            .instance()
            .get(&DATA_KEY.reserves)
            .unwrap_or(Map::new(&env))
    }

    /// Check if the total reserves are sufficient to back the ACBU supply
    pub fn is_reserve_sufficient(env: Env, total_acbu_supply: i128) -> bool {
        if total_acbu_supply <= 0 {
            return true;
        }

        let reserves = Self::get_all_reserves(env.clone());
        let mut total_reserve_usd = 0i128;

        for (_curr, data) in reserves.iter() {
            total_reserve_usd = total_reserve_usd
                .checked_add(data.value_usd)
                .expect("Overflow in reserve calculation");
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let acbu_usd_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            Vec::new(&env),
        );

        let total_acbu_usd = (total_acbu_supply * acbu_usd_rate) / 100_000_000;
        if total_acbu_usd == 0 {
            return true;
        }

        let min_reserve_ratio = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_reserve_ratio)
            .unwrap_or(10000i128); // Default to 100%

        let current_ratio = (total_reserve_usd * BASIS_POINTS) / total_acbu_usd;
        current_ratio >= min_reserve_ratio
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);

        let current_version = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        if new_version <= current_version {
            panic!("Invalid version upgrade");
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Run migrations
        #[allow(clippy::single_match)]
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

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}

fn migrate_v0_to_v1(_env: Env) {
    // Migration logic
}
