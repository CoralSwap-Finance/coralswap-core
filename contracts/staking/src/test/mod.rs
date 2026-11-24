mod auth_matrix;

use crate::errors::StakingError;
use crate::{Staking, StakingClient, MAX_REWARD_RATE};
use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ---------------------------------------------------------------------------
// Shared test helpers
// ---------------------------------------------------------------------------

/// Register a mock SEP-41 token (Stellar Asset Contract) in the test env,
/// mint `initial_balance` to `recipient`, and return the token address.
fn make_token(env: &Env, admin: &Address, recipient: &Address, initial_balance: i128) -> Address {
    let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    if initial_balance > 0 {
        StellarAssetClient::new(env, &token_id).mint(recipient, &initial_balance);
    }
    token_id
}

// ===========================================================================
// Initialization tests
// ===========================================================================

#[test]
fn test_initialize_stores_config_correctly() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    client.initialize(&admin, &lp, &rwd, &100u64);

    let cfg = client.get_config();
    assert_eq!(cfg.admin, admin);
    assert_eq!(cfg.lp_token, lp);
    assert_eq!(cfg.reward_token, rwd);
    assert_eq!(cfg.reward_rate, 100u64);
}

#[test]
fn test_initialize_sets_total_staked_to_zero() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    client.initialize(&admin, &lp, &rwd, &0u64);

    assert_eq!(client.get_total_staked(), 0i128);
}

#[test]
fn test_initialize_rejects_double_init() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    client.initialize(&admin, &lp, &rwd, &1u64);

    let result = client.try_initialize(&admin, &lp, &rwd, &1u64);
    assert_eq!(result, Err(Ok(StakingError::AlreadyInitialized)));
}

#[test]
fn test_initialize_rejects_reward_rate_too_high() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    let result = client.try_initialize(&admin, &lp, &rwd, &(MAX_REWARD_RATE + 1));
    assert_eq!(result, Err(Ok(StakingError::RewardRateTooHigh)));
}

#[test]
fn test_initialize_accepts_max_reward_rate() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    // Exactly MAX_REWARD_RATE must succeed.
    client.initialize(&admin, &lp, &rwd, &MAX_REWARD_RATE);
    assert_eq!(client.get_config().reward_rate, MAX_REWARD_RATE);
}

// ===========================================================================
// Stake tests
// ===========================================================================

#[test]
fn test_stake_increases_user_and_total_balance() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 5_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user, &2_000i128);

    assert_eq!(client.get_stake(&user), 2_000i128);
    assert_eq!(client.get_total_staked(), 2_000i128);
}

#[test]
fn test_stake_multiple_users_accumulate_in_total() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user_a, 10_000);
    StellarAssetClient::new(&env, &lp_token).mint(&user_b, &10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user_a, &3_000i128);
    client.stake(&user_b, &7_000i128);

    assert_eq!(client.get_stake(&user_a), 3_000i128);
    assert_eq!(client.get_stake(&user_b), 7_000i128);
    assert_eq!(client.get_total_staked(), 10_000i128);
}

#[test]
fn test_stake_twice_same_user_accumulates() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user, &1_000i128);
    client.stake(&user, &2_000i128);

    assert_eq!(client.get_stake(&user), 3_000i128);
    assert_eq!(client.get_total_staked(), 3_000i128);
}

#[test]
fn test_stake_rejects_zero_amount() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 5_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    let result = client.try_stake(&user, &0i128);
    assert_eq!(result, Err(Ok(StakingError::InvalidAmount)));
}

#[test]
fn test_stake_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 5_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    let result = client.try_stake(&user, &(-1i128));
    assert_eq!(result, Err(Ok(StakingError::InvalidAmount)));
}

#[test]
fn test_stake_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let user = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    let result = client.try_stake(&user, &100i128);
    assert_eq!(result, Err(Ok(StakingError::NotInitialized)));
}

// ===========================================================================
// Unstake tests
// ===========================================================================

#[test]
fn test_unstake_decreases_user_and_total_balance() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user, &5_000i128);
    client.unstake(&user, &2_000i128);

    assert_eq!(client.get_stake(&user), 3_000i128);
    assert_eq!(client.get_total_staked(), 3_000i128);
}

#[test]
fn test_unstake_full_amount_zeroes_stake() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user, &5_000i128);
    client.unstake(&user, &5_000i128);

    assert_eq!(client.get_stake(&user), 0i128);
    assert_eq!(client.get_total_staked(), 0i128);
}

#[test]
fn test_unstake_rejects_more_than_staked() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user, &1_000i128);

    let result = client.try_unstake(&user, &1_001i128);
    assert_eq!(result, Err(Ok(StakingError::InsufficientStake)));
    // Stake must remain untouched.
    assert_eq!(client.get_stake(&user), 1_000i128);
}

#[test]
fn test_unstake_rejects_zero_amount() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    client.stake(&user, &1_000i128);

    let result = client.try_unstake(&user, &0i128);
    assert_eq!(result, Err(Ok(StakingError::InvalidAmount)));
}

