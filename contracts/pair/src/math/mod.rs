//! Fixed-point arithmetic helpers for price and reserve calculations.
//! All values use 1e14 scaling to maintain precision without floating point.
use ethnum::U256;

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
