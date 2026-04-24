#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
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

// CONTRACT_VERSION is imported from shared
// Use shared version key to avoid drift
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

// Renamed fields to match spec: creator=borrower, amount, token
#[contracttype]
#[derive(Clone, Debug)]
pub struct BorrowEvent {
    pub creator: Address, // borrower is the creator in lending context
    pub amount: i128,
    pub token: Address,
    pub loan_id: u64,
    pub timestamp: u64,
}

// Renamed fields to match spec: creator=borrower, amount, token
#[contracttype]
#[derive(Clone, Debug)]
pub struct RepayEvent {
    pub creator: Address,
    pub amount: i128,
    pub token: Address,
    pub loan_id: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct LoanCreatedEvent {
    pub loan_id: u64,
    pub lender: Address,
    pub borrower: Address,
    pub amount: i128,
    pub interest_bps: i128,
    pub term_seconds: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct LoanRepaidEvent {
    pub loan_id: u64,
    pub borrower: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RepaymentEvent {
    pub borrower: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotFound = 1,
    InvalidState = 2,
    Unauthorized = 3,
    AlreadyInitialized = 4,
    Paused = 2001,
    InvalidAmount = 2002,
    InsufficientBalance = 2004,
    LoanAlreadyExists = 2005,
    InvalidRepaymentAmount = 2006,

    TokenError = 2007,
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    /// Initialize the lending pool contract
    pub fn initialize(
        env: Env,
        admin: Address,
        acbu_token: Address,
        fee_rate_bps: i128,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DATA_KEY.admin) {
            return Err(Error::AlreadyInitialized);
        }
        if fee_rate_bps < 0 || fee_rate_bps > BASIS_POINTS {
            return Err(Error::InvalidAmount);
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
        Ok(())
    }

    /// Deposit ACBU into the pool (lender supplies liquidity)
    pub fn deposit(env: Env, lender: Address, amount: i128) -> Result<i128, Error> {
        // Auth first: caller must be the lender themselves
        lender.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(Error::Paused);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let acbu: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(Error::NotFound)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&lender, &env.current_contract_address(), &amount);
        let existing: i128 = env.storage().temporary().get(&lender).unwrap_or(0);
        env.storage().temporary().set(&lender, &(existing + amount));
        Ok(existing + amount)
    }

    /// Withdraw ACBU from the pool
    pub fn withdraw(env: Env, lender: Address, amount: i128) -> Result<(), Error> {
        // Auth first: caller must be the lender themselves
        lender.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(Error::Paused);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let balance: i128 = env
            .storage()
            .temporary()
            .get(&lender)
            .ok_or(Error::NotFound)?;
        if balance < amount {
            return Err(Error::InsufficientBalance);
        }
        env.storage().temporary().set(&lender, &(balance - amount));
        let acbu: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(Error::NotFound)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&env.current_contract_address(), &lender, &amount);
        Ok(())
    }

    /// Get lender balance
    pub fn get_balance(env: Env, lender: Address) -> i128 {
        env.storage().temporary().get(&lender).unwrap_or(0)
    }

    /// Borrow ACBU from the pool
    pub fn borrow(
        env: Env,
        borrower: Address,
        amount: i128,
        collateral_amount: i128,
        loan_id: u64,
    ) -> Result<(), Error> {
        borrower.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(Error::Paused);
        }

        let key = LoanId(borrower.clone(), loan_id);
        if env.storage().temporary().has(&key) {
            return Err(Error::LoanAlreadyExists);
        }

        let acbu: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(Error::NotFound)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // Explicit liquidity guard: reject before attempting the transfer so the
        // caller receives a clean Error::InsufficientBalance rather than a token
        // panic when the pool does not hold enough ACBU.
        let available: i128 = client.balance(&env.current_contract_address());
        if amount > available {
            return Err(Error::InsufficientBalance);
        }

        // In MVP, we just transfer ACBU to borrower.
        // Real logic would check collateral value via oracle.
        client.transfer(&env.current_contract_address(), &borrower, &amount);

        let loan = LoanData {
            borrower: borrower.clone(),
            amount,
            collateral_amount,
            start_timestamp: env.ledger().timestamp(),
        };

        env.storage().temporary().set(&key, &loan);

        env.events().publish(
            (symbol_short!("borrow"),),
            BorrowEvent {
                creator: borrower.clone(),
                amount,
                token: acbu,
                loan_id,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    /// Repay a loan
    pub fn repay(env: Env, borrower: Address, amount: i128, loan_id: u64) -> Result<(), Error> {
        borrower.require_auth();

        let key = LoanId(borrower.clone(), loan_id);
        let mut loan: LoanData = env.storage().temporary().get(&key).ok_or(Error::NotFound)?;

        if amount > loan.amount {
            return Err(Error::InvalidRepaymentAmount);
        }

        let acbu: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(Error::NotFound)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&borrower, &env.current_contract_address(), &amount);

        loan.amount -= amount;
        if loan.amount == 0 {
            env.storage().temporary().remove(&key);
        } else {
            env.storage().temporary().set(&key, &loan);
        }

        env.events().publish(
            (symbol_short!("repay"),),
            RepayEvent {
                creator: borrower.clone(),
                amount,
                token: acbu,
                loan_id,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    // Getter for loan state so integration test can verify transitions
    pub fn get_loan(env: Env, borrower: Address, loan_id: u64) -> Option<LoanData> {
        let key = LoanId(borrower, loan_id);
        env.storage().temporary().get(&key)
    }

    pub fn pause(env: Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn migrate(env: Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();

        let current_version = Self::get_version(env.clone());
        if VERSION <= current_version {
            panic!("Invalid version upgrade");
        }
        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }
}

fn migrate_v0_to_v1(_env: Env) {
    // Migration logic for v0 to v1 if needed
}
