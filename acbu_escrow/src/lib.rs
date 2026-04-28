#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    Paused = 3001,
    InvalidAmount = 3002,
    EscrowNotFound = 3003,
    PayerMismatch = 3004,
    EscrowExists = 3005,
    UninitializedAdmin = 3006,
    UninitializedAcBuToken = 3007,
    AlreadyInitialized = 3008,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub paused: Symbol,
    pub version: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    paused: symbol_short!("PAUSED"),
    version: symbol_short!("VERSION"),
};

const VERSION: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowId(pub Address, pub u64);

#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowCreatedEvent {
    pub escrow_id: u64,
    pub payer: Address,
    pub payee: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowReleasedEvent {
    pub escrow_id: u64,
    pub payee: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowRefundedEvent {
    pub escrow_id: u64,
    pub payer: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contract]
pub struct Escrow;

#[contractimpl]
impl Escrow {
    fn get_admin(env: &Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(EscrowError::UninitializedAdmin)
    }

    fn get_acbu_token(env: &Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(EscrowError::UninitializedAcBuToken)
    }

    /// Initialize the escrow contract
    pub fn initialize(env: Env, admin: Address, acbu_token: Address) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(EscrowError::AlreadyInitialized);
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage().instance().set(&DATA_KEY.version, &VERSION);
    }

    /// Create escrow: payer deposits ACBU, payee can claim after release
    /// Escrow ID is unique per payer and provided by caller to prevent collisions
    pub fn create(
        env: Env,
        payer: Address,
        payee: Address,
        amount: i128,
        escrow_id: u64,
    ) -> Result<(), EscrowError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(EscrowError::Paused);
        }
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        payer.require_auth();
        let key = EscrowId(payer.clone(), escrow_id);

        if env.storage().temporary().has(&key) {
            return Err(EscrowError::EscrowExists);
        }

        let acbu = Self::get_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // CEI: write state before the external token transfer so any token-level
        // callback observes the escrow as already recorded.
        env.storage()
            .temporary()
            .set(&key, &(payer.clone(), payee.clone(), amount));

        client.transfer(&payer, &env.current_contract_address(), &amount);

        env.events().publish(
            (symbol_short!("esc_crtd"), escrow_id),
            EscrowCreatedEvent {
                escrow_id,
                payer,
                payee,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    /// Release escrow: payee receives ACBU (payer authorization required)
    /// caller must supply payer and escrow_id to identify which escrow to release
    pub fn release(env: Env, escrow_id: u64, payer: Address) -> Result<(), EscrowError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(EscrowError::Paused);
        }

        payer.require_auth();

        let key = EscrowId(payer.clone(), escrow_id);
        let (stored_payer, payee, amount): (Address, Address, i128) = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(EscrowError::EscrowNotFound)?;
        if stored_payer != payer {
            return Err(EscrowError::PayerMismatch);
        }

        let acbu = Self::get_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // CEI: remove the escrow record before the external transfer so the
        // escrow cannot be claimed a second time if the token executes a callback.
        env.storage().temporary().remove(&key);

        client.transfer(&env.current_contract_address(), &payee, &amount);

        env.events().publish(
            (symbol_short!("esc_rel"), escrow_id),
            EscrowReleasedEvent {
                escrow_id,
                payee,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    /// Refund escrow: payer gets ACBU back (admin or dispute resolution)
    /// key is same as release since it identifies which escrow to refund
    pub fn refund(env: Env, escrow_id: u64, payer: Address) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();

        let key = EscrowId(payer.clone(), escrow_id);
        let (stored_payer, _payee, amount): (Address, Address, i128) = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(EscrowError::EscrowNotFound)?;

        if stored_payer != payer {
            return Err(EscrowError::PayerMismatch);
        }

        let acbu = Self::get_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // CEI: remove the escrow record before the external transfer so the
        // escrow cannot be refunded twice if the token executes a callback.
        env.storage().temporary().remove(&key);

        client.transfer(&env.current_contract_address(), &payer, &amount);

        env.events().publish(
            (symbol_short!("esc_ref"), escrow_id),
            EscrowRefundedEvent {
                escrow_id,
                payer,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    pub fn migrate(env: Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::UninitializedAdmin));
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
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::UninitializedAdmin));
        admin.require_auth();
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            env.panic_with_error(EscrowError::Paused);
        }
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}
