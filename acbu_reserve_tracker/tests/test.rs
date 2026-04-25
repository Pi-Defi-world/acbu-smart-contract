#![cfg(test)]

use acbu_reserve_tracker::{ReserveTrackerContract, ReserveTrackerContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

#[test]
fn verify_reserves_uses_passed_supply_not_contract_balance() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1);

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let min_ratio_bps = 10_000i128; // 100%

    let contract_id = env.register_contract(None, ReserveTrackerContract);
    let client = ReserveTrackerContractClient::new(&env, &contract_id);

    client.initialize(&admin, &oracle, &min_ratio_bps);

    let ngn = CurrencyCode::new(&env, "NGN");
    client.update_reserve(&admin, &ngn, &1_000_000_000, &100_000_000); // 10 USD @ 7 decimals

    // 10 USD reserves vs 10 ACBU supply (10 * 10^7) at 100% min ratio → sufficient
    assert!(client.verify_reserves(&(10 * DECIMALS)));

    // Same reserves vs double the supply → insufficient
    assert!(!client.verify_reserves(&(20 * DECIMALS)));
}
