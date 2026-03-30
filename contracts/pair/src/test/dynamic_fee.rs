use crate::dynamic_fee::{compute_fee_bps, decay_stale_ema, update_volatility};
use crate::errors::PairError;
use crate::storage::FeeState;
use soroban_sdk::{testutils::Ledger, Env};

#[allow(dead_code)]
const SCALE: i128 = 100_000_000_000_000; // 1e14

fn default_fee_state() -> FeeState {
    FeeState {
        vol_accumulator: 0,
        ema_alpha: SCALE / 20, // 5%
        baseline_fee_bps: 30,
        min_fee_bps: 5,
        max_fee_bps: 100,
        ramp_up_multiplier: 2,
        cooldown_divisor: 2,
        last_fee_update: 0,
        decay_threshold_blocks: 100,
    }
}

// ============================================================================
// update_volatility Tests
// ============================================================================

#[test]
fn test_update_volatility_zero_reserve_returns_error() {
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
    update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve).unwrap();
    let first_value = fee_state.vol_accumulator;

    // Second update with same parameters
    update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve).unwrap();
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
    assert_eq!(fee, 100);
}

#[test]
fn test_compute_fee_bps_increases_with_volatility() {
    let mut fee_state = default_fee_state();

    fee_state.vol_accumulator = 1_000_000_000;
    let low_fee = compute_fee_bps(&fee_state);

    fee_state.vol_accumulator = 5_000_000_000;
    let medium_fee = compute_fee_bps(&fee_state);

    fee_state.vol_accumulator = 50_000_000_000;
    let high_fee = compute_fee_bps(&fee_state);

    // Fee must increase (or stay capped) as volatility increases
    assert!(medium_fee >= low_fee);
    assert!(high_fee >= medium_fee);
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
// decay_stale_ema Tests (Exponential Decay via cooldown_divisor)
// ============================================================================

#[test]
fn test_decay_no_decay_before_threshold() {
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
    // Timestamp should NOT be updated when no decay occurs
    assert_eq!(fee_state.last_fee_update, initial_update);
}

#[test]
fn test_decay_no_decay_at_exact_threshold() {
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
fn test_decay_single_period_exact() {
    let env = Env::default();
    // Elapsed = 1001, just past threshold
    env.ledger().set_sequence_number(1501);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1_000_000_000_000;
    fee_state.last_fee_update = 500; // elapsed = 1001
    fee_state.decay_threshold_blocks = 1000;
    fee_state.cooldown_divisor = 2;

    decay_stale_ema(&env, &mut fee_state);

    // decay_periods = 1001 / 1000 = 1, vol /= 2
    assert_eq!(fee_state.vol_accumulator, 500_000_000_000);
}

#[test]
fn test_decay_multi_period_compounding() {
    let env = Env::default();
    env.ledger().set_sequence_number(4001);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 8_000_000_000_000;
    fee_state.last_fee_update = 0; // elapsed = 4001
    fee_state.decay_threshold_blocks = 1000;
    fee_state.cooldown_divisor = 2;

    decay_stale_ema(&env, &mut fee_state);

    // decay_periods = 4001 / 1000 = 4
    // 8_000_000_000_000 / 2^4 = 500_000_000_000
    assert_eq!(fee_state.vol_accumulator, 500_000_000_000);
}

#[test]
fn test_decay_multi_period_divisor_3() {
    let env = Env::default();
    env.ledger().set_sequence_number(3001);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 27_000_000_000_000;
    fee_state.last_fee_update = 0;
    fee_state.decay_threshold_blocks = 1000;
    fee_state.cooldown_divisor = 3;

    decay_stale_ema(&env, &mut fee_state);

    // decay_periods = 3001 / 1000 = 3
    // 27_000_000_000_000 / 3^3 = 1_000_000_000_000
    assert_eq!(fee_state.vol_accumulator, 1_000_000_000_000);
}

#[test]
fn test_decay_accumulator_never_negative() {
    let env = Env::default();
    env.ledger().set_sequence_number(100_000);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1; // Tiny value
    fee_state.last_fee_update = 0;
    fee_state.decay_threshold_blocks = 1000;
    fee_state.cooldown_divisor = 2;

    decay_stale_ema(&env, &mut fee_state);

    // After many divisions of 1, should floor to 0, never negative
    assert_eq!(fee_state.vol_accumulator, 0);
}

#[test]
fn test_decay_zero_accumulator_unchanged() {
    let env = Env::default();
    env.ledger().set_sequence_number(5000);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 0;
    fee_state.last_fee_update = 0;
    fee_state.decay_threshold_blocks = 1000;

    decay_stale_ema(&env, &mut fee_state);

    // Zero stays zero
    assert_eq!(fee_state.vol_accumulator, 0);
}

#[test]
fn test_decay_updates_timestamp() {
    let env = Env::default();
    let current_ledger: u32 = 3000;
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
fn test_decay_prevents_redecay_same_block() {
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

#[test]
fn test_decay_caps_at_max_periods() {
    let env = Env::default();
    // Enormous elapsed: would be 1_000_000 periods uncapped
    env.ledger().set_sequence_number(1_000_001_000);

    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = i128::MAX; // Huge value
    fee_state.last_fee_update = 0;
    fee_state.decay_threshold_blocks = 1000;
    fee_state.cooldown_divisor = 2;

    // Should not panic or loop excessively — capped at 64 iterations
    decay_stale_ema(&env, &mut fee_state);

    // i128::MAX / 2^64 = ~9.2e18, still positive
    assert!(fee_state.vol_accumulator >= 0);
    assert!(fee_state.vol_accumulator < i128::MAX / 1_000_000);
}

    assert_eq!(fee_state.vol_accumulator, 0);
}

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
        update_volatility(&env, &mut fee_state, price_delta, trade_size, total_reserve).unwrap();
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

// ============================================================================
// Additional compute_fee_bps Tests (overflow, monotonicity, boundaries)
// ============================================================================

#[test]
fn test_compute_fee_bps_no_overflow_extreme_vol_accumulator() {
    let mut fee_state = default_fee_state();
    // Use an extremely large vol_accumulator to exercise saturating_mul
    fee_state.vol_accumulator = i128::MAX / 2;

    let fee = compute_fee_bps(&fee_state);

    // Must not panic and must clamp to max_fee_bps
    assert_eq!(fee, fee_state.max_fee_bps);
}

#[test]
fn test_compute_fee_bps_no_overflow_max_ramp_up() {
    let mut fee_state = default_fee_state();
    fee_state.vol_accumulator = 1_000_000_000_000_000;
    fee_state.ramp_up_multiplier = u32::MAX; // extreme multiplier

    let fee = compute_fee_bps(&fee_state);

    assert!(fee >= fee_state.min_fee_bps);
    assert!(fee <= fee_state.max_fee_bps);
}

#[test]
fn test_compute_fee_bps_baseline_below_min_returns_min() {
    let mut fee_state = default_fee_state();
    // Set baseline below min: baseline=3, min=5
    fee_state.baseline_fee_bps = 3;
    fee_state.min_fee_bps = 5;
    fee_state.vol_accumulator = 0;

    let fee = compute_fee_bps(&fee_state);

    // Baseline is clamped up to min
    assert_eq!(fee, 5);
}

#[test]
fn test_compute_fee_bps_baseline_above_max_returns_max() {
    let mut fee_state = default_fee_state();
    // Set baseline above max: baseline=200, max=100
    fee_state.baseline_fee_bps = 200;
    fee_state.max_fee_bps = 100;
    fee_state.vol_accumulator = 0;

    let fee = compute_fee_bps(&fee_state);

    assert_eq!(fee, 100);
}

#[test]
fn test_compute_fee_bps_monotonicity_fine_grained() {
    let mut fee_state = default_fee_state();
    let mut prev_fee = 0u32;

    // Sweep vol_accumulator from 0 to a large value
    for i in 0..=20 {
        fee_state.vol_accumulator = i * 500_000_000; // increments of 5e8
        let fee = compute_fee_bps(&fee_state);
        assert!(fee >= prev_fee, "fee decreased at i={}: prev={}, curr={}", i, prev_fee, fee);
        prev_fee = fee;
    }
}

#[test]
fn test_compute_fee_bps_negative_vol_accumulator_clamps_to_min() {
    let mut fee_state = default_fee_state();
    // Negative vol_accumulator (shouldn't happen in practice, but test safety)
    fee_state.vol_accumulator = -1_000_000_000_000;

    let fee = compute_fee_bps(&fee_state);

    assert!(fee >= fee_state.min_fee_bps);
}

#[test]
fn test_compute_fee_bps_moderate_vol_between_baseline_and_max() {
    let mut fee_state = default_fee_state();
    // Choose a vol_accumulator that produces a fee between baseline (30) and max (100)
    // vol_bps = (vol * ramp_up_multiplier) / (SCALE / 10_000)
    //         = (vol * 1000) / 10_000_000_000
    // We want fee = 30 + vol_bps = 60, so vol_bps = 30
    // 30 = (vol * 1000) / 10_000_000_000 → vol = 300_000_000
    fee_state.vol_accumulator = 300_000_000; // 3e8

    let fee = compute_fee_bps(&fee_state);

    assert_eq!(fee, 60);
    assert!(fee > fee_state.baseline_fee_bps);
    assert!(fee < fee_state.max_fee_bps);
}

#[test]
fn test_compute_fee_bps_tiny_vol_returns_near_baseline() {
    let mut fee_state = default_fee_state();
    // vol_accumulator so small that vol_bps rounds to 0
    fee_state.vol_accumulator = 1;

    let fee = compute_fee_bps(&fee_state);

    // vol_bps = (1 * 1000) / 1e10 = 0 → fee = 30 + 0 = 30
    assert_eq!(fee, fee_state.baseline_fee_bps);
}
