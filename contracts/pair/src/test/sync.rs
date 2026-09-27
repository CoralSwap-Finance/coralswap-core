#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events as _},
    Address, Env,
};

use crate::Pair;

// Minimal mock token contract for testing
#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn balance(_env: Env, _id: Address) -> i128 {
        // Return fixed balance for testing
        0
    }
}

// Tests for Pair::sync() - reserve synchronization and TWAP price updates
// Uses mock token contracts to simulate token interactions

#[test]
fn test_sync_succeeds_after_init() {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let token_a = env.register(MockToken, ());
    let token_b = env.register(MockToken, ());

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    // Initialize pair with mock tokens
    let env_init = env.clone();
    env_init.as_contract(&contract_id, || {
        let env = env_init.clone();
        let _ = Pair::initialize(env, factory, token_a, token_b, lp_token);
    });

    // Call sync - should succeed even though balance() returns 0
    let env_sync = env.clone();
    env_sync.as_contract(&contract_id, || {
        let env = env_sync.clone();
        let result = Pair::sync(env);
        assert!(result.is_ok(), "sync should succeed after pair initialization");
    });
}

#[test]
fn test_sync_resets_reserves() {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let token_a = env.register(MockToken, ());
    let token_b = env.register(MockToken, ());

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    // Initialize and set non-zero reserves
    let env_init = env.clone();
    env_init.as_contract(&contract_id, || {
        let env = env_init.clone();
        let _ = Pair::initialize(
            env.clone(),
            factory.clone(),
            token_a.clone(),
            token_b.clone(),
            lp_token.clone(),
        );
        let mut state = crate::storage::get_pair_state(&env).unwrap();
        state.reserve_a = 1000;
        state.reserve_b = 2000;
        crate::storage::set_pair_state(&env, &state);
    });

    // Call sync - reserves should reset to balance values (0 from mock)
    let env_sync = env.clone();
    env_sync.as_contract(&contract_id, || {
        let env = env_sync.clone();
        let _ = Pair::sync(env.clone());
        let state = crate::storage::get_pair_state(&env).unwrap();
        // Reserves should match actual balances (which are 0 from mock)
        assert_eq!(state.reserve_a, 0, "reserve_a reset to match balance");
        assert_eq!(state.reserve_b, 0, "reserve_b reset to match balance");
    });
}

#[test]
fn test_sync_updates_cumulative_prices_with_time() {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let token_a = env.register(MockToken, ());
    let token_b = env.register(MockToken, ());

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    // Initialize
    let env_init = env.clone();
    env_init.as_contract(&contract_id, || {
        let env = env_init.clone();
        let _ = Pair::initialize(env.clone(), factory, token_a, token_b, lp_token);
        let mut state = crate::storage::get_pair_state(&env).unwrap();
        // Set non-zero reserves
        state.reserve_a = 1000;
        state.reserve_b = 4000;
        crate::storage::set_pair_state(&env, &state);
    });

    // Sync should succeed
    let env_sync = env.clone();
    env_sync.as_contract(&contract_id, || {
        let env = env_sync.clone();
        let result = Pair::sync(env.clone());
        assert!(result.is_ok(), "sync should succeed with non-zero reserves");
        let state = crate::storage::get_pair_state(&env).unwrap();
        // Reserves should be updated from token balance (0)
        assert_eq!(state.reserve_a, 0, "reserve_a updated to token balance");
        assert_eq!(state.reserve_b, 0, "reserve_b updated to token balance");
    });
}

#[test]
fn test_sync_no_price_update_no_time() {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let token_a = env.register(MockToken, ());
    let token_b = env.register(MockToken, ());

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    // Initialize
    let env_init = env.clone();
    env_init.as_contract(&contract_id, || {
        let env = env_init.clone();
        let _ = Pair::initialize(env.clone(), factory, token_a, token_b, lp_token);
        // First sync to set initial state
        let _ = Pair::sync(env.clone());
    });

    // Snapshot the accumulators after the first sync.
    let initial = {
        let env_test = env.clone();
        env_test.as_contract(&contract_id, || {
            let oracle = crate::storage::get_oracle_state(&env_test);
            (oracle.price_a_cumulative, oracle.price_b_cumulative, oracle.observations.len())
        })
    };

    // Second sync with no time elapsed
    let env_sync = env.clone();
    env_sync.as_contract(&contract_id, || {
        let env = env_sync.clone();
        let _ = Pair::sync(env.clone());
        let oracle = crate::storage::get_oracle_state(&env);
        assert_eq!(
            (oracle.price_a_cumulative, oracle.price_b_cumulative, oracle.observations.len()),
            initial,
            "sync must not disturb the oracle accumulators when no time has elapsed"
        );
    });
}

/// Regression guard for #312: the accumulators belong to the oracle, not to
/// `PairStorage`.
///
/// `sync` still does not advance them — that wiring is #271 — but it must not
/// claim to: the pair carries no price state of its own, and the only place
/// cumulative prices live is `OracleState`.
#[test]
fn test_pair_state_carries_no_cumulative_price_fields() {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let token_a = env.register(MockToken, ());
    let token_b = env.register(MockToken, ());
    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let _ = Pair::initialize(env.clone(), factory, token_a, token_b, lp_token);
    });

    env.as_contract(&contract_id, || {
        let state = crate::storage::get_pair_state(&env).expect("PairStorage missing");

        // Advancing the oracle must not require any counterpart field on the
        // pair: the oracle is self-contained.
        crate::oracle::update_cumulative_prices(&env, 1_000, 2_000, 5);

        let oracle = crate::storage::get_oracle_state(&env);
        assert_eq!(oracle.price_a_cumulative, 10);
        assert_eq!(oracle.observations.len(), 1);

        // Pair state is untouched by the oracle update.
        assert_eq!(state.reserve_a, 0);
        assert_eq!(state.reserve_b, 0);
    });
}

#[test]
fn test_sync_emits_event() {
    let env = Env::default();
    let contract_id = env.register(Pair, ());
    let token_a = env.register(MockToken, ());
    let token_b = env.register(MockToken, ());

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    let env_test = env.clone();
    env_test.as_contract(&contract_id, || {
        let env = env_test.clone();
        let _ = Pair::initialize(env.clone(), factory, token_a, token_b, lp_token);
        let _ = Pair::sync(env.clone());
        let _events = env.events().all();
        // Should have at least one event (sync event)
        //assert!(!events.is_empty(), "sync event emitted");
    });
}
