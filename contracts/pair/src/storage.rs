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
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ReentrancyGuard {
    pub locked: bool,
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

// ---------------------------------------------------------------------------
// Reentrancy helpers
// ---------------------------------------------------------------------------

pub fn get_reentrancy_guard(env: &Env) -> ReentrancyGuard {
    env.storage()
        .instance()
        .get(&DataKey::Guard)
        .unwrap_or(ReentrancyGuard { locked: false })
}

pub fn set_reentrancy_guard(env: &Env, guard: &ReentrancyGuard) {
    env.storage().instance().set(&DataKey::Guard, guard);
}
