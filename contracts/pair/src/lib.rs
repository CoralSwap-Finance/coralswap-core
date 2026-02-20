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

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env};
use errors::PairError;
use events::PairEvents;
use storage::{get_fee_state, get_pair_storage, set_fee_state, set_pair_storage};

#[contract]
pub struct Pair;

#[contractimpl]
impl Pair {
    // ── Initialization ────────────────────────────────────────────────────────

    pub fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
    ) -> Result<(), PairError> {
        if env.storage().instance().has(&storage::StorageKey::Pair) {
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
        set_pair_storage(&env, &state);
        Ok(())
    }

    // ── Liquidity ─────────────────────────────────────────────────────────────

    pub fn mint(env: Env, to: Address) -> Result<i128, PairError> {
        let _ = (env, to);
        todo!()
    }

    pub fn burn(env: Env, to: Address) -> Result<(i128, i128), PairError> {
        let _ = (env, to);
        todo!()
    }

    // ── Swap ──────────────────────────────────────────────────────────────────

    /// Constant-product swap with dynamic fee and reentrancy protection.
    ///
    /// Caller must have already transferred input tokens to this contract
    /// before calling (standard Uniswap V2 pattern).
    ///
    /// # Arguments
    /// * `amount_a_out` – amount of token_a to send out (0 if swapping A→B)
    /// * `amount_b_out` – amount of token_b to send out (0 if swapping B→A)
    /// * `to`           – recipient of the output tokens
    pub fn swap(
        env: Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: Address,
    ) -> Result<(), PairError> {
        // ── 1. Reentrancy guard ───────────────────────────────────────────────
        reentrancy::acquire(&env)?;

        let result = Self::swap_inner(&env, amount_a_out, amount_b_out, &to);

        // Always release guard, even on error.
        reentrancy::release(&env);

        result
    }

    fn swap_inner(
        env: &Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: &Address,
    ) -> Result<(), PairError> {
        // ── 2. Input validation ───────────────────────────────────────────────
        if amount_a_out <= 0 && amount_b_out <= 0 {
            return Err(PairError::InsufficientOutputAmount);
        }

        // ── 3. Load state ─────────────────────────────────────────────────────
        let mut pair = get_pair_storage(env)?;
        let mut fee_state = get_fee_state(env);

        // ── 4. Check output vs reserves ───────────────────────────────────────
        if amount_a_out >= pair.reserve_a || amount_b_out >= pair.reserve_b {
            return Err(PairError::InsufficientLiquidity);
        }

        // ── 5. Decay stale fee before computing ───────────────────────────────
        dynamic_fee::decay_stale_ema(env, &mut fee_state);

        // ── 6. Compute fee ───────────────────────────────────────────────────
        let fee_bps = dynamic_fee::compute_fee_bps(&fee_state);

        // ── 7. Optimistic transfer: send output tokens to recipient ───────────
        let contract_address = env.current_contract_address();

        if amount_a_out > 0 {
            token::Client::new(env, &pair.token_a)
                .transfer(&contract_address, to, &amount_a_out);
        }
        if amount_b_out > 0 {
            token::Client::new(env, &pair.token_b)
                .transfer(&contract_address, to, &amount_b_out);
        }

        // ── 8. Read actual balances post-transfer ─────────────────────────────
        let balance_a = token::Client::new(env, &pair.token_a)
            .balance(&contract_address);
        let balance_b = token::Client::new(env, &pair.token_b)
            .balance(&contract_address);

        // ── 9. Compute effective amounts in ───────────────────────────────────
        // amount_in = new_balance - (old_reserve - amount_out), floored at 0
        let amount_a_in = (balance_a - (pair.reserve_a - amount_a_out)).max(0);
        let amount_b_in = (balance_b - (pair.reserve_b - amount_b_out)).max(0);

        if amount_a_in <= 0 && amount_b_in <= 0 {
            return Err(PairError::InsufficientInputAmount);
        }

        // ── 10. Fee-adjusted balances (Uniswap V2 K check) ───────────────────
        // balance_adjusted = balance * 10_000 - amount_in * fee_bps
        // This avoids floating point: multiply reserves by 10_000 so fee
        // subtraction is exact.
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

        if balance_a_adj <= 0 || balance_b_adj <= 0 {
            return Err(PairError::InsufficientOutputAmount);
        }

        // ── 11. K-invariant check ─────────────────────────────────────────────
        // balance_a_adj * balance_b_adj >= reserve_a * reserve_b * 10_000^2
        let k_before = pair
            .reserve_a
            .checked_mul(pair.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_mul(100_000_000) // 10_000^2
            .ok_or(PairError::Overflow)?;

        let k_after = balance_a_adj
            .checked_mul(balance_b_adj)
            .ok_or(PairError::Overflow)?;

        if k_after < k_before {
            return Err(PairError::InvalidK);
        }

        // ── 12. Update volatility EMA ─────────────────────────────────────────
        // Price delta: |reserve_b/reserve_a - new_balance_b/new_balance_a|
        // Approximate with integer arithmetic.
        let total_reserve = pair.reserve_a.saturating_add(pair.reserve_b);
        let trade_size = amount_a_in.max(amount_b_in);
        // Simple price delta proxy: change in effective reserve ratio.
        let old_price = if pair.reserve_a > 0 {
            (pair.reserve_b * 10_000) / pair.reserve_a
        } else {
            0
        };
        let new_price = if balance_a > 0 {
            (balance_b * 10_000) / balance_a
        } else {
            0
        };
        let price_delta = (new_price - old_price).unsigned_abs() as i128;

        dynamic_fee::update_volatility(
            env,
            &mut fee_state,
            price_delta,
            trade_size,
            total_reserve,
        );

        // ── 13. Update K_last and reserves ────────────────────────────────────
        pair.k_last = balance_a * balance_b;
        pair.reserve_a = balance_a;
        pair.reserve_b = balance_b;
        pair.block_timestamp_last = env.ledger().timestamp();

        // ── 14. Persist state ─────────────────────────────────────────────────
        set_pair_storage(env, &pair);
        set_fee_state(env, &fee_state);

        // ── 15. Emit swap event ───────────────────────────────────────────────
        // sender = invoker (the caller who initiated this swap)
        let sender = to; // conservative: use `to` as event sender proxy
        PairEvents::swap(
            env,
            sender,
            amount_a_in,
            amount_b_in,
            amount_a_out,
            amount_b_out,
            fee_bps,
            to,
        );

        Ok(())
    }

    // ── Flash Loan ────────────────────────────────────────────────────────────

    /// Executes a flash loan of up to `amount_a` of token_a and/or `amount_b`
    /// of token_b to `receiver`.  The receiver must repay principal + fee
    /// before the `on_flash_loan` callback returns.
    pub fn flash_loan(
        env: Env, receiver: Address, amount_a: i128,
        amount_b: i128, data: Bytes,
    ) -> Result<(), PairError> {
        flash_loan::execute_flash_loan(&env, &receiver, amount_a, amount_b, &data)
    }

        PairEvents::sync(&env, balance_a, balance_b);
        Ok(())
    }
}
