#![cfg(test)]

use acbu_multisig::{MultisigContract, MultisigContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, String as SorobanString,
};

fn setup(env: &Env, n: usize, threshold: u32) -> (Vec<Address>, MultisigContractClient<'_>) {
    let mut signers = soroban_sdk::Vec::new(env);
    let mut rust_signers = Vec::new();
    for _ in 0..n {
        let s = Address::generate(env);
        signers.push_back(s.clone());
        rust_signers.push(s);
    }
    let id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(env, &id);
    client.initialize(&signers, &threshold);
    (rust_signers, client)
}

// ── Basic initialisation ────────────────────────────────────────────────────

#[test]
fn test_initialize_2_of_3() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let cfg = client.get_config();
    assert_eq!(cfg.threshold, 2);
    assert_eq!(cfg.signers.len(), 3);
    assert!(client.is_signer(&signers[0]));
    assert!(client.is_signer(&signers[1]));
    assert!(client.is_signer(&signers[2]));
}

#[test]
fn test_initialize_3_of_5() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, client) = setup(&env, 5, 3);
    let cfg = client.get_config();
    assert_eq!(cfg.threshold, 3);
    assert_eq!(cfg.signers.len(), 5);
}

#[test]
#[should_panic]
fn test_initialize_twice_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup(&env, 3, 2);
    // second init must panic
    let mut s2 = soroban_sdk::Vec::new(&env);
    s2.push_back(Address::generate(&env));
    client.initialize(&s2, &1);
}

#[test]
#[should_panic]
fn test_threshold_zero_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(&env, &id);
    let mut s = soroban_sdk::Vec::new(&env);
    s.push_back(Address::generate(&env));
    client.initialize(&s, &0);
}

#[test]
#[should_panic]
fn test_threshold_exceeds_signers_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(&env, &id);
    let mut s = soroban_sdk::Vec::new(&env);
    s.push_back(Address::generate(&env));
    client.initialize(&s, &2); // threshold > signers
}

#[test]
#[should_panic]
fn test_duplicate_signer_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(&env, &id);
    let dup = Address::generate(&env);
    let mut s = soroban_sdk::Vec::new(&env);
    s.push_back(dup.clone());
    s.push_back(dup.clone());
    client.initialize(&s, &1);
}

// ── Propose ─────────────────────────────────────────────────────────────────

#[test]
fn test_propose_returns_id_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let id = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    assert_eq!(id, 0);
}

#[test]
fn test_propose_increments_id() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let id0 = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    let id1 = client.propose(&signers[1], &SorobanString::from_str(&env, "upgrade"));
    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
}

#[test]
fn test_proposer_approval_counted_immediately() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    assert_eq!(client.approval_count(&pid), 1);
}

#[test]
#[should_panic]
fn test_non_signer_cannot_propose() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, client) = setup(&env, 3, 2);
    let outsider = Address::generate(&env);
    client.propose(&outsider, &SorobanString::from_str(&env, "pause"));
}

// ── Approve ─────────────────────────────────────────────────────────────────

#[test]
fn test_approve_increments_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[1], &pid);
    assert_eq!(client.approval_count(&pid), 2);
}

#[test]
#[should_panic]
fn test_double_approve_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[0], &pid); // already approved via propose
}

#[test]
#[should_panic]
fn test_non_signer_cannot_approve() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    let outsider = Address::generate(&env);
    client.approve(&outsider, &pid);
}

#[test]
#[should_panic]
fn test_approve_expired_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    // advance time past TTL (48 h + 1 s)
    env.ledger().with_mut(|l| l.timestamp = 172_801);
    client.approve(&signers[1], &pid);
}

// ── Execute ──────────────────────────────────────────────────────────────────

/// Core acceptance check: M-of-N — 2-of-3 must succeed.
#[test]
fn test_execute_2_of_3_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[1], &pid);
    // threshold met — execute must succeed
    client.execute(&signers[2], &pid);
    let proposal = client.get_proposal(&pid);
    assert!(proposal.executed);
}

/// 3-of-5 acceptance check.
#[test]
fn test_execute_3_of_5_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 5, 3);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "upgrade"));
    client.approve(&signers[1], &pid);
    client.approve(&signers[2], &pid);
    client.execute(&signers[3], &pid);
    assert!(client.get_proposal(&pid).executed);
}

/// Threshold NOT met — execute must panic.
#[test]
#[should_panic]
fn test_execute_below_threshold_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    // only 1 approval (the proposer) — threshold is 2
    client.execute(&signers[1], &pid);
}

#[test]
#[should_panic]
fn test_execute_twice_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[1], &pid);
    client.execute(&signers[2], &pid);
    client.execute(&signers[2], &pid); // second execute must panic
}

#[test]
#[should_panic]
fn test_execute_expired_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[1], &pid);
    env.ledger().with_mut(|l| l.timestamp = 172_801);
    client.execute(&signers[2], &pid);
}

#[test]
#[should_panic]
fn test_non_signer_cannot_execute() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[1], &pid);
    let outsider = Address::generate(&env);
    client.execute(&outsider, &pid);
}

// ── Events ───────────────────────────────────────────────────────────────────

#[test]
fn test_propose_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    let events = env.events().all();
    assert!(!events.is_empty());
}

#[test]
fn test_execute_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (signers, client) = setup(&env, 3, 2);
    let pid = client.propose(&signers[0], &SorobanString::from_str(&env, "pause"));
    client.approve(&signers[1], &pid);
    client.execute(&signers[2], &pid);
    let events = env.events().all();
    // at least propose + approve + execute events
    assert!(events.len() >= 3);
}

// ── is_signer ────────────────────────────────────────────────────────────────

#[test]
fn test_is_signer_false_for_outsider() {
    let env = Env::default();
    env.mock_all_auths();
    let (_signers, client) = setup(&env, 3, 2);
    let outsider = Address::generate(&env);
    assert!(!client.is_signer(&outsider));
}
