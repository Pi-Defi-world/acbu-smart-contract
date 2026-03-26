#![cfg(test)]

use acbu_minting::{MintingContract, MintingContractClient};
use shared::{MintEvent, DECIMALS};
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Events},
    Address, Env, FromVal, IntoVal, String as SorobanString, Symbol,
};

// --- Mocks ---

mod oracle_mock {
    use super::*;
    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn get_acbu_usd_rate(_env: Env) -> i128 {
            DECIMALS // 1:1 rate
        }
    }
}

mod reserve_mock {
    use super::*;
    #[contract]
    pub struct MockReserveTracker;

    #[contractimpl]
    impl MockReserveTracker {
        pub fn is_reserve_sufficient(_env: Env, _supply: i128) -> bool {
            true
        }
    }
}

mod failing_reserve_mock {
    use super::*;
    #[contract]
    pub struct MockFailingReserveTracker;

    #[contractimpl]
    impl MockFailingReserveTracker {
        pub fn is_reserve_sufficient(_env: Env, _supply: i128) -> bool {
            false
        }
    }
}

// --- Setup ---

fn setup_test(env: &Env) -> (Address, Address, Address, Address, Address, MintingContractClient) {
    let admin = Address::generate(env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, reserve_mock::MockReserveTracker);
    
    // The MintingContract must be the admin of the ACBU token to be able to mint it.
    let contract_id = env.register_contract(None, MintingContract);
    let acbu_token = env.register_stellar_asset_contract_v2(contract_id.clone()).address();
    
    // USDC token can be admin'd by a random address (e.g. the test admin)
    let usdc_token = env.register_stellar_asset_contract_v2(admin.clone()).address();

    let client = MintingContractClient::new(env, &contract_id);

    (admin, oracle, reserve_tracker, acbu_token, usdc_token, client)
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let fee_rate = 300; // 0.3%

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &fee_rate,
    );

    assert_eq!(client.get_fee_rate(), fee_rate);
    assert_eq!(client.get_total_supply(), 0);
    assert!(!client.is_paused());
}

#[test]
#[should_panic(expected = "Contract already initialized")]
fn test_initialize_twice() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let fee_rate = 300;

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &fee_rate,
    );

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &fee_rate,
    );
}

#[test]
fn test_mint_from_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let fee_rate = 300;

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let usdc_client = soroban_sdk::token::Client::new(&env, &usdc_token_id);
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

    // Initial USDC for user
    let usdc_amount = 100 * 10_000_000;
    usdc_token_client.mint(&user, &usdc_amount);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &fee_rate,
    );

    let mint_amount = 50 * 10_000_000;
    let acbu_minted = client.mint_from_usdc(&user, &mint_amount, &user);

    let expected_fee = 15_000_000; // 0.3% of 50
    let expected_acbu = 485_000_000; // 48.5 (since 1.5 is fee)

    assert_eq!(acbu_minted, expected_acbu);
    assert_eq!(acbu_client.balance(&user), expected_acbu);
    assert_eq!(usdc_client.balance(&user), 50 * 10_000_000);
    assert_eq!(client.get_total_supply(), expected_acbu);

    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        if event.0 != client.address {
            continue;
        }
        let topics = event.1;
        if !topics.is_empty()
            && Symbol::from_val(&env, &topics.get(0).unwrap()) == symbol_short!("mint")
        {
            let event_data: MintEvent = event.2.into_val(&env);
            assert_eq!(event_data.usdc_amount, mint_amount);
            assert_eq!(event_data.acbu_amount, expected_acbu);
            assert_eq!(event_data.fee, expected_fee);
            found = true;
            break;
        }
    }
    assert!(found, "expected mint event");
}

#[test]
fn test_mint_from_fiat() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let fee_rate = 20;

    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &fee_rate,
    );

    let fiat_amount = 1000 * 10_000_000;
    let currency = SorobanString::from_str(&env, "NGN");
    let fintech_tx_id = SorobanString::from_str(&env, "fiat_tx_001");

    let acbu_minted =
        client.mint_from_fiat(&admin, &currency, &fiat_amount, &recipient, &fintech_tx_id);

    let expected_acbu = 998 * 10_000_000;
    assert_eq!(acbu_minted, expected_acbu);
    assert_eq!(acbu_client.balance(&recipient), expected_acbu);
    assert_eq!(client.get_total_supply(), expected_acbu);
}

#[test]
#[should_panic(expected = "Insufficient reserves")]
fn test_mint_insufficient_reserves() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, failing_reserve_mock::MockFailingReserveTracker);
    
    let contract_id = env.register_contract(None, MintingContract);
    let acbu_token = env.register_stellar_asset_contract_v2(contract_id.clone()).address();
    let usdc_token = env.register_stellar_asset_contract_v2(admin.clone()).address();

    let client = MintingContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &0,
    );

    let user = Address::generate(&env);
    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token);
    usdc_sac.mint(&user, &10_000_000);

    client.mint_from_usdc(&user, &10_000_000, &user);
}
