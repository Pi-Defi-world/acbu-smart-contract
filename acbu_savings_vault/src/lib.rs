#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol, Vec};

use shared::{calculate_fee, BASIS_POINTS};

mod shared {
    pub use shared::*;
}

// ---------------------------------------------------------------------------
// Error codes — every contract_error code is documented here.
// ---------------------------------------------------------------------------
/// 1001 — Contract is paused; no deposits or withdrawals allowed.
const ERR_PAUSED: u32 = 1001;
/// 1002 — Amount must be greater than zero.
const ERR_INVALID_AMOUNT: u32 = 1002;
/// 1003 — No deposit record found for this user + term combination.
const ERR_NO_DEPOSIT: u32 = 1003;
/// 1004 — Internal accounting error: amount_left > 0 after consuming all lots.
const ERR_ACCOUNTING: u32 = 1004;
/// 1005 — Arithmetic overflow while calculating yield.
const ERR_OVERFLOW: u32 = 1005;
/// 1006 — Requested withdrawal exceeds the unlocked (matured) balance.
const ERR_INSUFFICIENT_UNLOCKED: u32 = 1006;
/// 1007 — Term must be greater than zero.
const ERR_INVALID_TERM: u32 = 1007;
/// 1008 — Contract not initialized: ACBU token address missing from storage.
const ERR_NOT_INITIALIZED: u32 = 1008;
/// 1009 — Contract not initialized: admin address missing from storage.
const ERR_NO_ADMIN: u32 = 1009;
/// 1010 — Contract already initialized.
const ERR_ALREADY_INITIALIZED: u32 = 1010;
/// 1011 — Fee rate is out of the valid 0–10 000 bps range.
const ERR_INVALID_FEE_RATE: u32 = 1011;
/// 1012 — Yield rate is out of the valid 0–10 000 bps range.
const ERR_INVALID_YIELD_RATE: u32 = 1012;
/// 1013 — Fee rate missing from storage (storage corruption guard).
const ERR_NO_FEE_RATE: u32 = 1013;
/// 1014 — Yield rate missing from storage (storage corruption guard).
const ERR_NO_YIELD_RATE: u32 = 1014;
/// 1015 — Net deposit after fee is zero or negative, which is not allowed.
const ERR_ZERO_NET_DEPOSIT: u32 = 1015;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub yield_rate: Symbol,
    pub paused: Symbol,
    pub version: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    yield_rate: symbol_short!("YLD_RATE"),
    paused: symbol_short!("PAUSED"),
    version: symbol_short!("VERSION"),
};

const DEPOSIT_KEY: Symbol = symbol_short!("DEPOSITS");
const SECONDS_PER_YEAR: i128 = 31_536_000;
const VERSION: u32 = 1;

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
#[contract]
pub struct SavingsVault;

#[contractimpl]
impl SavingsVault {
    // -----------------------------------------------------------------------
    // Internal helpers — read required state, return typed errors on miss
    // -----------------------------------------------------------------------

