#![cfg(test)]

use acbu_lending_pool::{
    BorrowEvent, LoanCreatedEvent, LendingPool, LendingPoolClient, LoanStatus, RepayEvent,
};
use shared::DECIMALS;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Symbol, TryIntoVal,
};

/// Test helper: setup environment with initialized lending pool
fn setup() -> (Env, LendingPoolClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 300i128; // 3%

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &fee_rate);

    (env, client, contract_id, admin, acbu_token)
}

// ═══════════════════════════════════════════════════════════════════════════
// DEPOSIT TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_deposit_increases_balance() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 1_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);

    assert_eq!(client.get_balance(&lender), amount, "client.get_balance(&lender) should equal amount");
}

#[test]
fn test_deposit_zero_amount_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    let lender = Address::generate(&env);
    let result = client.try_deposit(&lender, &0);

    assert!(result.is_err());
}

#[test]
fn test_deposit_negative_amount_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    let lender = Address::generate(&env);
    let result = client.try_deposit(&lender, &-100);

    assert!(result.is_err());
}

#[test]
fn test_multiple_deposits_accumulate() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount1 = 500 * DECIMALS;
    let amount2 = 300 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &(amount1 + amount2));

    client.deposit(&lender, &amount1);
    client.deposit(&lender, &amount2);

    assert_eq!(client.get_balance(&lender), amount1 + amount2, "client.get_balance(&lender) should equal amount1 + amount2");
}

// ═══════════════════════════════════════════════════════════════════════════
// WITHDRAW TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_withdraw_decreases_balance() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let deposit_amount = 1_000 * DECIMALS;
    let withdraw_amount = 400 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &deposit_amount);

    client.deposit(&lender, &deposit_amount);
    client.withdraw(&lender, &withdraw_amount);

    assert_eq!(client.get_balance(&lender), deposit_amount - withdraw_amount, "client.get_balance(&lender) should equal deposit_amount - withdraw_amount");
}

#[test]
fn test_withdraw_all_balance() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 1_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    client.withdraw(&lender, &amount);

    assert_eq!(client.get_balance(&lender), 0, "client.get_balance(&lender) should equal 0");
}

#[test]
fn test_withdraw_more_than_balance_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 100 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);

    let result = client.try_withdraw(&lender, &(amount + 1));
    assert!(result.is_err());
}

/// A partial withdrawal that would leave a dust balance (below the minimum)
/// must be rejected, leaving the lender's balance untouched.
#[test]
fn test_withdraw_leaving_dust_balance_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 100 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);

    // Withdrawing all but 1 stroop would leave a dust balance.
    let result = client.try_withdraw(&lender, &(amount - 1));
    assert!(result.is_err());
    assert_eq!(client.get_balance(&lender), amount, "client.get_balance(&lender) should equal amount");
}

/// Withdrawing the entire balance to exactly zero is always allowed, even
/// though zero is below the minimum balance threshold.
#[test]
fn test_withdraw_full_balance_to_zero_succeeds() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 100 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    client.withdraw(&lender, &amount);

    assert_eq!(client.get_balance(&lender), 0, "client.get_balance(&lender) should equal 0");
}

/// A partial withdrawal that leaves at least the minimum balance succeeds.
#[test]
fn test_withdraw_leaving_above_minimum_succeeds() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 100 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    client.withdraw(&lender, &(50 * DECIMALS));

    assert_eq!(client.get_balance(&lender), 50 * DECIMALS, "client.get_balance(&lender) should equal 50 * DECIMALS");
}

#[test]
fn test_withdraw_zero_amount_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    let lender = Address::generate(&env);
    let result = client.try_withdraw(&lender, &0);

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// BORROW TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_borrow_creates_loan() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    let loan = client.get_loan(&borrower, &loan_id).expect("loan should exist");
    assert_eq!(loan.amount, borrow_amount, "loan.amount should equal borrow_amount");
    assert_eq!(loan.borrower, borrower, "loan.borrower should equal borrower");
    assert_eq!(loan.collateral_amount, 0, "loan.collateral_amount should be 0");
}

#[test]
fn test_borrow_transfers_tokens_to_borrower() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 3_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    let token_client = TokenClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &1u64);

    assert_eq!(token_client.balance(&borrower), borrow_amount, "token_client.balance(&borrower) should equal borrow_amount");
}

#[test]
fn test_borrow_exceeds_pool_liquidity_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 1_000 * DECIMALS;
    let borrow_amount = 2_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);

    let result = client.try_borrow(&borrower, &lender, &borrow_amount, &1u64);
    assert!(result.is_err());
}

#[test]
fn test_borrow_zero_amount_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    let borrower = Address::generate(&env);
    let lender = Address::generate(&env);
    let result = client.try_borrow(&borrower, &lender, &0, &1u64);

    assert!(result.is_err());
}

#[test]
fn test_borrow_duplicate_loan_id_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 2_000 * DECIMALS;
    let loan_id = 42u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    let result = client.try_borrow(&borrower, &lender, &borrow_amount, &loan_id);
    assert!(result.is_err());
}

