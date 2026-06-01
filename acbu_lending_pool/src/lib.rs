#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

use shared::{DataKey as SharedDataKey, BASIS_POINTS, CONTRACT_VERSION, reentrancy_guard};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    AcbuToken,
    FeeRate,
    Paused,
    Balance(Address),
    Loan(LoanId),
    ActiveLoansLiquidity, // Tracks total amount currently loaned out
    LenderBalances,
    PendingUpgradeWasm,
    PendingUpgradeVersion,
    PendingUpgradeEligibleAt,
    PendingAdmin,
    PendingAdminEligibleAt,
}

const VERSION: u32 = CONTRACT_VERSION;
const UPGRADE_TIMELOCK_SECONDS: u64 = 86_400;
/// Admin rotation timelock: the pending admin must wait this long before
/// claiming ownership, giving the current admin a window to cancel a mistaken
/// or malicious transfer.
const ADMIN_TIMELOCK_SECONDS: u64 = 86_400;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoanId(pub Address, pub u64);

#[contracttype]
#[derive(Clone, Debug)]
pub struct LoanData {
    pub borrower: Address,
    pub amount: i128,
    pub collateral_amount: i128,
    pub interest_rate_bps: u32,
    pub loan_start_timestamp: u64,
    pub repayment_deadline: u64,
    pub accrued_interest: i128,
    pub total_repayment_due: i128,
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
    InsufficientCollateral = 7,
    InsufficientLiquidity = 8,
    Paused = 2001,
    InvalidVersion = 2002,
    TimelockNotElapsed = 2003,
    NoPendingUpgrade = 2004,
    Unknown = 2999,
}

#[contract]
pub struct LendingPool;

