#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

use shared::{DataKey as SharedDataKey, BASIS_POINTS, CONTRACT_VERSION};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    AcbuToken,
    FeeRate,
    Paused,
    Balance(Address),
    Loan(LoanId),
}

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
    InsufficientBalance = 6,
    Paused = 2001,
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    pub fn initialize(env: Env, admin: Address, acbu_token: Address, fee_rate_bps: i128) {
        if env.storage().instance().has(&DataKey::Admin) {
            env.panic_with_error(Error::AlreadyInitialized);
        }
        if fee_rate_bps < 0 || fee_rate_bps > BASIS_POINTS {
            env.panic_with_error(Error::InvalidAmount);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::AcbuToken, &acbu_token);
        env.storage()
            .instance()
            .set(&DataKey::FeeRate, &fee_rate_bps);
        env.storage().instance().set(&DataKey::Paused, &false);
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

        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&lender, &env.current_contract_address(), &amount);

        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(lender.clone()))
            .unwrap_or(0);
        let new_balance = current_balance
            .checked_add(amount)
            .unwrap_or_else(|| env.panic_with_error(Error::InvalidAmount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(lender.clone()), &new_balance);

        env.events()
            .publish((symbol_short!("deposit"), lender), amount);
    }

    pub fn withdraw(env: Env, lender: Address, amount: i128) {
        lender.require_auth();

        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(lender.clone()))
            .unwrap_or(0);
        if current_balance < amount {
            env.panic_with_error(Error::InsufficientBalance);
        }

        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&env.current_contract_address(), &lender, &amount);

        let new_balance = current_balance
            .checked_sub(amount)
            .unwrap_or_else(|| env.panic_with_error(Error::InsufficientBalance));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(lender.clone()), &new_balance);

        env.events()
            .publish((symbol_short!("withdraw"), lender), amount);
    }

    pub fn get_balance(env: Env, lender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(lender))
            .unwrap_or(0)
    }

    pub fn borrow(
        env: Env,
        borrower: Address,
        amount: i128,
        collateral_amount: i128,
        loan_id: u64,
    ) {
        borrower.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let loan_key = LoanId(borrower.clone(), loan_id);
        if env.storage().persistent().has(&DataKey::Loan(loan_key.clone())) {
            env.panic_with_error(Error::InvalidState);
        }


        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        
        // Check if contract has enough balance
        let contract_balance = token.balance(&env.current_contract_address());
        if contract_balance < amount {
            env.panic_with_error(Error::InsufficientBalance);
        }
        
        // Pull collateral in BEFORE paying out the loan principal.
        token.transfer(&borrower, &env.current_contract_address(), &collateral_amount);
        token.transfer(&env.current_contract_address(), &borrower, &amount);

        let loan_data = LoanData {
            borrower: borrower.clone(),
            amount,
            collateral_amount,
            start_timestamp: env.ledger().timestamp(),
        };
        
        env.storage()
            .persistent()
            .set(&DataKey::Loan(loan_key), &loan_data);

        env.events().publish(
            (symbol_short!("borrow"), borrower.clone()),
            BorrowEvent {
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
        env.storage().persistent().get(&DataKey::Loan(loan_key))
    }

    pub fn repay(env: Env, borrower: Address, amount: i128, loan_id: u64) {
        borrower.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let loan_key = LoanId(borrower.clone(), loan_id);
        let mut loan_data: LoanData = env
            .storage()
            .persistent()
            .get(&DataKey::Loan(loan_key.clone()))
            .ok_or_else(|| Error::NotFound)
            .unwrap_or_else(|e| env.panic_with_error(e));

        if amount > loan_data.amount {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&borrower, &env.current_contract_address(), &amount);

        loan_data.amount -= amount;
        if loan_data.amount == 0 {
            // Return collateral on full repayment.
            if loan_data.collateral_amount > 0 {
                token.transfer(
                    &env.current_contract_address(),
                    &borrower,
                    &loan_data.collateral_amount,
                );
            }
            env.storage().persistent().remove(&DataKey::Loan(loan_key));
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Loan(loan_key), &loan_data);
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

    pub fn pause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &true);
    }

    pub fn unpause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &false);
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
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
    }

    fn check_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            env.panic_with_error(Error::Paused);
        }
    }
}

fn migrate_v0_to_v1(_env: Env) {}
