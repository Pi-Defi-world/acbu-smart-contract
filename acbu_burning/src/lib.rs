#![no_std]
use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, symbol_short, vec, Address, BytesN, Env,
    IntoVal, String as SorobanString, Symbol, Vec,
};

use shared::{
    calculate_fee, reentrancy_guard, BurnEvent, ContractError, ContractPhase, CurrencyCode,
    DataKey as SharedDataKey, BASIS_POINTS, CONTRACT_VERSION, DECIMALS, MIN_BURN_AMOUNT,
    ORACLE_GET_ACBU_RATE_WITH_TS, ORACLE_GET_BASKET_WEIGHT, ORACLE_GET_CURRENCIES,
    ORACLE_GET_RATE_WITH_TS, ORACLE_GET_S_TOKEN_ADDR, RESERVE_IS_SUFFICIENT,
    TOKEN_GET_TOTAL_SUPPLY, UPDATE_INTERVAL_SECONDS,
};

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
    pub phase: Symbol,
    pub min_burn_amount: Symbol,
    pub pending_admin: Symbol,
    pub pending_admin_eligible_at: Symbol,
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
    phase: symbol_short!("PHASE"),
    min_burn_amount: symbol_short!("MIN_BURN"),
    pending_admin: symbol_short!("PEND_ADM"),
    pending_admin_eligible_at: symbol_short!("PEND_ETA"),
};


contractmeta!(key = "version", val = "1");

/// Admin rotation timelock: the pending admin must wait this long before
/// claiming ownership, giving the current admin a window to cancel a mistaken
/// or malicious transfer.
const ADMIN_TIMELOCK_SECONDS: u64 = 86_400;

#[contract]
pub struct BurningContract;

