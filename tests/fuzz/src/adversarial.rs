//! Adversarial token profiles: decimal extremes, wildly asymmetric reserves,
//! and amounts at the i64 / i128 boundaries (issue #330).
//!
//! The properties in `lib.rs` draw reserves and trade sizes from one
//! "reasonable" range (1 … 1e15). That range cannot reach the shapes a real
//! pair actually sees once two tokens have different precision — a 1-decimal
//! token next to an 18-decimal one, or a pool whose reserves differ by twelve
//! orders of magnitude — so a trapped arithmetic overflow or a rounding
//! direction that quietly favours the user would slip through.
//!
//! Everything here drives the **real** shipped math in `coralswap_pair::math`
//! (through the `Option` adapters above); no formula is reimplemented. Each
//! property asserts the same three things:
//!
//! 1. no panic — an unsupported input comes back as `Err`, never a trap;
//! 2. the constant-product `K` never decreases, compared exactly in `U256` so
//!    the assertion itself cannot overflow at the boundaries; and
//! 3. rounding is bounded and always in the pool's favour: a swap output never
//!    exceeds the fee-free constant-product output, and minted / burned LP
//!    never exceeds the exact proportional share.

use ethnum::U256;
use proptest::prelude::*;

// Mirrors the real contract constant; the crate root's adapters below are
// called as `super::…` so this file needs no `use` that the non-test build of
// the lib (where `#[test]` items are stripped) would report as unused.
const MINIMUM_LIQUIDITY: i128 = super::MINIMUM_LIQUIDITY;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Raw units in one whole token for `decimals` decimal places.
fn unit_of(decimals: u32) -> i128 {
    (0..decimals).fold(1i128, |acc, _| acc.saturating_mul(10))
}

/// `U256` view of a non-negative amount, for overflow-free comparisons.
fn u(value: i128) -> U256 {
    debug_assert!(value >= 0, "amounts are non-negative by construction");
    U256::new(value as u128)
}

/// `K = reserve_in * reserve_out`, exactly, for any positive i128 reserves.
fn k(reserve_in: i128, reserve_out: i128) -> U256 {
    u(reserve_in) * u(reserve_out)
}

/// Fee-free constant-product output:
/// `amount_in * reserve_out / (reserve_in + amount_in)`.
fn ideal_out(amount_in: i128, reserve_in: i128, reserve_out: i128) -> U256 {
    (u(amount_in) * u(reserve_out)) / (u(reserve_in) + u(amount_in))
}

// ── Strategies ───────────────────────────────────────────────────────────────

/// The decimal profiles that break fixed-range fuzzing: the smallest realistic
/// precision (Stellar assets go down to 1) and tokens with 18 or more decimals,
/// where a single whole token is already 1e18 raw units.
fn decimal_profile() -> impl Strategy<Value = u32> {
    prop_oneof![Just(1u32), Just(7u32), Just(18u32), 18u32..=24u32]
}

/// A reserve expressed in whole tokens, then scaled to the profile's raw units.
///
/// Whole-token counts are capped so the raw value stays at or below 1e18: that
/// keeps `reserve * reserve` and `amount_in * reserve_out` inside `i128`, which
/// is the domain the real contract has to serve without overflowing.
fn raw_reserve() -> impl Strategy<Value = i128> {
    (decimal_profile(), 1i128..=1_000_000i128).prop_map(|(decimals, whole_tokens)| {
        let unit = unit_of(decimals);
        let max_whole = (1_000_000_000_000_000_000i128 / unit).max(1);
        whole_tokens.min(max_whole).saturating_mul(unit)
    })
}

/// Reserves whose ratio spans up to twelve orders of magnitude, which is what a
/// 1-decimal token paired with an 18-decimal one looks like in raw units.
fn asymmetric_reserves() -> impl Strategy<Value = (i128, i128)> {
    (
        1i128..=1_000_000i128,
        prop_oneof![Just(1i128), Just(1_000_000i128), Just(1_000_000_000_000i128)],
    )
        .prop_map(|(small, factor)| (small, small.saturating_mul(factor)))
}

/// Raw amounts pinned at the edges of the i64 and i128 ranges, where
/// `amount_in * (10_000 - fee_bps)` and `amount_in * reserve_out` overflow.
fn boundary_amount() -> impl Strategy<Value = i128> {
    prop_oneof![
        Just(1i128),
        Just(i64::MAX as i128),
        Just(i64::MAX as i128 + 1),
        Just(i64::MAX as i128 * 2),
        Just(i128::MAX / 2),
        Just(i128::MAX / 2 + 1),
        Just(i128::MAX - 1),
        Just(i128::MAX),
    ]
}

