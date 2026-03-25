//! Unit tests for Pair::mint()
use coralswap_lp_token::{LpToken, LpTokenClient};

use crate::{math::MINIMUM_LIQUIDITY, Pair, PairClient};
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

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
        let key = MockTokenKey::Balance(to);
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

// ── Shared setup ─────────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
fn setup_pair() -> (
    Env,
    PairClient<'static>,
    MockTokenClient<'static>,
    MockTokenClient<'static>,
    LpTokenClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

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

    lp_client.initialize(
        &admin,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );

    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    (env, pair_client, token_a, token_b, lp_client)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_first_deposit() {
    let (env, pair_client, token_a, token_b, lp_client) = setup_pair();
    let user = Address::generate(&env);
    let pair_address = pair_client.address.clone();

    let amount_a = 1_000_000_000_i128;
    let amount_b = 4_000_000_000_i128;
    token_a.mint(&user, &amount_a);
    token_b.mint(&user, &amount_b);
    token_a.transfer(&user, &pair_address, &amount_a);
    token_b.transfer(&user, &pair_address, &amount_b);

    let expected_liquidity = 2_000_000_000 - MINIMUM_LIQUIDITY;
    let liquidity = pair_client.mint(&user);

    assert_eq!(liquidity, expected_liquidity);
    assert_eq!(lp_client.balance(&user), expected_liquidity);
    assert_eq!(lp_client.balance(&pair_address), MINIMUM_LIQUIDITY);

    let (res_a, res_b, _) = pair_client.get_reserves();
    assert_eq!(res_a, amount_a);
    assert_eq!(res_b, amount_b);
}

#[test]
fn test_proportional_deposit() {
    let (env, pair_client, token_a, token_b, lp_client) = setup_pair();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let pair_address = pair_client.address.clone();

    token_a.mint(&user1, &1_000_000_i128);
    token_b.mint(&user1, &1_000_000_i128);
    token_a.transfer(&user1, &pair_address, &1_000_000_i128);
    token_b.transfer(&user1, &pair_address, &1_000_000_i128);
    pair_client.mint(&user1);

    let supply_after_1 = lp_client.total_supply();

    token_a.mint(&user2, &2_000_000_i128);
    token_b.mint(&user2, &2_000_000_i128);
    token_a.transfer(&user2, &pair_address, &2_000_000_i128);
    token_b.transfer(&user2, &pair_address, &2_000_000_i128);
    let liquidity2 = pair_client.mint(&user2);

    assert_eq!(liquidity2, 2 * supply_after_1);
}

#[test]
fn test_dust_deposit_fails() {
    let (env, pair_client, token_a, token_b, _lp_client) = setup_pair();
    let user = Address::generate(&env);

    // sqrt(10 * 10) = 10 < MINIMUM_LIQUIDITY (1000) → should fail
    token_a.mint(&user, &10_i128);
    token_b.mint(&user, &10_i128);
    token_a.transfer(&user, &pair_client.address, &10_i128);
    token_b.transfer(&user, &pair_client.address, &10_i128);

    let result = pair_client.try_mint(&user);
    assert!(result.is_err());
}

#[test]
fn test_price_accumulation() {
    let (env, pair_client, token_a, token_b, _lp_client) = setup_pair();
    let user = Address::generate(&env);

    token_a.mint(&user, &100_i128);
    token_b.mint(&user, &400_i128);
    token_a.transfer(&user, &pair_client.address, &100_i128);
    token_b.transfer(&user, &pair_client.address, &400_i128);
    pair_client.mint(&user);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    pair_client.sync();

    let (res_a, res_b, _) = pair_client.get_reserves();
    assert_eq!(res_a, 100);
    assert_eq!(res_b, 400);
}
