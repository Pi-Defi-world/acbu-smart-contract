#![cfg(test)]

use acbu_burning::{BurningContract, BurningContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{contract, contractimpl, symbol_short, testutils::Address as _, vec, Address, Env, Vec};

mod oracle_mock {
    use super::*;
    use shared::CurrencyCode;
    use soroban_sdk::Vec;

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn get_acbu_usd_rate_with_timestamp(env: Env) -> (i128, u64) {
            (DECIMALS, env.ledger().timestamp())
        }

        pub fn get_currencies(env: Env) -> Vec<CurrencyCode> {
            let mut v = Vec::new(&env);
            v.push_back(CurrencyCode::new(&env, "NGN"));
            v.push_back(CurrencyCode::new(&env, "KES"));
            v.push_back(CurrencyCode::new(&env, "GHS"));
            v
        }

        pub fn get_basket_weight(env: Env, c: CurrencyCode) -> i128 {
            if c == CurrencyCode::new(&env, "NGN") || c == CurrencyCode::new(&env, "KES") {
                3_333
            } else if c == CurrencyCode::new(&env, "GHS") {
                3_334
            } else {
                0
            }
        }

        pub fn get_rate(_env: Env, _c: CurrencyCode) -> i128 {
            DECIMALS
        }

        pub fn get_rate_with_timestamp(env: Env, _c: CurrencyCode) -> (i128, u64) {
            (DECIMALS, env.ledger().timestamp())
        }

        pub fn get_s_token_address(env: Env, _c: CurrencyCode) -> Address {
            env.storage()
                .instance()
                .get(&symbol_short!("STK"))
                .expect("seed_stoken")
        }

        pub fn seed_stoken(env: Env, stoken: Address) {
            env.storage().instance().set(&symbol_short!("STK"), &stoken);
        }
    }

    #[contract]
    pub struct MockReserveTracker;

    #[contractimpl]
    impl MockReserveTracker {
        pub fn is_reserve_sufficient(_env: Env, _supply: i128) -> bool {
            true
        }
    }

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn get_total_supply(_env: Env) -> i128 {
            100 * DECIMALS
        }
        pub fn burn(_env: Env, _from: Address, _amount: i128) {}
        pub fn mint(_env: Env, _to: Address, _amount: i128) {}
    }
}

#[test]
fn test_burning_initialize_and_version() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);
    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let withdrawal_processor = Address::generate(&env);
    let vault = admin.clone();

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &300,
        &150,
    );

    assert_eq!(client.version(), 2);
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single_redeem(), 150);
}

#[test]
fn test_redeem_single_transfers_stoken() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &300,
        &150,
    );

    // MockToken doesn't need minting in test as it returns hardcoded supply
    let burn_amt = 100 * DECIMALS;

    let st_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
    st_sac.mint(&vault, &(1_000_000 * DECIMALS));

    let token = soroban_sdk::token::Client::new(&env, &stoken);
    token.approve(&vault, &contract_id, &1_000_000_000_000_000, &100u32);

    let currency = CurrencyCode::new(&env, "NGN");
    let out = client.redeem_single(&user, &recipient, &burn_amt, &currency);
    assert!(out > 0);
}

#[test]
#[should_panic]
fn test_redeem_single_requires_vault_allowance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &300,
        &150,
    );

    let burn_amt = 100 * DECIMALS;
    let currency = CurrencyCode::new(&env, "NGN");
    client.redeem_single(&user, &recipient, &burn_amt, &currency);
}

#[test]
#[should_panic]
fn test_redeem_basket_requires_vault_allowance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &Address::generate(&env),
        &vault,
        &100,
        &150,
    );

    let burn_amt = 100 * DECIMALS;
    client.redeem_basket(&user, &recipient, &burn_amt);
}

#[test]
fn test_redeem_basket() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &Address::generate(&env),
        &vault,
        &100,
        &150,
    );

    let burn_amt = 100 * DECIMALS;

    let st_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
    st_sac.mint(&vault, &(1_000_000 * DECIMALS));

    let token = soroban_sdk::token::Client::new(&env, &stoken);
    token.approve(&vault, &contract_id, &1_000_000_000_000_000, &100u32);

    // C-057: provide 3 distinct recipients (one per basket currency)
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let recipients = vec![&env, r1, r2, r3];

    let amounts = client.redeem_basket(&user, &recipients, &burn_amt);
    assert_eq!(amounts.len(), 3);

    let mut total_out = 0i128;
    for amount in amounts.iter() {
        total_out += amount;
    }
    // With DECIMALS matching and weight sum = 10000, out should be burn_amt - fee
    let expected_fee = (burn_amt * 100) / 10_000;
    assert_eq!(total_out + expected_fee, burn_amt);
}

// C-057: empty recipients list must be rejected
#[test]
fn test_redeem_basket_rejects_empty_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);
    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    client.initialize(
        &admin, &oracle, &reserve_tracker, &acbu_token,
        &Address::generate(&env), &vault, &100, &150,
    );

    let empty: Vec<Address> = Vec::new(&env);
    let result = client.try_redeem_basket(&user, &empty, &(100 * DECIMALS));
    assert!(result.is_err());
}

// C-057: duplicate recipients must be rejected
#[test]
fn test_redeem_basket_rejects_duplicate_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);
    let acbu_token = env.register_contract(None, oracle_mock::MockToken);
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    client.initialize(
        &admin, &oracle, &reserve_tracker, &acbu_token,
        &Address::generate(&env), &vault, &100, &150,
    );

    let dup = Address::generate(&env);
    let r2 = Address::generate(&env);
    // First and third are the same — duplicate
    let recipients = vec![&env, dup.clone(), r2, dup.clone()];
    let result = client.try_redeem_basket(&user, &recipients, &(100 * DECIMALS));
    assert!(result.is_err());
}
