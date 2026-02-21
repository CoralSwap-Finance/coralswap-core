#![no_std]

mod errors;
mod helpers;

use soroban_sdk::{contract, contractimpl, contractclient, Address, Env, Vec};
use errors::RouterError;

#[contractclient(name = "FactoryClient")]
pub trait FactoryInterface {
    fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address>;
}

#[contractclient(name = "PairClient")]
pub trait PairInterface {
    fn swap(env: Env, amount_a_out: i128, amount_b_out: i128, to: Address);
    fn get_reserves(env: Env) -> (i128, i128, u64);
    fn get_current_fee_bps(env: Env) -> u32;
}

#[contract]
pub struct Router;

#[contractimpl]
impl Router {
    /// Swaps an exact amount of input tokens for output tokens through multiple hops.
    ///
    /// # Arguments
    /// * `amount_in` - The exact amount of input tokens to swap
    /// * `amount_out_min` - The minimum amount of output tokens to receive (slippage protection)
    /// * `path` - Vector of token addresses representing the swap route
    /// * `to` - The recipient address for output tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    ///
    /// # Returns
    /// Vector of amounts for each step in the path (including input and all outputs)
    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        // ── 1. Validate deadline ─────────────────────────────────────────────
        if env.ledger().timestamp() > deadline {
            return Err(RouterError::Expired);
        }

        // ── 2. Validate path ──────────────────────────────────────────────────
        if path.len() < 2 {
            return Err(RouterError::InvalidPath);
        }

        // Check for duplicate adjacent tokens
        for i in 0..(path.len() - 1) {
            if path.get(i).unwrap() == path.get(i + 1).unwrap() {
                return Err(RouterError::IdenticalTokens);
            }
        }

        // ── 3. Validate amounts ───────────────────────────────────────────────
        if amount_in <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        if amount_out_min < 0 {
            return Err(RouterError::InsufficientOutputAmount);
        }

        // Placeholder for remaining implementation
        todo!()
    }

    pub fn swap_tokens_for_exact_tokens(
        env: Env, amount_out: i128, amount_in_max: i128,
        path: Vec<Address>, to: Address, deadline: u64,
    ) -> Result<Vec<i128>, RouterError> { todo!() }

    pub fn add_liquidity(
        env: Env, token_a: Address, token_b: Address,
        amount_a_desired: i128, amount_b_desired: i128,
        amount_a_min: i128, amount_b_min: i128,
        to: Address, deadline: u64,
    ) -> Result<(i128, i128, i128), RouterError> { todo!() }

    pub fn remove_liquidity(
        env: Env, token_a: Address, token_b: Address,
        liquidity: i128, amount_a_min: i128, amount_b_min: i128,
        to: Address, deadline: u64,
    ) -> Result<(i128, i128), RouterError> { todo!() }
}
