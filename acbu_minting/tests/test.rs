#![cfg(test)]

use acbu_minting::{MintingContract, MintingContractClient};
use shared::{CurrencyCode, MintEvent, DECIMALS};
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
#[should_panic(expected = "#5003")]
fn test_mint_from_usdc_below_min() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);

    let tiny_amount = 1;
    usdc_sac.mint(&user, &tiny_amount);

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
    let tx_id = soroban_sdk::String::from_str(&env, "tx_1");
    let acbu = client.mint_from_demo_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &proof,
        &tx_id,
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

    let tx_id = soroban_sdk::String::from_str(&env, "tx_bad");
    let proof = SorobanString::from_str(&env, "proof_bad");
    client.mint_from_demo_fiat(
        &attacker,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &proof,
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

    let proof = SorobanString::from_str(&env, "demo_proof_operator");
    let tx_id = soroban_sdk::String::from_str(&env, "tx_ok");
    let acbu = client.mint_from_demo_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(20 * DECIMALS),
        &proof,
        &tx_id,
    );
    assert!(acbu > 0);
}

#[test]
#[should_panic(expected = "#5003")]
fn test_mint_from_usdc_exceeds_max() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);
    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);

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
    let proof = SorobanString::from_str(&env, "demo_proof_huge");
    let tx_id = soroban_sdk::String::from_str(&env, "tx_huge");
    client.mint_from_demo_fiat(
        &operator,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &huge_fiat_amount,
        &proof,
        &tx_id,
    );
}

// --- Upgrade path tests (issue #242) ---

#[test]
fn test_version_set_on_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker,
        &acbu_token, &usdc_token, &admin, &admin, 300, 100);
    assert_eq!(client.get_version(), 1);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_same_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker,
        &acbu_token, &usdc_token, &admin, &admin, 300, 100);
    let dummy_hash: BytesN<32> = bytesn!(&env, 0x0000000000000000000000000000000000000000000000000000000000000000);
    client.upgrade(&dummy_hash, &1u32);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_lower_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker,
        &acbu_token, &usdc_token, &admin, &admin, 300, 100);
    let dummy_hash: BytesN<32> = bytesn!(&env, 0x0000000000000000000000000000000000000000000000000000000000000000);
    client.upgrade(&dummy_hash, &0u32);
}

#[test]
fn test_storage_state_intact_across_upgrade_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker,
        &acbu_token, &usdc_token, &admin, &admin, 300, 100);
    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single(), 100);
    assert_eq!(client.get_total_supply(), 0);
    assert!(!client.is_paused());
}

#[test]
fn test_update_oracle_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

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
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

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
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

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
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

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
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

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
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

    let new_usdc = Address::generate(&env);
    client.update_usdc_token(&new_usdc);
}

#[test]
fn test_update_oracle_requires_admin_minting() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);
    let vault = Address::generate(&env);
    let treasury = Address::generate(&env);
    init_mint_client(&env, &client, &admin, &oracle, &reserve_tracker, &acbu_token, &usdc_token, &vault, &treasury, 100, 200);

    let env2 = Env::default();
    let (admin2, oracle2, rt2, acbu2, usdc2, client2) = setup_test(&env2);
    let vault2 = Address::generate(&env2);
    let treasury2 = Address::generate(&env2);
    env2.mock_all_auths();
    init_mint_client(&env2, &client2, &admin2, &oracle2, &rt2, &acbu2, &usdc2, &vault2, &treasury2, 100, 200);
    let new_oracle = Address::generate(&env2);
    client2.update_oracle(&new_oracle);
}

// ============================================================
// Tests added to resolve Issue #229:
// "No tests for core minting flows"
// Covers: mint_from_fiat, pause/unpause, and fee-rate management
// ============================================================

// --- Pause / Unpause ---

#[test]
fn test_pause_and_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);

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
        300,
        100,
    );

    assert!(!client.is_paused(), "contract should start unpaused");

    client.pause();
    assert!(client.is_paused(), "contract should be paused after pause()");

    client.unpause();
    assert!(!client.is_paused(), "contract should be unpaused after unpause()");
}

#[test]
#[should_panic(expected = "#5002")]
fn test_mint_from_usdc_reverts_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);

    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    usdc_sac.mint(&user, &(100 * DECIMALS));

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

    client.pause();

    // Should panic with error #5002 (paused)
    client.mint_from_usdc(&user, &(10 * DECIMALS), &user);
}

#[test]
#[should_panic(expected = "#5002")]
fn test_mint_from_fiat_reverts_when_paused() {
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

    client.pause();

    // mint_from_fiat must also respect the pause flag
    let tx_id = soroban_sdk::String::from_str(&env, "tx_paused");
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

// --- Fee Rate ---

#[test]
fn test_get_and_set_fee_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, oracle, reserve_tracker, acbu_token, usdc_token, client) = setup_test(&env);

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
        300,
        100,
    );

    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single(), 100);

    client.set_fee_rate(&150);
    assert_eq!(client.get_fee_rate(), 150, "fee_rate should update to 150");
}

