//! Tests for RWA token NAV normalization via price feeds (issue #128).
//!
//! These tests verify:
//! - A standard pair (no price feed) behaves identically to current behavior
//! - A RWA pair adjusts amounts by NAV ratio in `get_amounts_out()`
//! - `get_pair_config()` returns the full config in one call

#![cfg(test)]

use crate::storage::{get_pair_config, PairConfig};
use crate::{Pair, PairClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

// ---------------------------------------------------------------------------
// Mock price feed contract
// ---------------------------------------------------------------------------

/// A mock price feed that returns a fixed price scaled to PRICE_SCALE (1e18).
/// Tests can call `set_price` to simulate NAV changes.
#[contract]
pub struct MockPriceFeed;

#[contractimpl]
impl MockPriceFeed {
    /// Returns the current price. Defaults to PRICE_SCALE (1.0) if not set.
    pub fn get_price(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&soroban_sdk::symbol_short!("price"))
            .unwrap_or(crate::price_feed::PRICE_SCALE)
    }

    /// Sets the price for testing. `new_price` should be in PRICE_SCALE units.
    pub fn set_price(env: Env, new_price: i128) {
        env.storage().instance().set(&soroban_sdk::symbol_short!("price"), &new_price);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct PriceFeedTestEnv {
    env: Env,
    token_a: Address,
    token_b: Address,
    pair: Address,
    pair_client: PairClient<'static>,
    price_feed: Address,
    lp_token: Address,
}

/// Sets up a standard pair (no price feed) with initial liquidity.
fn setup_standard_pair() -> PriceFeedTestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);

    let token_a = env.register_stellar_asset_contract(admin.clone());
    let token_b = env.register_stellar_asset_contract(admin.clone());
    let (token_a, token_b) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };

    let lp_token = env.register_stellar_asset_contract(admin.clone());
    let pair = env.register_contract(None, Pair);
    let pair_client = PairClient::new(&env, &pair);

    let factory = Address::generate(&env);
    let price_feed = env.register_contract(None, MockPriceFeed);

    // Initialize WITHOUT price feeds (standard pair)
    pair_client.initialize(&factory, &token_a, &token_b, &lp_token, &None, &None);

    // Seed initial liquidity
    let user = Address::generate(&env);
    let amount_a = 1_000_000_000i128;
    let amount_b = 2_000_000_000i128;
    StellarAssetClient::new(&env, &token_a).mint(&user, &amount_a);
    StellarAssetClient::new(&env, &token_b).mint(&user, &amount_b);
    TokenClient::new(&env, &token_a).transfer(&user, &pair, &amount_a);
    TokenClient::new(&env, &token_b).transfer(&user, &pair, &amount_b);
    pair_client.mint(&user);

    PriceFeedTestEnv { env, token_a, token_b, pair, pair_client, price_feed, lp_token }
}

/// Sets up an RWA pair WITH price feeds for both tokens.
fn setup_rwa_pair() -> PriceFeedTestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);

    let token_a = env.register_stellar_asset_contract(admin.clone());
    let token_b = env.register_stellar_asset_contract(admin.clone());
    let (token_a, token_b) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };

    let lp_token = env.register_stellar_asset_contract(admin.clone());
    let pair = env.register_contract(None, Pair);
    let pair_client = PairClient::new(&env, &pair);

    let factory = Address::generate(&env);
    let price_feed = env.register_contract(None, MockPriceFeed);

    // Initialize WITH price feeds (RWA pair) - same feed for both tokens
    pair_client.initialize(
        &factory,
        &token_a,
        &token_b,
        &lp_token,
        &Some(price_feed.clone()),
        &Some(price_feed.clone()),
    );

    // Seed initial liquidity
    let user = Address::generate(&env);
    let amount_a = 1_000_000_000i128;
    let amount_b = 2_000_000_000i128;
    StellarAssetClient::new(&env, &token_a).mint(&user, &amount_a);
    StellarAssetClient::new(&env, &token_b).mint(&user, &amount_b);
    TokenClient::new(&env, &token_a).transfer(&user, &pair, &amount_a);
    TokenClient::new(&env, &token_b).transfer(&user, &pair, &amount_b);
    pair_client.mint(&user);

    PriceFeedTestEnv { env, token_a, token_b, pair, pair_client, price_feed, lp_token }
}

