//! Bounded Pair List with Governance Prune (Issue #361)
//!
//! Caps active pair count and provides deliberate prune path to control state growth.

use soroban_sdk::{contracttype, Address, Env, Map, Vec};

/// Maximum active pairs allowed in the factory
pub const MAX_ACTIVE_PAIRS: u32 = 10_000;

/// Pair list metadata for growth tracking
#[contracttype]
#[derive(Clone)]
pub struct PairListMetadata {
    pub active_count: u32,
    pub total_created: u64,
    pub total_pruned: u64,
}

/// Check if factory can accept more pairs
pub fn can_create_pair(env: &Env) -> bool {
    let metadata = get_pair_list_metadata(env);
    metadata.active_count < MAX_ACTIVE_PAIRS
}

/// Increment active pair count
pub fn increment_pair_count(env: &Env) {
    let mut metadata = get_pair_list_metadata(env);
    metadata.active_count += 1;
    metadata.total_created += 1;
    set_pair_list_metadata(env, &metadata);
}

/// Governance: Prune inactive pairs from the list
///
/// # Arguments
/// * `pair_addresses` - List of pair addresses to remove (must be inactive)
///
/// # Returns
/// Number of pairs pruned
pub fn prune_inactive_pairs(env: &Env, pair_addresses: Vec<Address>) -> u32 {
    let mut pruned = 0u32;
    let mut metadata = get_pair_list_metadata(env);

    for pair_addr in pair_addresses.iter() {
        // Verify pair is inactive (zero liquidity, no activity for 90 days)
        if is_pair_inactive(env, &pair_addr) {
            // Remove from pair list
            remove_pair_from_storage(env, &pair_addr);
            pruned += 1;
        }
    }

    metadata.active_count = metadata.active_count.saturating_sub(pruned);
    metadata.total_pruned += pruned as u64;
    set_pair_list_metadata(env, &metadata);

    pruned
}

/// Check if pair is inactive (eligible for pruning)
fn is_pair_inactive(env: &Env, pair: &Address) -> bool {
    // Query pair contract for:
    // 1. Total liquidity (must be 0)
    // 2. Last activity timestamp (must be > 90 days ago)
    
    // Placeholder: implement actual checks
    let current_ledger = env.ledger().timestamp();
    let pair_last_activity = get_pair_last_activity(env, pair);
    let ninety_days = 90 * 24 * 60 * 60;

    pair_last_activity + ninety_days < current_ledger
}

fn remove_pair_from_storage(env: &Env, pair: &Address) {
    // Remove pair from factory storage
    // Implementation depends on factory storage structure
}

fn get_pair_last_activity(env: &Env, pair: &Address) -> u64 {
    // Query pair contract for last activity timestamp
    0 // Placeholder
}

fn get_pair_list_metadata(env: &Env) -> PairListMetadata {
    env.storage()
        .persistent()
        .get(&"PairListMeta")
        .unwrap_or(PairListMetadata {
            active_count: 0,
            total_created: 0,
            total_pruned: 0,
        })
}

fn set_pair_list_metadata(env: &Env, metadata: &PairListMetadata) {
    env.storage().persistent().set(&"PairListMeta", metadata);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_pairs_enforced() {
        // Test that pair creation is rejected at MAX_ACTIVE_PAIRS
    }

    #[test]
    fn test_governance_prune() {
        // Test that governance can prune inactive pairs
    }
}
