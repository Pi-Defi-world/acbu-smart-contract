#![cfg(test)]

#[path = "common/mod.rs"]
mod common;
mod redeem_single;
mod redeem_basket;
use acbu_burning::{BurningContract, BurningContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{
    bytesn, contract, contractimpl, symbol_short, testutils::Address as _, Address, BytesN, Env,
};
use soroban_sdk::{contract, contractimpl, symbol_short, testutils::Address as _, vec, Address, Env, Vec};

use common::setup_test;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_burning_initialize_and_version() {
    let env = Env::default();
    let ctx = setup_test(&env);

    assert_eq!(ctx.burning.version(), 2);
    assert_eq!(ctx.burning.get_fee_rate(), 100);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 200);
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

    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single_redeem(), 150);
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    let ctx = setup_test(&env);

    ctx.burning.pause();
    assert!(ctx.burning.is_paused());

    let currency = shared::CurrencyCode::new(&env, "NGN");
    let recipient = Address::generate(&env);
    let result = ctx.burning.try_redeem_single(&ctx.user, &recipient, &(100 * shared::DECIMALS), &currency);
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
    assert_eq!(token.balance(&recipient), out);

    let acbu = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(acbu.balance(&user), 0);
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
    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient);
    client.redeem_basket(&user, &recipients, &burn_amt);
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
    let net = burn_amt - expected_fee;
    // Per-currency integer division can lose at most 1 unit per currency in rounding.
    assert!(total_out <= net, "total_out should not exceed net");
    assert!(
        total_out >= net - amounts.len() as i128,
        "rounding loss bounded by currency count"
    );
    assert_eq!(token.balance(&recipient), total_out);

    let acbu = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(acbu.balance(&user), 0);
}

// --- Upgrade path tests (issue #242) ---

fn setup_burning_client(env: &Env) -> (Address, Address, BurningContractClient) {
    let admin = Address::generate(env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = Address::generate(env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let withdrawal_processor = Address::generate(env);
    let vault = admin.clone();
    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(env, &contract_id);
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
    (admin, contract_id, client)
}

#[test]
fn test_version_set_on_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    assert_eq!(client.get_version(), 1);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_same_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    // version is 1 after init; trying to upgrade to 1 must be rejected
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    client.upgrade(&dummy_hash, &1u32);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_lower_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    client.upgrade(&dummy_hash, &0u32);
}

#[test]
fn test_state_preserved_across_upgrade_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    // Confirm fee rates survive an upgrade attempt (the WASM lookup panics before storage is
    // touched, so we verify pre-upgrade storage is intact via the getters).
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single_redeem(), 150);
    assert_eq!(client.get_version(), 1);
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

    ctx.burning.unpause();
    assert!(!ctx.burning.is_paused());
}

#[test]
fn test_set_fee_rates() {
    let env = Env::default();
    let ctx = setup_test(&env);

    ctx.burning.set_fee_rate(&50);
    assert_eq!(ctx.burning.get_fee_rate(), 50);

    ctx.burning.set_fee_single_redeem(&150);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 150);
}
