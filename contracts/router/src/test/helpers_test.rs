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