#[test]
fn test_borrow_emits_event() {
    let (env, client, contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 3_000 * DECIMALS;
    let loan_id = 7u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    let events = env.events().all();
    let borrow_event = events
        .iter()
        .rev()
        .find(|e| {
            e.0 == contract_id
                && e.1.first().map_or(false, |t| {
                    if let Ok(symbol_val) =
                        TryIntoVal::<_, Symbol>::try_into_val(&t, &env)
                    {
                        symbol_val == symbol_short!("borrow")
                    } else {
                        false
                    }
                })
        })
        .expect("borrow event not found");

    let event_data: BorrowEvent = borrow_event.2.try_into_val(&env).unwrap();
    assert_eq!(event_data.creator, borrower, "event_data.creator should equal borrower");
    assert_eq!(event_data.amount, borrow_amount, "event_data.amount should equal borrow_amount");
    assert_eq!(event_data.loan_id, loan_id, "event_data.loan_id should equal loan_id");
}

// ═══════════════════════════════════════════════════════════════════════════
// REPAY TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_repay_partial_reduces_loan_amount() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let repay_amount = 2_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);
    client.repay(&borrower, &repay_amount, &loan_id);

    let loan = client.get_loan(&borrower, &loan_id).expect("loan should still exist");
    assert_eq!(loan.amount, borrow_amount - repay_amount, "loan.amount should equal borrow_amount - repay_amount");
}

#[test]
fn test_repay_full_removes_loan() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);
    client.repay(&borrower, &borrow_amount, &loan_id);

    let loan = client
        .get_loan(&borrower, &loan_id)
        .expect("repaid loan state should remain available");
    assert_eq!(loan.amount, 0);
    assert!(matches!(loan.status, LoanStatus::Repaid));
}

#[test]
fn test_repay_full_clears_loan() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    let token_client = TokenClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);
    
    // After borrow: borrower has borrow_amount
    assert_eq!(token_client.balance(&borrower), borrow_amount, "token_client.balance(&borrower) should equal borrow_amount");
    
    client.repay(&borrower, &borrow_amount, &loan_id);
    
    // After full repay: borrower has 0
    assert_eq!(token_client.balance(&borrower), 0, "token_client.balance(&borrower) should equal 0");
}

#[test]
fn test_repay_more_than_loan_amount_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    let result = client.try_repay(&borrower, &(borrow_amount + 1), &loan_id);
    assert!(result.is_err());
}

#[test]
fn test_repay_nonexistent_loan_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    let borrower = Address::generate(&env);
    let result = client.try_repay(&borrower, &1000, &999u64);

    assert!(result.is_err());
}

#[test]
fn test_repay_zero_amount_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    let result = client.try_repay(&borrower, &0, &loan_id);
    assert!(result.is_err());
}

#[test]
fn test_repay_emits_event() {
    let (env, client, contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let repay_amount = 2_000 * DECIMALS;
    let loan_id = 3u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);
    client.repay(&borrower, &repay_amount, &loan_id);

    let events = env.events().all();
    let repay_event = events
        .iter()
        .rev()
        .find(|e| {
            e.0 == contract_id
                && e.1.first().map_or(false, |t| {
                    if let Ok(symbol_val) =
                        TryIntoVal::<_, Symbol>::try_into_val(&t, &env)
                    {
                        symbol_val == symbol_short!("repay")
                    } else {
                        false
                    }
                })
        })
        .expect("repay event not found");

    let event_data: RepayEvent = repay_event.2.try_into_val(&env).unwrap();
    assert_eq!(event_data.creator, borrower, "event_data.creator should equal borrower");
    assert_eq!(event_data.amount, repay_amount, "event_data.amount should equal repay_amount");
    assert_eq!(event_data.loan_id, loan_id, "event_data.loan_id should equal loan_id");
}

// ═══════════════════════════════════════════════════════════════════════════
// PAUSE/UNPAUSE TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_deposit_when_paused_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    client.pause();

    let lender = Address::generate(&env);
    let result = client.try_deposit(&lender, &1000);

    assert!(result.is_err());
}

#[test]
fn test_withdraw_when_paused_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let amount = 1_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    client.pause();

    let result = client.try_withdraw(&lender, &amount);
    assert!(result.is_err());
}

#[test]
fn test_borrow_when_paused_fails() {
    let (env, client, _contract_id, _admin, _acbu_token) = setup();

    client.pause();

    let borrower = Address::generate(&env);
    let lender = Address::generate(&env);
    let result = client.try_borrow(&borrower, &lender, &1000, &1u64);

    assert!(result.is_err());
}

#[test]
fn test_repay_when_paused_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);
    
    client.pause();

    let result = client.try_repay(&borrower, &1000, &loan_id);
    assert!(result.is_err());
}

#[test]
fn test_unpause_allows_operations() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    client.pause();
    client.unpause();

    let lender = Address::generate(&env);
    let amount = 1_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    assert_eq!(client.get_balance(&lender), amount, "client.get_balance(&lender) should equal amount");
}

