#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod storage;

use errors::LpTokenError;
use soroban_sdk::{contract, contractimpl, Address, Env, String};

#[contract]
pub struct LpToken;

#[contractimpl]
impl LpToken {
    pub fn initialize(
        _env: Env,
        _admin: Address,
        _decimals: u32,
        _name: String,
        _symbol: String,
    ) -> Result<(), LpTokenError> {
        todo!()
    }

    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        todo!()
    }

    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _expiration_ledger: u32,
    ) -> Result<(), LpTokenError> {
        todo!()
    }

    pub fn balance(_env: Env, _id: Address) -> i128 {
        todo!()
    }

    pub fn transfer(
        _env: Env,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) -> Result<(), LpTokenError> {
        todo!()
    }

    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) -> Result<(), LpTokenError> {
        todo!()
    }

    pub fn mint(_env: Env, _to: Address, _amount: i128) -> Result<(), LpTokenError> {
        todo!()
    }
    pub fn burn(_env: Env, _from: Address, _amount: i128) -> Result<(), LpTokenError> {
        todo!()
    }

    pub fn decimals(_env: Env) -> u32 {
        todo!()
    }
    pub fn name(_env: Env) -> String {
        todo!()
    }
    pub fn symbol(_env: Env) -> String {
        todo!()
    }
    pub fn total_supply(_env: Env) -> i128 {
        todo!()
    }
}
