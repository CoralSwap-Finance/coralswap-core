#![no_std]

mod errors;
mod helpers;

use soroban_sdk::{contract, contractimpl, contractclient, token, Address, Env, Vec};
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
    /// * `factory` - The Factory contract address for pair lookups
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
        factory: Address,
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

        // ── 4. Initialize amounts vector ──────────────────────────────────────
        let mut amounts = Vec::new(&env);
        amounts.push_back(amount_in);

        // ── 5. Calculate output amounts for each hop ──────────────────────────
        let factory_client = FactoryClient::new(&env, &factory);

        for i in 0..(path.len() - 1) {
            let token_in = path.get(i).unwrap();
            let token_out = path.get(i + 1).unwrap();

            // Get pair address from factory
            let pair_addr = factory_client
                .get_pair(&token_in, &token_out)
                .ok_or(RouterError::PairNotFound)?;

            let pair_client = PairClient::new(&env, &pair_addr);

            // Get reserves and fee
            let (reserve_a, reserve_b, _timestamp) = pair_client.get_reserves();
            let fee_bps = pair_client.get_current_fee_bps();

            // Determine which reserve is input and which is output based on token ordering
            let (token_a, token_b) = helpers::sort_tokens(&token_in, &token_out)?;
            let (reserve_in, reserve_out) = if token_in == token_a {
                (reserve_a, reserve_b)
            } else {
                (reserve_b, reserve_a)
            };

            // Calculate output amount for this hop
            let current_amount_in = amounts.get(i).unwrap();
            let amount_out = helpers::get_amount_out(
                &env,
                current_amount_in,
                reserve_in,
                reserve_out,
                fee_bps,
            )?;

            amounts.push_back(amount_out);
        }

        // ── 6. Verify minimum output (slippage protection) ───────────────────
        let final_amount = amounts.get(amounts.len() - 1).unwrap();
        if final_amount < amount_out_min {
            return Err(RouterError::InsufficientOutputAmount);
        }

        // ── 7. Transfer input tokens to first pair ───────────────────────────
        let first_token = path.get(0).unwrap();
        let second_token = path.get(1).unwrap();
        let first_pair = factory_client
            .get_pair(&first_token, &second_token)
            .ok_or(RouterError::PairNotFound)?;

        // Transfer from caller to first pair
        token::Client::new(&env, &first_token).transfer(
            &env.invoker(),
            &first_pair,
            &amount_in,
        );

        // ── 8. Execute swaps through each pair ────────────────────────────────
        for i in 0..(path.len() - 1) {
            let token_in = path.get(i).unwrap();
            let token_out = path.get(i + 1).unwrap();

            // Get pair address
            let pair_addr = factory_client
                .get_pair(&token_in, &token_out)
                .ok_or(RouterError::PairNotFound)?;

            // Determine destination for this hop
            let destination = if i < path.len() - 2 {
                // Intermediate hop: send to next pair
                let next_token = path.get(i + 2).unwrap();
                factory_client
                    .get_pair(&token_out, &next_token)
                    .ok_or(RouterError::PairNotFound)?
            } else {
                // Final hop: send to recipient
                to.clone()
            };

            // Determine swap parameters based on token ordering
            let (token_a, token_b) = helpers::sort_tokens(&token_in, &token_out)?;
            let amount_out = amounts.get(i + 1).unwrap();

            let pair_client = PairClient::new(&env, &pair_addr);

            if token_in == token_a {
                // Swapping A → B: amount_a_out = 0, amount_b_out = calculated
                pair_client.swap(&0, &amount_out, &destination);
            } else {
                // Swapping B → A: amount_a_out = calculated, amount_b_out = 0
                pair_client.swap(&amount_out, &0, &destination);
            }
        }

        Ok(amounts)
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
