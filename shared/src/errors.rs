/// shared/src/errors.rs
///
/// Canonical error taxonomy shared across all ACBU contracts.
/// Import this module in acbu_burning, acbu_lending_pool, acbu_savings_vault,
/// acbu_oracle, and acbu_reserve_tracker to get consistent error codes and
/// the same fix for Issue #355 (differentiated transfer errors).

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SharedError {
    // ── Lifecycle ─────────────────────────────────────────────────────────
    /// initialize() was called on a contract that is already initialised.
    /// Fix for Issue #357.
    AlreadyInitialized = 1,
    NotInitialized     = 2,

    // ── Auth ──────────────────────────────────────────────────────────────
    Unauthorized       = 3,

    // ── Validation ────────────────────────────────────────────────────────
    InvalidAmount      = 4,
    InvalidAddress     = 5,

    // ── Transfer ─────────────────────────────────────────────────────────
    /// token_client.try_transfer() was rejected by the *token* contract
    /// (e.g. insufficient balance, paused, bad allowance).
    /// Fix for Issue #355 — callers can now distinguish this from an
    /// internal disbursement failure without re-running with verbose logs.
    TokenXferFailed    = 6,

    /// An internal disbursement step failed (fee to admin, etc.).
    /// Kept separate from TokenXferFailed so the error code itself conveys
    /// which leg of the transfer pipeline failed.
    TransferFailed     = 7,
}