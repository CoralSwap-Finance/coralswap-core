#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

use crate::errors::OracleError;
use crate::oracle::{consult_twap, update_cumulative_prices, MAX_TWAP_WINDOW};
use crate::{Pair, PairClient};
use coralswap_lp_token::{LpToken, LpTokenClient};

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, Pair);
    (env, contract_id)
}

// ── Minimal mock token (supports transfer + balance + mint) ─────────────────

#[contracttype]
enum MockTokenKey {
    Balance(Address),
}

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = MockTokenKey::Balance(to.clone());
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let fk = MockTokenKey::Balance(from);
        let tk = MockTokenKey::Balance(to);
        let fb: i128 = env.storage().persistent().get(&fk).unwrap_or(0);
        let tb: i128 = env.storage().persistent().get(&tk).unwrap_or(0);
        env.storage().persistent().set(&fk, &(fb - amount));
        env.storage().persistent().set(&tk, &(tb + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&MockTokenKey::Balance(id)).unwrap_or(0)
    }
}

// ── Shared setup for integration tests ──────────────────────────────────────

#[allow(clippy::type_complexity)]
fn setup_pair_with_liquidity() -> (
    Env,
    PairClient<'static>,
    MockTokenClient<'static>,
    MockTokenClient<'static>,
    LpTokenClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let token_a_id = env.register_contract(None, MockToken);
    let token_b_id = env.register_contract(None, MockToken);
    let lp_id = env.register_contract(None, LpToken);
    let pair_id = env.register_contract(None, Pair);

    let token_a = MockTokenClient::new(&env, &token_a_id);
    let token_b = MockTokenClient::new(&env, &token_b_id);
    let lp_client = LpTokenClient::new(&env, &lp_id);
    let pair_client = PairClient::new(&env, &pair_id);

    let admin = Address::generate(&env);
    let factory = Address::generate(&env);
    let user = Address::generate(&env);

    lp_client.initialize(
        &admin,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );

    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    // Add initial liquidity
    let amount_a = 1_000_000_000_i128;
    let amount_b = 1_000_000_000_i128;
    token_a.mint(&user, &amount_a);
    token_b.mint(&user, &amount_b);
    token_a.transfer(&user, &pair_client.address, &amount_a);
    token_b.transfer(&user, &pair_client.address, &amount_b);
    pair_client.mint(&user);

    (env, pair_client, token_a, token_b, lp_client, user)
}

#[test]
fn ring_buffer_capped_at_24() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        // Push 30 observations
        for _i in 0..30 {
            update_cumulative_prices(&env, 100, 200, 1, &mut price_a, &mut price_b);
        }

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.observations.len(), 24, "ring buffer must not exceed 24 entries");
    });
}

#[test]
fn observations_are_appended_on_price_update() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let mut price_a: i128 = 0;
        let mut price_b: i128 = 0;

        update_cumulative_prices(&env, 100, 200, 10, &mut price_a, &mut price_b);
        assert_eq!(price_a, 20);

        let state = crate::storage::get_oracle_state(&env);
        assert_eq!(state.observations.len(), 1);
        let (_, cum_a, _) = state.observations.get(0).unwrap();
        assert_eq!(cum_a, 20);
    });
}

#[test]
fn consult_twap_window_zero_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, 0);
        assert_eq!(result, Err(OracleError::WindowTooShort));
    });
}

#[test]
fn consult_twap_window_too_long_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, MAX_TWAP_WINDOW + 1);
        assert_eq!(result, Err(OracleError::WindowTooLong));
    });
}

