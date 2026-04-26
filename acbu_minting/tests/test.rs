#![cfg(test)]

use acbu_minting::{MintingContract, MintingContractClient};
use shared::{CurrencyCode, MintEvent, DECIMALS};
use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Events},
    Address, Env, FromVal, IntoVal, Symbol, Vec,
};

// --- Mocks ---

mod oracle_mock {
    use super::*;
    use shared::CurrencyCode;

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn get_acbu_usd_rate(_env: Env) -> i128 {
            DECIMALS
        }

        pub fn get_acbu_usd_rate_with_timestamp(env: Env) -> (i128, u64) {
            (DECIMALS, env.ledger().timestamp())
        }

        pub fn get_currencies(env: Env) -> Vec<CurrencyCode> {
            let mut v = Vec::new(&env);
            v.push_back(CurrencyCode::new(&env, "NGN"));
            v
        }

        pub fn get_basket_weight(_env: Env, _c: CurrencyCode) -> i128 {
            10_000
        }

        pub fn get_rate(_env: Env, _c: CurrencyCode) -> i128 {
            DECIMALS
        }

        pub fn get_rate_with_timestamp(env: Env, _c: CurrencyCode) -> (i128, u64) {
            (DECIMALS, env.ledger().timestamp())
        }

        pub fn get_s_token_address(env: Env, _c: CurrencyCode) -> Address {
            env.storage()
                .instance()
                .get(&symbol_short!("STK"))
                .expect("seed_stoken not called in test")
        }

        pub fn seed_stoken(env: Env, stoken: Address) {
            env.storage().instance().set(&symbol_short!("STK"), &stoken);
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

fn init_mint_client(
    _env: &Env,
    client: &MintingContractClient,
    admin: &Address,
    oracle: &Address,
    reserve_tracker: &Address,
    acbu_token: &Address,
    usdc_token: &Address,
    vault: &Address,
    treasury: &Address,
    fee_rate: i128,
    fee_single: i128,
) {
    client.initialize(
        admin,
        oracle,
        reserve_tracker,
        acbu_token,
        usdc_token,
        vault,
        treasury,
        &fee_rate,
        &fee_single,
    );
}

// --- Setup ---

fn setup_test(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    Address,
    Address,
    MintingContractClient,
) {
    let admin = Address::generate(env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, reserve_mock::MockReserveTracker);

    let contract_id = env.register_contract(None, MintingContract);
    let acbu_token = env
        .register_stellar_asset_contract_v2(contract_id.clone())
        .address();

    let usdc_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let client = MintingContractClient::new(env, &contract_id);

    (
        admin,
        oracle,
        reserve_tracker,
        acbu_token,
        usdc_token,
        client,
    )
}

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let fee_rate = 300;
    let fee_single = 100;

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &admin,
        &admin,
        fee_rate,
        fee_single,
    );

    assert_eq!(client.get_fee_rate(), fee_rate);
    assert_eq!(client.get_fee_single(), fee_single);
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
    let fee_single = 100;

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &admin,
        &admin,
        fee_rate,
        fee_single,
    );

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &admin,
        &admin,
        fee_rate,
        fee_single,
    );
}

#[test]
fn test_mint_from_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let fee_rate = 300;
    let fee_single = 100;

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let usdc_client = soroban_sdk::token::Client::new(&env, &usdc_token_id);
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

    let usdc_amount = 100 * DECIMALS;
    usdc_token_client.mint(&user, &usdc_amount);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        fee_rate,
        fee_single,
    );

    let mint_amount = 50 * DECIMALS;
    let acbu_minted = client.mint_from_usdc(&user, &mint_amount, &user);

    let expected_fee = 15_000_000;
    let expected_acbu = 485_000_000;

    assert_eq!(acbu_minted, expected_acbu);
    assert_eq!(acbu_client.balance(&user), expected_acbu);
    assert_eq!(usdc_client.balance(&user), 50 * DECIMALS);
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
fn test_mint_from_basket() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&user, &(1_000 * DECIMALS));

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken_id);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        50,
        100,
    );

    let acbu_amt = 100 * DECIMALS;
    let proof_id = soroban_sdk::String::from_str(&env, "proof_1");
    let net = client.mint_from_basket(&user, &user, &acbu_amt, &proof_id);
    assert!(net > 0);
    assert_eq!(client.get_total_supply(), acbu_amt);
}

#[test]
#[should_panic(expected = "Insufficient reserves")]
fn test_mint_insufficient_reserves() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker =
        env.register_contract(None, failing_reserve_mock::MockFailingReserveTracker);

    let contract_id = env.register_contract(None, MintingContract);
    let acbu_token = env
        .register_stellar_asset_contract_v2(contract_id.clone())
        .address();
    let usdc_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let client = MintingContractClient::new(&env, &contract_id);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &usdc_token,
        &admin,
        &admin,
        0,
        100,
    );

    let user = Address::generate(&env);
    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token);
    usdc_sac.mint(&user, &DECIMALS);

    client.mint_from_usdc(&user, &DECIMALS, &user);
}

#[test]
fn test_mint_from_demo_fiat() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(100 * DECIMALS));
    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken_id);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        50,
        100,
    );

    let fiat_amount = 50 * DECIMALS;
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);
    let tx_id = soroban_sdk::String::from_str(&env, "tx_1");
    let acbu = client.mint_from_demo_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id,
    );
    assert!(acbu > 0);
    assert_eq!(acbu_client.balance(&recipient), acbu);
    assert_eq!(client.get_total_supply(), acbu);
}

#[test]
#[should_panic(expected = "Unauthorized operator")]
fn test_mint_from_demo_fiat_wrong_operator() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();
    let attacker = Address::generate(&env);

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(100 * DECIMALS));
    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken_id);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        50,
        100,
    );

    let tx_id = soroban_sdk::String::from_str(&env, "tx_bad");
    client.mint_from_demo_fiat(
        &attacker,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
fn test_set_operator_and_mint_demo_fiat() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(100 * DECIMALS));
    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken_id);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        50,
        100,
    );

    client.set_operator(&operator);
    assert_eq!(client.get_operator(), operator);

    let tx_id = soroban_sdk::String::from_str(&env, "tx_ok");
    let acbu = client.mint_from_demo_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(20 * DECIMALS),
        &tx_id,
    );
    assert!(acbu > 0);
}

#[test]
#[should_panic(expected = "Invalid mint amount")]
fn test_mint_from_usdc_exceeds_max() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);

    // Max mint amount is 1_000_000_000_000, so 2_000_000_000_000 is huge
    let huge_amount = 2_000_000_000_000;
    usdc_sac.mint(&user, &huge_amount);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        300,
        100,
    );

    client.mint_from_usdc(&user, &huge_amount, &user);
}

#[test]
#[should_panic(expected = "Invalid mint amount")]
fn test_mint_from_demo_fiat_exceeds_max() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(2_000_000_000_000));
    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken_id);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &admin,
        &admin,
        50,
        100,
    );

    client.set_operator(&operator);

    let huge_fiat_amount = 2_000_000_000_000;
    // huge_fiat_amount converted to USD gross will exceed max (given 1:1 rate in MockOracle)
    let tx_id = soroban_sdk::String::from_str(&env, "tx_huge");
    client.mint_from_demo_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &huge_fiat_amount,
        &tx_id,
    );
}
