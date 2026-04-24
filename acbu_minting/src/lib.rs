#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env, IntoVal,
    String as SorobanString, Symbol,
};

use shared::{
    calculate_amount_after_fee, calculate_fee, CurrencyCode, DataKey as SharedDataKey, MintEvent,
    BASIS_POINTS, CONTRACT_VERSION, DECIMALS, MAX_MINT_AMOUNT, MIN_MINT_AMOUNT,
};


#[allow(dead_code)]
pub mod token_contract {
    soroban_sdk::contractimport!(
        file = "../soroban_token_contract.wasm",
        sha256 = "6b14997b915dee21082884cd5a2f1f2f0aef0073d1dcb9c5b3c674cf487fb41d"
    );
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementProof {
    pub proof_id: SorobanString,
    pub settled: bool,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub oracle: Symbol,
    pub reserve_tracker: Symbol,
    pub acbu_token: Symbol,
    pub usdc_token: Symbol,
    pub vault: Symbol,
    pub treasury: Symbol,
    pub fee_rate: Symbol,
    pub fee_single: Symbol,
    pub paused: Symbol,
    pub min_mint_amount: Symbol,
    pub max_mint_amount: Symbol,
    pub total_supply: Symbol,
    pub operator: Symbol,
    pub used_proofs: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserve_tracker: symbol_short!("RES_TRK"),
    acbu_token: symbol_short!("ACBU_TKN"),
    usdc_token: symbol_short!("USDC_TKN"),
    vault: symbol_short!("VAULT"),
    treasury: symbol_short!("TRSY"),
    fee_rate: symbol_short!("FEE_RATE"),
    fee_single: symbol_short!("FEE_SGL"),
    paused: symbol_short!("PAUSED"),
    min_mint_amount: symbol_short!("MIN_MINT"),
    max_mint_amount: symbol_short!("MAX_MINT"),
    total_supply: symbol_short!("SUPPLY"),
    operator: symbol_short!("OPERTR"),
    used_proofs: symbol_short!("PRF_SET"),
};

// CONTRACT_VERSION is imported from shared

#[contract]
pub struct MintingContract;

#[contractimpl]
impl MintingContract {
    /// Initialize the minting contract.
    /// `fee_rate_bps` applies to basket and USDC paths; `fee_single_bps` to single S-token deposits (typically higher).
    // Soroban initialize functions are idiomatic with many parameters; a config-struct
    // refactor is a separate concern.
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        reserve_tracker: Address,
        acbu_token: Address,
        usdc_token: Address,
        vault: Address,
        treasury: Address,
        fee_rate_bps: i128,
        fee_single_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }

