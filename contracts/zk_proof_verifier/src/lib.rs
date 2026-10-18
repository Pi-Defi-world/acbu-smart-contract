#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Bytes, Env};
use ultrazk_soroban_verifier::{UltraHonkVerifier, PROOF_BYTES};

const PUBLIC_INPUT_BYTES: u32 = 5 * 32;
const MAX_VERIFICATION_KEY_BYTES: u32 = 4_096;
const INSTANCE_TTL_THRESHOLD: u32 = 2_628_000;
const INSTANCE_TTL_LEDGERS: u32 = 5_256_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ProofVerifierError {
    Unauthorized = 1,
    InvalidVerificationKey = 2,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    VerificationKey,
}

#[contract]
pub struct ZkProofVerifier;

#[contractimpl]
impl ZkProofVerifier {
    pub fn __constructor(env: Env, admin: Address, verification_key: Bytes) {
        admin.require_auth();
        Self::validate_verification_key(&env, &verification_key);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::VerificationKey, &verification_key);
        Self::extend_instance_ttl(&env);
    }

    pub fn set_verification_key(env: Env, admin: Address, verification_key: Bytes) {
        Self::require_admin(&env, &admin);
        Self::validate_verification_key(&env, &verification_key);
        env.storage()
            .instance()
            .set(&DataKey::VerificationKey, &verification_key);
        Self::extend_instance_ttl(&env);
    }

    pub fn verify(env: Env, proof: Bytes, public_inputs: Bytes) -> bool {
        if proof.len() != PROOF_BYTES as u32 || public_inputs.len() != PUBLIC_INPUT_BYTES {
            return false;
        }

        let Some(verification_key) = env
            .storage()
            .instance()
            .get::<_, Bytes>(&DataKey::VerificationKey)
        else {
            return false;
        };

        let Ok(verifier) = UltraHonkVerifier::new(&env, &verification_key) else {
            return false;
        };
        Self::extend_instance_ttl(&env);
        verifier.verify(&env, &proof, &public_inputs).is_ok()
    }

    fn require_admin(env: &Env, admin: &Address) {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| env.panic_with_error(ProofVerifierError::Unauthorized));
        if *admin != stored_admin {
            env.panic_with_error(ProofVerifierError::Unauthorized);
        }
        admin.require_auth();
    }

    fn validate_verification_key(env: &Env, verification_key: &Bytes) {
        if verification_key.is_empty()
            || verification_key.len() > MAX_VERIFICATION_KEY_BYTES
            || UltraHonkVerifier::new(env, verification_key).is_err()
        {
            env.panic_with_error(ProofVerifierError::InvalidVerificationKey);
        }
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_LEDGERS);
    }
}
