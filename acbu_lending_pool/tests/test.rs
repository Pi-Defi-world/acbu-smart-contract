#![cfg(test)]

use acbu_lending_pool::{LendingPool, LendingPoolClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};
// Add these imports for the lifecycle test
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

#[test]
fn test_deposit_and_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 300;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let amount = 10_000_000; // 1000 ACBU

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    assert_eq!(client.get_balance(&lender), amount);

    client.withdraw(&lender, &amount);

    assert_eq!(client.get_balance(&lender), 0);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&lender), amount);
}

#[test]
fn test_withdraw_more_than_balance_fails() {
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

    let result = client.try_withdraw(&lender, &(amount + 1));
    assert!(result.is_err());
}

/// Security test: an attacker must NOT be able to deposit on behalf of another address.
#[test]
fn test_unauthorized_deposit_fails() {
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;

    let env = Env::default();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 300;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let attacker = Address::generate(&env);
    let amount: i128 = 10_000_000;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    // Only authorize the *attacker*, not the lender — so lender.require_auth() will fail
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "deposit",
            args: (&lender, amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_deposit(&lender, &amount);
    assert!(result.is_err(), "Unauthorized deposit must be rejected");
}

/// Security test: an attacker must NOT be able to withdraw from another lender's balance.
/// This is the exact exploit described in issue #31 — without `lender.require_auth()`,
/// anyone could call withdraw(lender=victim, amount=Y) and drain the victim's deposited funds.
#[test]
fn test_unauthorized_withdraw_fails() {
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;

    let env = Env::default();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 0;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    // Setup: deposit normally with all auths mocked
    env.mock_all_auths();
    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let attacker = Address::generate(&env);
    let amount: i128 = 10_000_000;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);
    client.deposit(&lender, &amount);

    assert_eq!(client.get_balance(&lender), amount);

    // Now try to withdraw as the attacker — only attacker has auth, not the lender
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "withdraw",
            args: (&lender, amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_withdraw(&lender, &amount);
    assert!(result.is_err(), "Unauthorized withdrawal must be rejected");
    // Lender's balance must remain intact
    assert_eq!(client.get_balance(&lender), amount);
}

// Integration test for #106 - full loan lifecycle with events
#[test]
fn test_loan_lifecycle_emits_events() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_client = TokenClient::new(&env, &token_id);
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &token_id, &0);

    // Fund pool and borrower
    let pool_liquidity = 1_000_000i128;
    let collateral = 500_000i128;
    token_admin_client.mint(&lender, &pool_liquidity);
    token_admin_client.mint(&borrower, &collateral);
    client.deposit(&lender, &pool_liquidity);

    let loan_id = 42u64;
    let borrow_amount = 300_000i128;

    // 1. Borrow - verify state transition + event
    client.borrow(&borrower, &borrow_amount, &collateral, &loan_id);

    // Assert loan state created
    let loan = client.get_loan(&borrower, &loan_id).unwrap();
    assert_eq!(loan.amount, borrow_amount);
    assert_eq!(loan.borrower, borrower);

    // Assert borrow event emitted with correct fields
    let events = env.events().all();
    let borrow_event = events
        .iter()
        .rev()
        .find(|e| {
            e.1.first().map_or(false, |t| {
                t.try_into_val(&env).ok() == Some(symbol_short!("borrow"))
            })
        })
        .expect("borrow event not found");

    // BorrowEvent has 5 fields: creator, amount, token, loan_id, timestamp
    let (event_creator, event_amount, event_token, event_loan_id, _ts): (
        Address,
        i128,
        Address,
        u64,
        u64,
    ) = borrow_event.2.try_into_val(&env).unwrap();
    assert_eq!(event_creator, borrower);
    assert_eq!(event_amount, borrow_amount);
    assert_eq!(event_token, token_id);
    assert_eq!(event_loan_id, loan_id);

    // 2. Repay partial
    let repay_amount = 100_000i128;
    client.repay(&borrower, &repay_amount, &loan_id);

    // Assert state transition - loan amount reduced
    let loan = client.get_loan(&borrower, &loan_id).unwrap();
    assert_eq!(loan.amount, borrow_amount - repay_amount);

    // Assert repay event
    let events = env.events().all();
    let repay_event = events
        .iter()
        .rev()
        .find(|e| {
            e.1.first().map_or(false, |t| {
                t.try_into_val(&env).ok() == Some(symbol_short!("repay"))
            })
        })
        .expect("repay event not found");

    // RepayEvent also has 5 fields: creator, amount, token, loan_id, timestamp
    let (event_creator, event_amount, event_token, event_loan_id, _ts): (
        Address,
        i128,
        Address,
        u64,
        u64,
    ) = repay_event.2.try_into_val(&env).unwrap();
    assert_eq!(event_creator, borrower);
    assert_eq!(event_amount, repay_amount);
    assert_eq!(event_token, token_id);
    assert_eq!(event_loan_id, loan_id);

    // 3. Repay full - loan removed
    client.repay(&borrower, &(borrow_amount - repay_amount), &loan_id);

    // Assert loan deleted after full repayment
    assert!(client.get_loan(&borrower, &loan_id).is_none());
}