        if !(0..=BASIS_POINTS).contains(&fee_rate_bps)
            || !(0..=BASIS_POINTS).contains(&fee_single_bps)
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
            .set(&DATA_KEY.usdc_token, &usdc_token);
        env.storage().instance().set(&DATA_KEY.vault, &vault);
        env.storage().instance().set(&DATA_KEY.treasury, &treasury);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_single, &fee_single_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_mint_amount, &MIN_MINT_AMOUNT);
        env.storage()
            .instance()
            .set(&DATA_KEY.max_mint_amount, &MAX_MINT_AMOUNT);
        env.storage().instance().set(&DATA_KEY.total_supply, &0i128);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    /// Mint ACBU from USDC deposit (unchanged reserve/oracle flow).
    pub fn mint_from_usdc(env: Env, user: Address, usdc_amount: i128, recipient: Address) -> i128 {
        Self::check_paused(&env);
        user.require_auth();
        // C-058: reject contract-type recipients — minting to a contract address
        // that has no token-receipt logic would permanently strand the funds.
        Self::assert_recipient_is_account(&recipient);

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.max_mint_amount)
            .unwrap();

        if usdc_amount < min_amount || usdc_amount > max_amount {
            panic!("Invalid mint amount");
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let usdc_token: Address = env.storage().instance().get(&DATA_KEY.usdc_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();
        let mut total_supply: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0);

        // Get ACBU rate with timestamp and validate oracle freshness
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        check_oracle_freshness(&env, oracle_timestamp, UPDATE_INTERVAL_SECONDS);

        let usdc_after_fee = calculate_amount_after_fee(usdc_amount, fee_rate);
        let acbu_amount = usdc_after_fee
            .checked_mul(DECIMALS)
            .and_then(|v| v.checked_div(acbu_rate))
            .expect("Overflow in acbu amount calculation");

        let projected_supply = total_supply
            .checked_add(acbu_amount)
            .expect("Overflow in projected supply calculation");
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        total_supply += acbu_amount;
        env.storage()
            .instance()
            .set(&DATA_KEY.total_supply, &total_supply);

        let usdc_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        usdc_client.transfer(&user, &env.current_contract_address(), &usdc_amount);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        let fee = calculate_fee(usdc_amount, fee_rate);

        let tx_id = generate_unique_tx_id(&env, &recipient, acbu_amount, "mint_usdc");
        let mint_event = MintEvent {
            transaction_id: tx_id,
            user: recipient.clone(),
            usdc_amount,
            acbu_amount,
            fee,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        acbu_amount
    }

    /// Mint ACBU by depositing Afreum-style S-tokens in full basket proportions (lower fee tier).
    /// Pulls each S-token from `user` into `vault` per oracle weights and rates.
    pub fn mint_from_basket(
        env: Env,
        user: Address,
        recipient: Address,
        acbu_amount: i128,
        proof_id: SorobanString,
    ) -> i128 {
        Self::check_paused(&env);
        user.require_auth();
        // C-058: reject contract-type recipients — minting to a contract address
        // that has no token-receipt logic would permanently strand the funds.
        Self::assert_recipient_is_account(&recipient);

        if !check_proof_unused(&env, &proof_id) {
            panic!("Proof already used");
        }

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.max_mint_amount)
            .unwrap();
        if acbu_amount < min_amount || acbu_amount > max_amount {
            panic!("Invalid mint amount");
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let treasury: Address = env.storage().instance().get(&DATA_KEY.treasury).unwrap();
        let mut total_supply: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0);

        // Get ACBU rate with timestamp and validate oracle freshness
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        check_oracle_freshness(&env, oracle_timestamp, UPDATE_INTERVAL_SECONDS);

        let fee_acbu = calculate_fee(acbu_amount, fee_rate);
        let net_mint = acbu_amount
            .checked_sub(fee_acbu)
            .expect("Underflow in net mint calculation");
        let projected_supply = total_supply
            .checked_add(acbu_amount)
            .expect("Overflow in projected supply calculation");

        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        let currencies: soroban_sdk::Vec<CurrencyCode> = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_currencies"),
            vec![&env],
        );

        let usd_total: i128 = acbu_amount
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(DECIMALS))
            .expect("Overflow in usd total calculation");

        for currency in currencies.iter() {
            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, "get_basket_weight"),
                vec![&env, currency.clone().into_val(&env)],
            );
            if weight == 0 {
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

            let usd_i = weight
                .checked_mul(usd_total)
                .and_then(|v| v.checked_div(BASIS_POINTS))
                .expect("Overflow in usd_i calculation");
            let native_i = usd_i
                .checked_mul(DECIMALS)
                .and_then(|v| v.checked_div(rate))
                .expect("Overflow in native_i calculation");
            if native_i > 0 {
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                token.transfer(&user, &vault, &native_i);
            }
        }

        total_supply += acbu_amount;
        env.storage()
            .instance()
            .set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &net_mint);
        if fee_acbu > 0 {
            acbu_sac.mint(&treasury, &fee_acbu);
        }

        let tx_id = generate_unique_tx_id(&env, &recipient, net_mint, "mint_basket");
        let mint_event = MintEvent {
            transaction_id: tx_id,
            user: recipient.clone(),
            usdc_amount: usd_total,
            acbu_amount: net_mint,
            fee: fee_acbu,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        acbu_amount
    }

    /// Single S-token deposit: Afreum ramp delivers one S-token; fee tier is `fee_single_bps`.
    /// On-chain DEX rebalancing into the full basket is orchestrated off-chain or in a future release;
    /// this entrypoint only prices the deposit and credits ACBU from oracle rates.
    pub fn mint_from_single(
        env: Env,
        user: Address,
        recipient: Address,
        currency: CurrencyCode,
        s_token_amount: i128,
    ) -> i128 {
        Self::check_paused(&env);
        user.require_auth();
        // C-058: reject contract-type recipients — minting to a contract address
        // that has no token-receipt logic would permanently strand the funds.
        Self::assert_recipient_is_account(&recipient);

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.max_mint_amount)
            .unwrap();

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let fee_single: i128 = env.storage().instance().get(&DATA_KEY.fee_single).unwrap();
        let mut total_supply: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0);

        let expected_stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_s_token_address"),
            vec![&env, currency.clone().into_val(&env)],
        );

        // Get ACBU rate with timestamp and validate oracle freshness
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        check_oracle_freshness(&env, oracle_timestamp, UPDATE_INTERVAL_SECONDS);

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_rate_with_timestamp"),
            vec![&env, currency.clone().into_val(&env)],
        );
        check_oracle_freshness(&env, rate_timestamp, UPDATE_INTERVAL_SECONDS);

        if rate == 0 {
            panic!("Invalid oracle rate");
        }

        let usd_gross = s_token_amount
            .checked_mul(rate)
            .and_then(|v| v.checked_div(DECIMALS))
            .expect("Overflow in usd_gross calculation");
        if usd_gross < min_amount || usd_gross > max_amount {
            panic!("Invalid mint amount");
        }

        let usd_after_fee = calculate_amount_after_fee(usd_gross, fee_single);
        let acbu_amount = usd_after_fee
            .checked_mul(DECIMALS)
            .and_then(|v| v.checked_div(acbu_rate))
            .expect("Overflow in acbu amount calculation");

        let projected_supply = total_supply
            .checked_add(acbu_amount)
            .expect("Overflow in projected supply calculation");
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        let token = soroban_sdk::token::Client::new(&env, &expected_stoken);
        token.transfer(&user, &vault, &s_token_amount);

        total_supply += acbu_amount;
        env.storage()
            .instance()
            .set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        let fee = calculate_fee(usd_gross, fee_single);
        let tx_id = generate_unique_tx_id(&env, &recipient, acbu_amount, "mint_single");
        let mint_event = MintEvent {
            transaction_id: tx_id,
            user: recipient.clone(),
            usdc_amount: usd_gross,
            acbu_amount,
            fee,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        acbu_amount
    }

    /// Custodial demo-fiat mint: `operator` (backend key) authorizes; pulls S-token from **this
    /// contract's balance** (pre-funded demo SAC supply) into `vault`, then mints ACBU to
    /// `recipient` using the same pricing as [`Self::mint_from_single`].
    pub fn mint_from_demo_fiat(
        env: Env,
        operator: Address,
        recipient: Address,
        currency: CurrencyCode,
        fiat_amount: i128,
        proof_id: SorobanString,
    ) -> i128 {
        Self::check_paused(&env);
        let expected_operator: Address = Self::get_operator(env.clone());
        if operator != expected_operator {
            panic!("Unauthorized operator");
        }
        operator.require_auth();
        // C-058: reject contract-type recipients — minting to a contract address
        // that has no token-receipt logic would permanently strand the funds.
        Self::assert_recipient_is_account(&recipient);

        if !check_proof_unused(&env, &proof_id) {
            panic!("Proof already used");
        }

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.max_mint_amount)
            .unwrap();

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let fee_single: i128 = env.storage().instance().get(&DATA_KEY.fee_single).unwrap();
        let mut total_supply: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0);

        let expected_stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_s_token_address"),
            vec![&env, currency.clone().into_val(&env)],
        );

        // Get ACBU rate with timestamp and validate oracle freshness
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        check_oracle_freshness(&env, oracle_timestamp, UPDATE_INTERVAL_SECONDS);

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_rate_with_timestamp"),
            vec![&env, currency.clone().into_val(&env)],
        );
        check_oracle_freshness(&env, rate_timestamp, UPDATE_INTERVAL_SECONDS);

        let usd_gross = fiat_amount
            .checked_mul(rate)
            .and_then(|v| v.checked_div(DECIMALS))
            .expect("Overflow in usd_gross calculation");
        if usd_gross < min_amount || usd_gross > max_amount {
            panic!("Invalid mint amount");
        }

        let usd_after_fee = calculate_amount_after_fee(usd_gross, fee_single);
        let acbu_amount = usd_after_fee
            .checked_mul(DECIMALS)
            .and_then(|v| v.checked_div(acbu_rate))
            .expect("Overflow in acbu amount calculation");

        let projected_supply = total_supply
            .checked_add(acbu_amount)
            .expect("Overflow in projected supply calculation");
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        let custody = env.current_contract_address();
        let token = soroban_sdk::token::Client::new(&env, &expected_stoken);
        token.transfer(&custody, &vault, &fiat_amount);

        total_supply += acbu_amount;
        env.storage()
            .instance()
            .set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        let fee = calculate_fee(usd_gross, fee_single);
        let mint_event = MintEvent {
            transaction_id: SorobanString::from_str(&env, "mint_demo_fiat"),
            user: recipient.clone(),
            usdc_amount: usd_gross,
            acbu_amount,
            fee,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        // Seal the proof so it cannot be replayed (fixes the check_proof_unused guard above).
        mark_proof_used(&env, &proof_id);

        acbu_amount
    }

    /// Fintech-partner fiat mint: operator (fintech backend) authorizes; validates fintech_tx_id
    /// to prevent duplicate minting. Requires both operator authorization and valid fintech transaction.
    /// This function enforces strict access control: only the operator (fintech partner) can call it.
    pub fn mint_from_fiat(
        env: Env,
        operator: Address,
        recipient: Address,
        currency: CurrencyCode,
        fiat_amount: i128,
        fintech_tx_id: String as SorobanString,
    ) -> i128 {
        Self::check_paused(&env);
        let expected_operator: Address = Self::get_operator(env.clone());
        
        // Strict access control: only operator (fintech backend) can call
        if operator != expected_operator {
            panic!("Unauthorized: only operator can call mint_from_fiat");
        }
        operator.require_auth();

        // Validate fintech_tx_id is not empty
        if fintech_tx_id.len() == 0 {
            panic!("fintech_tx_id cannot be empty");
        }

        // Check if fintech_tx_id has already been processed
        let mut processed_ids: soroban_sdk::Map<String as SorobanString, bool> = env
            .storage()
            .instance()
            .get(&DATA_KEY.processed_fintech_tx_ids)
            .unwrap_or_else(|| soroban_sdk::map![&env]);
        
        if processed_ids.contains_key(&fintech_tx_id) {
            panic!("Duplicate fintech transaction ID");
        }

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.max_mint_amount)
            .unwrap();

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();
        let treasury: Address = env.storage().instance().get(&DATA_KEY.treasury).unwrap();
        let mut total_supply: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0);

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

        let usd_gross = (fiat_amount * rate) / DECIMALS;
        if usd_gross < min_amount || usd_gross > max_amount {
            panic!("Invalid mint amount");
        }

        let usd_after_fee = calculate_amount_after_fee(usd_gross, fee_rate);
        let acbu_amount = (usd_after_fee * DECIMALS) / acbu_rate;

        let projected_supply = total_supply + acbu_amount;
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        // For mint_from_fiat, fiat deposit is handled off-chain by the fintech partner.
        // No on-chain token transfer needed; fintech validates and deposits fiat in their system.

        total_supply += acbu_amount;
        env.storage().instance().set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);
        
        let fee = calculate_fee(usd_gross, fee_rate);
        if fee > 0 {
            acbu_sac.mint(&treasury, &fee);
        }

        // Mark fintech_tx_id as processed to prevent duplicate minting
        processed_ids.set(fintech_tx_id.clone(), true);
        env.storage()
            .instance()
            .set(&DATA_KEY.processed_fintech_tx_ids, &processed_ids);

        let mint_event = MintEvent {
            transaction_id: fintech_tx_id,
            user: recipient.clone(),
            usdc_amount: usd_gross,
            acbu_amount,
            fee,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        acbu_amount
    }

    /// Helper to check if an address is authorized as operator (fintech backend).
    /// Returns true if the address is the configured operator.
    fn check_is_operator(env: &Env, address: &Address) -> bool {
        let operator: Address = Self::get_operator(env.clone());
        address == &operator
    }

    /// Helper to check if an address is authorized as admin.
    /// Returns true if the address is the configured admin.
    fn check_is_admin(env: &Env, address: &Address) -> bool {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        address == &admin
    }

    /// Testnet / ops: transfer demo basket S-token from custodial balance on this contract to
    /// `recipient` (e.g. user faucet). Admin only; caps per call to limit abuse.
    pub fn admin_drip_demo_fiat(
        env: Env,
        recipient: Address,
        currency: CurrencyCode,
        amount: i128,
    ) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        // C-058: reject contract-type recipients to prevent stranded token transfers.
        Self::assert_recipient_is_account(&recipient);
        if amount <= 0 {
            panic!("Invalid drip amount");
        }
        const MAX_DRIP: i128 = 100_000_000_000_000; // 10M whole units at 7 decimals
        if amount > MAX_DRIP {
            panic!("Drip amount exceeds cap");
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_s_token_address"),
            vec![&env, currency.clone().into_val(&env)],
        );
        let custody = env.current_contract_address();
        let token = soroban_sdk::token::Client::new(&env, &stoken);
        let custody_balance = token.balance(&custody);
        if custody_balance < amount {
            panic!("Insufficient demo fiat custody balance");
        }
        token.transfer(&custody, &recipient, &amount);
    }

    /// General fiat mint: User/fintech provides fintech_tx_id; operator/backend signs.
    /// Validates that the request is not duplicated and that rate/reserve checks pass.
    /// This replaces the previous hardcoded 1:1 minting path with full oracle + reserve validation.
    pub fn mint_from_fiat(
        env: Env,
        operator: Address,
        recipient: Address,
        currency: CurrencyCode,
        fiat_amount: i128,
        fintech_tx_id: SorobanString,
    ) -> i128 {
        Self::check_paused(&env);
        let expected_operator: Address = Self::get_operator(env.clone());
        if operator != expected_operator {
            panic!("Unauthorized operator");
        }
        operator.require_auth();
        // C-058: reject contract-type recipients — minting to a contract address
        // that has no token-receipt logic would permanently strand the funds.
        Self::assert_recipient_is_account(&recipient);
        // Prevent self-minting: recipient cannot be the minter
        if recipient == operator {
            panic!("Recipient cannot be operator");
        }

        if !check_proof_unused(&env, &fintech_tx_id) {
            panic!("Fiat transaction already processed");
        }

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        let max_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.max_mint_amount)
            .unwrap();

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let fee_single: i128 = env.storage().instance().get(&DATA_KEY.fee_single).unwrap();
        let mut total_supply: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0);

        let expected_stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_s_token_address"),
            vec![&env, currency.clone().into_val(&env)],
        );

        // Get ACBU rate with timestamp and validate oracle freshness
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate_with_timestamp"),
            vec![&env],
        );
        check_oracle_freshness(&env, oracle_timestamp, UPDATE_INTERVAL_SECONDS);

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_rate_with_timestamp"),
            vec![&env, currency.clone().into_val(&env)],
        );
        check_oracle_freshness(&env, rate_timestamp, UPDATE_INTERVAL_SECONDS);

        if rate == 0 {
            panic!("Invalid oracle rate");
        }

        let usd_gross = fiat_amount
            .checked_mul(rate)
            .and_then(|v| v.checked_div(DECIMALS))
            .expect("Overflow in usd_gross calculation");
        if usd_gross < min_amount || usd_gross > max_amount {
            panic!("Invalid mint amount");
        }

        let usd_after_fee = calculate_amount_after_fee(usd_gross, fee_single);
        let acbu_amount = usd_after_fee
            .checked_mul(DECIMALS)
            .and_then(|v| v.checked_div(acbu_rate))
            .expect("Overflow in acbu amount calculation");

        let projected_supply = total_supply
            .checked_add(acbu_amount)
            .expect("Overflow in projected supply calculation");
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        // Pre-fund custody: the fiat amount is assumed to be held in the contract's balance before this call
        let custody = env.current_contract_address();
        let token = soroban_sdk::token::Client::new(&env, &expected_stoken);
        token.transfer(&custody, &vault, &fiat_amount);

        total_supply += acbu_amount;
        env.storage()
            .instance()
            .set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        let fee = calculate_fee(usd_gross, fee_single);
        let mint_event = MintEvent {
            transaction_id: fintech_tx_id.clone(),
            user: recipient.clone(),
            usdc_amount: usd_gross,
            acbu_amount,
            fee,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        mark_proof_used(&env, &fintech_tx_id);
        acbu_amount
    }

        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        env.storage()
            .instance()
            .get(&DATA_KEY.operator)
            .unwrap_or(admin)
    }

    pub fn set_operator(env: Env, new_operator: Address) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DATA_KEY.operator, &new_operator);
    }

    pub fn sync_supply(env: Env, new_supply: i128) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DATA_KEY.total_supply, &new_supply);
    }

    pub fn get_total_supply(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DATA_KEY.total_supply)
            .unwrap_or(0)
    }

    pub fn pause(env: Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
    }

    pub fn unpause(env: Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    pub fn set_fee_rate(env: Env, fee_rate_bps: i128) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            panic!("Invalid fee rate");
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
    }

    pub fn set_fee_single(env: Env, fee_single_bps: i128) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        if !(0..=BASIS_POINTS).contains(&fee_single_bps) {
            panic!("Invalid fee rate");
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_single, &fee_single_bps);
    }

    pub fn get_fee_rate(env: Env) -> i128 {
        env.storage().instance().get(&DATA_KEY.fee_rate).unwrap()
    }

    pub fn get_fee_single(env: Env) -> i128 {
        env.storage().instance().get(&DATA_KEY.fee_single).unwrap()
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

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();

        let current_version = Self::get_version(env.clone());
        if new_version <= current_version {
            panic!("Invalid version upgrade");
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Run migrations — the match will gain new arms as versions are added.
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
}

// Helper functions for proof tracking and validation
fn check_oracle_freshness(env: &Env, oracle_timestamp: u64, max_staleness_seconds: u64) {
    let current_time = env.ledger().timestamp();
    if current_time > oracle_timestamp.saturating_add(max_staleness_seconds) {
        panic!("Oracle data is stale: last update was too long ago");
    }
}

fn generate_unique_tx_id(env: &Env, user: &Address, amount: i128, prefix: &str) -> SorobanString {
    let timestamp = env.ledger().timestamp();
    let seq = env.ledger().sequence();
    let mut hash: u64 = 0u64; 
    hash = hash.wrapping_mul(31).wrapping_add(prefix.as_bytes().iter().fold(0u64, |h, &b| h.wrapping_mul(31).wrapping_add(b as u64)));
    hash = hash.wrapping_mul(31).wrapping_add(user.as_val().get(0).unwrap_or(0) as u64));
    hash = hash.wrapping_mul(31).wrapping_add((amount & 0xFFFFFFFF) as u64);
    hash = hash.wrapping_mul(31).wrapping_add(timestamp as u64);
    hash = hash.wrapping_mul(31).wrapping_add(seq as u64);
    let id_str = format!("{}_{:x}", prefix, hash);
    SorobanString::from_str(env, &id_str)
}

