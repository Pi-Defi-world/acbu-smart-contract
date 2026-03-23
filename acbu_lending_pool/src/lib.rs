#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

// Storage Keys

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub paused: Symbol,
    pub pool_liquidity: Symbol,
    pub loan_counter: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    paused: symbol_short!("PAUSED"),
    pool_liquidity: symbol_short!("POOL_LIQ"),
    loan_counter: symbol_short!("LOAN_CTR"),
};

// Domain Types

#[contracttype]
#[derive(Clone, Debug)]
pub struct Loan {
    pub id: u64,
    pub borrower: Address,
    pub principal: i128,      // ACBU amount, 7-decimal fixed-point
    pub interest_bps: i128,   // flat rate agreed at creation, in basis points
    pub term_seconds: u64,    // intended duration
    pub start_timestamp: u64, // ledger timestamp at loan creation
    pub repaid: bool,
}

// Events

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

// Contract

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    // Lifecycle

    pub fn initialize(env: Env, admin: Address, acbu_token: Address, fee_rate_bps: i128) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }
        if !(0..=10_000).contains(&fee_rate_bps) {
            panic!("Invalid fee rate");
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
            .set(&DATA_KEY.pool_liquidity, &0i128);
        env.storage().instance().set(&DATA_KEY.loan_counter, &0u64);
    }

    // Lender Interface

    pub fn deposit(env: Env, lender: Address, amount: i128) -> Result<i128, soroban_sdk::Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(2002));
        }
        lender.require_auth();

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        soroban_sdk::token::Client::new(&env, &acbu).transfer(
            &lender,
            &env.current_contract_address(),
            &amount,
        );

        let existing: i128 = env.storage().temporary().get(&lender).unwrap_or(0);
        env.storage().temporary().set(&lender, &(existing + amount));

        let liq: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pool_liquidity)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DATA_KEY.pool_liquidity, &(liq + amount));

        Ok(existing + amount)
    }

    pub fn withdraw(env: Env, lender: Address, amount: i128) -> Result<(), soroban_sdk::Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(2002));
        }
        lender.require_auth();

        let balance: i128 = env
            .storage()
            .temporary()
            .get(&lender)
            .ok_or(soroban_sdk::Error::from_contract_error(2003))?;

        if balance < amount {
            return Err(soroban_sdk::Error::from_contract_error(2004));
        }

        let liq: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pool_liquidity)
            .unwrap_or(0);
        if liq < amount {
            return Err(soroban_sdk::Error::from_contract_error(2004));
        }

        // Calculate protocol fee
        let fee_rate: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .unwrap_or(0);
        let fee = (amount * fee_rate) / 10_000;
        let net_amount = amount - fee;

        env.storage().temporary().set(&lender, &(balance - amount));
        env.storage()
            .instance()
            .set(&DATA_KEY.pool_liquidity, &(liq - amount));

        let acbu_addr: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let acbu = soroban_sdk::token::Client::new(&env, &acbu_addr);

        // Transfer net amount to lender
        acbu.transfer(&env.current_contract_address(), &lender, &net_amount);

        // Transfer fee to admin
        if fee > 0 {
            let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
            acbu.transfer(&env.current_contract_address(), &admin, &fee);
        }

        Ok(())
    }

    pub fn get_balance(env: Env, lender: Address) -> i128 {
        env.storage().temporary().get(&lender).unwrap_or(0)
    }

    // Borrower Interface

    pub fn borrow(
        env: Env,
        borrower: Address,
        amount: i128,
        interest_bps: i128,
        term_seconds: u64,
    ) -> Result<u64, soroban_sdk::Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(2002));
        }
        if !(0..=10_000).contains(&interest_bps) {
            return Err(soroban_sdk::Error::from_contract_error(2005));
        }
        if term_seconds == 0 {
            return Err(soroban_sdk::Error::from_contract_error(2006));
        }

        borrower.require_auth();

        let liq: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pool_liquidity)
            .unwrap_or(0);
        if liq < amount {
            return Err(soroban_sdk::Error::from_contract_error(2007));
        }

        let loan_counter: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.loan_counter)
            .unwrap_or(0);
        let loan_id = loan_counter + 1;
        env.storage()
            .instance()
            .set(&DATA_KEY.loan_counter, &loan_id);

        let now = env.ledger().timestamp();

        let loan = Loan {
            id: loan_id,
            borrower: borrower.clone(),
            principal: amount,
            interest_bps,
            term_seconds,
            start_timestamp: now,
            repaid: false,
        };
        env.storage()
            .persistent()
            .set(&(symbol_short!("LOAN"), loan_id), &loan);

        env.storage()
            .instance()
            .set(&DATA_KEY.pool_liquidity, &(liq - amount));

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        soroban_sdk::token::Client::new(&env, &acbu).transfer(
            &env.current_contract_address(),
            &borrower,
            &amount,
        );

        env.events().publish(
            (symbol_short!("borrow"), borrower.clone()),
            LoanCreatedEvent {
                lender: env.current_contract_address(),
                borrower,
                amount,
                interest_bps,
                term_seconds,
                timestamp: now,
            },
        );

        Ok(loan_id)
    }

    pub fn repay(env: Env, loan_id: u64) -> Result<(), soroban_sdk::Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }

        let loan_key = (symbol_short!("LOAN"), loan_id);
        let loan: Loan = env
            .storage()
            .persistent()
            .get(&loan_key)
            .ok_or(soroban_sdk::Error::from_contract_error(2008))?;

        if loan.repaid {
            return Err(soroban_sdk::Error::from_contract_error(2009));
        }

        loan.borrower.require_auth();

        let interest = (loan.principal * loan.interest_bps) / 10_000;
        let total_repayment = loan.principal + interest;

        let mut settled = loan.clone();
        settled.repaid = true;
        env.storage().persistent().set(&loan_key, &settled);

        let liq: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pool_liquidity)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DATA_KEY.pool_liquidity, &(liq + total_repayment));

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        soroban_sdk::token::Client::new(&env, &acbu).transfer(
            &loan.borrower,
            &env.current_contract_address(),
            &total_repayment,
        );

        env.events().publish(
            (symbol_short!("repay"), loan.borrower.clone()),
            RepaymentEvent {
                borrower: loan.borrower,
                amount: total_repayment,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    pub fn get_loan(env: Env, loan_id: u64) -> Option<Loan> {
        env.storage()
            .persistent()
            .get(&(symbol_short!("LOAN"), loan_id))
    }

    pub fn get_pool_liquidity(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DATA_KEY.pool_liquidity)
            .unwrap_or(0)
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
}
