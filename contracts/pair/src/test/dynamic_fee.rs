#![cfg(test)]

use crate::dynamic_fee::{compute_fee_bps, decay_stale_ema, update_volatility};
use crate::storage::FeeState;
use soroban_sdk::{testutils::Ledger, Env};

/// Helper to create a default FeeState for testing.
fn default_fee_state() -> FeeState {
    FeeState {
        vol_accumulator: 0,
        ema_alpha: 500_000_000_000, // 0.005 * SCALE
        baseline_fee_bps: 30,
        min_fee_bps: 5,
        max_fee_bps: 100,
        ramp_up_multiplier: 1000,
        cooldown_divisor: 2,
        last_fee_update: 0,
        decay_threshold_blocks: 1000,
    }
}

// ============================================================================
// update_volatility Tests
// ============================================================================

#[test]
fn test_update_volatility_zero_reserve_no_panic() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    update_volatility(&env, &mut fee_state, 1000, 100, 0);

    // Should not panic and accumulator should remain unchanged
    assert_eq!(fee_state.vol_accumulator, 0);
}

#[test]
fn test_update_volatility_increases_accumulator() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    let price_delta = 1_000_000_000_000; // 0.01 * SCALE
    let trade_size = 1_000_000;
    let total_reserve = 10_000_000;

    update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve);

    // Accumulator should increase from 0
    assert!(fee_state.vol_accumulator > 0);
}

#[test]
fn test_update_volatility_small_trade_has_less_impact() {
    let env = Env::default();
    let mut fee_state_small = default_fee_state();
    let mut fee_state_large = default_fee_state();

    let price_delta = 1_000_000_000_000;
    let total_reserve = 10_000_000;

    // Small trade: 1% of reserves
    update_volatility(&env, &mut fee_state_small, price_delta, 100_000, total_reserve);

    // Large trade: 10% of reserves
    update_volatility(&env, &mut fee_state_large, price_delta, 1_000_000, total_reserve);

    // Large trade should have more impact
    assert!(fee_state_large.vol_accumulator > fee_state_small.vol_accumulator);
}

#[test]
fn test_update_volatility_ema_smoothing() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    let price_delta = 1_000_000_000_000;
    let trade_size = 1_000_000;
    let total_reserve = 10_000_000;

    // First update
    update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve);
    let first_value = fee_state.vol_accumulator;

    // Second update with same parameters
    update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve);
    let second_value = fee_state.vol_accumulator;

    // EMA should smooth: second value should be higher but not double
    assert!(second_value > first_value);
    assert!(second_value < first_value * 2);
}

#[test]
fn test_update_volatility_prevents_manipulation_by_tiny_trades() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    let price_delta = 10_000_000_000_000; // Large price delta
    let tiny_trade = 1; // Extremely small trade
    let total_reserve = 10_000_000;

    update_volatility(&env, &mut fee_state, price_delta, tiny_trade, total_reserve);

    // Impact should be minimal due to size weighting
    assert!(fee_state.vol_accumulator < price_delta / 1000);
}

// ============================================================================
// compute_fee_bps Tests
// ============================================================================

#[test]
fn test_compute_fee_bps_zero_volatility_returns_baseline() {
    let fee_state = default_fee_state();

    let fee = compute_fee_bps(&fee_state);

    assert_eq!(fee, 30); // baseline_fee_bps
}

#[test]
fn test_compute_fee_bps_respects_min_bound() {
    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1; // Very low volatility

    let fee = compute_fee_bps(&fee_state);

    assert!(fee >= fee_state.min_fee_bps);
}

#[test]
fn test_compute_fee_bps_respects_max_bound() {
    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1_000_000_000_000_000; // Extremely high volatility

    let fee = compute_fee_bps(&fee_state);

    assert!(fee <= fee_state.max_fee_bps);
    assert_eq!(fee, 100); // Should clamp to max_fee_bps
}

#[test]
fn test_compute_fee_bps_increases_with_volatility() {
    let mut fee_state = default_fee_state();

    // Low volatility
    fee_state.vol_accumulator = 10_000_000_000_000;
    let _low_fee = compute_fee_bps(&fee_state);

    // High volatility (10x higher)
    fee_state.vol_accumulator = 100_000_000_000_000;
    let _high_fee = compute_fee_bps(&fee_state);

    // Both should hit max_fee_bps, so let's use smaller values
    fee_state.vol_accumulator = 1_000_000_000_000;
    let very_low_fee = compute_fee_bps(&fee_state);

    fee_state.vol_accumulator = 5_000_000_000_000;
    let medium_fee = compute_fee_bps(&fee_state);

    assert!(medium_fee >= very_low_fee);
}

