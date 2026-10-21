//! Path Length Guard Against Gas Griefing (Issue #360)
//! Sandwich Attack Protection (Issue #361)
//!
//! Central MAX_PATH_LENGTH check for all path-accepting entry points,
//! plus same-ledger swap mitigation and strict slippage enforcement helpers.

use soroban_sdk::{Address, Env, Vec};

use crate::errors::RouterError;

/// Maximum allowed hops in a swap path (gas griefing protection)
pub const MAX_PATH_LENGTH: usize = 3;

/// Storage key prefix for per-(user, pool) last swap ledger tracking.
///
/// Used to mitigate same-ledger sandwich attacks: a single actor cannot
/// perform more than one swap against the same pool within the same ledger.
const LAST_SWAP_LEDGER_PREFIX: &str = "last_swap_ledger";

/// Validate swap path length to prevent gas griefing
///
/// # Arguments
/// * `path` - Token path for swap
///
/// # Returns
/// Ok(()) if path is valid, Err if path too long
///
/// # Gas Griefing Protection
/// Each hop invokes multiple cross-contract calls:
/// - pair.swap() call
/// - token transfer callbacks
/// - reserve updates
/// - oracle updates
/// - event emissions
///
/// With 3 hops, this is ~9-12 cross-contract calls.
/// Unbounded paths could grief caller with excessive gas consumption.
pub fn validate_path_length(env: &Env, path: &Vec<Address>) -> Result<(), RouterError> {
    if path.len() < 2 {
        return Err(RouterError::InvalidPath);
    }

    if path.len() - 1 > MAX_PATH_LENGTH {
        return Err(RouterError::PathTooLong);
    }

    Ok(())
}

/// Validate path and compute hop count
///
/// # Returns
/// (hop_count, gas_estimate)
pub fn validate_and_estimate_gas(
    env: &Env,
    path: &Vec<Address>,
) -> Result<(usize, u64), RouterError> {
    validate_path_length(env, path)?;

    let hop_count = path.len() - 1;
    
    // Estimate gas per hop (conservative):
    // - pair.get_reserves(): ~500 gas
    // - pair.swap(): ~5000 gas
    // - token transfers: ~3000 gas each (2x per hop)
    // - callbacks: ~1000 gas
    // Total per hop: ~12,500 gas
    const GAS_PER_HOP: u64 = 12_500;
    let estimated_gas = GAS_PER_HOP * hop_count as u64;

    Ok((hop_count, estimated_gas))
}

/// Enforce strict `amount_out_minimum` slippage bounds.
///
/// # Arguments
/// * `amount_out` - Actual computed output amount
/// * `amount_out_minimum` - Minimum acceptable output (slippage bound)
///
/// # Returns
/// Ok(()) if `amount_out >= amount_out_minimum`, else `SlippageExceeded`.
///
/// # Sandwich Attack Protection
/// A non-zero minimum is required so that front-running cannot silently
/// degrade the user's execution price. A zero minimum is rejected because
/// it provides no slippage protection at all.
pub fn enforce_amount_out_minimum(
    amount_out: i128,
    amount_out_minimum: i128,
) -> Result<(), RouterError> {
    if amount_out_minimum <= 0 {
        return Err(RouterError::SlippageExceeded);
    }

    if amount_out < amount_out_minimum {
        return Err(RouterError::SlippageExceeded);
    }

    Ok(())
}

/// Enforce a strict deadline check.
///
/// # Arguments
/// * `env` - Contract environment (for current ledger timestamp)
/// * `deadline` - Unix timestamp after which the swap is invalid
///
/// # Returns
/// Ok(()) if `deadline >= current ledger timestamp`, else `DeadlineExpired`.
///
/// # Sandwich Attack Protection
/// A stale transaction held by a searcher and included later is rejected,
/// preventing delayed back-running against an outdated price.
pub fn enforce_deadline(env: &Env, deadline: u64) -> Result<(), RouterError> {
    let now = env.ledger().timestamp();
    if deadline < now {
        return Err(RouterError::DeadlineExpired);
    }
    Ok(())
}

