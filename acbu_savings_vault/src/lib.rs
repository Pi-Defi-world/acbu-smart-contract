#![no_std]
use core::fmt::{self, Display};
use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, Address,
    BytesN, Env, Symbol, Vec,
};

use shared::{calculate_fee, DataKey as SharedDataKey, reentrancy_guard, BASIS_POINTS, CONTRACT_VERSION};

mod shared {
    pub use shared::*;
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    Paused = 1001,
    InvalidAmount = 1002,
    NoDeposit = 1003,
    AccountingError = 1004,
    Overflow = 1005,
    InsufficientUnlocked = 1006,
    InvalidTerm = 1007,
    NotInitialized = 1008,
    NoAdmin = 1009,
    AlreadyInitialized = 1010,
    InvalidFeeRate = 1011,
    InvalidYieldRate = 1012,
    NoFeeRate = 1013,
    NoYieldRate = 1014,
    ZeroNetDeposit = 1015,
    InvalidVersion = 1016,
    TimelockNotElapsed = 1017,
    NoPendingUpgrade = 1018,
    NoPendingAdmin = 1019,
    AdminTimelockNotElapsed = 1020,
    NoPendingAdminToCancel = 1021,
    Unknown = 1999,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Paused => "savings vault is paused",
            Self::InvalidAmount => "invalid amount",
            Self::NoDeposit => "no deposit found",
            Self::AccountingError => "accounting error",
            Self::Overflow => "overflow",
            Self::InsufficientUnlocked => "insufficient unlocked balance",
            Self::InvalidTerm => "invalid term",
            Self::NotInitialized => "savings vault not initialized",
            Self::NoAdmin => "no admin configured",
            Self::AlreadyInitialized => "savings vault already initialized",
            Self::InvalidFeeRate => "invalid fee rate",
            Self::InvalidYieldRate => "invalid yield rate",
            Self::NoFeeRate => "fee rate not configured",
            Self::NoYieldRate => "yield rate not configured",
            Self::ZeroNetDeposit => "net deposit is zero",
            Self::InvalidVersion => "invalid contract version",
            Self::TimelockNotElapsed => "timelock has not elapsed",
            Self::NoPendingUpgrade => "no pending upgrade",
            Self::NoPendingAdmin => "no pending admin",
            Self::AdminTimelockNotElapsed => "admin timelock has not elapsed",
            Self::NoPendingAdminToCancel => "no pending admin to cancel",
            Self::Unknown => "unknown savings vault error",
        };
        f.write_str(message)
    }
}

// ---------------------------------------------------------------------------
// Storage keys — all keys centralised here to prevent accidental collisions
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub yield_rate: Symbol,
    pub paused: Symbol,
    pub pending_upgrade_wasm: Symbol,
    pub pending_upgrade_version: Symbol,
    pub pending_upgrade_eligible_at: Symbol,
    pub pending_admin: Symbol,
    pub pending_admin_eligible_at: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    yield_rate: symbol_short!("YLD_RATE"),
    paused: symbol_short!("PAUSED"),
    pending_upgrade_wasm: symbol_short!("PU_WASM"),
    pending_upgrade_version: symbol_short!("PU_VER"),
    pending_upgrade_eligible_at: symbol_short!("PU_ETA"),
    pending_admin: symbol_short!("PEND_ADM"),
    pending_admin_eligible_at: symbol_short!("PA_ETA"),
};

