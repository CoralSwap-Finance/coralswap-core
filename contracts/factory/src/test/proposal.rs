//! Unit tests for `create_proposal`, `queue_proposal`, and `execute_proposal`
//! with timelock enforcement (issue #162).
//!
//! Acceptance criteria:
//!  AC-1  Only `Succeeded` proposals can be queued.
//!  AC-2  `execute_proposal` fails if called before the timelock expires.
//!  AC-3  `execute_proposal` successfully calls the target contract.
//!  AC-4  Unit tests cover timelock constraints and execution.

#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Bytes, Env, Val, Vec,
};

use crate::{errors::FactoryError, proposal::PROPOSAL_DELAY_LEDGERS, Factory, FactoryClient};

// ---------------------------------------------------------------------------
// Minimal stub contract used as `target` in cross-contract execution tests.
// ---------------------------------------------------------------------------

/// A trivial contract whose `ping` method can be targeted by a governance
/// proposal.  `env.invoke_contract` requires the target to actually be
/// registered; this stub provides that without any side effects.
#[contract]
pub struct TargetContract;

#[contractimpl]
impl TargetContract {
    pub fn ping(_env: Env) {}
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Deploys the factory with 3 signers and returns the commonly-used handles.
fn setup() -> (Env, FactoryClient<'static>, Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();

    let factory_id = env.register(Factory, ());
    let client = FactoryClient::new(&env, &factory_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let s3 = Address::generate(&env);
    let signers = Vec::from_array(&env, [s1, s2, s3]);

    let pair_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
    let lp_wasm = env.deployer().upload_contract_wasm(Bytes::new(&env));
    let fee_setter = Address::generate(&env);

    client.initialize(&signers, &pair_wasm, &pair_wasm, &lp_wasm, &fee_setter);
    (env, client, signers)
}

/// Deploys the factory + TargetContract; returns handles and the target address.
fn setup_with_target() -> (Env, FactoryClient<'static>, Vec<Address>, Address) {
    let (env, client, signers) = setup();
    let target_id = env.register(TargetContract, ());
    (env, client, signers, target_id)
}

/// Returns the first `k` elements of `v`.
fn first(v: &Vec<Address>, k: u32) -> Vec<Address> {
    let mut out = Vec::new(v.env());
    for i in 0..k {
        out.push_back(v.get(i).unwrap());
    }
    out
}

/// Builds an empty `Vec<Val>` for proposals that take no arguments.
fn no_args(env: &Env) -> Vec<Val> {
    Vec::new(env)
}

/// Advance the ledger sequence by `delta` ledgers.
fn advance_ledger(env: &Env, delta: u32) {
    env.ledger().with_mut(|li| {
        li.sequence_number += delta;
    });
}

// ---------------------------------------------------------------------------
// AC-1  Only Succeeded proposals can be queued
// ---------------------------------------------------------------------------

#[test]
fn test_queue_succeeds_on_newly_created_proposal() {
    let (env, client, signers, target) = setup_with_target();
    // threshold for n=3 is 2
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));

    // Should succeed immediately — proposal starts in Succeeded state.
    client.queue_proposal(&id);
}

#[test]
fn test_queue_fails_if_already_queued() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);

    // Second queue must fail with ProposalAlreadyQueued.
    let err = client.try_queue_proposal(&id).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalAlreadyQueued);
}

#[test]
fn test_queue_fails_on_nonexistent_proposal() {
    let (_env, client, _, _) = setup_with_target();
    let err = client.try_queue_proposal(&999u32).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalNotFound);
}

#[test]
fn test_queue_fails_on_already_executed_proposal() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));

    // Queue → advance past timelock → execute.
    client.queue_proposal(&id);
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS + 1);
    client.execute_proposal(&id);

    // Trying to queue an already-executed proposal must fail.
    let err = client.try_queue_proposal(&id).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalAlreadyExecuted);
}

// ---------------------------------------------------------------------------
// AC-2  Execution fails if called before the timelock expires
// ---------------------------------------------------------------------------

#[test]
fn test_execute_fails_before_timelock_expires() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);

    // Advance ledger — but not enough to clear the timelock.
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS - 1);

    let err = client.try_execute_proposal(&id).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalTimelockNotExpired);
}

#[test]
fn test_execute_fails_at_exactly_one_ledger_before_expiry() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);

    // One ledger short of the required delay.
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS - 1);

    let err = client.try_execute_proposal(&id).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalTimelockNotExpired);
}

