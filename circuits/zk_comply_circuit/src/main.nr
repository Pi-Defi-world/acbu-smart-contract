// Poseidon2 hash helpers operating over Noir's default BN254 scalar field.
// Note: BN254 and Poseidon2 are NOT natively accelerated by Stellar/Soroban.
// Proof generation and verification run as pure-WASM Rust (no on-chain precompile).
// RATE = 3, state size = 4

fn poseidon2_hash_4(a: Field, b: Field, c: Field, d: Field) -> Field {
    let iv: Field = 4 * 18446744073709551616; // 4 << 64
    let mut state = [0, 0, 0, iv];
    state[0] = state[0] + a;
    state[1] = state[1] + b;
    state[2] = state[2] + c;
    state = std::hash::poseidon2_permutation(state);
    state[0] = state[0] + d;
    state = std::hash::poseidon2_permutation(state);
    state[0]
}

// AZ-001 — Defence-in-depth note
//
// Public inputs are ordered as commitment, required_kyc, allowed_country,
// nullifier, and wallet_address_hash. The prover commits to each value, but
// only the verifier contract's stored policy determines acceptable tier/country.
//
// The **on-chain contract** (`contracts/zk_verifier/src/lib.rs`) closes this
// gap with a second, independent check: it reads its own admin-controlled
// `PolicyParams` from storage and verifies that
//
//   public_inputs[1]  (≡ required_kyc)    ≥  policy.min_tier
//   public_inputs[2]  (≡ allowed_country) == policy.allowed_country
//
// Because the policy lives in contract storage and is only writable by the
// admin, the prover cannot influence these values. Both the circuit proof and
// the server-side policy check must pass for `verify_proof()` to succeed.
fn main(
    kyc_level: u64,
    country_code: u64,
    salt: Field,
    commitment: pub Field,
    required_kyc: pub u64,
    allowed_country: pub u64,
    nullifier: pub Field,
    wallet_address_hash: pub Field,
) {
    // Keep the credential and policy inputs inside their documented domains.
    // Without these guards, arbitrary u64 values could be committed and
    // compared successfully even though they are not valid KYC levels or ISO
    // 3166-1 numeric country codes.
    assert(kyc_level <= 3);
    assert(required_kyc >= 1);
    assert(required_kyc <= 3);
    assert(country_code > 0);
    assert(country_code <= 999);
    assert(allowed_country > 0);
    assert(allowed_country <= 999);

    // 1) Prove knowledge of the credential behind the commitment
    let computed: Field = poseidon2_hash_4(
        kyc_level as Field,
        country_code as Field,
        salt,
        0,
    );
    assert(computed == commitment);

    // 2) Prove KYC level meets or exceeds the required threshold.
    //    Note: the on-chain contract ALSO enforces this against its stored
    //    policy.min_tier, so the prover cannot bypass by setting required_kyc=0.
    assert(kyc_level >= required_kyc);

    // 3) Prove the country matches the allowed jurisdiction.
    //    Note: the on-chain contract ALSO enforces this against its stored
    //    policy.allowed_country, so the prover cannot bypass by choosing freely.
    assert(country_code == allowed_country);

    // 4) Bind the nullifier to the wallet to prevent cross-wallet replay.
    let expected_nullifier = poseidon2_hash_4(
        commitment,
        salt,
        wallet_address_hash,
        0,
    );
    assert(nullifier == expected_nullifier);
}

#[test]
fn test_valid_basic_kyc_ngn() {
    main(
        1,
        566,
        12345,
        poseidon2_hash_4(1, 566, 12345, 0),
        1,
        566,
        poseidon2_hash_4(poseidon2_hash_4(1, 566, 12345, 0), 12345, 9876, 0),
        9876,
    );
}

#[test]
fn test_valid_enterprise_kes() {
    main(
        3,
        404,
        99999,
        poseidon2_hash_4(3, 404, 99999, 0),
        2,
        404,
        poseidon2_hash_4(poseidon2_hash_4(3, 404, 99999, 0), 99999, 8765, 0),
        8765,
    );
}

#[test(should_fail)]
fn test_rejects_kyc_level_outside_documented_range() {
    let commitment = poseidon2_hash_4(4, 566, 12345, 0);
    main(
        1,
        566,
        12345,
        commitment,
        1,
        566,
        poseidon2_hash_4(commitment, 12345, 9876, 0),
        9876,
    );
}

#[test(should_fail)]
fn test_rejects_zero_required_kyc() {
    let commitment = poseidon2_hash_4(1, 566, 12345, 0);
    main(
        1,
        566,
        12345,
        commitment,
        0,
        566,
        poseidon2_hash_4(commitment, 12345, 9876, 0),
        9876,
    );
}

#[test(should_fail)]
fn test_rejects_country_code_outside_iso_numeric_range() {
    let commitment = poseidon2_hash_4(1, 1000, 12345, 0);
    main(
        1,
        1000,
        12345,
        commitment,
        1,
        1000,
        poseidon2_hash_4(commitment, 12345, 9876, 0),
        9876,
    );
}

#[test(should_fail)]
fn test_rejects_zero_country_code() {
    let commitment = poseidon2_hash_4(1, 0, 12345, 0);
    main(
        1,
        0,
        12345,
        commitment,
        1,
        0,
        poseidon2_hash_4(commitment, 12345, 9876, 0),
        9876,
    );
}