// ---------------------------------------------------------------------------
// Tests: Standard pair (no feed) - identical behavior
// ---------------------------------------------------------------------------

/// A standard pair with no price feed must compute the same output as the
/// traditional Uniswap V2 formula.
#[test]
fn test_standard_pair_no_feed_returns_standard_amounts_out() {
    let h = setup_standard_pair();
    let amount_in = 10_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();

    // Call get_amounts_out with token_a as input
    let amounts_out = h.pair_client.get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a);
    assert!(amounts_out.is_ok());
    let amount_out = amounts_out.unwrap();
    assert!(amount_out > 0);

    // Verify it matches standard CPMM: amount_out = (997 * amount_in * reserve_out)
    //                                    / (reserve_in * 1000 + 997 * amount_in)
    let fee_bps = 30u32;
    let fee_factor = 10_000i128 - fee_bps as i128;
    let expected_num = amount_in * fee_factor * reserve_b;
    let expected_den = reserve_a * 10_000 + amount_in * fee_factor;
    let expected_out = expected_num / expected_den;

    assert_eq!(
        amount_out, expected_out,
        "Standard pair must match CPMM formula: got {}, expected {}",
        amount_out, expected_out
    );
}

/// A standard pair with no feed must also work for token_b input.
#[test]
fn test_standard_pair_no_feed_b_to_a() {
    let h = setup_standard_pair();
    let amount_in = 5_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();

    // Call get_amounts_out with token_b as input
    let amounts_out = h.pair_client.get_amounts_out(&amount_in, &reserve_b, &reserve_a, &h.token_b);
    assert!(amounts_out.is_ok());
    let amount_out = amounts_out.unwrap();
    assert!(amount_out > 0);

    // Standard formula
    let fee_bps = 30u32;
    let fee_factor = 10_000i128 - fee_bps as i128;
    let expected_num = amount_in * fee_factor * reserve_a;
    let expected_den = reserve_b * 10_000 + amount_in * fee_factor;
    let expected_out = expected_num / expected_den;

    assert_eq!(amount_out, expected_out);
}

// ---------------------------------------------------------------------------
// Tests: RWA pair with price feed normalization
// ---------------------------------------------------------------------------

/// When the NAV (price) is 1.0 (PRICE_SCALE), the RWA pair must behave
/// identically to a standard pair.
#[test]
fn test_rwa_pair_price_at_parity_equals_standard() {
    let h = setup_rwa_pair();

    // Set price to 1.0 (PRICE_SCALE)
    MockPriceFeedClient::new(&h.env, &h.price_feed)
        .set_price(&crate::price_feed::PRICE_SCALE);

    let amount_in = 10_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();

    let amounts_out = h.pair_client.get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a);
    assert!(amounts_out.is_ok());
    let amount_out = amounts_out.unwrap();

    // Standard CPMM
    let fee_factor = 10_000i128 - 30i128;
    let expected_num = amount_in * fee_factor * reserve_b;
    let expected_den = reserve_a * 10_000 + amount_in * fee_factor;
    let expected_out = expected_num / expected_den;

    assert_eq!(
        amount_out, expected_out,
        "RWA pair at parity must behave identically to standard pair"
    );
}

/// When the NAV of token_a increases by 2x, the output for swapping token_a
/// should adjust: effectively the input amount is worth less in normalized
/// terms compared to the reserve.
#[test]
fn test_rwa_pair_nav_increases_adjusts_output() {
    let h = setup_rwa_pair();
    let price_feed_client = MockPriceFeedClient::new(&h.env, &h.price_feed);

    // First get baseline output at parity (NAV = 1.0)
    price_feed_client.set_price(&crate::price_feed::PRICE_SCALE);
    let amount_in = 10_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();
    let baseline_out =
        h.pair_client.get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a).unwrap();

    // Now set NAV to 2.0 (2 * PRICE_SCALE) - token_a is worth 2x more
    price_feed_client.set_price(&(crate::price_feed::PRICE_SCALE * 2));

    let nav_doubled_out =
        h.pair_client.get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a).unwrap();

    // When token_a's NAV doubles, the normalized input amount is effectively
    // doubled (amount_in * 2.0), making the swap larger in normalized space.
    // This should result in a *larger* output since the same raw input
    // represents more normalized value.
    //
    // More specifically:
    // norm_amount_in = amount_in * 2.0 / 1.0
    // norm_reserve_in = reserve_a * 2.0 / 1.0
    // norm_reserve_out = reserve_b * 2.0 / 1.0
    // So the normalized ratio stays the same, but the normalized input
    // amount is larger relative to the normalized reserves, resulting
    // in more output.
    assert!(
        nav_doubled_out > baseline_out,
        "Doubling NAV must increase output: baseline={}, nav_doubled={}",
        baseline_out,
        nav_doubled_out
    );
}

