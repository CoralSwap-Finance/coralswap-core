#![cfg(test)]

use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env};

use crate::errors::OracleError;
use crate::oracle::{consult_twap, update_cumulative_prices, MAX_OBSERVATIONS, MAX_TWAP_WINDOW};
use crate::Pair;

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    (env, contract_id)
}

#[test]
fn ring_buffer_capped_at_24() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Push 30 observations
        for _i in 0..30 {
            update_cumulative_prices(&env, 100, 200, 1);
        }

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.observations.len(), 24, "ring buffer must not exceed 24 entries");
        assert_eq!(MAX_OBSERVATIONS, 24);
    });
}

#[test]
fn observations_are_appended_on_price_update() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 100, 200, 10);

        let state = crate::storage::get_oracle_state(&env);
        // The accumulators live on the oracle state (issue #312) rather than on
        // PairStorage, where they used to sit unwritten.
        //
        // `price_a` is `reserve_b / reserve_a * elapsed` = 200/100 * 10 = 20.
        // `price_b` is `reserve_a / reserve_b * elapsed` = 100/200 * 10, and
        // that division truncates to 0 — the same integer-division truncation
        // Uniswap V2's accumulators inherit.
        assert_eq!(state.price_a_cumulative, 20);
        assert_eq!(state.price_b_cumulative, 0);
        assert_eq!(state.observations.len(), 1);
        let (_, cum_a, cum_b) = state.observations.get(0).unwrap();
        assert_eq!(cum_a, 20);
        assert_eq!(cum_b, 0);
    });
}

/// The accumulators must compound across calls, which is the whole point of a
/// cumulative price: a second sample adds to the first rather than replacing it.
#[test]
fn cumulative_prices_accumulate_across_calls() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 100, 200, 10);
        let first = crate::storage::get_oracle_state(&env);
        assert_eq!(first.price_a_cumulative, 20);

        update_cumulative_prices(&env, 100, 200, 5);
        let second = crate::storage::get_oracle_state(&env);
        assert_eq!(second.price_a_cumulative, 30, "accumulators must compound");
        assert_eq!(second.observations.len(), 2);

        // Every sample in the buffer must be non-decreasing.
        let (_, prev, _) = second.observations.get(0).unwrap();
        let (_, latest, _) = second.observations.get(1).unwrap();
        assert!(latest >= prev);
    });
}

/// Dropping the out-parameters means the state is the single source of truth:
/// a caller cannot update the buffer while discarding the accumulated totals.
#[test]
fn accumulator_and_buffer_cannot_diverge() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        for elapsed in [1i128, 2, 3] {
            update_cumulative_prices(&env, 100, 200, elapsed as u64);
        }

        let state = crate::storage::get_oracle_state(&env);
        let (_, latest_a, latest_b) = state.observations.last().unwrap();
        assert_eq!(state.price_a_cumulative, latest_a);
        assert_eq!(state.price_b_cumulative, latest_b);
    });
}

/// A zero reserve makes the price undefined, so the update must be a no-op
/// rather than a division-by-zero wrap.
#[test]
fn update_is_noop_when_reserves_are_empty() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 0, 200, 10);
        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.price_a_cumulative, 0);
        assert_eq!(state.observations.len(), 0);
    });
}

/// Zero elapsed time contributes nothing and must not create a sample.
#[test]
fn update_is_noop_when_no_time_has_elapsed() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        update_cumulative_prices(&env, 100, 200, 0);
        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.price_a_cumulative, 0);
        assert_eq!(state.observations.len(), 0);
    });
}

#[test]
fn consult_twap_window_zero_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, 0);
        assert_eq!(result, Err(OracleError::WindowTooShort));
    });
}

#[test]
fn consult_twap_window_too_long_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, MAX_TWAP_WINDOW + 1);
        assert_eq!(result, Err(OracleError::WindowTooLong));
    });
}

#[test]
fn consult_twap_no_observations_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, 100);
        assert_eq!(result, Err(OracleError::WindowTooShort));
    });
}

#[test]
fn consult_twap_boundary_exact_window() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10);

        // Observation 2 at sequence 200 (exact window of 100 ledgers)
        env.ledger().set_sequence_number(200);
        update_cumulative_prices(&env, 100, 200, 100);

        let result = consult_twap(&env, 100);
        assert!(result.is_ok(), "exact window boundary must succeed");
        let (avg_a, avg_b) = result.unwrap();
        assert!(avg_a > 0 || avg_b > 0);
    });
}

#[test]
fn consult_twap_boundary_window_plus_one() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10);

        // Observation 2 at sequence 201 (window + 1 for window_ledgers = 100)
        env.ledger().set_sequence_number(201);
        update_cumulative_prices(&env, 100, 200, 101);

        let result = consult_twap(&env, 100);
        assert!(result.is_ok(), "window + 1 boundary must succeed");
    });
}

#[test]
fn consult_twap_boundary_insufficient_window() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10);

        // Observation 2 at sequence 150
        env.ledger().set_sequence_number(150);
        update_cumulative_prices(&env, 100, 200, 50);

        // Advance ledger to 200 without updating observation (latest obs is at 150 < target 100 + window 100 = 200)
        env.ledger().set_sequence_number(200);

        let result = consult_twap(&env, 100);
        assert_eq!(
            result,
            Err(OracleError::WindowTooShort),
            "insufficient window must return WindowTooShort"
        );

        // Also test when oldest observation is newer than target:
        // Window 120 -> target = 200 - 120 = 80, but oldest obs is 100 > 80
        let result_too_short = consult_twap(&env, 120);
        assert_eq!(
            result_too_short,
            Err(OracleError::WindowTooShort),
            "window older than oldest observation must return WindowTooShort"
        );
    });
}
