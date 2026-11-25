//! Behavior tests for the propose/vote governance lifecycle (issue #161).
//!
//! These tests cover the full lifecycle — proposal creation, the
//! Pending/Active/Defeated/Succeeded state machine, snapshot-weighted vote
//! tallying, and double-vote prevention. Authorization is exercised under
//! scoped trees in [`auth_matrix`]; these tests use a blanket
//! `mock_all_auths` because their subject is state transitions and tallying,
//! not authorization (see the "Authorization Testing" note in ARCHITECTURE.md).
#![cfg(test)]

use crate::errors::GovTokenError;
use crate::storage::{ExecutionPayload, ProposalState};
use crate::{GovernanceToken, GovernanceTokenClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Events as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Bytes, BytesN, Env, String, Symbol, Vec};

// ── Test context ──────────────────────────────────────────────────────────
struct Gov {
    env: Env,
    gov: GovernanceTokenClient<'static>,
}

impl Gov {
    fn new() -> Self {
        let env = Env::default();
        // BLANKET MOCK (issue #314): behavior tests, not authorization. Guards
        // are covered by the `auth_matrix` module.
        env.mock_all_auths();
        let gov_id = env.register(GovernanceToken, ());
        let gov = GovernanceTokenClient::new(&env, &gov_id);
        let admin = Address::generate(&env);
        gov.initialize(
            &admin,
            &18u32,
            &String::from_str(&env, "Coral Governance"),
            &String::from_str(&env, "CGOV"),
        );
        Self { env, gov }
    }

    fn mint(&self, to: &Address, amount: i128) {
        self.gov.mint(to, &amount);
    }

    /// A deterministic description hash from a label string.
    fn desc_hash(&self, label: &str) -> BytesN<32> {
        self.env.crypto().sha256(&Bytes::from_slice(&self.env, label.as_bytes())).into()
    }

    /// A minimal execution payload targeting `target` (the no-op function is
    /// never invoked here — execution is issue #162).
    fn payload(&self, target: &Address) -> ExecutionPayload {
        ExecutionPayload {
            target: target.clone(),
            function: Symbol::new(&self.env, "noop"),
            args: Vec::new(&self.env),
        }
    }

    fn ledger(&self) -> u32 {
        self.env.ledger().sequence()
    }

    fn advance(&self, by: u32) {
        self.env.ledger().set_sequence_number(self.ledger() + by);
    }
}

// ── Governance parameters ─────────────────────────────────────────────────
#[test]
fn test_default_governance_params() {
    let g = Gov::new();
    let params = g.gov.governance_params();
    assert_eq!(params.voting_delay, 1);
    assert_eq!(params.voting_period, 10);
    assert_eq!(params.proposal_threshold, 1_000);
}

#[test]
fn test_set_governance_params() {
    let g = Gov::new();
    g.gov.set_governance_params(&2u32, &20u32, &5_000i128);
    let params = g.gov.governance_params();
    assert_eq!(params.voting_delay, 2);
    assert_eq!(params.voting_period, 20);
    assert_eq!(params.proposal_threshold, 5_000);
}

#[test]
fn test_set_governance_params_rejects_zero_voting_period() {
    let g = Gov::new();
    let result = g.gov.try_set_governance_params(&1u32, &0u32, &1_000i128);
    assert_eq!(result, Err(Ok(GovTokenError::InvalidGovernanceParams)));
    // Defaults are still in effect (no overwrite happened).
    assert_eq!(g.gov.governance_params().voting_period, 10);
}

#[test]
fn test_set_governance_params_rejects_negative_threshold() {
    let g = Gov::new();
    let result = g.gov.try_set_governance_params(&1u32, &10u32, &-1i128);
    assert_eq!(result, Err(Ok(GovTokenError::InvalidGovernanceParams)));
}

// ── Propose ───────────────────────────────────────────────────────────────
#[test]
fn test_propose_below_threshold_fails() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 500); // below the default 1_000 threshold
    let target = Address::generate(&g.env);
    let result = g.gov.try_propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    assert_eq!(result, Err(Ok(GovTokenError::BelowProposalThreshold)));
}

