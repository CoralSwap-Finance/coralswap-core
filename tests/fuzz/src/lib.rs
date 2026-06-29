//! Fuzz / property-based tests for CoralSwap Pair mint, burn, and swap math.
//!
//! Run with:
//!   cargo test -p coralswap-fuzz --features fuzz
//!
//! Each proptest macro generates 10 000 random inputs by default (configured
//! via `ProptestConfig::with_cases`).  Any invariant violation prints the
//! exact shrunk counter-example so the failure is immediately reproducible.

#![cfg(feature = "fuzz")]
#![allow(dead_code)] // helpers are used inside proptest! macros

use proptest::prelude::*;

// ── Constants (mirror contracts/pair/src/math/mod.rs) ────────────────────────

const MINIMUM_LIQUIDITY: i128 = 1_000;
const BPS_DENOMINATOR: i128 = 10_000;

// ── Pure math helpers (mirror on-chain logic) ─────────────────────────────────

fn sqrt(value: i128) -> i128 {
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

/// Uniswap V2-style constant-product output amount.
/// Returns None on invalid inputs or overflow.
fn get_amount_out(
    amount_in: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Option<i128> {
    if amount_in <= 0 || reserve_in <= 0 || reserve_out <= 0 {
        return None;
    }
    let fee_factor = BPS_DENOMINATOR.checked_sub(fee_bps as i128)?;
    let amount_in_with_fee = amount_in.checked_mul(fee_factor)?;
    let numerator = amount_in_with_fee.checked_mul(reserve_out)?;
    let denominator = reserve_in
        .checked_mul(BPS_DENOMINATOR)?
        .checked_add(amount_in_with_fee)?;
    if denominator == 0 {
        return None;
    }
    let out = numerator / denominator;
    if out <= 0 { None } else { Some(out) }
}

/// LP tokens minted for the initial deposit.
fn initial_liquidity(amount_a: i128, amount_b: i128) -> Option<i128> {
    let product = amount_a.checked_mul(amount_b)?;
    let liq = sqrt(product).checked_sub(MINIMUM_LIQUIDITY)?;
    if liq <= 0 { None } else { Some(liq) }
}

/// LP tokens minted for a subsequent deposit given current reserves and supply.
fn subsequent_liquidity(
    amount_a: i128,
    amount_b: i128,
    reserve_a: i128,
    reserve_b: i128,
    total_supply: i128,
) -> Option<i128> {
    if reserve_a <= 0 || reserve_b <= 0 || total_supply <= 0 {
        return None;
    }
    let liq_a = amount_a.checked_mul(total_supply)? / reserve_a;
    let liq_b = amount_b.checked_mul(total_supply)? / reserve_b;
    let liq = liq_a.min(liq_b);
    if liq <= 0 { None } else { Some(liq) }
}

/// Tokens returned when burning `lp_amount` from a pool.
fn burn_amounts(
    lp_amount: i128,
    reserve_a: i128,
    reserve_b: i128,
    total_supply: i128,
) -> Option<(i128, i128)> {
    if total_supply <= 0 || lp_amount <= 0 {
        return None;
    }
    let a = lp_amount.checked_mul(reserve_a)? / total_supply;
    let b = lp_amount.checked_mul(reserve_b)? / total_supply;
    if a <= 0 || b <= 0 { None } else { Some((a, b)) }
}

// ── Strategies ────────────────────────────────────────────────────────────────

/// Reserves in a realistic range: 1 … 10^15 (well below i128::MAX so
/// intermediate products don't overflow).
fn reserve() -> impl Strategy<Value = i128> {
    1_i128..=1_000_000_000_000_000_i128
}

/// Trade size: 1 … 10^12 (smaller than max reserve to keep swaps valid).
fn trade_size() -> impl Strategy<Value = i128> {
    1_i128..=1_000_000_000_000_i128
}

/// Fee in basis points: 0 … 500 (0 % … 5 %).
fn fee_bps() -> impl Strategy<Value = u32> {
    0_u32..=500_u32
}

// ── Property 1: K invariant never decreases after a valid swap ────────────────
//
// For any (reserve_in, reserve_out, amount_in, fee_bps) where get_amount_out
// returns Some(amount_out):
//   (reserve_in + amount_in) * (reserve_out - amount_out)  >=  reserve_in * reserve_out

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_k_invariant_never_decreases(
        reserve_in  in reserve(),
        reserve_out in reserve(),
        amount_in   in trade_size(),
        fee         in fee_bps(),
    ) {
        let Some(amount_out) = get_amount_out(amount_in, reserve_in, reserve_out, fee) else {
            return Ok(()); // invalid combination — skip
        };

        prop_assert!(
            amount_out < reserve_out,
            "amount_out ({}) must be < reserve_out ({})",
            amount_out, reserve_out
        );

        let k_before = reserve_in
            .checked_mul(reserve_out)
            .expect("k_before overflow");
        let k_after = (reserve_in + amount_in)
            .checked_mul(reserve_out - amount_out)
            .expect("k_after overflow");

        prop_assert!(
            k_after >= k_before,
            "K invariant violated: k_before={} k_after={} \
             (reserve_in={}, reserve_out={}, amount_in={}, amount_out={}, fee_bps={})",
            k_before, k_after, reserve_in, reserve_out, amount_in, amount_out, fee
        );
    }
}

// ── Property 2: amount_out > 0 for any valid positive inputs ─────────────────
//
// Whenever reserves are non-zero and amount_in > 0, get_amount_out must either
// return a strictly positive value or None (overflow / dust).
// It must never return Some(0) or Some(negative).

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_amount_out_positive_or_none(
        reserve_in  in reserve(),
        reserve_out in reserve(),
        amount_in   in trade_size(),
        fee         in fee_bps(),
    ) {
        if let Some(out) = get_amount_out(amount_in, reserve_in, reserve_out, fee) {
            prop_assert!(
                out > 0,
                "get_amount_out returned non-positive Some({}) for \
                 reserve_in={}, reserve_out={}, amount_in={}, fee_bps={}",
                out, reserve_in, reserve_out, amount_in, fee
            );
        }
        // None (overflow or dust) is acceptable
    }
}

