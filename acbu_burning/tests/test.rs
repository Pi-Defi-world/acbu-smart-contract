#![cfg(test)]

use acbu_burning::{BurningContract, BurningContractClient};
use shared::{AccountDetails, BurnEvent, CurrencyCode, BASIS_POINTS};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    Address, Env, FromVal, IntoVal, String as SorobanString, Vec,
};

#[test]
fn test_burn_for_basket_fee_accounting() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let reserve_tracker = Address::generate(&env);

    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let withdrawal_processor = Address::generate(&env);
    let fee_rate = 300; // 3% (300 bps)

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &fee_rate,
    );

    let user = Address::generate(&env);
    let acbu_amount = 1_000_000_000; // 1000 with 7 decimals

    let currency_ngn = CurrencyCode::new(&env, "NGN");
    let currency_kes = CurrencyCode::new(&env, "KES");
    let currency_rwf = CurrencyCode::new(&env, "RWF");

    let mut recipients = Vec::new(&env);
    recipients.push_back(AccountDetails {
        account_number: SorobanString::from_str(&env, "123"),
        bank_code: SorobanString::from_str(&env, "bank1"),
        account_name: SorobanString::from_str(&env, "User 1"),
        currency: currency_ngn.clone(),
    });
    recipients.push_back(AccountDetails {
        account_number: SorobanString::from_str(&env, "456"),
        bank_code: SorobanString::from_str(&env, "bank2"),
        account_name: SorobanString::from_str(&env, "User 2"),
        currency: currency_kes.clone(),
    });
    recipients.push_back(AccountDetails {
        account_number: SorobanString::from_str(&env, "789"),
        bank_code: SorobanString::from_str(&env, "bank3"),
        account_name: SorobanString::from_str(&env, "User 3"),
        currency: currency_rwf.clone(),
    });

    // Mock ACBU balance for user
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &acbu_amount);

    // Call burn_for_basket - new signature: user first
    client.burn_for_basket(&user, &acbu_amount, &recipients);

    // Verify events
    let events = env.events().all();
    let mut total_event_acbu = 0i128;
    let mut total_event_fee = 0i128;
    let mut burn_event_count = 0;

    for event in events.iter() {
        if event.0 != contract_id {
            continue;
        }
        let topics = event.1;
        if !topics.is_empty()
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("burn")
        {
            let burn_event: BurnEvent = event.2.into_val(&env);
            total_event_acbu += burn_event.acbu_amount;
            total_event_fee += burn_event.fee;
            burn_event_count += 1;
        }
    }

    assert_eq!(burn_event_count, 3);
    assert_eq!(
        total_event_acbu,
        1_000_000_000 - ((1_000_000_000 * fee_rate) / BASIS_POINTS),
        "Sum of acbu_amount in events should equal net burned"
    );

    let expected_total_fee = (acbu_amount * fee_rate) / BASIS_POINTS;
    assert_eq!(
        total_event_fee, expected_total_fee,
        "Sum of fees in events should equal total fee calculated"
    );
}

#[test]
fn test_burn_for_basket_dust_handling() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let reserve_tracker = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let withdrawal_processor = Address::generate(&env);
    let fee_rate = 0; // 0% to focus on amount dust

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &fee_rate,
    );

    let user = Address::generate(&env);
    // 10,000,001 is not divisible by 13 and exceeds MIN_BURN_AMOUNT
    let acbu_amount = 10_000_001;

    let currency_ngn = CurrencyCode::new(&env, "NGN");

    let mut recipients = Vec::new(&env);
    for _ in 0..13 {
        recipients.push_back(AccountDetails {
            account_number: SorobanString::from_str(&env, "dust_acc"),
            bank_code: SorobanString::from_str(&env, "bank"),
            account_name: SorobanString::from_str(&env, "User"),
            currency: currency_ngn.clone(),
        });
    }

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&user, &acbu_amount);

    client.burn_for_basket(&user, &acbu_amount, &recipients);

    let events = env.events().all();
    let mut total_event_acbu = 0i128;
    for event in events.iter() {
        if event.0 != contract_id {
            continue;
        }
        let topics = event.1;
        if !topics.is_empty()
            && soroban_sdk::Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("burn")
        {
            let burn_event: BurnEvent = event.2.into_val(&env);
            total_event_acbu += burn_event.acbu_amount;
        }
    }

    assert_eq!(
        total_event_acbu, acbu_amount,
        "Every single unit of ACBU (including dust) must be accounted for in events"
    );
}

#[test]
fn test_migrate_and_versioning() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let reserve_tracker = Address::generate(&env);
    let acbu_token = Address::generate(&env);
    let withdrawal_processor = Address::generate(&env);
    let fee_rate = 300;

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &fee_rate,
    );

    // Initial version should be 1
    assert_eq!(client.version(), 1);

    // Call migrate (as admin)
    client.migrate();

    // Still version 1 (since we haven't bumped it in code yet)
    assert_eq!(client.version(), 1);
}

