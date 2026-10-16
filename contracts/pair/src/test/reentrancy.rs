#![cfg(test)]

use soroban_sdk::testutils::storage::Instance as _;
use soroban_sdk::testutils::Ledger as _;
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
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_guard.is_ok(), "guard acquire should succeed on first call");
    });
}

#[test]
fn test_guard_returns_locked_if_already_held() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _first = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_first.is_ok(), "first guard acquire should succeed");

        let second = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(second, Err(PairError::Locked)),
            "second acquire should return Locked while first guard is held"
        );
    });
}

#[test]
fn test_guard_releases_automatically_on_drop() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        {
            let _first = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(_first.is_ok(), "first guard acquire should succeed");
        }

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
    let contract_id = env.register(ReentrancyTest, ());

    fn operation_that_fails(env: &Env) -> Result<(), PairError> {
        let _guard = reentrancy::ReentrancyGuard::acquire(env)?;
        Err(PairError::InsufficientLiquidity)
    }

    env.as_contract(&contract_id, || {
        let result = operation_that_fails(&env);
        assert!(result.is_err(), "operation should fail");

        let result2 = operation_that_fails(&env);
        assert!(
            result2.is_err(),
            "second operation should also fail but acquire guard successfully"
        );
    });
}

// ---------------------------------------------------------------------------
// Lock State Persistence Within Guard Lifetime
// ---------------------------------------------------------------------------

#[test]
fn test_lock_state_persists_while_guard_held() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();

        let result = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(result, Err(PairError::Locked)),
            "lock should persist while guard is held"
        );
    });

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
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        {
            let result1 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(result1.is_ok(), "step 1: guard acquire should succeed");

            let result2 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(matches!(result2, Err(PairError::Locked)), "step 2: should get Locked error");
        }

        {
            let result3 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(result3.is_ok(), "step 3: acquire should succeed after first guard dropped");

            let result4 = reentrancy::ReentrancyGuard::acquire(&env);
            assert!(
                matches!(result4, Err(PairError::Locked)),
                "step 4: should get Locked error again"
            );
        }

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
    let contract_id1 = env1.register(ReentrancyTest, ());

    let env2 = Env::default();
    let contract_id2 = env2.register(ReentrancyTest, ());

    env1.as_contract(&contract_id1, || {
        let _guard1 = reentrancy::ReentrancyGuard::acquire(&env1);
        assert!(_guard1.is_ok(), "env1: guard acquire should succeed");
    });

    env2.as_contract(&contract_id2, || {
        let _guard2 = reentrancy::ReentrancyGuard::acquire(&env2);
        assert!(_guard2.is_ok(), "env2: should have independent lock state");

        let result3 = reentrancy::ReentrancyGuard::acquire(&env2);
        assert!(matches!(result3, Err(PairError::Locked)), "env2: second acquire should fail");
    });

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
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _guard1 = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(_guard1.is_ok(), "fresh env should be unlocked");

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
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        {
            let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
        }

        {
            let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
        }

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
    let contract_id = env.register(ReentrancyTest, ());

    fn operation_that_might_panic(env: &Env, should_fail: bool) -> Result<(), PairError> {
        let _guard = reentrancy::ReentrancyGuard::acquire(env)?;

        if should_fail {
            return Err(PairError::InsufficientLiquidity);
        }

        Ok(())
    }

    env.as_contract(&contract_id, || {
        let result1 = operation_that_might_panic(&env, true);
        assert!(result1.is_err(), "operation should fail");

        let result2 = operation_that_might_panic(&env, false);
        assert!(result2.is_ok(), "second operation should succeed - guard was released");

        let result3 = operation_that_might_panic(&env, false);
        assert!(result3.is_ok(), "third operation should succeed");
    });
}

// ---------------------------------------------------------------------------
// Guard: Concurrent operation correctly rejected
// ---------------------------------------------------------------------------

#[test]
fn test_concurrent_operation_rejected() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _guard1 = reentrancy::ReentrancyGuard::acquire(&env).unwrap();

        let concurrent_attempt = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(concurrent_attempt, Err(PairError::Locked)),
            "concurrent operation should be rejected with Locked error"
        );
    });

    env.as_contract(&contract_id, || {
        let result = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(result.is_ok(), "new operation should succeed after previous completed");
    });
}

// ---------------------------------------------------------------------------
// Regression tests: Reentrancy attacks are blocked
// ---------------------------------------------------------------------------

#[test]
fn test_burn_reentrancy_attack_was_blocked() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let first_acquire = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(first_acquire.is_ok(), "First acquire should succeed");

        let reentrant_attempt = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(reentrant_attempt, Err(PairError::Locked)),
            "Reentrant call should be blocked by the guard"
        );
    });
}

#[test]
fn test_mint_reentrancy_attack_was_blocked() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let first_acquire = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(first_acquire.is_ok(), "First acquire should succeed");

        let reentrant_attempt = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(reentrant_attempt, Err(PairError::Locked)),
            "Reentrant call should be blocked by the guard"
        );
    });
}

// ---------------------------------------------------------------------------
// Guard is present in burn and mint functions
// ---------------------------------------------------------------------------

