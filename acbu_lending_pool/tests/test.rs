#![cfg(test)]

use acbu_lending_pool::{BorrowEvent, LendingPool, LendingPoolClient, RepayEvent};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    Address, Env, Error as SdkError, TryIntoVal,
};
use shared::DECIMALS;
// Add these imports for the lifecycle test
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

#[test]
fn test_deposit_and_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 300;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let amount = DECIMALS; // 1000 ACBU

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    client.deposit(&lender, &amount);
    assert_eq!(client.get_balance(&lender), amount);

    client.withdraw(&lender, &amount);

    assert_eq!(client.get_balance(&lender), 0);

    let token_client = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token_client.balance(&lender), amount);
}

#[test]
fn test_withdraw_more_than_balance_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 0;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let amount = DECIMALS;
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);
    client.deposit(&lender, &amount);

    let result = client.try_withdraw(&lender, &(amount + 1));
    assert!(result.is_err());
}

/// Security test: an attacker must NOT be able to deposit on behalf of another address.
#[test]
fn test_unauthorized_deposit_fails() {
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;

    let env = Env::default();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 300;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    env.mock_all_auths();
    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let attacker = Address::generate(&env);
    let amount: i128 = DECIMALS;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);

    // Only authorize the *attacker*, not the lender — so lender.require_auth() will fail
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "deposit",
            args: (&lender, amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_deposit(&lender, &amount);
    assert!(result.is_err(), "Unauthorized deposit must be rejected");
}

/// Security test: an attacker must NOT be able to withdraw from another lender's balance.
/// This is the exact exploit described in issue #31 — without `lender.require_auth()`,
/// anyone could call withdraw(lender=victim, amount=Y) and drain the victim's deposited funds.
#[test]
fn test_unauthorized_withdraw_fails() {
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;

    let env = Env::default();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let fee_rate = 0;

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    // Setup: deposit normally with all auths mocked
    env.mock_all_auths();
    client.initialize(&admin, &acbu_token, &fee_rate);

    let lender = Address::generate(&env);
    let attacker = Address::generate(&env);
    let amount: i128 = DECIMALS;

    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    token_admin.mint(&lender, &amount);
    client.deposit(&lender, &amount);

    assert_eq!(client.get_balance(&lender), amount);

    // Now try to withdraw as the attacker — only attacker has auth, not the lender
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "withdraw",
            args: (&lender, amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_withdraw(&lender, &amount);
    assert!(result.is_err(), "Unauthorized withdrawal must be rejected");
    // Lender's balance must remain intact
    assert_eq!(client.get_balance(&lender), amount);
}

// ── Issue #131: borrow / repay integration tests ─────────────────────────────

/// 1. Basic borrow: lender deposits, borrower borrows within available liquidity.
///    Asserts loan is recorded, borrower received tokens, pool balance decreased.
#[test]
fn test_borrow_basic() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0);

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    let token_client = TokenClient::new(&env, &acbu_token);

    // Lender deposits liquidity into the pool
    let lender = Address::generate(&env);
    let pool_liquidity: i128 = 1_000_000;
    token_admin.mint(&lender, &pool_liquidity);
    client.deposit(&lender, &pool_liquidity);

    // Borrower borrows half the pool
    let borrower = Address::generate(&env);
    let borrow_amount: i128 = 400_000;
    let collateral: i128 = 200_000;
    let loan_id: u64 = 1;

    // (contract transfers ACBU *out* to borrower; collateral is recorded but not transferred in MVP)
    client.borrow(&borrower, &borrow_amount, &collateral, &loan_id);

    // Loan is recorded with correct fields
    let loan = client.get_loan(&borrower, &loan_id).expect("loan must exist");
    assert_eq!(loan.amount, borrow_amount);
    assert_eq!(loan.borrower, borrower);
    assert_eq!(loan.collateral_amount, collateral);

    // Borrower received the tokens
    assert_eq!(token_client.balance(&borrower), borrow_amount);

    // Pool's token balance decreased by the borrowed amount
    assert_eq!(
        token_client.balance(&contract_id),
        pool_liquidity - borrow_amount
    );
}

