//! TTL Maintenance for Read-Only Paths (Issue #358)
//!
//! Ensures no read-only path lets state expire by extending TTL on all operations.

use soroban_sdk::Env;

/// TTL extension duration (30 days in ledgers, ~5s per ledger)
const TTL_EXTENSION_LEDGERS: u32 = 518_400; // 30 days * 24h * 60m * 60s / 5s

/// Storage keys that need TTL maintenance
const STATE_KEYS: &[&str] = &[
    "FeeState",
    "OracleState", 
    "GuardState",
    "Reserve0",
    "Reserve1",
    "BlockTimestampLast",
    "Price0Cumulative",
    "Price1Cumulative",
];

/// Extend TTL for all critical state entries
///
/// Call this in:
/// - All write operations (swap, mint, burn)
/// - All view operations (get_reserves, get_snapshot)
/// - All governance operations (set_fee, set_oracle)
///
/// This ensures state doesn't expire even if only reads happen.
pub fn extend_state_ttl(env: &Env) {
    for key in STATE_KEYS {
        extend_key_ttl(env, key);
    }
}

/// Extend TTL for a specific storage key
fn extend_key_ttl(env: &Env, key: &str) {
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_EXTENSION_LEDGERS, TTL_EXTENSION_LEDGERS);
    }
}

/// Extend TTL on view operations (read-only)
///
/// Called by:
/// - get_reserves()
/// - get_snapshot()
/// - get_fee_info()
/// - get_oracle_data()
pub fn extend_ttl_on_view(env: &Env) {
    extend_state_ttl(env);
}

/// Extend TTL on write operations
///
/// Called by:
/// - swap()
/// - mint()
/// - burn()
/// - sync()
pub fn extend_ttl_on_write(env: &Env) {
    extend_state_ttl(env);
}

/// Extend TTL on governance operations
///
/// Called by:
/// - set_fee_decay()
/// - set_stale_threshold()
/// - set_oracle_config()
/// - emergency_withdraw()
pub fn extend_ttl_on_governance(env: &Env) {
    extend_state_ttl(env);
}

/// Check if any state entry is close to expiration
///
/// Returns true if any entry expires in < 7 days
pub fn is_state_approaching_expiration(env: &Env) -> bool {
    const WARNING_THRESHOLD_LEDGERS: u32 = 120_960; // 7 days

    for key in STATE_KEYS {
        if env.storage().persistent().has(key) {
            let ttl = env.storage().persistent().get_ttl(key);
            if ttl < WARNING_THRESHOLD_LEDGERS {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ttl_extension() {
        let env = Env::default();
        
        // Setup state
        env.storage().persistent().set(&"FeeState", &123);
        env.storage().persistent().set(&"Reserve0", &1000);

        // Extend TTL
        extend_state_ttl(&env);

        // Verify TTL was extended
        let ttl = env.storage().persistent().get_ttl(&"FeeState");
        assert!(ttl >= TTL_EXTENSION_LEDGERS);
    }

    #[test]
    fn test_view_extends_ttl() {
        let env = Env::default();
        
        env.storage().persistent().set(&"Reserve0", &1000);
        
        // Simulate view call
        extend_ttl_on_view(&env);

        // Verify TTL extended
        let ttl = env.storage().persistent().get_ttl(&"Reserve0");
        assert!(ttl >= TTL_EXTENSION_LEDGERS);
    }

    #[test]
    fn test_expiration_warning() {
        let env = Env::default();
        
        // Set state with short TTL
        env.storage().persistent().set(&"FeeState", &123);
        env.storage().persistent().extend_ttl(&"FeeState", 100_000, 100_000); // < 7 days

        // Should trigger warning
        assert!(is_state_approaching_expiration(&env));
    }

    #[test]
    fn test_read_only_path_maintenance() {
        let env = Env::default();
        
        // Setup state
        env.storage().persistent().set(&"OracleState", &456);
        
        // Simulate read-only governance view
        extend_ttl_on_governance(&env);

        // Verify TTL extended even on read-only path
        let ttl = env.storage().persistent().get_ttl(&"OracleState");
        assert!(ttl >= TTL_EXTENSION_LEDGERS);
    }
}
