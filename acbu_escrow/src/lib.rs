#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub paused: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    paused: symbol_short!("PAUSED"),
};

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
    /// Initialize the escrow contract
    pub fn initialize(env: Env, admin: Address, acbu_token: Address) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            panic!("Contract already initialized");
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    /// Create escrow: payer deposits ACBU, payee can claim after release
    /// Escrow ID is unique per payer and provided by caller to prevent collisions
    pub fn create(
        env: Env,
        payer: Address,
        payee: Address,
        amount: i128,
        escrow_id: u64,
    ) -> Result<(), soroban_sdk::Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(3001));
        }
        if amount <= 0 {
            return Err(soroban_sdk::Error::from_contract_error(3002));
        }
        payer.require_auth();

        // Scope key to (payer, escrow_id) — prevents collisions across payers
        let key = EscrowId(payer.clone(), escrow_id);

        // Guard against silent overwrite of an existing escrow
        if env.storage().temporary().has(&key) {
            return Err(soroban_sdk::Error::from_contract_error(3005)); // ESCROW_ALREADY_EXISTS
        }

        // Resolve stored token address from instance storage.
        let acbu: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .expect("acbu_token not set — contract not initialized");
        let client = soroban_sdk::token::Client::new(&env, &acbu);
        client.transfer(&payer, &env.current_contract_address(), &amount);

        env.storage()
            .temporary()
            .set(&key, &(payer.clone(), payee.clone(), amount));
        // ... events unchanged
        Ok(())
    }

    /// Release escrow: payee receives ACBU (caller must be admin or authorized)
    /// caller must supply payer and escrow_id to identify which escrow to release
    pub fn release(env: Env, escrow_id: u64, payer: Address) -> Result<(), soroban_sdk::Error> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(soroban_sdk::Error::from_contract_error(3001));
        }

        // only admain can trigger release
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .expect("admin not set — contract not initialized");
        admin.require_auth();

        let key = EscrowId(payer.clone(), escrow_id);
        let (_payer, _payee, _amount): (Address, Address, i128) = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(soroban_sdk::Error::from_contract_error(3003))?;

        env.storage().temporary().remove(&key);
        // ... transfer + event unchanged
        Ok(())
    }

    /// Refund escrow: payer gets ACBU back (admin or dispute resolution)
    /// key is same as release since it identifies which escrow to refund
    pub fn refund(env: Env, escrow_id: u64, payer: Address) -> Result<(), soroban_sdk::Error> {
        let _admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .expect("admin not set — contract not initialized");

        let key = EscrowId(payer.clone(), escrow_id);
        let (stored_payer, _payee, _amount): (Address, Address, i128) = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(soroban_sdk::Error::from_contract_error(3003))?;

        if stored_payer != payer {
            return Err(soroban_sdk::Error::from_contract_error(3004));
        }

        env.storage().temporary().remove(&key);
        // ... transfer + event unchanged
        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .expect("admin not set — contract not initialized");
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), soroban_sdk::Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .expect("admin not set — contract not initialized");
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }
}
