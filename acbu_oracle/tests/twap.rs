#![cfg(test)]

//! Tests for the 24-hour time-weighted average (TWAP) oracle reads used by the
//! mint/burn conversion paths (W10-SEC-012).
//!
//! The accumulator is driven entirely through `update_rate`, so these tests
//! exercise the real commit path rather than the internal helpers.

use acbu_oracle::{OracleContract, OracleContractClient};
use shared::CurrencyCode;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Map, Vec,
};

/// Regular oracle updates are gated by `UPDATE_INTERVAL_SECONDS` (6 h).
const SIX_HOURS: u64 = 21_600;
/// The TWAP window under test.
const DAY: u64 = 86_400;

fn deploy<'a>(env: &'a Env) -> (OracleContractClient<'a>, Address, CurrencyCode) {
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let admin = Address::generate(env);
    let validator = Address::generate(env);
    let mut validators = Vec::new(env);
    validators.push_back(validator.clone());

    let ngn = CurrencyCode::new(env, "NGN");
    let mut currencies = Vec::new(env);
    currencies.push_back(ngn.clone());

    // A single-currency, 100 % basket keeps the basket TWAP equal to the
    // per-currency TWAP, which makes the expected values easy to state.
    let mut basket_weights = Map::new(env);
    basket_weights.set(ngn.clone(), 10_000i128);

    let contract_id = env.register_contract(None, OracleContract);
    let client = OracleContractClient::new(env, &contract_id);
    client.initialize(&admin, &validators, &1u32, &currencies, &basket_weights);

    (client, validator, ngn)
}

/// Submit an update at `timestamp` whose three identical sources make the
/// committed (median) rate exactly `rate`.
fn submit(
    env: &Env,
    client: &OracleContractClient,
    validator: &Address,
    ngn: &CurrencyCode,
    rate: i128,
    timestamp: u64,
) {
    env.ledger().with_mut(|l| l.timestamp = timestamp);
    let mut sources = Vec::new(env);
    sources.push_back(rate);
    sources.push_back(rate);
    sources.push_back(rate);
    client.update_rate(validator, ngn, &rate, &sources, &timestamp);
}

/// Before a second observation accrues, the TWAP falls back to the sole
/// observation (equivalently, the spot rate).
#[test]
fn twap_matches_spot_with_single_observation() {
    let env = Env::default();
    let (client, validator, ngn) = deploy(&env);

    submit(&env, &client, &validator, &ngn, 1_000, 1_000_000);

    assert_eq!(client.get_twap(&ngn), 1_000);
    let (twap, ts) = client.get_twap_with_timestamp(&ngn);
    assert_eq!(twap, 1_000);
    assert_eq!(ts, 1_000_000, "timestamp must be the committed spot timestamp");
}

/// The average is weighted by how long each rate was in effect: a rate of 1000
/// held for one 6 h interval followed by 2000 held for another averages to 1500;
/// letting the 2000 rate accrue for a third interval pulls the mean up to 1666
/// (1000·21600 + 2000·43200 over 64800 seconds).
#[test]
fn twap_is_time_weighted() {
    let env = Env::default();
    let (client, validator, ngn) = deploy(&env);

    submit(&env, &client, &validator, &ngn, 1_000, 1_000_000);
    submit(&env, &client, &validator, &ngn, 2_000, 1_000_000 + SIX_HOURS);

    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + 2 * SIX_HOURS);
    assert_eq!(client.get_twap(&ngn), 1_500);

    // The last rate keeps accruing after its update.
    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + 3 * SIX_HOURS);
    assert_eq!(client.get_twap(&ngn), 1_666);
}

/// Now-in-window test: the TWAP must line up with the 6h-gated accumulator
/// after three equal-length segments (1000, 2000, 3000 -> 2000).
#[test]
fn twap_averages_multiple_segments() {
    let env = Env::default();
    let (client, validator, ngn) = deploy(&env);

    submit(&env, &client, &validator, &ngn, 1_000, 1_000_000);
    submit(&env, &client, &validator, &ngn, 2_000, 1_000_000 + SIX_HOURS);
    submit(&env, &client, &validator, &ngn, 3_000, 1_000_000 + 2 * SIX_HOURS);

    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + 3 * SIX_HOURS);
    assert_eq!(client.get_twap(&ngn), 2_000);
}

/// History older than the 24 h window is excluded from the average.
#[test]
fn twap_excludes_history_outside_window() {
    let env = Env::default();
    let (client, validator, ngn) = deploy(&env);

    submit(&env, &client, &validator, &ngn, 1_000, 1_000_000);
    let second = 1_000_000 + SIX_HOURS;
    submit(&env, &client, &validator, &ngn, 3_000, second);

    // Read exactly 24 h after the last update: only the 3000 segment is in
    // scope, so the earlier 1000 rate must not dilute the result.
    env.ledger().with_mut(|l| l.timestamp = second + DAY);
    assert_eq!(client.get_twap(&ngn), 3_000);
}

/// The basket TWAP tracks the per-currency TWAP (100 % weight here) and the
/// ACBU/USD TWAP getters agree with each other.
#[test]
fn basket_twap_matches_per_currency_twap() {
    let env = Env::default();
    let (client, validator, ngn) = deploy(&env);

    submit(&env, &client, &validator, &ngn, 1_000, 1_000_000);
    submit(&env, &client, &validator, &ngn, 2_000, 1_000_000 + SIX_HOURS);

    env.ledger()
        .with_mut(|l| l.timestamp = 1_000_000 + 2 * SIX_HOURS);
    let per_currency = client.get_twap(&ngn);
    assert_eq!(client.get_acbu_usd_twap(), per_currency);
    assert_eq!(client.get_acbu_usd_twap_with_timestamp().0, per_currency);
}
