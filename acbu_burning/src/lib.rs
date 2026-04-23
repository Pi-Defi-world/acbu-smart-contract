#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env, IntoVal,
    String as SorobanString, Symbol, Vec,
};

use shared::{calculate_fee, BurnEvent, CurrencyCode, BASIS_POINTS, CONTRACT_VERSION, DECIMALS, DataKey as SharedDataKey, MIN_BURN_AMOUNT};

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
    pub reserve_tracker: Symbol,
    pub acbu_token: Symbol,
    pub withdrawal_processor: Symbol,
    pub vault: Symbol,
    pub fee_rate: Symbol,
    pub fee_single_redeem: Symbol,
    pub paused: Symbol,
    pub min_burn_amount: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserve_tracker: symbol_short!("RES_TRK"),
    acbu_token: symbol_short!("ACBU_TKN"),
    withdrawal_processor: symbol_short!("WD_PROC"),
    vault: symbol_short!("VAULT"),
    fee_rate: symbol_short!("FEE_RATE"),
    fee_single_redeem: symbol_short!("FEE_S_R"),
    paused: symbol_short!("PAUSED"),
    min_burn_amount: symbol_short!("MIN_BURN"),
};

// CONTRACT_VERSION is imported from shared

#[contract]
pub struct BurningContract;

