#![cfg(test)]

use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env};

use crate::errors::OracleError;
use crate::oracle::{consult_twap, update_cumulative_prices, MAX_TWAP_WINDOW};
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
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        // Push 30 observations
        for _i in 0..30 {
            update_cumulative_prices(&env, 100, 200, 1, &mut price_a, &mut price_b);
        }

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.observations.len(), 24, "ring buffer must not exceed 24 entries");
    });
}

#[test]
fn observations_are_appended_on_price_update() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        update_cumulative_prices(&env, 100, 200, 10, &mut price_a, &mut price_b);
        assert_eq!(price_a, 20);

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.observations.len(), 1);
        let (_, cum_a, _) = state.observations.get(0).unwrap();
        assert_eq!(cum_a, 20);
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
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10, &mut price_a, &mut price_b);

        // Observation 2 at sequence 200 (exact window of 100 ledgers)
        env.ledger().set_sequence_number(200);
        update_cumulative_prices(&env, 100, 200, 100, &mut price_a, &mut price_b);

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
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10, &mut price_a, &mut price_b);

        // Observation 2 at sequence 201 (window + 1 for window_ledgers = 100)
        env.ledger().set_sequence_number(201);
        update_cumulative_prices(&env, 100, 200, 101, &mut price_a, &mut price_b);

        let result = consult_twap(&env, 100);
        assert!(result.is_ok(), "window + 1 boundary must succeed");
    });
}

#[test]
fn consult_twap_boundary_insufficient_window() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        // Observation 1 at sequence 100
        env.ledger().set_sequence_number(100);
        update_cumulative_prices(&env, 100, 200, 10, &mut price_a, &mut price_b);

        // Observation 2 at sequence 150
        env.ledger().set_sequence_number(150);
        update_cumulative_prices(&env, 100, 200, 50, &mut price_a, &mut price_b);

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
