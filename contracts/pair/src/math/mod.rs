//! Fixed-point arithmetic helpers for price and reserve calculations.
//! All values use 1e14 scaling to maintain precision without floating point.
use ethnum::U256;

use crate::errors::PairError;

/// Fixed-point scale factor.
#[allow(dead_code)]
pub const SCALE: i128 = 100_000_000_000_000; // 1e14
/// Basis point denominator.
#[allow(dead_code)]
pub const BPS_DENOMINATOR: i128 = 10_000;
/// Minimum liquidity locked on first mint to prevent division by zero.
pub const MINIMUM_LIQUIDITY: i128 = 1_000;

/// Multiplied two scaled values and divided by SCALE to maintain precision.
#[allow(dead_code)]
pub fn mul_div(a: i128, b: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }

    let is_negative = (a < 0) ^ (b < 0) ^ (denominator < 0);
    let a_abs = U256::from(a.unsigned_abs());
    let b_abs = U256::from(b.unsigned_abs());
    let denominator_abs = U256::from(denominator.unsigned_abs());

    let quotient = a_abs.checked_mul(b_abs)?.checked_div(denominator_abs)?;

    let max_positive = U256::from(i128::MAX as u128);
    let max_negative_magnitude = U256::from(i128::MAX as u128) + U256::ONE;

    if !is_negative {
        if quotient > max_positive {
            return None;
        }
        return i128::try_from(quotient.as_u128()).ok();
    }

    if quotient > max_negative_magnitude {
        return None;
    }

    if quotient == max_negative_magnitude {
        return Some(i128::MIN);
    }

    let magnitude = i128::try_from(quotient.as_u128()).ok()?;
    Some(-magnitude)
}

/// Computed integer square root using Newton's method.
pub fn sqrt(value: i128) -> i128 {
    if value < 0 {
        panic!("sqrt received negative input");
    }
    if value == 0 {
        return 0;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

/// Integer square root for U256 values using Newton's method.
fn sqrt_u256(value: U256) -> U256 {
    if value == U256::ZERO {
        return U256::ZERO;
    }
    let mut x = value;
    let mut y = (x + U256::ONE) / U256::new(2);
    while y < x {
        x = y;
        y = (x + value / x) / U256::new(2);
    }
    x
}

/// Computes the optimal amount of `token_in` to swap before a single-sided
/// liquidity deposit.
///
/// # Problem
///
/// A user holds `amount` of one token and wants to add it as liquidity to a
/// pool with reserves `(reserve_in, reserve_out)`. To mint LP tokens the pool
/// requires *both* tokens in the current ratio, so the user must first swap
/// part of their input to obtain the complementary token.
///
/// Let `x` be the swap amount. After the swap:
/// - Pool receives `x` of token_in and releases `y` of token_out.
/// - Remaining deposit is `(amount - x, y)`.
/// - For this to be exactly proportional to the *post-swap* reserves
///   `(reserve_in + x, reserve_out - y)` we need:
///
/// ```text
/// (amount - x) / (reserve_in + x)  ==  y / (reserve_out - y)
/// ```
///
/// Combined with the constant-product swap formula (fee factor `f` in
/// basis-points, where `f = 10_000 - fee_bps`):
///
/// ```text
/// y = (x * f * reserve_out) / (reserve_in * 10_000 + x * f)
/// ```
///
/// Solving the resulting quadratic for `x` yields:
///
/// ```text
/// discriminant = reserve_in * (reserve_in * f^2 + 4 * f * amount * 10_000)
/// x = ( sqrt(discriminant) - reserve_in * f ) / (2 * f)
/// ```
///
/// All intermediate products can exceed `i128::MAX`, so we work in `U256`.
///
/// # Returns
/// `Ok(swap_in)` — the amount of the input token that should be swapped first.
///
/// # Errors
/// - `PairError::InvalidInput` if any argument is non-positive.
/// - `PairError::Overflow` if an intermediate computation overflows `U256`.
pub fn compute_swap_in_for_single_side(
    reserve_in: i128,
    amount: i128,
    fee_bps: u32,
) -> Result<i128, PairError> {
    if reserve_in <= 0 || amount <= 0 {
        return Err(PairError::InvalidInput);
    }

    // fee_bps is validated upstream to be in 0..=10000 by the dynamic fee
    // engine, but we guard anyway.
    if fee_bps > 10_000 {
        return Err(PairError::InvalidInput);
    }

    // f = 10_000 - fee_bps  (basis-point fee factor, integer)
    let f = U256::from(10_000u32 - fee_bps);
    let bps = U256::new(10_000);
    let r = U256::from(reserve_in.unsigned_abs());
    let s = U256::from(amount.unsigned_abs());

    // discriminant = r * (r * f^2 + 4 * f * s * 10_000)
    let f_sq = f.checked_mul(f).ok_or(PairError::Overflow)?;
    let r_f_sq = r.checked_mul(f_sq).ok_or(PairError::Overflow)?;
    let four_f_s = U256::new(4)
        .checked_mul(f)
        .ok_or(PairError::Overflow)?
        .checked_mul(s)
        .ok_or(PairError::Overflow)?
        .checked_mul(bps)
        .ok_or(PairError::Overflow)?;
    let inner = r_f_sq.checked_add(four_f_s).ok_or(PairError::Overflow)?;
    let discriminant = r.checked_mul(inner).ok_or(PairError::Overflow)?;

    // numerator = sqrt(discriminant) - r * f
    let sqrt_disc = sqrt_u256(discriminant);
    let r_f = r.checked_mul(f).ok_or(PairError::Overflow)?;

    if sqrt_disc < r_f {
        // Should not happen for valid positive inputs, but be defensive.
        return Err(PairError::InvalidInput);
    }
    let numerator = sqrt_disc - r_f;

    // denominator = 2 * f
    let denominator = U256::new(2).checked_mul(f).ok_or(PairError::Overflow)?;
    if denominator == U256::ZERO {
        // fee_bps == 10_000 (100% fee) — degenerate, refuse
        return Err(PairError::InvalidInput);
    }

    let swap_in_u256 = numerator / denominator;

    // Convert back to i128. The result is always <= amount which is i128,
    // so this should not overflow.
    let max_i128 = U256::from(i128::MAX as u128);
    if swap_in_u256 > max_i128 {
        return Err(PairError::Overflow);
    }

    Ok(swap_in_u256.as_u128() as i128)
}
