// #![cfg(test)]

// use acbu_escrow::{Escrow, EscrowClient};
// use soroban_sdk::{
//     testutils::{Address as _, MockAuth, MockAuthInvoke},
//     Address, Env, IntoVal,
// };

// #[test]
// fn test_unauthorized_release_fails() {
//     let env = Env::default();
//     let admin = Address::generate(&env);
//     let payer = Address::generate(&env);
//     let payee = Address::generate(&env);
//     let attacker = Address::generate(&env);
//     let escrow_id = 42u64;
//     let amount = 10_000_000i128;

//     let acbu_token = env
//         .register_stellar_asset_contract_v2(admin.clone())
//         .address();
//     let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
//     env.mock_all_auths();
//     token_admin.mint(&payer, &amount);

//     let contract_id = env.register_contract(None, Escrow);
//     let client = EscrowClient::new(&env, &contract_id);

//     client.initialize(&admin, &acbu_token);
//     client.create(&payer, &payee, &amount, &escrow_id);

//     // Only attacker auth is provided; release() requires payer auth.
//     env.mock_auths(&[MockAuth {
//         address: &attacker,
//         invoke: &MockAuthInvoke {
//             contract: &contract_id,
//             fn_name: "release",
//             args: (escrow_id, payer.clone()).into_val(&env),
//             sub_invokes: &[],
//         },
//     }]);
//     let result = client.try_release(&escrow_id, &payer);
//     assert!(result.is_err(), "Release without payer auth must fail");
// }

// #[test]
// fn test_payer_can_release() {
//     let env = Env::default();
//     let admin = Address::generate(&env);
//     let payer = Address::generate(&env);
//     let payee = Address::generate(&env);
//     let escrow_id = 99u64;
//     let amount = 12_500_000i128;

//     let acbu_token = env
//         .register_stellar_asset_contract_v2(admin.clone())
//         .address();
//     let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
//     env.mock_all_auths();
//     token_admin.mint(&payer, &amount);

//     let contract_id = env.register_contract(None, Escrow);
//     let client = EscrowClient::new(&env, &contract_id);

//     client.initialize(&admin, &acbu_token);
//     client.create(&payer, &payee, &amount, &escrow_id);

//     env.mock_auths(&[MockAuth {
//         address: &payer,
//         invoke: &MockAuthInvoke {
//             contract: &contract_id,
//             fn_name: "release",
//             args: (escrow_id, payer.clone()).into_val(&env),
//             sub_invokes: &[],
//         },
//     }]);
//     client.release(&escrow_id, &payer);

//     let token = soroban_sdk::token::Client::new(&env, &acbu_token);
//     assert_eq!(token.balance(&payee), amount);
// }

// mod auth_tests {
//     use super::*;

//     #[test]
//     fn test_unauthorized_release_fails() {
//         let env = Env::default();
//         let admin = Address::generate(&env);
//         let payer = Address::generate(&env);
//         let payee = Address::generate(&env);
//         let attacker = Address::generate(&env);
//         let escrow_id = 42u64;
//         let amount = 10_000_000i128;

//         let acbu_token = env
//            .register_stellar_asset_contract_v2(admin.clone())
//            .address();
//         let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
//         env.mock_all_auths();
//         token_admin.mint(&payer, &amount);

//         let contract_id = env.register_contract(None, Escrow);
//         let client = EscrowClient::new(&env, &contract_id);

//         client.initialize(&admin, &acbu_token);
//         client.create(&payer, &payee, &amount, &escrow_id);

//         // Only attacker auth is provided; release() requires payer auth.
//         env.mock_auths(&[MockAuth {
//             address: &attacker,
//             invoke: &MockAuthInvoke {
//                 contract: &contract_id,
//                 fn_name: "release",
//                 args: (escrow_id, payer.clone()).into_val(&env),
//                 sub_invokes: &[],
//             },
//         }]);
//         let result = client.try_release(&escrow_id, &payer);
//         assert!(result.is_err(), "Release without payer auth must fail");
//     }

//     #[test]
//     fn test_payer_can_release() {
//         let env = Env::default();
//         let admin = Address::generate(&env);
//         let payer = Address::generate(&env);
//         let payee = Address::generate(&env);
//         let escrow_id = 99u64;
//         let amount = 12_500_000i128;

//         let acbu_token = env
//            .register_stellar_asset_contract_v2(admin.clone())
//            .address();
//         let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
//         env.mock_all_auths();
//         token_admin.mint(&payer, &amount);

//         let contract_id = env.register_contract(None, Escrow);
//         let client = EscrowClient::new(&env, &contract_id);

//         client.initialize(&admin, &acbu_token);
//         client.create(&payer, &payee, &amount, &escrow_id);

//         env.mock_auths(&[MockAuth {
//             address: &payer,
//             invoke: &MockAuthInvoke {
//                 contract: &contract_id,
//                 fn_name: "release",
//                 args: (escrow_id, payer.clone()).into_val(&env),
//                 sub_invokes: &[],
//             },
//         }]);
//         client.release(&escrow_id, &payer);

