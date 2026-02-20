#![no_std]

mod dynamic_fee;
mod errors;
mod events;
mod fee_decay;
mod flash_loan;
mod math;
mod oracle;
mod reentrancy;
mod storage;

#[cfg(test)]
extern crate std; // soroban-sdk testutils require std; pair is no_std so we must opt-in explicitly.

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env};
use errors::PairError;

#[contract]
pub struct Pair;

#[contractimpl]
impl Pair {
    pub fn initialize(
        env: Env, factory: Address, token_a: Address,
        token_b: Address, lp_token: Address,
    ) -> Result<(), PairError> { todo!() }

    pub fn mint(env: Env, to: Address) -> Result<i128, PairError> { todo!() }
    pub fn burn(env: Env, to: Address) -> Result<(i128, i128), PairError> { todo!() }

    pub fn swap(
        env: Env, amount_a_out: i128, amount_b_out: i128, to: Address,
    ) -> Result<(), PairError> { todo!() }

    /// Executes a flash loan of up to `amount_a` of token_a and/or `amount_b`
    /// of token_b to `receiver`.  The receiver must repay principal + fee
    /// before the `on_flash_loan` callback returns.
    pub fn flash_loan(
        env: Env, receiver: Address, amount_a: i128,
        amount_b: i128, data: Bytes,
    ) -> Result<(), PairError> {
        flash_loan::execute_flash_loan(&env, &receiver, amount_a, amount_b, &data)
    }

    pub fn get_reserves(env: Env) -> (i128, i128, u64) { todo!() }
    pub fn get_current_fee_bps(env: Env) -> u32 { todo!() }
    pub fn sync(env: Env) -> Result<(), PairError> { todo!() }
}