#[test]
fn test_unstake_rejects_from_unstaked_user() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    // User never staked — their balance is 0, so any positive amount overdraws.
    let result = client.try_unstake(&user, &1i128);
    assert_eq!(result, Err(Ok(StakingError::InsufficientStake)));
}

#[test]
fn test_unstake_returns_tokens_to_user() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    let lp_client = TokenClient::new(&env, &lp_token);

    client.stake(&user, &5_000i128);
    let balance_after_stake = lp_client.balance(&user);

    client.unstake(&user, &3_000i128);
    let balance_after_unstake = lp_client.balance(&user);

    assert_eq!(balance_after_stake, 5_000i128); // user had 10k, staked 5k
    assert_eq!(balance_after_unstake, 8_000i128); // got 3k back
}

// ===========================================================================
// Reward claiming tests
// ===========================================================================

#[test]
fn test_claim_rewards_pays_accrued_rewards() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    // Fund the staking contract with reward tokens it can pay out.
    let reward_client = StellarAssetClient::new(&env, &reward_token);
    reward_client.mint(&staking_id, &100_000i128);

    client.stake(&user, &5_000i128);

    // Advance the ledger so rewards accrue (100 ledgers @ 100/ledger).
    env.ledger().set_sequence(100);

    let reward_before = reward_client.balance(&user);
    let paid = client.claim_rewards(&user);

    // 5_000 stake * (100 rate * 100 ledgers / 5_000 total) = 10_000 rewards.
    assert_eq!(paid, 10_000i128);
    assert_eq!(reward_client.balance(&user), reward_before + 10_000i128);

    // Claiming does not modify the user's staked LP balance.
    assert_eq!(client.get_stake(&user), 5_000i128);
}

#[test]
fn test_claim_rewards_twice_pays_only_once() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    let reward_client = StellarAssetClient::new(&env, &reward_token);
    reward_client.mint(&staking_id, &100_000i128);

    client.stake(&user, &5_000i128);
    env.ledger().set_sequence(100);

    let paid_first = client.claim_rewards(&user);
    assert_eq!(paid_first, 10_000i128);

    // Reward debt was reset, so a second claim with no new accrual pays nothing.
    let paid_second = client.claim_rewards(&user);
    assert_eq!(paid_second, 0i128);

    // User's LP stake is unchanged throughout.
    assert_eq!(client.get_stake(&user), 5_000i128);
}

#[test]
fn test_claim_rewards_skips_when_contract_has_no_reward_balance() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp_token = make_token(&env, &admin, &user, 10_000);
    let reward_token = make_token(&env, &admin, &admin, 0);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp_token, &reward_token, &100u64);

    // No reward tokens funded into the contract — empty balance must not revert.
    client.stake(&user, &5_000i128);
    env.ledger().set_sequence(100);

    let paid = client.claim_rewards(&user);
    assert_eq!(paid, 0i128);
    assert_eq!(client.get_stake(&user), 5_000i128);
}

#[test]
fn test_claim_rewards_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let user = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    let result = client.try_claim_rewards(&user);
    assert_eq!(result, Err(Ok(StakingError::NotInitialized)));
}

// ===========================================================================
// Admin: set_reward_config
// ===========================================================================

#[test]
fn test_set_reward_config_updates_token_and_rate() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp, &rwd, &100u64);

    let new_rwd = Address::generate(&env);
    client.set_reward_config(&admin, &new_rwd, &200u64);

    let cfg = client.get_config();
    assert_eq!(cfg.reward_token, new_rwd);
    assert_eq!(cfg.reward_rate, 200u64);
}

#[test]
fn test_set_reward_config_rejects_non_admin() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp, &rwd, &100u64);

    let new_rwd = Address::generate(&env);
    let result = client.try_set_reward_config(&stranger, &new_rwd, &200u64);
    assert_eq!(result, Err(Ok(StakingError::Unauthorized)));
}

#[test]
fn test_set_reward_config_rejects_rate_too_high() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp, &rwd, &100u64);

    let result = client.try_set_reward_config(&admin, &rwd, &(MAX_REWARD_RATE + 1));
    assert_eq!(result, Err(Ok(StakingError::RewardRateTooHigh)));

    // Original rate must be unchanged.
    assert_eq!(client.get_config().reward_rate, 100u64);
}

#[test]
fn test_set_reward_config_accepts_zero_rate() {
    // A zero rate legally disables emissions without clearing the token.
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp, &rwd, &100u64);

    client.set_reward_config(&admin, &rwd, &0u64);
    assert_eq!(client.get_config().reward_rate, 0u64);
}

#[test]
fn test_set_reward_config_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    let result = client.try_set_reward_config(&admin, &rwd, &100u64);
    assert_eq!(result, Err(Ok(StakingError::NotInitialized)));
}

// ===========================================================================
// View function tests
// ===========================================================================

#[test]
fn test_get_stake_returns_zero_for_unknown_user() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);
    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);
    client.initialize(&admin, &lp, &rwd, &0u64);

    let stranger = Address::generate(&env);
    assert_eq!(client.get_stake(&stranger), 0i128);
}

#[test]
fn test_get_config_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    let result = client.try_get_config();
    assert_eq!(result, Err(Ok(StakingError::NotInitialized)));
}