// ── Property 4: K invariant and bounded rounding across decimal extremes ─────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_swap_decimal_extremes_preserve_k(
        reserve_in  in raw_reserve(),
        reserve_out in raw_reserve(),
        amount_in   in raw_reserve(),
        fee         in super::fee_bps(),
    ) {
        let Some(amount_out) = super::get_amount_out(amount_in, reserve_in, reserve_out, fee) else {
            return Ok(()); // Err(Overflow) / Err(DustAmount) is an allowed outcome
        };

        prop_assert!(
            amount_out > 0 && amount_out < reserve_out,
            "Ok({}) must be positive and below reserve_out {}",
            amount_out, reserve_out
        );

        // K must never decrease. The raw cap keeps this add inside i128 for a
        // swap the contract accepted; skip rather than panic if it ever isn't.
        if let Some(reserve_in_after) = reserve_in.checked_add(amount_in) {
            prop_assert!(
                k(reserve_in_after, reserve_out - amount_out) >= k(reserve_in, reserve_out),
                "K decreased: r_in={} r_out={} in={} out={} fee={}",
                reserve_in, reserve_out, amount_in, amount_out, fee
            );
        }

        // Rounding bound: the output can never exceed the fee-free output, so
        // integer truncation always favours the pool. This is the assertion a
        // fee-factor or denominator typo trips, and it also rules out an
        // "Ok but effectively free" output at the decimal extremes.
        let ideal = ideal_out(amount_in, reserve_in, reserve_out);
        prop_assert!(
            u(amount_out) <= ideal,
            "output {} exceeds the fee-free bound {} (r_in={} r_out={} in={} fee={})",
            amount_out, ideal, reserve_in, reserve_out, amount_in, fee
        );
    }
}

// ── Property 5: 1 : 1e12 reserve asymmetry, in both directions ──────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_swap_asymmetric_reserves_both_directions(
        (small, big) in asymmetric_reserves(),
        amount_small in 1i128..=1_000_000i128,
        amount_big   in 1i128..=1_000_000_000_000i128,
        fee          in super::fee_bps(),
    ) {
        // Small side in, big side out.
        if let Some(out) = super::get_amount_out(amount_small, small, big, fee) {
            prop_assert!(
                out > 0 && out < big,
                "small->big output {} out of range (small={} big={})",
                out, small, big
            );
            prop_assert!(
                k(small + amount_small, big - out) >= k(small, big),
                "K decreased (small->big): small={} big={} in={} out={}",
                small, big, amount_small, out
            );
            prop_assert!(
                u(out) <= ideal_out(amount_small, small, big),
                "output {} exceeds the fee-free bound from the small reserve",
                out
            );
        }

        // Big side in, small side out — the direction where the output can be
        // truncated all the way down to dust.
        if let Some(out) = super::get_amount_out(amount_big, big, small, fee) {
            prop_assert!(
                out > 0 && out < small,
                "big->small output {} out of range (big={} small={})",
                out, big, small
            );
            prop_assert!(
                k(big + amount_big, small - out) >= k(big, small),
                "K decreased (big->small): big={} small={} in={} out={}",
                big, small, amount_big, out
            );
            prop_assert!(
                u(out) <= ideal_out(amount_big, big, small),
                "output {} exceeds the fee-free bound from the big reserve",
                out
            );
        }
    }
}

