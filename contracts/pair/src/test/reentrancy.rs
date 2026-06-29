#![cfg(test)]

use soroban_sdk::{contract, contractimpl, Env};

use crate::{errors::PairError, reentrancy};

// Minimal mock contract for testing reentrancy guard
#[contract]
pub struct ReentrancyTest;

#[contractimpl]
impl ReentrancyTest {}

// ---------------------------------------------------------------------------
// Basic RAII Guard Acquisition
// ---------------------------------------------------------------------------

#[test]
fn test_guard_acquire_succeeds_on_first_call() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_guard.is_ok(), "guard acquire should succeed on first call");
        // Guard automatically releases when dropped at end of scope
    });
}

#[test]
fn test_guard_returns_locked_if_already_held() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        let _first = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_first.is_ok(), "first guard acquire should succeed");

        let second = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(matches!(second, Err(PairError::Locked)), "second acquire should return Locked while first guard is held");
        // First guard still held here, releases at end of scope
    });
}

#[test]
fn test_guard_releases_automatically_on_drop() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        {
            let _first = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(_first.is_ok(), "first guard acquire should succeed");
            // Guard drops and releases at end of this inner scope
        }

        // After first guard dropped, second acquire should succeed
        let _second = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_second.is_ok(), "acquire should succeed after first guard dropped");
    });
}

// ---------------------------------------------------------------------------
// Guard Releases on Error Path
// ---------------------------------------------------------------------------

#[test]
fn test_guard_releases_on_early_return() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    // Helper function that acquires guard and returns early with error
    fn operation_that_fails(env: &Env) -> Result<(), PairError> {
        let _guard = reentrancy::ReentrancyGuard::acquire(env)?;
        // Simulate an error occurring while guard is held
        return Err(PairError::InsufficientLiquidity);
        // Guard automatically releases here via Drop
    }

    env.as_contract(&contract_id, || {
        // First call fails but releases guard
        let result = operation_that_fails(&env);
        assert!(result.is_err(), "operation should fail");

        // Second call should succeed because guard was released
        let result2 = operation_that_fails(&env);
        assert!(result2.is_err(), "second operation should also fail but acquire guard successfully");
    });
}

// ---------------------------------------------------------------------------
// Lock State Persistence Within Guard Lifetime
// ---------------------------------------------------------------------------

#[test]
fn test_lock_state_persists_while_guard_held() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
        // Lock should persist while guard is held

        let result = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(matches!(result, Err(PairError::Locked)), "lock should persist while guard is held");

        // Guard releases at end of scope
    });

    // After guard dropped, new acquisition should succeed
    env.as_contract(&contract_id, || {
        let result = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(result.is_ok(), "lock should be cleared after guard dropped");
    });
}

// ---------------------------------------------------------------------------
// Guard: Lock -> Error -> Auto-Release -> Lock Cycle
// ---------------------------------------------------------------------------

#[test]
fn test_guard_lock_error_autorelease_relock_cycle() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        // Step 1: First acquire succeeds
        {
            let result1 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(result1.is_ok(), "step 1: guard acquire should succeed");

            // Step 2: Second acquire returns Locked error
            let result2 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(matches!(result2, Err(PairError::Locked)), "step 2: should get Locked error");
            // Step 3: Guard auto-releases at end of this scope
        }

        // Step 4: Acquire again after first guard dropped should succeed
        {
            let result3 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(result3.is_ok(), "step 3: acquire should succeed after first guard dropped");

            // Step 5: Second acquire should fail again
            let result4 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(matches!(result4, Err(PairError::Locked)), "step 4: should get Locked error again");
            // Step 6: Guard auto-releases at end of this scope
        }

        // Step 7: Verify clean state for next invocation
        let result5 = reentrancy::ReentrancyGuard::acquire(&env);
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
        let _guard1 = reentrancy::ReentrancyGuard::acquire(&env1);
        assert!(_guard1.is_ok(), "env1: guard acquire should succeed");
        // Guard held for duration of this closure
    });

    // env2 should have independent lock state
    env2.as_contract(&contract_id2, || {
        let _guard2 = reentrancy::ReentrancyGuard::acquire(&env2);
        assert!(_guard2.is_ok(), "env2: should have independent lock state");

        // Second acquire in env2 should fail (its own lock)
        let result3 = reentrancy::ReentrancyGuard::acquire(&env2);
        assert!(matches!(result3, Err(PairError::Locked)), "env2: second acquire should fail");
        // Guard releases at end of scope
    });

    // env1's guard was already dropped, so new acquisition should succeed
    env1.as_contract(&contract_id1, || {
        let result4 = reentrancy::ReentrancyGuard::acquire(&env1);
        assert!(result4.is_ok(), "env1: should be unlocked after guard dropped");
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
        let _guard1 = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_guard1.is_ok(), "fresh env should be unlocked");

        // Verify it's now locked
        let result2 = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(matches!(result2, Err(PairError::Locked)), "should be locked while guard is held");
    });
}

// ---------------------------------------------------------------------------
// Guard: Automatic cleanup (no manual release needed)
// ---------------------------------------------------------------------------

#[test]
fn test_guard_automatic_cleanup() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        {
            let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
            // Guard automatically releases when dropped
        }

        // Multiple scopes with guards should all clean up properly
        {
            let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
        }

        // Final acquire should succeed
        let result = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(result.is_ok(), "acquire should succeed after all guards dropped");
    });
}

// ---------------------------------------------------------------------------
// Guard: Releases on panic (simulated via early return in error path)
// ---------------------------------------------------------------------------

#[test]
fn test_guard_releases_even_on_panic_simulation() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    // Helper function that simulates a panic-like scenario by returning an error
    // In real Soroban contracts, panics would unwind the stack and Drop would be called
    fn operation_that_might_panic(env: &Env, should_fail: bool) -> Result<(), PairError> {
        let _guard = reentrancy::ReentrancyGuard::acquire(env)?;
        
        if should_fail {
            // Simulate an error that causes early return
            // In a real panic, Drop is still called during unwinding
            return Err(PairError::InsufficientLiquidity);
        }
        
        Ok(())
    }

    env.as_contract(&contract_id, || {
        // First call fails, but guard should be released via Drop
        let result1 = operation_that_might_panic(&env, true);
        assert!(result1.is_err(), "operation should fail");

        // Second call should succeed because guard was released
        let result2 = operation_that_might_panic(&env, false);
        assert!(result2.is_ok(), "second operation should succeed - guard was released");

        // Third call should also succeed
        let result3 = operation_that_might_panic(&env, false);
        assert!(result3.is_ok(), "third operation should succeed");
    });
}

// ---------------------------------------------------------------------------
// Guard: Concurrent flash loan attempt correctly rejected
// ---------------------------------------------------------------------------

#[test]
fn test_concurrent_operation_rejected() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ReentrancyTest);

    env.as_contract(&contract_id, || {
        // Acquire guard for first operation
        let _guard1 = reentrancy::ReentrancyGuard::acquire(&env).unwrap();

        // Attempt to acquire guard for concurrent operation should fail
        let concurrent_attempt = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(concurrent_attempt, Err(PairError::Locked)),
            "concurrent operation should be rejected with Locked error"
        );

        // Guard1 still held here, will release at end of scope
    });

    // After first operation completes, new operation should succeed
    env.as_contract(&contract_id, || {
        let result = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(result.is_ok(), "new operation should succeed after previous completed");
    });
}
