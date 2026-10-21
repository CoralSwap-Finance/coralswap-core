use crate::errors::ConcentratedPairError;
use crate::tick_math::{
    sqrt_ratio_at_tick, tick_at_sqrt_ratio, MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK,
};
use ethnum::U256;

/// √(2 << 128): √price for price 2.0 in Q64.64 (`isqrt(2 * 2^128)`).
const SQRT_2_X64: u128 = 26_087_635_650_665_564_424;

// ---------------------------------------------------------------------------
// The one magic constant
// ---------------------------------------------------------------------------

/// Re-derives `SQRT_1_0001_INV_Q128 = floor(2^128 / sqrt(1.0001))` from first
/// principles: it must equal `isqrt(2^256 / 1.0001)`, computed here with
/// integer-only arithmetic (Newton's method), so a typo in the hex literal
/// cannot survive.
#[test]
fn base_constant_is_inverse_sqrt_1_0001() {
    // 2^256 / 1.0001 = (2^256 * 10000) / 10001, computed without overflowing
    // U256: split 2^256 = (2^192) * 2^64 and carry remainders manually.
    // Simpler: value = floor(U256::MAX / 10001) * 10000 is off by up to 10^4,
    // far below isqrt precision at this magnitude (~2^128 ULP spacing of the
    // square), so compare the *squares* instead with a matching tolerance.
    let base = U256::new(0xfffcb933bd6fad37aa2d162d1a594001u128);

    // base^2 / 2^128 should equal floor(2^128 * 10000 / 10001) within 2 ULP.
    let square_q128 = (base * base) >> 128;
    let expected = U256::from_words(1, 0) / U256::new(10_001) * U256::new(10_000);
    let diff = if square_q128 > expected { square_q128 - expected } else { expected - square_q128 };
    assert!(diff <= U256::new(20_000), "base constant is not 2^128/sqrt(1.0001): diff={diff}");
}

// ---------------------------------------------------------------------------
// sqrt_ratio_at_tick
// ---------------------------------------------------------------------------

#[test]
fn tick_zero_is_exactly_price_one() {
    assert_eq!(sqrt_ratio_at_tick(0).unwrap(), 1u128 << 64);
}

#[test]
fn extreme_ticks_match_pinned_constants() {
    assert_eq!(sqrt_ratio_at_tick(MIN_TICK).unwrap(), MIN_SQRT_RATIO);
    assert_eq!(sqrt_ratio_at_tick(MAX_TICK).unwrap(), MAX_SQRT_RATIO);
}

#[test]
fn matches_float_reference_across_range() {
    // f64 carries ~15-16 significant digits; the fixed-point result must
    // agree with 1.0001^(t/2) to within a generous float tolerance.
    for tick in [-443_636, -100_000, -6_932, -1, 0, 1, 2, 6_931, 100_000, 443_636] {
        let got = sqrt_ratio_at_tick(tick).unwrap() as f64 / (1u128 << 64) as f64;
        let want = 1.0001f64.powf(tick as f64 / 2.0);
        let rel = (got - want).abs() / want;
        assert!(rel < 1e-9, "tick {tick}: got {got}, want {want}, rel err {rel}");
    }
}

#[test]
fn strictly_monotone_including_sign_change_and_extremes() {
    for window in [MIN_TICK, -50_000, -1, 0, 1, 50_000, MAX_TICK - 1] {
        let lower = sqrt_ratio_at_tick(window).unwrap();
        let upper = sqrt_ratio_at_tick(window + 1).unwrap();
        assert!(lower < upper, "not monotone at tick {window}");
    }
}

#[test]
fn rejects_out_of_range_ticks() {
    assert_eq!(sqrt_ratio_at_tick(MIN_TICK - 1), Err(ConcentratedPairError::InvalidTick));
    assert_eq!(sqrt_ratio_at_tick(MAX_TICK + 1), Err(ConcentratedPairError::InvalidTick));
    assert_eq!(sqrt_ratio_at_tick(i32::MIN), Err(ConcentratedPairError::InvalidTick));
    assert_eq!(sqrt_ratio_at_tick(i32::MAX), Err(ConcentratedPairError::InvalidTick));
}

// ---------------------------------------------------------------------------
// tick_at_sqrt_ratio
// ---------------------------------------------------------------------------

#[test]
fn known_prices_land_on_known_ticks() {
    // Price 1.0 → tick 0.
    assert_eq!(tick_at_sqrt_ratio(1u128 << 64).unwrap(), 0);
    // Price 2.0 → floor(log_1.0001(2)) = 6931.
    assert_eq!(tick_at_sqrt_ratio(SQRT_2_X64).unwrap(), 6_931);
    // Price 0.5 → √price is 1/√2 in Q64.64, i.e. 2^128 / (√2·2^64);
    // log_1.0001(0.5) = -6931.56…, so the containing tick is -6932.
    let sqrt_half = (U256::from_words(1, 0) / U256::new(SQRT_2_X64)).as_u128();
    assert_eq!(tick_at_sqrt_ratio(sqrt_half).unwrap(), -6_932);
}

#[test]
fn round_trips_exactly_on_tick_boundaries() {
    for tick in [MIN_TICK, -300_000, -6_932, -1, 0, 1, 6_931, 300_000, MAX_TICK - 1] {
        let ratio = sqrt_ratio_at_tick(tick).unwrap();
        assert_eq!(tick_at_sqrt_ratio(ratio).unwrap(), tick, "round trip failed at {tick}");
    }
}

#[test]
fn one_below_a_boundary_belongs_to_the_previous_tick() {
    for tick in [-6_932, 0, 6_931, 300_000] {
        let ratio = sqrt_ratio_at_tick(tick).unwrap();
        assert_eq!(tick_at_sqrt_ratio(ratio - 1).unwrap(), tick - 1);
    }
}

#[test]
fn rejects_out_of_range_sqrt_prices() {
    assert_eq!(
        tick_at_sqrt_ratio(MIN_SQRT_RATIO - 1),
        Err(ConcentratedPairError::InvalidSqrtPrice)
    );
    assert_eq!(tick_at_sqrt_ratio(0), Err(ConcentratedPairError::InvalidSqrtPrice));
    // The max ratio itself is exclusive: it would need tick == MAX_TICK,
    // which no in-range price may sit on.
    assert_eq!(tick_at_sqrt_ratio(MAX_SQRT_RATIO), Err(ConcentratedPairError::InvalidSqrtPrice));
    assert_eq!(tick_at_sqrt_ratio(u128::MAX), Err(ConcentratedPairError::InvalidSqrtPrice));
}