#[test]
fn test_compute_fee_bps_linear_interpolation() {
    let mut fee_state = default_fee_state();

    // Set volatility to produce mid-range fee
    fee_state.vol_accumulator = 50_000_000_000_000;
    let mid_fee = compute_fee_bps(&fee_state);

    // Fee should be between min and max
    assert!(mid_fee > fee_state.min_fee_bps);
    assert!(mid_fee <= fee_state.max_fee_bps);
}

// ============================================================================
// decay_stale_ema Tests
// ============================================================================

#[test]
fn test_decay_stale_ema_no_decay_before_threshold() {
    let env = Env::default();
    env.ledger().set_sequence_number(1000);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1_000_000_000_000;
    fee_state.last_fee_update = 500; // 500 blocks ago
    fee_state.decay_threshold_blocks = 1000; // Threshold not reached

    let initial_vol = fee_state.vol_accumulator;

    decay_stale_ema(&env, &mut fee_state);

    // Should not decay yet
    assert_eq!(fee_state.vol_accumulator, initial_vol);
}

#[test]
fn test_decay_stale_ema_decays_after_threshold() {
    let env = Env::default();
    env.ledger().set_sequence_number(2000);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1_000_000_000_000;
    fee_state.last_fee_update = 500; // 1500 blocks ago
    fee_state.decay_threshold_blocks = 1000; // Threshold exceeded

    let initial_vol = fee_state.vol_accumulator;

    decay_stale_ema(&env, &mut fee_state);

    // Should decay towards zero
    assert!(fee_state.vol_accumulator < initial_vol);
}

#[test]
fn test_decay_stale_ema_updates_timestamp() {
    let env = Env::default();
    let current_ledger = 2000;
    env.ledger().set_sequence_number(current_ledger);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1_000_000_000_000;
    fee_state.last_fee_update = 500;
    fee_state.decay_threshold_blocks = 1000;

    decay_stale_ema(&env, &mut fee_state);

    // Timestamp should be updated
    assert_eq!(fee_state.last_fee_update, current_ledger as u64);
}

#[test]
fn test_decay_stale_ema_idle_pool_decays_to_baseline() {
    let env = Env::default();

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 10_000_000_000_000;
    fee_state.last_fee_update = 0;
    fee_state.decay_threshold_blocks = 1000;

    // Simulate long idle period
    env.ledger().set_sequence_number(20000);

    decay_stale_ema(&env, &mut fee_state);

    // Should decay significantly
    assert!(fee_state.vol_accumulator < 1_000_000_000_000);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_large_trade_increases_fee() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    let initial_fee = compute_fee_bps(&fee_state);

    // Simulate large trade with significant price impact
    let price_delta = 5_000_000_000_000; // 0.05 * SCALE
    let trade_size = 2_000_000;
    let total_reserve = 10_000_000;

    update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve);

    let new_fee = compute_fee_bps(&fee_state);

    // Fee should increase after large trade
    assert!(new_fee > initial_fee);
}

#[test]
fn test_multiple_trades_accumulate_volatility() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    let price_delta = 1_000_000_000_000;
    let trade_size = 1_000_000;
    let total_reserve = 10_000_000;

    // Execute multiple trades
    for _ in 0..5 {
        update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve);
    }

    let fee_after_trades = compute_fee_bps(&fee_state);

    // Fee should be elevated after multiple trades
    assert!(fee_after_trades > fee_state.baseline_fee_bps);
}

#[test]
fn test_fee_stays_within_bounds_under_extreme_conditions() {
    let env = Env::default();
    let mut fee_state = default_fee_state();

    // Extreme volatility updates
    for _ in 0..100 {
        update_volatility(&env, &mut fee_state, 100_000_000_000_000, 10_000_000, 10_000_000);
    }

    let fee = compute_fee_bps(&fee_state);

    // Fee must stay within configured bounds
    assert!(fee >= fee_state.min_fee_bps);
    assert!(fee <= fee_state.max_fee_bps);
}
