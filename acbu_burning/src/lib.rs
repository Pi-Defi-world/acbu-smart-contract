#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env,
    IntoVal, String as SorobanString, Symbol, Vec,
};

use shared::{
    calculate_fee, BurnEvent, ContractError, CurrencyCode, DataKey as SharedDataKey, BASIS_POINTS,
    DECIMALS, MIN_BURN_AMOUNT, UPDATE_INTERVAL_SECONDS,
};

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
        env.storage().instance().set(&SharedDataKey::Version, &2u32);
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
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        let current_time = env.ledger().timestamp();
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_rate_with_timestamp"),
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
            &Symbol::new(&env, "get_s_token_address"),
            vec![&env, currency.clone().into_val(&env)],
        );

        let fee = calculate_fee(acbu_amount, fee_single);
        let net_acbu = acbu_amount
            .checked_sub(fee)
            .expect("Underflow in net acbu calculation");

        // C-019: Simplify math by removing redundant DECIMALS scaling.
        // stoken_out = (net_acbu * acbu_rate) / rate
        let stoken_out = net_acbu
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(rate))
            .expect("Overflow in stoken out calculation");

        // C-012: Call reserve tracker to verify protocol health before burning.
        // Even though burning improves the ratio, we ensure the tracker is responsive.
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
            // Optional: protocol might want to allow burning even if under-collateralized.
            // But here we enforce the check to fulfill the "call" requirement.
            env.panic_with_error(ContractError::InsufficientReserves);
        }

        // C-038: The burn call requires `user` to have authorized this contract
        // to burn on their behalf.  Soroban propagates the auth tree automatically
        // when `user.require_auth()` is called above, but we document the
        // dependency explicitly: the token contract will verify that the invoking
        // contract (this contract's address) is in the auth tree for `user`.
        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        // C-038: `transfer_from` uses this contract as the spender.  The vault
        // must have pre-approved this contract address as an allowed spender for
        // the S-token (via `approve`).  This is an explicit trust assumption:
        //   vault → approve(burning_contract, stoken, allowance)
        // If that approval is absent the call will revert with an auth error,
        // which is the correct safe-fail behaviour.
        let token = soroban_sdk::token::Client::new(&env, &stoken);
        let spender = env.current_contract_address();
        token.transfer_from(&spender, &vault, &recipient, &stoken_out);

        let tx_id = SorobanString::from_str(&env, "redeem_single");
        // FIX(#102): Emit gross acbu_amount and explicit net_acbu so indexers can
        // independently verify: acbu_amount - fee == net_acbu, and reconcile to
        // within 1 unit without needing to re-derive the fee off-chain.
        let burn_event = BurnEvent {
            transaction_id: tx_id,
            user: user.clone(),
            acbu_amount, // gross amount burned (unchanged — was already correct)
            net_acbu,    // explicit post-fee net so indexer needs no arithmetic
            local_amount: stoken_out,
            currency: currency.clone(),
            fee, // total fee for this redemption
            rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("burn"), user), burn_event);

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
        Self::check_paused(&env);
        user.require_auth();

        // C-057: Validate recipients list is non-empty.
        if recipients.is_empty() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }

        // C-057: Enforce all recipient addresses are distinct.
        // O(n²) is acceptable here — basket sizes are small (≤ ~20 currencies).
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
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        let current_time = env.ledger().timestamp();
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        if acbu_rate <= 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        // FIX(#102): Compute totals from gross acbu_amount before any deduction.
        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let net_acbu = acbu_amount
            .checked_sub(total_fee)
            .expect("Underflow in net acbu calculation");

        // C-019: Simplified USD total calculation (net_acbu * acbu_rate) / DECIMALS
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

        // C-038: The burn call requires `user` to have authorized this contract
        // to burn on their behalf.  Soroban propagates the auth tree automatically
        // when `user.require_auth()` is called above, but we document the
        // dependency explicitly: the token contract will verify that the invoking
        // contract (this contract's address) is in the auth tree for `user`.
        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let currencies: Vec<CurrencyCode> = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_currencies"),
            vec![&env],
        );

        if currencies.is_empty() {
            env.panic_with_error(ContractError::InvalidCurrency);
        }

        let mut amounts_out = Vec::new(&env);
        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();

            // C-057: Each currency slot maps to the corresponding recipient by index.
            // If the caller supplied fewer recipients than currencies, reject.
            if i >= recipients.len() {
                env.panic_with_error(ContractError::InvalidRecipient);
            }
            let recipient = recipients.get(i).unwrap();

            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_basket_weight"),
                vec![&env, currency.into_val(&env)],
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
                env.panic_with_error(ContractError::InvalidRate);
            }

            let stoken: Address = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_s_token_address"),
                vec![&env, currency.clone().into_val(&env)],
            );

            // Per-currency gross ACBU slice and fee slice (both derived from gross acbu_amount).
            let acbu_gross_i = (weight * acbu_amount) / BASIS_POINTS;
            let fee_i = (weight * total_fee) / BASIS_POINTS;
            let net_acbu_i = acbu_gross_i - fee_i;

            let usd_i = (weight * usd_total) / BASIS_POINTS;
            let native_i = (usd_i * DECIMALS) / rate;

            if native_i > 0 {
                // C-038: `transfer_from` uses this contract as the spender.  The vault
                // must have pre-approved this contract address as an allowed spender for
                // each S-token (via `approve`).  This is an explicit trust assumption:
                //   vault → approve(burning_contract, stoken, allowance)
                // If that approval is absent the call will revert with an auth error,
                // which is the correct safe-fail behaviour.
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                let spender = env.current_contract_address();
                token.transfer_from(&spender, &vault, &recipient, &native_i);
            }

            amounts_out.push_back(native_i);

            let tx_id = SorobanString::from_str(&env, "redeem_basket");
            // FIX(#102): Emit gross per-currency acbu slice (acbu_gross_i) not the
            // already-deducted net_acbu. Also emit net_acbu_i and fee_i so indexers
            // can verify acbu_gross_i - fee_i == net_acbu_i for each currency leg,
            // and sum all acbu_gross_i values back to the top-level acbu_amount.
            let burn_event = BurnEvent {
                transaction_id: tx_id,
                user: user.clone(),
                acbu_amount: acbu_gross_i, // per-currency gross slice (was incorrectly net_acbu total)
                net_acbu: net_acbu_i,      // per-currency net slice
                local_amount: native_i,
                currency: currency.clone(),
                fee: fee_i, // per-currency fee slice
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
        Self::check_paused(&env);

        let current_version = Self::version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(ContractError::InvalidVersion);
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
}

fn migrate_v0_to_v1(_env: Env) {
    // Initial migration logic
}
