#![cfg(test)]

use acbu_burning::{BurningContract, BurningContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{
    testutils::Address as _,
    Address, Env, Vec
};
use proptest::prelude::*;

mod mocks {
    use soroban_sdk::{contract, contractimpl, Env, Address, Vec};
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
            env.storage().instance().get(&soroban_sdk::symbol_short!("STK")).unwrap()
        }
        pub fn seed_stoken(env: Env, stoken: Address) {
            env.storage().instance().set(&soroban_sdk::symbol_short!("STK"), &stoken);
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
    pub struct MockACBUToken;

    #[contractimpl]
    impl MockACBUToken {
        pub fn get_total_supply(env: Env) -> i128 {
            env.storage().instance().get(&soroban_sdk::symbol_short!("supply")).unwrap_or(0)
        }
        pub fn burn(env: Env, _from: Address, amount: i128) {
            let supply = Self::get_total_supply(env.clone());
            env.storage().instance().set(&soroban_sdk::symbol_short!("supply"), &(supply - amount));
        }
        pub fn mint(env: Env, _to: Address, amount: i128) {
            let supply = Self::get_total_supply(env.clone());
            env.storage().instance().set(&soroban_sdk::symbol_short!("supply"), &(supply + amount));
        }
    }
}

proptest! {
    #[test]
    fn fuzz_redeem_basket_recipients(num_recipients in 0usize..20usize) {
        let env = Env::default();
        env.mock_all_auths();
        
        let admin = Address::generate(&env);
        let oracle = env.register_contract(None, mocks::MockOracle);
        let reserve_tracker = env.register_contract(None, mocks::MockReserveTracker);
        let acbu_token = env.register_contract(None, mocks::MockACBUToken);
        let contract_id = env.register_contract(None, BurningContract);
        let client = BurningContractClient::new(&env, &contract_id);
        
        let processor = Address::generate(&env);
        let vault = Address::generate(&env);
        
        client.initialize(
            &admin,
            &oracle,
            &reserve_tracker,
            &acbu_token,
            &processor,
            &vault,
            &300,
            &100,
        );
        
        let user = Address::generate(&env);
        let acbu_client = mocks::MockACBUTokenClient::new(&env, &acbu_token);
        acbu_client.mint(&user, &(1_000_000 * DECIMALS));
        
        let stoken = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let stoken_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
        stoken_sac.mint(&vault, &(1_000_000 * DECIMALS));
        
        let token = soroban_sdk::token::Client::new(&env, &stoken);
        token.approve(&vault, &contract_id, &1_000_000_000_000_000, &100u32);
        
        let oracle_client = mocks::MockOracleClient::new(&env, &oracle);
        oracle_client.seed_stoken(&stoken);
        
        let mut recipients = Vec::new(&env);
        for _ in 0..num_recipients {
            recipients.push_back(Address::generate(&env));
        }
        
        let burn_amount = 100 * DECIMALS;
        
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.redeem_basket(&user, &recipients, &burn_amount);
        }));
        
        // Since get_currencies returns exactly 1 item ("NGN"), the recipients list length must be exactly 1
        // as per the contract logic for basket redemptions.
        if num_recipients != 1 {
            assert!(res.is_err());
        } else {
            assert!(res.is_ok());
        }
    }
}