// ── Property 3: LP supply consistency across mint / burn sequences ────────────
//
// Simulate: initial mint → N subsequent mints → full burn of all user LP.
// Invariants checked after every burn:
//   - out_a, out_b are non-negative and do not exceed current reserves
//   - total_supply never falls below MINIMUM_LIQUIDITY (the locked seed)
//   - reserves remain non-negative throughout

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn prop_lp_supply_consistent_mint_burn(
        init_a in 1_000_i128..=1_000_000_000_i128,
        init_b in 1_000_i128..=1_000_000_000_i128,
        // Up to 4 subsequent deposits expressed as a percentage of current reserves (1–50 %)
        extra_pcts in prop::collection::vec(1_u32..=50_u32, 0..=4),
    ) {
        // ── Initial mint ──────────────────────────────────────────────────────
        let Some(init_lp) = initial_liquidity(init_a, init_b) else {
            return Ok(()); // product too small — skip
        };

        let mut reserve_a    = init_a;
        let mut reserve_b    = init_b;
        // MINIMUM_LIQUIDITY is permanently locked; total_supply tracks all LP.
        let mut total_supply = init_lp + MINIMUM_LIQUIDITY;
        let mut lp_balances: Vec<i128> = vec![init_lp];

        // ── Subsequent mints ──────────────────────────────────────────────────
        for pct in &extra_pcts {
            let add_a = (reserve_a * (*pct as i128)) / 100;
            let add_b = (reserve_b * (*pct as i128)) / 100;
            if add_a <= 0 || add_b <= 0 {
                continue;
            }
            let Some(lp) = subsequent_liquidity(add_a, add_b, reserve_a, reserve_b, total_supply)
            else {
                continue; // overflow or dust — skip this round
            };
            reserve_a    += add_a;
            reserve_b    += add_b;
            total_supply += lp;
            lp_balances.push(lp);
        }

        // ── Burn every depositor's LP ─────────────────────────────────────────
        for lp in lp_balances {
            let Some((out_a, out_b)) = burn_amounts(lp, reserve_a, reserve_b, total_supply)
            else {
                // Dust burn — rounds to zero tokens; just reduce supply.
                total_supply -= lp;
                continue;
            };

            prop_assert!(out_a >= 0, "burn returned negative out_a={}", out_a);
            prop_assert!(out_b >= 0, "burn returned negative out_b={}", out_b);
            prop_assert!(out_a <= reserve_a,
                "burn out_a={} exceeds reserve_a={}", out_a, reserve_a);
            prop_assert!(out_b <= reserve_b,
                "burn out_b={} exceeds reserve_b={}", out_b, reserve_b);

            reserve_a    -= out_a;
            reserve_b    -= out_b;
            total_supply -= lp;
        }

        // After all user LP is burned, MINIMUM_LIQUIDITY remains locked forever.
        prop_assert!(
            total_supply >= MINIMUM_LIQUIDITY,
            "total_supply ({}) fell below MINIMUM_LIQUIDITY after burns",
            total_supply
        );
        prop_assert!(reserve_a >= 0, "reserve_a went negative: {}", reserve_a);
        prop_assert!(reserve_b >= 0, "reserve_b went negative: {}", reserve_b);
    }
}
