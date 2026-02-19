#![cfg(test)]

extern crate std;

use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::Address as _,
    Address, Env,
};

use crate::{
    errors::PairError,
    storage::{set_reentrancy_guard, ReentrancyGuard},
    Pair, PairClient,
};

// ── Minimal mock SEP-41 token ─────────────────────────────────────────────────
// Uses raw Symbol keys to avoid #[contracttype] + Arbitrary derive conflict
// in no_std + testutils mode.

/// Very simple token mock that stores per-address balances in a Vec<(Address, i128)>
/// stored under a single "balances" key. Sufficient for our swap tests.
#[contract]
pub struct MockToken;

// We store balances as a Vec serialised in instance storage.
// Key = symbol "bals", value = Vec<(Address, i128)>.

fn get_bal(env: &Env, id: &Address) -> i128 {
    let bals: soroban_sdk::Vec<(Address, i128)> = env
        .storage()
        .instance()
        .get(&symbol_short!("bals"))
        .unwrap_or(soroban_sdk::Vec::new(env));
    for i in 0..bals.len() {
        let (addr, amt) = bals.get(i).unwrap();
        if addr == *id {
            return amt;
        }
    }
    0
}

fn set_bal(env: &Env, id: &Address, amount: i128) {
    let mut bals: soroban_sdk::Vec<(Address, i128)> = env
        .storage()
        .instance()
        .get(&symbol_short!("bals"))
        .unwrap_or(soroban_sdk::Vec::new(env));
    for i in 0..bals.len() {
        let (addr, _) = bals.get(i).unwrap();
        if addr == *id {
            bals.set(i, (id.clone(), amount));
            env.storage().instance().set(&symbol_short!("bals"), &bals);
            return;
        }
    }
    bals.push_back((id.clone(), amount));
    env.storage().instance().set(&symbol_short!("bals"), &bals);
}

#[contractimpl]
impl MockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        set_bal(&env, &to, get_bal(&env, &to) + amount);
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        get_bal(&env, &id)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        set_bal(&env, &from, get_bal(&env, &from) - amount);
        set_bal(&env, &to, get_bal(&env, &to) + amount);
    }

    // SEP-41 required stubs
    pub fn approve(_env: Env, _from: Address, _spender: Address, _amount: i128, _exp: u32) {}
    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 { 0 }
    pub fn transfer_from(_env: Env, _sp: Address, _from: Address, _to: Address, _amt: i128) {}
    pub fn burn(_env: Env, _from: Address, _amount: i128) {}
    pub fn burn_from(_env: Env, _sp: Address, _from: Address, _amount: i128) {}
    pub fn decimals(_env: Env) -> u32 { 7 }
    pub fn name(env: Env) -> soroban_sdk::String { soroban_sdk::String::from_str(&env, "Mock") }
    pub fn symbol(env: Env) -> soroban_sdk::String { soroban_sdk::String::from_str(&env, "MCK") }
}

// ── Setup helper ──────────────────────────────────────────────────────────────

fn make_pool(
    env: &Env,
    reserve_a: i128,
    reserve_b: i128,
) -> (Address, Address, Address) {
    let factory = Address::generate(env);
    let lp_token = Address::generate(env);

    let token_a = env.register_contract(None, MockToken);
    let token_b = env.register_contract(None, MockToken);
    let pair_addr = env.register_contract(None, Pair);

    PairClient::new(env, &pair_addr)
        .initialize(&factory, &token_a, &token_b, &lp_token);

    // Seed reserves: mint into pair, then sync reserves into storage.
    MockTokenClient::new(env, &token_a).mint(&pair_addr, &reserve_a);
    MockTokenClient::new(env, &token_b).mint(&pair_addr, &reserve_b);
    PairClient::new(env, &pair_addr).sync();

    (pair_addr, token_a, token_b)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_swap_basic_b_out() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, token_b) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // Pre-deposit token_a into the pair (simulates the caller's transfer).
    // 115_000 A in comfortably covers the 30bps fee for 100_000 B out.
    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &115_000);

    let to = Address::generate(&env);
    let result = pair.try_swap(&0, &100_000, &to);
    assert!(result.is_ok(), "basic swap should succeed: {result:?}");
    assert_eq!(
        MockTokenClient::new(&env, &token_b).balance(&to),
        100_000,
        "recipient should receive 100_000 token_b"
    );
}

#[test]
fn test_swap_reverts_zero_output() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    let to = Address::generate(&env);
    let err = pair.try_swap(&0, &0, &to).unwrap_err().unwrap();
    assert_eq!(err, PairError::InsufficientOutputAmount);
}

#[test]
fn test_swap_reverts_invalid_k_no_input() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // No token_a pre-deposited → amount_in = 0 → K violated.
    let to = Address::generate(&env);
    let err = pair.try_swap(&0, &100_000, &to).unwrap_err().unwrap();
    assert!(
        err == PairError::InvalidK || err == PairError::InsufficientInputAmount,
        "expected InvalidK or InsufficientInputAmount, got {err:?}"
    );
}

#[test]
fn test_swap_reverts_output_exceeds_reserves() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, _) = make_pool(&env, 1_000_000, 500_000);
    let pair = PairClient::new(&env, &pair_addr);

    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &999_999_999);
    let to = Address::generate(&env);
    let err = pair
        .try_swap(&0, &600_000, &to)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, PairError::InsufficientLiquidity);
}

#[test]
fn test_swap_reentrancy_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // Manually lock the guard using the contract's context.
    env.as_contract(&pair_addr, || {
        set_reentrancy_guard(&env, &ReentrancyGuard { locked: true });
    });

    let to = Address::generate(&env);
    let err = pair.try_swap(&0, &1_000, &to).unwrap_err().unwrap();
    assert_eq!(err, PairError::Locked);

    // Reset guard.
    env.as_contract(&pair_addr, || {
        set_reentrancy_guard(&env, &ReentrancyGuard { locked: false });
    });
}

#[test]
fn test_swap_fee_sufficient_input_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &10_500);
    let to = Address::generate(&env);
    let result = pair.try_swap(&0, &10_000, &to);
    assert!(
        result.is_ok(),
        "swap with sufficient input+fee should succeed: {result:?}"
    );
}

#[test]
fn test_get_reserves_reflects_swap() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, token_a, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    let (ra, rb, _) = pair.get_reserves();
    assert_eq!(ra, 1_000_000);
    assert_eq!(rb, 1_000_000);

    MockTokenClient::new(&env, &token_a).mint(&pair_addr, &11_000);
    let to = Address::generate(&env);
    pair.swap(&0, &10_000, &to);

    let (ra2, rb2, _) = pair.get_reserves();
    assert!(ra2 > 1_000_000, "reserve_a should increase after swap");
    assert_eq!(rb2, 990_000, "reserve_b should equal 1_000_000 - 10_000");
}

#[test]
fn test_get_current_fee_bps_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (pair_addr, _, _) = make_pool(&env, 1_000_000, 1_000_000);
    let pair = PairClient::new(&env, &pair_addr);

    // No trades → vol_accumulator = 0 → baseline 30 bps.
    assert_eq!(pair.get_current_fee_bps(), 30);
}