const DEPOSIT_KEY: Symbol = symbol_short!("DEPOSITS");
const SECONDS_PER_YEAR: i128 = 31_536_000;
const UPGRADE_TIMELOCK_SECONDS: u64 = 86_400;
const ADMIN_TIMELOCK_SECONDS: u64 = 86_400;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositLot {
    pub amount: i128,
    pub timestamp: u64,
    pub term_seconds: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositEvent {
    pub user: Address,
    pub gross_amount: i128,
    pub fee_amount: i128,
    pub net_amount: i128,
    pub term_seconds: u64,
    pub timestamp: u64,
    pub maturity_timestamp: u64,
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

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------
contractmeta!(key = "version", val = "1");

#[contract]
pub struct SavingsVault;

#[contractimpl]
impl SavingsVault {
    // -----------------------------------------------------------------------
    // Internal helpers — read required state, return typed errors on miss
    // -----------------------------------------------------------------------

    fn load_admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(Error::NoAdmin)
    }

    fn load_acbu_token(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(Error::NotInitialized)
    }

    fn load_fee_rate(env: &Env) -> Result<i128, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .ok_or(Error::NoFeeRate)
    }

    fn load_yield_rate(env: &Env) -> Result<i128, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.yield_rate)
            .ok_or(Error::NoYieldRate)
    }

    fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Public logic
    // -----------------------------------------------------------------------

    pub fn initialize(
        env: Env,
        admin: Address,
        acbu_token: Address,
        fee_rate_bps: i128,
        yield_rate_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(Error::AlreadyInitialized);
        }
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            env.panic_with_error(Error::InvalidFeeRate);
        }
        if !(0..=BASIS_POINTS).contains(&yield_rate_bps) {
            env.panic_with_error(Error::InvalidYieldRate);
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.yield_rate, &yield_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage().instance().set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    /// Deposit (lock) ACBU for a term. User transfers ACBU to this contract.
    pub fn deposit(env: Env, user: Address, amount: i128, term_seconds: u64) -> i128 {
        reentrancy_guard::acquire_guard(&env);

        user.require_auth();

        if Self::is_paused(&env) {
            env.panic_with_error(Error::Paused);
        }
        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }
        if term_seconds == 0 {
            env.panic_with_error(Error::InvalidTerm);
        }
        // FIX(#328): Validate that term_seconds won't overflow when added to
        // the current ledger timestamp. All term comparisons stay in u64 space
        // to avoid unintended sign behavior from u64→i128 casts.
        let now = env.ledger().timestamp();
        let maturity_timestamp = now
            .checked_add(term_seconds)
            .unwrap_or_else(|| env.panic_with_error(Error::InvalidTerm));

        let fee_rate = Self::load_fee_rate(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let fee_amount = calculate_fee(amount, fee_rate);
        let net_amount = amount
            .checked_sub(fee_amount)
            .unwrap_or_else(|| env.panic_with_error(Error::Overflow));

        if net_amount <= 0 {
            env.panic_with_error(Error::ZeroNetDeposit);
        }

        let acbu = Self::load_acbu_token(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let token = soroban_sdk::token::Client::new(&env, &acbu);
        let vault_addr = env.current_contract_address();

        // Transfer the net amount to the vault first, verifying success.
        match token.try_transfer(&user, &vault_addr, &net_amount) {
            Ok(Ok(())) => {}
            _ => env.panic_with_error(Error::AccountingError),
        }
        // Transfer the fee to the admin if applicable, verifying success.
        if fee_amount > 0 {
            match token.try_transfer(&user, &admin, &fee_amount) {
                Ok(Ok(())) => {}
                _ => env.panic_with_error(Error::AccountingError),
            }
        }

        // Record the deposit lot in storage after the transfers succeed
        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let mut lots: Vec<DepositLot> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));

        lots.push_back(DepositLot {
            amount: net_amount,
            timestamp: now,
            term_seconds,
        });

        env.storage().persistent().set(&key, &lots);


        env.events().publish(
            (symbol_short!("Deposit"), user.clone()),
            DepositEvent {
                user,
                gross_amount: amount,
                fee_amount,
                net_amount,
                term_seconds,
                timestamp: now,
                maturity_timestamp,
            },
        );

        reentrancy_guard::release_guard(&env);

        net_amount
    }

    /// Withdraw unlocked ACBU + yield for a specific term.
    pub fn withdraw(env: Env, user: Address, term_seconds: u64, amount: i128) -> i128 {
        reentrancy_guard::acquire_guard(&env);

        user.require_auth();

        if Self::is_paused(&env) {
            env.panic_with_error(Error::Paused);
        }
        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| env.panic_with_error(Error::NoDeposit));

        let now = env.ledger().timestamp();

        let mut unlocked_balance = 0i128;
        for lot in lots.iter() {
            if now >= lot.timestamp.saturating_add(lot.term_seconds) {
                unlocked_balance = unlocked_balance
                    .checked_add(lot.amount)
                    .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
            }
        }

        if amount > unlocked_balance {
            env.panic_with_error(Error::InsufficientUnlocked);
        }

        let yield_rate = Self::load_yield_rate(&env).unwrap_or_else(|e| env.panic_with_error(e));

        let mut amount_left = amount;
        let mut yield_amount = 0i128;
        let mut updated_lots = Vec::new(&env);

        for lot in lots.iter() {
            if amount_left == 0 || now < lot.timestamp.saturating_add(lot.term_seconds) {
                updated_lots.push_back(lot);
                continue;
            }

            if lot.amount <= amount_left {
                amount_left = amount_left
                    .checked_sub(lot.amount)
                    .unwrap_or_else(|| env.panic_with_error(Error::AccountingError));
                let elapsed = now.saturating_sub(lot.timestamp);
                let lot_yield = Self::calculate_yield(lot.amount, yield_rate, elapsed)
                    .unwrap_or_else(|e| env.panic_with_error(e));
                yield_amount = yield_amount
                    .checked_add(lot_yield)
                    .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
            } else {
                let consumed = amount_left;
                let remaining = lot
                    .amount
                    .checked_sub(consumed)
                    .unwrap_or_else(|| env.panic_with_error(Error::AccountingError));
                let elapsed = now.saturating_sub(lot.timestamp);
                let lot_yield = Self::calculate_yield(consumed, yield_rate, elapsed)
                    .unwrap_or_else(|e| env.panic_with_error(e));
                yield_amount = yield_amount
                    .checked_add(lot_yield)
                    .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
                updated_lots.push_back(DepositLot {
                    amount: remaining,
                    timestamp: lot.timestamp,
                    term_seconds: lot.term_seconds,
                });
                amount_left = 0;
            }
        }

        if amount_left > 0 {
            env.panic_with_error(Error::AccountingError);
        }

        if updated_lots.is_empty() {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &updated_lots);
        }

        let payout_amount = amount
            .checked_add(yield_amount)
            .unwrap_or_else(|| env.panic_with_error(Error::Overflow));

        let acbu = Self::load_acbu_token(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let token = soroban_sdk::token::Client::new(&env, &acbu);
        let vault_addr = env.current_contract_address();

        token.transfer(&vault_addr, &user, &amount);
        if yield_amount > 0 {
            token.transfer(&vault_addr, &user, &yield_amount);
        }

        env.events().publish(
            (symbol_short!("Withdraw"), user.clone()),
            WithdrawEvent {
                user,
                amount,
                fee_amount: 0,
                yield_amount,
                timestamp: now,
            },
        );

        reentrancy_guard::release_guard(&env);

        payout_amount
    }

    pub fn get_balance(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        Self::sum_lots(&lots)
    }

    pub fn get_pending_yield(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| env.panic_with_error(Error::NoDeposit));

        let yield_rate = Self::load_yield_rate(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let now = env.ledger().timestamp();
        let mut yield_amount = 0i128;

        for lot in lots.iter() {
            let unlocked = now >= lot.timestamp.saturating_add(lot.term_seconds);
            if !unlocked {
                continue;
            }
            let elapsed = now.saturating_sub(lot.timestamp);
            let lot_yield = Self::calculate_yield(lot.amount, yield_rate, elapsed)
                .unwrap_or_else(|e| env.panic_with_error(e));
            yield_amount = yield_amount
                .checked_add(lot_yield)
                .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
        }

        yield_amount
    }

    /// Returns the unlock timestamp for each deposit lot belonging to `user` for the given term.
    ///
    /// Uses `checked_add` on every `lot.timestamp + lot.term_seconds` computation so that
    /// extreme values (e.g. near-max epoch timestamps) cannot silently wrap around and make
    /// a locked deposit appear as already expired, which would release funds prematurely.
    pub fn get_term_end(env: Env, user: Address, term_seconds: u64) -> Vec<u64> {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env));

        let mut ends = Vec::new(&env);
        for lot in lots.iter() {
            let term_end = lot
                .timestamp
                .checked_add(lot.term_seconds)
                .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
            ends.push_back(term_end);
        }
        ends
    }

    pub fn pause(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
    }

    pub fn unpause(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    pub fn update_acbu_token(env: Env, new_acbu_token: Address) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &new_acbu_token);
    }

    pub fn set_fee_rate(env: Env, fee_rate_bps: i128) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            env.panic_with_error(Error::InvalidFeeRate);
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
    }

    pub fn set_yield_rate(env: Env, yield_rate_bps: i128) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        if !(0..=BASIS_POINTS).contains(&yield_rate_bps) {
            env.panic_with_error(Error::InvalidYieldRate);
        }
        env.storage()
            .instance()
            .set(&DATA_KEY.yield_rate, &yield_rate_bps);
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DATA_KEY.admin).unwrap()
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        let current_version = Self::version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(Error::InvalidVersion);
        }
        // Use checked_add to prevent u64 overflow on the eligible-at timestamp.
        let eligible_at = env
            .ledger()
            .timestamp()
            .checked_add(UPGRADE_TIMELOCK_SECONDS)
            .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_wasm, &new_wasm_hash);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_version, &new_version);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_eligible_at, &eligible_at);
    }

    pub fn execute_upgrade(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        let wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_wasm)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let new_version: u32 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_version)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(Error::TimelockNotElapsed);
        }
        let current_version = Self::version(env.clone());
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_version);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
        env.deployer().update_current_contract_wasm(wasm_hash);
        for v in current_version..new_version {
            match v {
                0 => Self::migrate_v0_to_v1(env.clone()),
                _ => {}
            }
        }
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &new_version);
    }

    pub fn cancel_upgrade(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_version);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
    }

    // -----------------------------------------------------------------------
    // Two-step admin rotation
    // -----------------------------------------------------------------------

    pub fn transfer_admin(env: Env, new_admin: Address) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        // Use checked_add to prevent u64 overflow on the eligible-at timestamp.
        let eligible_at = env
            .ledger()
            .timestamp()
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_admin, &new_admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_admin_eligible_at, &eligible_at);
        env.events().publish(
            (symbol_short!("adm_init"),),
            (admin, new_admin, eligible_at),
        );
    }

    pub fn accept_admin(env: Env) {
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin)
            .unwrap_or_else(|| env.panic_with_error(Error::NoAdmin));
        pending_admin.require_auth();

        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(Error::TimelockNotElapsed);
        }

        let old_admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        env.storage().instance().set(&DATA_KEY.admin, &pending_admin);
        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);

        env.events().publish(
            (symbol_short!("adm_done"),),
            (old_admin, pending_admin, env.ledger().timestamp()),
        );
    }

    pub fn cancel_admin_transfer(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin)
            .unwrap_or_else(|| env.panic_with_error(Error::NoAdmin));
        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);
        env.events().publish(
            (symbol_short!("adm_cncl"),),
            (admin, pending_admin, env.ledger().timestamp()),
        );
    }

    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DATA_KEY.pending_admin)
    }

    pub fn version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn get_pending_admin_eligible_at(env: Env) -> Option<u64> {
        env.storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn sum_lots(lots: &Vec<DepositLot>) -> i128 {
        let mut total = 0i128;
        for lot in lots.iter() {
            total = total
                .checked_add(lot.amount)
                .expect("Overflow in total balance calculation");
        }
        total
    }

    // FIX(#328): Use explicit i128::from() for u64→i128 conversion of
    // elapsed_seconds to avoid any unintended sign behavior. Guard against
    // zero divisor and zero inputs for safety.
    fn calculate_yield(
        principal: i128,
        yield_rate_bps: i128,
        elapsed_seconds: u64,
    ) -> Result<i128, Error> {
        if principal == 0 || yield_rate_bps == 0 || elapsed_seconds == 0 {
            return Ok(0);
        }
        let elapsed_i128: i128 = i128::from(elapsed_seconds);
        let divisor = BASIS_POINTS.checked_mul(SECONDS_PER_YEAR).ok_or(Error::Overflow)?;
        if divisor == 0 {
            return Err(Error::Overflow);
        }
        let numerator = principal
            .checked_mul(yield_rate_bps)
            .and_then(|v| v.checked_mul(elapsed_i128))
            .ok_or(Error::Overflow)?;
        numerator.checked_div(divisor).ok_or(Error::Overflow)
    }
    fn migrate_v0_to_v1(_env: Env) {
        // No storage schema changes between v0 and v1.
    }
}
