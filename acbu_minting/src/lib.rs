#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, Env,
    IntoVal, String as SorobanString, Symbol,
};


use shared::{
    calculate_amount_after_fee, calculate_fee, MintEvent, BASIS_POINTS, DECIMALS, MAX_MINT_AMOUNT,
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
    pub fee_rate: Symbol,
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
    fee_rate: symbol_short!("FEE_RATE"),
    paused: symbol_short!("PAUSED"),
    min_mint_amount: symbol_short!("MIN_MINT"),
    max_mint_amount: symbol_short!("MAX_MINT"),
    total_supply: symbol_short!("SUPPLY"),
    version: symbol_short!("VERSION"),
};

const VERSION: u32 = 1;


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
        env.storage().instance().set(&DATA_KEY.total_supply, &0i128);
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Mint ACBU from USDC deposit.
    ///
    /// Fetches the live ACBU/USD basket rate from the oracle contract and verifies that
    /// reserves remain adequate via the reserve tracker before any tokens are minted.
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

        // Get contract configuration
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

        // --- Oracle integration ---
        // Call oracle.get_acbu_usd_rate() cross-contract to get the live ACBU/USD basket rate.
        let acbu_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            vec![&env],
        );

        // Calculate ACBU amount using the live rate
        let usdc_after_fee = calculate_amount_after_fee(usdc_amount, fee_rate);
        let acbu_amount = (usdc_after_fee * DECIMALS) / acbu_rate;

        // --- Reserve-tracker integration ---
        // Verify reserves against projected post-mint supply.
        let projected_supply = total_supply + acbu_amount;
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        // Update tracking
        total_supply += acbu_amount;
        env.storage().instance().set(&DATA_KEY.total_supply, &total_supply);

        // Transfer USDC from user to contract
        let usdc_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        usdc_client.transfer(&user, &env.current_contract_address(), &usdc_amount);

        // Mint ACBU to recipient
        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

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

    /// Mint ACBU from fiat deposit (via fintech partner).
    ///
    /// Fetches the live ACBU/USD rate from the oracle and verifies reserve adequacy before minting.
    pub fn mint_from_fiat(
        env: Env,
        admin: Address,
        _currency: SorobanString,
        amount: i128,
        recipient: Address,
        fintech_tx_id: SorobanString,
    ) -> i128 {
        Self::check_paused(&env);
        admin.require_auth();
        Self::check_admin(&env, &admin);

        // Get contract configuration
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
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

        // --- Oracle integration ---
        // Fetch the live ACBU/USD basket rate from the oracle contract.
        let acbu_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, "get_acbu_usd_rate"),
            vec![&env],
        );

        // For fiat mints the `amount` is already expressed as USD-equivalent in 7-decimal units.
        let usd_value = (amount * acbu_rate) / DECIMALS;

        // Same min/max bounds as `mint_from_usdc` on USD-equivalent notional (7-decimal fixed point)
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
        if usd_value < min_amount || usd_value > max_amount {
            panic!("Invalid mint amount");
        }

        // Calculate ACBU amount
        let usd_after_fee = calculate_amount_after_fee(usd_value, fee_rate);
        let acbu_amount = (usd_after_fee * DECIMALS) / acbu_rate;

        let used_key = (symbol_short!("USED_TX"), fintech_tx_id.clone());
        if env.storage().persistent().has(&used_key) {
            panic!("Duplicate fintech_tx_id");
        }

        // Mark the tx_id as used before minting (checks-effects-interactions pattern)
        env.storage().persistent().set(&used_key, &true);

        // --- Reserve-tracker integration ---
        let projected_supply = total_supply + acbu_amount;
        let reserve_ok: bool = env.invoke_contract(
            &reserve_tracker_addr,
            &Symbol::new(&env, "is_reserve_sufficient"),
            vec![&env, projected_supply.into_val(&env)],
        );
        if !reserve_ok {
            panic!("Insufficient reserves: minting would violate the minimum collateral ratio");
        }

        // Update tracking
        total_supply += acbu_amount;
        env.storage().instance().set(&DATA_KEY.total_supply, &total_supply);

        // Mint ACBU to recipient
        let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
        acbu_sac.mint(&recipient, &acbu_amount);

        // Calculate fee
        let fee = calculate_fee(usd_value, fee_rate);

        // Emit MintEvent
        let mint_event = MintEvent {
            transaction_id: fintech_tx_id,
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

    /// Update the internal total supply counter (admin only).
    /// Used to synchronize if tokens are burned or minted through other contracts.
    pub fn sync_supply(env: Env, new_supply: i128) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.total_supply, &new_supply);
    }

    /// Get the tracked total supply.
    pub fn get_total_supply(env: Env) -> i128 {
        env.storage().instance().get(&DATA_KEY.total_supply).unwrap_or(0)
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