/// 2. Basic repay: borrower repays the full loan.
///    Asserts loan is cleared, pool liquidity restored, borrower balance zeroed.
#[test]
fn test_repay_basic() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0);

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    let token_client = TokenClient::new(&env, &acbu_token);

    let lender = Address::generate(&env);
    let pool_liquidity: i128 = 1_000_000;
    token_admin.mint(&lender, &pool_liquidity);
    client.deposit(&lender, &pool_liquidity);

    let borrower = Address::generate(&env);
    let borrow_amount: i128 = 300_000;
    let loan_id: u64 = 7;
    client.borrow(&borrower, &borrow_amount, &0, &loan_id);

    // Borrower now holds borrow_amount tokens; repay the full amount
    client.repay(&borrower, &borrow_amount, &loan_id);

    // Loan must be removed after full repayment
    assert!(
        client.get_loan(&borrower, &loan_id).is_none(),
        "loan must be cleared after full repayment"
    );

    // Pool token balance is restored to original liquidity
    assert_eq!(token_client.balance(&contract_id), pool_liquidity);

    // Borrower's token balance is back to zero (no fee in this test; fee_rate = 0)
    assert_eq!(token_client.balance(&borrower), 0);
}

/// 3. Borrow exceeds available liquidity — must return an error.
#[test]
fn test_borrow_exceeds_liquidity_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0);

    let token_admin = StellarAssetClient::new(&env, &acbu_token);

    // Deposit only a small amount
    let lender = Address::generate(&env);
    let small_liquidity: i128 = 100_000;
    token_admin.mint(&lender, &small_liquidity);
    client.deposit(&lender, &small_liquidity);

    // Attempt to borrow more than what is in the pool
    let borrower = Address::generate(&env);
    let over_amount: i128 = small_liquidity + 1;
    let result = client.try_borrow(&borrower, &over_amount, &0, &1u64);

    assert!(
        matches!(
            result,
            Err(Ok(e)) if e == SdkError::from_contract_error(4007)
        ),
        "borrow exceeding pool liquidity must return InsufficientBalance (4007)"
    );
}

/// 4. Repay with a wrong loan_id — must return an error.
#[test]
fn test_repay_wrong_loan_id_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0);

    let token_admin = StellarAssetClient::new(&env, &acbu_token);

    let lender = Address::generate(&env);
    let pool_liquidity: i128 = 500_000;
    token_admin.mint(&lender, &pool_liquidity);
    client.deposit(&lender, &pool_liquidity);

    let borrower = Address::generate(&env);
    let borrow_amount: i128 = 100_000;
    let real_loan_id: u64 = 42;
    client.borrow(&borrower, &borrow_amount, &0, &real_loan_id);

    // Attempt to repay using a different loan_id
    let wrong_loan_id: u64 = 99;
    let result = client.try_repay(&borrower, &borrow_amount, &wrong_loan_id);

    assert!(
        matches!(
            result,
            Err(Ok(e)) if e == SdkError::from_contract_error(4001)
        ),
        "repay with wrong loan_id must return NotFound (4001)"
    );
}

/// 5. Loan default scenario.
///
/// The contract does not implement a liquidation or default function in the current
/// MVP — there is no `liquidate()`, `mark_default()`, or time-based enforcement.
/// This test documents that behaviour: a loan that is never repaid simply remains
/// open in storage indefinitely.  When a liquidation path is added (tracked in the
/// issue backlog), this test should be updated to call that function and assert the
/// correct state transition.
#[test]
fn test_loan_default_scenario() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0);

    let token_admin = StellarAssetClient::new(&env, &acbu_token);

    let lender = Address::generate(&env);
    let pool_liquidity: i128 = 1_000_000;
    token_admin.mint(&lender, &pool_liquidity);
    client.deposit(&lender, &pool_liquidity);

    let borrower = Address::generate(&env);
    let borrow_amount: i128 = 200_000;
    let loan_id: u64 = 55;
    client.borrow(&borrower, &borrow_amount, &0, &loan_id);

    // Borrower never repays — loan remains open.
    // No liquidation function exists yet; assert the loan is still present and overdue.
    let loan = client
        .get_loan(&borrower, &loan_id)
        .expect("defaulted loan must still be present in storage");
    assert_eq!(loan.amount, borrow_amount);

    // TODO: when a `liquidate(loan_id)` function is implemented, call it here and
    // assert that:
    //   - the loan is removed from storage
    //   - collateral is transferred to the protocol / lender
    //   - a LiquidationEvent is emitted
}

