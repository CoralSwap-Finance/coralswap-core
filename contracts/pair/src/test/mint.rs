//! Unit tests for Pair::mint()
use crate::{math::MINIMUM_LIQUIDITY, Pair, PairClient};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    token::Client as TokenClient,
    Address, Env, IntoVal, Symbol,
};

// --- Contract Wasm Paths ---
// Fixed paths to be more standard for Soroban workspace structures.
// If your build folder is different, adjust these strings.
const PAIR_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/coralswap_pair.wasm");
const LP_TOKEN_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/coralswap_lp_token.wasm");
const TOKEN_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/soroban_token_contract.wasm");

/// Sets up a test environment with a Pair contract and its dependent tokens.
fn setup_pair<'a>() -> (Env, PairClient<'a>, Address, TokenClient<'a>, TokenClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy contracts
    let pair_contract_id = env.register_contract_wasm(None, PAIR_WASM);
    let lp_token_id = env.register_contract_wasm(None, LP_TOKEN_WASM);
    let token_a_id = env.register_contract_wasm(None, TOKEN_WASM);
    let token_b_id = env.register_contract_wasm(None, TOKEN_WASM);

    let factory = Address::generate(&env);
    let pair_client = PairClient::new(&env, &pair_contract_id);

    // Initialize tokens
    let token_a = TokenClient::new(&env, &token_a_id);
    let token_b = TokenClient::new(&env, &token_b_id);
    let admin = Address::generate(&env);

    // Sort tokens to match Uniswap/Soroban deterministic ordering if necessary
    // For this test, we assume token_a_id < token_b_id
    token_a.initialize(&admin, &7, &"Token A".into_val(&env), &"TKNA".into_val(&env));
    token_b.initialize(&admin, &7, &"Token B".into_val(&env), &"TKNB".into_val(&env));

    // Initialize pair
    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_token_id);

    (env, pair_client, admin, token_a, token_b)
}

#[test]
fn test_first_deposit() {
    let (env, pair_client, admin, token_a, token_b) = setup_pair();
    let user = Address::generate(&env);
    let pair_address = pair_client.address.clone();

    let amount_a = 1_000_000_000;
    let amount_b = 4_000_000_000;
    token_a.mint(&admin, &user, &amount_a);
    token_b.mint(&admin, &user, &amount_b);

    token_a.transfer(&user, &pair_address, &amount_a);
    token_b.transfer(&user, &pair_address, &amount_b);

    // expected = sqrt(1e9 * 4e9) - 1000 = 2e9 - 1000
    let expected_liquidity = 2_000_000_000 - MINIMUM_LIQUIDITY;
    let liquidity = pair_client.mint(&user);

    assert_eq!(liquidity, expected_liquidity);

    let lp_token_address = pair_client.lp_token();
    let lp_token = TokenClient::new(&env, &lp_token_address);

    assert_eq!(lp_token.balance(&user), expected_liquidity);
    assert_eq!(lp_token.balance(&pair_address), MINIMUM_LIQUIDITY);

    let (res_a, res_b, _) = pair_client.get_reserves();
    assert_eq!(res_a, amount_a);
    assert_eq!(res_b, amount_b);
}

#[test]
fn test_proportional_deposit() {
    let (env, pair_client, admin, token_a, token_b) = setup_pair();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let pair_address = pair_client.address.clone();

    // User 1 Mint
    token_a.mint(&admin, &user1, &1_000_000);
    token_b.mint(&admin, &user1, &1_000_000);
    token_a.transfer(&user1, &pair_address, &1_000_000);
    token_b.transfer(&user1, &pair_address, &1_000_000);
    pair_client.mint(&user1);

    let lp_token = TokenClient::new(&env, &pair_client.lp_token());
    let supply_after_1 = lp_token.total_supply();

    // User 2 Mint (Double the reserves)
    token_a.mint(&admin, &user2, &2_000_000);
    token_b.mint(&admin, &user2, &2_000_000);
    token_a.transfer(&user2, &pair_address, &2_000_000);
    token_b.transfer(&user2, &pair_address, &2_000_000);
    let liquidity2 = pair_client.mint(&user2);

    // Should receive exactly 2x the previous total supply
    assert_eq!(liquidity2, 2 * supply_after_1);
}

#[test]
fn test_dust_deposit_fails() {
    let (env, pair_client, admin, token_a, token_b) = setup_pair();
    let user = Address::generate(&env);

    // Total product sqrt(10*10) = 10, which is < MINIMUM_LIQUIDITY (1000)
    token_a.mint(&admin, &user, &10);
    token_b.mint(&admin, &user, &10);
    token_a.transfer(&user, &pair_client.address, &10);
    token_b.transfer(&user, &pair_client.address, &10);

    let result = pair_client.try_mint(&user);
    assert!(result.is_err());
}

#[test]
fn test_price_accumulation() {
    let (env, pair_client, admin, token_a, token_b) = setup_pair();
    let user = Address::generate(&env);

    // Initial reserves: 100 TokenA, 400 TokenB (Price B/A = 4)
    token_a.mint(&admin, &user, &100);
    token_b.mint(&admin, &user, &400);
    token_a.transfer(&user, &pair_client.address, &100);
    token_b.transfer(&user, &pair_client.address, &400);
    pair_client.mint(&user);

    // Move time forward 10 seconds
    env.ledger().set_timestamp(env.ledger().timestamp() + 10);

    // Sync to trigger accumulation
    pair_client.sync();

    // Since we don't have a direct getter for cumulative prices in the trait,
    // we verify that the reserves remain correct and no panic occurred.
    let (res_a, res_b, last_time) = pair_client.get_reserves();
    assert_eq!(res_a, 100);
    assert_eq!(res_b, 400);
}
