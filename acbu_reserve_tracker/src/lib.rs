#![no_std]
use core::fmt::{self, Display};
use soroban_sdk::{
    contract, contracterror, contractimpl, contractmeta, contracttype, symbol_short, Address,
    BytesN, Env, Map, Symbol, Vec,
};

use shared::{
    CurrencyCode, DataKey as SharedDataKey, ReserveData, BASIS_POINTS, CONTRACT_VERSION,
    ORACLE_GET_ACBU_RATE, TOKEN_GET_TOTAL_SUPPLY,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ReserveTrackerError {
    AlreadyInitialized = 8001,
    InvalidVersion = 8002,
    ZeroSupply = 8003,
    NoPendingAdmin = 8004,
    AdminTimelockNotElapsed = 8005,
    NoPendingAdminToCancel = 8006,
    Unauthorized = 8007,
    Unknown = 8999,
}

impl Display for ReserveTrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AlreadyInitialized => "reserve tracker already initialized",
            Self::InvalidVersion => "invalid contract version",
            Self::ZeroSupply => "zero supply",
            Self::NoPendingAdmin => "no pending admin",
            Self::AdminTimelockNotElapsed => "admin timelock has not elapsed",
            Self::NoPendingAdminToCancel => "no pending admin to cancel",
            Self::Unauthorized => "unauthorized",
            Self::Unknown => "unknown reserve tracker error",
        };
        f.write_str(message)
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub oracle: Symbol,
    pub reserves: Symbol,
    pub min_reserve_ratio: Symbol,
    pub acbu_token: Symbol,
    pub version: Symbol,
    pub pending_admin: Symbol,
    pub pending_admin_eligible_at: Symbol,
    pub last_verify_call: Symbol,
    pub last_verify_result: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserves: symbol_short!("RESERVES"),
    min_reserve_ratio: symbol_short!("MIN_RES"),
    acbu_token: symbol_short!("ACBU_TKN"),
    version: symbol_short!("VERSION"),
    pending_admin: symbol_short!("PEND_ADM"),
    pending_admin_eligible_at: symbol_short!("PEND_ETA"),
    last_verify_call: symbol_short!("LAST_VFY"),
    last_verify_result: symbol_short!("LAST_RES"),
};

/// Admin rotation timelock: the pending admin must wait this long before
/// claiming ownership, giving the current admin a window to cancel a mistaken
/// or malicious transfer.
const ADMIN_TIMELOCK_SECONDS: u64 = 86_400;

/// Rate limit for verify_reserves calls to prevent spam and ledger load.
/// Only one verify_reserves call is allowed per this cooldown period.
const VERIFY_RESERVES_COOLDOWN_SECONDS: u64 = 60;

contractmeta!(key = "version", val = "1");

#[contract]
pub struct ReserveTrackerContract;

