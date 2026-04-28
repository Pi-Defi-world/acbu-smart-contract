#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Map,
    Symbol,
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

const LENDER_BALANCES: Symbol = symbol_short!("LDBALS");
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
    NotFound = 4001,
    InvalidState = 4002,
    Unauthorized = 4003,
    AlreadyInitialized = 4004,
    InvalidAmount = 4005,
    Paused = 4006,
    InsufficientBalance = 4007,
    InvalidVersion = 4008,
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
            .set(&LENDER_BALANCES, &Map::<Address, i128>::new(&env));
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &VERSION);
    }

    /// Tracked deposit balance for a lender (ACBU units in the pool ledger).
    pub fn get_balance(env: Env, lender: Address) -> i128 {
        Self::lender_balance(&env, &lender)
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

        let mut bals: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&LENDER_BALANCES)
            .unwrap_or_else(|| Map::new(&env));
        let cur = bals.get(lender.clone()).unwrap_or(0);
        bals.set(
            lender.clone(),
            cur.checked_add(amount).expect("overflow lender balance"),
        );
        env.storage().instance().set(&LENDER_BALANCES, &bals);

        env.events()
            .publish((symbol_short!("deposit"), lender), amount);
    }

    pub fn withdraw(env: Env, lender: Address, amount: i128) {
        lender.require_auth();

        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let bal = Self::lender_balance(&env, &lender);
        if bal < amount {
            env.panic_with_error(Error::InsufficientBalance);
        }

        let mut bals: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&LENDER_BALANCES)
            .unwrap_or_else(|| Map::new(&env));
        bals.set(lender.clone(), bal - amount);
        env.storage().instance().set(&LENDER_BALANCES, &bals);

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&env.current_contract_address(), &lender, &amount);

        env.events()
            .publish((symbol_short!("withdraw"), lender), amount);
    }

    pub fn borrow(env: Env, borrower: Address, amount: i128, collateral_amount: i128, loan_id: u64) {
        borrower.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        let pool_balance = token.balance(&env.current_contract_address());
        if pool_balance < amount {
            env.panic_with_error(Error::InsufficientBalance);
        }

        let loan_key = LoanId(borrower.clone(), loan_id);
        if env.storage().instance().has(&loan_key) {
            env.panic_with_error(Error::InvalidState);
        }

        let loan = LoanData {
            borrower: borrower.clone(),
            amount,
            collateral_amount,
            start_timestamp: env.ledger().timestamp(),
        };
        env.storage().instance().set(&loan_key, &loan);

        token.transfer(&env.current_contract_address(), &borrower, &amount);

        env.events().publish(
            (symbol_short!("borrow"), borrower.clone()),
            BorrowEvent {
                creator: borrower.clone(),
                amount,
                token: acbu_token.clone(),
                loan_id,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    pub fn repay(env: Env, borrower: Address, amount: i128, loan_id: u64) {
        borrower.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let loan_key = LoanId(borrower.clone(), loan_id);
        let loan: LoanData = match env.storage().instance().get(&loan_key) {
            Some(l) => l,
            None => env.panic_with_error(Error::NotFound),
        };

        if amount > loan.amount {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&borrower, &env.current_contract_address(), &amount);

        let new_amount = loan.amount - amount;
        if new_amount == 0 {
            env.storage().instance().remove(&loan_key);
        } else {
            env.storage().instance().set(
                &loan_key,
                &LoanData {
                    borrower: loan.borrower,
                    amount: new_amount,
                    collateral_amount: loan.collateral_amount,
                    start_timestamp: loan.start_timestamp,
                },
            );
        }

        env.events().publish(
            (symbol_short!("repay"), borrower.clone()),
            RepayEvent {
                creator: borrower,
                amount,
                token: acbu_token,
                loan_id,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    pub fn get_loan(env: Env, borrower: Address, loan_id: u64) -> Option<LoanData> {
        let loan_key = LoanId(borrower, loan_id);
        env.storage().instance().get(&loan_key)
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
            env.panic_with_error(Error::InvalidVersion);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

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

    fn lender_balance(env: &Env, lender: &Address) -> i128 {
        let bals: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&LENDER_BALANCES)
            .unwrap_or_else(|| Map::new(env));
        bals.get(lender.clone()).unwrap_or(0)
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
