#![cfg(test)]

use crate::helpers::{get_amount_in, get_amount_out, sort_tokens};
use crate::RouterError;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_get_amount_out_basic() {
    let env = Env::default();

    // Test with 0.3% fee (30 bps)
    let amount_in = 1000i128;
    let reserve_in = 10000i128;
    let reserve_out = 10000i128;
    let fee_bps = 30u32;

    let result = get_amount_out(&env, amount_in, reserve_in, reserve_out, fee_bps);
    assert!(result.is_ok());

    let amount_out = result.unwrap();
    // With 1000 in, 10000 reserves, 0.3% fee: ~906 out
    assert!(amount_out > 900 && amount_out < 910);
}

#[test]
fn test_get_amount_out_zero_amount() {
    let env = Env::default();

    let result = get_amount_out(&env, 0, 10000, 10000, 30);
    assert_eq!(result, Err(RouterError::ZeroAmount));
}

#[test]
fn test_get_amount_out_zero_reserves() {
    let env = Env::default();

    let result = get_amount_out(&env, 1000, 0, 10000, 30);
    assert_eq!(result, Err(RouterError::InsufficientLiquidity));

    let result = get_amount_out(&env, 1000, 10000, 0, 30);
    assert_eq!(result, Err(RouterError::InsufficientLiquidity));
}

#[test]
fn test_get_amount_in_basic() {
    let env = Env::default();

    // Test with 0.3% fee (30 bps)
    let amount_out = 900i128;
    let reserve_in = 10000i128;
    let reserve_out = 10000i128;
    let fee_bps = 30u32;

    let result = get_amount_in(&env, amount_out, reserve_in, reserve_out, fee_bps);
    assert!(result.is_ok());

    let amount_in = result.unwrap();
    // With 900 out, 10000 reserves, 0.3% fee: ~1000 in
    assert!(amount_in > 990 && amount_in < 1010);
}

#[test]
fn test_get_amount_in_zero_amount() {
    let env = Env::default();

    let result = get_amount_in(&env, 0, 10000, 10000, 30);
    assert_eq!(result, Err(RouterError::ZeroAmount));
}

#[test]
fn test_get_amount_in_exceeds_reserve() {
    let env = Env::default();

    // Requesting more output than available
    let result = get_amount_in(&env, 10000, 10000, 10000, 30);
    assert_eq!(result, Err(RouterError::InsufficientLiquidity));
}

#[test]
fn test_sort_tokens() {
    let env = Env::default();

    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    let result = sort_tokens(&token_a, &token_b);
    assert!(result.is_ok());

    let (first, second) = result.unwrap();
    // Verify they're sorted
    assert!(first < second);

    // Verify sorting is consistent
    let result2 = sort_tokens(&token_b, &token_a);
    assert!(result2.is_ok());
    let (first2, second2) = result2.unwrap();
    assert_eq!(first, first2);
    assert_eq!(second, second2);
}

#[test]
fn test_sort_tokens_identical() {
    let env = Env::default();

    let token = Address::generate(&env);

    let result = sort_tokens(&token, &token);
    assert_eq!(result, Err(RouterError::IdenticalTokens));
}

#[test]
fn test_get_amount_out_high_fee() {
    let env = Env::default();

    // Test with 1% fee (100 bps)
    let amount_in = 1000i128;
    let reserve_in = 10000i128;
    let reserve_out = 10000i128;
    let fee_bps = 100u32;

    let result = get_amount_out(&env, amount_in, reserve_in, reserve_out, fee_bps);
    assert!(result.is_ok());

    let amount_out = result.unwrap();
    // With higher fee, output should be less than with 0.3% fee
    assert!(amount_out > 880 && amount_out < 910);
}

#[test]
fn test_get_amount_in_high_fee() {
    let env = Env::default();

    // Test with 1% fee (100 bps)
    let amount_out = 900i128;
    let reserve_in = 10000i128;
    let reserve_out = 10000i128;
    let fee_bps = 100u32;

    let result = get_amount_in(&env, amount_out, reserve_in, reserve_out, fee_bps);
    assert!(result.is_ok());

    let amount_in = result.unwrap();
    // With higher fee, input required should be more
    // Formula gives us approximately 1010
    assert!(amount_in >= 1000);
}

// --- Multi-hop path computation ---

#[test]
fn test_two_hop_amount_out() {
    let env = Env::default();

    // Two-hop: A → B → C with 30 bps fee each hop
    let amount_in = 1000i128;
    let fee_bps = 30u32;

    // Hop 1: A(10000) → B(10000)
    let mid = get_amount_out(&env, amount_in, 10000, 10000, fee_bps).unwrap();
    // Hop 2: B(10000) → C(10000)
    let final_out = get_amount_out(&env, mid, 10000, 10000, fee_bps).unwrap();

    assert!(mid > 0, "intermediate output must be positive");
    assert!(final_out > 0, "final output must be positive");
    assert!(final_out < mid, "two hops with fees yields less than one hop");
    assert!(mid < amount_in, "first hop output less than input due to fee");
}

#[test]
fn test_three_hop_amount_out() {
    let env = Env::default();

    let amount_in = 10000i128;
    let fee_bps = 30u32;

    // A → B → C → D, all pools balanced
    let hop1 = get_amount_out(&env, amount_in, 100000, 100000, fee_bps).unwrap();
    let hop2 = get_amount_out(&env, hop1, 100000, 100000, fee_bps).unwrap();
    let hop3 = get_amount_out(&env, hop2, 100000, 100000, fee_bps).unwrap();

    assert!(hop3 > 0, "3-hop output must be positive");
    assert!(hop3 < hop2, "each hop reduces output");
    assert!(hop2 < hop1, "each hop reduces output");
    assert!(hop1 < amount_in, "first hop reduces output");
}

#[test]
fn test_multi_hop_prefers_better_liquidity() {
    let env = Env::default();
    let amount_in = 1000i128;
    let fee_bps = 30u32;

    // Route via shallow pool (low liquidity)
    let mid_shallow = get_amount_out(&env, amount_in, 1000, 1000, fee_bps).unwrap();
    let out_shallow = get_amount_out(&env, mid_shallow, 1000, 1000, fee_bps).unwrap();

    // Route via deep pool (high liquidity)
    let mid_deep = get_amount_out(&env, amount_in, 100000, 100000, fee_bps).unwrap();
    let out_deep = get_amount_out(&env, mid_deep, 100000, 100000, fee_bps).unwrap();

    assert!(out_deep > out_shallow, "deeper pools yield higher output");
}