// ── Property 6: mint / burn rounding across decimals stays pool-favouring ───

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_mint_burn_rounding_is_bounded_across_decimals(
        amount_a  in raw_reserve(),
        amount_b  in raw_reserve(),
        reserve_a in raw_reserve(),
        reserve_b in raw_reserve(),
        supply    in 1i128..=1_000_000_000_000_000_000i128,
        burn_pct  in 1u32..=100u32,
    ) {
        // ── Initial mint: sqrt(a*b) - MINIMUM_LIQUIDITY, rounded down ─────────
        if let Some(lp) = super::initial_liquidity(amount_a, amount_b) {
            let product = u(amount_a) * u(amount_b);
            let seeded = u(lp) + u(MINIMUM_LIQUIDITY);
            prop_assert!(
                seeded * seeded <= product,
                "initial LP {} over-mints: (lp+MIN)^2 > amount_a*amount_b",
                lp
            );
            prop_assert!(
                (seeded + U256::ONE) * (seeded + U256::ONE) > product,
                "initial LP {} under-mints by more than one raw unit",
                lp
            );
        }

        // ── Subsequent mint: floor of the proportional share ─────────────────
        if let Some(lp) = super::subsequent_liquidity(amount_a, amount_b, reserve_a, reserve_b, supply) {
            let share_a = (u(amount_a) * u(supply)) / u(reserve_a);
            let share_b = (u(amount_b) * u(supply)) / u(reserve_b);
            let expected = share_a.min(share_b);
            prop_assert!(
                u(lp) <= expected,
                "minted LP {} exceeds the proportional share {}",
                lp, expected
            );
            prop_assert!(
                u(lp) + U256::ONE > expected,
                "minted LP {} is more than one raw unit below the share {}",
                lp, expected
            );
        }

        // ── Burn: never more than the proportional share, never negative ─────
        let burn_lp = (supply / 100).saturating_mul(burn_pct as i128).max(1);
        if let Some((out_a, out_b)) = super::burn_amounts(burn_lp, reserve_a, reserve_b, supply) {
            prop_assert!(out_a > 0 && out_b > 0);
            prop_assert!(
                out_a <= reserve_a && out_b <= reserve_b,
                "burned {} / {} exceeds reserves {} / {}",
                out_a, out_b, reserve_a, reserve_b
            );
            prop_assert!(
                u(out_a) <= (u(burn_lp) * u(reserve_a)) / u(supply),
                "burn out_a {} exceeds the proportional share",
                out_a
            );
            prop_assert!(
                u(out_b) <= (u(burn_lp) * u(reserve_b)) / u(supply),
                "burn out_b {} exceeds the proportional share",
                out_b
            );
        }
    }
}

// ── Property 7: i64 / i128 boundary inputs fail cleanly, never trap ─────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_boundary_amounts_fail_cleanly(
        reserve_in  in boundary_amount(),
        reserve_out in boundary_amount(),
        amount_in   in boundary_amount(),
        fee         in super::fee_bps(),
    ) {
        // The point of this property is that none of these inputs traps: the
        // call either answers or reports Err(Overflow) / Err(DustAmount).
        if let Some(out) = super::get_amount_out(amount_in, reserve_in, reserve_out, fee) {
            prop_assert!(out > 0 && out < reserve_out);
            if let Some(reserve_in_after) = reserve_in.checked_add(amount_in) {
                prop_assert!(
                    k(reserve_in_after, reserve_out - out) >= k(reserve_in, reserve_out),
                    "K decreased at the boundary: r_in={} r_out={} in={} out={}",
                    reserve_in, reserve_out, amount_in, out
                );
            }
            prop_assert!(u(out) <= ideal_out(amount_in, reserve_in, reserve_out));
        }

        // Mint / burn helpers must survive the same boundary values: an
        // overflow is an `Err`, never a panic (see the regression test below —
        // `sqrt` used to die on a product of `i128::MAX`).
        let _ = super::initial_liquidity(reserve_in, reserve_out);
        let _ = super::subsequent_liquidity(reserve_in, reserve_out, amount_in, reserve_out, reserve_in);
        let _ = super::burn_amounts(amount_in, reserve_in, reserve_out, reserve_in);
    }
}

// ── Regression: the largest representable product ───────────────────────────

/// Regression for the boundary case property 7 above shrank to: the initial
/// mint on the largest in-range product used to panic inside `sqrt`, because
/// Newton's seed `(value + 1) / 2` overflowed when `value == i128::MAX`.
/// The seed is now computed as `value / 2 + value % 2`, which is the same
/// number without the intermediate overflow.
#[test]
fn initial_liquidity_at_i128_max_product_does_not_panic() {
    let lp = super::initial_liquidity(i128::MAX, 1).expect("i128::MAX * 1 fits in i128");
    assert_eq!(lp, 13_043_817_825_332_782_212 - MINIMUM_LIQUIDITY);

    // The floor sqrt stays exact at the top of the range: lp + MIN is the
    // largest n with n * n <= i128::MAX.
    let seeded = 13_043_817_825_332_782_212u128;
    assert!(seeded * seeded <= i128::MAX as u128);
    assert!((seeded + 1) * (seeded + 1) > i128::MAX as u128);
}
