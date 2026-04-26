#![cfg(test)]

use acbu_reserve_tracker::{ReserveTrackerContract, ReserveTrackerContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger},
    Address, Env, Symbol, Vec,
};

#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn get_acbu_usd_rate(_env: Env) -> i128 {
        100_000_000 // 1 USD
    }
}

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
