#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env, Vec,
};
use ethnum::U256;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Fixed-point Q96 scale used for sqrt price representation.
const Q96: U256 = U256::from(1u32) << 96;

/// Minimum sqrt price (x96), inclusive — matches Uniswap V3 v3-core.
const MIN_SQRT_PRICE_X96: u128 = 4_295_128_739;

/// Maximum sqrt price (x96), inclusive — matches Uniswap V3 v3-core.
const MAX_SQRT_PRICE_X96: u128 = 146_144_670_348_521_010_328_327_307_297_368_668_99u128;

/// Default fee (in hundredths of a basis point; 3000 = 0.30%).
pub const DEFAULT_FEE_PUPIPS: u32 = 3000;

/// Tick bounds (Uniswap V3 v3-core).
const MIN_TICK: i32 = -887_272;
const MAX_TICK: i32 = 887_272;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ConcentratedPairError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InvalidInput = 3,
    InsufficientLiquidity = 4,
    InsufficientOutputAmount = 5,
    InvalidSqrtPriceLimit = 6,
    TickOutOfBounds = 7,
    ZeroAmount = 8,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Snapshot of a tick: net liquidity added at this tick and gross liquidity.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TickInfo {
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

/// Persistent pair state.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PairState {
    pub token0: Address,
    pub token1: Address,
    pub sqrt_price_x96: u128,
    pub tick: i32,
    pub liquidity: u128,
    pub fee_pips: u32,
}

#[contracttype]
pub enum DataKey {
    PairState,
    Tick(i32),
}

mod storage {
    use super::*;

    pub fn get_state(env: &Env) -> Option<PairState> {
        env.storage().instance().get(&DataKey::PairState)
    }

    pub fn set_state(env: &Env, state: &PairState) {
        env.storage().instance().set(&DataKey::PairState, state);
    }

    pub fn get_tick(env: &Env, tick: i32) -> Option<TickInfo> {
        env.storage().instance().get(&DataKey::Tick(tick))
    }

    pub fn set_tick(env: &Env, tick: i32, info: &TickInfo) {
        env.storage().instance().set(&DataKey::Tick(tick), info);
    }
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ConcentratedPair;

#[contractimpl]
impl ConcentratedPair {
    // ------------------------------------------------------------------
    // Initialize
    // ------------------------------------------------------------------

    pub fn initialize(
        env: Env,
        factory: Address,
        token0: Address,
        token1: Address,
        sqrt_price_x96: u128,
        tick: i32,
        liquidity: u128,
        fee_pips: u32,
    ) -> Result<(), ConcentratedPairError> {
        if storage::get_state(&env).is_some() {
            return Err(ConcentratedPairError::AlreadyInitialized);
        }

        if sqrt_price_x96 < MIN_SQRT_PRICE_X96 || sqrt_price_x96 > MAX_SQRT_PRICE_X96 {
            return Err(ConcentratedPairError::InvalidSqrtPriceLimit);
        }
        if tick < MIN_TICK || tick > MAX_TICK {
            return Err(ConcentratedPairError::TickOutOfBounds);
        }
        if liquidity == 0 {
            return Err(ConcentratedPairError::InvalidInput);
        }

        storage::set_state(
            &env,
            &PairState {
                token0,
                token1,
                sqrt_price_x96,
                tick,
                liquidity,
                fee_pips,
            },
        );

        env.storage().instance().extend_ttl(
            coralswap_shared::INSTANCE_TTL_THRESHOLD,
            coralswap_shared::INSTANCE_TTL_EXTEND_TO,
        );

        Ok(())
    }

    // ------------------------------------------------------------------
    // Tick liquidity setup (seed path so `swap` has in-range liquidity)
    // ------------------------------------------------------------------