#[test]
fn consult_twap_no_observations_returns_error() {
    let (env, contract_id) = setup_env();

    env.as_contract(&contract_id, || {
        let result = consult_twap(&env, 100);
        assert_eq!(result, Err(OracleError::WindowTooShort));
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Bug Condition Exploration Tests
// ═══════════════════════════════════════════════════════════════════════════
//
// These tests demonstrate the bug: swap/mint/burn operations execute without
// calling update_cumulative_prices, leaving the observation buffer empty.
//
// EXPECTED OUTCOME ON UNFIXED CODE: These tests WILL FAIL (this is correct!)
// EXPECTED OUTCOME AFTER FIX: These tests WILL PASS
//

#[test]
fn bug_condition_swap_does_not_populate_oracle() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    // Advance time and execute a swap
    env.ledger().set_timestamp(100);

    let swap_amount = 1_000_000_i128;
    token_a.mint(&user, &swap_amount);
    token_a.transfer(&user, &pair_client.address, &swap_amount);

    // Execute swap
    pair_client.swap(&0_i128, &1_000_i128, &user);

    // Query oracle state AFTER the swap
    // BUG: observation buffer should be populated but it's empty
    env.as_contract(&pair_client.address, || {
        let oracle_state = crate::storage::get_oracle_state(&env);

        // COUNTEREXAMPLE: After swap with time elapsed, buffer should have observations
        // but on unfixed code it remains empty
        assert!(
            oracle_state.observations.len() > 0,
            "BUG DETECTED: After swap, observation buffer is empty (expected: populated)"
        );
    });
}

#[test]
fn bug_condition_mint_does_not_populate_oracle() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    // Advance time and execute a mint
    env.ledger().set_timestamp(100);

    let mint_amount_a = 100_000_i128;
    let mint_amount_b = 100_000_i128;
    token_a.mint(&user, &mint_amount_a);
    token_b.mint(&user, &mint_amount_b);
    token_a.transfer(&user, &pair_client.address, &mint_amount_a);
    token_b.transfer(&user, &pair_client.address, &mint_amount_b);

    // Execute mint
    pair_client.mint(&user);

    // Query oracle state AFTER the mint
    env.as_contract(&pair_client.address, || {
        let oracle_state = crate::storage::get_oracle_state(&env);

        // COUNTEREXAMPLE: After mint with time elapsed, buffer should have observations
        assert!(
            oracle_state.observations.len() > 0,
            "BUG DETECTED: After mint, observation buffer is empty (expected: populated)"
        );
    });
}

#[test]
fn bug_condition_burn_does_not_populate_oracle() {
    let (env, pair_client, token_a, token_b, lp_client, user) = setup_pair_with_liquidity();

    // Advance time and execute a burn
    env.ledger().set_timestamp(100);

    let lp_balance = lp_client.balance(&user);
    let burn_amount = lp_balance / 10; // Burn 10% of LP tokens
    lp_client.transfer(&user, &pair_client.address, &burn_amount);

    // Execute burn
    pair_client.burn(&user);

    // Query oracle state AFTER the burn
    env.as_contract(&pair_client.address, || {
        let oracle_state = crate::storage::get_oracle_state(&env);

        // COUNTEREXAMPLE: After burn with time elapsed, buffer should have observations
        assert!(
            oracle_state.observations.len() > 0,
            "BUG DETECTED: After burn, observation buffer is empty (expected: populated)"
        );
    });
}

#[test]
fn bug_condition_multiple_swaps_consult_twap_fails() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    // Execute 10 swaps with advancing timestamps
    for i in 1..=10 {
        env.ledger().set_timestamp(i as u64 * 10);

        let swap_amount = 10_000_i128;
        token_a.mint(&user, &swap_amount);
        token_a.transfer(&user, &pair_client.address, &swap_amount);

        pair_client.swap(&0_i128, &100_i128, &user);
    }

    // After 10 swaps across 100 timestamp units, consult_twap should work
    // BUG: consult_twap returns error because observation buffer was never populated
    let result = pair_client.try_consult_twap(&50);

    // COUNTEREXAMPLE: After multiple swaps with time advancement, consult_twap
    // should return Ok(prices) but on unfixed code it returns WindowTooShort error
    assert!(
        result.is_ok(),
        "BUG DETECTED: After 10 swaps with advancing time, consult_twap still returns error (expected: Ok)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Preservation Property Tests
// ═══════════════════════════════════════════════════════════════════════════
//
// These tests capture existing swap/mint/burn behavior BEFORE the fix.
// After implementing the fix, these tests MUST still pass to ensure no
// regressions in core AMM mechanics.
//
// EXPECTED OUTCOME ON UNFIXED CODE: These tests WILL PASS
// EXPECTED OUTCOME AFTER FIX: These tests MUST STILL PASS
//

#[test]
fn preservation_swap_reserve_updates_correct() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    let (res_a_before, res_b_before, _) = pair_client.get_reserves();

    // Execute a swap: provide token_a, receive token_b
    let swap_in = 10_000_i128;
    token_a.mint(&user, &swap_in);
    token_a.transfer(&user, &pair_client.address, &swap_in);

    // Request ~100 token_b out (actual amount will be calculated by AMM)
    pair_client.swap(&0_i128, &100_i128, &user);

    let (res_a_after, res_b_after, _) = pair_client.get_reserves();

    // Verify reserve updates follow constant-product formula
    // Reserve A should increase (we sent token_a in)
    assert!(res_a_after > res_a_before, "Reserve A should increase after swap");
    // Reserve B should decrease (token_b was sent out)
    assert!(res_b_after < res_b_before, "Reserve B should decrease after swap");

    // K-invariant should not decrease (fees cause it to increase slightly)
    let k_before = res_a_before * res_b_before;
    let k_after = res_a_after * res_b_after;
    assert!(k_after >= k_before, "K-invariant must not decrease");
}

#[test]
fn preservation_mint_lp_calculation_correct() {
    let (env, pair_client, token_a, token_b, lp_client, user) = setup_pair_with_liquidity();

    let supply_before = lp_client.total_supply();
    let (res_a_before, res_b_before, _) = pair_client.get_reserves();

    // Add 10% more liquidity
    let add_a = res_a_before / 10;
    let add_b = res_b_before / 10;

    token_a.mint(&user, &add_a);
    token_b.mint(&user, &add_b);
    token_a.transfer(&user, &pair_client.address, &add_a);
    token_b.transfer(&user, &pair_client.address, &add_b);

    let lp_minted = pair_client.mint(&user);

    let supply_after = lp_client.total_supply();

    // LP tokens minted should be proportional to liquidity added
    // Adding 10% liquidity should mint ~10% of total supply
    let expected_lp = supply_before / 10;
    assert!(
        (lp_minted - expected_lp).abs() <= 1,
        "LP minted should be proportional to liquidity added"
    );

    assert_eq!(
        supply_after,
        supply_before + lp_minted,
        "Total supply should increase by minted amount"
    );
}

#[test]
fn preservation_burn_withdrawal_correct() {
    let (env, pair_client, token_a, token_b, lp_client, user) = setup_pair_with_liquidity();

    let lp_balance = lp_client.balance(&user);
    let total_supply = lp_client.total_supply();
    let (res_a_before, res_b_before, _) = pair_client.get_reserves();

    let balance_a_before = token_a.balance(&user);
    let balance_b_before = token_b.balance(&user);

    // Burn 10% of LP tokens
    let burn_amount = lp_balance / 10;
    lp_client.transfer(&user, &pair_client.address, &burn_amount);

    let (amount_a_out, amount_b_out) = pair_client.burn(&user);

    let balance_a_after = token_a.balance(&user);
    let balance_b_after = token_b.balance(&user);

    // User should receive tokens proportional to LP burned
    assert_eq!(balance_a_after, balance_a_before + amount_a_out, "User should receive token A");
    assert_eq!(balance_b_after, balance_b_before + amount_b_out, "User should receive token B");

    // Withdrawn amounts should be proportional to (LP burned / total supply) * reserves
    let expected_a = (burn_amount * res_a_before) / total_supply;
    let expected_b = (burn_amount * res_b_before) / total_supply;

    assert!(
        (amount_a_out - expected_a).abs() <= 1000,
        "Withdrawn token A should be proportional to LP burned: got {}, expected {}",
        amount_a_out,
        expected_a
    );
    assert!(
        (amount_b_out - expected_b).abs() <= 1000,
        "Withdrawn token B should be proportional to LP burned: got {}, expected {}",
        amount_b_out,
        expected_b
    );
}

#[test]
fn preservation_k_invariant_enforced() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    let (res_a, res_b, _) = pair_client.get_reserves();

    // Try to execute a swap that would violate K-invariant
    // This is artificially constructed to demonstrate the check
    let swap_in = 1_i128;
    token_a.mint(&user, &swap_in);
    token_a.transfer(&user, &pair_client.address, &swap_in);

    // Try to withdraw nearly all of reserve B (would violate K)
    let invalid_out = res_b - 1;
    let result = pair_client.try_swap(&0_i128, &invalid_out, &user);

    // Should fail due to K-invariant violation
    assert!(result.is_err(), "Swap violating K-invariant should fail");
}

