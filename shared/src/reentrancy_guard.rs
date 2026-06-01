#![no_std]

use soroban_sdk::{contracterror, contracttype, symbol_short, Env, Symbol};

/// Re-entrancy guard error
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ReentrancyError {
    ReentrantCall = 6001,
}

/// Storage key for re-entrancy guard status
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReentrancyGuardKey {
    pub guard: Symbol,
}

const REENTRANCY_GUARD_KEY: ReentrancyGuardKey = ReentrancyGuardKey {
    guard: symbol_short!("REENTRANCY_GUARD"),
};

/// Acquire the re-entrancy guard
/// This must be called at the beginning of any function that makes external calls
/// If the guard is already set, it will panic with ReentrancyError::ReentrantCall
pub fn acquire_guard(env: &Env) {
    if env.storage().instance().has(&REENTRANCY_GUARD_KEY) {
        env.panic_with_error(ReentrancyError::ReentrantCall);
    }
    env.storage().instance().set(&REENTRANCY_GUARD_KEY, &true);
}

/// Release the re-entrancy guard
/// This must be called at the end of any function that acquired the guard
/// Typically called in a drop guard or at the end of the function
pub fn release_guard(env: &Env) {
    env.storage().instance().remove(&REENTRANCY_GUARD_KEY);
}

/// Check if the re-entrancy guard is currently set
pub fn is_guard_active(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&REENTRANCY_GUARD_KEY)
        .unwrap_or(false)
}