//         let token = soroban_sdk::token::Client::new(&env, &acbu_token);
//         assert_eq!(token.balance(&payee), amount);
//     }
// }

// mod key_collision_tests {
//     use super::*;
//    // use soroban_sdk::testutils::Ledger;

//     /// Test helpers
//     fn setup() -> (Env, EscrowClient<'static>, Address, Address) {
//         let env = Env::default();
//         env.mock_all_auths();

//         let contract_id = env.register_contract(None, Escrow);
//         let client = EscrowClient::new(&env, &contract_id);

//         let admin = Address::generate(&env);
//         let acbu_token = env.register_stellar_asset_contract_v2(admin.clone()).address();

//         client.initialize(&admin, &acbu_token);

//         // SAFETY: env and client share lifetime in tests.
//         // This transmute is needed because EscrowClient borrows &env but we want to return both.
//         // Safe here because we don't drop env before client in any test.
//         let client: EscrowClient<'static> = unsafe { core::mem::transmute(client) };
//         let env: Env = unsafe { core::mem::transmute(env) };

//         (env, client, admin, acbu_token)
//     }

//     /// Mint `amount` tokens to `recipient` via the asset admin.
//     fn mint(env: &Env, admin: &Address, token: &Address, recipient: &Address, amount: i128) {
//         soroban_sdk::token::StellarAssetClient::new(env, token).mint(recipient, &amount);
//     }

//     /// PROPERTY: two different payers using the same escrow_id produce two
//     /// independent storage slots — neither overwrites the other.
//     ///
//     /// Before the fix: key = (ESCROW, escrow_id) — same for both payers.
//     /// After the fix: key = EscrowId(payer, escrow_id) — unique per payer.
//     #[test]
//     fn two_payers_same_escrow_id_do_not_collide() {
//         let (env, client, _admin, token) = setup();

//         let payer_a = Address::generate(&env);
//         let payer_b = Address::generate(&env);
//         let payee = Address::generate(&env);

//         mint(&env, &_admin, &token, &payer_a, 1_000);
//         mint(&env, &_admin, &token, &payer_b, 1_000);

//         let escrow_id = 42u64; // same ID for both payers

//         // Both creates must succeed independently
//         client.create(&payer_a, &payee, &500i128, &escrow_id)
//            .expect("payer_a create must succeed");
//         client.create(&payer_b, &payee, &300i128, &escrow_id)
//            .expect("payer_b create must succeed — must not collide with payer_a");

//         // payer_a can release their own escrow — amount must be correct
//         client.release(&escrow_id, &payer_a)
//            .expect("payer_a release must succeed");

//         // payer_b's escrow must still exist and be releasable independently
//         client.release(&escrow_id, &payer_b)
//            .expect("payer_b release must succeed independently");
//     }

//     /// PROPERTY: first payer's funds are not stuck after a second payer creates
//     /// with the same escrow_id.
//     ///
//     /// This directly tests the original impact described in issue #82:
//     /// "Second create overwrites first → funds stuck/lost."
//     #[test]
//     fn first_payer_funds_not_lost_when_second_payer_uses_same_id() {
//         let (env, client, admin, token) = setup();

//         let payer_a = Address::generate(&env);
//         let payer_b = Address::generate(&env);
//         let payee = Address::generate(&env);
//         let escrow_id = 1u64;

//         mint(&env, &admin, &token, &payer_a, 1_000);
//         mint(&env, &admin, &token, &payer_b, 1_000);

//         // payer_a creates first
//         client.create(&payer_a, &payee, &700i128, &escrow_id).unwrap();

//         // payer_b creates with the same ID — must not overwrite payer_a
//         client.create(&payer_b, &payee, &200i128, &escrow_id).unwrap();

//         // Admin can refund payer_a — if the key were overwritten, this would fail
//         // with EscrowNotFound (3003) because payer_a's record would be gone
//         client.refund(&escrow_id, &payer_a)
//            .expect("payer_a must be refundable — funds must not be lost after payer_b creates");
//     }

//     /// PROPERTY: payer_b cannot release or refund payer_a's escrow, even with
//     /// the same escrow_id, because the key includes the payer address.
//     #[test]
//     fn payer_b_cannot_release_payer_a_escrow() {
//         let (env, client, admin, token) = setup();

//         let payer_a = Address::generate(&env);
//         let payer_b = Address::generate(&env);
//         let payee = Address::generate(&env);
//         let escrow_id = 7u64;

//         mint(&env, &admin, &token, &payer_a, 500);

//         client.create(&payer_a, &payee, &500i128, &escrow_id).unwrap();

//         // payer_b attempts to release payer_a's escrow using the same ID
//         // Must fail with EscrowNotFound (3003) — payer_b has no record at this key
//         let result = client.try_release(&escrow_id, &payer_b);
//         assert!(
//             result.is_err(),
//             "payer_b must not be able to release payer_a's escrow"
//         );
//     }

//     /// PROPERTY: a payer cannot create two escrows with the same escrow_id.
//     /// The duplicate guard (error 3005) must fire on the second attempt.
//     #[test]
//     fn same_payer_same_escrow_id_is_rejected() {
//         let (env, client, admin, token) = setup();

