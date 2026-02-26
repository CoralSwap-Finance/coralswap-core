#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

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
mod test;

use errors::PairError;
use events::PairEvents;
use math::MINIMUM_LIQUIDITY;
use soroban_sdk::{contract, contractclient, contractimpl, token::TokenClient, Address, Env};
use storage::{get_fee_state, get_pair_state, set_fee_state, set_pair_state};

#[contractclient(name = "LpTokenClient")]
pub trait LpTokenInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn total_supply(env: Env) -> i128;
}

#[contract]
pub struct Pair;

#[contractimpl]
impl Pair {
    // ─────────────────────────────────────────
    // Initialize
    // ─────────────────────────────────────────

    pub fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
    ) -> Result<(), PairError> {
        if get_pair_state(&env).is_some() {
            return Err(PairError::AlreadyInitialized);
        }

        let state = storage::PairStorage {
            factory,
            token_a,
            token_b,
            lp_token,
            reserve_a: 0,
            reserve_b: 0,
            block_timestamp_last: env.ledger().timestamp(),
            price_a_cumulative: 0,
            price_b_cumulative: 0,
            k_last: 0,
        };

        set_pair_state(&env, &state);
        Ok(())
    }

    // ─────────────────────────────────────────
    // Mint
    // ─────────────────────────────────────────

    pub fn mint(env: Env, to: Address) -> Result<i128, PairError> {
        to.require_auth();

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();

        let balance_a = TokenClient::new(&env, &state.token_a).balance(&contract);
        let balance_b = TokenClient::new(&env, &state.token_b).balance(&contract);

        let amount_a = balance_a.checked_sub(state.reserve_a).ok_or(PairError::InvalidInput)?;

        let amount_b = balance_b.checked_sub(state.reserve_b).ok_or(PairError::InvalidInput)?;

        let lp_client = LpTokenClient::new(&env, &state.lp_token);
        let total_supply = lp_client.total_supply();

        let liquidity;

        if total_supply == 0 {
            let product = amount_a.checked_mul(amount_b).ok_or(PairError::Overflow)?;

            liquidity = math::sqrt(product)
                .checked_sub(MINIMUM_LIQUIDITY)
                .ok_or(PairError::InsufficientLiquidityMinted)?;

            lp_client.mint(&contract, &MINIMUM_LIQUIDITY);
        } else {
            let liquidity_a = amount_a
                .checked_mul(total_supply)
                .ok_or(PairError::Overflow)?
                .checked_div(state.reserve_a)
                .ok_or(PairError::Overflow)?;

            let liquidity_b = amount_b
                .checked_mul(total_supply)
                .ok_or(PairError::Overflow)?
                .checked_div(state.reserve_b)
                .ok_or(PairError::Overflow)?;

            liquidity = liquidity_a.min(liquidity_b);
        }

        if liquidity <= 0 {
            return Err(PairError::InsufficientLiquidityMinted);
        }

        lp_client.mint(&to, &liquidity);

        state.reserve_a = balance_a;
        state.reserve_b = balance_b;
        state.k_last = balance_a.checked_mul(balance_b).ok_or(PairError::Overflow)?;

        state.block_timestamp_last = env.ledger().timestamp();

        set_pair_state(&env, &state);

        PairEvents::mint(&env, &to, amount_a, amount_b);

        Ok(liquidity)
    }

    // ─────────────────────────────────────────
    // Burn
    // ─────────────────────────────────────────

    pub fn burn(env: Env, to: Address) -> Result<(i128, i128), PairError> {
        to.require_auth();

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();

        let lp_balance = TokenClient::new(&env, &state.lp_token).balance(&contract);

        let total_supply = LpTokenClient::new(&env, &state.lp_token).total_supply();

        if total_supply == 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        let amount_a = lp_balance
            .checked_mul(state.reserve_a)
            .ok_or(PairError::Overflow)?
            .checked_div(total_supply)
            .ok_or(PairError::Overflow)?;

        let amount_b = lp_balance
            .checked_mul(state.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_div(total_supply)
            .ok_or(PairError::Overflow)?;

        if amount_a <= 0 || amount_b <= 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        LpTokenClient::new(&env, &state.lp_token).burn(&contract, &lp_balance);

        TokenClient::new(&env, &state.token_a).transfer(&contract, &to, &amount_a);

        TokenClient::new(&env, &state.token_b).transfer(&contract, &to, &amount_b);

        state.reserve_a = state.reserve_a.checked_sub(amount_a).ok_or(PairError::Overflow)?;

        state.reserve_b = state.reserve_b.checked_sub(amount_b).ok_or(PairError::Overflow)?;

        state.k_last = state.reserve_a.checked_mul(state.reserve_b).ok_or(PairError::Overflow)?;

        state.block_timestamp_last = env.ledger().timestamp();

        set_pair_state(&env, &state);

        PairEvents::burn(&env, &to, amount_a, amount_b, &to);

        Ok((amount_a, amount_b))
    }

    // ─────────────────────────────────────────
    // Swap
    // ─────────────────────────────────────────

    pub fn swap(
        env: Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: Address,
    ) -> Result<(), PairError> {
        reentrancy::acquire(&env)?;
        let result = Self::swap_inner(&env, amount_a_out, amount_b_out, &to);
        reentrancy::release(&env);
        result
    }

    fn swap_inner(
        env: &Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: &Address,
    ) -> Result<(), PairError> {
        if amount_a_out <= 0 && amount_b_out <= 0 {
            return Err(PairError::InsufficientOutputAmount);
        }

        let mut pair = get_pair_state(env).ok_or(PairError::NotInitialized)?;
        let mut fee_state = get_fee_state(env).ok_or(PairError::NotInitialized)?;

        if amount_a_out >= pair.reserve_a || amount_b_out >= pair.reserve_b {
            return Err(PairError::InsufficientLiquidity);
        }

        dynamic_fee::decay_stale_ema(env, &mut fee_state);
        let fee_bps = dynamic_fee::compute_fee_bps(&fee_state);

        let contract_address = env.current_contract_address();

        if amount_a_out > 0 {
            TokenClient::new(env, &pair.token_a).transfer(&contract_address, to, &amount_a_out);
        }

        if amount_b_out > 0 {
            TokenClient::new(env, &pair.token_b).transfer(&contract_address, to, &amount_b_out);
        }

        let balance_a = TokenClient::new(env, &pair.token_a).balance(&contract_address);

        let balance_b = TokenClient::new(env, &pair.token_b).balance(&contract_address);

        let amount_a_in = (balance_a - (pair.reserve_a - amount_a_out)).max(0);

        let amount_b_in = (balance_b - (pair.reserve_b - amount_b_out)).max(0);

        if amount_a_in <= 0 && amount_b_in <= 0 {
            return Err(PairError::InsufficientInputAmount);
        }

        let fee = fee_bps as i128;

        let balance_a_adj = balance_a
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(amount_a_in * fee)
            .ok_or(PairError::Overflow)?;

        let balance_b_adj = balance_b
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(amount_b_in * fee)
            .ok_or(PairError::Overflow)?;

        let k_before = pair
            .reserve_a
            .checked_mul(pair.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_mul(100_000_000)
            .ok_or(PairError::Overflow)?;

        let k_after = balance_a_adj.checked_mul(balance_b_adj).ok_or(PairError::Overflow)?;

        if k_after < k_before {
            return Err(PairError::InvalidK);
        }

        pair.reserve_a = balance_a;
        pair.reserve_b = balance_b;
        pair.k_last = balance_a.checked_mul(balance_b).ok_or(PairError::Overflow)?;

        pair.block_timestamp_last = env.ledger().timestamp();

        set_pair_state(env, &pair);
        set_fee_state(env, &fee_state);

        PairEvents::swap(
            env,
            to,
            amount_a_in,
            amount_b_in,
            amount_a_out,
            amount_b_out,
            fee_bps,
            to,
        );

        Ok(())
    }
}
