use crate::errors::PairError;
use crate::fee_decay::apply_time_decay;
use crate::storage::FeeState;
use soroban_sdk::Env;

/// Fixed-point scale factor (1e14) — must match `math::SCALE`.
const SCALE: i128 = 100_000_000_000_000;

/// Updates the EMA volatility accumulator with a new price observation.
///
/// Uses size-weighted EMA so that large trades move the accumulator more than
/// small (dust) trades, making fee manipulation expensive.
///
/// # Math (all fixed-point with SCALE = 1e14)
///
/// ```text
/// weight      = trade_size * SCALE / total_reserve   (∈ [0, SCALE])
/// observation = price_delta_abs * weight / SCALE
/// new_accum   = (alpha * observation + (SCALE - alpha) * old_accum) / SCALE
/// ```
#[allow(dead_code)]
pub fn update_volatility(
    env: &Env,
    fee_state: &mut FeeState,
    price_delta_abs: i128,
    trade_size: i128,
    total_reserve: i128,
) -> Result<(), PairError> {
    // --- Input validation ---------------------------------------------------
    if price_delta_abs < 0 || trade_size <= 0 || total_reserve <= 0 {
        return Err(PairError::InvalidInput);
    }

    // EMA alpha must be in [0, SCALE] to ensure the weight split is valid.
    if fee_state.ema_alpha < 0 || fee_state.ema_alpha > SCALE {
        return Err(PairError::InvalidEmaAlpha);
    }

    // --- Size-weighted observation ------------------------------------------
    // weight = trade_size * SCALE / total_reserve
    let weight = trade_size
        .checked_mul(SCALE)
        .ok_or(PairError::Overflow)?
        .checked_div(total_reserve)
        .ok_or(PairError::Overflow)?;

    // observation = price_delta_abs * weight / SCALE
    let observation = price_delta_abs
        .checked_mul(weight)
        .ok_or(PairError::Overflow)?
        .checked_div(SCALE)
        .ok_or(PairError::Overflow)?;

    // --- EMA update ---------------------------------------------------------
    // alpha_term = ema_alpha * observation
    let alpha_term = fee_state.ema_alpha.checked_mul(observation).ok_or(PairError::Overflow)?;

    // prev_term = (SCALE - ema_alpha) * vol_accumulator
    let complement = SCALE.checked_sub(fee_state.ema_alpha).ok_or(PairError::Overflow)?;
    let prev_term = complement.checked_mul(fee_state.vol_accumulator).ok_or(PairError::Overflow)?;

    // new_accumulator = (alpha_term + prev_term) / SCALE
    fee_state.vol_accumulator = alpha_term
        .checked_add(prev_term)
        .ok_or(PairError::Overflow)?
        .checked_div(SCALE)
        .ok_or(PairError::Overflow)?;

    // --- Cap accumulator to prevent unbounded growth -------------------------
    // Compute the accumulator value that produces max_fee_bps and clamp to it.
    // This ensures the dynamic fee can recover normally via time decay instead
    // of being pegged at maximum indefinitely after a griefing attack.
    if fee_state.ramp_up_multiplier > 0 {
        let scale_to_bps = SCALE / 10_000;
        let fee_headroom =
            (fee_state.max_fee_bps as i128).saturating_sub(fee_state.baseline_fee_bps as i128);
        let max_vol =
            fee_headroom.saturating_mul(scale_to_bps) / (fee_state.ramp_up_multiplier as i128);
        fee_state.vol_accumulator = fee_state.vol_accumulator.min(max_vol);
    }

    // --- Timestamp ----------------------------------------------------------
    fee_state.last_fee_update = env.ledger().timestamp();

    Ok(())
}

/// Computes the current fee in basis points from the EMA state.
///
/// Formula: `fee = baseline_fee_bps + (vol_accumulator * ramp_up_multiplier) / (SCALE / 10_000)`
///
/// The result is clamped to `[min_fee_bps, max_fee_bps]`.
///
/// - Zero volatility returns `baseline_fee_bps` (clamped to bounds).
/// - Low volatility yields a fee slightly above baseline.
/// - High volatility pushes the fee towards `max_fee_bps`.
/// - Overflow-safe via saturating arithmetic.
pub fn compute_fee_bps(fee_state: &FeeState) -> u32 {
    // If volatility accumulator is zero, fall back to baseline.
    if fee_state.vol_accumulator == 0 {
        return fee_state.baseline_fee_bps.clamp(fee_state.min_fee_bps, fee_state.max_fee_bps);
    }

    // Normalize vol_accumulator into a bps contribution.
    // vol_accumulator lives in SCALE space (1e14).
    // scale_to_bps = SCALE / 10_000 = 1e10
    let scale_to_bps = SCALE / 10_000;

    let vol_bps = fee_state.vol_accumulator.saturating_mul(fee_state.ramp_up_multiplier as i128)
        / scale_to_bps;

    // Linear interpolation: fee = baseline + dynamic volatility component.
    let fee = (fee_state.baseline_fee_bps as i128).saturating_add(vol_bps);

    // Clamp to configured bounds.
    fee.clamp(fee_state.min_fee_bps as i128, fee_state.max_fee_bps as i128) as u32
}

