#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::oracle::{consult_twap, update_cumulative_prices, MAX_TWAP_WINDOW};
use crate::errors::OracleError;
use crate::Pair;

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, Pair);
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
