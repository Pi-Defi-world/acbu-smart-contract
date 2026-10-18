#![cfg(test)]

use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env, Vec};
use zk_verifier::{ZkVerifier, ZkVerifierClient};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Compute `sha256(wallet.to_xdr())[0..16]` as a little-endian `u128`.
///
/// This mirrors the logic in `ZkVerifier::verify()` exactly so tests can build
/// a correctly caller-bound `public_inputs` vector.
fn wallet_hash(env: &Env, wallet: &Address) -> u128 {
    let xdr: Bytes = wallet.clone().to_xdr(env);
    let digest: BytesN<32> = env.crypto().sha256(&xdr).into();
    let bytes = digest.to_array();
    let mut hash: u128 = 0u128;
    let mut i: u32 = 0;
    while i < 16 {
        hash |= (bytes[i as usize] as u128) << (i * 8);
        i += 1;
    }
    hash
}

/// Deterministic 32-byte commitment from a single seed byte.
fn commitment(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

/// Build a valid 6-element `public_inputs` vector bound to `wallet`.
///
/// Index layout (mirrors `zk/circuits/kyc_verifier/src/main.nr`):
///   [0] min_tier
///   [1] country_code
///   [2] requested_amount
///   [3] daily_cap
///   [4] already_used
///   [5] wallet_address_hash (AZ-032 / W10-SEC-015)
fn public_inputs(env: &Env, wallet: &Address, tier: u128, country: u128) -> Vec<u128> {
    Vec::from_array(
        env,
        [
            tier,
            country,
            100_000_000u128,
            1_000_000_000u128,
            0u128,
            wallet_hash(env, wallet),
        ],
    )
}

/// Register the contract, initialise it, and mock all auths.
fn setup(env: &Env) -> (ZkVerifierClient<'_>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ZkVerifier);
    let client = ZkVerifierClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_sets_admin_and_unpaused() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    assert_eq!(client.admin(), admin);
    assert!(!client.paused());
    assert_eq!(client.get_policy(), None);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")] // Unauthorized
fn test_initialize_twice_panics() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    client.initialize(&Address::generate(&env));
}

// ---------------------------------------------------------------------------
// Compliance policy
// ---------------------------------------------------------------------------

#[test]
fn test_set_and_get_policy() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&2u128, &566u128);

    let policy = client.get_policy().expect("policy must be set");
    assert_eq!(policy.min_tier, 2);
    assert_eq!(policy.allowed_country, 566);
}

// ---------------------------------------------------------------------------
// Commitment registry
// ---------------------------------------------------------------------------

#[test]
fn test_register_commitment_is_attested() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    let c = commitment(&env, 1);

    assert!(!client.is_attested(&c));
    client.register_commitment(&c);
    assert!(client.is_attested(&c));
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")] // CommitmentAlreadyAttested
fn test_register_commitment_twice_panics() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    let c = commitment(&env, 1);

    client.register_commitment(&c);
    client.register_commitment(&c);
}

// ---------------------------------------------------------------------------
// verify() — happy path
// ---------------------------------------------------------------------------

#[test]
fn test_verify_happy_path_records_verification() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);

    assert!(client.is_verified(&wallet));
    assert!(client.verification(&wallet).is_some());
    assert!(client.is_nullifier_spent(&c, &nullifier));
    assert!(!client.is_verified(&Address::generate(&env)));
}

#[test]
fn test_is_verified_false_for_unknown_wallet() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    assert!(!client.is_verified(&Address::generate(&env)));
    assert!(client.verification(&Address::generate(&env)).is_none());
}

// ---------------------------------------------------------------------------
// W10-SEC-015 — proof must be bound to the submitting account
// ---------------------------------------------------------------------------

/// A proof generated for one wallet must not be replayable to mark a different
/// wallet as verified. `public_inputs[5]` is committed inside the proof, so the
/// contract recomputes the caller's hash and rejects any mismatch.
#[test]
#[should_panic(expected = "Error(Contract, #20)")] // ProofCallerMismatch
fn test_verify_rejects_proof_bound_to_another_wallet() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);

    // The proof (and its wallet_address_hash) was created for `victim`.
    let victim = Address::generate(&env);
    let c = commitment(&env, 1);
    client.register_commitment(&c);
    let nullifier = commitment(&env, 9);
    let victim_inputs = public_inputs(&env, &victim, 1, 566);

    // An attacker signs the transaction with their own address but replays the
    // victim's proof. The caller-hash check must reject the call.
    let attacker = Address::generate(&env);
    client.verify(&attacker, &nullifier, &c, &victim_inputs);
}

#[test]
#[should_panic(expected = "Error(Contract, #19)")] // InvalidPublicInputsLength
fn test_verify_rejects_unbound_short_public_inputs() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    // Only 5 inputs — the wallet_address_hash binding is missing.
    let short_inputs = Vec::from_array(&env, [1u128, 566u128, 100_000_000u128, 1_000_000_000u128, 0u128]);
    client.verify(&wallet, &nullifier, &c, &short_inputs);
}

// ---------------------------------------------------------------------------
// verify() — policy enforcement (AZ-001)
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #21)")] // PolicyNotConfigured
fn test_verify_rejects_when_policy_not_configured() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")] // KycTierTooLow
fn test_verify_rejects_tier_below_policy_minimum() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&2u128, &566u128);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    // Prover claims tier 1 but the contract requires tier 2.
    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);
}

#[test]
#[should_panic(expected = "Error(Contract, #23)")] // CountryNotAllowed
fn test_verify_rejects_country_not_allowed() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    // Prover claims country 404 (KE) but the policy only allows 566 (NG).
    let inputs = public_inputs(&env, &wallet, 1, 404);
    client.verify(&wallet, &nullifier, &c, &inputs);
}

// ---------------------------------------------------------------------------
// verify() — commitment / nullifier checks (AZ-002, AZ-014)
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #17)")] // CommitmentNotAttested
fn test_verify_rejects_unattested_commitment() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    // Deliberately do NOT register the commitment.

    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")] // NullifierAlreadySpent
fn test_verify_rejects_replayed_scoped_nullifier() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);

    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);
    // Replaying the same (commitment, nullifier) pair must fail.
    client.verify(&wallet, &nullifier, &c, &inputs);
}

// ---------------------------------------------------------------------------
// Pause / revoke
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "Error(Contract, #2)")] // Paused
fn test_pause_blocks_verification() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);
    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    client.pause();

    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);
}

#[test]
fn test_revoke_verification() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    client.set_policy(&1u128, &566u128);
    let wallet = Address::generate(&env);
    let nullifier = commitment(&env, 9);
    let c = commitment(&env, 1);
    client.register_commitment(&c);

    let inputs = public_inputs(&env, &wallet, 1, 566);
    client.verify(&wallet, &nullifier, &c, &inputs);
    assert!(client.is_verified(&wallet));

    assert!(client.revoke_verification(&wallet));
    assert!(!client.is_verified(&wallet));
    assert!(client.verification(&wallet).is_none());

    // Revoking a wallet with no record returns false.
    assert!(!client.revoke_verification(&Address::generate(&env)));
}