/// 6. Full lifecycle: initialize → deposit → borrow → repay → withdraw.
///    Verifies all balances are correct at every step.
#[test]
fn test_borrow_repay_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token, &0);

    let token_admin = StellarAssetClient::new(&env, &acbu_token);
    let token_client = TokenClient::new(&env, &acbu_token);

    // ── Step 1: lender deposits ───────────────────────────────────────────────
    let lender = Address::generate(&env);
    let pool_liquidity: i128 = 1_000_000;
    token_admin.mint(&lender, &pool_liquidity);
    client.deposit(&lender, &pool_liquidity);

    assert_eq!(client.get_balance(&lender), pool_liquidity);
    assert_eq!(token_client.balance(&contract_id), pool_liquidity);
    assert_eq!(token_client.balance(&lender), 0);

    // ── Step 2: borrower borrows ──────────────────────────────────────────────
    let borrower = Address::generate(&env);
    let borrow_amount: i128 = 600_000;
    let loan_id: u64 = 100;
    client.borrow(&borrower, &borrow_amount, &0, &loan_id);

    assert_eq!(token_client.balance(&borrower), borrow_amount);
    assert_eq!(
        token_client.balance(&contract_id),
        pool_liquidity - borrow_amount
    );
    let loan = client.get_loan(&borrower, &loan_id).expect("loan must exist after borrow");
    assert_eq!(loan.amount, borrow_amount);

    // ── Step 3: borrower repays in full ───────────────────────────────────────
    client.repay(&borrower, &borrow_amount, &loan_id);

    assert_eq!(token_client.balance(&borrower), 0);
    assert_eq!(token_client.balance(&contract_id), pool_liquidity);
    assert!(
        client.get_loan(&borrower, &loan_id).is_none(),
        "loan must be gone after full repayment"
    );

    // ── Step 4: lender withdraws ──────────────────────────────────────────────
    client.withdraw(&lender, &pool_liquidity);

    assert_eq!(client.get_balance(&lender), 0);
    assert_eq!(token_client.balance(&lender), pool_liquidity);
    assert_eq!(token_client.balance(&contract_id), 0);
}

// ── End of issue #131 tests ───────────────────────────────────────────────────

// Integration test for #106 - full loan lifecycle with events
#[test]
fn test_loan_lifecycle_emits_events() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let lender = Address::generate(&env);
    let borrower = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let _token_client = TokenClient::new(&env, &token_id);
    let token_admin_client = StellarAssetClient::new(&env, &token_id);

    let contract_id = env.register_contract(None, LendingPool);
    let client = LendingPoolClient::new(&env, &contract_id);

    client.initialize(&admin, &token_id, &0);

    // Fund pool and borrower
    let pool_liquidity = 1_000_000i128;
    let collateral = 500_000i128;
    token_admin_client.mint(&lender, &pool_liquidity);
    token_admin_client.mint(&borrower, &collateral);
    client.deposit(&lender, &pool_liquidity);

    let loan_id = 42u64;
    let borrow_amount = 300_000i128;

    // 1. Borrow - verify state transition + event
    client.borrow(&borrower, &borrow_amount, &collateral, &loan_id);

    // Assert loan state created
    let loan = client.get_loan(&borrower, &loan_id).unwrap();
    assert_eq!(loan.amount, borrow_amount);
    assert_eq!(loan.borrower, borrower);

    // Assert borrow event emitted with correct fields
    let events = env.events().all();
    let borrow_event = events
        .iter()
        .rev()
        .find(|e| {
            e.1.first().map_or(false, |t| {
                t.try_into_val(&env).ok() == Some(symbol_short!("borrow"))
            })
        })
        .expect("borrow event not found");

    let borrow_payload: BorrowEvent = borrow_event.2.try_into_val(&env).unwrap();
    assert_eq!(borrow_payload.creator, borrower);
    assert_eq!(borrow_payload.amount, borrow_amount);
    assert_eq!(borrow_payload.token, token_id);
    assert_eq!(borrow_payload.loan_id, loan_id);

    // 2. Repay partial
    let repay_amount = 100_000i128;
    client.repay(&borrower, &repay_amount, &loan_id);

    // Assert state transition - loan amount reduced
    let loan = client.get_loan(&borrower, &loan_id).unwrap();
    assert_eq!(loan.amount, borrow_amount - repay_amount);

    // Assert repay event
    let events = env.events().all();
    let repay_event = events
        .iter()
        .rev()
        .find(|e| {
            e.1.first().map_or(false, |t| {
                t.try_into_val(&env).ok() == Some(symbol_short!("repay"))
            })
        })
        .expect("repay event not found");

    let repay_payload: RepayEvent = repay_event.2.try_into_val(&env).unwrap();
    assert_eq!(repay_payload.creator, borrower);
    assert_eq!(repay_payload.amount, repay_amount);
    assert_eq!(repay_payload.token, token_id);
    assert_eq!(repay_payload.loan_id, loan_id);

    // 3. Repay full - loan removed
    client.repay(&borrower, &(borrow_amount - repay_amount), &loan_id);

    // Assert loan deleted after full repayment
    assert!(client.get_loan(&borrower, &loan_id).is_none());
}
