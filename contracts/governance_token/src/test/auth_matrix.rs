//! Scoped-authorization matrix for the governance token contract (issue #314).
//!
//! Mirrors the LP token auth matrix: each test installs an exact authorization
//! tree, so it fails if a `require_auth` is deleted, rebound to the wrong
//! address, or if the contract starts over-authorizing.
//!
//! | Entry point        | Signature                                  | Authorizer      |
//! |--------------------|--------------------------------------------|-----------------|
//! | `admin_transfer`   | `(new_admin)`                              | current `admin` |
//! | `mint`             | `(to, amount)`                             | `admin`         |
//! | `burn`             | `(from, amount)`                           | `from`          |
//! | `approve`          | `(from, spender, amount, expiration)`      | `from`          |
//! | `transfer`         | `(from, to, amount)`                       | `from`          |
//! | `transfer_from`    | `(spender, from, to, amount)`              | `spender`       |
//! | `delegate`         | `(from, to)`                               | `from`          |
//! | `propose`          | `(proposer, description_hash, payload)`   | `proposer`      |
//! | `vote`             | `(voter, proposal_id, support)`           | `voter`         |
//! | `set_governance_params` | `(delay, period, threshold)`        | `admin`         |

#![cfg(test)]

use coralswap_shared::auth_args;
use coralswap_shared::test_support as auth;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Bytes, BytesN, Env, String, Symbol, Vec};

use crate::storage::ExecutionPayload;
use crate::{GovernanceToken, GovernanceTokenClient};

struct Ctx {
    env: Env,
    gov: GovernanceTokenClient<'static>,
    gov_id: Address,
    admin: Address,
    holder: Address,
    spender: Address,
    recipient: Address,
    delegate_target: Address,
    stranger: Address,
}

impl Ctx {
    fn new() -> Self {
        let env = Env::default();
        let gov_id = env.register(GovernanceToken, ());
        let admin = Address::generate(&env);
        let holder = Address::generate(&env);
        let spender = Address::generate(&env);
        let recipient = Address::generate(&env);
        let delegate_target = Address::generate(&env);
        let stranger = Address::generate(&env);

        let gov = GovernanceTokenClient::new(&env, &gov_id);
        gov.initialize(
            &admin,
            &18u32,
            &String::from_str(&env, "Coral Governance"),
            &String::from_str(&env, "CGOV"),
        );

        let c =
            Self { env, gov, gov_id, admin, holder, spender, recipient, delegate_target, stranger };
        c.mint(&c.holder.clone(), 1_000i128);
        c
    }

    fn mint(&self, to: &Address, amount: i128) {
        auth::allow(
            &self.env,
            &self.admin,
            &self.gov_id,
            "mint",
            auth_args!(&self.env, to.clone(), amount),
        );
        self.gov.mint(to, &amount);
    }

    fn future_ledger(&self) -> u32 {
        self.env.ledger().sequence() + 1_000
    }

    /// A deterministic description hash from a label string.
    fn desc_hash(&self, label: &str) -> BytesN<32> {
        self.env.crypto().sha256(&Bytes::from_slice(&self.env, label.as_bytes())).into()
    }

    /// A minimal execution payload targeting `target`.
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

// ─────────────────────────────────────────────
// Positive: the right authorizer is accepted
// ─────────────────────────────────────────────

#[test]
fn admin_transfer_is_authorized_by_the_current_admin() {
    let c = Ctx::new();
    let next = Address::generate(&c.env);

    auth::allow(&c.env, &c.admin, &c.gov_id, "admin_transfer", auth_args!(&c.env, next.clone()));
    c.gov.admin_transfer(&next);

    auth::assert_authorized(
        &c.env,
        &c.admin,
        &c.gov_id,
        "admin_transfer",
        auth_args!(&c.env, next.clone()),
    );
    assert_eq!(c.gov.admin(), next);
}

#[test]
fn mint_is_authorized_by_the_admin() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.admin,
        &c.gov_id,
        "mint",
        auth_args!(&c.env, c.recipient.clone(), 500i128),
    );
    c.gov.mint(&c.recipient, &500);

    auth::assert_authorized(
        &c.env,
        &c.admin,
        &c.gov_id,
        "mint",
        auth_args!(&c.env, c.recipient.clone(), 500i128),
    );
    assert_eq!(c.gov.balance(&c.recipient), 500i128);
}

#[test]
fn burn_is_authorized_by_the_holder() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "burn",
        auth_args!(&c.env, c.holder.clone(), 400i128),
    );
    c.gov.burn(&c.holder, &400);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.gov_id,
        "burn",
        auth_args!(&c.env, c.holder.clone(), 400i128),
    );
    assert_eq!(c.gov.balance(&c.holder), 600i128);
}

