//! Tick ↔ √price conversions in unsigned Q64.64 fixed point.
//!
//! A tick `t` represents the price `1.0001^t` (token B per token A), so the
//! square-root price at `t` is `1.0001^(t/2)`. Prices are carried as √price
//! because every swap-step quantity (amount deltas, liquidity) is linear in
//! √price, which keeps the hot-path math to one multiplication or division.
//!
//! # Why Q64.64 and ±443 636
//!
//! Soroban's widest native integer in a `contracttype` is `u128`/`i128`, so
//! √price is stored as a `u128` with 64 integer and 64 fractional bits
//! (Uniswap V3 instead uses Q64.96 inside a `uint160`, which does not fit).
//! The tick range is the largest symmetric range whose *price* stays within
//! `[2^-64, 2^64]`, i.e. `|t| <= floor(log_1.0001(2^64)) = 443_636` — the same
//! convention other Q64.64 CLMMs (e.g. Cetus, Turbos) use, so √prices are
//! directly comparable across ecosystems. Inside this range adjacent ticks
//! are always ≥ 2^17 raw units apart — far above the computation's few-ULP
//! rounding error — so [`sqrt_ratio_at_tick`] is strictly monotone and the
//! binary search in [`tick_at_sqrt_ratio`] is exact.
//!
//! # Algorithm
//!
//! [`sqrt_ratio_at_tick`] is the classic square-and-multiply over the bits of
//! `|t|`, in Q128.128 (`U256`) for headroom: start from
//! `2^128 / sqrt(1.0001)` and square it per bit position, multiplying into the
//! accumulator where the bit of `|t|` is set; positive ticks invert at the
//! end. Uniswap V3 inlines the 20 squared constants; here they are derived at
//! run time from the single base constant, which keeps exactly one magic
//! number in the source. Truncation across the ~40 Q128.128 steps keeps the
//! result within a few ULP of exact — the pinned-constant tests show 1 ULP at
//! the extremes. Conversions run only on `initialize` (and later on
//! tick-crossing maintenance), never per swap step, so the extra
//! multiplications are not hot-path cost.

use crate::errors::ConcentratedPairError;
use ethnum::U256;

/// Lowest representable tick: price `1.0001^MIN_TICK ~= 2^-64`.
pub const MIN_TICK: i32 = -443_636;
/// Highest representable tick: price `1.0001^MAX_TICK ~= 2^64`.
pub const MAX_TICK: i32 = 443_636;

/// `sqrt_ratio_at_tick(MIN_TICK)`: smallest valid Q64.64 √price (~2^32).
pub const MIN_SQRT_RATIO: u128 = 4_295_048_017;
/// `sqrt_ratio_at_tick(MAX_TICK)`: supremum of valid Q64.64 √prices (~2^96).
/// A pool price must stay strictly below this.
pub const MAX_SQRT_RATIO: u128 = 79_226_673_515_401_279_992_447_579_062;

/// `1.0` in Q128.128.
const ONE_Q128: U256 = U256::from_words(1, 0);

/// `floor(2^128 / sqrt(1.0001))` — `1.0001^-0.5` in Q128.128, the only magic
/// constant in this module (same base constant as Uniswap V3's `TickMath`).
/// `test::tick_math::base_constant_is_inverse_sqrt_1_0001` re-derives it from
/// first principles.
const SQRT_1_0001_INV_Q128: U256 = U256::new(0xfffcb933bd6fad37aa2d162d1a594001);

/// Returns `1.0001^(tick/2)` as a Q64.64 `u128`.
///
/// Errors with [`ConcentratedPairError::InvalidTick`] outside
/// `[MIN_TICK, MAX_TICK]`.
pub fn sqrt_ratio_at_tick(tick: i32) -> Result<u128, ConcentratedPairError> {
    if !(MIN_TICK..=MAX_TICK).contains(&tick) {
        return Err(ConcentratedPairError::InvalidTick);
    }

    let abs = tick.unsigned_abs();

    // Square-and-multiply over the bits of |tick|, accumulating
    // 1.0001^-(abs/2) in Q128.128 (every factor is < 1, so no overflow).
    let mut ratio = if abs & 1 != 0 { SQRT_1_0001_INV_Q128 } else { ONE_Q128 };
    let mut factor = SQRT_1_0001_INV_Q128; // 1.0001^-(2^(i-1)/2) at bit i
    let mut bit = 2u32;
    while bit <= abs {
        factor = (factor * factor) >> 128u32;
        if abs & bit != 0 {
            ratio = (ratio * factor) >> 128u32;
        }
        bit <<= 1;
    }

    if tick > 0 {
        // 2^256 / ratio, within 1 ULP (U256::MAX = 2^256 - 1). |tick| is
        // bounded so ratio >= 2^64 in Q128.128 and the quotient fits Q128.128
        // with a < 2^64 integer part.
        ratio = U256::MAX / ratio;
    }

    // Q128.128 → Q64.64, rounding up so a pool sitting exactly on a tick
    // boundary never rounds below it.
    let truncated = (ratio >> 64u32).as_u128();
    let round_up = u128::from(ratio.as_u128() << 64u32 != 0);
    Ok(truncated + round_up)
}

/// Returns the greatest tick whose √price is ≤ `sqrt_price` (i.e. the tick
/// the pool is "in" at that price).
///
/// Errors with [`ConcentratedPairError::InvalidSqrtPrice`] unless
/// `MIN_SQRT_RATIO <= sqrt_price < MAX_SQRT_RATIO`.
///
/// Binary search over [`sqrt_ratio_at_tick`] — ~20 iterations, each a handful
/// of `U256` multiplications. Deliberately not the bit-twiddling log2
/// approximation: this runs on `initialize` and tick transitions only, and
/// searching the *same* function that defines the forward mapping makes
/// `tick_at_sqrt_ratio(sqrt_ratio_at_tick(t)) == t` hold by construction
/// instead of by delicate error analysis.
pub fn tick_at_sqrt_ratio(sqrt_price: u128) -> Result<i32, ConcentratedPairError> {
    if !(MIN_SQRT_RATIO..MAX_SQRT_RATIO).contains(&sqrt_price) {
        return Err(ConcentratedPairError::InvalidSqrtPrice);
    }

    let mut lo = MIN_TICK;
    let mut hi = MAX_TICK - 1;
    while lo < hi {
        // Upper midpoint so the search closes from above onto the greatest
        // satisfying tick without looping on `lo == mid`.
        let mid = lo + (hi - lo + 1) / 2;
        // `mid` is always within [MIN_TICK, MAX_TICK], so this cannot fail.
        if sqrt_ratio_at_tick(mid)? <= sqrt_price {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Ok(lo)
}
