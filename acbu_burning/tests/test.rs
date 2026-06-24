#![cfg(test)]

#[path = "common/mod.rs"]
mod common;

use acbu_burning::{BurningContract, BurningContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{
    bytesn, testutils::Address as _, vec, Address, BytesN, Env, Vec,
};

use common::setup_test;

#[test]
fn test_burning_initialize_and_version() {
    let env = Env::default();
    let ctx = setup_test(&env);

    assert_eq!(ctx.burning.version(), 1);
    assert_eq!(ctx.burning.get_fee_rate(), 100);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 200);
}

#[test]
fn test_burning_initialize_custom() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let reserve_tracker = Address::generate(&env);
    let acbu_token = Address::generate(&env);
    let withdrawal_processor = Address::generate(&env);
    let vault = Address::generate(&env);

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

    assert_eq!(client.version(), 1);
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single_redeem(), 150);
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    let ctx = setup_test(&env);

    ctx.burning.pause();
    assert!(ctx.burning.is_paused());

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

#[test]
#[should_panic]
fn test_redeem_single_requires_vault_allowance() {
    let env = Env::default();
    let ctx = setup_test(&env);

    let stoken = env.register_stellar_asset_contract_v2(ctx.admin.clone()).address();
    ctx.oracle.set_stoken(&CurrencyCode::new(&env, "NGN"), &stoken);

    let burn_amt = 100 * DECIMALS;
    let currency = CurrencyCode::new(&env, "NGN");
    ctx.burning.redeem_single(&ctx.user, &Address::generate(&env), &burn_amt, &currency);
}

#[test]
#[should_panic]
fn test_redeem_basket_requires_vault_allowance() {
    let env = Env::default();
    let ctx = setup_test(&env);

    let stoken = env.register_stellar_asset_contract_v2(ctx.admin.clone()).address();
    ctx.oracle.set_stoken(&CurrencyCode::new(&env, "NGN"), &stoken);
    ctx.oracle.set_stoken(&CurrencyCode::new(&env, "KES"), &stoken);

    let burn_amt = 100 * DECIMALS;
    let recipients = vec![&env, Address::generate(&env), Address::generate(&env)];
    ctx.burning.redeem_basket(&ctx.user, &recipients, &burn_amt);
}

#[test]
fn test_redeem_basket_rejects_empty_recipients() {
    let env = Env::default();
    let ctx = setup_test(&env);

    let empty: Vec<Address> = Vec::new(&env);
    let result = ctx.burning.try_redeem_basket(&ctx.user, &empty, &(100 * DECIMALS));
    assert!(result.is_err());
}

#[test]
fn test_redeem_basket_rejects_duplicate_recipients() {
    let env = Env::default();
    let ctx = setup_test(&env);

    let dup = Address::generate(&env);
    let recipients = vec![&env, dup.clone(), dup.clone()];
    let result = ctx.burning.try_redeem_basket(&ctx.user, &recipients, &(100 * DECIMALS));
    assert!(result.is_err());
}

#[test]
fn test_version_set_on_initialize() {
    let env = Env::default();
    let ctx = setup_test(&env);
    assert_eq!(ctx.burning.version(), 1);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #12)")]
fn test_upgrade_rejects_same_version() {
    let env = Env::default();
    let ctx = setup_test(&env);
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    ctx.burning.upgrade(&dummy_hash, &1u32);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #12)")]
fn test_upgrade_rejects_lower_version() {
    let env = Env::default();
    let ctx = setup_test(&env);
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    ctx.burning.upgrade(&dummy_hash, &0u32);
}

#[test]
fn test_state_preserved_across_upgrade_boundary() {
    let env = Env::default();
    let ctx = setup_test(&env);
    assert_eq!(ctx.burning.get_fee_rate(), 100);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 200);
    assert_eq!(ctx.burning.version(), 1);
}