#[test]
fn preservation_insufficient_liquidity_error() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    // Try to swap with zero input
    let result = pair_client.try_swap(&0_i128, &0_i128, &user);

    // Should fail due to insufficient output
    assert!(result.is_err(), "Swap with zero output should fail");
}

#[test]
fn preservation_dynamic_fee_calculation_unchanged() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    // Execute a swap and check that fee is calculated
    let swap_in = 10_000_i128;
    token_a.mint(&user, &swap_in);
    token_a.transfer(&user, &pair_client.address, &swap_in);

    let (res_a_before, res_b_before, _) = pair_client.get_reserves();

    pair_client.swap(&0_i128, &100_i128, &user);

    let (res_a_after, res_b_after, _) = pair_client.get_reserves();

    // Fee should cause K to increase (fees accrue to the pool)
    let k_before = res_a_before * res_b_before;
    let k_after = res_a_after * res_b_after;

    assert!(k_after > k_before, "K should increase due to fees");
}

#[test]
fn preservation_timestamp_updated_after_operations() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    let (_, _, ts_before) = pair_client.get_reserves();

    // Advance time
    env.ledger().set_timestamp(ts_before + 100);

    // Execute mint
    let mint_a = 1_000_i128;
    let mint_b = 1_000_i128;
    token_a.mint(&user, &mint_a);
    token_b.mint(&user, &mint_b);
    token_a.transfer(&user, &pair_client.address, &mint_a);
    token_b.transfer(&user, &pair_client.address, &mint_b);

    pair_client.mint(&user);

    let (_, _, ts_after) = pair_client.get_reserves();

    // Timestamp should be updated
    assert_eq!(ts_after, ts_before + 100, "Timestamp should be updated after mint");
}

