//! Fixed-point arithmetic helpers for price and reserve calculations.
//! All values use 1e14 scaling to maintain precision without floating point.

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
    // Use u256 intermediate to avoid overflow on large reserves.
    // TODO: implement with soroban U256 type
    Some((a * b) / denominator)
}

/// Computed integer square root using Newton's method.
pub fn sqrt(value: i128) -> i128 {
    if value <= 0 {
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