/// Decays the volatility accumulator if the pool has been idle.
pub fn decay_stale_ema(env: &Env, fee_state: &mut FeeState) {
    let current_ledger = env.ledger().sequence() as u64;

    if current_ledger > fee_state.last_fee_update + fee_state.decay_threshold_blocks {
        apply_time_decay(env, fee_state, current_ledger);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    /// Helper: build a default FeeState for testing.
    fn default_fee_state(alpha: i128) -> FeeState {
        FeeState {
            vol_accumulator: 0,
            ema_alpha: alpha,
            baseline_fee_bps: 30,
            min_fee_bps: 5,
            max_fee_bps: 100,
            ramp_up_multiplier: 2,
            cooldown_divisor: 2,
            last_fee_update: 0,
            decay_threshold_blocks: 100,
        }
    }

    // ------ Size weighting ---------------------------------------------------

    #[test]
    fn large_trade_produces_larger_update_than_small_trade() {
        let env = Env::default();
        let alpha = SCALE / 10; // 10 %
        let total_reserve = 1_000_000;
        let price_delta = 500;

        // Small trade
        let mut state_small = default_fee_state(alpha);
        update_volatility(&env, &mut state_small, price_delta, 1_000, total_reserve).unwrap();

        // Large trade (100× bigger)
        let mut state_large = default_fee_state(alpha);
        update_volatility(&env, &mut state_large, price_delta, 100_000, total_reserve).unwrap();

        assert!(
            state_large.vol_accumulator > state_small.vol_accumulator,
            "large trade ({}) must move accumulator more than small trade ({})",
            state_large.vol_accumulator,
            state_small.vol_accumulator,
        );
    }

    // ------ EMA smoothing ----------------------------------------------------

    #[test]
    fn ema_smooths_observations_towards_steady_state() {
        let env = Env::default();
        let alpha = SCALE / 10; // 10 %
        let total_reserve = 1_000_000;
        let trade_size = 100_000; // 10 % of reserve
        let price_delta = 1_000;
        let mut state = default_fee_state(alpha);

        // Run 200 identical observations — accumulator must converge.
        // With alpha=10%, convergence needs ~ln(0.01)/ln(0.9) ≈ 44 steps for 99%,
        // but integer rounding slows it, so we use 200 to be safe.
        let mut prev = 0i128;
        for _ in 0..200 {
            update_volatility(&env, &mut state, price_delta, trade_size, total_reserve).unwrap();
            // Each step should move closer (or equal) to steady state.
            assert!(
                state.vol_accumulator >= prev,
                "accumulator should be non-decreasing under constant positive input"
            );
            prev = state.vol_accumulator;
        }

        // Steady-state theoretical value:
        //   observation = price_delta * (trade_size / total_reserve) = 1000 * 0.1 = 100
        // EMA converges to observation = 100.
        // With SCALE = 1e14 the accumulator stores the value in unscaled i128 form.
        // Due to integer truncation in fixed-point division, the steady-state
        // value is slightly below the theoretical 100. The EMA converges to
        // a floor caused by rounding. We verify it's within 10% of theoretical.
        let theoretical = 100i128;
        assert!(
            state.vol_accumulator > theoretical * 9 / 10 && state.vol_accumulator <= theoretical,
            "accumulator {} should converge to ~{} (within 10%)",
            state.vol_accumulator,
            theoretical,
        );
    }

    #[test]
    fn alpha_controls_responsiveness() {
        let env = Env::default();
        let total_reserve = 1_000_000;
        let trade_size = 100_000;
        let price_delta = 1_000;

        // Fast alpha (50 %)
        let mut fast = default_fee_state(SCALE / 2);
        // Slow alpha (5 %)
        let mut slow = default_fee_state(SCALE / 20);

        // After one observation, fast alpha should react more.
        update_volatility(&env, &mut fast, price_delta, trade_size, total_reserve).unwrap();
        update_volatility(&env, &mut slow, price_delta, trade_size, total_reserve).unwrap();

        assert!(
            fast.vol_accumulator > slow.vol_accumulator,
            "fast alpha ({}) should yield larger first update than slow alpha ({})",
            fast.vol_accumulator,
            slow.vol_accumulator,
        );
    }

    // ------ Zero price delta -------------------------------------------------

    #[test]
    fn zero_price_delta_does_not_increase_volatility() {
        let env = Env::default();
        let alpha = SCALE / 10;
        let mut state = default_fee_state(alpha);
        // Seed with some existing volatility.
        state.vol_accumulator = 500;

        update_volatility(&env, &mut state, 0, 100_000, 1_000_000).unwrap();

        // With zero delta the observation is 0 and the EMA decays.
        assert!(
            state.vol_accumulator <= 500,
            "accumulator should not increase on zero delta, got {}",
            state.vol_accumulator,
        );
    }

    // ------ Timestamp --------------------------------------------------------

    #[test]
    fn timestamp_is_updated_after_call() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE / 10);
        assert_eq!(state.last_fee_update, 0);

        update_volatility(&env, &mut state, 100, 1_000, 1_000_000).unwrap();

        // Soroban default test env ledger timestamp is 0, but the field must
        // equal whatever the ledger reports.
        assert_eq!(state.last_fee_update, env.ledger().timestamp());
    }

    // ------ Overflow safety --------------------------------------------------

    #[test]
    fn overflow_on_huge_price_delta_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE / 10);

        // price_delta near i128::MAX should overflow in checked_mul.
        let result = update_volatility(&env, &mut state, i128::MAX / 2, i128::MAX / 2, 1);
        assert_eq!(result, Err(PairError::Overflow));
    }

    #[test]
    fn overflow_on_huge_trade_size_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE / 10);

        let result = update_volatility(&env, &mut state, 100, i128::MAX, 1);
        assert_eq!(result, Err(PairError::Overflow));
    }

    // ------ Input validation -------------------------------------------------

    #[test]
    fn negative_price_delta_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE / 10);

        let result = update_volatility(&env, &mut state, -1, 1_000, 1_000_000);
        assert_eq!(result, Err(PairError::InvalidInput));
    }

    #[test]
    fn zero_trade_size_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE / 10);

        let result = update_volatility(&env, &mut state, 100, 0, 1_000_000);
        assert_eq!(result, Err(PairError::InvalidInput));
    }

    #[test]
    fn zero_total_reserve_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE / 10);

        let result = update_volatility(&env, &mut state, 100, 1_000, 0);
        assert_eq!(result, Err(PairError::InvalidInput));
    }

    // ------ Accumulator cap ---------------------------------------------------

    #[test]
    fn vol_accumulator_capped_at_max_fee_level() {
        let env = Env::default();
        let alpha = SCALE; // 100% — instant replacement
        let mut state = default_fee_state(alpha);

        // Repeatedly feed huge price deltas to try to blow up the accumulator.
        for _ in 0..100 {
            update_volatility(&env, &mut state, 1_000_000, 1_000_000, 1_000_000).unwrap();
        }

        // Fee should never exceed max_fee_bps
        let fee = compute_fee_bps(&state);
        assert!(
            fee <= state.max_fee_bps,
            "fee {} must not exceed max_fee_bps {}",
            fee,
            state.max_fee_bps,
        );
    }

    // ------ Alpha edge cases -------------------------------------------------

    #[test]
    fn alpha_above_scale_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE + 1);

        let result = update_volatility(&env, &mut state, 100, 1_000, 1_000_000);
        assert_eq!(result, Err(PairError::InvalidEmaAlpha));
    }

    #[test]
    fn negative_alpha_returns_error() {
        let env = Env::default();
        let mut state = default_fee_state(-1);

        let result = update_volatility(&env, &mut state, 100, 1_000, 1_000_000);
        assert_eq!(result, Err(PairError::InvalidEmaAlpha));
    }

    #[test]
    fn alpha_zero_means_no_update() {
        let env = Env::default();
        let mut state = default_fee_state(0); // alpha = 0
        state.vol_accumulator = 500;

        update_volatility(&env, &mut state, 1_000, 100_000, 1_000_000).unwrap();

        // With alpha=0 the new observation has zero weight — accumulator unchanged.
        assert_eq!(state.vol_accumulator, 500);
    }

    #[test]
    fn alpha_scale_means_full_replace() {
        let env = Env::default();
        let mut state = default_fee_state(SCALE); // alpha = SCALE (100 %)
        state.vol_accumulator = 999_999;

        // observation = 1000 * (100_000 / 1_000_000) = 100
        update_volatility(&env, &mut state, 1_000, 100_000, 1_000_000).unwrap();

        assert_eq!(
            state.vol_accumulator, 100,
            "alpha=SCALE should fully replace old accumulator with new observation"
        );
    }
}