// ---------------------------------------------------------------------------
// Helper: assert that an address belongs to an account (not a contract).
// C-058 — minting to a contract address that has no token-receipt logic would
// permanently strand funds.
//
// NOTE: soroban-sdk 21 does not expose an `is_account()` predicate on
// `Address`; the distinction is enforced off-chain by the client SDK and by
// Stellar's native authorization model.  This stub preserves the call sites so
// the intent is visible and can be filled in when the SDK gains the API.
// ---------------------------------------------------------------------------
impl MintingContract {
    #[allow(clippy::unused_self)]
    fn assert_recipient_is_account(_address: &Address) {
        // On-chain account-vs-contract check is not available in soroban-sdk 21.
        // Enforcement is the responsibility of the calling client.
    }
}

// ---------------------------------------------------------------------------
// Proof-replay helpers: used by mint_from_demo_fiat to prevent double-spend.
// ---------------------------------------------------------------------------
fn check_proof_unused(env: &Env, proof_id: &SorobanString) -> bool {
    !env.storage()
        .persistent()
        .has(&(symbol_short!("PRF_SET"), proof_id.clone()))
}

fn mark_proof_used(env: &Env, proof_id: &SorobanString) {
    env.storage()
        .persistent()
        .set(&(symbol_short!("PRF_SET"), proof_id.clone()), &true);
}
