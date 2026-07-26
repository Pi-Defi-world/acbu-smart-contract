#![cfg(test)]

use acbu_minting::{MintingContract, MintingContractClient};
use shared::{calculate_fee, CurrencyCode, MintEvent, DECIMALS};
use soroban_sdk::{
    bytesn, contract, contractimpl, symbol_short,
    testutils::{Address as _, Events},
    Address, BytesN, Env, FromVal, IntoVal, String as SorobanString, Symbol, Vec,
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

#[allow(clippy::too_many_arguments)]
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
#[should_panic(expected = "#5001")]
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

// =====================================================================
// mint_from_usdc
// =====================================================================

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
fn test_mint_from_usdc_with_fee_calculation() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let fee_rate = 300; // 3%

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let usdc_amount = 100 * DECIMALS;
    usdc_token_client.mint(&user, &usdc_amount);

    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

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
        100,
    );

    let mint_amount = 100 * DECIMALS; // Full amount
    let acbu_minted = client.mint_from_usdc(&user, &mint_amount, &user);

    // With 3% fee, we should get 97 * DECIMALS of ACBU
    let expected_acbu = 97_000_000_000;
    assert_eq!(acbu_minted, expected_acbu);
    assert_eq!(acbu_client.balance(&user), expected_acbu);
}

#[test]
#[should_panic(expected = "#5003")]
fn test_mint_from_usdc_below_min_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let tiny_amount = 1; // Way below MIN_MINT_AMOUNT
    usdc_token_client.mint(&user, &tiny_amount);

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

    client.mint_from_usdc(&user, &tiny_amount, &user);
}

#[test]
#[should_panic(expected = "#5003")]
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
fn test_mint_from_usdc_multiple_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let usdc_amount = 100 * DECIMALS;
    usdc_token_client.mint(&user, &usdc_amount);

    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

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

    let mint_amount = 50 * DECIMALS;
    let acbu_minted1 = client.mint_from_usdc(&user, &mint_amount, &recipient1);
    let acbu_minted2 = client.mint_from_usdc(&user, &mint_amount, &recipient2);

    assert_eq!(acbu_client.balance(&recipient1), acbu_minted1);
    assert_eq!(acbu_client.balance(&recipient2), acbu_minted2);
    assert_eq!(client.get_total_supply(), acbu_minted1 + acbu_minted2);
}

#[test]
#[should_panic(expected = "#5012")]
fn test_pause_prevents_mint_from_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let fee_rate = 300;
    let fee_single = 100;

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
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

    assert!(!client.is_paused());
    client.pause();
    assert!(client.is_paused());

    // Attempting to mint while paused should fail
    let mint_amount = 50 * DECIMALS;
    client.mint_from_usdc(&user, &mint_amount, &user);
}

#[test]
fn test_unpause_allows_mint_from_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let fee_rate = 300;
    let fee_single = 100;

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
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

    // Pause and then unpause
    client.pause();
    assert!(client.is_paused());
    client.unpause();
    assert!(!client.is_paused());

    // Minting should now succeed
    let mint_amount = 50 * DECIMALS;
    let acbu_minted = client.mint_from_usdc(&user, &mint_amount, &user);
    assert!(acbu_minted > 0);
}

#[test]
#[should_panic(expected = "#5004")]
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

// =====================================================================
// mint_from_fiat
// =====================================================================
//
// mint_from_fiat is the fintech-partner path: an `operator` address (not the
// depositing user) authorizes the mint, no on-chain token is pulled (the
// fiat leg is settled off-chain by the fintech partner), and duplicate
// `fintech_tx_id`s must be rejected. Fee is charged at `fee_rate` (the
// basket/USDC tier), not `fee_single`.

#[test]
fn test_mint_from_fiat() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    let fee_rate = 50; // mint_from_fiat charges fee_rate, not fee_single
    let fee_single = 100;

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        fee_rate,
        fee_single,
    );

    client.set_operator(&operator);

    let fiat_amount = 50 * DECIMALS;
    // Mock oracle rate is 1:1 (DECIMALS), so usd_gross == fiat_amount.
    let expected_fee = calculate_fee(fiat_amount, fee_rate);
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);
    let tx_id = SorobanString::from_str(&env, "fiat_tx_001");

    let acbu_minted = client.mint_from_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id,
    );

    assert!(acbu_minted > 0);
    assert_eq!(acbu_client.balance(&recipient), acbu_minted);
    // Fee is minted in ACBU to the treasury; no on-chain S-token/vault
    // transfer occurs for this path since settlement happens off-chain.
    assert_eq!(acbu_client.balance(&treasury), expected_fee);
    assert_eq!(client.get_total_supply(), acbu_minted);
}

