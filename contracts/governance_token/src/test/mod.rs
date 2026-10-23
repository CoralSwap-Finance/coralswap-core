mod auth_matrix;

use crate::errors::GovTokenError;
use crate::storage::GovTokenKey;
use crate::{GovernanceToken, GovernanceTokenClient};
use soroban_sdk::testutils::storage::Persistent as _;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env, String};

// ── Test context ──────────────────────────────────────────────────────────

struct Ctx {
    env: Env,
    gov: GovernanceTokenClient<'static>,
    gov_id: Address,
    admin: Address,
}

impl Ctx {
    fn new() -> Self {
        let env = Env::default();
        // BLANKET MOCK: behavior tests, not authorization. Guards are
        // covered by the per-contract `auth_matrix` module.
        env.mock_all_auths();
        let gov_id = env.register(GovernanceToken, ());
        let gov = GovernanceTokenClient::new(&env, &gov_id);
        let admin = Address::generate(&env);
        gov.initialize(
            &admin,
            &18,
            &String::from_str(&env, "Coral Governance"),
            &String::from_str(&env, "CGOV"),
        );
        Self { env, gov, gov_id, admin }
    }

    /// Mint tokens to `to` using the admin, advancing the ledger so each
    /// mint lands on its own checkpoint.
    fn mint(&self, to: &Address, amount: i128) {
        self.gov.mint(to, &amount);
    }
}

// ── Initialization & metadata ──────────────────────────────────────────────

#[test]
fn test_initialize_sets_admin_and_metadata() {
    let c = Ctx::new();
    assert_eq!(c.gov.admin(), c.admin);
    assert_eq!(c.gov.decimals(), 18);
    assert_eq!(c.gov.name(), String::from_str(&c.env, "Coral Governance"));
    assert_eq!(c.gov.symbol(), String::from_str(&c.env, "CGOV"));
    assert_eq!(c.gov.total_supply(), 0);
}

#[test]
fn test_double_initialize_fails() {
    let c = Ctx::new();
    let result = c.gov.try_initialize(
        &c.admin,
        &18,
        &String::from_str(&c.env, "Coral Governance"),
        &String::from_str(&c.env, "CGOV"),
    );
    assert_eq!(result, Err(Ok(GovTokenError::AlreadyInitialized)));
}

#[test]
fn test_invalid_metadata_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let gov_id = env.register(GovernanceToken, ());
    let gov = GovernanceTokenClient::new(&env, &gov_id);
    let admin = Address::generate(&env);

    // decimals > 18
    let result = gov.try_initialize(
        &admin,
        &19,
        &String::from_str(&env, "Test"),
        &String::from_str(&env, "TST"),
    );
    assert_eq!(result, Err(Ok(GovTokenError::InvalidMetadata)));

    // name > 32 chars
    let long_name =
        String::from_str(&env, "ThisIsAVeryLongTokenNameThatExceedsThirtyTwoCharacters!");
    let result = gov.try_initialize(&admin, &18, &long_name, &String::from_str(&env, "TST"));
    assert_eq!(result, Err(Ok(GovTokenError::InvalidMetadata)));
}

// ── Basic token operations ─────────────────────────────────────────────────

#[test]
fn test_mint_and_balance() {
    let c = Ctx::new();
    let user = Address::generate(&c.env);

    c.mint(&user, 1_000);
    assert_eq!(c.gov.balance(&user), 1_000);
    assert_eq!(c.gov.total_supply(), 1_000);
}

#[test]
fn test_transfer() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    c.gov.transfer(&alice, &bob, &400);

    assert_eq!(c.gov.balance(&alice), 600);
    assert_eq!(c.gov.balance(&bob), 400);
}

#[test]
fn test_transfer_insufficient_balance() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 100);
    let result = c.gov.try_transfer(&alice, &bob, &101);
    assert_eq!(result, Err(Ok(GovTokenError::InsufficientBalance)));
}

#[test]
fn test_approve_and_transfer_from() {
    let c = Ctx::new();
    let owner = Address::generate(&c.env);
    let spender = Address::generate(&c.env);
    let recipient = Address::generate(&c.env);
    let expiry = c.env.ledger().sequence() + 1_000;

    c.mint(&owner, 1_000);
    c.gov.approve(&owner, &spender, &500, &expiry);
    assert_eq!(c.gov.allowance(&owner, &spender), 500);

    c.gov.transfer_from(&spender, &owner, &recipient, &300);
    assert_eq!(c.gov.allowance(&owner, &spender), 200);
    assert_eq!(c.gov.balance(&recipient), 300);
    assert_eq!(c.gov.balance(&owner), 700);
}

#[test]
fn test_burn() {
    let c = Ctx::new();
    let user = Address::generate(&c.env);

    c.mint(&user, 1_000);
    c.gov.burn(&user, &400);
    assert_eq!(c.gov.balance(&user), 600);
    assert_eq!(c.gov.total_supply(), 600);
}

