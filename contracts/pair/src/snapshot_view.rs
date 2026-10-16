//! Historical Snapshot View for TWAP Consumers (Issue #359)
//!
//! Extended view returning reserves + block_timestamp_last + cumulative prices
//! for TWAP (Time-Weighted Average Price) consumers.

use soroban_sdk::{contracttype, Env};

/// Comprehensive snapshot for TWAP consumers
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairSnapshot {
    /// Current reserve of token0
    pub reserve0: i128,
    /// Current reserve of token1
    pub reserve1: i128,
    /// Last block timestamp when reserves were updated
    pub block_timestamp_last: u64,
    /// Cumulative price for token0 (token1 / token0)
    pub price0_cumulative: u128,
    /// Cumulative price for token1 (token0 / token1)
    pub price1_cumulative: u128,
    /// Current K (constant product: reserve0 * reserve1)
    pub k_last: i128,
}

/// Get comprehensive snapshot for TWAP calculations
///
/// This view returns all data needed for TWAP computation in one call,
/// eliminating the need for consumers to stitch together multiple views.
///
/// # Returns
/// PairSnapshot with reserves, timestamp, and cumulative prices
///
/// # Usage Example
/// ```rust
/// // Consumer fetches snapshot
/// let snapshot = pair.get_snapshot();
///
/// // Calculate TWAP over period
/// let price_avg = (snapshot.price0_cumulative - prev_price0_cumulative)
///     / (snapshot.block_timestamp_last - prev_timestamp);
/// ```
pub fn get_snapshot(env: &Env) -> PairSnapshot {
    // Load reserves
    let (reserve0, reserve1, block_timestamp_last) = get_reserves_with_timestamp(env);

    // Load cumulative prices from oracle state
    let (price0_cumulative, price1_cumulative) = get_cumulative_prices(env);

    // Calculate current K
    let k_last = reserve0.checked_mul(reserve1).unwrap_or(0);

    PairSnapshot {
        reserve0,
        reserve1,
        block_timestamp_last,
        price0_cumulative,
        price1_cumulative,
        k_last,
    }
}

/// Get reserves with last update timestamp
fn get_reserves_with_timestamp(env: &Env) -> (i128, i128, u64) {
    // Load from pair storage
    // Placeholder: replace with actual storage access
    let reserve0 = env.storage().persistent().get(&"Reserve0").unwrap_or(0);
    let reserve1 = env.storage().persistent().get(&"Reserve1").unwrap_or(0);
    let timestamp = env.storage().persistent().get(&"BlockTimestampLast").unwrap_or(0);

    (reserve0, reserve1, timestamp)
}

/// Get cumulative prices from oracle state
fn get_cumulative_prices(env: &Env) -> (u128, u128) {
    // Load from oracle storage
    // Placeholder: replace with actual oracle access
    let price0_cum = env.storage().persistent().get(&"Price0Cumulative").unwrap_or(0);
    let price1_cum = env.storage().persistent().get(&"Price1Cumulative").unwrap_or(0);

    (price0_cum, price1_cum)
}

/// Legacy get_reserves for backward compatibility
///
/// Returns only (reserve0, reserve1) without timestamp or cumulative prices.
/// Kept for existing integrations.
pub fn get_reserves(env: &Env) -> (i128, i128) {
    let (reserve0, reserve1, _) = get_reserves_with_timestamp(env);
    (reserve0, reserve1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_consistency() {
        let env = Env::default();
        
        // Setup mock state
        env.storage().persistent().set(&"Reserve0", &1000000i128);
        env.storage().persistent().set(&"Reserve1", &2000000i128);
        env.storage().persistent().set(&"BlockTimestampLast", &12345u64);
        env.storage().persistent().set(&"Price0Cumulative", &100000u128);
        env.storage().persistent().set(&"Price1Cumulative", &200000u128);

        let snapshot = get_snapshot(&env);

        assert_eq!(snapshot.reserve0, 1000000);
        assert_eq!(snapshot.reserve1, 2000000);
        assert_eq!(snapshot.block_timestamp_last, 12345);
        assert_eq!(snapshot.price0_cumulative, 100000);
        assert_eq!(snapshot.price1_cumulative, 200000);
        assert_eq!(snapshot.k_last, 1000000 * 2000000);
    }

    #[test]
    fn test_get_reserves_compat() {
        let env = Env::default();
        
        env.storage().persistent().set(&"Reserve0", &1000i128);
        env.storage().persistent().set(&"Reserve1", &2000i128);

        let (r0, r1) = get_reserves(&env);
        assert_eq!(r0, 1000);
        assert_eq!(r1, 2000);
    }
}