/// When NAV decreases, the output should decrease proportionally.
#[test]
fn test_rwa_pair_nav_decreases_adjusts_output() {
    let h = setup_rwa_pair();
    let price_feed_client = MockPriceFeedClient::new(&h.env, &h.price_feed);

    price_feed_client.set_price(&crate::price_feed::PRICE_SCALE);
    let amount_in = 10_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();
    let baseline_out =
        h.pair_client.get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a).unwrap();

    // Set NAV to 0.5 (half)
    price_feed_client.set_price(&(crate::price_feed::PRICE_SCALE / 2));

    let nav_halved_out =
        h.pair_client.get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a).unwrap();

    assert!(
        nav_halved_out < baseline_out,
        "Halving NAV must decrease output: baseline={}, nav_halved={}",
        baseline_out,
        nav_halved_out
    );
}

/// Get_amounts_out must fail with InvalidPriceFeed when feed returns zero.
#[test]
fn test_rwa_pair_zero_price_feed_fails() {
    let h = setup_rwa_pair();
    let price_feed_client = MockPriceFeedClient::new(&h.env, &h.price_feed);

    // Set price to zero (invalid)
    price_feed_client.set_price(&0i128);

    let amount_in = 10_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();
    let result = h.pair_client.try_get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a);

    assert!(result.is_err(), "Zero price feed must return error");
}

/// Get_amounts_out must fail with InvalidPriceFeed when feed returns negative.
#[test]
fn test_rwa_pair_negative_price_feed_fails() {
    let h = setup_rwa_pair();
    let price_feed_client = MockPriceFeedClient::new(&h.env, &h.price_feed);

    price_feed_client.set_price(&-1i128);

    let amount_in = 10_000i128;
    let (reserve_a, reserve_b, _) = h.pair_client.get_reserves();
    let result = h.pair_client.try_get_amounts_out(&amount_in, &reserve_a, &reserve_b, &h.token_a);

    assert!(result.is_err(), "Negative price feed must return error");
}

// ---------------------------------------------------------------------------
// Tests: get_pair_config()
// ---------------------------------------------------------------------------

/// get_pair_config must return the full PairConfig for a standard pair.
#[test]
fn test_get_pair_config_standard_pair() {
    let h = setup_standard_pair();

    let config = h.pair_client.get_pair_config();

    assert_eq!(config.fee_bps, 0);
    assert_eq!(config.price_feed_0, None);
    assert_eq!(config.price_feed_1, None);
    assert_eq!(config.is_paused, false);
}

/// get_pair_config must return the full PairConfig with price feeds for RWA pair.
#[test]
fn test_get_pair_config_rwa_pair() {
    let h = setup_rwa_pair();

    let config = h.pair_client.get_pair_config();

    // Price feeds should be set
    assert_eq!(config.price_feed_0, Some(h.price_feed.clone()));
    assert_eq!(config.price_feed_1, Some(h.price_feed.clone()));

    // Default values
    assert_eq!(config.fee_bps, 0);
    assert_eq!(config.is_paused, false);
}

/// get_pair_config must be readable from storage directly.
#[test]
fn test_get_pair_config_storage() {
    let h = setup_rwa_pair();

    let config = h.env.as_contract(&h.pair, || get_pair_config(&h.env));

    assert_eq!(config.fee_bps, 0);
    assert_eq!(config.price_feed_0, Some(h.price_feed.clone()));
    assert_eq!(config.price_feed_1, Some(h.price_feed.clone()));
    assert_eq!(config.is_paused, false);
}