#[test]
fn test_admin_transfer() {
    let c = Ctx::new();
    let new_admin = Address::generate(&c.env);

    c.gov.admin_transfer(&new_admin);
    assert_eq!(c.gov.admin(), new_admin);
}

// ── Delegation & voting power ─────────────────────────────────────────────

#[test]
fn test_delegates_default_is_self() {
    let c = Ctx::new();
    let user = Address::generate(&c.env);
    // No delegation set yet → should return self
    assert_eq!(c.gov.delegates(&user), user);
}

#[test]
fn test_self_delegate() {
    let c = Ctx::new();
    let user = Address::generate(&c.env);

    c.mint(&user, 1_000);
    // Explicitly delegate to self
    c.gov.delegate(&user, &user);

    assert_eq!(c.gov.delegates(&user), user);
    assert_eq!(c.gov.get_votes(&user), 1_000);
}

#[test]
fn test_delegate_to_other() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    c.gov.delegate(&alice, &bob);

    assert_eq!(c.gov.delegates(&alice), bob);
    // Alice's voting power moved to Bob
    assert_eq!(c.gov.get_votes(&alice), 0);
    assert_eq!(c.gov.get_votes(&bob), 1_000);
}

#[test]
fn test_redelegate() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);
    let carol = Address::generate(&c.env);

    c.mint(&alice, 1_000);

    // Delegate to Bob
    c.gov.delegate(&alice, &bob);
    assert_eq!(c.gov.get_votes(&bob), 1_000);
    assert_eq!(c.gov.get_votes(&alice), 0);

    // Re-delegate to Carol
    c.gov.delegate(&alice, &carol);
    assert_eq!(c.gov.get_votes(&bob), 0);
    assert_eq!(c.gov.get_votes(&carol), 1_000);
    assert_eq!(c.gov.get_votes(&alice), 0);
}

#[test]
fn test_transfer_changes_delegate_votes() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    c.gov.delegate(&alice, &bob);

    // Alice transfers 300 to a new user (self-delegated by default)
    let dave = Address::generate(&c.env);
    c.gov.transfer(&alice, &dave, &300);

    // Bob's votes decreased by 300 (Alice's delegate)
    assert_eq!(c.gov.get_votes(&bob), 700);
    // Dave's votes = 300 (self-delegated)
    assert_eq!(c.gov.get_votes(&dave), 300);
    assert_eq!(c.gov.balance(&alice), 700);
    assert_eq!(c.gov.balance(&dave), 300);
}

#[test]
fn test_mint_increases_delegate_votes() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 500);
    c.gov.delegate(&alice, &bob);
    assert_eq!(c.gov.get_votes(&bob), 500);

    // Mint more to Alice → Bob's votes increase
    c.mint(&alice, 300);
    assert_eq!(c.gov.get_votes(&bob), 800);
    assert_eq!(c.gov.balance(&alice), 800);
}

#[test]
fn test_burn_decreases_delegate_votes() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    c.gov.delegate(&alice, &bob);
    assert_eq!(c.gov.get_votes(&bob), 1_000);

    // Burn 400 → Bob's votes decrease
    c.gov.burn(&alice, &400);
    assert_eq!(c.gov.get_votes(&bob), 600);
    assert_eq!(c.gov.balance(&alice), 600);
}

#[test]
fn test_delegate_with_zero_balance() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    // Alice has 0 balance, delegates to Bob — no voting power moves
    c.gov.delegate(&alice, &bob);
    assert_eq!(c.gov.delegates(&alice), bob);
    assert_eq!(c.gov.get_votes(&bob), 0);

    // Later, mint to Alice → voting power goes to Bob
    c.mint(&alice, 500);
    assert_eq!(c.gov.get_votes(&bob), 500);
    assert_eq!(c.gov.get_votes(&alice), 0);
}

#[test]
fn test_multiple_delegators() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);
    let carol = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    c.mint(&bob, 500);

    // Both delegate to Carol
    c.gov.delegate(&alice, &carol);
    c.gov.delegate(&bob, &carol);

    assert_eq!(c.gov.get_votes(&carol), 1_500);
    assert_eq!(c.gov.get_votes(&alice), 0);
    assert_eq!(c.gov.get_votes(&bob), 0);
}

// ── Checkpoints & historical voting power ─────────────────────────────────

#[test]
fn test_get_past_votes_basic() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    let ledger_0 = c.env.ledger().sequence();

    // Mint at ledger_0 → Alice has 1_000 voting power (self-delegated)
    c.mint(&alice, 1_000);

    // Advance ledger
    c.env.ledger().set_sequence_number(ledger_0 + 10);

    // Delegate to Bob at ledger_0 + 10
    c.gov.delegate(&alice, &bob);

    // Advance ledger again
    c.env.ledger().set_sequence_number(ledger_0 + 20);

    // Query past votes
    // At ledger_0: Alice had 1_000 votes
    assert_eq!(c.gov.get_past_votes(&alice, &ledger_0), 1_000);
    // At ledger_0 + 5: still 1_000 (before delegation)
    assert_eq!(c.gov.get_past_votes(&alice, &(ledger_0 + 5)), 1_000);
    // At ledger_0 + 10: Alice delegated to Bob → Alice has 0 votes
    assert_eq!(c.gov.get_past_votes(&alice, &(ledger_0 + 10)), 0);
    // At ledger_0 + 10: Bob received 1_000 votes
    assert_eq!(c.gov.get_past_votes(&bob, &(ledger_0 + 10)), 1_000);
}

