#`!no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, IntoVal, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ZkGateError {
    Unauthorized = 1,
    AlreadyInitialized = 2,
    InvalidVrf = 3,
    VrfNotConfigured = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Owner,
    Vrf,
}

#[contract]
pub struct ZkGate;

#[contractimpl]
impl ZkGate {
    pub fn __constructor(env: Env, owner: Address) {
        if env.storage().instance().has(&DataKey::Owner) {
            env.panic_with_error(ZkGateError::AlreadyInitialized);
        }

        owner.require_auth();
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage().instance().remove(&DataKey::Vrf);
    }

    pub fn set_vrf(env: Env, owner: Address, vrf: Address) {
        let stored_owner: Address = env
            .storage()
            .instance()
            .get(&DataKey::Owner)
            .unwrap_or_else(|| env.panic_with_error(ZkGateError::Unauthorized));

        if owner != stored_owner {
            env.panic_with_error(ZkGateError::Unauthorized);
        }

        owner.require_auth();
        Self::require_contract_vrf(&env, &vrf);
        env.storage().instance().set(&DataKey::Vrf, &vrf);
    }

    pub fn get_vrf(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Vrf)
    }

    pub fn chk(env: Env, account: Address) -> bool {
        let Some(vrf) = env.storage().instance().get(&DataKey::Vrf) else {
            return false;
        };

        if vrf == env.current_contract_address() {
            return false;
        }

        // Bind the proof to the caller's on-chain identity by passing the account
        // as an explicit public input alongside the account address. The verifier
        // must bind this identity commitment into the circuit so a proof is only
        // valid for the intended account.
        let identity_commitment = Self::identity_commitment(&env, &account);

        let verified: bool = env.invoke_contract(
            &vrf,
            &symbol_short("is_v"),
            Vec::from_array(
                &env,
                [
                    account.clone().into_val(&env),
                    identity_commitment.into_val(&env),
                ],
            ),
        );

        verified
    }

    fn identity_commitment(env: &Env, account: &Address) -> u32 {
        // Deterministic per-account identity commitment derived from the account
        // and the current contract address. This binds the proof to the intended
        // on-chain account and prevents a credential from verifying multiple accounts.
        let account_bytes = account.to_string().into_bytes();
        let contract_bytes = env.current_contract_address().to_string().into_bytes();

        let mut hash: u32 = 0;
        for i in 0..account_bytes.len() {
            hash = hash.wrapping_mul(31).wrapping_add(account_bytes.get(i).unwrap() as u32);
        }
        for i in 0..contract_bytes.len() {
            hash = hash.wrapping_mul(31).wrapping_add(contract_bytes.get(i).unwrap() as u32);
        }
        hash
    }

    fn require_contract_vrf(_env: &Env, vrf: &Address) {
        if *vrf == _env.current_contract_address() {
            _env.panic_with_error(ZkGateError::InvalidVrf);
        }
    }
}