    /// Sets the liquidity net/gross at a single tick.
    pub fn set_tick_liquidity(
        env: Env,
        tick: i32,
        liquidity_net: i128,
        liquidity_gross: u128,
    ) -> Result<(), ConcentratedPairError> {
        if tick < MIN_TICK || tick > MAX_TICK {
            return Err(ConcentratedPairError::TickOutOfBounds);
        }
        storage::set_tick(
            &env,
            tick,
            &TickInfo { liquidity_net, liquidity_gross },
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // Swap (issue #200)
    // ------------------------------------------------------------------

    /// Swaps `amount_specified` of one token for the other, stepping through
    /// ticks until `amount_specified` is exhausted or `sqrt_price_limit` is
    /// reached, using [`compute_swap_step`] for each tick interval.
    ///
    /// # Arguments
    /// * `recipient` — address that receives the output token.
    /// * `zero_for_one` — `true` swaps token0 → token1 (price down),
    ///   `false` swaps token1 → token0 (price up).
    /// * `amount_specified` — exact amount of the input token.
    /// * `sqrt_price_limit` — price boundary the swap must not cross.
    /// * `data` — opaque swap data forwarded to hooks (unused here).
    ///
    /// # Errors
    /// | Error | Condition |
    /// |-------|-----------|
    /// | `InvalidInput` | zero amount or invalid limit direction |
    /// | `InsufficientLiquidity` | no in-range liquidity to complete the swap |
    /// | `InsufficientOutputAmount` | swap would exceed the price limit before finishing |
    pub fn swap(
        env: Env,
        recipient: Address,
        zero_for_one: bool,
        amount_specified: u128,
        sqrt_price_limit: u128,
        _data: Vec<u8>,
    ) -> Result<u128, ConcentratedPairError> {
        let mut state = storage::get_state(&env).ok_or(ConcentratedPairError::NotInitialized)?;

        if amount_specified == 0 {
            return Err(ConcentratedPairError::ZeroAmount);
        }
        if sqrt_price_limit < MIN_SQRT_PRICE_X96 || sqrt_price_limit > MAX_SQRT_PRICE_X96 {
            return Err(ConcentratedPairError::InvalidSqrtPriceLimit);
        }
        // The limit must move the price in the swap's direction.
        if zero_for_one && !(sqrt_price_limit < state.sqrt_price_x96) {
            return Err(ConcentratedPairError::InvalidInput);
        }
        if !zero_for_one && !(sqrt_price_limit > state.sqrt_price_x96) {
            return Err(ConcentratedPairError::InvalidInput);
        }

        let mut sqrt_price = state.sqrt_price_x96;
        let mut tick_current = state.tick;
        let mut liquidity = state.liquidity;
        let mut amount_remaining = amount_specified;
        let mut amount_calculated: i128 = 0;

        while amount_remaining > 0 && sqrt_price != sqrt_price_limit {
            // Move toward the limit, one tick interval at a time.
            let (sqrt_price_next, amount_in, amount_out) = compute_swap_step(
                sqrt_price,
                sqrt_price_limit,
                liquidity,
                amount_remaining,
                state.fee_pips,
                zero_for_one,
            )?;

            let amount_in_u128 = amount_in as u128;
            let amount_out_u128 = amount_out as u128;

            if amount_in_u128 > 0 {
                amount_remaining = amount_remaining.checked_sub(amount_in_u128).ok_or(
                    ConcentratedPairError::InvalidInput,
                )?;
            }
            // amount_calculated is signed: token0-in is positive, token1-in negative.
            if zero_for_one {
                amount_calculated = amount_calculated.checked_add(amount_out as i128).ok_or(
                    ConcentratedPairError::InvalidInput,
                )?;
            } else {
                amount_calculated = amount_calculated.checked_sub(amount_out as i128).ok_or(
                    ConcentratedPairError::InvalidInput,
                )?;
            }

            // Advance the price and tick to the computed next sqrt price.
            sqrt_price = sqrt_price_next;
            tick_current = sqrt_price_to_tick(sqrt_price)?;

            // Cross the tick boundary: update in-range liquidity by the tick's net.
            if let Some(tick_info) = storage::get_tick(&env, tick_current) {
                liquidity = (liquidity as i128)
                    .checked_add(tick_info.liquidity_net)
                    .and_then(|v| v.try_into().ok())
                    .ok_or(ConcentratedPairError::InvalidInput)?;
            }
        }

        if amount_remaining > 0 {
            return Err(ConcentratedPairError::InsufficientOutputAmount);
        }

        // Persist updated state.
        state.sqrt_price_x96 = sqrt_price;
        state.tick = tick_current;
        state.liquidity = liquidity;
        storage::set_state(&env, &state);

        // Transfer output tokens to the recipient.
        let token = if zero_for_one {
            state.token1.clone()
        } else {
            state.token0.clone()
        };
        soroban_sdk::token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount_calculated.unsigned_abs(),
        );

        env.storage().instance().extend_ttl(
            coralswap_shared::INSTANCE_TTL_THRESHOLD,
            coralswap_shared::INSTANCE_TTL_EXTEND_TO,
        );

        Ok(amount_calculated.unsigned_abs())
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_sqrt_price(env: Env) -> u128 {
        storage::get_state(&env).map(|s| s.sqrt_price_x96).unwrap_or(0)
    }

    pub fn get_tick_view(env: Env) -> i32 {
        storage::get_state(&env).map(|s| s.tick).unwrap_or(0)
    }

    pub fn get_liquidity(env: Env) -> u128 {
        storage::get_state(&env).map(|s| s.liquidity).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// compute_swap_step (issue #200)
// ---------------------------------------------------------------------------

/// Computes the result of moving the price from `sqrt_price_current_x96`
/// toward `sqrt_price_target_x96` with `liquidity` in range, consuming at most
/// `amount_remaining` of the input token.
///
/// Returns `(sqrt_price_next_x96, amount_in, amount_out)` in raw token units.
fn compute_swap_step(
    sqrt_price_current_x96: u128,
    sqrt_price_target_x96: u128,
    liquidity: u128,
    amount_remaining: u128,
    _fee_pips: u32,
    zero_for_one: bool,
) -> Result<(u128, u128, u128), ConcentratedPairError> {
    if liquidity == 0 {
        return Err(ConcentratedPairError::InsufficientLiquidity);
    }

    let q96 = Q96;
    let start = U256::from(sqrt_price_current_x96);
    let target = U256::from(sqrt_price_target_x96);
    let liq = U256::from(liquidity);
    let amt = U256::from(amount_remaining);

    if zero_for_one {
        // Price moving down: token0 in, token1 out.
        // amount_in = liquidity * (Q96/sqrt_next - Q96/sqrt_start)
        // => Q96/sqrt_next = Q96/sqrt_start + amount_in/liquidity   (scaled by Q96)
        let q96_over_start = q96 / start;
        let amount_scaled = (amt << 96) / liq;
        let q96_over_next = q96_over_start.checked_add(amount_scaled).ok_or(
            ConcentratedPairError::InvalidInput,
        )?;
        if q96_over_next == U256::from(0u32) {
            return Err(ConcentratedPairError::InsufficientLiquidity);
        }
        let mut sqrt_next = (q96 / q96_over_next)
            .try_into()
            .map_err(|_| ConcentratedPairError::InvalidInput)?;
        // Never cross the target price (target < start when moving down).
        if sqrt_next < sqrt_price_target_x96 {
            sqrt_next = sqrt_price_target_x96;
        }
        let sqrt_next_u256 = U256::from(sqrt_next);

        // amount_in consumed to reach sqrt_next
        let amount_in = ((q96_over_next - q96_over_start) * liq >> 96)
            .try_into()
            .map_err(|_| ConcentratedPairError::InvalidInput)?;
        // amount_out = liquidity * (sqrt_start - sqrt_next) / Q96
        let amount_out = (liq * (start - sqrt_next_u256) / q96)
            .try_into()
            .map_err(|_| ConcentratedPairError::InvalidInput)?;

        Ok((sqrt_next, amount_in, amount_out))
    } else {
        // Price moving up: token1 in, token0 out.
        // amount1 = liquidity * (sqrt_next - sqrt_start) / Q96
        let amount_scaled = (amt << 96) / liq;
        let mut sqrt_next = (start + amount_scaled)
            .try_into()
            .map_err(|_| ConcentratedPairError::InvalidInput)?;
        if sqrt_next > sqrt_price_target_x96 {
            sqrt_next = sqrt_price_target_x96;
        }
        let sqrt_next_u256 = U256::from(sqrt_next);

        // amount_in (token1) = liquidity * (sqrt_next - sqrt_start) / Q96
        let amount_in = (liq * (sqrt_next_u256 - start) / q96)
            .try_into()
            .map_err(|_| ConcentratedPairError::InvalidInput)?;
        // amount_out (token0) = liquidity * (Q96/sqrt_start - Q96/sqrt_next)
        let q96_over_start = q96 / start;
        let q96_over_next = q96 / sqrt_next_u256;
        let amount_out = ((q96_over_start - q96_over_next) * liq >> 96)
            .try_into()
            .map_err(|_| ConcentratedPairError::InvalidInput)?;

        Ok((sqrt_next, amount_in, amount_out))
    }
}

/// Converts a sqrt price (x96) to the nearest tick (best-effort approximation).
fn sqrt_price_to_tick(sqrt_price_x96: u128) -> Result<i32, ConcentratedPairError> {
    if sqrt_price_x96 == 0 {
        return Err(ConcentratedPairError::InvalidInput);
    }
    let v = U256::from(sqrt_price_x96);
    let leading = v.leading_zeros();
    let bit_len = 256u128 - leading as u128;
    let log2_sqrt = bit_len as i128 - 96i128;
    let log2_price = 2 * log2_sqrt;
    // log2(1.0001) ≈ 0.0001442695; scaled by 1e6 → 144
    let tick = (log2_price * 1_000_000 / 144) / 2;
    Ok(tick.clamp(MIN_TICK as i128, MAX_TICK as i128) as i32)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_swap_step_zero_for_one_moves_price_down() {
        let (sqrt_next, amount_in, amount_out) = compute_swap_step(
            1_000_000_000_000_000_000_000_000_000_000_000_000_000u128,
            1,
            1_000_000,
            10_000,
            3000,
            true,
        )
        .unwrap();
        assert!(sqrt_next < 1_000_000_000_000_000_000_000_000_000_000_000_000_000u128);
        assert!(amount_in > 0);
        assert!(amount_out > 0);
    }

    #[test]
    fn test_compute_swap_step_zero_liquidity_fails() {
        let result = compute_swap_step(
            1_000_000_000_000_000_000_000_000_000_000_000_000_000u128,
            1,
            0,
            10_000,
            3000,
            true,
        );
        assert_eq!(result, Err(ConcentratedPairError::InsufficientLiquidity));
    }
}
