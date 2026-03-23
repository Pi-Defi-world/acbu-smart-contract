#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String as SorobanString,
    Symbol, Vec,
};

use shared::{
    calculate_amount_after_fee, calculate_fee, AccountDetails, BurnEvent, CurrencyCode,
    BASIS_POINTS, DECIMALS, MIN_BURN_AMOUNT,
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
    pub withdrawal_processor: Symbol,
    pub fee_rate: Symbol,
    pub paused: Symbol,
    pub min_burn_amount: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserve_tracker: symbol_short!("RES_TRK"),
    acbu_token: symbol_short!("ACBU_TKN"),
    withdrawal_processor: symbol_short!("WD_PROC"),
    fee_rate: symbol_short!("FEE_RATE"),
    paused: symbol_short!("PAUSED"),
    min_burn_amount: symbol_short!("MIN_BURN"),
};

#[contract]
pub struct BurningContract;

#[contractimpl]
impl BurningContract {
    /// Initialize the burning contract
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        reserve_tracker: Address,
        acbu_token: Address,
        withdrawal_processor: Address,
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
            .set(&DATA_KEY.withdrawal_processor, &withdrawal_processor);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_burn_amount, &MIN_BURN_AMOUNT);
    }

    /// Burn ACBU for single currency redemption
    pub fn burn_for_currency(
        env: Env,
        user: Address,
        acbu_amount: i128,
        currency: SorobanString,
        _recipient_account: AccountDetails,
    ) -> i128 {
        Self::check_paused(&env);
        user.require_auth();

        // Validate amount
        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            panic!("Invalid burn amount");
        }

        // Get contract addresses
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();

        // Get currency/USD rate from oracle
        let currency_code = CurrencyCode(currency.clone());
        let currency_rate = DECIMALS; // 1:1 with USD initially

        // Calculate local currency amount
        let acbu_after_fee = calculate_amount_after_fee(acbu_amount, fee_rate);
        let usd_value = (acbu_after_fee * DECIMALS) / DECIMALS; // Assuming 1:1 ACBU:USD
        let local_amount = (usd_value * DECIMALS) / currency_rate;

        // Burn ACBU from user
        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        // Calculate fee
        let fee = calculate_fee(acbu_amount, fee_rate);

        // Emit BurnEvent
        let tx_id = SorobanString::from_str(&env, "burn_tx_static");
        let burn_event = BurnEvent {
            transaction_id: tx_id,
            user: user.clone(),
            acbu_amount,
            local_amount,
            currency: currency_code,
            fee,
            rate: currency_rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("burn"), user), burn_event);

        local_amount
    }

    /// Burn ACBU for basket redemption (proportional)
    pub fn burn_for_basket(
        env: Env,
        user: Address,
        acbu_amount: i128,
        recipient_accounts: Vec<AccountDetails>,
    ) -> Vec<i128> {
        Self::check_paused(&env);
        user.require_auth();

        // Validate amount
        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            panic!("Invalid burn amount");
        }

        let num_recipients = recipient_accounts.len() as i128;
        if num_recipients == 0 {
            panic!("No recipient accounts provided");
        }

        // Get contract addresses
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();

        // Burn ACBU from user first
        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        // Calculate total fee and total net amount
        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let total_net_amount = acbu_amount - total_fee;

        // Calculate amounts per account, handling remainders
        let amount_per_account = total_net_amount / num_recipients;
        let fee_per_account = total_fee / num_recipients;
        let mut remainder_net = total_net_amount % num_recipients;
        let mut remainder_fee = total_fee % num_recipients;

        let mut local_amounts = Vec::new(&env);
        for i in 0..recipient_accounts.len() {
            let account = recipient_accounts.get(i).unwrap();

            let mut current_net = amount_per_account;
            if remainder_net > 0 {
                current_net += 1;
                remainder_net -= 1;
            }

            let mut current_fee = fee_per_account;
            if remainder_fee > 0 {
                current_fee += 1;
                remainder_fee -= 1;
            }

            // Get currency rate
            let currency_rate = DECIMALS; // 1:1 with USD initially
            let local_amount = (current_net * DECIMALS) / currency_rate;
            local_amounts.push_back(local_amount);

            // Emit BurnEvent for each currency
            let tx_id = SorobanString::from_str(&env, "burn_basket_tx");
            let burn_event = BurnEvent {
                transaction_id: tx_id,
                user: user.clone(),
                acbu_amount: current_net,
                local_amount,
                currency: account.currency.clone(),
                fee: current_fee,
                rate: currency_rate,
                timestamp: env.ledger().timestamp(),
            };
            env.events()
                .publish((symbol_short!("burn"), user.clone()), burn_event);
        }

        local_amounts
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

    /// Set fee rate (admin only)
    pub fn set_fee_rate(env: Env, fee_rate_bps: i128) {
        Self::check_admin(&env);
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

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}
