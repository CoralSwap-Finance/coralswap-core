use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub struct PairStorage {
    pub factory: Address,
    pub token_a: Address,
    pub token_b: Address,
    pub lp_token: Address,
    pub reserve_a: i128,
    pub reserve_b: i128,
    pub block_timestamp_last: u64,
    pub price_a_cumulative: i128,
    pub price_b_cumulative: i128,
    pub k_last: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FeeState {
    pub vol_accumulator: i128,
    pub ema_alpha: i128,
    pub baseline_fee_bps: u32,
    pub min_fee_bps: u32,
    pub max_fee_bps: u32,
    pub ramp_up_multiplier: u32,
    pub cooldown_divisor: u32,
    pub last_fee_update: u64,
    pub decay_threshold_blocks: u64,
    /// Configurable staleness threshold in ledgers.
    ///
    /// The EMA volatility accumulator begins time-based exponential decay
    /// when the pool has been idle (no trades) for this many ledgers.
    /// This prevents idle pools from charging inflated fees.
    ///
    /// # Valid Range
    /// - Minimum: 1 ledger
    /// - Maximum: 100,000 ledgers (~11.6 days at 5s/ledger)
    /// - Default: 100 ledgers (~8.3 minutes)
    ///
    /// This threshold can only be updated by calling `Pair::set_stale_threshold()`
    /// with factory admin authorization.
    pub stale_threshold: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ReentrancyGuard {
    pub locked: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct OracleState {
    pub observations: soroban_sdk::Vec<(u64, i128, i128)>,
}

/// Per-pair configuration that is set once during initialization.
/// Includes optional price feed addresses for RWA (yield-bearing) tokens.
/// When a price feed is set, reserve amounts are normalized by the feed price
/// during liquidity math (NAV-normalized reserves).
#[contracttype]
#[derive(Clone, Debug)]
pub struct PairConfig {
    /// Fee in basis points (set by factory during creation).
    pub fee_bps: u32,
    /// Optional price feed address for token_0 (RWA NAV oracle).
    pub price_feed_0: Option<Address>,
    /// Optional price feed address for token_1 (RWA NAV oracle).
    pub price_feed_1: Option<Address>,
    /// Whether the pair is paused (set by factory governance).
    pub is_paused: bool,
}

/// Storage keys for all persistent contract state.
#[contracttype]
pub enum DataKey {
    /// Core pair configuration and reserve state.
    PairState,
    /// Dynamic fee EMA accumulator state.
    FeeState,
    /// Reentrancy lock for flash loan guard.
    Guard,
    /// Oracle ring buffer.
    OracleState,
    /// Per-pair configuration (fee_bps, price feeds, pause state).
    PairConfig,
}

// ---------------------------------------------------------------------------
// OracleState helpers
// ---------------------------------------------------------------------------

pub fn get_oracle_state(env: &Env) -> OracleState {
    env.storage()
        .instance()
        .get(&DataKey::OracleState)
        .unwrap_or(OracleState { observations: soroban_sdk::Vec::new(env) })
}

pub fn set_oracle_state(env: &Env, state: &OracleState) {
    env.storage().instance().set(&DataKey::OracleState, state);
}

// ---------------------------------------------------------------------------
// PairStorage helpers
// ---------------------------------------------------------------------------

pub fn get_pair_state(env: &Env) -> Option<PairStorage> {
    env.storage().instance().get(&DataKey::PairState)
}

pub fn set_pair_state(env: &Env, state: &PairStorage) {
    env.storage().instance().set(&DataKey::PairState, state);
}

// ---------------------------------------------------------------------------
// FeeState helpers
// ---------------------------------------------------------------------------

pub fn get_fee_state(env: &Env) -> Option<FeeState> {
    env.storage().instance().get(&DataKey::FeeState)
}

pub fn set_fee_state(env: &Env, state: &FeeState) {
    env.storage().instance().set(&DataKey::FeeState, state);
}

// ---------------------------------------------------------------------------
// PairConfig helpers
// ---------------------------------------------------------------------------

/// Returns the pair's configuration (fee_bps, price feeds, pause state),
/// or a sensible default if not yet set (post-migration compat).
pub fn get_pair_config(env: &Env) -> PairConfig {
    env.storage()
        .instance()
        .get(&DataKey::PairConfig)
        .unwrap_or(PairConfig {
            fee_bps: 0,
            price_feed_0: None,
            price_feed_1: None,
            is_paused: false,
        })
}

/// Stores the pair configuration.
pub fn set_pair_config(env: &Env, config: &PairConfig) {
    env.storage().instance().set(&DataKey::PairConfig, config);
}

/// Looks up the price feed address for a given token, returning `None` if the
/// token is not one of the pair's tokens or if no price feed is configured.
///
/// Price feeds are used to normalize reserves for RWA (yield-bearing) tokens
/// whose NAV grows over time. A standard x*y=k pool with a yield-bearing
/// token drifts out of balance as NAV accrues. An optional price feed enables
/// NAV-normalized reserve math.
pub fn get_price_feed(env: &Env, token: &Address, state: &PairStorage) -> Option<Address> {
    let config = get_pair_config(env);
    if token == &state.token_a {
        config.price_feed_0
    } else if token == &state.token_b {
        config.price_feed_1
    } else {
        None
    }
}

/// Returns the price feed address for token_0, if configured.
pub fn get_price_feed_0(env: &Env) -> Option<Address> {
    get_pair_config(env).price_feed_0
}

/// Returns the price feed address for token_1, if configured.
pub fn get_price_feed_1(env: &Env) -> Option<Address> {
    get_pair_config(env).price_feed_1
}

// ---------------------------------------------------------------------------
// Reentrancy helpers
// ---------------------------------------------------------------------------

pub fn get_reentrancy_guard(env: &Env) -> ReentrancyGuard {
    env.storage().instance().get(&DataKey::Guard).unwrap_or(ReentrancyGuard { locked: false })
}

pub fn set_reentrancy_guard(env: &Env, guard: &ReentrancyGuard) {
    env.storage().instance().set(&DataKey::Guard, guard);
}
