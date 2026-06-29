use soroban_sdk::Env;

use crate::{
    errors::PairError,
    storage::{get_reentrancy_guard, set_reentrancy_guard, ReentrancyGuard as StorageGuard},
};

// ---------------------------------------------------------------------------
// Internal helpers — kept private so callers must use the guard type.
// ---------------------------------------------------------------------------

fn acquire_lock(env: &Env) -> Result<(), PairError> {
    let guard = get_reentrancy_guard(env);
    if guard.locked {
        return Err(PairError::Locked);
    }
    set_reentrancy_guard(env, &StorageGuard { locked: true });
    Ok(())
}

fn release_lock(env: &Env) {
    set_reentrancy_guard(env, &StorageGuard { locked: false });
}

// ---------------------------------------------------------------------------
// RAII scope guard
// ---------------------------------------------------------------------------

/// A scope guard that holds the reentrancy lock for the duration of its
/// lifetime.  When the guard is dropped — whether on the happy path or via an
/// early `return Err(...)` — the lock is unconditionally released.
///
/// # Usage
/// ```ignore
/// let _guard = ReentrancyGuard::acquire(&env)?;
/// // ... do work that must not be re-entered ...
/// // lock released automatically when `_guard` goes out of scope
/// ```
pub struct ReentrancyGuard {
    // Raw pointer used to avoid lifetime complications in Soroban's `no_std`
    // environment.  Safety: the `Env` outlives the guard in all normal
    // Soroban call frames, and the guard is intended to be a short-lived
    // local variable within a single contract invocation.
    env_ptr: *const Env,
}

impl ReentrancyGuard {
    /// Acquires the reentrancy lock.
    ///
    /// Returns `Err(PairError::Locked)` if the lock is already held (i.e.,
    /// a re-entrant call is occurring).  On success the lock is set and will
    /// be automatically released when the returned guard is dropped.
    pub fn acquire(env: &Env) -> Result<Self, PairError> {
        acquire_lock(env)?;
        Ok(ReentrancyGuard { env_ptr: env as *const Env })
    }
}

impl Drop for ReentrancyGuard {
    fn drop(&mut self) {
        // SAFETY: `env_ptr` was obtained from a valid `&Env` reference and
        // this guard is always a stack-local within the same Soroban
        // invocation, so the `Env` is guaranteed to outlive the guard.
        let env = unsafe { &*self.env_ptr };
        release_lock(env);
    }
}
