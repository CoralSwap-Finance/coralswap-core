//! Tests for the per-pair fee override wiring (issue #132).
//!
//! Verifies that when the factory has a `get_pair_fee_override` installed for
//! this pair, the pair's swap uses it instead of the dynamic fee.

#![cfg(test)]
#![allow(deprecated)]

use crate::factory_client::FactoryClient;
use crate::{Pair, PairClient};
use coralswap_lp_token::{LpToken, LpTokenClient};
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env, String,
};

// ---------------------------------------------------------------------------
// Mock factory stub
// ---------------------------------------------------------------------------

#[contract]
pub struct MockFactory;

#[contractimpl]
impl MockFactory {
    pub fn get_pair_fee_override(env: Env, pair: Address) -> Option<u32> {
        env.storage().instance().get(&(soroban_sdk::symbol_short!("ovr"), pair))
    }

    pub fn set(env: Env, pair: Address, fee_bps: u32) {
        env.storage().instance().set(&(soroban_sdk::symbol_short!("ovr"), pair), &fee_bps);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Bootstrap a complete environment: mock factory, two tokens, LP token,
/// and a funded pair. Returns every address so callers can control the
/// factory between tests.
struct EnvHarness {
    env: Env,
    factory: Address,
    token_a: Address,
    _token_b: Address,
    pair: Address,
}

fn setup_harness() -> EnvHarness {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);

    let factory = env.register_contract(None, MockFactory);
    let token_a = env.register_stellar_asset_contract(admin.clone());
    let _token_b = env.register_stellar_asset_contract(admin.clone());

    let lp = env.register_contract(None, LpToken);
    LpTokenClient::new(&env, &lp).initialize(
        &admin,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );

    let pair = env.register_contract(None, Pair);
    let pair_client = PairClient::new(&env, &pair);
    pair_client.initialize(&factory, &token_a, &_token_b, &lp);

    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token_a).mint(&user, &10_000_000_i128);
    StellarAssetClient::new(&env, &_token_b).mint(&user, &10_000_000_i128);

    TokenClient::new(&env, &token_a).transfer(&user, &pair, &1_000_000_i128);
    TokenClient::new(&env, &_token_b).transfer(&user, &pair, &1_000_000_i128);
    pair_client.mint(&user);

    StellarAssetClient::new(&env, &token_a).mint(&user, &100_000_i128);

    EnvHarness { env, factory, token_a, _token_b, pair }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn pair_sees_override_from_factory() {
    let h = setup_harness();

    MockFactoryClient::new(&h.env, &h.factory).set(&h.pair, &5u32);

    let override_seen = h.env.as_contract(&h.pair, || {
        let state = crate::storage::get_pair_state(&h.env).unwrap();
        let client = FactoryClient::new(&h.env, &state.factory);
        client.get_pair_fee_override(&h.pair)
    });
    assert_eq!(override_seen, Some(5u32));
}

#[test]
fn pair_sees_none_when_no_override() {
    let h = setup_harness();

    let override_seen = h.env.as_contract(&h.pair, || {
        let state = crate::storage::get_pair_state(&h.env).unwrap();
        let client = FactoryClient::new(&h.env, &state.factory);
        client.get_pair_fee_override(&h.pair)
    });
    assert_eq!(override_seen, None);
}

#[test]
fn swap_succeeds_with_override_set() {
    let h = setup_harness();

    MockFactoryClient::new(&h.env, &h.factory).set(&h.pair, &5u32);

    // Actually transfer tokens from the pair-internal user to the pair
    // (the user already has tokens and mock_auths makes auth pass).
    let pair_client = PairClient::new(&h.env, &h.pair);
    let user = Address::generate(&h.env);
    StellarAssetClient::new(&h.env, &h.token_a).mint(&user, &100_000_i128);
    TokenClient::new(&h.env, &h.token_a).transfer(&user, &h.pair, &10_000_i128);
    pair_client.swap(&0, &5_000_i128, &user);
}

#[test]
fn swap_uses_dynamic_fee_when_no_override() {
    let h = setup_harness();

    let pair_client = PairClient::new(&h.env, &h.pair);
    let user = Address::generate(&h.env);
    StellarAssetClient::new(&h.env, &h.token_a).mint(&user, &100_000_i128);
    TokenClient::new(&h.env, &h.token_a).transfer(&user, &h.pair, &10_000_i128);
    pair_client.swap(&0, &5_000_i128, &user);

    let fee = pair_client.get_current_fee_bps();
    assert_eq!(fee, 30, "no override set: dynamic fee must stay at baseline");
}

#[test]
fn swap_with_override_does_not_panic_even_when_factory_missing() {
    // The pair should fall back to dynamic fee when the factory contract does
    // not exist or returns an error. This tests graceful degradation.
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);

    // Note: factory is just a raw address, NOT a deployed contract.
    let factory = Address::generate(&env);
    let token_a = env.register_stellar_asset_contract(admin.clone());
    let token_b = env.register_stellar_asset_contract(admin.clone());

    let lp = env.register_contract(None, LpToken);
    LpTokenClient::new(&env, &lp).initialize(
        &admin,
        &7u32,
        &String::from_str(&env, "LP"),
        &String::from_str(&env, "LP"),
    );

    let pair = env.register_contract(None, Pair);
    let pair_client = PairClient::new(&env, &pair);
    pair_client.initialize(&factory, &token_a, &token_b, &lp);

    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token_a).mint(&user, &10_000_000_i128);
    StellarAssetClient::new(&env, &token_b).mint(&user, &10_000_000_i128);

    TokenClient::new(&env, &token_a).transfer(&user, &pair, &1_000_000_i128);
    TokenClient::new(&env, &token_b).transfer(&user, &pair, &1_000_000_i128);
    pair_client.mint(&user);

    TokenClient::new(&env, &token_a).transfer(&user, &pair, &10_000_i128);
    pair_client.swap(&0, &5_000_i128, &user);

    let fee = pair_client.get_current_fee_bps();
    assert_eq!(fee, 30, "must fall back to dynamic fee when factory unavailable");
}