#[test]
fn transfer_is_authorized_by_the_sender() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.recipient.clone(), 250i128),
    );
    c.gov.transfer(&c.holder, &c.recipient, &250);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.gov_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.recipient.clone(), 250i128),
    );
    assert_eq!(c.gov.balance(&c.recipient), 250i128);
    assert_eq!(c.gov.balance(&c.holder), 750i128);
}

#[test]
fn approve_is_authorized_by_the_owner() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    c.gov.approve(&c.holder, &c.spender, &300, &expiry);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.gov_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    assert_eq!(c.gov.allowance(&c.holder, &c.spender), 300i128);
}

#[test]
fn transfer_from_is_authorized_by_the_spender() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    c.gov.approve(&c.holder, &c.spender, &300, &expiry);

    auth::allow(
        &c.env,
        &c.spender,
        &c.gov_id,
        "transfer_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), c.recipient.clone(), 300i128),
    );
    c.gov.transfer_from(&c.spender, &c.holder, &c.recipient, &300);

    auth::assert_authorized(
        &c.env,
        &c.spender,
        &c.gov_id,
        "transfer_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), c.recipient.clone(), 300i128),
    );
    assert_eq!(c.gov.balance(&c.recipient), 300i128);
}

#[test]
fn delegate_is_authorized_by_the_delegator() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "delegate",
        auth_args!(&c.env, c.holder.clone(), c.delegate_target.clone()),
    );
    c.gov.delegate(&c.holder, &c.delegate_target);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.gov_id,
        "delegate",
        auth_args!(&c.env, c.holder.clone(), c.delegate_target.clone()),
    );
    assert_eq!(c.gov.delegates(&c.holder), c.delegate_target);
    assert_eq!(c.gov.get_votes(&c.delegate_target), 1_000i128);
}

#[test]
fn propose_is_authorized_by_the_proposer() {
    let c = Ctx::new();
    let target = Address::generate(&c.env);
    let desc = c.desc_hash("p1");
    let payload = c.payload(&target);
    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "propose",
        auth_args!(&c.env, c.holder.clone(), desc.clone(), payload.clone()),
    );
    let id = c.gov.propose(&c.holder, &desc, &payload);
    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.gov_id,
        "propose",
        auth_args!(&c.env, c.holder.clone(), desc.clone(), payload.clone()),
    );
    assert_eq!(id, 0);
    let proposal = c.gov.get_proposal(&id);
    assert_eq!(proposal.proposer, c.holder);
}

#[test]
fn vote_is_authorized_by_the_voter() {
    let c = Ctx::new();
    // Give the recipient voting power before the proposal's snapshot ledger.
    c.mint(&c.recipient, 500i128);
    // holder creates the proposal.
    let target = Address::generate(&c.env);
    let desc = c.desc_hash("p1");
    let payload = c.payload(&target);
    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "propose",
        auth_args!(&c.env, c.holder.clone(), desc.clone(), payload.clone()),
    );
    let id = c.gov.propose(&c.holder, &desc, &payload);
    c.advance(1); // to the active voting window
                  // recipient casts a vote.
    auth::allow(
        &c.env,
        &c.recipient,
        &c.gov_id,
        "vote",
        auth_args!(&c.env, c.recipient.clone(), id, true),
    );
    c.gov.vote(&c.recipient, &id, &true);
    auth::assert_authorized(
        &c.env,
        &c.recipient,
        &c.gov_id,
        "vote",
        auth_args!(&c.env, c.recipient.clone(), id, true),
    );
    assert!(c.gov.has_voted(&id, &c.recipient));
    let proposal = c.gov.get_proposal(&id);
    assert_eq!(proposal.for_votes, 500i128);
}

#[test]
fn set_governance_params_is_authorized_by_the_admin() {
    let c = Ctx::new();
    auth::allow(
        &c.env,
        &c.admin,
        &c.gov_id,
        "set_governance_params",
        auth_args!(&c.env, 2u32, 20u32, 5_000i128),
    );
    c.gov.set_governance_params(&2u32, &20u32, &5_000i128);
    auth::assert_authorized(
        &c.env,
        &c.admin,
        &c.gov_id,
        "set_governance_params",
        auth_args!(&c.env, 2u32, 20u32, 5_000i128),
    );
    let params = c.gov.governance_params();
    assert_eq!(params.voting_delay, 2);
    assert_eq!(params.voting_period, 20);
    assert_eq!(params.proposal_threshold, 5_000);
}

// ─────────────────────────────────────────────
// Negative: the matrix
// ─────────────────────────────────────────────

