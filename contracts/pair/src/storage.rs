use soroban_sdk::{contracttype, Address, Env};
use crate::errors::PairError;

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub enum StorageKey {
    Pair,
    Fee,
    Reentrancy,
}

// ── Data structs ──────────────────────────────────────────────────────────────

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

// ── PairStorage helpers ───────────────────────────────────────────────────────

pub fn get_pair_storage(env: &Env) -> Result<PairStorage, PairError> {
    env.storage()
        .instance()
        .get(&StorageKey::Pair)
        .ok_or(PairError::NotInitialized)
}

pub fn set_pair_storage(env: &Env, state: &PairStorage) {
    env.storage().instance().set(&StorageKey::Pair, state);
}

// ── FeeState helpers ──────────────────────────────────────────────────────────

pub fn get_fee_state(env: &Env) -> FeeState {
    env.storage()
        .instance()
        .get(&StorageKey::Fee)
        .unwrap_or(FeeState {
            vol_accumulator: 0,
            ema_alpha: 500_000_000_000, // 0.005 * 1e14 ≈ conservative alpha
            baseline_fee_bps: 30,
            min_fee_bps: 5,
            max_fee_bps: 100,
            ramp_up_multiplier: 3,
            cooldown_divisor: 2,
            last_fee_update: env.ledger().sequence() as u64,
            decay_threshold_blocks: 120, // ~10 mins at 5s blocks
        })
}

pub fn set_fee_state(env: &Env, fee_state: &FeeState) {
    env.storage().instance().set(&StorageKey::Fee, fee_state);
}

// ── ReentrancyGuard helpers ───────────────────────────────────────────────────

pub fn get_reentrancy_guard(env: &Env) -> ReentrancyGuard {
    env.storage()
        .instance()
        .get(&StorageKey::Reentrancy)
        .unwrap_or(ReentrancyGuard { locked: false })
}

pub fn set_reentrancy_guard(env: &Env, guard: &ReentrancyGuard) {
    env.storage().instance().set(&StorageKey::Reentrancy, guard);
}
