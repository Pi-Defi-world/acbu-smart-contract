#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Env, IntoVal, Map, Vec,
};

use crate::{OracleContract, OracleContractClient, ADMIN_TIMELOCK_SECONDS};
use shared::CurrencyCode;

fn make_env() -> Env {
    Env::default()
}

fn dummy_currencies(env: &Env) -> (Vec<CurrencyCode>, Map<CurrencyCode, i128>) {
    let ngn = CurrencyCode::new(env, "NGN");
    let mut currencies = Vec::new(env);
    currencies.push_back(ngn.clone());
    let mut weights = Map::new(env);
    weights.set(ngn, 10_000i128);
    (currencies, weights)
}

/// Advance the ledger timestamp by `delta` seconds.
fn advance_time(env: &Env, delta: u64) {
    let now = env.ledger().timestamp();
    env.ledger().set(LedgerInfo {
        timestamp: now + delta,
        protocol_version: 20,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 10,
        min_persistent_entry_ttl: 10,
        max_entry_ttl: 3_110_400,
    });
}

fn setup() -> (Env, Address, OracleContractClient<'static>) {
    let env = make_env();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, OracleContract);
    let client = OracleContractClient::new(&env, &contract_id);

    let validators: Vec<Address> = {
        let mut v = Vec::new(&env);
        v.push_back(Address::generate(&env));
        v
    };
    let (currencies, weights) = dummy_currencies(&env);

    env.mock_all_auths();
    client.initialize(&admin, &validators, &1u32, &currencies, &weights);
    (env, admin, client)
}

// ─── happy path ──────────────────────────────────────────────────────────────

#[test]
fn test_transfer_and_accept_after_timelock() {
    let (env, _admin, client) = setup();
    let new_admin = Address::generate(&env);

    env.mock_all_auths();

    // Step 1 – initiate
    client.transfer_admin(&new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));

    let eta = client.get_pending_admin_eligible_at().unwrap();
    assert_eq!(eta, env.ledger().timestamp() + ADMIN_TIMELOCK_SECONDS);

    // Step 2 – advance past timelock and accept
    advance_time(&env, ADMIN_TIMELOCK_SECONDS + 1);
    client.accept_admin();

    assert_eq!(client.get_admin(), new_admin);
    assert!(client.get_pending_admin().is_none());
    assert!(client.get_pending_admin_eligible_at().is_none());
}

#[test]
fn test_cancel_clears_pending_state() {
    let (env, _admin, client) = setup();
    let new_admin = Address::generate(&env);

    env.mock_all_auths();
    client.transfer_admin(&new_admin);
    client.cancel_admin_transfer();

    assert!(client.get_pending_admin().is_none());
    // Original admin unchanged
    assert_ne!(client.get_admin(), new_admin);
}

#[test]
fn test_replace_pending_nomination() {
    let (env, _admin, client) = setup();
    let wrong_addr = Address::generate(&env);
    let correct_addr = Address::generate(&env);

    env.mock_all_auths();
    client.transfer_admin(&wrong_addr);
    // Correct the mistake before timelock expires
    client.transfer_admin(&correct_addr);
    assert_eq!(client.get_pending_admin(), Some(correct_addr.clone()));

    advance_time(&env, ADMIN_TIMELOCK_SECONDS + 1);
    client.accept_admin();
    assert_eq!(client.get_admin(), correct_addr);
}

// ─── sad paths ───────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Admin transfer timelock has not elapsed")]
fn test_accept_before_timelock_panics() {
    let (env, _admin, client) = setup();
    let new_admin = Address::generate(&env);

    env.mock_all_auths();
    client.transfer_admin(&new_admin);
    // Do NOT advance time → should panic
    client.accept_admin();
}

#[test]
#[should_panic(expected = "No pending admin transfer")]
fn test_accept_without_pending_panics() {
    let (_env, _admin, client) = setup();
    client.accept_admin();
}

#[test]
#[should_panic(expected = "No pending admin transfer to cancel")]
fn test_cancel_without_pending_panics() {
    let (env, _admin, client) = setup();
    env.mock_all_auths();
    client.cancel_admin_transfer();
}

#[test]
#[should_panic]
fn test_transfer_admin_requires_current_admin_auth() {
    let (env, _admin, client) = setup();
    let attacker = Address::generate(&env);
    let victim = Address::generate(&env);

    // Only mock auth for the attacker (not the real admin) → should panic
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "transfer_admin",
            args: (&victim,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.transfer_admin(&victim);
}
