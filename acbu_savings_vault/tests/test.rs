#![cfg(test)]

use acbu_savings_vault::{SavingsVault, SavingsVaultClient, WithdrawEvent};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, Env, FromVal, IntoVal, Symbol,
};

const SECONDS_PER_YEAR: u64 = 31_536_000;

#[test]
fn test_withdraw_immediate_has_zero_yield() {
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
    let term_seconds = 30 * 24 * 3600;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &deposit_amount);

    client.deposit(&user, &deposit_amount, &term_seconds);
    client.withdraw(&user, &term_seconds, &deposit_amount);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&user), 9_700_000);
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
