#![cfg(test)]

use soroban_sdk::{contract, contractimpl, Env};

use crate::{errors::PairError, reentrancy};

// Minimal mock contract for testing reentrancy guard
#[contract]
pub struct ReentrancyTest;

#[contractimpl]
impl ReentrancyTest {}

// ---------------------------------------------------------------------------
// Basic Lock/Unlock Cycle
// ---------------------------------------------------------------------------

#[test]
fn test_acquire_succeeds_on_first_call() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        let result = reentrancy::acquire(&env);
        assert!(result.is_ok(), "acquire should succeed on first call");
    });
}

#[test]
fn test_acquire_returns_locked_if_already_held() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        let first = reentrancy::acquire(&env);
        assert!(first.is_ok(), "first acquire should succeed");

        let second = reentrancy::acquire(&env);
        assert_eq!(second, Err(PairError::Locked), "second acquire should return Locked");
    });
}

#[test]
fn test_release_clears_lock() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        let first = reentrancy::acquire(&env);
        assert!(first.is_ok(), "first acquire should succeed");

        reentrancy::release(&env);

        let second = reentrancy::acquire(&env);
        assert!(second.is_ok(), "acquire should succeed after release");
    });
}

// ---------------------------------------------------------------------------
// Lock State Persistence
// ---------------------------------------------------------------------------

#[test]
fn test_lock_state_persists_within_invocation() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        reentrancy::acquire(&env).unwrap();
        // Lock should persist - we don't leave the invocation

        let result = reentrancy::acquire(&env);
        assert_eq!(result, Err(PairError::Locked), "lock should persist within invocation");

        reentrancy::release(&env);

        // After release, lock should be cleared
        let result = reentrancy::acquire(&env);
        assert!(result.is_ok(), "lock should be cleared after release");
    });
}

// ---------------------------------------------------------------------------
// Guard: Lock -> Error -> Release -> Lock Cycle
// ---------------------------------------------------------------------------

#[test]
fn test_lock_error_release_relock_cycle() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        // Step 1: First acquire succeeds
        let result1 = reentrancy::acquire(&env);
        assert!(result1.is_ok(), "step 1: acquire should succeed");

        // Step 2: Second acquire returns Locked error
        let result2 = reentrancy::acquire(&env);
        assert_eq!(result2, Err(PairError::Locked), "step 2: should get Locked error");

        // Step 3: Release clears lock
        reentrancy::release(&env);

        // Step 4: Acquire again after release should succeed
        let result3 = reentrancy::acquire(&env);
        assert!(result3.is_ok(), "step 3: acquire should succeed after release");

        // Step 5: Second acquire should fail again
        let result4 = reentrancy::acquire(&env);
        assert_eq!(result4, Err(PairError::Locked), "step 4: should get Locked error again");

        // Step 6: Release again to clean up
        reentrancy::release(&env);

        // Step 7: Verify clean state for next invocation
        let result5 = reentrancy::acquire(&env);
        assert!(result5.is_ok(), "step 5: clean state for next invocation");
    });
}

// ---------------------------------------------------------------------------
// Guard: Lock state is independent per environment/contract
// ---------------------------------------------------------------------------

#[test]
fn test_separate_envs_have_independent_locks() {
    let env1 = Env::default();
    let contract_id1 = env1.register_contract(None, ReentrancyTest);

    let env2 = Env::default();
    let contract_id2 = env2.register_contract(None, ReentrancyTest);

    // Lock in env1
    env1.as_contract(&contract_id1, || {
        let result1 = reentrancy::acquire(&env1);
        assert!(result1.is_ok(), "env1: acquire should succeed");
    });

    // env2 should have independent lock state
    env2.as_contract(&contract_id2, || {
        let result2 = reentrancy::acquire(&env2);
        assert!(result2.is_ok(), "env2: should have independent lock state");

        // Second acquire in env2 should fail (its own lock)
        let result3 = reentrancy::acquire(&env2);
        assert_eq!(result3, Err(PairError::Locked), "env2: second acquire should fail");

        // Releasing env2 shouldn't affect env1's lock
        reentrancy::release(&env2);
    });

    // env1 should still be locked
    env1.as_contract(&contract_id1, || {
        let result4 = reentrancy::acquire(&env1);
        assert_eq!(result4, Err(PairError::Locked), "env1: should still be locked");

        reentrancy::release(&env1);
    });
}

// ---------------------------------------------------------------------------
// Guard: Default state is unlocked
// ---------------------------------------------------------------------------

#[test]
fn test_default_state_is_unlocked() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        // Fresh environment should allow acquire immediately
        let result1 = reentrancy::acquire(&env);
        assert!(result1.is_ok(), "fresh env should be unlocked");

        // Verify it's now locked
        let result2 = reentrancy::acquire(&env);
        assert_eq!(result2, Err(PairError::Locked), "should be locked after acquire");
    });
}

// ---------------------------------------------------------------------------
// Guard: Multiple release calls (idempotency of release)
// ---------------------------------------------------------------------------

#[test]
fn test_release_is_idempotent() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        reentrancy::acquire(&env).unwrap();

        // First release clears lock
        reentrancy::release(&env);

        // Second release should also work (no error)
        reentrancy::release(&env);

        // Third acquire should succeed
        let result = reentrancy::acquire(&env);
        assert!(result.is_ok(), "acquire should succeed after multiple releases");
    });
}