// ═══════════════════════════════════════════════════════════════════════════
// End-to-End TWAP Integration Test
// ═══════════════════════════════════════════════════════════════════════════
//
// Exercises consult_twap() through the real swap path (not in isolation):
// a sequence of real swaps advances both the ledger sequence (the unit
// consult_twap windows over) and the ledger timestamp (the unit
// update_cumulative_prices accrues over), and the resulting TWAP is checked
// against the known reserve trajectory instead of just checking `is_ok()`.

#[test]
fn integration_consult_twap_correct_after_real_swap_sequence() {
    let (env, pair_client, token_a, token_b, _lp, user) = setup_pair_with_liquidity();

    // Perform 10 real swaps, each advancing the ledger by one simulated
    // block (sequence +5, timestamp +5s) before the swap executes. This
    // populates the oracle observation buffer via the real swap path.
    for i in 1..=10u32 {
        env.ledger().set_sequence_number(i * 5);
        env.ledger().set_timestamp(i as u64 * 5);

        let swap_amount = 10_000_i128;
        token_a.mint(&user, &swap_amount);
        token_a.transfer(&user, &pair_client.address, &swap_amount);

        pair_client.swap(&0_i128, &100_i128, &user);
    }

    // The observation buffer should have grown to one entry per swap.
    env.as_contract(&pair_client.address, || {
        let oracle_state = crate::storage::get_oracle_state(&env);
        assert_eq!(
            oracle_state.observations.len(),
            10,
            "observation buffer should have one entry per swap"
        );
    });

    let (reserve_a, reserve_b, _) = pair_client.get_reserves();
    assert!(
        reserve_a > reserve_b,
        "swaps sent token_a in, so reserve_a should now exceed reserve_b"
    );

    // Query a TWAP window shorter than the full swap history, real ledger
    // advancement means this must resolve to a genuine (non-degenerate)
    // interpolated result instead of the previous empty-buffer error.
    for window in [10u32, 15, 20, 25, 30] {
        let (price_a_avg, price_b_avg) = pair_client
            .try_consult_twap(&window)
            .expect("consult_twap call should not panic")
            .expect(
                "consult_twap should return Ok after a real swap sequence with ledger advancement",
            );

        // reserve_a > reserve_b throughout, so price_a (= reserve_b/reserve_a,
        // integer division) truncates to 0, while price_b (= reserve_a/reserve_b)
        // truncates to 1 — deterministic given the reserve trajectory above.
        assert_eq!(
            price_a_avg, 0,
            "price_a TWAP should reflect reserve_b/reserve_a ratio for window {window}"
        );
        assert_eq!(
            price_b_avg, 1,
            "price_b TWAP should reflect reserve_a/reserve_b ratio for window {window}"
        );
    }
}
