#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env, IntoVal,
    String as SorobanString, Symbol, Vec,
};

use shared::{
    calculate_fee, BurnEvent, ContractError, CurrencyCode, DataKey as SharedDataKey, BASIS_POINTS,
    CONTRACT_VERSION, DECIMALS, MIN_BURN_AMOUNT,
    ORACLE_GET_ACBU_RATE, ORACLE_GET_CURRENCIES, ORACLE_GET_BASKET_WEIGHT,
    ORACLE_GET_RATE, ORACLE_GET_S_TOKEN_ADDR, UPDATE_INTERVAL_SECONDS,
};

mod shared {
    pub use shared::*;
}

/*
#[allow(dead_code)]
pub mod token_contract {
    soroban_sdk::contractimport!(
        file = "../soroban_token_contract.wasm",
        sha256 = "8759e8ea16c858a6d3b743dd0be8b580e363d0097538fb77b375965619288d95"
    );
}
*/

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
    /// `vault` holds Afreum S-tokens. Redemption flows use a pull model:
    /// the vault must approve this contract for `transfer_from` on each S-token.
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
            env.panic_with_error(ContractError::Unauthorized);
        }

        if !(0..=BASIS_POINTS).contains(&fee_rate_bps)
            || !(0..=BASIS_POINTS).contains(&fee_single_redeem_bps)
        {
            env.panic_with_error(ContractError::InvalidRate);
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
        env.storage().instance().set(&SharedDataKey::Version, &CONTRACT_VERSION);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_burn_amount, &MIN_BURN_AMOUNT);
    }

    /// Redeem ACBU for a single Afreum S-token (higher fee tier). Requires vault approval.
    pub fn redeem_single(
        env: Env,
        user: Address,
        recipient: Address,
        acbu_amount: i128,
        currency: CurrencyCode,
    ) -> i128 {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        Self::check_paused(&env);
        user.require_auth();

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            env.panic_with_error(ContractError::InvalidAmount);
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_single: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.fee_single_redeem)
            .unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();

        // C-012: Ensure oracle rates are fresh before burning.
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE),
            vec![&env],
        );
        let current_time = env.ledger().timestamp();
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_RATE),
            vec![&env, currency.clone().into_val(&env)],
        );
        if current_time > rate_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        if rate <= 0 || acbu_rate <= 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        let stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_S_TOKEN_ADDR),
            vec![&env, currency.clone().into_val(&env)],
        );

        let fee = calculate_fee(acbu_amount, fee_single);
        let net_acbu = acbu_amount
            .checked_sub(fee)
            .expect("Underflow in net acbu calculation");

        // C-019: stoken_out = (net_acbu * acbu_rate) / rate
        // The redundant (net_acbu * DECIMALS) / DECIMALS scaling is removed — it cancels out.
        let stoken_out = net_acbu
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(rate))
            .expect("Overflow in stoken out calculation");

        // C-012: Call reserve tracker to verify protocol health before burning.
        let current_supply: i128 = env.invoke_contract(
            &acbu_token,
            &Symbol::new(&env, "get_total_supply"),
            vec![&env],
        );
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, current_supply.into_val(&env)],
        );
        if !reserve_ok {
            env.panic_with_error(ContractError::InsufficientReserves);
        }

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
            net_acbu,
            local_amount: stoken_out,
            currency: currency.clone(),
            fee,
            rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("burn"), user), burn_event);

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

        stoken_out
    }

    /// Redeem ACBU for proportional Afreum S-tokens across the basket (lower fee tier).
    ///
    /// `recipients` must be non-empty and contain no duplicate addresses — one entry per
    /// basket currency (in the same order returned by the oracle's `get_currencies`).
    /// Duplicate or empty recipient lists are rejected to prevent double-payment in
    /// off-chain mapping (C-057).
    pub fn redeem_basket(
        env: Env,
        user: Address,
        recipients: Vec<Address>,
        acbu_amount: i128,
    ) -> Vec<i128> {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        Self::check_paused(&env);
        user.require_auth();

        // C-057: Validate recipients list is non-empty.
        if recipients.is_empty() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }

        // C-057: Enforce all recipient addresses are distinct.
        let rlen = recipients.len();
        for i in 0..rlen {
            for j in (i + 1)..rlen {
                if recipients.get(i).unwrap() == recipients.get(j).unwrap() {
                    env.panic_with_error(ContractError::InvalidRecipient);
                }
            }
        }

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            env.panic_with_error(ContractError::InvalidAmount);
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();

        // C-012: Ensure oracle rates are fresh before burning.
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE),
            vec![&env],
        );
        let current_time = env.ledger().timestamp();
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        if acbu_rate <= 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let net_acbu = acbu_amount
            .checked_sub(total_fee)
            .expect("Underflow in net acbu calculation");

        // usd_total = (net_acbu * acbu_rate) / DECIMALS
        let usd_total = net_acbu
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(DECIMALS))
            .expect("Overflow in usd total calculation");

        // C-012: Call reserve tracker to verify protocol health before burning.
        let current_supply: i128 = env.invoke_contract(
            &acbu_token,
            &Symbol::new(&env, "get_total_supply"),
            vec![&env],
        );
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, current_supply.into_val(&env)],
        );
        if !reserve_ok {
            env.panic_with_error(ContractError::InsufficientReserves);
        }

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let currencies: Vec<CurrencyCode> = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_CURRENCIES),
            vec![&env],
        );

        if currencies.is_empty() {
            env.panic_with_error(ContractError::InvalidCurrency);
        }

        let mut amounts_out = Vec::new(&env);
        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();

            // C-057: Each currency slot maps to the corresponding recipient by index.
            if i >= recipients.len() {
                env.panic_with_error(ContractError::InvalidRecipient);
            }
            let recipient = recipients.get(i).unwrap();

            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_BASKET_WEIGHT),
                vec![&env, currency.clone().into_val(&env)],
            );
            if weight == 0 {
                amounts_out.push_back(0);
                continue;
            }

            let rate: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_RATE),
                vec![&env, currency.clone().into_val(&env)],
            );
            if rate == 0 {
                env.panic_with_error(ContractError::InvalidRate);
            }

            let stoken: Address = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_S_TOKEN_ADDR),
                vec![&env, currency.clone().into_val(&env)],
            );

            // Per-currency gross ACBU slice and fee slice.
            let acbu_gross_i = (weight * acbu_amount) / BASIS_POINTS;
            let fee_i = (weight * total_fee) / BASIS_POINTS;
            let net_acbu_i = acbu_gross_i - fee_i;

            let usd_i = (weight * usd_total) / BASIS_POINTS;
            // native_i = (usd_i * DECIMALS) / rate — correct because usd_total was already
            // divided by DECIMALS, so multiplying back restores the fixed-point scaling.
            let native_i = usd_i
                .checked_mul(DECIMALS)
                .and_then(|v| v.checked_div(rate))
                .expect("Overflow in native_i calculation");

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
                acbu_amount: acbu_gross_i,
                net_acbu: net_acbu_i,
                local_amount: native_i,
                currency: currency.clone(),
                fee: fee_i,
                rate,
                timestamp: env.ledger().timestamp(),
            };
            env.events()
                .publish((symbol_short!("burn"), user.clone()), burn_event);
        }

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

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
        Self::check_paused(&env);
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            env.panic_with_error(ContractError::InvalidRate);
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
    }

    pub fn set_fee_single_redeem(env: Env, fee_bps: i128) {
        Self::check_admin(&env);
        Self::check_paused(&env);
        if !(0..=BASIS_POINTS).contains(&fee_bps) {
            env.panic_with_error(ContractError::InvalidRate);
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

    // ── Dependency address updaters (admin only) ──────────────────────────

    pub fn update_oracle(env: Env, new_oracle: Address) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.oracle, &new_oracle);
    }

    pub fn update_reserve_tracker(env: Env, new_reserve_tracker: Address) {
        Self::check_admin(&env);
        env.storage()
            .instance()
            .set(&DATA_KEY.reserve_tracker, &new_reserve_tracker);
    }

    pub fn update_acbu_token(env: Env, new_acbu_token: Address) {
        Self::check_admin(&env);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &new_acbu_token);
    }

    pub fn update_vault(env: Env, new_vault: Address) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.vault, &new_vault);
    }

    fn check_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            env.panic_with_error(ContractError::Paused);
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }

    pub fn version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();

        let current_version = Self::version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(ContractError::InvalidVersion);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

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
}

fn migrate_v0_to_v1(_env: Env) {
    // No storage schema changes between v0 and v1.
}
