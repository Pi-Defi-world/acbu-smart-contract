#![cfg(test)]

use acbu_lending_pool::{LendingPool, LendingPoolClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_deposit_and_withdraw_with_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 300; // 3%

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let amount = 10_000_000; // 1000 ACBU

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    assert_eq!(client.get_balance(&lender), amount);
    assert_eq!(client.get_pool_liquidity(), amount);

    client.withdraw(&lender, &amount);

    // Verify lender balance in pool is 0
    assert_eq!(client.get_balance(&lender), 0);

    // Verify lender on-chain balance: 1000 - 3% fee = 970
    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&lender), 9_700_000);

    // Verify admin received fee: 30
    assert_eq!(token_client.balance(&admin), 300_000);
}

#[test]
fn test_borrow_and_repay() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 0;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let amount = 10_000_000;
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);
    client.deposit(&lender, &amount);

    let borrower = Address::generate(&env);
    let loan_amount = 5_000_000;
    let interest_bps = 500; // 5%
    let term = 3600;

    let loan_id = client.borrow(&borrower, &loan_amount, &interest_bps, &term);
    assert_eq!(loan_id, 1);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&borrower), loan_amount);

    // Repay loan: principal + 5% interest = 5,250,000
    token_admin.mint(&borrower, &250_000); // Add interest funds
    client.repay(&loan_id);

    assert_eq!(token_client.balance(&borrower), 0);
    let loan = client.get_loan(&loan_id).unwrap();
    assert!(loan.repaid);
}
