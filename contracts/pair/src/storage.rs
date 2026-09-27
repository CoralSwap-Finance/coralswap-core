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

/// Oracle bookkeeping for the pair.
///
/// The cumulative price accumulators live here, next to the observation ring
/// buffer that is derived from them, rather than on [`PairStorage`].
///
/// They used to sit on `PairStorage`, where they were written once at
/// `initialize` and never again (issue #312): the only writer,
/// `oracle::update_cumulative_prices`, is not reachable from any pair entry
/// point, so the two fields were dead storage that cost rent on every pool and
/// implied a working TWAP that did not exist. Keeping the accumulators with the
/// observations they feed makes the oracle self-contained: once #271 wires
/// `update_cumulative_prices` into `swap`/`mint`/`burn`, a single
/// `get_oracle_state` read serves both the accumulators and the buffer, and
/// there is no second copy of the same numbers to fall out of sync.
#[contracttype]
#[derive(Clone, Debug)]
pub struct OracleState {
    /// Running Uniswap-V2-style `price * elapsed` accumulator for token A.
    pub price_a_cumulative: i128,
    /// Running accumulator for token B.
    pub price_b_cumulative: i128,
    /// Ring buffer of `(ledger_sequence, cumulative_a, cumulative_b)` samples.
    pub observations: soroban_sdk::Vec<(u64, i128, i128)>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ProtocolFeeState {
    pub fee_a: i128,
    pub fee_b: i128,
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
    /// Cumulative protocol-fee accounting.
    ProtocolFeeState,
    /// Cached "is this pair's LP token paused" flag, written by
    /// `Pair::set_lp_token_paused` so the mint/burn/swap hot path can produce a
    /// typed `PairError::LpTokenPaused` without a nested call per call.
    LpTokenPaused,
}

// ---------------------------------------------------------------------------
// LP-token pause cache
// ---------------------------------------------------------------------------

/// The pair's cached view of its LP token's pause flag.
///
/// This is a *hint*, not the enforcement point. The LP token refuses
/// `mint` / `burn` while paused regardless of what this says, so a stale `false`
/// costs a worse error code, never a paused pool that keeps trading. It exists
/// because reading the flag over a contract call on every `swap`, `mint` and
/// `burn` is a nested sub-invocation per call, which is a measurable share of
/// the Soroban budget for a function as hot as `swap`.
///
/// Absent means `false`; the flag is only written by
/// [`crate::Pair::set_lp_token_paused`].
pub fn get_lp_token_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::LpTokenPaused).unwrap_or(false)
}

/// Records the LP token pause state the pair just applied.
pub fn set_lp_token_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::LpTokenPaused, &paused);
}

// ---------------------------------------------------------------------------
// OracleState helpers
// ---------------------------------------------------------------------------

pub fn get_oracle_state(env: &Env) -> OracleState {
    env.storage().instance().get(&DataKey::OracleState).unwrap_or(OracleState {
        price_a_cumulative: 0,
        price_b_cumulative: 0,
        observations: soroban_sdk::Vec::new(env),
    })
}

/// Persists the oracle bookkeeping.
///
/// Only `update_cumulative_prices` writes this today, and that is not yet
/// reached from a pair entry point (#271), so this is dead until then.
#[allow(dead_code)]
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
// Reentrancy helpers
// ---------------------------------------------------------------------------

pub fn get_reentrancy_guard(env: &Env) -> ReentrancyGuard {
    env.storage().instance().get(&DataKey::Guard).unwrap_or(ReentrancyGuard { locked: false })
}

pub fn set_reentrancy_guard(env: &Env, guard: &ReentrancyGuard) {
    env.storage().instance().set(&DataKey::Guard, guard);
}

// -----------------------------------------------------------------------
// ProtocolFeeState helpers
// -----------------------------------------------------------------------
#[allow(dead_code)]
pub fn get_protocol_fee_state(env: &Env) -> ProtocolFeeState {
    env.storage()
        .instance()
        .get(&DataKey::ProtocolFeeState)
        .unwrap_or(ProtocolFeeState { fee_a: 0, fee_b: 0 })
}

#[allow(dead_code)]
pub fn set_protocol_fee_state(env: &Env, state: &ProtocolFeeState) {
    env.storage().instance().set(&DataKey::ProtocolFeeState, state);
}
