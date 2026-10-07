//! Path Length Guard Against Gas Griefing (Issue #360)
//!
//! Central MAX_PATH_LENGTH check for all path-accepting entry points.

use soroban_sdk::{Address, Env, Vec};

use crate::errors::RouterError;

/// Maximum allowed hops in a swap path (gas griefing protection)
pub const MAX_PATH_LENGTH: usize = 3;

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
}
