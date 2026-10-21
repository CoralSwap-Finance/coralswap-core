//! Optional price-impact cap on the router's exact-in path (issue #345).
//!
//! `max_price_impact_bps: Option<u32>` bounds how far an exact-in swap may
//! move the market, measured against the path's mid price net of fees. It is
//! checked on the realized quote before any token moves. `None` disables it.

#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _, token::StellarAssetClient, token::TokenClient, Address, Env, Vec,
};

use super::{deploy_router, generate_asset_tokens, setup_pair, RouterClient};
use crate::helpers::get_path_price_impact_bps;

const PRICE_IMPACT_TOO_HIGH: u32 = 319; // RouterError::PriceImpactTooHigh
const INVALID_PRICE_IMPACT_CAP: u32 = 320; // RouterError::InvalidPriceImpactCap

const RESERVE: i128 = 1_000_000;
const AMOUNT_IN: i128 = 10_000;
// 10_000 in against 1M/1M at 30 bps: ideal 9_970, realized 9_871 → 99 bps.
const ONE_HOP_IMPACT: u32 = 99;
// Two such hops: ideal 9_940, realized 9_745 → 196 bps.
const TWO_HOP_IMPACT: u32 = 196;

/// Extracts the contract error code from a failed `try_*` call.
macro_rules! code {
    ($call:expr) => {
        match $call {
            Err(Ok(e)) => e.get_code(),
            other => panic!("expected a contract error, got {:?}", other.map(|_| ())),
        }
    };
}

struct Setup {
    env: Env,
    router_id: Address,
    factory_id: Address,
    path: Vec<Address>,
    user: Address,
}

/// Builds a `hops`-hop path of 1M/1M mock pairs and funds `user` with the
/// input token (and the router with each intermediate token, since
/// `MockPair::swap` moves nothing on its own).
fn setup(hops: u32) -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let (router_id, factory_id) = deploy_router(&env);
    let tokens = generate_asset_tokens(&env, hops + 1);
    for i in 0..hops {
        setup_pair(
            &env,
            &factory_id,
            &tokens.get(i).unwrap(),
            &tokens.get(i + 1).unwrap(),
            RESERVE,
            RESERVE,
        );
    }
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &tokens.get(0).unwrap()).mint(&user, &RESERVE);
    for i in 1..hops {
        StellarAssetClient::new(&env, &tokens.get(i).unwrap()).mint(&router_id, &RESERVE);
    }
    Setup { env, router_id, factory_id, path: tokens, user }
}

#[test]
fn test_helper_reports_curve_impact_net_of_fees() {
    let s = setup(2);
    let mut one_hop = Vec::new(&s.env);
    one_hop.push_back(s.path.get(0).unwrap());
    one_hop.push_back(s.path.get(1).unwrap());

    let impact = s.env.as_contract(&s.router_id, || {
        get_path_price_impact_bps(&s.env, &s.factory_id, &one_hop, AMOUNT_IN, 9_871)
    });
    assert_eq!(impact, Ok(ONE_HOP_IMPACT));

    let impact = s.env.as_contract(&s.router_id, || {
        get_path_price_impact_bps(&s.env, &s.factory_id, &s.path, AMOUNT_IN, 9_745)
    });
    assert_eq!(impact, Ok(TWO_HOP_IMPACT));

    // Output at (or above) the fee-adjusted mid price is zero impact.
    let impact = s.env.as_contract(&s.router_id, || {
        get_path_price_impact_bps(&s.env, &s.factory_id, &one_hop, AMOUNT_IN, 9_970)
    });
    assert_eq!(impact, Ok(0));
}

#[test]
fn test_no_cap_is_the_default_and_allows_any_impact() {
    let s = setup(1);
    let router = RouterClient::new(&s.env, &s.router_id);
    // Half the pool: far beyond any sane cap, still executes without one.
    let out = router.swap_exact_tokens_multi_hop(
        &s.path,
        &(RESERVE / 2),
        &0,
        &s.user,
        &u64::MAX,
        &None,
        &None,
    );
    assert!(out > 0);
}

#[test]
fn test_cap_at_or_above_impact_executes() {
    let s = setup(1);
    let router = RouterClient::new(&s.env, &s.router_id);
    let out = router.swap_exact_tokens_multi_hop(
        &s.path,
        &AMOUNT_IN,
        &0,
        &s.user,
        &u64::MAX,
        &None,
        &Some(ONE_HOP_IMPACT),
    );
    assert_eq!(out, 9_871);
}

#[test]
fn test_cap_below_impact_reverts_before_any_transfer() {
    let s = setup(1);
    let router = RouterClient::new(&s.env, &s.router_id);
    let token_in = TokenClient::new(&s.env, &s.path.get(0).unwrap());
    let before = token_in.balance(&s.user);

    assert_eq!(
        code!(router.try_swap_exact_tokens_multi_hop(
            &s.path,
            &AMOUNT_IN,
            &0,
            &s.user,
            &u64::MAX,
            &None,
            &Some(ONE_HOP_IMPACT - 1),
        )),
        PRICE_IMPACT_TOO_HIGH
    );
    assert_eq!(token_in.balance(&s.user), before);
}

#[test]
fn test_cap_applies_to_the_whole_multi_hop_path() {
    let s = setup(2);
    let router = RouterClient::new(&s.env, &s.router_id);

    // Each hop alone is under 100 bps, but the path compounds to 196 bps.
    assert_eq!(
        code!(router.try_swap_exact_tokens_multi_hop(
            &s.path,
            &AMOUNT_IN,
            &0,
            &s.user,
            &u64::MAX,
            &None,
            &Some(ONE_HOP_IMPACT + 1),
        )),
        PRICE_IMPACT_TOO_HIGH
    );
    let out = router.swap_exact_tokens_multi_hop(
        &s.path,
        &AMOUNT_IN,
        &0,
        &s.user,
        &u64::MAX,
        &None,
        &Some(TWO_HOP_IMPACT),
    );
    assert_eq!(out, 9_745);
}

#[test]
fn test_swap_exact_tokens_for_tokens_forwards_the_cap() {
    let s = setup(1);
    let router = RouterClient::new(&s.env, &s.router_id);
    assert_eq!(
        code!(router.try_swap_exact_tokens_for_tokens(
            &AMOUNT_IN,
            &0,
            &s.path,
            &s.user,
            &u64::MAX,
            &None,
            &Some(0),
        )),
        PRICE_IMPACT_TOO_HIGH
    );
    let amounts = router.swap_exact_tokens_for_tokens(
        &AMOUNT_IN,
        &0,
        &s.path,
        &s.user,
        &u64::MAX,
        &None,
        &Some(ONE_HOP_IMPACT),
    );
    assert_eq!(amounts.get(0).unwrap(), 9_871);
}

#[test]
fn test_cap_above_100_percent_is_rejected() {
    let s = setup(1);
    let router = RouterClient::new(&s.env, &s.router_id);
    assert_eq!(
        code!(router.try_swap_exact_tokens_multi_hop(
            &s.path,
            &AMOUNT_IN,
            &0,
            &s.user,
            &u64::MAX,
            &None,
            &Some(10_001),
        )),
        INVALID_PRICE_IMPACT_CAP
    );
    // Exactly 100% is a valid (if permissive) cap.
    let out = router.swap_exact_tokens_multi_hop(
        &s.path,
        &AMOUNT_IN,
        &0,
        &s.user,
        &u64::MAX,
        &None,
        &Some(10_000),
    );
    assert_eq!(out, 9_871);
}