#[contractimpl]
impl ReserveTrackerContract {
    /// Initialize the reserve tracker contract
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        acbu_token: Address,
        min_reserve_ratio_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(ReserveTrackerError::AlreadyInitialized);
        }

        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.oracle, &oracle);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_reserve_ratio, &min_reserve_ratio_bps);

        let reserves: Map<CurrencyCode, ReserveData> = Map::new(&env);
        env.storage().instance().set(&DATA_KEY.reserves, &reserves);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    /// Return the circulating ACBU total supply by cross-contract-calling
    /// `get_total_supply` on the registered ACBU token contract.
    pub fn get_total_supply_from_token(env: &Env) -> i128 {
        let acbu_token_addr: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        // Use invoke_contract to avoid dependency on a specific token client implementation
        env.invoke_contract(
            &acbu_token_addr,
            &Symbol::new(env, TOKEN_GET_TOTAL_SUPPLY),
            Vec::new(env),
        )
    }

    /// Verify that on-chain reserves are sufficient to back the circulating ACBU supply.
    ///
    /// Total supply is obtained by cross-contract-calling `get_total_supply` on the
    /// registered ACBU token contract.  Previously this used
    /// `acbu_client.balance(&env.current_contract_address())`, which always returned 0
    /// because the reserve tracker holds no ACBU — causing reserve checks to be skipped
    /// entirely (fix for issue #193).
    ///
    /// Rate limited to prevent spam: only one call per VERIFY_RESERVES_COOLDOWN_SECONDS.
    pub fn verify_reserves(env: Env) -> bool {
        let now = env.ledger().timestamp();

        let last_call: Option<u64> = env.storage().instance().get(&DATA_KEY.last_verify_call);
        if let Some(last) = last_call {
            if now.saturating_sub(last) < VERIFY_RESERVES_COOLDOWN_SECONDS {
                let cached: bool = env
                    .storage()
                    .instance()
                    .get(&DATA_KEY.last_verify_result)
                    .unwrap_or(true);
                return cached;
            }
        }

        env.storage().instance().set(&DATA_KEY.last_verify_call, &now);

        let total_acbu_supply = Self::get_total_supply_from_token(&env);
        if total_acbu_supply == 0 {
            env.panic_with_error(ReserveTrackerError::ZeroSupply);
        }
        let result = Self::is_reserve_sufficient(env.clone(), total_acbu_supply);
        env.storage()
            .instance()
            .set(&DATA_KEY.last_verify_result, &result);
        result
    }

    /// Like [`Self::verify_reserves`] but uses the caller-supplied
    /// `total_acbu_supply` instead of querying the token contract. Returns `true`
    /// if reserves meet the minimum ratio.
    pub fn verify_reserves_manual(env: Env, total_acbu_supply: i128) -> bool {
        Self::is_reserve_sufficient(env, total_acbu_supply)
    }

    /// Update reserve amount for a currency (admin or authorized address)
    pub fn update_reserve(
        env: Env,
        updater: Address,
        currency: CurrencyCode,
        amount: i128,
        value_usd: i128,
    ) {
        // Authorize updater
        updater.require_auth();
        let admin = Self::get_admin(env.clone());
        if updater != admin {
            env.panic_with_error(ReserveTrackerError::Unauthorized);
        }

        let current_time = env.ledger().timestamp();

        // Update reserves map
        let mut reserves: Map<CurrencyCode, ReserveData> = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserves)
            .unwrap_or(Map::new(&env));
        let reserve_data = ReserveData {
            currency: currency.clone(),
            amount,
            value_usd,
            timestamp: current_time,
        };

        reserves.set(currency.clone(), reserve_data.clone());
        env.storage().instance().set(&DATA_KEY.reserves, &reserves);

        env.events()
            .publish((symbol_short!("reserve"), currency.clone()), reserve_data);
    }

    /// Get current reserves for all currencies
    pub fn get_all_reserves(env: Env) -> Map<CurrencyCode, ReserveData> {
        env.storage().instance().extend_ttl(5184000, 5184000);
        env.storage()
            .instance()
            .get(&DATA_KEY.reserves)
            .unwrap_or(Map::new(&env))
    }

    /// Check if the total reserves are sufficient to back the ACBU supply
    pub fn total_reserves(env: Env) -> i128 {
        let reserves = Self::get_all_reserves(env.clone());
        let mut total_usd = 0i128;
        for (_curr, data) in reserves.iter() {
            total_usd = total_usd.checked_add(data.value_usd).expect("Overflow");
        }
        total_usd
    }

    /// Return `true` if total reserve USD value backs `total_acbu_supply` at or
    /// above the configured minimum reserve ratio (default 100%).
    ///
    /// Sums the USD value of all stored reserves and compares it against the ACBU
    /// supply valued at the oracle's ACBU/USD rate. Trivially returns `true` when
    /// supply is non-positive or values to zero.
    pub fn is_reserve_sufficient(env: Env, total_acbu_supply: i128) -> bool {
        if total_acbu_supply <= 0 {
            return true;
        }

        let reserves = Self::get_all_reserves(env.clone());
        let mut total_reserve_usd = 0i128;

        for (_curr, data) in reserves.iter() {
            total_reserve_usd = total_reserve_usd
                .checked_add(data.value_usd)
                .expect("Overflow in reserve calculation");
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let acbu_usd_rate: i128 = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE),
            Vec::new(&env),
        );

        let total_acbu_usd = (total_acbu_supply * acbu_usd_rate) / 100_000_000;
        if total_acbu_usd == 0 {
            return true;
        }

        let min_reserve_ratio = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_reserve_ratio)
            .unwrap_or(10000i128); // Default to 100%

        let current_ratio = (total_reserve_usd * BASIS_POINTS) / total_acbu_usd;
        current_ratio >= min_reserve_ratio
    }

    /// Upgrade the contract WASM to `new_wasm_hash` and bump the stored version to
    /// `new_version` (admin only). `new_version` must exceed the current version.
    /// Runs any required migrations.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);

        let current_version = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        if new_version <= current_version {
            env.panic_with_error(ReserveTrackerError::InvalidVersion);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        // Run migrations
        for v in current_version..new_version {
            match v {
                0 => shared::migrate_v0_to_v1(&env),
                _ => {}
            }
        }

        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &new_version);
    }

    // -----------------------------------------------------------------------
    // Reserve management (admin only)
    // -----------------------------------------------------------------------

    /// Replace all stored reserves with an empty map (admin only).
    ///
    /// Requires admin authorisation.  Without the auth gate any caller could
    /// wipe the reserves map, making verify_reserves trivially pass (empty
    /// reserves → zero total_reserve_usd → ratio check skipped when supply is
    /// also zero).  This function is intentionally destructive and should only
    /// be used for emergency recovery or contract reset by the admin.
    pub fn reset_reserves(env: Env) {
        Self::check_admin(&env);
        let empty: Map<CurrencyCode, ReserveData> = Map::new(&env);
        env.storage().instance().set(&DATA_KEY.reserves, &empty);
    }

    // -----------------------------------------------------------------------
    // Dependency address updaters (admin only)
    // -----------------------------------------------------------------------

    /// Update the oracle contract address (admin only).
    pub fn update_oracle(env: Env, new_oracle: Address) {
        Self::check_admin(&env);
        env.storage().instance().set(&DATA_KEY.oracle, &new_oracle);
    }

    /// Update the ACBU token contract address (admin only).
    pub fn update_acbu_token(env: Env, new_acbu_token: Address) {
        Self::check_admin(&env);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &new_acbu_token);
    }

    // -----------------------------------------------------------------------
    // Two-step admin rotation
    //
    // Mirrors the oracle's pattern: the current admin nominates a successor and
    // starts a timelock; the successor must explicitly accept after the timelock
    // elapses; the current admin can cancel a pending transfer at any time. This
    // prevents a single fat-fingered or compromised key from being unrecoverable.
    // -----------------------------------------------------------------------

    /// Step 1 — current admin nominates `new_admin` and starts the timelock.
    /// The current admin keeps full authority until `accept_admin` completes.
    pub fn transfer_admin(env: Env, new_admin: Address) {
        Self::check_admin(&env);
        let eligible_at = env.ledger().timestamp() + ADMIN_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_admin, &new_admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_admin_eligible_at, &eligible_at);
        let current_admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
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
            .get(&DATA_KEY.pending_admin)
            .unwrap_or_else(|| env.panic_with_error(ReserveTrackerError::NoPendingAdmin));
        pending_admin.require_auth();

        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(ReserveTrackerError::AdminTimelockNotElapsed);
        }

        let old_admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        env.storage()
            .instance()
            .set(&DATA_KEY.admin, &pending_admin);
        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);

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
            .get(&DATA_KEY.pending_admin)
            .unwrap_or_else(|| env.panic_with_error(ReserveTrackerError::NoPendingAdminToCancel));
        env.storage().instance().remove(&DATA_KEY.pending_admin);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_admin_eligible_at);
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        env.events().publish(
            (symbol_short!("adm_cncl"),),
            (admin, pending_admin, env.ledger().timestamp()),
        );
    }

    /// Current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DATA_KEY.admin).unwrap()
    }

    /// Check if the contract has been initialized.
    ///
    /// Backend services can call this before invoking other functions to avoid
    /// cryptic storage-not-found errors from uninitialized contracts.
    pub fn is_initialized(env: Env) -> bool {
        env.storage().instance().has(&SharedDataKey::Version)
    }

    /// Pending successor, if a transfer is in progress.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DATA_KEY.pending_admin)
    }

    /// Timestamp after which `accept_admin` becomes callable.
    pub fn get_pending_admin_eligible_at(env: Env) -> Option<u64> {
        env.storage()
            .instance()
            .get(&DATA_KEY.pending_admin_eligible_at)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}
