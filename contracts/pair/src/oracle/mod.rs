use crate::errors::OracleError;
use crate::storage::{get_oracle_state, set_oracle_state};
use soroban_sdk::Env;

pub const MAX_TWAP_WINDOW: u32 = 86400;

// The accumulator, its sample buffer and the constant bounding that buffer are
// only reachable from tests: `update_cumulative_prices` is not yet called by any
// pair entry point, which is tracked as #271. They are kept compiling and
// tested here so that #271 is a wiring change rather than a rewrite, and so the
// dead storage they used to occupy is not simply re-added later.
//
// Issue #312 removed the unwritten `price_a_cumulative` / `price_b_cumulative`
// fields from `PairStorage`; these are the replacement home for them.
#[allow(dead_code)]
pub const MAX_OBSERVATIONS: u32 = 24;

/// Advances the cumulative-price accumulators and records a sample.
///
/// The accumulators are owned by [`OracleState`] (issue #312) rather than by
/// `PairStorage`, so this function no longer takes mutable out-parameters: the
/// previous pair always holds the authoritative value, which removed the
/// possibility of a caller silently dropping an update on the floor.
///
/// This is still not wired into any pair entry point — that is #271. Until then
/// it is reachable only from tests, and the accumulators it maintains are
/// honestly scoped to the oracle rather than advertised as pool state.
///
/// No-op when either reserve is zero (the price is undefined) or when no time
/// has elapsed.
#[allow(dead_code)]
pub fn update_cumulative_prices(env: &Env, reserve_a: i128, reserve_b: i128, time_elapsed: u64) {
    if reserve_a == 0 || reserve_b == 0 || time_elapsed == 0 {
        return;
    }

    let price_a_delta = (reserve_b / reserve_a).wrapping_mul(time_elapsed as i128);
    let price_b_delta = (reserve_a / reserve_b).wrapping_mul(time_elapsed as i128);

    let mut oracle_state = get_oracle_state(env);
    oracle_state.price_a_cumulative = oracle_state.price_a_cumulative.wrapping_add(price_a_delta);
    oracle_state.price_b_cumulative = oracle_state.price_b_cumulative.wrapping_add(price_b_delta);

    if oracle_state.observations.len() >= MAX_OBSERVATIONS {
        oracle_state.observations.remove(0);
    }
    oracle_state.observations.push_back((
        env.ledger().sequence() as u64,
        oracle_state.price_a_cumulative,
        oracle_state.price_b_cumulative,
    ));
    set_oracle_state(env, &oracle_state);
}

pub fn consult_twap(env: &Env, window_ledgers: u32) -> Result<(i128, i128), OracleError> {
    if window_ledgers == 0 {
        return Err(OracleError::WindowTooShort);
    }
    if window_ledgers > MAX_TWAP_WINDOW {
        return Err(OracleError::WindowTooLong);
    }

    let oracle_state = get_oracle_state(env);
    if oracle_state.observations.is_empty() {
        return Err(OracleError::WindowTooShort);
    }

    let current_ledger = env.ledger().sequence() as u64;
    let target = current_ledger.saturating_sub(window_ledgers as u64);

    let obs = &oracle_state.observations;
    let len = obs.len();

    // If the oldest observation is newer than our target, the window is too short.
    let (oldest_ledger, oldest_a, oldest_b) = obs.get(0).unwrap();
    if oldest_ledger > target {
        return Err(OracleError::WindowTooShort);
    }

    let (mut start_ledger, mut start_a, mut start_b) = (oldest_ledger, oldest_a, oldest_b);
    let (mut end_ledger, mut end_a, mut end_b) = (oldest_ledger, oldest_a, oldest_b);

    for i in 0..len {
        let (l, a, b) = obs.get(i).unwrap();
        if l <= target {
            start_ledger = l;
            start_a = a;
            start_b = b;
        }
        if l >= target && end_ledger <= target {
            end_ledger = l;
            end_a = a;
            end_b = b;
            break;
        }
    }

    // Interpolate
    let interpolated_a = if end_ledger == start_ledger {
        start_a
    } else {
        start_a.wrapping_add(
            (end_a.wrapping_sub(start_a)) * (target as i128 - start_ledger as i128)
                / (end_ledger as i128 - start_ledger as i128),
        )
    };

    let interpolated_b = if end_ledger == start_ledger {
        start_b
    } else {
        start_b.wrapping_add(
            (end_b.wrapping_sub(start_b)) * (target as i128 - start_ledger as i128)
                / (end_ledger as i128 - start_ledger as i128),
        )
    };

    // A TWAP is the change in cumulative price over the change in time, so the
    // most recent sample is the "now" endpoint; only the start needs
    // interpolation against the requested target ledger.
    let (latest_ledger, latest_a, latest_b) = obs.last().unwrap();

    if latest_ledger < target + window_ledgers as u64 {
        return Err(OracleError::WindowTooShort);
    }
    let time_elapsed = latest_ledger.saturating_sub(target);
    if time_elapsed == 0 {
        return Ok((0, 0));
    }

    let delta_a = latest_a.wrapping_sub(interpolated_a);
    let delta_b = latest_b.wrapping_sub(interpolated_b);

    let price_a_avg = delta_a / (time_elapsed as i128);
    let price_b_avg = delta_b / (time_elapsed as i128);

    Ok((price_a_avg, price_b_avg))
}