/// Record and enforce same-ledger swap restriction per (user, pool).
///
/// # Arguments
/// * `env` - Contract environment
/// * `user` - The actor performing the swap
/// * `pool` - The pool being swapped against
///
/// # Returns
/// Ok(()) if this is the first swap by `user` against `pool` in this ledger,
/// else `SameLedgerSwap`.
///
/// # Sandwich Attack Protection
/// Soroban exposes `env.ledger().sequence()` which is stable within a ledger.
/// By storing the last ledger in which `user` swapped against `pool`, we can
/// reject a second swap from the same actor in the same ledger. This blocks
/// the classic front-run + back-run pattern executed by a single account
/// within one ledger. (Distinct accounts controlled by the same attacker are
/// out of scope for on-chain detection.)
pub fn enforce_same_ledger_guard(
    env: &Env,
    user: &Address,
    pool: &Address,
) -> Result<(), RouterError> {
    let current_ledger = env.ledger().sequence();
    let key = (LAST_SWAP_LEDGER_PREFIX, user.clone(), pool.clone());

    if let Some(last_ledger) = env.storage().persistent().get::<_, u32>(&key) {
        if last_ledger == current_ledger {
            return Err(RouterError::SameLedgerSwap);
        }
    }

    env.storage().persistent().set(&key, &current_ledger);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_path_length_validation() {
        let env = Env::default();

        // Valid 2-token path (1 hop)
        let path = Vec::from_array(&env, [
            Address::generate(&env),
            Address::generate(&env),
        ]);
        assert!(validate_path_length(&env, &path).is_ok());

        // Valid 4-token path (3 hops)
        let path = Vec::from_array(&env, [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ]);
        assert!(validate_path_length(&env, &path).is_ok());

        // Invalid: 5-token path (4 hops - exceeds MAX_PATH_LENGTH)
        let path = Vec::from_array(&env, [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ]);
        assert!(validate_path_length(&env, &path).is_err());
    }

    #[test]
    fn test_gas_estimation() {
        let env = Env::default();

        let path = Vec::from_array(&env, [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ]);

        let (hops, gas) = validate_and_estimate_gas(&env, &path).unwrap();
        assert_eq!(hops, 2);
        assert_eq!(gas, 25_000); // 2 hops * 12,500 gas
    }

    #[test]
    fn test_worst_case_gas_bounded() {
        let env = Env::default();

        // Worst case: 3 hops (MAX_PATH_LENGTH)
        let path = Vec::from_array(&env, [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ]);

        let (hops, gas) = validate_and_estimate_gas(&env, &path).unwrap();
        assert_eq!(hops, 3);
        assert_eq!(gas, 37_500); // 3 hops * 12,500 gas
        
        // Assert gas is bounded (< 50k gas)
        assert!(gas < 50_000);
    }

    #[test]
    fn test_amount_out_minimum_strict_enforcement() {
        // Exact minimum is accepted
        assert!(enforce_amount_out_minimum(100, 100).is_ok());
        // Above minimum is accepted
        assert!(enforce_amount_out_minimum(101, 100).is_ok());
        // Below minimum is rejected
        assert!(enforce_amount_out_minimum(99, 100).is_err());
        // Zero minimum is rejected (no slippage protection)
        assert!(enforce_amount_out_minimum(100, 0).is_err());
    }

    #[test]
    fn test_deadline_enforcement() {
        let env = Env::default();
        // Deadline in the future is accepted
        assert!(enforce_deadline(&env, env.ledger().timestamp() + 100).is_ok());
        // Deadline in the past is rejected
        assert!(enforce_deadline(&env, 0).is_err());
    }

    #[test]
    fn test_same_ledger_guard_blocks_second_swap() {
        let env = Env::default();
        let user = Address::generate(&env);
        let pool = Address::generate(&env);

        // First swap in this ledger succeeds
        assert!(enforce_same_ledger_guard(&env, &user, &pool).is_ok());
        // Second swap by same user against same pool in same ledger is blocked
        assert!(enforce_same_ledger_guard(&env, &user, &pool).is_err());

        // A different pool is unaffected
        let other_pool = Address::generate(&env);
        assert!(enforce_same_ledger_guard(&env, &user, &other_pool).is_ok());
    }

    #[test]
    fn test_sandwich_attack_mitigated() {
        let env = Env::default();
        let attacker = Address::generate(&env);
        let victim = Address::generate(&env);
        let pool = Address::generate(&env);

        // Attacker front-runs: first swap in the ledger succeeds.
        assert!(enforce_same_ledger_guard(&env, &attacker, &pool).is_ok());

        // Victim's swap executes (different actor, allowed).
        assert!(enforce_same_ledger_guard(&env, &victim, &pool).is_ok());

        // Attacker back-runs in the same ledger: blocked by the guard.
        assert!(enforce_same_ledger_guard(&env, &attacker, &pool).is_err());

        // Additionally, strict slippage enforcement rejects a degraded output.
        // Victim expected 1000 out; sandwich degraded it to 900.
        assert!(enforce_amount_out_minimum(900, 1000).is_err());
    }
}
