#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env, IntoVal, String as SorobanString,
    Symbol,
};

use shared::{
    calculate_amount_after_fee, calculate_fee, CurrencyCode, MintEvent, BASIS_POINTS, DECIMALS, MAX_MINT_AMOUNT,
    MIN_MINT_AMOUNT,
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
    pub usdc_token: Symbol,
    pub vault: Symbol,
    pub treasury: Symbol,
    pub fee_rate: Symbol,
    pub fee_single: Symbol,
    pub paused: Symbol,
    pub min_mint_amount: Symbol,
    pub max_mint_amount: Symbol,
    pub total_supply: Symbol,
    pub version: Symbol,
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
    version: symbol_short!("VERSION"),
};

const VERSION: u32 = 2;

#[contract]
pub struct MintingContract;

#[contractimpl]
impl MintingContract {
    /// Initialize the minting contract.
    /// `fee_rate_bps` applies to basket and USDC paths; `fee_single_bps` to single S-token deposits (typically higher).
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
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Mint ACBU from USDC deposit (unchanged reserve/oracle flow).
    pub fn mint_from_usdc(env: Env, user: Address, usdc_amount: i128, recipient: Address) -> i128 {
        Self::check_paused(&env);
        user.require_auth();

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

        let acbu_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            vec![&env],
        );

        let usdc_after_fee = calculate_amount_after_fee(usdc_amount, fee_rate);
        let acbu_amount = (usdc_after_fee * DECIMALS) / acbu_rate;

        let projected_supply = total_supply + acbu_amount;
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        total_supply += acbu_amount;
        env.storage().instance().set(&DATA_KEY.total_supply, &total_supply);

        let usdc_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        usdc_client.transfer(&user, &env.current_contract_address(), &usdc_amount);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        let fee = calculate_fee(usdc_amount, fee_rate);

        let tx_id = SorobanString::from_str(&env, "mint_tx_static");
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
    pub fn mint_from_basket(env: Env, user: Address, recipient: Address, acbu_amount: i128) -> i128 {
        Self::check_paused(&env);
        user.require_auth();

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

        let acbu_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            vec![&env],
        );

        let fee_acbu = calculate_fee(acbu_amount, fee_rate);
        let net_mint = acbu_amount - fee_acbu;
        let projected_supply = total_supply + acbu_amount;

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

        let usd_total: i128 = (acbu_amount * acbu_rate) / DECIMALS;

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

            let usd_i = (weight * usd_total) / BASIS_POINTS;
            let native_i = (usd_i * DECIMALS) / rate;
            if native_i > 0 {
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                token.transfer(&user, &vault, &native_i);
            }
        }

        total_supply += acbu_amount;
        env.storage().instance().set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &net_mint);
        if fee_acbu > 0 {
            acbu_sac.mint(&treasury, &fee_acbu);
        }

        let tx_id = SorobanString::from_str(&env, "mint_basket");
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

        net_mint
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

        let usd_gross = (s_token_amount * rate) / DECIMALS;
        if usd_gross < min_amount || usd_gross > max_amount {
            panic!("Invalid mint amount");
        }

        let usd_after_fee = calculate_amount_after_fee(usd_gross, fee_single);
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

        let token = soroban_sdk::token::Client::new(&env, &expected_stoken);
        token.transfer(&user, &vault, &s_token_amount);

        total_supply += acbu_amount;
        env.storage().instance().set(&DATA_KEY.total_supply, &total_supply);

        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        let fee = calculate_fee(usd_gross, fee_single);
        let mint_event = MintEvent {
            transaction_id: SorobanString::from_str(&env, "mint_single"),
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

    pub fn sync_supply(env: Env, new_supply: i128) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.total_supply, &new_supply);
    }

    pub fn get_total_supply(env: Env) -> i128 {
        env.storage().instance().get(&DATA_KEY.total_supply).unwrap_or(0)
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

    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    pub fn migrate(env: Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();

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
