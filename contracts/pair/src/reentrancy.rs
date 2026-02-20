use soroban_sdk::Env;

use crate::{
    errors::PairError,
    storage::{get_reentrancy_guard, set_reentrancy_guard, ReentrancyGuard},
};

/// Acquires the reentrancy lock. Reverts with `Locked` if already held.
///
/// Called at the start of `execute_flash_loan` to prevent recursive flash
/// loans. Because Soroban rolls back all state on a failed invocation,
/// the lock is automatically cleared if the outer call reverts.
pub fn acquire(env: &Env) -> Result<(), PairError> {
    let guard = get_reentrancy_guard(env);
    if guard.locked {
        return Err(PairError::Locked);
    }
    set_reentrancy_guard(env, &ReentrancyGuard { locked: true });
    Ok(())
}

/// Releases the reentrancy lock after all flash loan checks pass.
///
/// Only called on the happy path; error paths rely on Soroban's atomic
/// state rollback to reset the lock automatically.
pub fn release(env: &Env) {
    set_reentrancy_guard(env, &ReentrancyGuard { locked: false });
}
