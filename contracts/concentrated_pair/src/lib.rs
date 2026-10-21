#![cfg_attr(not(test), no_std)]

//! Concentrated-liquidity pair (issue #196).
//!
//! Successor architecture to the V2 constant-product [`pair`]: liquidity is
//! provided inside `[tick_lower, tick_upper)` ranges instead of across the
//! whole curve, so the pool tracks a current √price plus the net liquidity
//! change at each tick boundary rather than two reserves.
//!
//! This crate lands the state model and initialization; positions, swaps and
//! fee accounting build on it in follow-up issues.
//!
//! [`pair`]: ../coralswap_pair/index.html

#[cfg(test)]
extern crate std;

pub mod errors;
mod events;
pub mod storage;
pub mod tick_math;

#[cfg(test)]
mod test;

use errors::ConcentratedPairError;
use events::ConcentratedPairEvents;
use soroban_sdk::{contract, contractimpl, Address, Env};
use storage::{
    get_config, get_liquidity, get_slot0, get_tick, set_config, set_liquidity, set_slot0, Config,
    Slot0, TickInfo,
};

/// Upper bound on the LP fee: 10%. Generous next to the V2 pair's dynamic-fee
/// ceiling (100 bps) so exotic fee tiers stay possible, but far from a value
/// that could be abused to effectively confiscate swaps.
pub const MAX_FEE_BPS: u32 = 1_000;

/// Upper bound on tick spacing. One usable tick must always exist on each
/// side of any price: `MAX_TICK / MAX_TICK_SPACING ≈ 27`.
pub const MAX_TICK_SPACING: u32 = 16_384;

#[contract]
pub struct ConcentratedPair;

#[contractimpl]
impl ConcentratedPair {
    // ─────────────────────────────────────────
    // Initialize
    // ─────────────────────────────────────────

    /// Initializes the pool at `sqrt_price` (Q64.64 √(token B per token A)).
    ///
    /// The current tick is *derived* from `sqrt_price` rather than accepted
    /// from the caller, so `slot0.tick == tick_at_sqrt_ratio(slot0.sqrt_price)`
    /// holds from the first ledger — an inconsistent pair handed in by a
    /// deployer could otherwise place the price on the wrong side of a tick
    /// boundary and corrupt the first crossing.
    ///
    /// # Errors
    /// - [`ConcentratedPairError::AlreadyInitialized`] on re-entry.
    /// - [`ConcentratedPairError::InvalidInput`] for identical or
    ///   self-referential tokens, a zero or out-of-range fee, or a zero or
    ///   out-of-range tick spacing.
    /// - [`ConcentratedPairError::InvalidSqrtPrice`] when `sqrt_price` is
    ///   outside `[MIN_SQRT_RATIO, MAX_SQRT_RATIO)`.
    pub fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        fee_bps: u32,
        tick_spacing: u32,
        sqrt_price: u128,
    ) -> Result<(), ConcentratedPairError> {
        // 1. Double-init guard.
        if get_config(&env).is_some() {
            return Err(ConcentratedPairError::AlreadyInitialized);
        }

        // 2. Token sanity — same policy as Pair::initialize: a pool token may
        // not be the factory, the pool itself, or the other token, or reserve
        // accounting becomes circular.
        if token_a == token_b
            || token_a == factory
            || token_b == factory
            || token_a == env.current_contract_address()
            || token_b == env.current_contract_address()
        {
            return Err(ConcentratedPairError::InvalidInput);
        }

        // 3. Parameter bounds.
        if fee_bps == 0 || fee_bps > MAX_FEE_BPS {
            return Err(ConcentratedPairError::InvalidInput);
        }
        if tick_spacing == 0 || tick_spacing > MAX_TICK_SPACING {
            return Err(ConcentratedPairError::InvalidInput);
        }

        // 4. Initial price: validate and derive the current tick from it.
        // tick_at_sqrt_ratio enforces the [MIN, MAX) √price range itself.
        let tick = tick_math::tick_at_sqrt_ratio(sqrt_price)?;

        set_config(
            &env,
            &Config {
                factory,
                token_a: token_a.clone(),
                token_b: token_b.clone(),
                fee_bps,
                tick_spacing,
            },
        );
        set_slot0(&env, &Slot0 { sqrt_price, tick, fee_protocol: 0 });
        set_liquidity(&env, 0);

        // 5. Shared instance-TTL policy (issue #390) covers Config, Slot0 and
        // Liquidity in one bump.
        coralswap_shared::extend_instance_ttl(&env);

        ConcentratedPairEvents::initialize(
            &env,
            &token_a,
            &token_b,
            sqrt_price,
            tick,
            fee_bps,
            tick_spacing,
        );

        Ok(())
    }

    // ─────────────────────────────────────────
    // Views
    // ─────────────────────────────────────────

    /// The pool's hot price state.
    pub fn get_slot0(env: Env) -> Result<Slot0, ConcentratedPairError> {
        get_slot0(&env).ok_or(ConcentratedPairError::NotInitialized)
    }

    /// Immutable pool parameters.
    pub fn get_config(env: Env) -> Result<Config, ConcentratedPairError> {
        get_config(&env).ok_or(ConcentratedPairError::NotInitialized)
    }

    /// Liquidity active at the current tick.
    pub fn get_liquidity(env: Env) -> Result<u128, ConcentratedPairError> {
        if get_config(&env).is_none() {
            return Err(ConcentratedPairError::NotInitialized);
        }
        Ok(get_liquidity(&env))
    }

    /// Bookkeeping for one tick; `None` for an uninitialized tick.
    ///
    /// Errors with [`ConcentratedPairError::InvalidTick`] outside
    /// `[MIN_TICK, MAX_TICK]` so a caller can tell "no liquidity boundary
    /// here" apart from "not a tick at all".
    pub fn get_tick(env: Env, tick: i32) -> Result<Option<TickInfo>, ConcentratedPairError> {
        if get_config(&env).is_none() {
            return Err(ConcentratedPairError::NotInitialized);
        }
        if !(tick_math::MIN_TICK..=tick_math::MAX_TICK).contains(&tick) {
            return Err(ConcentratedPairError::InvalidTick);
        }
        Ok(get_tick(&env, tick))
    }
}
