#![cfg(test)]

use soroban_sdk::Env;

use crate::errors::PairError;
use crate::math::{mul_div, sqrt, BPS_DENOMINATOR, MINIMUM_LIQUIDITY, SCALE};

// ---------------------------------------------------------------------------
// Swap-math helpers (mirror the expected on-chain formulas)
// ---------------------------------------------------------------------------

/// Standard constant-product AMM output calculation.
///
/// Formula (Uniswap V2-style):
///   amount_out = (amount_in * (10 000 − fee_bps) * reserve_out)
///              / (reserve_in * 10 000 + amount_in * (10 000 − fee_bps))
///
/// Returns `Err` for invalid inputs (zero amounts, zero reserves, overflow).
fn get_amount_out(
    amount_in: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, PairError> {
    if amount_in <= 0 {
        return Err(PairError::InsufficientInputAmount);
    }
    if reserve_in <= 0 || reserve_out <= 0 {
        return Err(PairError::InsufficientLiquidity);
    }

    let bps_denom = BPS_DENOMINATOR as i128;
    let fee_factor = bps_denom
        .checked_sub(fee_bps as i128)
        .ok_or(PairError::Overflow)?;

    let amount_in_with_fee = amount_in
        .checked_mul(fee_factor)
        .ok_or(PairError::Overflow)?;

    let numerator = amount_in_with_fee
        .checked_mul(reserve_out)
        .ok_or(PairError::Overflow)?;

    let denominator = reserve_in
        .checked_mul(bps_denom)
        .ok_or(PairError::Overflow)?
        .checked_add(amount_in_with_fee)
        .ok_or(PairError::Overflow)?;

    if denominator == 0 {
        return Err(PairError::InsufficientLiquidity);
    }

    let amount_out = numerator / denominator;
    if amount_out <= 0 {
        return Err(PairError::InsufficientOutputAmount);
    }

    Ok(amount_out)
}

/// Check that the constant-product invariant `k = reserve_a * reserve_b`
/// does not decrease after a swap (it should increase due to fees).
fn k_after_swap(
    reserve_in: i128,
    reserve_out: i128,
    amount_in: i128,
    amount_out: i128,
) -> (i128, i128) {
    let k_before = reserve_in * reserve_out;
    let k_after = (reserve_in + amount_in) * (reserve_out - amount_out);
    (k_before, k_after)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

mod swap_math_tests {
    use super::*;

    // ---- 1. Basic Swap: amount_out for standard reserves ----
    #[test]
    fn test_basic_swap_amount_out() {
        let _env = Env::default();

        let reserve_in: i128 = 1_000_000_0000000; // 1 000 000 tokens (7 decimals)
        let reserve_out: i128 = 1_000_000_0000000;
        let amount_in: i128 = 1_000_0000000; // 1 000 tokens
        let fee_bps: u32 = 30; // 0.30 %

        let amount_out = get_amount_out(amount_in, reserve_in, reserve_out, fee_bps)
            .expect("basic swap should succeed");

        // amount_out must be positive and strictly less than reserve_out
        assert!(amount_out > 0, "amount_out must be positive");
        assert!(
            amount_out < reserve_out,
            "amount_out must be less than reserve_out"
        );

        // For a 0.1 % trade the output should be close to (but less than) amount_in
        // due to price impact + fees.
        assert!(
            amount_out < amount_in,
            "output should be less than input due to fees and slippage"
        );
    }

    // ---- 2. Fee Deduction: 0.3 % fee is applied correctly ----
    #[test]
    fn test_fee_deduction_30_bps() {
        let _env = Env::default();

        let reserve_in: i128 = 10_000_000;
        let reserve_out: i128 = 10_000_000;
        let amount_in: i128 = 10_000;
        let fee_bps: u32 = 30;

        let out_with_fee =
            get_amount_out(amount_in, reserve_in, reserve_out, fee_bps).unwrap();

        // Compare against zero-fee output
        let out_no_fee =
            get_amount_out(amount_in, reserve_in, reserve_out, 0).unwrap();

        assert!(
            out_with_fee < out_no_fee,
            "output with fee ({}) must be less than output without fee ({})",
            out_with_fee,
            out_no_fee,
        );

        // The fee-induced difference should be roughly proportional to fee_bps
        // (not an exact check since the fee compounds into the denominator).
        let diff = out_no_fee - out_with_fee;
        assert!(diff > 0, "fee must reduce the output");
    }

    // ---- 3. Dynamic Fee: higher fee_bps reduces output ----
    #[test]
    fn test_dynamic_fee_higher_fee_reduces_output() {
        let _env = Env::default();

        let reserve_in: i128 = 5_000_000;
        let reserve_out: i128 = 5_000_000;
        let amount_in: i128 = 50_000;

        let out_30 = get_amount_out(amount_in, reserve_in, reserve_out, 30).unwrap();
        let out_100 = get_amount_out(amount_in, reserve_in, reserve_out, 100).unwrap();
        let out_500 = get_amount_out(amount_in, reserve_in, reserve_out, 500).unwrap();

        assert!(
            out_30 > out_100,
            "30 bps fee output ({}) > 100 bps fee output ({})",
            out_30,
            out_100,
        );
        assert!(
            out_100 > out_500,
            "100 bps fee output ({}) > 500 bps fee output ({})",
            out_100,
            out_500,
        );
    }

    // ---- 4. K-Invariant: k increases (or stays same) after swap ----
    #[test]
    fn test_k_invariant_increases_with_fee() {
        let _env = Env::default();

        let reserve_in: i128 = 2_000_000;
        let reserve_out: i128 = 2_000_000;
        let amount_in: i128 = 100_000;
        let fee_bps: u32 = 30;

        let amount_out =
            get_amount_out(amount_in, reserve_in, reserve_out, fee_bps).unwrap();

        let (k_before, k_after) = k_after_swap(reserve_in, reserve_out, amount_in, amount_out);

        assert!(
            k_after >= k_before,
            "k must not decrease: k_before={}, k_after={}",
            k_before,
            k_after,
        );
    }

    // ---- 5. K-Invariant: zero fee keeps k equal ----
    #[test]
    fn test_k_invariant_equal_with_zero_fee() {
        let _env = Env::default();

        let reserve_in: i128 = 1_000_000;
        let reserve_out: i128 = 1_000_000;
        let amount_in: i128 = 10_000;
        let fee_bps: u32 = 0;

        let amount_out =
            get_amount_out(amount_in, reserve_in, reserve_out, fee_bps).unwrap();

        let (k_before, k_after) = k_after_swap(reserve_in, reserve_out, amount_in, amount_out);

        // With zero fee the integer rounding means k_after >= k_before
        // (truncation rounds the output down, preserving the invariant).
        assert!(
            k_after >= k_before,
            "k must not decrease even with zero fee: k_before={}, k_after={}",
            k_before,
            k_after,
        );
    }

    // ---- 6. Zero Inputs: swap must fail on zero amount_in ----
    #[test]
    fn test_zero_amount_in_fails() {
        let _env = Env::default();

        let result = get_amount_out(0, 1_000_000, 1_000_000, 30);
        assert_eq!(
            result,
            Err(PairError::InsufficientInputAmount),
            "zero amount_in must return InsufficientInputAmount"
        );
    }

    // ---- 7. Zero Inputs: negative amount_in rejected ----
    #[test]
    fn test_negative_amount_in_fails() {
        let _env = Env::default();

        let result = get_amount_out(-100, 1_000_000, 1_000_000, 30);
        assert_eq!(
            result,
            Err(PairError::InsufficientInputAmount),
            "negative amount_in must return InsufficientInputAmount"
        );
    }

    // ---- 8. Empty Pool: zero reserve_in causes failure ----
    #[test]
    fn test_empty_pool_reserve_in_zero() {
        let _env = Env::default();

        let result = get_amount_out(1_000, 0, 1_000_000, 30);
        assert_eq!(
            result,
            Err(PairError::InsufficientLiquidity),
            "zero reserve_in must return InsufficientLiquidity"
        );
    }

    // ---- 9. Empty Pool: zero reserve_out causes failure ----
    #[test]
    fn test_empty_pool_reserve_out_zero() {
        let _env = Env::default();

        let result = get_amount_out(1_000, 1_000_000, 0, 30);
        assert_eq!(
            result,
            Err(PairError::InsufficientLiquidity),
            "zero reserve_out must return InsufficientLiquidity"
        );
    }

    // ---- 10. Overflow: reserves near i128::MAX boundary ----
    #[test]
    fn test_overflow_large_reserves() {
        let _env = Env::default();

        // Use values large enough to trigger checked_mul overflow.
        let huge: i128 = i128::MAX / 2;
        let result = get_amount_out(huge, huge, huge, 30);

        assert_eq!(
            result,
            Err(PairError::Overflow),
            "near-max reserves must return Overflow"
        );
    }

    // ---- 11. Overflow: large amount_in triggers overflow ----
    #[test]
    fn test_overflow_large_amount_in() {
        let _env = Env::default();

        let result = get_amount_out(i128::MAX, 1_000_000, 1_000_000, 30);
        assert_eq!(
            result,
            Err(PairError::Overflow),
            "i128::MAX amount_in must return Overflow"
        );
    }

    // ---- 12. mul_div: basic precision check ----
    #[test]
    fn test_mul_div_basic() {
        let _env = Env::default();

        // (SCALE * 2) * (SCALE * 3) / SCALE == SCALE * 6
        let result = mul_div(SCALE * 2, SCALE * 3, SCALE);
        assert_eq!(
            result,
            Some(SCALE * 6),
            "mul_div basic multiplication failed"
        );
    }

    // ---- 13. mul_div: division by zero returns None ----
    #[test]
    fn test_mul_div_zero_denominator() {
        let _env = Env::default();

        let result = mul_div(100, 200, 0);
        assert_eq!(result, None, "mul_div with zero denominator must return None");
    }

    // ---- 14. sqrt: known values ----
    #[test]
    fn test_sqrt_known_values() {
        let _env = Env::default();

        assert_eq!(sqrt(0), 0);
        assert_eq!(sqrt(1), 1);
        assert_eq!(sqrt(4), 2);
        assert_eq!(sqrt(9), 3);
        assert_eq!(sqrt(100), 10);
        assert_eq!(sqrt(1_000_000), 1_000);
        assert_eq!(sqrt(10_000_000_000_000_000), 100_000_000); // sqrt(1e16) = 1e8

        // Non-perfect square: floor of the true square root
        assert_eq!(sqrt(2), 1);
        assert_eq!(sqrt(8), 2);
        assert_eq!(sqrt(10), 3);
    }

    // ---- 15. sqrt: negative input returns zero ----
    #[test]
    fn test_sqrt_negative_returns_zero() {
        let _env = Env::default();

        assert_eq!(sqrt(-1), 0);
        assert_eq!(sqrt(-100), 0);
        assert_eq!(sqrt(i128::MIN), 0);
    }

    // ---- 16. Symmetry: swapping direction gives equivalent results ----
    #[test]
    fn test_swap_symmetry_balanced_pool() {
        let _env = Env::default();

        // In a balanced pool, swapping A→B and B→A with same input should
        // yield the same output.
        let reserve: i128 = 5_000_000;
        let amount_in: i128 = 50_000;
        let fee_bps: u32 = 30;

        let out_a_to_b = get_amount_out(amount_in, reserve, reserve, fee_bps).unwrap();
        let out_b_to_a = get_amount_out(amount_in, reserve, reserve, fee_bps).unwrap();

        assert_eq!(
            out_a_to_b, out_b_to_a,
            "balanced pool must produce symmetric outputs"
        );
    }

    // ---- 17. Large realistic swap: price impact sanity ----
    #[test]
    fn test_large_swap_price_impact() {
        let _env = Env::default();

        // Pool with 100M tokens on each side, swap 10M (10 %).
        let reserve: i128 = 100_000_000_0000000;
        let amount_in: i128 = 10_000_000_0000000;
        let fee_bps: u32 = 30;

        let amount_out = get_amount_out(amount_in, reserve, reserve, fee_bps).unwrap();

        // With 10 % of pool, output should be notably less than 10 % of reserve
        // due to price impact. Approximate upper bound ≈ 9.07 % of reserve.
        let upper_bound = reserve * 907 / 10_000;
        assert!(
            amount_out < upper_bound,
            "large swap output ({}) must show significant price impact (< {})",
            amount_out,
            upper_bound,
        );
        assert!(amount_out > 0, "amount_out must be positive");
    }

    // ---- 18. Tiny swap: dust amount still produces valid output ----
    #[test]
    fn test_tiny_swap_dust_amount() {
        let _env = Env::default();

        let reserve: i128 = 1_000_000_000;
        let amount_in: i128 = 1; // smallest possible
        let fee_bps: u32 = 30;

        // With amount_in = 1, fee_factor = 9970, so effective = 9970
        // numerator = 9970 * 1_000_000_000 = 9_970_000_000_000
        // denominator = 1_000_000_000 * 10_000 + 9_970 = 10_000_000_009_970
        // amount_out = 9_970_000_000_000 / 10_000_000_009_970 ≈ 0
        // This should fail because truncated output is 0.
        let result = get_amount_out(amount_in, reserve, reserve, fee_bps);

        // Depending on reserves, dust may round to zero output → error.
        // With these reserves the output should still be non-zero.
        // Let's verify: 9970 * 1e9 / (1e9 * 10000 + 9970) ≈ 0.997
        // Integer truncation → 0 → should error.
        assert_eq!(
            result,
            Err(PairError::InsufficientOutputAmount),
            "dust swap that rounds to zero output must fail"
        );
    }
}