#[test]
fn admin_transfer_rejects_a_stranger() {
    let c = Ctx::new();
    let next = Address::generate(&c.env);

    auth::allow(&c.env, &c.stranger, &c.gov_id, "admin_transfer", auth_args!(&c.env, next.clone()));
    assert!(c.gov.try_admin_transfer(&next).is_err(), "a stranger must not reassign the admin");
    assert_eq!(c.gov.admin(), c.admin);
}

#[test]
fn mint_rejects_a_stranger() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "mint",
        auth_args!(&c.env, c.stranger.clone(), 1i128),
    );
    assert!(c.gov.try_mint(&c.stranger, &1).is_err(), "a stranger must not mint governance tokens");
    assert_eq!(c.gov.total_supply(), 1_000i128, "supply must be unchanged");
}

#[test]
fn burn_rejects_a_stranger_burning_another_accounts_tokens() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "burn",
        auth_args!(&c.env, c.holder.clone(), 1_000i128),
    );
    assert!(
        c.gov.try_burn(&c.holder, &1_000).is_err(),
        "burning another account's balance must require that account's authorization"
    );
    assert_eq!(c.gov.balance(&c.holder), 1_000i128, "balance must be unchanged");
}

#[test]
fn transfer_rejects_a_stranger_moving_another_accounts_tokens() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.stranger.clone(), 1_000i128),
    );
    assert!(
        c.gov.try_transfer(&c.holder, &c.stranger, &1_000).is_err(),
        "draining another account's balance must require that account's authorization"
    );
    assert_eq!(c.gov.balance(&c.holder), 1_000i128);
    assert_eq!(c.gov.balance(&c.stranger), 0i128);
}

#[test]
fn approve_rejects_a_stranger_approving_on_anothers_behalf() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.stranger.clone(), 1_000i128, expiry),
    );
    assert!(
        c.gov.try_approve(&c.holder, &c.stranger, &1_000, &expiry).is_err(),
        "a stranger must not open an allowance on someone else's balance"
    );
    assert_eq!(c.gov.allowance(&c.holder, &c.stranger), 0i128);
}

#[test]
fn delegate_rejects_a_stranger() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "delegate",
        auth_args!(&c.env, c.holder.clone(), c.stranger.clone()),
    );
    assert!(
        c.gov.try_delegate(&c.holder, &c.stranger).is_err(),
        "a stranger must not delegate another account's voting power"
    );
    assert_eq!(c.gov.delegates(&c.holder), c.holder);
    assert_eq!(c.gov.get_votes(&c.holder), 1_000i128);
}

#[test]
fn propose_rejects_a_stranger() {
    let c = Ctx::new();
    let target = Address::generate(&c.env);
    let desc = c.desc_hash("p1");
    let payload = c.payload(&target);
    // Authorize a stranger — `propose` requires the proposer's own auth, which
    // the stranger's tree does not supply. The holder meets the threshold, so
    // this fails only if the auth guard is present.
    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "propose",
        auth_args!(&c.env, c.holder.clone(), desc.clone(), payload.clone()),
    );
    assert!(
        c.gov.try_propose(&c.holder, &desc, &payload).is_err(),
        "a stranger must not create a proposal in the holder's name"
    );
    assert!(c.gov.try_get_proposal(&0u32).is_err(), "no proposal must be created");
}

#[test]
fn vote_rejects_a_stranger() {
    let c = Ctx::new();
    c.mint(&c.recipient, 500i128);
    let target = Address::generate(&c.env);
    let desc = c.desc_hash("p1");
    let payload = c.payload(&target);
    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "propose",
        auth_args!(&c.env, c.holder.clone(), desc.clone(), payload.clone()),
    );
    let id = c.gov.propose(&c.holder, &desc, &payload);
    c.advance(1); // active
                  // Authorize a stranger — `vote` requires the voter's own auth.
    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "vote",
        auth_args!(&c.env, c.recipient.clone(), id, true),
    );
    assert!(
        c.gov.try_vote(&c.recipient, &id, &true).is_err(),
        "a stranger must not cast a vote in the recipient's name"
    );
    assert!(!c.gov.has_voted(&id, &c.recipient));
}

#[test]
fn set_governance_params_rejects_a_stranger() {
    let c = Ctx::new();
    auth::allow(
        &c.env,
        &c.stranger,
        &c.gov_id,
        "set_governance_params",
        auth_args!(&c.env, 2u32, 20u32, 5_000i128),
    );
    assert!(
        c.gov.try_set_governance_params(&2u32, &20u32, &5_000i128).is_err(),
        "a stranger must not configure governance parameters"
    );
    assert_eq!(c.gov.governance_params().voting_period, 10, "defaults must be unchanged");
}