#[test]
fn test_mint_from_fiat_emits_mint_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );
    client.set_operator(&operator);

    let fiat_amount = 20 * DECIMALS;
    let tx_id = SorobanString::from_str(&env, "fiat_tx_event_01");
    let acbu_minted = client.mint_from_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id,
    );

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
            assert_eq!(event_data.transaction_id, tx_id);
            assert_eq!(event_data.acbu_amount, acbu_minted);
            found = true;
            break;
        }
    }
    assert!(found, "expected mint event for mint_from_fiat");
}

#[test]
#[should_panic(expected = "#5007")]
fn test_mint_from_fiat_wrong_operator() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );
    client.set_operator(&operator);

    let tx_id = SorobanString::from_str(&env, "fiat_tx_attacker");
    client.mint_from_fiat(
        &attacker,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5012")]
fn test_pause_prevents_mint_from_fiat() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    client.pause();
    assert!(client.is_paused());

    let tx_id = SorobanString::from_str(&env, "fiat_tx_paused");
    // admin is the default operator (falls back to admin when unset)
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5008")]
fn test_mint_from_fiat_rejects_duplicate_tx_id() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );
    client.set_operator(&operator);

    let fiat_amount = 50 * DECIMALS;
    let tx_id = SorobanString::from_str(&env, "fiat_tx_duplicate");

    client.mint_from_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id,
    );

    // Same fintech_tx_id must be rejected on replay.
    client.mint_from_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id,
    );
}

#[test]
fn test_mint_from_fiat_different_tx_ids_both_succeed() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let operator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );
    client.set_operator(&operator);

    let fiat_amount = 10 * DECIMALS;
    let tx_id_1 = SorobanString::from_str(&env, "fiat_tx_multi_001");
    let tx_id_2 = SorobanString::from_str(&env, "fiat_tx_multi_002");

    let minted_1 = client.mint_from_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id_1,
    );
    let minted_2 = client.mint_from_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id_2,
    );

    assert!(minted_1 > 0);
    assert!(minted_2 > 0);
    assert_eq!(client.get_total_supply(), minted_1 + minted_2);
}

#[test]
#[should_panic(expected = "#5003")]
fn test_mint_from_fiat_below_min_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    let tx_id = SorobanString::from_str(&env, "fiat_tx_tiny_amount");
    // 1 stroop; well below MIN_MINT_AMOUNT once converted at the (1:1) mock rate.
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &1,
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5003")]
fn test_mint_from_fiat_exceeds_max_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    let tx_id = SorobanString::from_str(&env, "fiat_tx_huge_amount");
    // MAX_MINT_AMOUNT is 1_000_000_000_000; well above that at the 1:1 mock rate.
    let huge_fiat_amount = 2_000_000_000_000;
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &huge_fiat_amount,
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5014")]
fn test_mint_from_fiat_rejects_empty_tx_id() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    let tx_id = SorobanString::from_str(&env, "");
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5015")]
fn test_mint_from_fiat_rejects_too_short_tx_id() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    // Minimum length is 8; this is 5 chars.
    let tx_id = SorobanString::from_str(&env, "short");
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5016")]
fn test_mint_from_fiat_rejects_too_long_tx_id() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    // Maximum length is 64; this is 65 chars.
    let long_id =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 65 'a's
    let tx_id = SorobanString::from_str(&env, long_id);
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5017")]
fn test_mint_from_fiat_rejects_invalid_char_in_tx_id() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        50,
        100,
    );

    // Contains a space, which is outside the allowed [A-Za-z0-9-_] charset.
    let tx_id = SorobanString::from_str(&env, "bad tx id!!");
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

// --- Version / upgrade tests ---

#[test]
fn test_version_set_on_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &admin,
        &admin, 300, 100,
    );
    assert_eq!(client.get_version(), 1);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_same_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &admin,
        &admin, 300, 100,
    );
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    client.upgrade(&dummy_hash, &1u32);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_lower_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &admin,
        &admin, 300, 100,
    );
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    client.upgrade(&dummy_hash, &0u32);
}

#[test]
fn test_storage_state_intact_across_upgrade_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &admin,
        &admin, 300, 100,
    );
    // All configured values must be intact regardless of whether an upgrade is attempted.
    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single(), 100);
    assert_eq!(client.get_total_supply(), 0);
    assert!(!client.is_paused());
}

// --- Dependency address updaters ---

#[test]
fn test_update_oracle_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault,
        &treasury, 100, 200,
    );

    let new_oracle = Address::generate(&env);
    client.update_oracle(&new_oracle);
}

#[test]
fn test_update_reserve_tracker_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault,
        &treasury, 100, 200,
    );

    let new_rt = Address::generate(&env);
    client.update_reserve_tracker(&new_rt);
}

#[test]
fn test_update_acbu_token_by_admin_minting() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault,
        &treasury, 100, 200,
    );

    let new_token = Address::generate(&env);
    client.update_acbu_token(&new_token);
}

