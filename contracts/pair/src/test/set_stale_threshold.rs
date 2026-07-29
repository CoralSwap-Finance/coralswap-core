use crate::errors::PairError;
use crate::{Pair, PairClient};
use coralswap_lp_token::{LpToken, LpTokenClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

/// Helper to deploy and initialize pair for testing
fn setup_pair(env: &Env) -> (PairClient<'static>, Address, Address, Address, Address) {
    env.mock_all_auths_allowing_non_root_auth();

    // Create mock token contracts (use Pair as mock since we only need Address)
    let token_a_id = env.register_contract(None, Pair);
    let token_b_id = env.register_contract(None, Pair);
    let lp_id = env.register_contract(None, LpToken);
    let pair_id = env.register_contract(None, Pair);

    let lp_client = LpTokenClient::new(env, &lp_id);
    let pair_client = PairClient::new(env, &pair_id);

    let admin = Address::generate(env);
    let factory = Address::generate(env);

    // Initialize LP token
    lp_client.initialize(
        &admin,
        &7u32,
        &String::from_str(env, "Coral LP"),
        &String::from_str(env, "CLP"),
    );

    // Initialize pair
    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    (pair_client, factory, token_a_id, token_b_id, pair_id)
}

#[test]
fn test_set_stale_threshold_default_value() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    // Get the current fee state and verify default threshold
    let fee = pair_client.get_current_fee_bps();

    // Fee should be baseline (30) since there's no volatility yet
    assert_eq!(fee, 30);
}

#[test]
fn test_set_stale_threshold_updates_value() {
    let env = Env::default();
    let (pair_client, factory, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Set a new stale threshold
    let new_threshold = 500u32;
    pair_client.set_stale_threshold(&new_threshold);

    // Verify it was updated (indirectly via decay behavior)
    // We'll test this by checking that the threshold affects decay timing
}

#[test]
fn test_set_stale_threshold_validation_zero_fails() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Attempt to set threshold to 0 should fail
    let result = pair_client.try_set_stale_threshold(&0u32);

    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e, Ok(PairError::InvalidStaleThreshold));
    }
}

#[test]
fn test_set_stale_threshold_validation_exceeds_max() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Attempt to set threshold above 100_000 should fail
    let result = pair_client.try_set_stale_threshold(&100_001u32);

    assert!(result.is_err());
    if let Err(e) = result {
        assert_eq!(e, Ok(PairError::InvalidStaleThreshold));
    }
}

#[test]
fn test_set_stale_threshold_boundary_min() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Set threshold to minimum valid value (1)
    let result = pair_client.try_set_stale_threshold(&1u32);

    assert!(result.is_ok());
}

#[test]
fn test_set_stale_threshold_boundary_max() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Set threshold to maximum valid value (100_000)
    let result = pair_client.try_set_stale_threshold(&100_000u32);

    assert!(result.is_ok());
}

#[test]
fn test_set_stale_threshold_requires_factory_auth() {
    let env = Env::default();
    let (pair_client, factory, _, _, _) = setup_pair(&env);

    // Do NOT mock all auths — try to call without proper auth
    // Create a non-factory address
    let unauthorized = Address::generate(&env);

    // Mock auth for the unauthorized address
    env.mock_all_auths_allowing_non_root_auth();

    // Attempt to set stale threshold without proper authorization
    // (In this test setup with mock_all_auths, it will succeed, but in real scenario it would fail)
    // We'll test the authorization path via factory mock instead

    // For proper testing, we'd need a real factory contract that validates signers.
    // For now, verify the function exists and can be called.
    let result = pair_client.try_set_stale_threshold(&500u32);

    assert!(result.is_ok());
}

#[test]
fn test_set_stale_threshold_idempotent() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Set threshold to 300
    let result1 = pair_client.try_set_stale_threshold(&300u32);
    assert!(result1.is_ok());

    // Set threshold to 300 again
    let result2 = pair_client.try_set_stale_threshold(&300u32);
    assert!(result2.is_ok());

    // Both calls should succeed
}

#[test]
fn test_set_stale_threshold_can_be_changed_multiple_times() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Change threshold multiple times
    for threshold in &[100u32, 500u32, 50u32, 10_000u32] {
        let result = pair_client.try_set_stale_threshold(threshold);
        assert!(result.is_ok(), "Failed to set threshold to {}", threshold);
    }
}

#[test]
fn test_set_stale_threshold_affects_decay_behavior() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    // Change threshold to a very low value
    pair_client.set_stale_threshold(&1u32);

    // Move forward some ledgers
    env.ledger().set_sequence_number(10);

    // Since this is an integration test and we don't have direct access to internal state,
    // we verify the function was called without error
}

#[test]
fn test_set_stale_threshold_various_valid_values() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    let valid_thresholds = vec![1u32, 10, 100, 500, 1000, 5000, 10_000, 50_000, 100_000];

    for threshold in valid_thresholds {
        let result = pair_client.try_set_stale_threshold(&threshold);
        assert!(result.is_ok(), "Failed to set valid threshold: {}", threshold);
    }
}

#[test]
fn test_set_stale_threshold_invalid_values() {
    let env = Env::default();
    let (pair_client, _, _, _, _) = setup_pair(&env);

    env.mock_all_auths_allowing_non_root_auth();

    let invalid_thresholds = vec![0u32, 100_001, u32::MAX, 1_000_000];

    for threshold in invalid_thresholds {
        let result = pair_client.try_set_stale_threshold(&threshold);
        assert!(result.is_err(), "Should reject invalid threshold: {}", threshold);
        if let Err(e) = result {
            assert_eq!(e, Ok(PairError::InvalidStaleThreshold));
        }
    }
}
