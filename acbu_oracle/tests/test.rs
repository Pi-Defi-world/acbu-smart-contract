#![cfg(test)]

use acbu_oracle::{OracleContract, OracleContractClient};
use shared::CurrencyCode;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Map, Vec,
};

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let validator1 = Address::generate(&env);
    let validator2 = Address::generate(&env);
    let validator3 = Address::generate(&env);

    let mut validators = Vec::new(&env);
    validators.push_back(validator1);
    validators.push_back(validator2);
    validators.push_back(validator3);

    let min_signatures = 2u32;

    let ngn = CurrencyCode::new(&env, "NGN");
    let kes = CurrencyCode::new(&env, "KES");
    let mut currencies = Vec::new(&env);
    currencies.push_back(ngn.clone());
    currencies.push_back(kes.clone());

    let mut basket_weights = Map::new(&env);
    basket_weights.set(ngn.clone(), 1800i128); // 18%
    basket_weights.set(kes.clone(), 1200i128); // 12%

    let contract_id = env.register_contract(None, OracleContract);
    let client = OracleContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &validators,
        &min_signatures,
        &currencies,
        &basket_weights,
    );

    let stored_validators = client.get_validators();
    assert_eq!(stored_validators.len(), 3);
    assert_eq!(client.get_min_signatures(), min_signatures);
}

#[test]
fn test_update_rate() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000); // Exceed 6h interval
    let admin = Address::generate(&env);
    let validator = Address::generate(&env);

    let mut validators = Vec::new(&env);
    validators.push_back(validator.clone());

    let min_signatures = 1u32;

    let ngn = CurrencyCode::new(&env, "NGN");
    let mut currencies = Vec::new(&env);
    currencies.push_back(ngn.clone());

    let mut basket_weights = Map::new(&env);
    basket_weights.set(ngn.clone(), 10000i128); // 100%

    let contract_id = env.register_contract(None, OracleContract);
    let client = OracleContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &validators,
        &min_signatures,
        &currencies,
        &basket_weights,
    );

    let rate = 1234567i128; // 0.1234567 USD per NGN
    let mut sources = Vec::new(&env);
    sources.push_back(1230000i128);
    sources.push_back(1235000i128);
    sources.push_back(1239000i128);

    client.update_rate(&validator, &ngn, &rate, &sources, &env.ledger().timestamp());

    let stored_rate = client.get_rate(&ngn);
    assert_eq!(stored_rate, 1235000);
}
