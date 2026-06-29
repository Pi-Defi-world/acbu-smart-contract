#![cfg(test)]

use acbu_reserve_tracker::{ReserveTrackerContract, ReserveTrackerContractClient};
use shared::{CurrencyCode, ReserveData, DECIMALS};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ── Mock contracts in isolated modules to prevent symbol-name collisions ──────

mod mock_oracle {
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn get_acbu_usd_rate(_env: Env) -> i128 {
            100_000_000 // 1 USD (8 decimals)
        }
    }
}

mod mock_token {
    use shared::DECIMALS;
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn get_total_supply(_env: Env) -> i128 {
            10 * DECIMALS
        }
    }
}

mod mock_token_zero {
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct MockTokenZero;

    #[contractimpl]
    impl MockTokenZero {
        pub fn get_total_supply(_env: Env) -> i128 {
            0
        }
    }
}

use mock_oracle::MockOracle;
use mock_token::MockToken;
use mock_token_zero::MockTokenZero;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn verify_reserves_uses_passed_supply_not_contract_balance() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);
    let min_ratio_bps = 10_000i128; // 100%

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    let acbu_token = Address::generate(&env);
    client.initialize(&admin, &oracle, &acbu_token, &min_ratio_bps);

    let ngn = CurrencyCode::new(&env, "NGN");
    client.update_reserve(&admin, &ngn, &1_000_000_000, &100_000_000); // 10 USD @ 7 decimals

    // 10 USD reserves vs 10 ACBU supply (10 * 10^7) at 100% min ratio → sufficient
    assert!(client.verify_reserves_manual(&(10 * 10_000_000)));

    // Same reserves vs double the supply → insufficient
    assert!(!client.verify_reserves_manual(&(20 * 10_000_000)));
}

#[test]
fn test_update_and_get_all_reserves_and_timestamp() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 12345);

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    let acbu_token = Address::generate(&env);
    client.initialize(&admin, &oracle, &acbu_token, &10_000i128);

    let ngn = CurrencyCode::new(&env, "NGN");
    client.update_reserve(&admin, &ngn, &500, &(5 * DECIMALS));

    let reserves: soroban_sdk::Map<CurrencyCode, ReserveData> = client.get_all_reserves();
    let mut found = false;
    for (_c, d) in reserves.iter() {
        if d.currency == ngn {
            found = true;
            assert_eq!(d.amount, 500);
            assert_eq!(d.value_usd, 5 * DECIMALS);
            assert_eq!(d.timestamp, 12345);
        }
    }
    assert!(found);

    env.ledger().with_mut(|l| l.timestamp = 22345);
    client.update_reserve(&admin, &ngn, &1000, &(10 * DECIMALS));

    let reserves2: soroban_sdk::Map<CurrencyCode, ReserveData> = client.get_all_reserves();
    let mut found2 = false;
    for (_c, d) in reserves2.iter() {
        if d.currency == ngn {
            found2 = true;
            assert_eq!(d.amount, 1000);
            assert_eq!(d.value_usd, 10 * DECIMALS);
            assert_eq!(d.timestamp, 22345);
        }
    }
    assert!(found2);
}

#[test]
fn test_is_reserve_sufficient_multiple_currencies_and_verify_from_token() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);
    let token = env.register_contract(None, MockToken);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    client.initialize(&admin, &oracle, &token, &10_000i128); // 100% min ratio

    let ngn = CurrencyCode::new(&env, "NGN");
    let kes = CurrencyCode::new(&env, "KES");

    // 5 USD each -> total 10 USD
    client.update_reserve(&admin, &ngn, &1_000, &(5 * DECIMALS));
    client.update_reserve(&admin, &kes, &2_000, &(5 * DECIMALS));

    // supply 10 ACBU (10 * DECIMALS) → sufficient
    assert!(client.verify_reserves_manual(&(10 * DECIMALS)));

    // supply 20 ACBU → insufficient
    assert!(!client.verify_reserves_manual(&(20 * DECIMALS)));

    // verify_reserves reads MockToken which returns 10 * DECIMALS → sufficient
    assert!(client.verify_reserves());
}

#[test]
fn test_zero_and_negative_total_supply_returns_true() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);
    let token_zero = env.register_contract(None, MockTokenZero);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    client.initialize(&admin, &oracle, &token_zero, &10_000i128);

    let zero: i128 = 0;
    let neg: i128 = -10;
    // verify_reserves_manual bypasses the token read — zero/negative supply
    // is defined as trivially sufficient (no outstanding obligations).
    assert!(client.verify_reserves_manual(&zero));
    assert!(client.verify_reserves_manual(&neg));
}

#[test]
fn test_reset_reserves_by_admin_clears_all_entries() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    let acbu_token = Address::generate(&env);
    client.initialize(&admin, &oracle, &acbu_token, &10_000i128);

    let ngn = CurrencyCode::new(&env, "NGN");
    let kes = CurrencyCode::new(&env, "KES");
    client.update_reserve(&admin, &ngn, &1_000, &(5 * DECIMALS));
    client.update_reserve(&admin, &kes, &2_000, &(5 * DECIMALS));

    assert_eq!(client.get_all_reserves().len(), 2);

    client.reset_reserves();

    assert_eq!(
        client.get_all_reserves().len(),
        0,
        "reset_reserves must wipe all stored reserve entries"
    );
}

#[test]
fn test_reset_reserves_without_admin_auth_fails() {
    let env = Env::default();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    let acbu_token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&admin, &oracle, &acbu_token, &10_000i128);

    let ngn = CurrencyCode::new(&env, "NGN");
    client.update_reserve(&admin, &ngn, &1_000, &(5 * DECIMALS));

    // Provide only the attacker's auth — reset_reserves must reject it.
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "reset_reserves",
            args: ().into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_reset_reserves();
    assert!(
        result.is_err(),
        "reset_reserves must reject callers that are not the admin"
    );

    // Reserves must be untouched after the failed attempt.
    env.mock_all_auths();
    assert_eq!(
        client.get_all_reserves().len(),
        1,
        "reserves must remain intact after a failed reset attempt"
    );
}

#[test]
fn test_verify_reserves_errors_when_total_supply_is_zero() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);
    let token_zero = env.register_contract(None, MockTokenZero);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    client.initialize(&admin, &oracle, &token_zero, &10_000i128);

    // verify_reserves reads from the token; when the token reports zero supply
    // it must error (ZeroSupply = 8003) rather than silently returning true —
    // callers must not rely on verify_reserves as a solvency signal before any
    // tokens are minted.
    let result = client.try_verify_reserves();
    assert!(
        result.is_err(),
        "verify_reserves must error when total_acbu_supply is zero"
    );
}

#[test]
fn test_update_reserve_without_admin_auth_fails() {
    let env = Env::default();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let oracle = env.register_contract(None, MockOracle);

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    let acbu_token = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&admin, &oracle, &acbu_token, &10_000i128);

    let ngn = CurrencyCode::new(&env, "NGN");

    // Provide only the attacker auth, update_reserve must reject it
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "update_reserve",
            args: (attacker.clone(), ngn.clone(), 1_000i128, 5i128 * DECIMALS).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_update_reserve(&attacker, &ngn, &1_000, &(5 * DECIMALS));
    assert!(
        result.is_err(),
        "update_reserve must reject callers that are not the admin"
    );
}