#[test]
fn test_get_past_votes_before_checkpoint() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);

    let _ledger_0 = c.env.ledger().sequence();
    // Advance to ledger 5 before minting
    c.env.ledger().set_sequence_number(5);
    c.mint(&alice, 1_000);

    // Query at ledger 2 (before any checkpoint) → 0
    assert_eq!(c.gov.get_past_votes(&alice, &2), 0);
}

#[test]
fn test_get_past_votes_future_ledger_rejected() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    let current = c.env.ledger().sequence();
    let result = c.gov.try_get_past_votes(&alice, &(current + 1));
    assert_eq!(result, Err(Ok(GovTokenError::FutureLedger)));
}

#[test]
fn test_checkpoint_same_ledger_collapses() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);
    let carol = Address::generate(&c.env);

    c.mint(&alice, 1_000);

    // All in the same ledger: delegate to Bob then re-delegate to Carol
    c.gov.delegate(&alice, &bob);
    c.gov.delegate(&alice, &carol);

    let current = c.env.ledger().sequence();
    // At the current ledger, Alice's final state is delegated to Carol
    assert_eq!(c.gov.get_past_votes(&alice, &current), 0);
    assert_eq!(c.gov.get_past_votes(&bob, &current), 0);
    assert_eq!(c.gov.get_past_votes(&carol, &current), 1_000);
}

#[test]
fn test_get_past_total_supply() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);

    let ledger_0 = c.env.ledger().sequence();
    c.mint(&alice, 1_000);

    c.env.ledger().set_sequence_number(ledger_0 + 10);
    c.gov.burn(&alice, &400);

    c.env.ledger().set_sequence_number(ledger_0 + 20);

    // At ledger_0: supply was 1_000
    assert_eq!(c.gov.get_past_total_supply(&ledger_0), 1_000);
    // At ledger_0 + 5: still 1_000 (before burn)
    assert_eq!(c.gov.get_past_total_supply(&(ledger_0 + 5)), 1_000);
    // At ledger_0 + 10: supply dropped to 600
    assert_eq!(c.gov.get_past_total_supply(&(ledger_0 + 10)), 600);
    // Current: 600
    assert_eq!(c.gov.total_supply(), 600);
}

#[test]
fn test_get_past_total_supply_future_ledger_rejected() {
    let c = Ctx::new();
    let current = c.env.ledger().sequence();
    let result = c.gov.try_get_past_total_supply(&(current + 1));
    assert_eq!(result, Err(Ok(GovTokenError::FutureLedger)));
}

// ── Voting power with transfer across ledgers ──────────────────────────────

#[test]
fn test_transfer_across_ledgers_updates_past_votes() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    let ledger_0 = c.env.ledger().sequence();
    c.mint(&alice, 1_000);

    c.env.ledger().set_sequence_number(ledger_0 + 10);
    c.gov.transfer(&alice, &bob, &300);

    c.env.ledger().set_sequence_number(ledger_0 + 20);

    // At ledger_0: Alice had 1_000 votes, Bob had 0
    assert_eq!(c.gov.get_past_votes(&alice, &ledger_0), 1_000);
    assert_eq!(c.gov.get_past_votes(&bob, &ledger_0), 0);

    // At ledger_0 + 10: after transfer, Alice has 700, Bob has 300
    assert_eq!(c.gov.get_past_votes(&alice, &(ledger_0 + 10)), 700);
    assert_eq!(c.gov.get_past_votes(&bob, &(ledger_0 + 10)), 300);
}

// ── Checkpoint TTL ─────────────────────────────────────────────────────────

#[test]
fn test_checkpoint_ttl_extended() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);

    c.mint(&alice, 1_000);

    let cp_key = GovTokenKey::Checkpoints(alice);
    c.env.as_contract(&c.gov_id, || {
        let ttl = c.env.storage().persistent().get_ttl(&cp_key);
        assert!(ttl >= 518_400, "Checkpoint TTL should be extended on write");
    });
}

#[test]
fn test_delegate_ttl_extended() {
    let c = Ctx::new();
    let alice = Address::generate(&c.env);
    let bob = Address::generate(&c.env);

    c.mint(&alice, 1_000);
    c.gov.delegate(&alice, &bob);

    let delegate_key = GovTokenKey::Delegate(alice);
    c.env.as_contract(&c.gov_id, || {
        let ttl = c.env.storage().persistent().get_ttl(&delegate_key);
        assert!(ttl >= 518_400, "Delegate entry TTL should be extended on write");
    });
}
