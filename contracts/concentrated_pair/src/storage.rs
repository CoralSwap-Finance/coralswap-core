//! Storage layout for the concentrated pair.
//!
//! # Key layout is the optimization (issue #196 acceptance criterion)
//!
//! Soroban charges per storage *entry* touched, so the layout splits state by
//! access pattern instead of mirroring Uniswap V3's slot packing:
//!
//! - **Instance storage** carries what every invocation needs anyway
//!   ([`Config`], [`Slot0`], the active [`DataKey::Liquidity`]): instance
//!   entries ride along with the contract instance that is already loaded to
//!   run the invocation, so reading them adds no extra entry access, and
//!   they all share one TTL that [`crate::ConcentratedPair`] bumps on write.
//! - **Each initialized tick is its own persistent entry**
//!   ([`DataKey::Tick`]). A swap pays only for the ticks it actually
//!   crosses; a `Map<i32, TickInfo>` under a single key would instead be
//!   read, deserialized and re-written whole on every touch, and its rent
//!   would grow with the number of LPs rather than with use. An absent key
//!   *is* the "uninitialized tick" state — no `initialized` flag is stored.

use soroban_sdk::{contracttype, Address, Env};

use crate::errors::ConcentratedPairError;

/// The pool's hot price state — the fields every swap reads and writes
/// together, kept in one instance entry (the "slot0" of Uniswap V3).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Slot0 {
    /// Current √price in Q64.64 (token B per token A), within
    /// `[tick_math::MIN_SQRT_RATIO, tick_math::MAX_SQRT_RATIO)`.
    pub sqrt_price: u128,
    /// Greatest tick whose √price is ≤ `sqrt_price`; always
    /// `tick_at_sqrt_ratio(sqrt_price)`.
    pub tick: i32,
    /// Protocol's share of swap fees, expressed as a denominator
    /// (`0` = off, otherwise `1/fee_protocol` of fees accrue to the
    /// protocol). Set to `0` at initialization; governance wiring lands with
    /// the swap path.
    pub fee_protocol: u32,
}

/// Immutable pool parameters fixed at `initialize`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub factory: Address,
    pub token_a: Address,
    pub token_b: Address,
    /// LP swap fee in basis points (1 bps = 0.01%).
    pub fee_bps: u32,
    /// Granularity of usable ticks: positions may only be minted on ticks
    /// divisible by this.
    pub tick_spacing: u32,
}

/// Liquidity bookkeeping for one initialized tick.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TickInfo {
    /// Total position liquidity referencing this tick as a range boundary.
    /// Tracks when the entry can be reclaimed: the tick is deleted when this
    /// returns to zero.
    pub liquidity_gross: u128,
    /// Net liquidity added to the pool when the tick is crossed left→right
    /// (price increasing); subtracted right→left.
    pub liquidity_net: i128,
}

/// Storage keys for all contract state.
#[contracttype]
pub enum DataKey {
    /// Immutable pool parameters ([`Config`]; instance).
    Config,
    /// Hot price state ([`Slot0`]; instance).
    Slot0,
    /// Liquidity active at the current tick (`u128`; instance).
    Liquidity,
    /// Per-tick liquidity bookkeeping ([`TickInfo`]; persistent, one entry
    /// per initialized tick). Absent ⇒ the tick is uninitialized.
    Tick(i32),
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

pub fn get_config(env: &Env) -> Option<Config> {
    env.storage().instance().get(&DataKey::Config)
}

pub fn set_config(env: &Env, config: &Config) {
    env.storage().instance().set(&DataKey::Config, config);
}

// ---------------------------------------------------------------------------
// Slot0 helpers
// ---------------------------------------------------------------------------

pub fn get_slot0(env: &Env) -> Option<Slot0> {
    env.storage().instance().get(&DataKey::Slot0)
}

pub fn set_slot0(env: &Env, slot0: &Slot0) {
    env.storage().instance().set(&DataKey::Slot0, slot0);
}

// ---------------------------------------------------------------------------
// Active-liquidity helpers
// ---------------------------------------------------------------------------

pub fn get_liquidity(env: &Env) -> u128 {
    env.storage().instance().get(&DataKey::Liquidity).unwrap_or(0)
}

pub fn set_liquidity(env: &Env, liquidity: u128) {
    env.storage().instance().set(&DataKey::Liquidity, &liquidity);
}

// ---------------------------------------------------------------------------
// Per-tick helpers
// ---------------------------------------------------------------------------

/// Returns the tick's bookkeeping, or `None` for an uninitialized tick.
pub fn get_tick(env: &Env, tick: i32) -> Option<TickInfo> {
    env.storage().persistent().get(&DataKey::Tick(tick))
}

/// Writes a tick entry, reclaiming the key once no position references it.
///
/// Tick entries must outlive idle periods just like LP balances — an expired
/// tick would silently corrupt crossing math — so they share the LP
/// persistent TTL policy from `coralswap-shared`.
// `dead_code`: the writer is wired in by the position mint/burn follow-up to
// issue #196; until then it is exercised by tests only.
#[allow(dead_code)]
pub fn set_tick(env: &Env, tick: i32, info: &TickInfo) -> Result<(), ConcentratedPairError> {
    // A tick referenced by no position must not linger as a rent-bearing
    // entry, and liquidity_net without liquidity_gross is a bookkeeping bug.
    if info.liquidity_gross == 0 && info.liquidity_net != 0 {
        return Err(ConcentratedPairError::InvalidInput);
    }
    let key = DataKey::Tick(tick);
    if info.liquidity_gross == 0 {
        env.storage().persistent().remove(&key);
    } else {
        env.storage().persistent().set(&key, info);
        env.storage().persistent().extend_ttl(
            &key,
            coralswap_shared::LP_PERSISTENT_THRESHOLD,
            coralswap_shared::LP_PERSISTENT_EXTEND_TO,
        );
    }
    Ok(())
}
