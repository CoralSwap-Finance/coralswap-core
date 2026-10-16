#![cfg(test)]

//! Property tests for `math::compute_swap_in_for_single_side`, the swap split
//! used by `Pair::mint_with_one_token`.
//!
//! The function's documentation promises that after swapping `x` of the input
//! token, the remaining deposit `(amount - x, y)` is proportional to the
//! post-swap reserves `(reserve_in + x, reserve_out - y)`. These tests check
//! that promise against the shipped math (not a re-implementation) over
//! thousands of pseudo-random pools, plus exact closed-form values and a
//! no-panic sweep of extreme inputs.
//!
//! Solving that stated condition together with the constant-product swap
//! gives `g*x^2 + r*(1 + g)*x - r*a = 0` (with `g = 1 - fee`), i.e. the standard
//! Uniswap-V2 zap. Dropping the `(1 + g)` terms instead solves
//! `x*g / r = a / (r + x)` and over-swaps: ~62% of a pool-sized deposit and ~99%
//! of a 1%-of-pool deposit.

use ethnum::U256;

use crate::errors::PairError;
use crate::math::{compute_swap_in_for_single_side, get_amount_out};

/// xorshift64*: deterministic, dependency-free, reproducible pseudo-random numbers.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[lo, hi]`.
    fn range(&mut self, lo: i128, hi: i128) -> i128 {
        lo + (self.next() as i128) % (hi - lo + 1)
    }

    /// Log-uniform in `[10^min_exp, 10^(max_exp + 1))`, so every order of
    /// magnitude is exercised rather than only the largest values.
    fn magnitude(&mut self, min_exp: u32, max_exp: u32) -> i128 {
        let exp = min_exp + (self.next() % u64::from(max_exp - min_exp + 1)) as u32;
        let base = 10i128.pow(exp);
        self.range(base, base * 10 - 1)
    }
}

/// The split exactly as `mint_with_one_token` computes it:
/// `(swap_in, deposit_in, deposit_out, post_swap_reserve_in, post_swap_reserve_out)`.
fn split(
    reserve_in: i128,
    reserve_out: i128,
    amount: i128,
    fee_bps: u32,
) -> (i128, i128, i128, i128, i128) {
    let swap_in = compute_swap_in_for_single_side(reserve_in, amount, fee_bps)
        .expect("a valid pool and deposit must split");
    let swap_out =
        get_amount_out(swap_in, reserve_in, reserve_out, fee_bps).expect("the swap must price");
    (swap_in, amount - swap_in, swap_out, reserve_in + swap_in, reserve_out - swap_out)
}

/// Relative gap between `deposit_in / post_in` and `deposit_out / post_out`,
/// in parts per million. Zero means the leftover deposit is exactly balanced.
fn imbalance_ppm(deposit_in: i128, deposit_out: i128, post_in: i128, post_out: i128) -> u128 {
    let lhs = U256::from(deposit_in as u128) * U256::from(post_out as u128);
    let rhs = U256::from(deposit_out as u128) * U256::from(post_in as u128);
    let (hi, lo) = if lhs > rhs { (lhs, rhs) } else { (rhs, lhs) };
    ((hi - lo) * U256::from(1_000_000u32) / hi).as_u128()
}

// ---------------------------------------------------------------------------
// Exact closed-form values
// ---------------------------------------------------------------------------

/// Zero fee, deposit equal to the reserve: the optimal swap is
/// `(sqrt(2) - 1) * amount = 414_213`. (The quadratic without the `(1 + g)`
/// terms returns the golden-ratio fraction, `618_033`, instead.)
#[test]
fn single_side_split_matches_closed_form_at_zero_fee() {
    assert_eq!(compute_swap_in_for_single_side(1_000_000, 1_000_000, 0), Ok(414_213));
}

/// A 1%-of-pool deposit at the default 30 bps fee swaps just under half of it.
/// (The quadratic without the `(1 + g)` terms swaps 9_931_456, i.e. 99.3%.)
#[test]
fn single_side_split_swaps_about_half_of_a_small_deposit() {
    assert_eq!(compute_swap_in_for_single_side(1_000_000_000, 10_000_000, 30), Ok(4_995_054));
}

// ---------------------------------------------------------------------------
// The documented invariant, over random pools
// ---------------------------------------------------------------------------

/// Over 5_000 pseudo-random pools (reserves 1e7..1e14, prices 1e-3..1e3,
/// deposits 1e-4x..3x the reserve, fees 0..10%), the swap stays strictly inside
/// `(0, amount)` and the leftover deposit is proportional to the post-swap
/// reserves within 0.1%. That tolerance is far above integer rounding and far
/// below any real mis-split (the broken quadratic is off by >60%).
///
/// Reserves are capped at 1e14 because `get_amount_out` works in `i128` and
/// overflows once `amount_in * 10_000 * reserve_out` exceeds `i128::MAX`; pools
/// that large cannot use `mint_with_one_token` at all, so they are out of scope.
#[test]
fn single_side_split_leaves_a_balanced_deposit() {
    let mut rng = Rng(0x00C0_2A15_3A9F_1CE5);
    let mut checked = 0u32;

    for _ in 0..5_000 {
        let reserve_in = rng.magnitude(7, 13);
        let reserve_out = reserve_in / 1_000 * rng.range(1, 1_000_000);
        let amount =
            (reserve_in / 10i128.pow(rng.range(0, 4) as u32) * rng.range(1, 3)).max(1_000_000);
        let fee_bps = rng.range(0, 1_000) as u32;

        let (swap_in, deposit_in, deposit_out, post_in, post_out) =
            split(reserve_in, reserve_out, amount, fee_bps);

        assert!(
            swap_in > 0 && swap_in < amount,
            "swap_in {swap_in} outside (0, {amount}) for reserve_in={reserve_in} \
             reserve_out={reserve_out} fee_bps={fee_bps}"
        );

        // A received side this small is rounding dust (mint rejects it anyway).
        if deposit_out < 100_000 {
            continue;
        }

        let ppm = imbalance_ppm(deposit_in, deposit_out, post_in, post_out);
        assert!(
            ppm <= 1_000,
            "leftover deposit is {ppm} ppm out of balance for reserve_in={reserve_in} \
             reserve_out={reserve_out} amount={amount} fee_bps={fee_bps}"
        );
        checked += 1;
    }

    assert!(checked >= 4_000, "only {checked} of 5_000 cases were large enough to check");
}

// ---------------------------------------------------------------------------
// Extremes: typed errors, never panics
// ---------------------------------------------------------------------------

/// Dust, zero-width fees, 100% fees and values near `i128::MAX` must return
/// `Ok` or a typed `PairError`, never panic.
#[test]
fn single_side_split_never_panics_on_extremes() {
    let big = i128::MAX / 4;
    for reserve_in in [1, 2, 1_000, 1_000_000_007, 10i128.pow(30), big, i128::MAX] {
        for amount in [1, 2, 1_000, reserve_in, big, i128::MAX] {
            for fee_bps in [0u32, 1, 30, 100, 9_999, 10_000, 10_001] {
                match compute_swap_in_for_single_side(reserve_in, amount, fee_bps) {
                    Ok(swap_in) => assert!(
                        (0..=amount).contains(&swap_in),
                        "swap_in {swap_in} outside [0, {amount}]"
                    ),
                    Err(PairError::InvalidInput | PairError::Overflow) => {}
                    Err(other) => panic!("unexpected error {other:?}"),
                }
            }
        }
    }
}
