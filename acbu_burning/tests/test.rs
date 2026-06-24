#![cfg(test)]

#[path = "common/mod.rs"]
mod common;
mod redeem_single;
mod redeem_basket;

use common::{create_stoken, setup_test};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};

#[test]
fn test_burning_initialize_and_version() {
    let env = Env::default();
    let ctx = setup_test(&env);

    assert_eq!(ctx.burning.version(), 1);
    assert_eq!(ctx.burning.get_fee_rate(), 100);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 200);
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    let ctx = setup_test(&env);

    assert!(!ctx.burning.is_paused());

    ctx.burning.pause();
    assert!(ctx.burning.is_paused());

    ctx.burning.unpause();
    assert!(!ctx.burning.is_paused());
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_redeem_single_when_paused() {
    let env = Env::default();
    let ctx = setup_test(&env);

    let currency = CurrencyCode::new(&env, "NGN");
    let (stoken_id, stoken_client, stoken_sac) = create_stoken(&env, &ctx.admin);
    ctx.oracle.set_stoken(&currency, &stoken_id);

    let burn_amount: i128 = 100 * DECIMALS;
    ctx.acbu_token.mint(&ctx.user, &burn_amount);

    let vault_amount: i128 = 500 * DECIMALS;
    stoken_sac.mint(&ctx.vault, &vault_amount);
    stoken_client.approve(&ctx.vault, &ctx.burning_id, &vault_amount, &200u32);

    let ts = env.ledger().timestamp();
    ctx.oracle.set_acbu_rate(&DECIMALS, &ts);
    ctx.oracle.set_currency_rate(&currency, &DECIMALS);
    ctx.oracle.set_timestamp(&currency, &ts);

    ctx.burning.pause();

    let recipient = Address::generate(&env);
    ctx.burning
        .redeem_single(&ctx.user, &recipient, &burn_amount, &currency);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_redeem_basket_when_paused() {
    let env = Env::default();
    let ctx = setup_test(&env);

    let currency = CurrencyCode::new(&env, "NGN");
    let (stoken_id, _, stoken_sac) = create_stoken(&env, &ctx.admin);
    ctx.oracle.set_stoken(&currency, &stoken_id);

    let burn_amount: i128 = 100 * DECIMALS;
    ctx.acbu_token.mint(&ctx.user, &burn_amount);

    let vault_amount: i128 = 500 * DECIMALS;
    stoken_sac.mint(&ctx.vault, &vault_amount);

    let ts = env.ledger().timestamp();
    ctx.oracle.set_acbu_rate(&DECIMALS, &ts);
    ctx.oracle.set_currency_rate(&currency, &DECIMALS);
    ctx.oracle.set_timestamp(&currency, &ts);

    ctx.burning.pause();

    let recipients = vec![&env, Address::generate(&env)];
    ctx.burning
        .redeem_basket(&ctx.user, &recipients, &burn_amount);
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
fn test_redeem_basket_rejects_empty_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let ctx = setup_test(&env);
    let empty: Vec<Address> = Vec::new(&env);
    let result = ctx
        .burning
        .try_redeem_basket(&ctx.user, &empty, &(100 * DECIMALS));
    assert!(result.is_err());
}

#[test]
fn test_redeem_basket_rejects_duplicate_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let ctx = setup_test(&env);
    let dup = Address::generate(&env);
    let r2 = Address::generate(&env);
    let recipients = vec![&env, dup.clone(), r2, dup.clone()];
    let result = ctx
        .burning
        .try_redeem_basket(&ctx.user, &recipients, &(100 * DECIMALS));
    assert!(result.is_err());
}

// --- Upgrade path tests (issue #242) ---

#[test]
fn test_version_set_on_initialize() {
    let env = Env::default();
    let ctx = setup_test(&env);
    assert_eq!(ctx.burning.version(), 1);
}