#[test]
fn test_propose_creates_proposal_with_start_and_end() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let l0 = g.ledger();
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    assert_eq!(id, 0);

    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.id, 0);
    assert_eq!(proposal.proposer, proposer);
    assert_eq!(proposal.snapshot_ledger, l0);
    // start = snapshot + voting_delay(1), end = start + voting_period(10)
    assert_eq!(proposal.start_ledger, l0 + 1);
    assert_eq!(proposal.end_ledger, l0 + 11);
    assert_eq!(proposal.for_votes, 0);
    assert_eq!(proposal.against_votes, 0);
}

#[test]
fn test_propose_assigns_increasing_ids() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let target = Address::generate(&g.env);
    let id0 = g.gov.propose(&proposer, &g.desc_hash("p0"), &g.payload(&target));
    let id1 = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
}

// ── Proposal state ────────────────────────────────────────────────────────
#[test]
fn test_proposal_state_pending() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    // current == snapshot < start_ledger (delay = 1) → Pending
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Pending);
}

#[test]
fn test_proposal_state_active() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // to start_ledger
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Active);
    g.advance(10); // to end_ledger
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Active);
}

#[test]
fn test_state_transitions_with_custom_params() {
    let g = Gov::new();
    g.gov.set_governance_params(&2u32, &5u32, &500i128); // delay=2, period=5, threshold=500
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 500);
    let l0 = g.ledger();
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.start_ledger, l0 + 2);
    assert_eq!(proposal.end_ledger, l0 + 7);

    assert_eq!(g.gov.proposal_state(&id), ProposalState::Pending); // l0 < l0+2
    g.advance(2); // to start
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Active); // l0+2
    g.advance(5); // to end
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Active); // l0+7 inclusive
    g.advance(1); // past end
                  // no votes cast → for == against == 0 → Defeated
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Defeated);
}

// ── Voting ────────────────────────────────────────────────────────────────
#[test]
fn test_vote_for() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 700);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&voter, &id, &true);

    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 700);
    assert_eq!(proposal.against_votes, 0);
    assert!(g.gov.has_voted(&id, &voter));
    let receipt = g.gov.get_vote_receipt(&id, &voter).unwrap();
    assert!(receipt.support);
    assert_eq!(receipt.weight, 700);
}

#[test]
fn test_vote_against() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 300);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&voter, &id, &false);

    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 0);
    assert_eq!(proposal.against_votes, 300);
}

#[test]
fn test_vote_uses_snapshot_voting_power() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 100); // 100 voting power at the snapshot ledger
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    // snapshot is the creation ledger; voter currently holds 100 there.
    g.advance(1); // active
                  // Acquire *more* power after the snapshot — it must NOT count.
    g.mint(&voter, 900);
    assert_eq!(g.gov.get_votes(&voter), 1_000); // current power grew…
    g.gov.vote(&voter, &id, &true);

    // …but the vote is weighted by the snapshot (100), not the current (1_000).
    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 100);
    let receipt = g.gov.get_vote_receipt(&id, &voter).unwrap();
    assert_eq!(receipt.weight, 100);
}

#[test]
fn test_double_voting_prevented() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 500);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&voter, &id, &true);

    // A second vote (even with the opposite stance) is rejected.
    let result = g.gov.try_vote(&voter, &id, &false);
    assert_eq!(result, Err(Ok(GovTokenError::AlreadyVoted)));
    // Tally unchanged from the first vote.
    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 500);
    assert_eq!(proposal.against_votes, 0);
}

#[test]
fn test_voting_before_start_fails() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 500);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    // still Pending (current == snapshot < start)
    let result = g.gov.try_vote(&voter, &id, &true);
    assert_eq!(result, Err(Ok(GovTokenError::VotingNotActive)));
}

#[test]
fn test_voting_after_end_fails() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 500);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(12); // past end_ledger (snapshot + 11 + 1)
    let result = g.gov.try_vote(&voter, &id, &true);
    assert_eq!(result, Err(Ok(GovTokenError::VotingNotActive)));
}

#[test]
fn test_vote_with_zero_power_fails() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env); // never minted → 0 power at snapshot
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    let result = g.gov.try_vote(&voter, &id, &true);
    assert_eq!(result, Err(Ok(GovTokenError::NoVotingPower)));
    assert!(!g.gov.has_voted(&id, &voter));
}

#[test]
fn test_vote_on_unknown_proposal_fails() {
    let g = Gov::new();
    let voter = Address::generate(&g.env);
    g.mint(&voter, 500);
    let result = g.gov.try_vote(&voter, &999u32, &true);
    assert_eq!(result, Err(Ok(GovTokenError::ProposalNotFound)));
}