    fn load_admin(env: &Env) -> Result<Address, soroban_sdk::Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(soroban_sdk::Error::from_contract_error(ERR_NO_ADMIN))
    }

    fn load_acbu_token(env: &Env) -> Result<Address, soroban_sdk::Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(soroban_sdk::Error::from_contract_error(ERR_NOT_INITIALIZED))
    }

    fn load_fee_rate(env: &Env) -> Result<i128, soroban_sdk::Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .ok_or(soroban_sdk::Error::from_contract_error(ERR_NO_FEE_RATE))
    }

    fn load_yield_rate(env: &Env) -> Result<i128, soroban_sdk::Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.yield_rate)
            .ok_or(soroban_sdk::Error::from_contract_error(ERR_NO_YIELD_RATE))
    }

    fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Initialize the savings vault contract.
    pub fn initialize(
        env: Env,
        admin: Address,
        acbu_token: Address,
        fee_rate_bps: i128,
        yield_rate_bps: i128,
    ) -> Result<(), soroban_sdk::Error> {
        if env.storage().instance().has(&DATA_KEY.admin) {
            return Err(soroban_sdk::Error::from_contract_error(ERR_ALREADY_INITIALIZED));
        }
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            return Err(soroban_sdk::Error::from_contract_error(ERR_INVALID_FEE_RATE));
        }
        if !(0..=BASIS_POINTS).contains(&yield_rate_bps) {
            return Err(soroban_sdk::Error::from_contract_error(ERR_INVALID_YIELD_RATE));
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.yield_rate, &yield_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
        Ok(())
    }

    /// Deposit (lock) ACBU for a term. User transfers ACBU to this contract.
    pub fn deposit(
        env: Env,
        user: Address,
        amount: i128,
        term_seconds: u64,
    ) -> Result<i128, soroban_sdk::Error> {
        user.require_auth();

        if Self::is_paused(&env) {
            return Err(soroban_sdk::Error::from_contract_error(ERR_PAUSED));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(ERR_INVALID_AMOUNT));
        }
        if term_seconds == 0 {
            return Err(soroban_sdk::Error::from_contract_error(ERR_INVALID_TERM));
        }

        // Load fee_rate and calculate fee before transfer.
        let fee_rate = Self::load_fee_rate(&env)?;
        let fee_amount = calculate_fee(amount, fee_rate);
        let net_amount = amount - fee_amount;

        // Guard against fee consuming entire deposit.
        if net_amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(ERR_ZERO_NET_DEPOSIT));
        }

        let acbu = Self::load_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&user, &env.current_contract_address(), &amount);

        // Immediately forward fee to admin if non-zero.
        if fee_amount > 0 {
            let admin = Self::load_admin(&env)?;
            client.transfer(&env.current_contract_address(), &admin, &fee_amount);
        }

        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let mut lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        lots.push_back(DepositLot {
                        //Store net_amount instead of gross amount.
            amount: net_amount,
            timestamp: env.ledger().timestamp(),
            term_seconds,
        });
        env.storage().temporary().set(&key, &lots);

        env.events().publish(
            (symbol_short!("Deposit"), user.clone()),
            DepositEvent {
                user,
                // Emit gross, fee, and net for transparency.
                gross_amount: amount,
                fee_amount,
                net_amount,
                term_seconds,
                timestamp: env.ledger().timestamp(),
            },
        );
        //Return net balance. get_balance will now reflect fees.
        Ok(Self::sum_lots(&lots))
    }

    /// Withdraw (unlock) ACBU after term. Applies the stored protocol fee.
    pub fn withdraw(
        env: Env,
        user: Address,
        term_seconds: u64,
        amount: i128,
    ) -> Result<(), soroban_sdk::Error> {
        user.require_auth();

        if Self::is_paused(&env) {
            return Err(soroban_sdk::Error::from_contract_error(ERR_PAUSED));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(ERR_INVALID_AMOUNT));
        }

        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(soroban_sdk::Error::from_contract_error(ERR_NO_DEPOSIT))?;

        let now = env.ledger().timestamp();
        let unlocked_balance: i128 = lots
            .iter()
            .filter(|lot| now >= lot.timestamp.saturating_add(lot.term_seconds))
            .fold(0i128, |acc, lot| acc + lot.amount);

        if unlocked_balance < amount {
            return Err(soroban_sdk::Error::from_contract_error(ERR_INSUFFICIENT_UNLOCKED));
        }

        // Removed load_fee_rate and calculate_fee. Fee no longer charged on withdraw.
        
        let yield_rate = Self::load_yield_rate(&env)?;
        

        let mut amount_left = amount;
        let mut updated_lots = Vec::new(&env);
        let mut yield_amount: i128 = 0;

        for lot in lots.iter() {
            if amount_left == 0 {
                updated_lots.push_back(lot);
                continue;
            }
            let unlocked = now >= lot.timestamp.saturating_add(lot.term_seconds);
            if !unlocked {
                updated_lots.push_back(lot);
                continue;
            }
            if lot.amount <= amount_left {
                amount_left -= lot.amount;
                let elapsed = now.saturating_sub(lot.timestamp);
                yield_amount += Self::calculate_yield(lot.amount, yield_rate, elapsed)?;
            } else {
                let consumed = amount_left;
                let remaining = lot.amount - consumed;
                let elapsed = now.saturating_sub(lot.timestamp);
                yield_amount += Self::calculate_yield(consumed, yield_rate, elapsed)?;
                updated_lots.push_back(DepositLot {
                    amount: remaining,
                    timestamp: lot.timestamp,
                    term_seconds: lot.term_seconds,
                });
                amount_left = 0;
            }
        }

        if amount_left > 0 {
            return Err(soroban_sdk::Error::from_contract_error(ERR_ACCOUNTING));
        }

        if updated_lots.is_empty() {
            env.storage().temporary().remove(&key);
        } else {
            env.storage().temporary().set(&key, &updated_lots);
        }

        // net_amount removed. Payout is just principal + yield.
        let payout_amount: i128 = amount + yield_amount;

        // Single storage read for the token — reuse the client for both transfers.
        let acbu = Self::load_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&env.current_contract_address(), &user, &payout_amount);
        // Removed fee transfer to admin. Fee already paid on deposit.

        env.events().publish(
            (symbol_short!("Withdraw"), user.clone()),
            WithdrawEvent {
                user,
                amount,
                fee_amount: 0,
                yield_amount,
                timestamp: env.ledger().timestamp(),
            },
        );
        Ok(())
    }

    /// Get total deposited balance for a user + term combination. Net of deposit fees.
    pub fn get_balance(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        Self::sum_lots(&lots)
    }

        pub fn get_pending_yield(env: Env, user: Address, term_seconds: u64) -> Result<i128, soroban_sdk::Error> {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
         .storage()
         .temporary()
         .get(&key)
         .unwrap_or(Vec::new(&env));

        let now = env.ledger().timestamp();
        let yield_rate = Self::load_yield_rate(&env)?;
        let mut yield_amount: i128 = 0;

        for lot in lots.iter() {
            let unlocked = now >= lot.timestamp.saturating_add(lot.term_seconds);
            if!unlocked {
                continue;
            }
            let elapsed = now.saturating_sub(lot.timestamp);
            // yield computed on net principal stored in lot.amount.
            yield_amount += Self::calculate_yield(lot.amount, yield_rate, elapsed)?;
        }
        Ok(yield_amount)
    }

    pub fn pause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin = Self::load_admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin = Self::load_admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    pub fn migrate(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin = Self::load_admin(&env)?;
        admin.require_auth();
        let stored_version: u32 = env
            .storage()
            .instance()
            .get(&DATA_KEY.version)
            .unwrap_or(0);
        if stored_version < VERSION {
            env.storage().instance().set(&DATA_KEY.version, &VERSION);
        }
        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), soroban_sdk::Error> {
        let admin = Self::load_admin(&env)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn sum_lots(lots: &Vec<DepositLot>) -> i128 {
        let mut total = 0i128;
        for lot in lots.iter() {
            total += lot.amount;
        }
        total
    }

    fn calculate_yield(
        principal: i128,
        yield_rate_bps: i128,
        elapsed_seconds: u64,
    ) -> Result<i128, soroban_sdk::Error> {
        let elapsed_i128 = i128::from(elapsed_seconds);
        let numerator = principal
            .checked_mul(yield_rate_bps)
            .and_then(|v| v.checked_mul(elapsed_i128))
            .ok_or(soroban_sdk::Error::from_contract_error(ERR_OVERFLOW))?;
        Ok(numerator / (BASIS_POINTS * SECONDS_PER_YEAR))
    }
}
