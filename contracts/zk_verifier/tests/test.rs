#![cfg(test)]

use zk_verifier::{ZkVerifier, ZkVerifierClient};
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env, Vec};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn fixed_bytesn(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1; 32])
}

/// Compute sha256(wallet.to_xdr())[0..16] as a little-endian u128.
/// Mirrors the binding check in ZkVerifier::verify() (AZ-032).
fn wallet_hash(env: &Env, wallet: &Address) -> u128 {
    let xdr: Bytes = wallet.clone().to_xdr(env);
    let digest: BytesN<32> = env.crypto().sha256(&xdr);
    let bytes = digest.to_array();
    let mut h: u128 = 0u128;
    for i in 0..16u32 {
        h |= (bytes[i as usize] as u128) << (i * 8);
    }
    h
}

/// Build a valid 6-element public_inputs Vec bound to `wallet`.
///
/// Index layout (mirrors zk/circuits/kyc_verifier/src/main.nr):
///   [0] min_tier            (1)
///   [1] country_code        (566 = NG)
///   [2] requested_amount    (100_000_000)
///   [3] daily_cap           (1_000_000_000)
///   [4] already_used        (0)
///   [5] wallet_address_hash (sha256(wallet_xdr)[0..16] as u128)
fn valid_public_inputs(env: &Env, wallet: &Address) -> Vec<u128> {
    Vec::from_array(
        env,
        [
            1u128,
            566u128,
            100_000_000u128,
            1_000_000_000u128,
            0u128,
            wallet_hash(env, wallet),
        ],
    )
}

/// Deploy and initialise the contract. Returns the client and admin address.
fn setup(env: &Env) -> (ZkVerifierClient<'_>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ZkVerifier);
    let admin = Address::generate(env);
    let client = ZkVerifierClient::new(env, &contract_id);
    client.initialize(&admin);
    (client, admin)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    assert_eq!(client.admin(), admin);
    assert!(!client.paused());
}

#[test]
#[should_panic]
fn test_initialize_already_initialized_panics() {
    let env = Env::default();
    let (client, _) = setup(&env);
    client.initialize(&Address::generate(&env));
}

// ---------------------------------------------------------------------------
// Pause / unpause
// ---------------------------------------------------------------------------

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    let (client, _) = setup(&env);

    assert!(!client.paused());
    client.pause();
    assert!(client.paused());
    client.unpause();
    assert!(!client.paused());
}

// ---------------------------------------------------------------------------
// Commitment registry
// ---------------------------------------------------------------------------

#[test]
fn test_register_commitment() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let commitment = fixed_bytesn(&env);
    client.register_commitment(&commitment);
    assert!(client.is_attested(&commitment));
}

#[test]
#[should_panic]
fn test_register_commitment_already_attested_panics() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let commitment = fixed_bytesn(&env);
    client.register_commitment(&commitment);
    client.register_commitment(&commitment); // should panic
}

// ---------------------------------------------------------------------------
// verify() — happy path
// ---------------------------------------------------------------------------

#[test]
fn test_verify() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);
    let public_inputs = valid_public_inputs(&env, &wallet);

    client.register_commitment(&commitment);
    client.verify(&wallet, &nullifier, &commitment, &public_inputs);

    assert!(client.is_verified(&wallet));
    assert!(client.is_nullifier_spent(&commitment, &nullifier));
}

// ---------------------------------------------------------------------------
// verify() — error cases
// ---------------------------------------------------------------------------

#[test]
#[should_panic]
fn verification_rejects_wrong_public_inputs_length() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);
    // 5 inputs instead of required 6 → InvalidPublicInputsLength
    let short_inputs: Vec<u128> = Vec::from_array(&env, [0u128; 5]);

    client.register_commitment(&commitment);
    client.verify(&wallet, &nullifier, &commitment, &short_inputs);
}

#[test]
#[should_panic]
fn verification_rejects_unattested_commitment() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);
    let public_inputs = valid_public_inputs(&env, &wallet);

    // commitment never registered → CommitmentNotAttested
    client.verify(&wallet, &nullifier, &commitment, &public_inputs);
}

#[test]
#[should_panic]
fn test_verify_nullifier_reused_panics() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);
    let public_inputs = valid_public_inputs(&env, &wallet);

    client.register_commitment(&commitment);
    client.verify(&wallet, &nullifier, &commitment, &public_inputs);
    // second call with the same nullifier → NullifierAlreadySpent
    client.verify(&wallet, &nullifier, &commitment, &public_inputs);
}

/// AZ-032: Proof must be bound to the submitting wallet.
///
/// public_inputs[5] is computed from `original_wallet`; submitting it under
/// `attacker` must be rejected with ProofCallerMismatch.
#[test]
#[should_panic]
fn test_verify_wrong_caller_panics() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let original_wallet = Address::generate(&env);
    let attacker = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);

    let public_inputs_for_original = valid_public_inputs(&env, &original_wallet);

    client.register_commitment(&commitment);
    client.verify(&attacker, &nullifier, &commitment, &public_inputs_for_original);
}

// ---------------------------------------------------------------------------
// Admin operations
// ---------------------------------------------------------------------------

#[test]
fn admin_can_revoke_verification() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);
    let public_inputs = valid_public_inputs(&env, &wallet);

    client.register_commitment(&commitment);
    client.verify(&wallet, &nullifier, &commitment, &public_inputs);
    assert!(client.is_verified(&wallet));

    client.revoke_verification(&wallet);
    assert!(!client.is_verified(&wallet));
}

#[test]
fn unverified_wallet_is_not_verified() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let unverified = Address::generate(&env);
    assert!(!client.is_verified(&unverified));
}

#[test]
fn unspent_nullifier_reports_false() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let commitment = fixed_bytesn(&env);
    let unspent = BytesN::from_array(&env, &[2; 32]);
    assert!(!client.is_nullifier_spent(&commitment, &unspent));
}

#[test]
#[should_panic]
fn pause_blocks_verification() {
    let env = Env::default();
    let (client, _) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = fixed_bytesn(&env);
    let commitment = fixed_bytesn(&env);
    let public_inputs = valid_public_inputs(&env, &wallet);

    client.register_commitment(&commitment);
    client.pause();
    // should panic with Paused
    client.verify(&wallet, &nullifier, &commitment, &public_inputs);
}
