#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};


#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub paused: Symbol,
    pub version: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    paused: symbol_short!("PAUSED"),
    version: symbol_short!("VERSION"),
};

const VERSION: u32 = 1;


#[contracttype]
#[derive(Clone, Debug)]
pub struct LoanCreatedEvent {
    pub lender: Address,
    pub borrower: Address,
    pub amount: i128,
    pub interest_bps: i128,
    pub term_seconds: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RepaymentEvent {
    pub borrower: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    /// Initialize the lending pool contract
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
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Deposit ACBU into the pool (lender supplies liquidity)
    pub fn deposit(env: Env, lender: Address, amount: i128) -> Result<i128, soroban_sdk::Error> {
        // Auth first: caller must be the lender themselves
        lender.require_auth();
        let paused: bool = env.storage().instance().get(&DATA_KEY.paused).unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(2002));
        }
        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&lender, &env.current_contract_address(), &amount);
        let existing: i128 = env.storage().temporary().get(&lender).unwrap_or(0);
        env.storage().temporary().set(&lender, &(existing + amount));
        Ok(existing + amount)
    }

    /// Withdraw ACBU from the pool
    pub fn withdraw(env: Env, lender: Address, amount: i128) -> Result<(), soroban_sdk::Error> {
        // Auth first: caller must be the lender themselves
        lender.require_auth();
        let paused: bool = env.storage().instance().get(&DATA_KEY.paused).unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(2002));
        }
        let balance: i128 = env.storage().temporary().get(&lender).ok_or(soroban_sdk::Error::from_contract_error(2003))?;
        if balance < amount {
            return Err(soroban_sdk::Error::from_contract_error(2004));
        }
        env.storage().temporary().set(&lender, &(balance - amount));
        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&env.current_contract_address(), &lender, &amount);
        Ok(())
    }

    /// Get lender balance
    pub fn get_balance(env: Env, lender: Address) -> i128 {
        env.storage().temporary().get(&lender).unwrap_or(0)
    }

    pub fn pause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
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