#[test]
fn a_vote_authorization_for_a_different_support_value_is_rejected() {
    let c = Ctx::new();
    c.mint(&c.recipient, 500i128);
    let target = Address::generate(&c.env);
    let desc = c.desc_hash("p1");
    let payload = c.payload(&target);
    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "propose",
        auth_args!(&c.env, c.holder.clone(), desc.clone(), payload.clone()),
    );
    let id = c.gov.propose(&c.holder, &desc, &payload);
    c.advance(1); // active
                  // Authorize a "for" vote, then attempt an "against" vote.
    auth::allow(
        &c.env,
        &c.recipient,
        &c.gov_id,
        "vote",
        auth_args!(&c.env, c.recipient.clone(), id, true),
    );
    assert!(
        c.gov.try_vote(&c.recipient, &id, &false).is_err(),
        "an authorization to vote 'for' must not authorize a vote 'against'"
    );
    assert!(!c.gov.has_voted(&id, &c.recipient));
}

#[test]
fn every_gated_entry_point_fails_with_no_authorization_at_all() {
    let c = Ctx::new();
    let expiry = c.future_ledger();
    auth::allow_nothing(&c.env);

    assert!(c.gov.try_admin_transfer(&c.stranger).is_err());
    assert!(c.gov.try_mint(&c.stranger, &1).is_err());
    assert!(c.gov.try_burn(&c.holder, &1).is_err());
    assert!(c.gov.try_burn_from(&c.spender, &c.holder, &1).is_err());
    assert!(c.gov.try_approve(&c.holder, &c.spender, &1, &expiry).is_err());
    assert!(c.gov.try_transfer(&c.holder, &c.recipient, &1).is_err());
    assert!(c.gov.try_transfer_from(&c.spender, &c.holder, &c.recipient, &1).is_err());
    assert!(c.gov.try_delegate(&c.holder, &c.delegate_target).is_err());
    let target = Address::generate(&c.env);
    let desc = c.desc_hash("p1");
    let payload = c.payload(&target);
    assert!(c.gov.try_propose(&c.holder, &desc, &payload).is_err());
    assert!(c.gov.try_vote(&c.holder, &0u32, &true).is_err());
    assert!(c.gov.try_set_governance_params(&1u32, &10u32, &1_000i128).is_err());

    assert_eq!(c.gov.admin(), c.admin);
    assert_eq!(c.gov.total_supply(), 1_000i128);
    assert_eq!(c.gov.balance(&c.holder), 1_000i128);
    assert_eq!(c.gov.delegates(&c.holder), c.holder);
    assert_eq!(c.gov.get_votes(&c.holder), 1_000i128);
    assert!(c.gov.try_get_proposal(&0u32).is_err(), "no proposal must exist");
    assert_eq!(c.gov.governance_params().voting_period, 10, "defaults must be unchanged");
}

/// An authorization bound to the wrong *arguments* must not satisfy the call.
#[test]
fn an_authorization_for_a_different_amount_is_rejected() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.holder,
        &c.gov_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.recipient.clone(), 1_000i128),
    );
    assert!(
        c.gov.try_transfer(&c.holder, &c.recipient, &1).is_err(),
        "an authorization for 1000 must not authorize a 1-token transfer"
    );
    assert_eq!(c.gov.balance(&c.recipient), 0i128);
    assert_eq!(c.gov.balance(&c.holder), 1_000i128);

    // The matching amount does go through.
    c.gov.transfer(&c.holder, &c.recipient, &1_000);
    assert_eq!(c.gov.balance(&c.recipient), 1_000i128);
}

// ─────────────────────────────────────────────
// The permissionless surface stays permissionless
// ─────────────────────────────────────────────

#[test]
fn a_view_call_needs_no_authorization() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    let _ = c.gov.balance(&c.holder);
    let _ = c.gov.total_supply();
    let _ = c.gov.allowance(&c.holder, &c.spender);
    let _ = c.gov.delegates(&c.holder);
    let _ = c.gov.get_votes(&c.holder);
    let _ = c.gov.get_past_votes(&c.holder, &c.env.ledger().sequence());
    let _ = c.gov.get_past_total_supply(&c.env.ledger().sequence());
    let _ = c.gov.admin();
    // Governance views (issue #161) are also permissionless.
    let _ = c.gov.governance_params();
    let _ = c.gov.try_get_proposal(&0u32);
    let _ = c.gov.try_proposal_state(&0u32);
    let _ = c.gov.has_voted(&0u32, &c.holder);
    let _ = c.gov.try_get_vote_receipt(&0u32, &c.holder);

    auth::assert_unauthorized(&c.env);
}
