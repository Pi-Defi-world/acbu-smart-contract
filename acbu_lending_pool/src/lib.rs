#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

use shared::{DataKey as SharedDataKey, BASIS_POINTS, CONTRACT_VERSION};

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

const VERSION: u32 = CONTRACT_VERSION;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoanId(pub Address, pub u64);

#[contracttype]
#[derive(Clone, Debug)]
pub struct LoanData {
    pub borrower: Address,
    pub amount: i128,
    pub collateral_amount: i128,
    pub start_timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BorrowEvent {
    pub creator: Address,
    pub amount: i128,
    pub token: Address,
    pub loan_id: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RepayEvent {
    pub creator: Address,
    pub amount: i128,
    pub token: Address,
    pub loan_id: u64,
    pub timestamp: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotFound = 1,
    InvalidState = 2,
    Unauthorized = 3,
    AlreadyInitialized = 4,
    InvalidAmount = 5,
    Paused = 2001,
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    pub fn initialize(env: Env, admin: Address, acbu_token: Address, fee_rate_bps: i128) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(Error::AlreadyInitialized);
        }
        if fee_rate_bps < 0 || fee_rate_bps > BASIS_POINTS {
            env.panic_with_error(Error::InvalidAmount);
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &VERSION);
    }

    pub fn deposit(env: Env, lender: Address, amount: i128) {
        lender.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&lender, &env.current_contract_address(), &amount);

        env.events()
            .publish((symbol_short!("deposit"), lender), amount);
    }

    pub fn withdraw(env: Env, lender: Address, amount: i128) {
        lender.require_auth();

        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&env.current_contract_address(), &lender, &amount);

        env.events()
            .publish((symbol_short!("withdraw"), lender), amount);
    }

    pub fn pause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.paused, &true);
    }

    pub fn unpause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);
        Self::check_not_paused(&env);

        let current_version = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        if new_version <= current_version {
            panic!("Invalid version upgrade");
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Run migrations
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

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }

    fn check_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            env.panic_with_error(Error::Paused);
        }
    }
}

fn migrate_v0_to_v1(_env: Env) {}