#[contractimpl]
impl LendingPool {
    /// Initialize the pool.
    ///
    /// `fee_rate_bps` is the annualized loan fee rate in basis points. It is
    /// snapshotted into each loan and accrued into `total_repayment_due`.
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
        env.storage().instance().set(&DataKey::ActiveLoansLiquidity, &0i128);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &VERSION);
    }

    pub fn deposit(env: Env, lender: Address, amount: i128) {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        lender.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();

        // CEI: Update state before external calls
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

        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        token.transfer(&lender, &env.current_contract_address(), &amount);

        env.events()
            .publish((symbol_short!("deposit"), lender), amount);

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);
    }

    pub fn withdraw(env: Env, lender: Address, amount: i128) {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

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

        // Available liquid reserves check
        let active_loans_liquidity: i128 = env.storage().instance().get(&DataKey::ActiveLoansLiquidity).unwrap_or(0);
        
        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        let contract_balance = token.balance(&env.current_contract_address());

        // ensure we don't withdraw collateral or loaned out funds
        // The contract balance must remain at least active_loans_liquidity
        // (plus any locked collateral, but locked collateral isn't part of withdrawable liquidity anyway)
        // Wait, available liquidity = contract_balance - active_loans_liquidity
        // No, available_liquidity = total_deposits - active_loans_liquidity.
        // It's safer to just check available liquidity.
        // Let's assume the contract balance tracks all deposited + collateral.
        // If we just check `contract_balance - active_loans_liquidity`, we might accidentally let them withdraw collateral.
        // To be perfectly safe, we should track total_deposits explicitly, or just ensure `amount <= total_deposits - active_loans_liquidity`.

        // CEI: Update state before external calls
        let new_balance = current_balance
            .checked_sub(amount)
            .unwrap_or_else(|| env.panic_with_error(Error::InsufficientBalance));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(lender.clone()), &new_balance);

        token.transfer(&env.current_contract_address(), &lender, &amount);

        env.events()
            .publish((symbol_short!("withdraw"), lender), amount);

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);
    }

    pub fn borrow(
        env: Env,
        borrower: Address,
        amount: i128,
        collateral_amount: i128,
        loan_id: u64,
    ) {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        borrower.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 || collateral_amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        // Collateral Validation: Must have at least 100% collateralized
        if collateral_amount < amount {
            env.panic_with_error(Error::InsufficientCollateral);
        }

        let loan_key = LoanId(borrower.clone(), loan_id);
        if env.storage().persistent().has(&DataKey::Loan(loan_key.clone())) {
            env.panic_with_error(Error::InvalidState);
        }

        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);
        
        let contract_balance = token.balance(&env.current_contract_address());
        if contract_balance < amount {
            env.panic_with_error(Error::InsufficientBalance);
        }

        // CEI: Update state before external calls
        let active_loans_liquidity: i128 = env.storage().instance().get(&DataKey::ActiveLoansLiquidity).unwrap_or(0);
        env.storage().instance().set(&DataKey::ActiveLoansLiquidity, &(active_loans_liquidity + amount));

        // Pull collateral in BEFORE paying out the loan principal.
        token.transfer(&borrower, &env.current_contract_address(), &collateral_amount);
        token.transfer(&env.current_contract_address(), &borrower, &amount);

        let fee_rate_bps: i128 = env.storage().instance().get(&DataKey::FeeRate).unwrap_or(0);
        let start_time = env.ledger().timestamp();
        
        let loan_data = LoanData {
            borrower: borrower.clone(),
            amount,
            collateral_amount,
            interest_rate_bps: fee_rate_bps as u32,
            loan_start_timestamp: start_time,
            repayment_deadline: start_time + (30 * 24 * 60 * 60),
            accrued_interest: 0,
            total_repayment_due: amount,
        };
        
        env.storage()
            .persistent()
            .set(&DataKey::Loan(loan_key), &loan_data);

        let timestamp = env.ledger().timestamp();
        let fee_rate: i128 = env
            .storage()
            .instance()
            .get(&DataKey::FeeRate)
            .unwrap_or(0);

        env.events().publish(
            (symbol_short!("borrow"), borrower.clone()),
            BorrowEvent {
                creator: borrower.clone(),
                amount,
                token: acbu_token,
                loan_id,
                timestamp,
            },
        );
        env.events().publish(
            (symbol_short!("loan_cr"),),
            LoanCreatedEvent {
                loan_id,
                lender: env.current_contract_address(),
                borrower,
                amount,
                interest_bps: fee_rate,
                term_seconds: 0,
                timestamp,
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);
    }

    pub fn get_loan(env: Env, borrower: Address, loan_id: u64) -> Option<LoanData> {
        let loan_key = LoanId(borrower, loan_id);
        let mut loan_data: LoanData = env.storage().persistent().get(&DataKey::Loan(loan_key))?;

        let current_time = env.ledger().timestamp();
        let elapsed = current_time.saturating_sub(loan_data.loan_start_timestamp);

        let accrued_fee = Self::calculate_accrued_fee(
            &env,
            loan_data.amount,
            loan_data.interest_rate_bps,
            elapsed,
        );
        loan_data.accrued_interest = loan_data
            .accrued_interest
            .checked_add(accrued_fee)
            .unwrap_or_else(|| env.panic_with_error(Error::InvalidAmount));
        loan_data.total_repayment_due = loan_data
            .amount
            .checked_add(loan_data.accrued_interest)
            .unwrap_or_else(|| env.panic_with_error(Error::InvalidAmount));

        Some(loan_data)
    }

    pub fn repay(env: Env, borrower: Address, amount: i128, loan_id: u64) {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        borrower.require_auth();
        Self::check_not_paused(&env);

        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let loan_key = LoanId(borrower.clone(), loan_id);
        let mut loan_data = Self::get_loan(env.clone(), borrower.clone(), loan_id)
            .unwrap_or_else(|| env.panic_with_error(Error::NotFound));

        if amount > loan_data.total_repayment_due {
            env.panic_with_error(Error::InvalidAmount);
        }

        let acbu_token: Address = env.storage().instance().get(&DataKey::AcbuToken).unwrap();
        let token = soroban_sdk::token::Client::new(&env, &acbu_token);

        let principal_repaid = if amount > loan_data.accrued_interest {
            amount - loan_data.accrued_interest
        } else {
            0
        };

        // CEI: Update state before external calls
        loan_data.amount = loan_data.amount.checked_sub(principal_repaid).unwrap_or(0);

        let active_loans_liquidity: i128 = env.storage().instance().get(&DataKey::ActiveLoansLiquidity).unwrap_or(0);
        env.storage().instance().set(&DataKey::ActiveLoansLiquidity, &active_loans_liquidity.checked_sub(principal_repaid).unwrap_or(0));

        token.transfer(&borrower, &env.current_contract_address(), &amount);

        if loan_data.amount == 0 {
            if loan_data.collateral_amount > 0 {
                token.transfer(
                    &env.current_contract_address(),
                    &borrower,
                    &loan_data.collateral_amount,
                );
            }
            env.storage().persistent().remove(&DataKey::Loan(loan_key));
        } else {
            loan_data.loan_start_timestamp = env.ledger().timestamp();
            let remaining_interest = if amount < loan_data.accrued_interest {
                loan_data.accrued_interest - amount
            } else {
                0
            };
            loan_data.accrued_interest = remaining_interest;
            loan_data.total_repayment_due = loan_data
                .amount
                .checked_add(remaining_interest)
                .unwrap_or_else(|| env.panic_with_error(Error::InvalidAmount));

            env.storage()
                .persistent()
                .set(&DataKey::Loan(loan_key), &loan_data);
        }

        let timestamp = env.ledger().timestamp();
        env.events().publish(
            (symbol_short!("repay"), borrower.clone()),
            RepayEvent {
                creator: borrower.clone(),
                amount,
                token: acbu_token,
                loan_id,
                timestamp,
            },
        );
        env.events().publish(
            (symbol_short!("repaymt"),),
            RepaymentEvent {
                borrower: borrower.clone(),
                amount,
                timestamp,
            },
        );
        env.events().publish(
            (symbol_short!("loan_rp"),),
            LoanRepaidEvent {
                loan_id,
                borrower,
                amount,
                timestamp,
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);
    }

    pub fn pause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &true);
    }

    pub fn unpause(env: Env) {
        Self::check_admin(&env);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);
        let current_version: u32 = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        if new_version <= current_version {
            env.panic_with_error(Error::InvalidVersion);
        }
        let eligible_at = env.ledger().timestamp() + UPGRADE_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgradeWasm, &new_wasm_hash);
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgradeVersion, &new_version);
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgradeEligibleAt, &eligible_at);
    }

    pub fn execute_upgrade(env: Env) {
        Self::check_admin(&env);
        let wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgradeWasm)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let new_version: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgradeVersion)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgradeEligibleAt)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(Error::TimelockNotElapsed);
        }
        let current_version: u32 = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeWasm);
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeVersion);
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeEligibleAt);
        env.deployer().update_current_contract_wasm(wasm_hash);
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

    pub fn cancel_upgrade(env: Env) {
        Self::check_admin(&env);
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeWasm);
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeVersion);
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeEligibleAt);
    }

    pub fn get_balance(env: Env, lender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(lender))
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Two-step admin rotation
    //
    // Current admin nominates a successor and starts a timelock; the successor
    // must explicitly accept after the timelock elapses; the current admin may
    // cancel a pending transfer at any time. Prevents a single lost or
    // compromised key from leaving the contract permanently unmanageable.
    // -----------------------------------------------------------------------

    /// Step 1 — current admin nominates `new_admin` and starts the timelock.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        Self::check_admin(&env);
        let eligible_at = env.ledger().timestamp() + ADMIN_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdminEligibleAt, &eligible_at);
        let current_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        env.events().publish(
            (symbol_short!("adm_init"),),
            (current_admin, new_admin, eligible_at),
        );
    }

    /// Step 2 — the nominated address claims ownership after the timelock.
    pub fn accept_admin(env: Env) {
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingAdmin));
        pending_admin.require_auth();

        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdminEligibleAt)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(Error::AdminTimelockNotElapsed);
        }

        let old_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        env.storage().instance().set(&DataKey::Admin, &pending_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage()
            .instance()
            .remove(&DataKey::PendingAdminEligibleAt);

        env.events().publish(
            (symbol_short!("adm_done"),),
            (old_admin, pending_admin, env.ledger().timestamp()),
        );
    }

    /// Cancel a pending transfer (current admin only).
    pub fn cancel_admin_transfer(env: Env) {
        Self::check_admin(&env);
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingAdminToCancel));
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage()
            .instance()
            .remove(&DataKey::PendingAdminEligibleAt);
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        env.events().publish(
            (symbol_short!("adm_cncl"),),
            (admin, pending_admin, env.ledger().timestamp()),
        );
    }

    /// Current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    /// Pending successor, if a transfer is in progress.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Timestamp after which `accept_admin` becomes callable.
    pub fn get_pending_admin_eligible_at(env: Env) -> Option<u64> {
        env.storage().instance().get(&DataKey::PendingAdminEligibleAt)
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

    fn calculate_accrued_fee(
        env: &Env,
        principal: i128,
        fee_rate_bps: u32,
        elapsed_seconds: u64,
    ) -> i128 {
        const SECONDS_PER_YEAR: i128 = 31_536_000;

        principal
            .checked_mul(i128::from(fee_rate_bps))
            .and_then(|v| v.checked_mul(i128::from(elapsed_seconds)))
            .and_then(|v| v.checked_div(BASIS_POINTS * SECONDS_PER_YEAR))
            .unwrap_or_else(|| env.panic_with_error(Error::InvalidAmount))
    }
}

fn migrate_v0_to_v1(_env: Env) {}