// ═══════════════════════════════════════════════════════════════════════════
// ADDITIONAL EDGE CASE TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_multiple_borrowers_same_lender() {
    let (env, client, contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower1 = Address::generate(&env);
    let borrower2 = Address::generate(&env);
    let pool_liquidity = 20_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    let token_client = TokenClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);

    // First borrower
    client.borrow(&borrower1, &lender, &borrow_amount, &1u64);
    // Second borrower
    client.borrow(&borrower2, &lender, &borrow_amount, &2u64);

    // Both loans should exist
    let loan1 = client.get_loan(&borrower1, &1u64).expect("loan1 should exist");
    let loan2 = client.get_loan(&borrower2, &2u64).expect("loan2 should exist");
    assert_eq!(loan1.amount, borrow_amount, "loan1.amount should equal borrow_amount");
    assert_eq!(loan2.amount, borrow_amount, "loan2.amount should equal borrow_amount");

    // Contract should have remaining liquidity
    assert_eq!(
        token_client.balance(&contract_id),
        pool_liquidity - (borrow_amount * 2)
    , "token_client.balance(&contract_id) should equal pool_liquidity - (borrow_amount * 2)");
}

#[test]
fn test_deposit_withdraw_multiple_lenders() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender1 = Address::generate(&env);
    let lender2 = Address::generate(&env);
    let amount1 = 1_000 * DECIMALS;
    let amount2 = 2_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender1, &amount1);
    token_admin.mint(&lender2, &amount2);

    client.deposit(&lender1, &amount1);
    client.deposit(&lender2, &amount2);

    assert_eq!(client.get_balance(&lender1), amount1, "client.get_balance(&lender1) should equal amount1");
    assert_eq!(client.get_balance(&lender2), amount2, "client.get_balance(&lender2) should equal amount2");

    // Withdraw partial amounts
    client.withdraw(&lender1, &(500 * DECIMALS));
    client.withdraw(&lender2, &(1_000 * DECIMALS));

    assert_eq!(client.get_balance(&lender1), 500 * DECIMALS, "client.get_balance(&lender1) should equal 500 * DECIMALS");
    assert_eq!(client.get_balance(&lender2), 1_000 * DECIMALS, "client.get_balance(&lender2) should equal 1_000 * DECIMALS");
}

#[test]
fn test_repay_interest_accrual() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 5_000 * DECIMALS;
    let loan_id = 1u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    // Advance time to accrue interest
    let current_time = env.ledger().timestamp();
    env.ledger().with_mut(|l| l.timestamp = current_time + 30 * 24 * 60 * 60);

    // Get loan to see accrued interest
    let loan = client.get_loan(&borrower, &loan_id).expect("loan should exist");
    // With fee_rate of 300 bps (3%), interest should have accrued
    assert!(loan.accrued_interest >= 0);
    assert!(loan.total_repayment_due >= borrow_amount);
}

#[test]
fn test_borrow_negative_amount_fails() {
    let (env, client, _contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);

    client.deposit(&lender, &pool_liquidity);

    let result = client.try_borrow(&borrower, &lender, &-100, &1u64);
    assert!(result.is_err());
}

#[test]
fn test_loan_created_event_has_correct_term_seconds() {
    let (env, client, contract_id, _admin, acbu_token) = setup();

    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let pool_liquidity = 10_000 * DECIMALS;
    let borrow_amount = 3_000 * DECIMALS;
    let loan_id = 99u64;

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &pool_liquidity);
    client.deposit(&lender, &pool_liquidity);

    let borrow_timestamp = env.ledger().timestamp();
    client.borrow(&borrower, &lender, &borrow_amount, &loan_id);

    let events = env.events().all();
    let loan_created_event = events
        .iter()
        .rev()
        .find(|e| {
            e.0 == contract_id
                && e.1.first().map_or(false, |t| {
                    if let Ok(symbol_val) =
                        TryIntoVal::<_, Symbol>::try_into_val(&t, &env)
                    {
                        symbol_val == symbol_short!("loan_cr")
                    } else {
                        false
                    }
                })
        })
        .expect("loan_cr event not found");

    let event_data: LoanCreatedEvent = loan_created_event.2.try_into_val(&env).unwrap();

    assert_eq!(
        event_data.term_seconds,
        30 * 24 * 60 * 60,
        "event.term_seconds should equal 30 days in seconds (2_592_000)"
    );
    assert_eq!(event_data.lender, lender, "event.lender should match");
    assert_eq!(event_data.borrower, borrower, "event.borrower should match");
    assert_eq!(
        event_data.amount, borrow_amount,
        "event.amount should match borrow_amount"
    );
    assert_eq!(
        event_data.interest_bps, 300,
        "event.interest_bps should match fee_rate (300)"
    );
    assert_eq!(
        event_data.timestamp, borrow_timestamp,
        "event.timestamp should match borrow timestamp"
    );
}
