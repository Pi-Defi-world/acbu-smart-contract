#![cfg(test)]

use acbu_lending_pool::{LendingPool, LendingPoolClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

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

