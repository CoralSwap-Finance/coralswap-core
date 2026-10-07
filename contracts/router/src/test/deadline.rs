//! Optional ledger-number deadline on every router swap / liquidity entry
//! point (issue #365).
//!
//! `deadline_ledger: Option<u32>` is checked alongside the existing timestamp
//! `deadline`, before any pair or token is touched. `None` keeps the old
//! behaviour, so a caller that does not care simply passes `None`.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use super::{deploy_router, RouterClient};

const NOW_LEDGER: u32 = 100;
const EXPIRED: u32 = 300; // RouterError::Expired
const ZERO_AMOUNT: u32 = 306; // RouterError::ZeroAmount — first check after the deadline

/// Extracts the contract error code from a failed `try_*` call.
macro_rules! code {
    ($call:expr) => {
        match $call {
            Err(Ok(e)) => e.get_code(),
            other => panic!("expected a contract error, got {:?}", other.map(|_| ())),
        }
    };
}

fn setup() -> (Env, RouterClient<'static>, Vec<Address>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| {
        l.sequence_number = NOW_LEDGER;
        l.timestamp = 2000;
    });
    let (router_id, _factory) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let mut path = Vec::new(&env);
    path.push_back(Address::generate(&env));
    path.push_back(Address::generate(&env));
    let to = Address::generate(&env);
    (env, router, path, to)
}

// Every call below uses a zero amount, so a call that clears the deadline stops
// at `ZeroAmount` without needing pairs, tokens or liquidity.

#[test]
fn test_ledger_deadline_expired_rejected_on_every_entry_point() {
    let (env, r, path, to) = setup();
    let (a, b) = (Address::generate(&env), Address::generate(&env));
    let stale = Some(NOW_LEDGER - 1);
    let ts = u64::MAX;

    assert_eq!(code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &ts, &stale)), EXPIRED);
    assert_eq!(code!(r.try_swap_exact_tokens_for_tokens(&0, &0, &path, &to, &ts, &stale)), EXPIRED);
    assert_eq!(code!(r.try_swap_tokens_for_exact_tokens(&0, &0, &path, &to, &ts, &stale)), EXPIRED);
    assert_eq!(code!(r.try_add_liquidity(&a, &b, &0, &0, &0, &0, &to, &ts, &stale)), EXPIRED);
    assert_eq!(code!(r.try_remove_liquidity(&a, &b, &0, &0, &0, &to, &ts, &stale)), EXPIRED);
}

#[test]
fn test_ledger_deadline_boundary_is_inclusive() {
    let (env, r, path, to) = setup();
    let (a, b) = (Address::generate(&env), Address::generate(&env));
    // deadline_ledger == current ledger is still valid.
    let now = Some(NOW_LEDGER);
    let ts = u64::MAX;

    assert_eq!(
        code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &ts, &now)),
        ZERO_AMOUNT
    );
    assert_eq!(
        code!(r.try_swap_exact_tokens_for_tokens(&0, &0, &path, &to, &ts, &now)),
        ZERO_AMOUNT
    );
    assert_eq!(
        code!(r.try_swap_tokens_for_exact_tokens(&0, &0, &path, &to, &ts, &now)),
        ZERO_AMOUNT
    );
    assert_eq!(code!(r.try_add_liquidity(&a, &b, &0, &0, &0, &0, &to, &ts, &now)), ZERO_AMOUNT);
    assert_eq!(code!(r.try_remove_liquidity(&a, &b, &0, &0, &0, &to, &ts, &now)), ZERO_AMOUNT);
}

#[test]
fn test_no_ledger_deadline_keeps_previous_behaviour() {
    let (env, r, path, to) = setup();
    let (a, b) = (Address::generate(&env), Address::generate(&env));
    let ts = u64::MAX;

    // Far in the future, and `None`, must behave identically: no bound.
    for none_like in [None, Some(u32::MAX)] {
        assert_eq!(
            code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &ts, &none_like)),
            ZERO_AMOUNT
        );
        assert_eq!(
            code!(r.try_add_liquidity(&a, &b, &0, &0, &0, &0, &to, &ts, &none_like)),
            ZERO_AMOUNT
        );
        assert_eq!(
            code!(r.try_remove_liquidity(&a, &b, &0, &0, &0, &to, &ts, &none_like)),
            ZERO_AMOUNT
        );
    }
}

#[test]
fn test_ledger_deadline_tracks_ledger_advance() {
    let (env, r, path, to) = setup();
    let limit = Some(NOW_LEDGER + 5);

    assert_eq!(
        code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &u64::MAX, &limit)),
        ZERO_AMOUNT
    );

    // Six ledgers later the same call is stale.
    env.ledger().with_mut(|l| l.sequence_number = NOW_LEDGER + 6);
    assert_eq!(
        code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &u64::MAX, &limit)),
        EXPIRED
    );
}

#[test]
fn test_timestamp_and_ledger_deadlines_are_independent() {
    let (env, r, path, to) = setup();
    let past_ts = env.ledger().timestamp().saturating_sub(1);
    // Stale timestamp is still rejected even with a generous ledger bound.
    assert_eq!(
        code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &past_ts, &Some(u32::MAX))),
        EXPIRED
    );
    assert_eq!(
        code!(r.try_swap_exact_tokens_multi_hop(&path, &0, &0, &to, &past_ts, &None)),
        EXPIRED
    );
}
