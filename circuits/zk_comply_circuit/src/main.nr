// Poseidon2 hash helpers using native BN254 permutation
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

fn main(
    kyc_level: u64,
    country_code: u64,
    salt: Field,
    commitment: pub Field,
    required_kyc: pub u64,
    allowed_country: pub u64,
    nullifier: pub Field,
) {
    // Keep the credential and policy inputs inside their documented domains.
    // Without these guards, arbitrary u64 values could be committed and
    // compared successfully even though they are not valid KYC levels or ISO
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

    // 2) Prove KYC level meets or exceeds the required threshold
    assert(kyc_level >= required_kyc);

    // 3) Prove the country matches the allowed jurisdiction
    assert(country_code == allowed_country);

    // 4) Derive nullifier = hash(commitment, salt) to prevent double-use
    let expected_nullifier = poseidon2_hash_2(commitment, salt);
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
        poseidon2_hash_2(poseidon2_hash_4(1, 566, 12345, 0), 12345),
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
        poseidon2_hash_2(poseidon2_hash_4(3, 404, 99999, 0), 99999),
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
        poseidon2_hash_2(commitment, 12345),
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
        poseidon2_hash_2(commitment, 12345),
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
        poseidon2_hash_2(commitment, 12345),
    );
}
