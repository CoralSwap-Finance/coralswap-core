use soroban_sdk::Env;
use crate::errors::PairError;
use crate::storage::{get_reentrancy_guard, set_reentrancy_guard, ReentrancyGuard};

/// Acquires the reentrancy lock. Reverts with Locked if already held.
pub fn acquire(env: &Env) -> Result<(), PairError> {
    let guard = get_reentrancy_guard(env);
    if guard.locked {
        return Err(PairError::Locked);
    }
    set_reentrancy_guard(env, &ReentrancyGuard { locked: true });
    Ok(())
}

/// Releases the reentrancy lock.
pub fn release(env: &Env) {
    set_reentrancy_guard(env, &ReentrancyGuard { locked: false });
}