#[contractimpl]
impl BurningContract {
    /// Initialize the burning contract.
    /// `vault` holds Afreum S-tokens (must have approved this contract for `transfer_from`).
    /// `fee_rate_bps` applies to full basket redemption; `fee_single_redeem_bps` to single-currency payout (typically higher).
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        reserve_tracker: Address,
        acbu_token: Address,
        withdrawal_processor: Address,
        vault: Address,
        fee_rate_bps: i128,
        fee_single_redeem_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }

        if !(0..=BASIS_POINTS).contains(&fee_rate_bps)
            || !(0..=BASIS_POINTS).contains(&fee_single_redeem_bps)
        {
            panic!("Invalid fee rate");
        }

        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.oracle, &oracle);
        env.storage()
            .instance()
            .set(&DATA_KEY.reserve_tracker, &reserve_tracker);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.withdrawal_processor, &withdrawal_processor);
        env.storage().instance().set(&DATA_KEY.vault, &vault);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_single_redeem, &fee_single_redeem_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_burn_amount, &MIN_BURN_AMOUNT);
        env.storage().instance().set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    /// Redeem ACBU for a single Afreum S-token (higher fee tier). Requires vault approval.
    pub fn redeem_single(
        env: Env,
        user: Address,
        recipient: Address,
        acbu_amount: i128,
        currency: CurrencyCode,
    ) -> i128 {
        Self::check_paused(&env);
        user.require_auth();

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            panic!("Invalid burn amount");
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let fee_single: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.fee_single_redeem)
            .unwrap();

        let acbu_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            vec![&env],
        );
        let rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_rate"),
            vec![&env, currency.clone().into_val(&env)],
        );
        if rate == 0 {
            panic!("Invalid oracle rate");
        }

        let stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_s_token_address"),
            vec![&env, currency.clone().into_val(&env)],
        );

        let fee = calculate_fee(acbu_amount, fee_single);
        let net_acbu = acbu_amount - fee;
        let usd_out = (net_acbu * acbu_rate) / DECIMALS;
        let stoken_out = (usd_out * DECIMALS) / rate;

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let token = soroban_sdk::token::Client::new(&env, &stoken);
        let spender = env.current_contract_address();
        token.transfer_from(&spender, &vault, &recipient, &stoken_out);

        let tx_id = SorobanString::from_str(&env, "redeem_single");
        let burn_event = BurnEvent {
            transaction_id: tx_id,
            user: user.clone(),
            acbu_amount,
            local_amount: stoken_out,
            currency: currency.clone(),
            fee,
            rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("burn"), user), burn_event);

        stoken_out
    }

    /// Redeem ACBU for proportional Afreum S-tokens across the basket (lower fee tier).
    pub fn redeem_basket(
        env: Env,
        user: Address,
        recipient: Address,
        acbu_amount: i128,
    ) -> Vec<i128> {
        Self::check_paused(&env);
        user.require_auth();

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            panic!("Invalid burn amount");
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();

        let acbu_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            vec![&env],
        );

        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let net_acbu = acbu_amount - total_fee;
        let usd_total = (net_acbu * acbu_rate) / DECIMALS;

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let currencies: Vec<CurrencyCode> = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_currencies"),
            vec![&env],
        );

        let mut amounts_out = Vec::new(&env);
        let mut last_weighted_idx: Option<u32> = None;
        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_basket_weight"),
                vec![&env, currency.into_val(&env)],
            );
            if weight > 0 {
                last_weighted_idx = Some(i);
            }
        }

        let mut usd_allocated = 0i128;
        let mut fee_allocated = 0i128;

        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_basket_weight"),
                vec![&env, currency.clone().into_val(&env)],
            );
            if weight == 0 {
                amounts_out.push_back(0);
                continue;
            }

            let rate: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_rate"),
                vec![&env, currency.clone().into_val(&env)],
            );
            if rate == 0 {
                panic!("Invalid oracle rate");
            }

            let stoken: Address = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_s_token_address"),
                vec![&env, currency.clone().into_val(&env)],
            );

            let (usd_i, fee_i) = if Some(i) == last_weighted_idx {
                (usd_total - usd_allocated, total_fee - fee_allocated)
            } else {
                let usd_i = (weight * usd_total) / BASIS_POINTS;
                let fee_i = (weight * total_fee) / BASIS_POINTS;
                usd_allocated += usd_i;
                fee_allocated += fee_i;
                (usd_i, fee_i)
            };
            let native_i = (usd_i * DECIMALS) / rate;

            if native_i > 0 {
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                let spender = env.current_contract_address();
                token.transfer_from(&spender, &vault, &recipient, &native_i);
            }

            amounts_out.push_back(native_i);

            let tx_id = SorobanString::from_str(&env, "redeem_basket");
            let burn_event = BurnEvent {
                transaction_id: tx_id,
                user: user.clone(),
                acbu_amount: net_acbu,
                local_amount: native_i,
                currency: currency.clone(),
                fee: fee_i,
                rate,
                timestamp: env.ledger().timestamp(),
            };
            env.events()
                .publish((symbol_short!("burn"), user.clone()), burn_event);
        }

        amounts_out
    }

    /// Pause the contract (admin only)
    pub fn pause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.paused, &true);
    }

    /// Unpause the contract (admin only)
    pub fn unpause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    /// Set basket redemption fee (admin only)
    pub fn set_fee_rate(env: Env, fee_rate_bps: i128) {
        Self::check_admin(&env);
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            panic!("Invalid fee rate");
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
    }

    pub fn set_fee_single_redeem(env: Env, fee_bps: i128) {
        Self::check_admin(&env);
        if !(0..=BASIS_POINTS).contains(&fee_bps) {
            panic!("Invalid fee rate");
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_single_redeem, &fee_bps);
    }

    pub fn get_fee_rate(env: Env) -> i128 {
        env.storage().instance().get(&DATA_KEY.fee_rate).unwrap()
    }

    pub fn get_fee_single_redeem(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DATA_KEY.fee_single_redeem)
            .unwrap()
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false)
    }

    fn check_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            panic!("Contract is paused");
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage().instance().get(&SharedDataKey::Version).unwrap_or(0)
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();

        let current_version = Self::get_version(env.clone());
        if new_version <= current_version {
            panic!("Invalid version upgrade");
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Run migrations
        for v in current_version..new_version {
            match v {
                0 => migrate_v0_to_v1(env.clone()),
                _ => {}
            }
        }

        env.storage().instance().set(&SharedDataKey::Version, &new_version);
    }
}

fn migrate_v0_to_v1(_env: Env) {
    // Migration logic
}
