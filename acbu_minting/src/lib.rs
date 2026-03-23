#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String as SorobanString,
    Symbol,
};

use shared::{
    calculate_amount_after_fee, calculate_fee, MintEvent, BASIS_POINTS, DECIMALS, MAX_MINT_AMOUNT,
    MIN_MINT_AMOUNT,
};

mod shared {
    pub use shared::*;
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub oracle: Symbol,
    pub reserve_tracker: Symbol,
    pub acbu_token: Symbol,
    pub usdc_token: Symbol,
    pub fee_rate: Symbol,
    pub paused: Symbol,
    pub min_mint_amount: Symbol,
    pub max_mint_amount: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserve_tracker: symbol_short!("RES_TRK"),
    acbu_token: symbol_short!("ACBU_TKN"),
    usdc_token: symbol_short!("USDC_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    paused: symbol_short!("PAUSED"),
    min_mint_amount: symbol_short!("MIN_MINT"),
    max_mint_amount: symbol_short!("MAX_MINT"),
};

#[contract]
pub struct MintingContract;

#[contractimpl]
impl MintingContract {
    /// Initialize the minting contract
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        reserve_tracker: Address,
        acbu_token: Address,
        usdc_token: Address,
        fee_rate_bps: i128,
    ) {
        // Check if already initialized
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }

        // Validate inputs
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            panic!("Invalid fee rate");
        }

        // Store configuration
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
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_mint_amount, &MIN_MINT_AMOUNT);
        env.storage()
            .instance()
            .set(&DATA_KEY.max_mint_amount, &MAX_MINT_AMOUNT);
    }

    /// Mint ACBU from USDC deposit
    pub fn mint_from_usdc(env: Env, user: Address, usdc_amount: i128, recipient: Address) -> i128 {
        Self::check_paused(&env);
        user.require_auth();

        // Validate amount
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

        // Get contract addresses
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let usdc_token: Address = env.storage().instance().get(&DATA_KEY.usdc_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();

        // Get ACBU/USD rate from oracle
        let acbu_rate = DECIMALS; // 1:1 with USD initially

        // Calculate ACBU amount
        let usdc_after_fee = calculate_amount_after_fee(usdc_amount, fee_rate);
        let acbu_amount = (usdc_after_fee * DECIMALS) / acbu_rate;

        // Transfer USDC from user to contract
        let usdc_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        usdc_client.transfer(&user, &env.current_contract_address(), &usdc_amount);

        // Mint ACBU to recipient
        let acbu_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_admin_client.mint(&recipient, &acbu_amount);

        // Calculate fee
        let fee = calculate_fee(usdc_amount, fee_rate);

        // Emit MintEvent
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

    /// Mint ACBU from fiat deposit (via fintech partner)
    pub fn mint_from_fiat(
        env: Env,
        admin: Address,
        _currency: SorobanString,
        amount: i128,
        recipient: Address,
        _fintech_tx_id: SorobanString,
    ) -> i128 {
        Self::check_paused(&env);
        admin.require_auth();
        Self::check_admin(&env, &admin);

        // Validate amount
        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_mint_amount)
            .unwrap();
        if amount < min_amount {
            panic!("Invalid mint amount");
        }

        // Get contract addresses
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();

        // Get currency/USD rate from oracle
        let currency_rate = DECIMALS; // 1:1 with USD initially

        // Convert fiat amount to USD
        let usd_value = (amount * currency_rate) / DECIMALS;

        // Get ACBU/USD rate
        let acbu_rate = DECIMALS; // 1:1 with USD initially

        // Calculate ACBU amount
        let usd_after_fee = calculate_amount_after_fee(usd_value, fee_rate);
        let acbu_amount = (usd_after_fee * DECIMALS) / acbu_rate;

        // Mint ACBU to recipient
        let acbu_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_admin_client.mint(&recipient, &acbu_amount);

        // Calculate fee
        let fee = calculate_fee(usd_value, fee_rate);

        // Emit MintEvent
        let tx_id = SorobanString::from_str(&env, "mint_fiat_tx");
        let mint_event = MintEvent {
            transaction_id: tx_id,
            user: recipient.clone(),
            usdc_amount: usd_value,
            acbu_amount,
            fee,
            rate: acbu_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("mint"), recipient), mint_event);

        acbu_amount
    }

    /// Pause the contract (admin only)
    pub fn pause(env: Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
    }

    /// Unpause the contract (admin only)
    pub fn unpause(env: Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    /// Set fee rate (admin only)
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

    /// Get current fee rate
    pub fn get_fee_rate(env: Env) -> i128 {
        env.storage().instance().get(&DATA_KEY.fee_rate).unwrap()
    }

    /// Check if contract is paused
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false)
    }

    // Private helper functions
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

    fn check_admin(env: &Env, admin_to_check: &Address) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        if admin != *admin_to_check {
            panic!("Unauthorized: admin only");
        }
    }
}