#[test]
fn test_execute_succeeds_at_exactly_delay_ledgers() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);

    // Advance to exactly `PROPOSAL_DELAY_LEDGERS` after queuing.
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS);

    // Must succeed — timelock boundary is inclusive.
    client.execute_proposal(&id);
}

#[test]
fn test_execute_fails_without_queuing_first() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));

    // Do NOT queue — attempt direct execution after enough time.
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS + 1);

    let err = client.try_execute_proposal(&id).unwrap_err().unwrap();
    // Proposal is still in Succeeded state (never queued).
    assert_eq!(err, FactoryError::ProposalNotSucceeded);
}

// ---------------------------------------------------------------------------
// AC-3  Execution successfully calls the target contract with the payload
// ---------------------------------------------------------------------------

#[test]
fn test_execute_calls_target_contract() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));

    client.queue_proposal(&id);
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS);

    // Must succeed — the cross-contract call to TargetContract::ping completed.
    client.execute_proposal(&id);
}

#[test]
fn test_execute_marks_proposal_as_executed() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));

    client.queue_proposal(&id);
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS);
    client.execute_proposal(&id);

    // A second execute attempt must fail with ProposalAlreadyExecuted.
    let err = client.try_execute_proposal(&id).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalAlreadyExecuted);
}

#[test]
fn test_execute_on_nonexistent_proposal_fails() {
    let (_env, client, _, _) = setup_with_target();
    let err = client.try_execute_proposal(&999u32).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalNotFound);
}

// ---------------------------------------------------------------------------
// AC-4  Additional timelock constraint and lifecycle tests
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_proposals_are_independent() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let fn_sym = symbol_short!("ping");

    let id0 = client.create_proposal(&quorum, &target, &fn_sym, &no_args(&env));
    let id1 = client.create_proposal(&quorum, &target, &fn_sym, &no_args(&env));

    assert_ne!(id0, id1, "each proposal must get a unique ID");

    // Queue only id0; id1 remains in Succeeded state.
    client.queue_proposal(&id0);

    // id1 is Succeeded but not queued — execute must reject it.
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS + 1);
    let err = client.try_execute_proposal(&id1).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::ProposalNotSucceeded);

    // id0 executes fine.
    client.execute_proposal(&id0);
}

#[test]
fn test_queue_fails_on_factory_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let factory_id = env.register(Factory, ());
    let client = FactoryClient::new(&env, &factory_id);

    let err = client.try_queue_proposal(&0u32).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::NotInitialized);
}

#[test]
fn test_execute_fails_on_factory_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let factory_id = env.register(Factory, ());
    let client = FactoryClient::new(&env, &factory_id);

    let err = client.try_execute_proposal(&0u32).unwrap_err().unwrap();
    assert_eq!(err, FactoryError::NotInitialized);
}

#[test]
fn test_create_proposal_requires_multisig_quorum() {
    let (env, client, signers, target) = setup_with_target();
    // Only 1 signer for a 3-signer factory (threshold = 2) → InsufficientSignatures.
    let below = first(&signers, 1);

    let err = client
        .try_create_proposal(&below, &target, &symbol_short!("ping"), &no_args(&env))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, FactoryError::InsufficientSignatures);
}

#[test]
fn test_proposal_ids_are_monotonically_increasing() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let fn_sym = symbol_short!("ping");

    for expected_id in 0u32..5 {
        let id = client.create_proposal(&quorum, &target, &fn_sym, &no_args(&env));
        assert_eq!(id, expected_id);
    }
}

#[test]
fn test_execute_well_after_delay_still_succeeds() {
    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);

    // Advance far beyond the delay.
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS * 10);

    client.execute_proposal(&id);
}

#[test]
fn test_queue_proposal_emits_event() {
    use soroban_sdk::testutils::Events;

    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);

    // At least the prop_qued event must have been emitted.
    let all_events = env.events().all();
    assert!(!all_events.events().is_empty(), "at least one event must have been emitted");
}

#[test]
fn test_execute_proposal_emits_event() {
    use soroban_sdk::testutils::Events;

    let (env, client, signers, target) = setup_with_target();
    let quorum = first(&signers, 2);
    let id = client.create_proposal(&quorum, &target, &symbol_short!("ping"), &no_args(&env));
    client.queue_proposal(&id);
    advance_ledger(&env, PROPOSAL_DELAY_LEDGERS);
    client.execute_proposal(&id);

    // The prop_exec event must have been emitted during execute_proposal.
    let all_events = env.events().all();
    assert!(!all_events.events().is_empty(), "execution must have emitted an event");
}
