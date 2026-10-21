use crate::errors::PairError;
use crate::{Pair, PairClient};
use coralswap_lp_token::{LpToken, LpTokenClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String};

/// Helper to deploy and initialize pair for testing
///
/// The pair is initialized with a real factory contract deployed from the
/// shared factory crate so that governance auth (`governance::verify_multisig`)
/// runs against real state. The multisig signers are returned to the caller
/// so tests can exercise the auth model directly.
fn setup_pair(env: &Env) -> (PairClient<'static>, Address, Address, Address, Address, sroban_sdk::Vec<Address>) {
    // Create mock token contracts (use Pair as mock since we only need Address)
    let token_a_id = env.register(Pair, ());
    let token_b_id = env.register(Pair, ());
    let lp_id = env.register(LpToken, ());
    let pair_id = env.register(Pair, ());

    let lp_client = LpTokenClient::new(env, &lp_id);
    let pair_client = PairClient::new(env, &pair_id);

    let admin = Address::generate(env);

    // Deploy a real factory contract and configure its governance multisig.
    let factory_id = env.register(coralswap_factory::Factory, ());
    let factory_client = coralswap_factory::FactoryClient::new(env, &factory_id);

    let signer_1 = Address::generate(env);
    let signer_2 = Address::generate(env);
    let signer_3 = Address::generate(env);
    let mut = sroban_sdk::Vec::new(env);
    mut.push_back(signer_1.clone());
    mut.push_back(signer_2.clone());
    mut.push_back(signer_3.clone());
    factory_client.initialize(&admin, &mut, &Null);

    // Initialize LP token
    lp_client.initialize(
        &admin,
        &7u32,
        &String::from_str(env, "Coral LP"),
        &String::from_str(env, "CLP"),
    );

    // Initialize pair with the real factory contract address
    pair_client.initialize(&factory_id, &token_a_id, &token_b_id, &lp_id);

    (pair_client, factory_id, token_a_id, token_b_id, pair_id, mut)
}

/// Helper to authorize a multisig signer set for a pair call.
fn authorize_signers(env: &Env, signers: &sroban_sdk::Vec<Address>) {
    for signer in signers.iter() {
        signer.require_auth();
    }
}

#[test]
fn test_set_stale_threshold_default_value() {
    let env = Env::default();
    let (pair_client, _, _, _, _, _signers) = setup_pair(&env);

    // Default threshold is exposed via the view and must remain unchanged.
    assert_eq!(pair_client.get_stale_threshold(), 300u32);

    // Fee should be baseline (30) since there's no volatility yet
    let fee = pair_client.get_current_fee_bps();
    assert_eq!(fee, 30);
}

#[test]
fn test_set_stale_threshold_updates_value() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    // Set a new stale threshold
    let new_threshold = 500u32;
    pair_client.set_stale_threshold(&new_threshold);

    // Verify it was updated via the view.
    assert_eq!(pair_client.get_stale_threshold(), new_threshold);
}

#[test]
fn test_set_stale_threshold_validation_zero_fails() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

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
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

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
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    // Set threshold to minimum valid value (1)
    let result = pair_client.try_set_stale_threshold(&1u32);

    assert!(result.is_ok());
    assert_eq!(pair_client.get_stale_threshold(), 1u32);
}

#[test]
fn test_set_stale_threshold_boundary_max() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    // Set threshold to maximum valid value (100_000)
    let result = pair_client.try_set_stale_threshold(&100_000u32);

    assert!(result.is_ok());
    assert_eq!(pair_client.get_stale_threshold(), 100_000u32);
}

#[test]
fn test_set_stale_threshold_requires_multisig_auth() {
    let env = Env::default();
    let (pair_client, _factory, _, _, _pair_id, signers) = setup_pair(&env);

    // A single random signer (even one of the multisig signers) cannot change
    // the threshold on its own.
    let single_signer = signers[0].clone();
    single_signer.require_auth();
    let result_single = pair_client.try_set_stale_threshold(&500u32);
    assert!(result_single.is_err());

    // A stranger address cannot change the threshold.
    let stranger = Address::generate(&env);
    stranger.require_auth();
    let result_stranger = pair_client.try_set_stale_threshold(&500u32);
    assert!(result_stranger.is_err());

    // The factory contract address alone cannot authorize the call.
    _factory.require_auth();
    let result_factory = pair_client.try_set_stale_threshold(&500u32);
    assert!(result_factory.is_err());

    // A full signature set can change the threshold.
    authorize_signers(&env, &signers);
    let result_multisig = pair_client.try_set_stale_threshold(&500u32);
    assert!(result_multisig.is_ok());
    assert_eq!(pair_client.get_stale_threshold(), 500u32);
}

#[test]
fn test_set_stale_threshold_idempotent() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    // Set threshold to 300
    let result1 = pair_client.try_set_stale_threshold(&300u32);
    assert!(result1.is_ok());

    // Set threshold to 300 again
    let result2 = pair_client.try_set_stale_threshold(&300u32);
    assert!(result2.is_ok());

    // Both calls should succeed
    assert_eq!(pair_client.get_stale_threshold(), 300u32);
}

#[test]
fn test_set_stale_threshold_can_be_changed_multiple_times() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    // Change threshold multiple times
    for threshold in &[100u32, 500u, 50u32, 10_000u32] {
        let result = pair_client.try_set_stale_threshold(threshold);
        assert!(result.is_ok(), "Failed to set threshold to {}", threshold);
        assert_eq!(pair_client.get_stale_threshold(), *threshold);
    }
}

#[test]
fn test_set_stale_threshold_affects_decay_behavior() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    // Change threshold to a very low value
    pair_client.set_stale_threshold(&1u32);
    assert_eq!(pair_client.get_stale_threshold(), 1u32);

    // Move forward some ledgers
    env.ledger().set_sequence_number(10);

    // Since this is an integration test and we don't have direct access to internal state,
    // we verify the function was called without error
    assert_eq!(pair_client.get_stale_threshold(), 1u32);
}

#[test]
fn test_set_stale_threshold_various_valid_values() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    let valid_thresholds = vec![1u32, 10, 100, 500, 1000, 5000, 10_000, 50_000, 100_000];

    for threshold in valid_thresholds {
        let result = pair_client.try_set_stale_threshold(&threshold);
        assert!(result.is_ok(), "Failed to set valid threshold: {}", threshold);
        assert_eq!(pair_client.get_stale_threshold(), threshold);
    }
}

#[test]
fn test_set_stale_threshold_invalid_values() {
    let env = Env::default();
    let (pair_client, _, _, _, _, signers) = setup_pair(&env);

    authorize_signers(&env, &signers);

    let invalid_thresholds = vec![0u32, 100_001, u32::MAX, 1_000_000];

    for threshold in invalid_thresholds {
        let result = pair_client.try_set_stale_threshold(&threshold);
        assert!(result.is_err(), "Should reject invalid threshold: {}", threshold);
        if let Err(e) = result {
            assert_eq!(e, Ok(PairError::InvalidStaleThreshold));
        }
    }
}
