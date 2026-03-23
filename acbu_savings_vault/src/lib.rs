#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub paused: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    paused: symbol_short!("PAUSED"),
};

#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositEvent {
    pub user: Address,
    pub amount: i128,
    pub term_seconds: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WithdrawEvent {
    pub user: Address,
    pub amount: i128,
    pub fee_amount: i128,
    pub yield_amount: i128,
    pub timestamp: u64,
}

#[contract]
pub struct SavingsVault;

#[contractimpl]
impl SavingsVault {
    /// Initialize the savings vault contract
    pub fn initialize(env: Env, admin: Address, acbu_token: Address, fee_rate_bps: i128) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }
        if fee_rate_bps < 0 || fee_rate_bps > 10_000 {
            panic!("Invalid fee rate");
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    /// Deposit (lock) ACBU for a term. User transfers ACBU to this contract.
    pub fn deposit(
        env: Env,
        user: Address,
        amount: i128,
        term_seconds: u64,
    ) -> Result<i128, soroban_sdk::Error> {
        let paused: bool = env.storage().instance().get(&DATA_KEY.paused).unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(1001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(1002));
        }
        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap().unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&user, &env.current_contract_address(), &amount);
        let key = (user.clone(), term_seconds);
        let existing: i128 = env.storage().temporary().get(&key).unwrap_or(0);
        env.storage().temporary().set(&key, &(existing + amount));
        env.events().publish(
            (symbol_short!("Deposit"), user.clone()),
            DepositEvent {
                user,
                amount,
                term_seconds,
                timestamp: env.ledger().timestamp(),
            },
        );
        Ok(existing + amount)
    }

    /// Withdraw (unlock) ACBU after term. Applies the stored protocol fee.
    pub fn withdraw(env: Env, user: Address, term_seconds: u64, amount: i128) -> Result<(), soroban_sdk::Error> {
        let paused: bool = env.storage().instance().get(&DATA_KEY.paused).unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(1001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(1002));
        }
        user.require_auth();
        let key = (user.clone(), term_seconds);
        let balance: i128 = env.storage().temporary().get(&key).ok_or(soroban_sdk::Error::from_contract_error(1003))?;
        if balance < amount {
            return Err(soroban_sdk::Error::from_contract_error(1004));
        }
        env.storage().temporary().set(&key, &(balance - amount));

        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap_or(0);
        let fee_amount: i128 = (amount * fee_rate) / 10_000;
        let net_amount: i128 = amount - fee_amount;

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap().unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&env.current_contract_address(), &user, &net_amount);
        if fee_amount > 0 {
            let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap().unwrap();
            client.transfer(&env.current_contract_address(), &admin, &fee_amount);
        }

        env.events().publish(
            (symbol_short!("Withdraw"), user.clone()),
            WithdrawEvent {
                user,
                amount,
                fee_amount,
                yield_amount: 0,
                timestamp: env.ledger().timestamp(),
            },
        );
        Ok(())
    }

    /// Get balance for user and term
    pub fn get_balance(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (user, term_seconds);
        env.storage().temporary().get(&key).unwrap_or(0)
    }

    pub fn pause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap().unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap().unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }
}