#[test]
fn test_proposal_state_view_on_unknown_fails() {
    let g = Gov::new();
    let result = g.gov.try_proposal_state(&999u32);
    assert_eq!(result, Err(Ok(GovTokenError::ProposalNotFound)));
}

// ── Outcome: Succeeded vs Defeated ────────────────────────────────────────
#[test]
fn test_proposal_succeeded() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let a = Address::generate(&g.env);
    let b = Address::generate(&g.env);
    g.mint(&a, 600); // for
    g.mint(&b, 400); // against
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&a, &id, &true);
    g.gov.vote(&b, &id, &false);
    g.advance(11); // past end_ledger
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Succeeded);
    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 600);
    assert_eq!(proposal.against_votes, 400);
}

#[test]
fn test_proposal_defeated() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let a = Address::generate(&g.env);
    let b = Address::generate(&g.env);
    g.mint(&a, 300); // for
    g.mint(&b, 600); // against
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&a, &id, &true);
    g.gov.vote(&b, &id, &false);
    g.advance(11); // past end
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Defeated);
}

#[test]
fn test_proposal_tie_is_defeated() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let a = Address::generate(&g.env);
    let b = Address::generate(&g.env);
    g.mint(&a, 500); // for
    g.mint(&b, 500); // against → tie
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&a, &id, &true);
    g.gov.vote(&b, &id, &false);
    g.advance(11); // past end
                   // for == against → not strictly greater → Defeated
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Defeated);
}

#[test]
fn test_proposal_with_no_votes_is_defeated() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(12); // past end, no votes cast
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Defeated);
}

// ── Delegated voting power at the snapshot ─────────────────────────────────
#[test]
fn test_vote_with_delegated_power() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    // Alice mints tokens but delegates her voting power to Bob before the
    // proposal is created, so Bob holds the snapshot power.
    let alice = Address::generate(&g.env);
    let bob = Address::generate(&g.env);
    g.mint(&alice, 800);
    g.gov.delegate(&alice, &bob);
    assert_eq!(g.gov.get_votes(&bob), 800);

    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&bob, &id, &true);

    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 800);
}

// ── Full lifecycle ─────────────────────────────────────────────────────────
#[test]
fn test_full_propose_and_vote_lifecycle() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 3_000);
    let for_voter = Address::generate(&g.env);
    let against_voter = Address::generate(&g.env);
    let abstainer = Address::generate(&g.env); // holds tokens but does not vote
    g.mint(&for_voter, 1_500);
    g.mint(&against_voter, 800);
    g.mint(&abstainer, 5_000);

    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("lifecycle"), &g.payload(&target));
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Pending);

    // Voting opens at start_ledger = l0 + 1.
    g.advance(1);
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Active);

    // A vote before/after the window is rejected while the window is open.
    g.gov.vote(&for_voter, &id, &true);
    g.gov.vote(&against_voter, &id, &false);

    // Double voting is prevented.
    let again = g.gov.try_vote(&for_voter, &id, &true);
    assert_eq!(again, Err(Ok(GovTokenError::AlreadyVoted)));

    // The abstainer's power is not counted (they never voted).
    let proposal = g.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 1_500);
    assert_eq!(proposal.against_votes, 800);

    // Voting closes; the proposal succeeds (for > against).
    g.advance(11); // past end_ledger
    assert_eq!(g.gov.proposal_state(&id), ProposalState::Succeeded);
}

#[test]
fn test_propose_emits_event() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let target = Address::generate(&g.env);
    // `events().all()` reports the most recent invocation, so check before
    // any getter call.
    let _ = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    assert_eq!(g.env.events().all().events().len(), 1, "propose must emit exactly one event");
}

#[test]
fn test_vote_emits_event() {
    let g = Gov::new();
    let proposer = Address::generate(&g.env);
    g.mint(&proposer, 2_000);
    let voter = Address::generate(&g.env);
    g.mint(&voter, 500);
    let target = Address::generate(&g.env);
    let id = g.gov.propose(&proposer, &g.desc_hash("p1"), &g.payload(&target));
    g.advance(1); // active
    g.gov.vote(&voter, &id, &true);
    assert_eq!(g.env.events().all().events().len(), 1, "vote must emit exactly one event");
}