#[test]
fn test_update_vault_by_admin_minting() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault,
        &treasury, 100, 200,
    );

    let new_vault = Address::generate(&env);
    client.update_vault(&new_vault);
}

#[test]
fn test_update_treasury_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault,
        &treasury, 100, 200,
    );

    let new_treasury = Address::generate(&env);
    client.update_treasury(&new_treasury);
}

#[test]
fn test_update_usdc_token_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(
        &env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault,
        &treasury, 100, 200,
    );

    let new_usdc = Address::generate(&env);
    client.update_usdc_token(&new_usdc);
}

// --- mint_from_basket ---

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
    let proof_id = SorobanString::from_str(&env, "proof_1");
    let net = client.mint_from_basket(&user, &user, &acbu_amt, &proof_id);
    assert!(net > 0);
    assert_eq!(client.get_total_supply(), acbu_amt);
}

#[test]
#[should_panic(expected = "#5012")]
fn test_pause_prevents_mint_from_basket() {
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

    client.pause();
    assert!(client.is_paused());

    let acbu_amt = 100 * DECIMALS;
    let proof_id = SorobanString::from_str(&env, "proof_1");

    client.mint_from_basket(&user, &user, &acbu_amt, &proof_id);
}

// --- mint_from_demo_fiat ---

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
    let proof = SorobanString::from_str(&env, "demo_proof_001");
    let acbu = client.mint_from_demo_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &proof,
    );
    assert!(acbu > 0);
    assert_eq!(acbu_client.balance(&recipient), acbu);
    assert_eq!(client.get_total_supply(), acbu);
}

#[test]
#[should_panic(expected = "#5007")]
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

    let proof = SorobanString::from_str(&env, "demo_proof_attacker");
    client.mint_from_demo_fiat(
        &attacker,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &proof,
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

    let proof = SorobanString::from_str(&env, "demo_proof_operator");
    let acbu = client.mint_from_demo_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(20 * DECIMALS),
        &proof,
    );
    assert!(acbu > 0);
}

#[test]
#[should_panic(expected = "#5003")]
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
    let proof = SorobanString::from_str(&env, "demo_proof_huge");
    client.mint_from_demo_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &huge_fiat_amount,
        &proof,
    );
}

// --- Set Fee Rate Tests ---

#[test]
fn test_set_fee_rate_updates_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let fee_rate = 300;
    let fee_single = 100;

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

    assert_eq!(client.get_fee_rate(), fee_rate);

    let new_fee_rate = 500;
    client.set_fee_rate(&new_fee_rate);
    assert_eq!(client.get_fee_rate(), new_fee_rate);
}

#[test]
#[should_panic(expected = "#5002")]
fn test_set_fee_rate_exceeds_basis_points() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let fee_rate = 300;
    let fee_single = 100;

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

    let invalid_fee_rate = 10_001;
    client.set_fee_rate(&invalid_fee_rate);
}

#[test]
fn test_set_fee_rate_zero_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let fee_rate = 300;
    let fee_single = 100;

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

    client.set_fee_rate(&0);
    assert_eq!(client.get_fee_rate(), 0);
}

#[test]
fn test_set_fee_rate_max_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let fee_rate = 300;
    let fee_single = 100;

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

    let max_fee_rate = 10_000;
    client.set_fee_rate(&max_fee_rate);
    assert_eq!(client.get_fee_rate(), max_fee_rate);
}

#[test]
fn test_set_fee_rate_affects_mint_calculation() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);

    let usdc_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
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
        300, // 3% fee
        100,
    );

    let mint_amount = 50 * DECIMALS;
    let acbu_minted_first = client.mint_from_usdc(&user, &mint_amount, &user);

    client.set_fee_rate(&500);

    let user2 = Address::generate(&env);
    usdc_token_client.mint(&user2, &usdc_amount);
    let acbu_minted_second = client.mint_from_usdc(&user2, &mint_amount, &user2);

    assert!(acbu_minted_second < acbu_minted_first);
}

// mint_from_fiat also uses fee_rate, so raising it should reduce the net mint too.
#[test]
fn test_set_fee_rate_affects_mint_from_fiat_calculation() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);

    init_mint_client(
        &env,
        &client,
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token_id,
        &usdc_token_id,
        &vault,
        &treasury,
        300, // 3% fee_rate
        100,
    );

    let fiat_amount = 50 * DECIMALS;
    let tx_id_1 = SorobanString::from_str(&env, "fiat_tx_fee_before");
    let minted_before = client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id_1,
    );

    client.set_fee_rate(&500); // raise to 5%

    let tx_id_2 = SorobanString::from_str(&env, "fiat_tx_fee_after");
    let minted_after = client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id_2,
    );

    assert!(minted_after < minted_before);
}
