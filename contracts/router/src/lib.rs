#![no_std]

mod errors;
mod helpers;
mod storage;

#[cfg(test)]
mod test;

use errors::RouterError;
use helpers::{get_pair_address, PairClient};
use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Env, Vec};
use storage::{get_factory, set_factory};

#[contract]
pub struct Router;

#[contractimpl]
impl Router {
    pub fn initialize(env: Env, factory: Address) {
        set_factory(&env, &factory);
    }
    pub fn swap_exact_tokens_for_tokens(
        _env: Env,
        _amount_in: i128,
        _amount_out_min: i128,
        _path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        todo!()
    }

    /// Swaps tokens to receive an exact amount of output tokens (not yet implemented).
    ///
    /// # Arguments
    /// * `amount_out` - The exact amount of output tokens desired
    /// * `amount_in_max` - The maximum amount of input tokens to spend
    /// * `path` - Vector of token addresses representing the swap route
    /// * `to` - The recipient address for output tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn swap_tokens_for_exact_tokens(
        _env: Env,
        _amount_out: i128,
        _amount_in_max: i128,
        _path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        todo!()
    }

    /// Adds liquidity to a token pair (not yet implemented).
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `amount_a_desired` - Desired amount of token_a to add
    /// * `amount_b_desired` - Desired amount of token_b to add
    /// * `amount_a_min` - Minimum amount of token_a to add
    /// * `amount_b_min` - Minimum amount of token_b to add
    /// * `to` - Recipient of LP tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn add_liquidity(
        _env: Env,
        _token_a: Address,
        _token_b: Address,
        _amount_a_desired: i128,
        _amount_b_desired: i128,
        _amount_a_min: i128,
        _amount_b_min: i128,
        _to: Address,
        _deadline: u64,
    ) -> Result<(i128, i128, i128), RouterError> {
        todo!()
    }

    /// Removes liquidity from a token pair (not yet implemented).
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `liquidity` - Amount of LP tokens to burn
    /// * `amount_a_min` - Minimum amount of token_a to receive
    /// * `amount_b_min` - Minimum amount of token_b to receive
    /// * `to` - Recipient of underlying tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn remove_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        liquidity: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> Result<(i128, i128), RouterError> {
        // Check deadline
        if env.ledger().timestamp() > deadline {
            return Err(RouterError::Expired);
        }

        // Check for zero amounts
        if liquidity <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        // Get factory address
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Get pair address
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b)?;

        // Get pair contract client
        let pair_client = PairClient::new(&env, &pair_address);

        // Get LP token address from pair
        let lp_token_address = pair_client.lp_token();

        // The user must provide authorization for the Router to transfer LP tokens
        to.require_auth();

        // Transfer LP tokens from 'to' to pair
        let lp_token_client = TokenClient::new(&env, &lp_token_address);
        lp_token_client.transfer(&to, &pair_address, &liquidity);

        // Call Pair::burn(to) - this will burn LP tokens from the pair and transfer underlying tokens
        let (amount_a, amount_b) = pair_client.burn(&to);

        // Check slippage
        if amount_a < amount_a_min || amount_b < amount_b_min {
            return Err(RouterError::SlippageExceeded);
        }

        Ok((amount_a, amount_b))
    }
}