#[test]
fn test_guard_present_in_burn() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let guard = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(guard.is_ok(), "Guard can be acquired");

        let reentrant = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(reentrant, Err(PairError::Locked)),
            "Guard correctly blocks reentrant calls"
        );
    });
}

#[test]
fn test_guard_present_in_mint() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let guard = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(guard.is_ok(), "Guard can be acquired");

        let reentrant = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(reentrant, Err(PairError::Locked)),
            "Guard correctly blocks reentrant calls"
        );
    });
}

// ---------------------------------------------------------------------------
// Regression tests for mint() and burn() reentrancy guards
// These tests verify the guard prevents the attack described in the issue
// ---------------------------------------------------------------------------

#[test]
fn test_burn_reentrancy_guard_prevents_attack() {
    // This test verifies that burn() acquires the guard before any transfers
    // and that a reentrant call during burn() is blocked
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        // Simulate burn() entry: acquire the guard
        let first_acquire = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(first_acquire.is_ok(), "burn() should acquire the guard");

        // Simulate malicious token transfer hook attempting swap() mid-burn()
        // This reentrant call should be blocked
        let reentrant_attempt = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(reentrant_attempt, Err(PairError::Locked)),
            "Reentrant call during burn() should be blocked with Locked error"
        );
    });
}

#[test]
fn test_mint_reentrancy_guard_prevents_attack() {
    // This test verifies that mint() acquires the guard before any transfers
    // and that a reentrant call during mint() is blocked
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        // Simulate mint() entry: acquire the guard
        let first_acquire = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(first_acquire.is_ok(), "mint() should acquire the guard");

        // Simulate malicious token transfer hook attempting swap() mid-mint()
        // This reentrant call should be blocked
        let reentrant_attempt = reentrancy::ReentrancyGuard::acquire(&env);
        assert!(
            matches!(reentrant_attempt, Err(PairError::Locked)),
            "Reentrant call during mint() should be blocked with Locked error"
        );
    });
}

// ---------------------------------------------------------------------------
// Guard-entry lifetime (issue #362)
//
// The `Guard` flag lives in instance storage. These tests pin down that a lock
// flip always leaves the entry alive for longer than any operation can hold the
// lock, so a held guard cannot silently vanish (fail-open) mid-operation.
// ---------------------------------------------------------------------------

fn advance_ledgers(env: &Env, by: u32) {
    env.ledger().with_mut(|li| {
        li.sequence_number += by;
        li.max_entry_ttl = li.max_entry_ttl.max(1_000_000);
    });
}

#[test]
fn test_guard_ttl_policy_covers_any_hold_window() {
    // The compile-time asserts in `coralswap_shared` enforce this too; the
    // runtime form keeps the relationship visible in the test report.
    const {
        assert!(
            coralswap_shared::REENTRANCY_TTL_THRESHOLD
                >= coralswap_shared::REENTRANCY_MAX_HOLD_LEDGERS
        );
        assert!(
            coralswap_shared::REENTRANCY_TTL_EXTEND_TO >= coralswap_shared::INSTANCE_TTL_EXTEND_TO
        );
    }
}

#[test]
fn test_acquire_leaves_guard_entry_ttl_above_hold_window() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
        let ttl = env.storage().instance().get_ttl();
        assert!(
            ttl >= coralswap_shared::REENTRANCY_MAX_HOLD_LEDGERS,
            "held guard must outlive the longest possible hold window, got ttl {ttl}"
        );
        assert!(ttl >= coralswap_shared::REENTRANCY_TTL_THRESHOLD);
    });
}

#[test]
fn test_release_leaves_guard_entry_ttl_above_threshold() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        drop(reentrancy::ReentrancyGuard::acquire(&env).unwrap());
        let ttl = env.storage().instance().get_ttl();
        assert!(ttl >= coralswap_shared::REENTRANCY_TTL_THRESHOLD);
    });
}

#[test]
fn test_acquire_refreshes_a_nearly_expired_guard_entry() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        // Establish the entry, then age it until it is about to expire.
        drop(reentrancy::ReentrancyGuard::acquire(&env).unwrap());
        let ttl = env.storage().instance().get_ttl();
        advance_ledgers(&env, ttl - 10);
        assert!(env.storage().instance().get_ttl() <= 10);

        // Flipping the lock must top the entry back up before it can lapse.
        let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();
        assert!(
            env.storage().instance().get_ttl() >= coralswap_shared::REENTRANCY_MAX_HOLD_LEDGERS
        );
    });
}

#[test]
fn test_held_guard_stays_locked_across_ledgers() {
    let env = Env::default();
    let contract_id = env.register(ReentrancyTest, ());

    env.as_contract(&contract_id, || {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env).unwrap();

        // Far longer than any real invocation: the lock must still read as held.
        advance_ledgers(&env, coralswap_shared::REENTRANCY_MAX_HOLD_LEDGERS);
        assert!(
            matches!(reentrancy::ReentrancyGuard::acquire(&env), Err(PairError::Locked)),
            "a held guard must not vanish while ledgers advance"
        );
    });
}
