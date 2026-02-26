#![cfg(test)]
use crate::test::PairTest; 
use crate::errors::PairError;
use soroban_sdk::{testutils::Address as _, vec, symbol_short};

#[test]
fn test_burn_full_withdrawal() {
    let test = PairTest::setup();
    
    let amount = 100_000_i128;
    test.token_a.mint(&test.user, &amount);
    test.token_b.mint(&test.user, &amount);
    
    test.token_a.transfer(&test.user, &test.pair_address, &amount);
    test.token_b.transfer(&test.user, &test.pair_address, &amount);
    let liquidity = test.pair.mint(&test.user);

    // Full withdrawal: transfer all user LP tokens back to the pair
    test.lp_token.transfer(&test.user, &test.pair_address, &liquidity);
    let (received_a, received_b) = test.pair.burn(&test.user);

    // Expected: 100,000 - (100,000 * 1000 / 100,000) = 99,000 
    // The first 1000 LP tokens are permanently locked as MINIMUM_LIQUIDITY
    assert_eq!(received_a, 99_000);
    assert_eq!(received_b, 99_000);
    
    // Total supply remains 1000 (the locked amount)
    assert_eq!(test.lp_token.total_supply(), 1000);
}

#[test]
fn test_burn_partial_withdrawal() {
    let test = PairTest::setup();
    
    let amount = 200_000_i128;
    test.token_a.mint(&test.user, &amount);
    test.token_b.mint(&test.user, &amount);
    
    test.token_a.transfer(&test.user, &test.pair_address, &amount);
    test.token_b.transfer(&test.user, &test.pair_address, &amount);
    let liquidity = test.pair.mint(&test.user);

    // Burn exactly 50% of user's liquidity
    let half_lp = liquidity / 2;
    test.lp_token.transfer(&test.user, &test.pair_address, &half_lp);
    let (received_a, received_b) = test.pair.burn(&test.user);

    assert!(received_a > 0);
    assert!(received_b > 0);
    
    // Check remaining LP balance in user's account
    assert_eq!(test.lp_token.balance(&test.user), liquidity - half_lp);
}

#[test]
fn test_burn_dust_reverts() {
    let test = PairTest::setup();
    
    // Setup a pool with liquidity
    test.token_a.mint(&test.user, &100_000);
    test.token_b.mint(&test.user, &100_000);
    test.token_a.transfer(&test.user, &test.pair_address, &100_000);
    test.token_b.transfer(&test.user, &test.pair_address, &100_000);
    test.pair.mint(&test.user);

    // Attempt to burn 0 LP tokens or a tiny amount that results in 0 output tokens
    // We send only 1 unit of LP token. 
    let dust_lp = 1_i128;
    test.lp_token.transfer(&test.user, &test.pair_address, &dust_lp);
    
    // Should revert with InsufficientLiquidityBurned
    let result = test.pair.try_burn(&test.user);
    
    assert_eq!(
        result.err().unwrap(),
        Ok(PairError::InsufficientLiquidityBurned.into())
    );
}