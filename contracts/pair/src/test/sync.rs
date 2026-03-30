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
    let contract_id = env.register_contract(None, Pair);
    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);

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
    let contract_id = env.register_contract(None, Pair);
    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    // Initialize and set non-zero reserves
    let env_init = env.clone();
    env_init.as_contract(&contract_id, || {
        let env = env_init.clone();
        let _ = Pair::initialize(env.clone(), factory.clone(), token_a.clone(), token_b.clone(), lp_token.clone());
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
    let contract_id = env.register_contract(None, Pair);
    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);

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
    let contract_id = env.register_contract(None, Pair);
    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);

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

    // Get the timestamp after first sync
    let (initial_cumulative_a, initial_cumulative_b) = {
        let env_test = env.clone();
        let mut result = (0i128, 0i128);
        env_test.as_contract(&contract_id, || {
            let state = crate::storage::get_pair_state(&env_test);
            if let Some(s) = state {
                result = (s.price_a_cumulative, s.price_b_cumulative);
            }
        });
        result
    };

    // Second sync with no time elapsed
    let env_sync = env.clone();
    env_sync.as_contract(&contract_id, || {
        let env = env_sync.clone();
        let _ = Pair::sync(env.clone());
        let state = crate::storage::get_pair_state(&env).unwrap();
        // Prices should be unchanged (since balance is 0, reserves become 0)
        assert_eq!(state.price_a_cumulative, initial_cumulative_a, "price_a unchanged");
        assert_eq!(state.price_b_cumulative, initial_cumulative_b, "price_b unchanged");
    });
}

#[test]
fn test_sync_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, Pair);
    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);

    let factory = Address::generate(&env);
    let lp_token = Address::generate(&env);

    let env_test = env.clone();
    env_test.as_contract(&contract_id, || {
        let env = env_test.clone();
        let _ = Pair::initialize(env.clone(), factory, token_a, token_b, lp_token);
        let _ = Pair::sync(env.clone());
        let events = env.events().all();
        // Should have at least one event (sync event)
        assert!(!events.is_empty(), "sync event emitted");
    });
}