#[contractimpl]
impl BurningContract {
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
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
        env.storage().instance().set(&DATA_KEY.phase, &ContractPhase::Active);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_burn_amount, &MIN_BURN_AMOUNT);
    }

    pub fn redeem_single(
        env: Env,
        user: Address,
        recipient: Address,
        acbu_amount: i128,
        currency: CurrencyCode,
    ) -> i128 {

        Self::check_paused(&env);
        user.require_auth();
        Self::validate_recipient(&env, &recipient);

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

        let current_time = env.ledger().timestamp();
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE_WITH_TS),
            vec![&env],
        );
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_RATE_WITH_TS),
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


        let stoken_out = net_acbu
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(rate))
            .expect("Overflow in stoken out calculation");

        Self::check_reserves(&env, &acbu_token, &reserve_tracker_addr);

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let token = soroban_sdk::token::Client::new(&env, &stoken);
        let spender = env.current_contract_address();
        token.transfer_from(&spender, &vault, &recipient, &stoken_out);


        let burn_event = BurnEvent {
            transaction_id: SorobanString::from_str(&env, "redeem_single"),
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


        stoken_out
    }

    /// Redeem ACBU for proportional Afreum S-tokens across the basket (lower fee tier).
    pub fn redeem_basket(
        env: Env,
        user: Address,
        recipients: Vec<Address>,
        acbu_amount: i128,
    ) -> Vec<i128> {
        Self::check_paused(&env);
        user.require_auth();

        if recipients.is_empty() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }

        for i in 0..recipients.len() {
            for j in (i + 1)..recipients.len() {
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

        let current_time = env.ledger().timestamp();
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE_WITH_TS),
            vec![&env],
        );
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }
        if acbu_rate <= 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        let currencies: Vec<CurrencyCode> = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_CURRENCIES),
            vec![&env],
        );
        if currencies.is_empty() {
            env.panic_with_error(ContractError::InvalidCurrency);
        }
        if recipients.len() != currencies.len() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }

        let mut weights = Vec::new(&env);
        let mut total_weight: i128 = 0;
        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_BASKET_WEIGHT),
                vec![&env, currency.into_val(&env)],
            );
            total_weight = total_weight
                .checked_add(weight)
                .expect("Overflow in total weight");
            weights.push_back(weight);
        }

        if total_weight == 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let net_acbu = acbu_amount
            .checked_sub(total_fee)
            .expect("Underflow in net acbu");
        let usd_total = net_acbu
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(DECIMALS))
            .expect("Overflow in usd total");

        reentrancy_guard::acquire_guard(&env);
        Self::check_reserves(&env, &acbu_token, &reserve_tracker_addr);

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let mut last_positive_weight_index: Option<usize> = None;
        for i in 0..weights.len() {
            if weights.get(i).unwrap() > 0 {
                last_positive_weight_index = Some(i);
            }
        }

        let mut amounts_out = Vec::new(&env);
        let mut allocated_usd = 0i128;
        let mut allocated_gross = 0i128;
        let mut allocated_fee = 0i128;

        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let recipient = recipients.get(i).unwrap();
            let weight = weights.get(i).unwrap();

            if weight == 0 {
                amounts_out.push_back(0);
                continue;
            }

            let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_RATE_WITH_TS),
                vec![&env, currency.clone().into_val(&env)],
            );
            if current_time > rate_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
                env.panic_with_error(ContractError::OracleError);
            }
            if rate <= 0 {
                env.panic_with_error(ContractError::InvalidRate);
            }

            let (usd_i, acbu_gross_i, fee_i) = if last_positive_weight_index == Some(i) {
                (
                    usd_total
                        .checked_sub(allocated_usd)
                        .expect("Underflow in remaining usd"),
                    acbu_amount
                        .checked_sub(allocated_gross)
                        .expect("Underflow in remaining gross"),
                    total_fee
                        .checked_sub(allocated_fee)
                        .expect("Underflow in remaining fee"),
                )
            } else {
                (
                    Self::weighted_floor(usd_total, weight, total_weight),
                    Self::weighted_floor(acbu_amount, weight, total_weight),
                    Self::weighted_floor(total_fee, weight, total_weight),
                )
            };

            allocated_usd = allocated_usd
                .checked_add(usd_i)
                .expect("Overflow in allocated usd");
            allocated_gross = allocated_gross
                .checked_add(acbu_gross_i)
                .expect("Overflow in allocated gross");
            allocated_fee = allocated_fee
                .checked_add(fee_i)
                .expect("Overflow in allocated fee");

            let stoken: Address = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_S_TOKEN_ADDR),
                vec![&env, currency.clone().into_val(&env)],
            );

            let net_acbu_i = acbu_gross_i
                .checked_sub(fee_i)
                .expect("Underflow in net per-leg");

            let native_i = net_acbu_i
                .checked_mul(acbu_rate)
                .and_then(|v| v.checked_div(rate))
                .expect("Overflow in native amount");

            if native_i > 0 {
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                let spender = env.current_contract_address();
                token.transfer_from(&spender, &vault, &recipient, &native_i);
            }
            amounts_out.push_back(native_i);

            let burn_event = BurnEvent {
                transaction_id: SorobanString::from_str(&env, "redeem_basket"),
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

        reentrancy_guard::release_guard(&env);
        amounts_out
    }



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
            (current_admin, new_admin, eligible_at),
        );
    }

    pub fn accept_admin(env: Env) {
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin)
            .unwrap_or_else(|| env.panic_with_error(ContractError::Unknown));
        pending_admin.require_auth();

        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(ContractError::Unauthorized);
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
            (old_admin, pending_admin, env.ledger().timestamp()),
        );
    }

    pub fn cancel_admin_transfer(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        env.events().publish(
            (symbol_short!("adm_cncl"),),
            (admin, env.ledger().timestamp()),
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

    pub fn is_paused(env: Env) -> bool {
        let phase: ContractPhase = env
            .storage()
            .instance()
            .get(&DATA_KEY.phase)
            .unwrap_or(ContractPhase::Active);
        matches!(phase, ContractPhase::Paused)
    }

    pub fn get_phase(env: Env) -> ContractPhase {
        env.storage()
            .instance()
            .get(&DATA_KEY.phase)
            .unwrap_or(ContractPhase::Active)
    }

    pub fn get_fee_rate(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .unwrap_or(0)
    }

    pub fn get_fee_single_redeem(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DATA_KEY.fee_single_redeem)
            .unwrap_or(0)
    }

    pub fn get_acbu_token(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .unwrap()
    }

    pub fn get_oracle(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DATA_KEY.oracle)
            .unwrap()
    }

    pub fn get_reserve_tracker(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap()
    }

    pub fn get_withdrawal_processor(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DATA_KEY.withdrawal_processor)
            .unwrap()
    }

    pub fn get_vault(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DATA_KEY.vault)
            .unwrap()
    }

    pub fn get_min_burn_amount(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap_or(MIN_BURN_AMOUNT)
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn is_initialized(env: Env) -> bool {
        env.storage().instance().has(&DATA_KEY.admin)
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);
        let current_version = Self::get_version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(ContractError::InvalidVersion);
        }
        env.deployer().update_current_contract_wasm(new_wasm_hash);

        for v in current_version..new_version {
            if v == 0 {
                shared::migrate_v0_to_v1(&env);
            }
        }
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &new_version);
    }

    fn weighted_floor(total: i128, weight: i128, total_weight: i128) -> i128 {
        total
            .checked_mul(weight)
            .and_then(|v| v.checked_div(total_weight))
            .expect("Overflow in weighted allocation")
    }

    fn check_reserves(env: &Env, acbu_token: &Address, reserve_tracker_addr: &Address) {
        let current_supply: i128 = env.invoke_contract(
            acbu_token,
            &Symbol::new(env, TOKEN_GET_TOTAL_SUPPLY),
            vec![env],
        );
        let reserve_ok: bool = env.invoke_contract(
            reserve_tracker_addr,
            &Symbol::new(env, RESERVE_IS_SUFFICIENT),
            vec![env, current_supply.into_val(env)],
        );
        if !reserve_ok {
            env.panic_with_error(ContractError::InsufficientReserves);
        }
    }

    fn check_paused(env: &Env) {
        let phase: ContractPhase = env
            .storage()
            .instance()
            .get(&DATA_KEY.phase)
            .unwrap_or(ContractPhase::Active);
        if matches!(phase, ContractPhase::Paused) {
            env.panic_with_error(ContractError::Paused);
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }

    fn validate_recipient(env: &Env, recipient: &Address) {
        if *recipient == env.current_contract_address() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }
    }
}
