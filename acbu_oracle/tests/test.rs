#![cfg(test)]

use acbu_oracle::{OracleContract, OracleContractClient};
use shared::{CurrencyCode, OutlierDetectionEvent, RateUpdateEvent};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, Env, FromVal, IntoVal, Map, Symbol, Vec,
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
    let validator2 = Address::generate(&env);
    let validator3 = Address::generate(&env);

    let mut validators = Vec::new(&env);
    validators.push_back(validator.clone());
    validators.push_back(validator2);
    validators.push_back(validator3);

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

    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        if event.0 != contract_id {
            continue;
        }
        let topics = event.1;
        if !topics.is_empty()
            && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("rate_upd")
        {
            let event_data: RateUpdateEvent = event.2.into_val(&env);
            assert_eq!(event_data.currency, ngn.clone());
            assert_eq!(event_data.rate, 1235000);
            assert_eq!(event_data.validators, validators);
            found = true;
            break;
        }
    }
    assert!(found, "expected rate_upd event");
}

#[test]
fn test_outlier_detection() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    let admin = Address::generate(&env);
    let validator = Address::generate(&env);
    let validator2 = Address::generate(&env);
    let validator3 = Address::generate(&env);

    let mut validators = Vec::new(&env);
    validators.push_back(validator.clone());
    validators.push_back(validator2);
    validators.push_back(validator3);

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

    let rate = 1234567i128;
    let mut sources = Vec::new(&env);
    // Create sources with one significant outlier
    // Median will be around 1000000
    sources.push_back(1000000i128);   // Normal
    sources.push_back(1005000i128);   // Normal
    sources.push_back(1350000i128);   // Outlier (>3% deviation)

    client.update_rate(&validator, &ngn, &rate, &sources, &env.ledger().timestamp());

    let stored_rate = client.get_rate(&ngn);
    // Median of [1000000, 1005000, 1350000] is 1005000
    assert_eq!(stored_rate, 1005000);

    let events = env.events().all();
    let mut outlier_found = false;
    let mut rate_update_found = false;

    for event in events.iter() {
        if event.0 != contract_id {
            continue;
        }
        let topics = event.1;
        if !topics.is_empty() {
            let event_symbol = Symbol::from_val(&env, &topics.get(0).unwrap());
            
            if event_symbol == symbol_short!("rate_upd") {
                rate_update_found = true;
            } else if event_symbol == symbol_short!("outlier") {
                let event_data: OutlierDetectionEvent = event.2.into_val(&env);
                assert_eq!(event_data.currency, ngn.clone());
                assert_eq!(event_data.median_rate, 1005000);
                assert_eq!(event_data.outlier_rate, 1350000);
                // Deviation should be > 300 bps (3%)
                assert!(event_data.deviation_bps > 300);
                outlier_found = true;
            }
        }
    }
    
    assert!(rate_update_found, "expected rate_upd event");
    assert!(outlier_found, "expected outlier detection event");
}

