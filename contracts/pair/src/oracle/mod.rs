use soroban_sdk::Env;

// Cumulative price oracle (TWAP support).
// Tracks cumulative token prices for time-weighted average price queries.
//
// Uses wrapping arithmetic for accumulator updates (Uniswap V2 pattern)
// so that i128 overflow is handled correctly over long time horizons.

/// Update cumulative price accumulators with current reserves.
/// Called during every swap and liquidity event.
///
/// Skips the update when either reserve is zero (empty or uninitialized pool)
/// to prevent division-by-zero panics.
///
/// Uses wrapping arithmetic so that accumulator overflow is well-defined
/// and TWAP consumers can compute correct deltas across wrap-around.
#[allow(dead_code)]
pub fn update_cumulative_prices(
    _env: &Env,
    reserve_a: i128,
    reserve_b: i128,
    time_elapsed: u64,
    price_a_cumulative: &mut i128,
    price_b_cumulative: &mut i128,
) {
    // Guard: skip price update for empty or uninitialized pools.
    if reserve_a == 0 || reserve_b == 0 || time_elapsed == 0 {
        return;
    }

    // price_a = reserve_b / reserve_a (how much B per unit of A)
    // price_b = reserve_a / reserve_b (how much A per unit of B)
    // Multiply by time_elapsed to get the cumulative contribution.
    //
    // Use wrapping arithmetic so overflow wraps around rather than panicking.
    // TWAP consumers must use wrapping subtraction when computing deltas.
    let price_a_delta = (reserve_b / reserve_a).wrapping_mul(time_elapsed as i128);
    let price_b_delta = (reserve_a / reserve_b).wrapping_mul(time_elapsed as i128);

    *price_a_cumulative = price_a_cumulative.wrapping_add(price_a_delta);
    *price_b_cumulative = price_b_cumulative.wrapping_add(price_b_delta);
}

/// Compute the time-weighted average price (TWAP) over a period.
///
/// Uses wrapping subtraction to handle accumulator wrap-around correctly.
/// Returns 0 if time_elapsed is 0 to avoid division by zero.
#[allow(dead_code)]
pub fn consult_twap(
    price_cumulative_start: i128,
    price_cumulative_end: i128,
    time_elapsed: u64,
) -> i128 {
    if time_elapsed == 0 {
        return 0;
    }
    // Wrapping subtraction handles overflow wrap-around correctly.
    let delta = price_cumulative_end.wrapping_sub(price_cumulative_start);
    delta / (time_elapsed as i128)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    #[test]
    fn basic_price_accumulation() {
        let env = Env::default();
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        // reserve_a = 100, reserve_b = 200, time = 10
        // price_a += (200/100) * 10 = 20
        // price_b += (100/200) * 10 = 0 (integer division)
        update_cumulative_prices(&env, 100, 200, 10, &mut price_a, &mut price_b);
        assert_eq!(price_a, 20);
        assert_eq!(price_b, 0); // floor(100/200)*10 = 0
    }

    #[test]
    fn zero_reserve_a_skips_update() {
        let env = Env::default();
        let mut price_a: i128 = 42;
        let mut price_b: i128 = 42;

        update_cumulative_prices(&env, 0, 100, 10, &mut price_a, &mut price_b);
        assert_eq!(price_a, 42); // unchanged
        assert_eq!(price_b, 42); // unchanged
    }

    #[test]
    fn zero_reserve_b_skips_update() {
        let env = Env::default();
        let mut price_a: i128 = 42;
        let mut price_b: i128 = 42;

        update_cumulative_prices(&env, 100, 0, 10, &mut price_a, &mut price_b);
        assert_eq!(price_a, 42);
        assert_eq!(price_b, 42);
    }

    #[test]
    fn zero_time_elapsed_skips_update() {
        let env = Env::default();
        let mut price_a: i128 = 42;
        let mut price_b: i128 = 42;

        update_cumulative_prices(&env, 100, 200, 0, &mut price_a, &mut price_b);
        assert_eq!(price_a, 42);
        assert_eq!(price_b, 42);
    }

    #[test]
    fn wrapping_near_i128_max() {
        let env = Env::default();
        let mut price_a: i128 = i128::MAX - 10;
        let mut price_b: i128 = 0;

        // This should wrap around without panicking.
        // reserve_b/reserve_a = 100/1 = 100, * time 1 = 100
        // i128::MAX - 10 + 100 wraps
        update_cumulative_prices(&env, 1, 100, 1, &mut price_a, &mut price_b);

        // Should not panic, and the value should have wrapped
        assert!(price_a < 0 || price_a < i128::MAX - 10, "should wrap around i128::MAX");
    }

    #[test]
    fn twap_basic_computation() {
        let start: i128 = 100;
        let end: i128 = 300;
        let elapsed: u64 = 10;

        let twap = consult_twap(start, end, elapsed);
        assert_eq!(twap, 20); // (300-100)/10
    }

    #[test]
    fn twap_handles_wrap_around() {
        // Simulate wrap: start near MAX, end wrapped to small positive
        let start: i128 = i128::MAX - 5;
        let end: i128 = 10; // wrapped around

        let twap = consult_twap(start, end, 1);
        assert_eq!(twap, end.wrapping_sub(start));
    }

    #[test]
    fn twap_zero_time_returns_zero() {
        assert_eq!(consult_twap(100, 200, 0), 0);
    }
}
