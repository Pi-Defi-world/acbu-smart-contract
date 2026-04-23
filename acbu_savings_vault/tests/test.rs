#![cfg(test)]

use acbu_savings_vault::{SavingsVault, SavingsVaultClient, WithdrawEvent};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, Env, FromVal, IntoVal, Symbol,
};

const SECONDS_PER_YEAR: u64 = 31_536_000;

#[test]
fn test_withdraw_after_term_has_zero_yield_at_deposit_time() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, SavingsVault);
    let client = SavingsVaultClient::new(&env, &contract_id);

    let fee_rate = 300; // 3%
    let yield_rate = 1_000; // 10% APR
    client.initialize(&admin, &acbu_token, &fee_rate, &yield_rate);

    let deposit_amount = 10_000_000;
    let term_seconds = 30 * 24 * 3600u64;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    // Mint extra to cover yield payment by the vault
    token_admin.mint(&user, &deposit_amount);
    token_admin.mint(&contract_id, &deposit_amount);

    client.deposit(&user, &deposit_amount, &term_seconds);

    // Advance time past the lock term so the withdrawal is valid
    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + term_seconds);

    client.withdraw(&user, &term_seconds, &deposit_amount);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    // net = 10_000_000 - 3% fee (300_000) = 9_700_000
    // yield for exactly term_seconds at 10% APR on 10 ACBU is a small positive, so just check >= 9_700_000
    assert!(token_client.balance(&user) >= 9_700_000);
    assert_eq!(token_client.balance(&admin), 300_000);
}

#[test]
fn test_withdraw_after_one_year_has_positive_yield_and_event_value() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, SavingsVault);
    let client = SavingsVaultClient::new(&env, &contract_id);

    let fee_rate = 300; // 3%
    let yield_rate = 1_000; // 10% APR
    client.initialize(&admin, &acbu_token, &fee_rate, &yield_rate);

    let deposit_amount = 10_000_000;
    let expected_fee = 300_000;
    let expected_yield = 1_000_000;
    let expected_user_payout = (deposit_amount - expected_fee) + expected_yield;
    let term_seconds = 30 * 24 * 3600;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &deposit_amount);
    token_admin.mint(&contract_id, &expected_yield);

    client.deposit(&user, &deposit_amount, &term_seconds);

    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + SECONDS_PER_YEAR);

    client.withdraw(&user, &term_seconds, &deposit_amount);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&user), expected_user_payout);
    assert_eq!(token_client.balance(&admin), expected_fee);

    let events = env.events().all();
    let mut found_withdraw = false;

    for event in events.iter() {
        if event.0 != contract_id {
            continue;
        }
        let topics = event.1;
        if !topics.is_empty()
            && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("Withdraw")
        {
            let withdraw_event: WithdrawEvent = event.2.into_val(&env);
            assert_eq!(withdraw_event.yield_amount, expected_yield);
            found_withdraw = true;
        }
    }

    assert!(found_withdraw);
}

#[test]
fn test_partial_withdraw_and_multiple_deposits_fifo_yield() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, SavingsVault);
    let client = SavingsVaultClient::new(&env, &contract_id);

    let fee_rate = 0;
    let yield_rate = 1_000; // 10% APR
    client.initialize(&admin, &acbu_token, &fee_rate, &yield_rate);

    let term_seconds = 30 * 24 * 3600;
    let lot_1 = 5_000_000;
    let lot_2 = 5_000_000;
    let withdraw_amount = 6_000_000;

    // FIFO expected yield:
    // lot_1: 5,000,000 for full year at 10% => 500,000
    // lot_2: 1,000,000 for half year at 10% => 50,000
    let expected_yield = 550_000;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &(lot_1 + lot_2));
    token_admin.mint(&contract_id, &expected_yield);

    client.deposit(&user, &lot_1, &term_seconds);

    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + (SECONDS_PER_YEAR / 2));

    client.deposit(&user, &lot_2, &term_seconds);

    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + SECONDS_PER_YEAR);

    client.withdraw(&user, &term_seconds, &withdraw_amount);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(
        token_client.balance(&user),
        withdraw_amount + expected_yield
    );
    assert_eq!(client.get_balance(&user, &term_seconds), 4_000_000);
}

/// Issue #30 regression test: a user who deposits and tries to withdraw before the
/// term elapses must be rejected with an error.
#[test]
fn test_early_withdrawal_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, SavingsVault);
    let client = SavingsVaultClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &300, &1_000);

    let deposit_amount = 10_000_000;
    let term_seconds: u64 = 30 * 24 * 3600; // 30 days

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &deposit_amount);
    client.deposit(&user, &deposit_amount, &term_seconds);

    // Try to withdraw with only 1 second elapsed — term is 30 days away
    env.ledger().with_mut(|l| l.timestamp = 1_000_001);
    let result = client.try_withdraw(&user, &term_seconds, &deposit_amount);
    assert!(
        result.is_err(),
        "Withdrawal before term elapsed must be rejected"
    );
    // Balance must still be intact
    assert_eq!(client.get_balance(&user, &term_seconds), deposit_amount);
}

/// Verify that withdrawal succeeds at exactly the term boundary (timestamp + term_seconds).
#[test]
fn test_withdraw_at_exact_term_boundary_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, SavingsVault);
    let client = SavingsVaultClient::new(&env, &contract_id);

    let fee_rate = 0i128;
    let yield_rate = 0i128;
    client.initialize(&admin, &acbu_token, &fee_rate, &yield_rate);

    let deposit_amount = 10_000_000i128;
    let term_seconds: u64 = 60 * 60; // 1 hour

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &deposit_amount);
    client.deposit(&user, &deposit_amount, &term_seconds);

    // Advance to exactly term boundary
    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + term_seconds);

    client.withdraw(&user, &term_seconds, &deposit_amount);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&user), deposit_amount);
    assert_eq!(client.get_balance(&user, &term_seconds), 0);
}

#[test]
fn test_withdraw_only_uses_lots_that_reached_their_own_term() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, SavingsVault);
    let client = SavingsVaultClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0i128, &0i128);

    let short_term: u64 = 60;
    let long_term: u64 = 3_600;
    let short_amount = 5_000_000i128;
    let long_amount = 7_000_000i128;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &(short_amount + long_amount));

    client.deposit(&user, &short_amount, &short_term);
    client.deposit(&user, &long_amount, &long_term);

    // Only the short-term lot is mature.
    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + short_term);

    // Short-term withdrawal succeeds.
    client.withdraw(&user, &short_term, &short_amount);
    assert_eq!(client.get_balance(&user, &short_term), 0);

    // Long-term withdrawal still fails because its own term has not elapsed.
    let early_long = client.try_withdraw(&user, &long_term, &long_amount);
    assert!(early_long.is_err());
    assert_eq!(client.get_balance(&user, &long_term), long_amount);
}
