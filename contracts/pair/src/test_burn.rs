//! Unit tests for Pair::burn()
use crate::{math::MINIMUM_LIQUIDITY, Pair, PairClient, PairError};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token::Client as TokenClient,
    Address, Env, IntoVal, Vec,
};

// --- Contract Wasm Paths ---
const PAIR_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/coralswap_pair.wasm");
const LP_TOKEN_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/coralswap_lp_token.wasm");
const TOKEN_WASM: &[u8] =
    include_bytes!("../../../../target/wasm32-unknown-unknown/release/soroban_token_contract.wasm");

/// Sets up a test environment with a Pair contract, its tokens, and initial liquidity.
fn setup_test_with_liquidity<'a>() -> (
    Env,
    PairClient<'a>,
    Address, // admin
    Address, // user
    TokenClient<'a>,
    TokenClient<'a>,
    TokenClient<'a>, // lp_token
) {
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
    token_a.initialize(&admin, &7, &"Token A".into_val(&env), &"TKNA".into_val(&env));
    token_b.initialize(&admin, &7, &"Token B".into_val(&env), &"TKNB".into_val(&env));

    // Initialize pair
    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_token_id);
    let lp_token = TokenClient::new(&env, &pair_client.lp_token());

    // Mint initial liquidity
    let user = Address::generate(&env);
    let initial_a = 1_000_000;
    let initial_b = 4_000_000;
    token_a.mint(&admin, &user, &initial_a);
    token_b.mint(&admin, &user, &initial_b);
    token_a.transfer(&user, &pair_client.address, &initial_a);
    token_b.transfer(&user, &pair_client.address, &initial_b);
    pair_client.mint(&user);

    (env, pair_client, admin, user, token_a, token_b, lp_token)
}

#[test]
fn test_full_withdrawal() {
    let (env, pair_client, _admin, user, token_a, token_b, lp_token) = setup_test_with_liquidity();
    let pair_address = pair_client.address.clone();

    let user_lp_balance = lp_token.balance(&user);
    let total_lp_supply = lp_token.total_supply();
    let (reserve_a, reserve_b, _) = pair_client.get_reserves();

    // Transfer all user's LP tokens to the pair for burning
    lp_token.transfer(&user, &pair_address, &user_lp_balance);

    let (amount_a, amount_b) = pair_client.burn(&user);

    // Expected amounts back
    let expected_a = user_lp_balance * reserve_a / total_lp_supply;
    let expected_b = user_lp_balance * reserve_b / total_lp_supply;

    assert_eq!(amount_a, expected_a);
    assert_eq!(amount_b, expected_b);

    // User should have received the tokens
    assert_eq!(token_a.balance(&user), expected_a);
    assert_eq!(token_b.balance(&user), expected_b);

    // Pair reserves should be updated (decreased)
    let (new_reserve_a, new_reserve_b, _) = pair_client.get_reserves();
    assert_eq!(new_reserve_a, reserve_a - expected_a);
    assert_eq!(new_reserve_b, reserve_b - expected_b);

    // LP tokens should be burned
    assert_eq!(lp_token.total_supply(), total_lp_supply - user_lp_balance);
}

#[test]
fn test_partial_withdrawal() {
    let (env, pair_client, _admin, user, token_a, token_b, lp_token) = setup_test_with_liquidity();
    let pair_address = pair_client.address.clone();

    let user_lp_balance = lp_token.balance(&user);
    let total_lp_supply = lp_token.total_supply();
    let (initial_reserve_a, initial_reserve_b, _) = pair_client.get_reserves();

    let burn_amount = user_lp_balance / 2;
    lp_token.transfer(&user, &pair_address, &burn_amount);

    let (amount_a, amount_b) = pair_client.burn(&user);

    let expected_a = burn_amount * initial_reserve_a / total_lp_supply;
    let expected_b = burn_amount * initial_reserve_b / total_lp_supply;

    assert_eq!(amount_a, expected_a);
    assert_eq!(amount_b, expected_b);

    assert_eq!(token_a.balance(&user), expected_a);
    assert_eq!(token_b.balance(&user), expected_b);

    let (new_reserve_a, new_reserve_b, _) = pair_client.get_reserves();
    assert_eq!(new_reserve_a, initial_reserve_a - expected_a);
    assert_eq!(new_reserve_b, initial_reserve_b - expected_b);

    assert_eq!(lp_token.total_supply(), total_lp_supply - burn_amount);
    assert_eq!(lp_token.balance(&user), user_lp_balance - burn_amount);
}

#[test]
fn test_burn_insufficient_liquidity_fails() {
    let (env, pair_client, _admin, user, _token_a, _token_b, lp_token) =
        setup_test_with_liquidity();
    let pair_address = pair_client.address.clone();

    // A very small amount of LP tokens that would result in 0 of one token
    let dust_amount = 1;
    lp_token.transfer(&user, &pair_address, &dust_amount);

    let result = pair_client.try_burn(&user);
    assert_eq!(result.err(), Some(Ok(PairError::InsufficientLiquidityBurned)));
}

#[test]
fn test_burn_event() {
    let (env, pair_client, _admin, user, _token_a, _token_b, lp_token) =
        setup_test_with_liquidity();
    let pair_address = pair_client.address.clone();

    let burn_amount = lp_token.balance(&user) / 3;
    lp_token.transfer(&user, &pair_address, &burn_amount);

    let (amount_a, amount_b) = pair_client.burn(&user);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    let expected_topics = (soroban_sdk::symbol_short!("burn"), user.clone()).into_val(&env);

    let expected_data = (amount_a, amount_b, user.clone()).into_val(&env);

    assert_eq!(last_event.topics, expected_topics);
    assert_eq!(last_event.data, expected_data);
}
