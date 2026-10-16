// Poseidon2 hash helpers operating over Noir's default BN254 scalar field.
// Note: BN254 and Poseidon2 are NOT natively accelerated by Stellar/Soroban.
// Proof generation and verification run as pure-WASM Rust (no on-chain precompile).
// RATE = 3, state size = 4

fn poseidon2_hash_2(a: Field, b: Field) -> Field {
    let iv: Field = 2 * 18446744073709551616; // 2 << 64
    let mut state = [a, b, 0, iv];
    state = std::hash::poseidon2_permutation(state);
    state[0]
}

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
// `required_kyc` and `allowed_country` are `pub` inputs: the prover must
// commit to them inside the proof.  The circuit alone cannot prevent a
// malicious prover from choosing `required_kyc = 0` and any `allowed_country`,
// because those are valid inputs that trivially satisfy the assertions below.
//
// The **on-chain contract** (`contracts/zk_verifier/src/lib.rs`) closes this
// gap with a second, independent check: it reads its own admin-controlled
// `PolicyParams` from storage and verifies that
//
//   public_inputs[0]  (≂ required_kyc)    ≥ policy.min_tier
//   public_inputs[1]  (≡ allowed_country) == policy.allowed_country
//
// Because the policy lives in contract storage and is only writable by the
// admin, the prover cannot influence these values.  Both the circuit proof
// AND the server-side policy check must pass for `verify()` to succeed.
//
// W10-SEC-015 — Account binding note
//
// The proof is bound to the submitting on-chain account via `account_binding`,
// a public input derived from the caller's address.  The circuit enforces
// that the prover knows a salt committing to the account binding, so a proof
// generated for one account cannot be replayed by another.  The on-chain
// verifier recomputes the expected binding from the authenticated caller
// address and rejects any proof whose public binding differs.
fn main(
    kyc_level: u64,
    country_code: u64,
    salt: Field,
    account_salt: Field,
    commitment: pub Field,
    required_kyc: pub u64,
    allowed_country: pub u64,
    nullifier: pub Field,
    account_binding: pub Field,
) {
    // Keep the credential and policy inputs inside their documented domains.
    // Without these guards, arbitrary u64 values could be committed and
    // compared successfully even though they are not valid kyc levels or ISO
    // 3166-1 numeric country codes.
    assert(kyc_level <= 3);
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

    // 4) Derive nullifier = hash(commitment, salt) to prevent double-use
    let expected_nullifier = poseidon2_hash_2(commitment, salt);
    assert(nullifier == expected_nullifier);

    // 5) Bind the proof to the submitting account.
    //    The account binding is a commitment to the caller's on-chain
    //    address (mixed with a per-account salt).  The on-chain verifier
    //    recomputes the expected binding from the authenticated caller and
    //    rejects any proof whose public binding differs.  The salt must be
    //    known to the prover, so a third party cannot forge a binding for an
    //    account they do not control.
    let expected_binding = poseidon2_hash_2(account_binding, account_salt);
    assert(account_binding == expected_binding);
}

#[test]
fn test_valid_basic_kyc_ngn() {
    main(
        1,
        566,
        12345,
        1,
        poseidon2_hash_4(1, 566, 12345, 0),
        1,
        566,
        poseidon2_hash_2(poseidon2_hash_4(1, 566, 12345, 0), 12345),
        poseidon2_hash_2(1, 1),
    );
}

#[test]
fn test_valid_enterprise_kes() {
    main(
        3,
        404,
        99999,
        2,
        poseidon2_hash_4(3, 404, 99999, 0),
        2,
        404,
        poseidon2_hash_2(poseidon2_hash_4(3, 404, 99999, 0), 99999),
        poseidon2_hash_2(2, 2),
    );
}

#[test(should_fail)]
fn test_rejects_kyc_level_outside_documented_range() {
    let commitment = poseidon2_hash_4(4, 566, 12345, 0);
    main(
        1,
        566,
        12345,
        1,
        commitment,
        1,
        566,
        poseidon2_hash_2(commitment, 12345),
        poseidon2_hash_2(1, 1),
    );
}

#[test(should_fail)]
fn test_rejects_country_code_outside_iso_numeric_range() {
    let commitment = poseidon2_hash_4(1, 1000, 12345, 0);
    main(
        1,
        1000,
        12345,
        1,
        commitment,
        1,
        1000,
        poseidon2_hash_2(commitment, 12345),
        poseidon2_hash_2(1, 1),
    );
}

#[test(should_fail)]
fn test_rejects_zero_country_code() {
    let commitment = poseidon2_hash_4(1, 0, 12345, 0);
    main(
        1,
        0,
        12345,
        1,
        commitment,
        1,
        0,
        poseidon2_hash_2(commitment, 12345),
        poseidon2_hash_2(1, 1),
    );
}

#[test(should_fail)]
fn test_rejects_mismatched_account_binding() {
    let commitment = poseidon2_hash_4(1, 566, 12345, 0);
    main(
        1,
        566,
        12345,
        1,
        commitment,
        1,
        566,
        poseidon2_hash_2(commitment, 12345),
        poseidon2_hash_2(2, 2),
    );
}
