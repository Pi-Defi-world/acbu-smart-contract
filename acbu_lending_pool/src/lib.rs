#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

use shared::BASIS_POINTS;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub treasury: Symbol,
    pub paused: Symbol,
    pub version: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    treasury: symbol_short!("TRSY"),
    paused: symbol_short!("PAUSED"),
    version: symbol_short!("VERSION"),
};

const VERSION: u32 = 1;

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
    pub fee: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FeeCollectedEvent {
    pub treasury: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    /// Initialize the lending pool contract
pub fn initialize(env: Env, admin: Address, acbu_token: Address, fee_rate_bps: i128, treasury: Address) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }
        if fee_rate_bps < 0 || fee_rate_bps > BASIS_POINTS {
            panic!("Invalid fee rate");
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.treasury, &treasury);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }
        if fee_rate_bps < 0 || fee_rate_bps > BASIS_POINTS {
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
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Deposit ACBU into the pool (lender supplies liquidity)
    pub fn deposit(env: Env, lender: Address, amount: i128) -> Result<i128, soroban_sdk::Error> {
        // Auth first: caller must be the lender themselves
        lender.require_auth();
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
        let balance: i128 = env
            .storage()
            .temporary()
            .get(&lender)
            .ok_or(soroban_sdk::Error::from_contract_error(2003))?;
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

    /// Borrow ACBU from the pool
    pub fn borrow(
        env: Env,
        borrower: Address,
        amount: i128,
        collateral_amount: i128,
        loan_id: u64,
    ) -> Result<(), soroban_sdk::Error> {
        borrower.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(2001));
        }

        let key = LoanId(borrower.clone(), loan_id);
        if env.storage().temporary().has(&key) {
            return Err(soroban_sdk::Error::from_contract_error(2005));
        }

        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);

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
            (symbol_short!("loan_crt"), borrower.clone()),
            LoanCreatedEvent {
                loan_id,
                lender: env.current_contract_address(),
                borrower,
                amount,
                interest_bps: 0, // MVP
                term_seconds: 0, // MVP
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    /// Repay a loan
    pub fn repay(
        env: Env,
        borrower: Address,
        amount: i128,
        loan_id: u64,
    ) -> Result<(), soroban_sdk::Error> {
        borrower.require_auth();

        let key = LoanId(borrower.clone(), loan_id);
        let mut loan: LoanData = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(soroban_sdk::Error::from_contract_error(2003))?;

        if amount > loan.amount {
            return Err(soroban_sdk::Error::from_contract_error(2006));
        }

        let fee_rate: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .unwrap_or(0);
        let treasury: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.treasury)
            .unwrap_or(env.current_contract_address());
        let acbu: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        let fee = (amount * fee_rate) / 10_000;
        let net_amount = amount - fee;

        client.transfer(&borrower, &env.current_contract_address(), &net_amount);
        if fee > 0 {
            client.transfer(&borrower, &treasury, &fee);

            env.events().publish(
                (symbol_short!("fee_col"),),
                FeeCollectedEvent {
                    treasury,
                    amount: fee,
                    timestamp: env.ledger().timestamp(),
                },
            );
        }

        loan.amount -= amount;
        if loan.amount == 0 {
            env.storage().temporary().remove(&key);
        } else {
            env.storage().temporary().set(&key, &loan);
        }

        env.events().publish(
            (symbol_short!("repay"), borrower.clone()),
            LoanRepaidEvent {
                loan_id,
                borrower,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
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