#[test]
fn test_fee_rate_zero_means_no_fee_deducted() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);

    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);
    let usdc_amount = 10 * DECIMALS;
    usdc_sac.mint(&user, &usdc_amount);

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
        0, // zero percentage fee
        0, // zero flat fee
    );

    let acbu_minted = client.mint_from_usdc(&user, &usdc_amount, &user);

    // With no fee the minted ACBU must equal the full USDC input (1:1 rate from mock oracle)
    assert_eq!(acbu_minted, usdc_amount, "zero fee must yield 1:1 mint");
    assert_eq!(acbu_client.balance(&user), usdc_amount);
    assert_eq!(client.get_total_supply(), usdc_amount);
}

#[test]
fn test_fee_applied_correctly_on_usdc_mint() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let user = Address::generate(&env);

    let usdc_sac = soroban_sdk::token::StellarAssetClient::new(&env, &usdc_token_id);
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

    let usdc_amount = 100 * DECIMALS; // 100 USDC
    usdc_sac.mint(&user, &usdc_amount);

    // fee_rate = 100 bps (1%), fee_single = 0
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
        100,
        0,
    );

    let acbu_minted = client.mint_from_usdc(&user, &usdc_amount, &user);

    // 1% of 100 USDC = 1 USDC fee → 99 ACBU minted
    let expected_acbu = 99 * DECIMALS;
    assert_eq!(
        acbu_minted, expected_acbu,
        "1% fee on 100 USDC should yield 99 ACBU"
    );
    assert_eq!(acbu_client.balance(&user), expected_acbu);
}

// --- mint_from_fiat (admin/operator fiat path) ---

#[test]
fn test_mint_from_fiat_happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(500 * DECIMALS));
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

    let fiat_amount = 100 * DECIMALS;
    let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token_id);

    let tx_id = soroban_sdk::String::from_str(&env, "fiat_tx_001");
    let acbu_minted = client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &fiat_amount,
        &tx_id,
    );

    assert!(acbu_minted > 0, "mint_from_fiat must return positive ACBU");
    assert_eq!(
        acbu_client.balance(&recipient),
        acbu_minted,
        "recipient balance must equal minted amount"
    );
    assert_eq!(
        client.get_total_supply(),
        acbu_minted,
        "total_supply must reflect the mint"
    );
}

#[test]
fn test_mint_from_fiat_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(200 * DECIMALS));
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
        0,
        0,
    );

    let tx_id = soroban_sdk::String::from_str(&env, "fiat_tx_event");
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(50 * DECIMALS),
        &tx_id,
    );

    let events = env.events().all();
    let contract_events: std::vec::Vec<_> = events
        .iter()
        .filter(|e| e.0 == client.address)
        .collect();
    assert!(
        !contract_events.is_empty(),
        "mint_from_fiat must emit at least one contract event"
    );
}

#[test]
#[should_panic(expected = "#5007")]
fn test_mint_from_fiat_rejects_non_operator() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let attacker = Address::generate(&env);
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

    // attacker is neither admin nor a set operator — must panic #5007
    let tx_id = soroban_sdk::String::from_str(&env, "tx_hack");
    client.mint_from_fiat(
        &attacker,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &(10 * DECIMALS),
        &tx_id,
    );
}

#[test]
#[should_panic(expected = "#5003")]
fn test_mint_from_fiat_exceeds_max() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let recipient = Address::generate(&env);
    let mint_addr = client.address.clone();

    let huge_amount = 2_000_000_000_000_i128;

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(huge_amount * 2));
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
        0,
        0,
    );

    let tx_id = soroban_sdk::String::from_str(&env, "tx_huge");
    client.mint_from_fiat(
        &admin,
        &recipient,
        &CurrencyCode::new(&env, "NGN"),
        &huge_amount,
        &tx_id,
    );
}

#[test]
fn test_mint_from_fiat_total_supply_accumulates() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, oracle, reserve_tracker, acbu_token_id, usdc_token_id, client) = setup_test(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let mint_addr = client.address.clone();

    let stoken_id = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken_id);
    stoken_sac.mint(&mint_addr, &(1_000 * DECIMALS));
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
        0,
        0,
    );

    let tx1 = soroban_sdk::String::from_str(&env, "tx_acc_1");
    let tx2 = soroban_sdk::String::from_str(&env, "tx_acc_2");

    let a1 = client.mint_from_fiat(
        &admin,
        &r1,
        &CurrencyCode::new(&env, "NGN"),
        &(30 * DECIMALS),
        &tx1,
    );
    let a2 = client.mint_from_fiat(
        &admin,
        &r2,
        &CurrencyCode::new(&env, "NGN"),
        &(70 * DECIMALS),
        &tx2,
    );

    assert_eq!(
        client.get_total_supply(),
        a1 + a2,
        "total_supply must equal sum of all minted ACBU"
    );
}
