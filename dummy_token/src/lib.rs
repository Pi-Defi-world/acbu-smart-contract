#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct Token;

#[contractimpl]
impl Token {
    pub fn dummy(env: Env) {}
}