//         let payer = Address::generate(&env);
//         let payee = Address::generate(&env);
//         let escrow_id = 99u64;

//         mint(&env, &admin, &token, &payer, 2_000);

//         // First create must succeed
//         client.create(&payer, &payee, &500i128, &escrow_id).unwrap();

//         // Second create with same (payer, escrow_id) must be rejected
//         let result = client.try_create(&payer, &payee, &500i128, &escrow_id);
//         assert_eq!(
//             result,
//             Err(Ok(soroban_sdk::Error::from_contract_error(3005))),
//             "duplicate (payer, escrow_id) must return error 3005 (ESCROW_ALREADY_EXISTS)"
//         );
//     }

//     /// PROPERTY: same payer CAN reuse an escrow_id after the first escrow is released.
//     /// The duplicate guard must not permanently block a payer from reusing an ID.
//     #[test]
//     fn same_payer_can_reuse_escrow_id_after_release() {
//         let (env, client, admin, token) = setup();

//         let payer = Address::generate(&env);
//         let payee = Address::generate(&env);
//         let escrow_id = 5u64;

//         mint(&env, &admin, &token, &payer, 2_000);

//         // First lifecycle: create → release
//         client.create(&payer, &payee, &400i128, &escrow_id).unwrap();
//         client.release(&escrow_id, &payer).unwrap();

//         // Same ID is now free — second create must succeed
//         client.create(&payer, &payee, &300i128, &escrow_id)
//            .expect("payer must be able to reuse escrow_id after prior release");
//     }

//     /// PROPERTY: n distinct payers each using escrow_id=1 all produce independent
//     /// records. This is the multi-payer property test required by the acceptance check.
//     #[test]
//     fn n_payers_same_escrow_id_all_independent() {
//         let (env, client, admin, token) = setup();

//         let n: u64 = 10;
//         let escrow_id = 1u64;
//         let payee = Address::generate(&env);

//         let payers: Vec<Address> = (0..n).map(|_| Address::generate(&env)).collect();

//         for payer in &payers {
//             mint(&env, &admin, &token, payer, 1_000);
//             client.create(payer, &payee, &100i128, &escrow_id)
//                .unwrap_or_else(|_| panic!("create failed for payer {:?}", payer));
//         }

//         // Every payer must be able to release their escrow independently
//         for payer in &payers {
//             client.release(&escrow_id, payer)
//                .unwrap_or_else(|_| panic!("release failed for payer {:?} — key collision suspected", payer));
//         }
//     }

//     /// PROPERTY: error codes are stable and correctly typed.
//     /// Pins 3001=PAUSED, 3003=ESCROW_NOT_FOUND, 3004=PAYER_MISMATCH,
//     /// 3005=ESCROW_ALREADY_EXISTS so any renumbering is caught immediately.
//     #[test]
//     fn error_code_contract_is_stable() {
//         assert_eq!(soroban_sdk::Error::from_contract_error(3001).get_code(), 3001, "3001 = PAUSED");
//         assert_eq!(soroban_sdk::Error::from_contract_error(3003).get_code(), 3003, "3003 = ESCROW_NOT_FOUND");
//         assert_eq!(soroban_sdk::Error::from_contract_error(3004).get_code(), 3004, "3004 = PAYER_MISMATCH");
//         assert_eq!(soroban_sdk::Error::from_contract_error(3005).get_code(), 3005, "3005 = ESCROW_ALREADY_EXISTS");
//     }
// }

#![cfg(test)]

use acbu_escrow::{Escrow, EscrowClient};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, Error, IntoVal,
};

#[test]
fn test_unauthorized_release_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    let attacker = Address::generate(&env);
    let escrow_id = 42u64;
    let amount = 10_000_000i128;

    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    env.mock_all_auths();
    token_admin.mint(&payer, &amount);

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token);
    client.create(&payer, &payee, &amount, &escrow_id);

    // Only attacker auth is provided; release() requires payer auth.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "release",
            args: (escrow_id, payer.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let result = client.try_release(&escrow_id, &payer);
    assert!(result.is_err(), "Release without payer auth must fail");
}

#[test]
fn test_payer_can_release() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    let escrow_id = 99u64;
    let amount = 12_500_000i128;

    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    env.mock_all_auths();
    token_admin.mint(&payer, &amount);

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token);
    client.create(&payer, &payee, &amount, &escrow_id);

    env.mock_auths(&[MockAuth {
        address: &payer,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "release",
            args: (escrow_id, payer.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.release(&escrow_id, &payer);

    let token = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(token.balance(&payee), amount);
}

#[test]
fn test_release_missing_escrow_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token);

    let result = client.try_release(&1u64, &payer);
    assert_eq!(result, Err(Ok(Error::from_contract_error(3003))));
}

#[test]
fn test_refund_missing_escrow_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token);

    let result = client.try_refund(&1u64, &payer);
    assert_eq!(result, Err(Ok(Error::from_contract_error(3003))));
}

#[test]
fn test_pause_without_initialize_returns_uninitialized_admin_error() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    let result = client.try_pause();
    assert_eq!(result, Err(Ok(Error::from_contract_error(3006))));
}
