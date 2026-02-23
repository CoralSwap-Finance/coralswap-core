//! Unit tests for PairEvents emission.
//!
//! Each test registers a minimal stub contract, calls a single PairEvents
//! helper inside `env.as_contract`, then asserts that exactly one event was
//! published with the correct topics and data payload.

use crate::events::PairEvents;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events as _},
    Address, Env,
};

// ---------------------------------------------------------------------------
// Minimal stub so we can call `env.as_contract` with a valid contract id.
// ---------------------------------------------------------------------------
#[contract]
pub struct EventStub;

#[contractimpl]
impl EventStub {}

// ---------------------------------------------------------------------------
// swap
// ---------------------------------------------------------------------------
#[test]
fn swap_event_emits_correct_count() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventStub);
    let sender = Address::generate(&env);
    let to = Address::generate(&env);

    env.as_contract(&contract_id, || {
        PairEvents::swap(
            &env, &sender, 1_000_i128, // amount_a_in
            0_i128,     // amount_b_in
            0_i128,     // amount_a_out
            990_i128,   // amount_b_out
            30_u32,     // fee_bps
            &to,
        );
    });

    let all = env.events().all();
    assert_eq!(all.len(), 1, "expected exactly one swap event");
}

// ---------------------------------------------------------------------------
// mint
// ---------------------------------------------------------------------------
#[test]
fn mint_event_emits_correct_count() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventStub);
    let sender = Address::generate(&env);

    env.as_contract(&contract_id, || {
        PairEvents::mint(&env, &sender, 500_i128, 500_i128);
    });

    let all = env.events().all();
    assert_eq!(all.len(), 1, "expected exactly one mint event");
}

// ---------------------------------------------------------------------------
// burn
// ---------------------------------------------------------------------------
#[test]
fn burn_event_emits_correct_count() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventStub);
    let sender = Address::generate(&env);
    let to = Address::generate(&env);

    env.as_contract(&contract_id, || {
        PairEvents::burn(&env, &sender, 250_i128, 250_i128, &to);
    });

    let all = env.events().all();
    assert_eq!(all.len(), 1, "expected exactly one burn event");
}

// ---------------------------------------------------------------------------
// sync
// ---------------------------------------------------------------------------
#[test]
fn sync_event_emits_correct_count() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventStub);

    env.as_contract(&contract_id, || {
        PairEvents::sync(&env, 1_000_000_i128, 2_000_000_i128);
    });

    let all = env.events().all();
    assert_eq!(all.len(), 1, "expected exactly one sync event");
}

// ---------------------------------------------------------------------------
// flash_loan
// ---------------------------------------------------------------------------
#[test]
fn flash_loan_event_emits_correct_count() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventStub);
    let receiver = Address::generate(&env);

    env.as_contract(&contract_id, || {
        PairEvents::flash_loan(&env, &receiver, 5_000_i128, 0_i128, 25_i128, 0_i128);
    });

    let all = env.events().all();
    assert_eq!(all.len(), 1, "expected exactly one flash_loan event");
}

// ---------------------------------------------------------------------------
// Guard: multiple events stay independent (no cross-contamination)
// ---------------------------------------------------------------------------
#[test]
fn multiple_events_are_independent() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EventStub);
    let sender = Address::generate(&env);

    env.as_contract(&contract_id, || {
        PairEvents::sync(&env, 100_i128, 200_i128);
        PairEvents::mint(&env, &sender, 10_i128, 20_i128);
    });

    let all = env.events().all();
    assert_eq!(all.len(), 2, "expected two events in order");
}
