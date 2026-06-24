#![cfg(test)]

use acbu_burning::{BurningContract, BurningContractClient};
use shared::DECIMALS;
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Vec,
};
use proptest::prelude::*;

mod mocks {
    use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Vec};
    use shared::CurrencyCode;
    use shared::DECIMALS;

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn get_acbu_usd_rate(_env: Env) -> i128 { DECIMALS }

        pub fn get_acbu_usd_rate_with_timestamp(env: Env) -> (i128, u64) {
            (DECIMALS, env.ledger().timestamp())
        }

        pub fn get_basket_weight(_env: Env, _c: CurrencyCode) -> i128 { 10_000 }

        pub fn get_rate(_env: Env, _c: CurrencyCode) -> i128 { DECIMALS }

        pub fn get_rate_with_timestamp(env: Env, _c: CurrencyCode) -> (i128, u64) {
            (DECIMALS, env.ledger().timestamp())
        }

        pub fn get_currencies(env: Env) -> Vec<CurrencyCode> {
            let mut v = Vec::new(&env);
            v.push_back(CurrencyCode::new(&env, "NGN"));
            v
        }

        pub fn get_s_token_address(env: Env, _c: CurrencyCode) -> Address {
            env.storage().instance().get(&symbol_short!("STK")).unwrap()
        }

        pub fn seed_stoken(env: Env, stoken: Address) {
            env.storage().instance().set(&symbol_short!("STK"), &stoken);
        }
    }

    #[contract]
    pub struct MockReserveTracker;

    #[contractimpl]
    impl MockReserveTracker {
        pub fn is_reserve_sufficient(_env: Env, _supply: i128) -> bool {
            true
        }
    }

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn get_total_supply(_env: Env) -> i128 {
            1_000_000_000_000_000_000
        }

        pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    }
}

proptest! {
    #[test]
    fn fuzz_redeem_basket_recipients(num_recipients in 0usize..20usize) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        let oracle = env.register_contract(None, mocks::MockOracle);
        let reserve_tracker = env.register_contract(None, mocks::MockReserveTracker);
        let acbu_token = env.register_contract(None, mocks::MockToken);
        let contract_id = env.register_contract(None, BurningContract);
        let client = BurningContractClient::new(&env, &contract_id);

        let vault = Address::generate(&env);
        let withdrawal_processor = Address::generate(&env);

        client.initialize(
            &admin,
            &oracle,
            &reserve_tracker,
            &acbu_token,
            &withdrawal_processor,
            &vault,
            &100,
            &200,
        );

        let stoken = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
        stoken_sac.mint(&vault, &(1_000_000 * DECIMALS));

        let stoken_client = soroban_sdk::token::Client::new(&env, &stoken);
        stoken_client.approve(&vault, &contract_id, &(1_000_000_000 * DECIMALS), &200u32);

        let oracle_client = mocks::MockOracleClient::new(&env, &oracle);
        oracle_client.seed_stoken(&stoken);

        let mut recipients = Vec::new(&env);
        for _ in 0..num_recipients {
            recipients.push_back(Address::generate(&env));
        }

        let burn_amount = 100 * DECIMALS;

        let result = client.try_redeem_basket(&user, &recipients, &burn_amount);

        if num_recipients != 1 {
            assert!(result.is_err());
        } else {
            assert!(result.is_ok());
        }
    }
}
