//! Scoped-authorization matrix for the LP token contract (issue #314).
//!
//! The LP token calls no other contract, so every one of its entry points is
//! authorized purely by externally-signed addresses. That makes this the
//! cleanest place to prove the scoped-auth approach works: each test installs
//! an exact authorization tree, so it fails if a `require_auth` is deleted, if
//! it is rebound to the wrong address, or if the token starts over-authorizing.
//!
//! | Entry point        | Signature                                | Authorizer       |
//! |--------------------|------------------------------------------|------------------|
//! | `pause`            | `()`                                     | `admin`          |
//! | `unpause`          | `()`                                     | `admin`          |
//! | `admin_transfer`   | `(new_admin)`                            | current `admin`  |
//! | `mint`             | `(to, amount)`                           | `admin`          |
//! | `burn`             | `(from, amount)`                         | `from`           |
//! | `approve`          | `(from, spender, amount, expiration)`    | `from`           |
//! | `transfer`         | `(from, to, amount)`                     | `from`           |
//! | `transfer_from`    | `(spender, from, to, amount)`            | `spender`        |
//! | `burn_from`        | `(spender, from, amount)`                | `spender`        |
//!
//! Note the deliberate asymmetry between `transfer`/`burn` (authorized by the
//! token holder) and `transfer_from`/`burn_from` (authorized by the *spender*,
//! with the allowance check acting on the holder's balance). Getting that
//! backwards is exactly the kind of bug a blanket mock hides, so both
//! directions are asserted here.

#![cfg(test)]

use coralswap_shared::auth_args;
use coralswap_shared::test_support as auth;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

use crate::{LpToken, LpTokenClient};

struct Ctx {
    env: Env,
    lp: LpTokenClient<'static>,
    lp_id: Address,
    admin: Address,
    holder: Address,
    spender: Address,
    recipient: Address,
    stranger: Address,
}

impl Ctx {
    fn new() -> Self {
        let env = Env::default();
        let lp_id = env.register(LpToken, ());
        let admin = Address::generate(&env);
        let holder = Address::generate(&env);

        let spender = Address::generate(&env);
        let recipient = Address::generate(&env);
        let stranger = Address::generate(&env);

        let lp = LpTokenClient::new(&env, &lp_id);
        lp.initialize(
            &admin,
            &7u32,
            &String::from_str(&env, "Coral LP"),
            &String::from_str(&env, "CORAL-LP"),
        );

        // Fixture minting is scoped too, so no test starts from a blanket mock.
        let c = Self { env, lp, lp_id, admin, holder, spender, recipient, stranger };
        c.mint(&c.holder.clone(), 1_000i128);
        c
    }

    /// Admin mints to `to`, scoping the authorization.
    fn mint(&self, to: &Address, amount: i128) {
        auth::allow(
            &self.env,
            &self.admin,
            &self.lp_id,
            "mint",
            auth_args!(&self.env, to.clone(), amount),
        );
        self.lp.mint(to, &amount);
    }

    fn future_ledger(&self) -> u32 {
        self.env.ledger().sequence() + 1_000
    }
}

// ─────────────────────────────────────────────
// Positive: the right authorizer is accepted
// ─────────────────────────────────────────────

#[test]
fn pause_and_unpause_are_authorized_by_the_admin() {
    let c = Ctx::new();

    auth::allow(&c.env, &c.admin, &c.lp_id, "pause", auth::no_args(&c.env));
    c.lp.pause();
    auth::assert_authorized(&c.env, &c.admin, &c.lp_id, "pause", auth::no_args(&c.env));
    assert!(c.lp.is_paused());

    auth::allow(&c.env, &c.admin, &c.lp_id, "unpause", auth::no_args(&c.env));
    c.lp.unpause();
    auth::assert_authorized(&c.env, &c.admin, &c.lp_id, "unpause", auth::no_args(&c.env));
    assert!(!c.lp.is_paused());
}

#[test]
fn admin_transfer_is_authorized_by_the_current_admin() {
    let c = Ctx::new();
    let next = Address::generate(&c.env);

    auth::allow(&c.env, &c.admin, &c.lp_id, "admin_transfer", auth_args!(&c.env, next.clone()));
    c.lp.admin_transfer(&next);

    auth::assert_authorized(
        &c.env,
        &c.admin,
        &c.lp_id,
        "admin_transfer",
        auth_args!(&c.env, next.clone()),
    );
    assert_eq!(c.lp.admin(), next);
}

#[test]
fn mint_is_authorized_by_the_admin() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.admin,
        &c.lp_id,
        "mint",
        auth_args!(&c.env, c.recipient.clone(), 500i128),
    );
    c.lp.mint(&c.recipient, &500);

    auth::assert_authorized(
        &c.env,
        &c.admin,
        &c.lp_id,
        "mint",
        auth_args!(&c.env, c.recipient.clone(), 500i128),
    );
    assert_eq!(c.lp.balance(&c.recipient), 500i128);
}

#[test]
fn burn_is_authorized_by_the_holder() {
    let c = Ctx::new();

    auth::allow(&c.env, &c.holder, &c.lp_id, "burn", auth_args!(&c.env, c.holder.clone(), 400i128));
    c.lp.burn(&c.holder, &400);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.lp_id,
        "burn",
        auth_args!(&c.env, c.holder.clone(), 400i128),
    );
    assert_eq!(c.lp.balance(&c.holder), 600i128);
}

#[test]
fn transfer_is_authorized_by_the_sender() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.recipient.clone(), 250i128),
    );
    c.lp.transfer(&c.holder, &c.recipient, &250);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.lp_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.recipient.clone(), 250i128),
    );
    assert_eq!(c.lp.balance(&c.recipient), 250i128);
    assert_eq!(c.lp.balance(&c.holder), 750i128);
}

#[test]
fn approve_is_authorized_by_the_owner() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    c.lp.approve(&c.holder, &c.spender, &300, &expiry);

    auth::assert_authorized(
        &c.env,
        &c.holder,
        &c.lp_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    assert_eq!(c.lp.allowance(&c.holder, &c.spender), 300i128);
}

/// `transfer_from` is authorized by the *spender*, not the holder. The
/// holder's signature would be the wrong tree.
#[test]
fn transfer_from_is_authorized_by_the_spender() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    c.lp.approve(&c.holder, &c.spender, &300, &expiry);

    auth::allow(
        &c.env,
        &c.spender,
        &c.lp_id,
        "transfer_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), c.recipient.clone(), 300i128),
    );
    c.lp.transfer_from(&c.spender, &c.holder, &c.recipient, &300);

    auth::assert_authorized(
        &c.env,
        &c.spender,
        &c.lp_id,
        "transfer_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), c.recipient.clone(), 300i128),
    );
    assert_eq!(c.lp.balance(&c.recipient), 300i128);
}

#[test]
fn burn_from_is_authorized_by_the_spender() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    c.lp.approve(&c.holder, &c.spender, &300, &expiry);

    auth::allow(
        &c.env,
        &c.spender,
        &c.lp_id,
        "burn_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), 300i128),
    );
    c.lp.burn_from(&c.spender, &c.holder, &300);

    auth::assert_authorized(
        &c.env,
        &c.spender,
        &c.lp_id,
        "burn_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), 300i128),
    );
    assert_eq!(c.lp.balance(&c.holder), 700i128);
}

// ─────────────────────────────────────────────
// Negative: the matrix
// ─────────────────────────────────────────────

#[test]
fn pause_and_unpause_reject_a_stranger() {
    let c = Ctx::new();

    auth::allow(&c.env, &c.stranger, &c.lp_id, "pause", auth::no_args(&c.env));
    assert!(c.lp.try_pause().is_err(), "a stranger must not be able to pause the LP token");
    assert!(!c.lp.is_paused(), "pause state must be unchanged");
}

#[test]
fn admin_transfer_rejects_a_stranger() {
    let c = Ctx::new();
    let next = Address::generate(&c.env);

    auth::allow(&c.env, &c.stranger, &c.lp_id, "admin_transfer", auth_args!(&c.env, next.clone()));
    assert!(c.lp.try_admin_transfer(&next).is_err(), "a stranger must not reassign the admin");
    assert_eq!(c.lp.admin(), c.admin);
}

#[test]
fn mint_rejects_a_stranger() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.lp_id,
        "mint",
        auth_args!(&c.env, c.stranger.clone(), 1i128),
    );
    assert!(c.lp.try_mint(&c.stranger, &1).is_err(), "a stranger must not mint LP");
    assert_eq!(c.lp.total_supply(), 1_000i128, "supply must be unchanged");
}

#[test]
fn burn_rejects_a_stranger_burning_another_accounts_tokens() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.lp_id,
        "burn",
        auth_args!(&c.env, c.holder.clone(), 1_000i128),
    );
    assert!(
        c.lp.try_burn(&c.holder, &1_000).is_err(),
        "burning another account's balance must require that account's authorization"
    );
    assert_eq!(c.lp.balance(&c.holder), 1_000i128, "balance must be unchanged");
}

#[test]
fn transfer_rejects_a_stranger_moving_another_accounts_tokens() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.lp_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.stranger.clone(), 1_000i128),
    );
    assert!(
        c.lp.try_transfer(&c.holder, &c.stranger, &1_000).is_err(),
        "draining another account's balance must require that account's authorization"
    );
    assert_eq!(c.lp.balance(&c.holder), 1_000i128);
    assert_eq!(c.lp.balance(&c.stranger), 0i128);
}

#[test]
fn approve_rejects_a_stranger_approving_on_anothers_behalf() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.lp_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.stranger.clone(), 1_000i128, expiry),
    );
    assert!(
        c.lp.try_approve(&c.holder, &c.stranger, &1_000, &expiry).is_err(),
        "a stranger must not open an allowance on someone else's balance"
    );
    assert_eq!(c.lp.allowance(&c.holder, &c.stranger), 0i128);
}

/// The inverse asymmetry: a holder's signature must not authorize a
/// `transfer_from`, because the spender is the party acting.
#[test]
fn transfer_from_rejects_the_holders_authorization_instead_of_the_spenders() {
    let c = Ctx::new();
    let expiry = c.future_ledger();

    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "approve",
        auth_args!(&c.env, c.holder.clone(), c.spender.clone(), 300i128, expiry),
    );
    c.lp.approve(&c.holder, &c.spender, &300, &expiry);

    // The holder signs, but the spender is the actor.
    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "transfer_from",
        auth_args!(&c.env, c.spender.clone(), c.holder.clone(), c.recipient.clone(), 300i128),
    );
    assert!(
        c.lp.try_transfer_from(&c.spender, &c.holder, &c.recipient, &300).is_err(),
        "transfer_from must require the spender's authorization, not the holder's"
    );
    assert_eq!(c.lp.balance(&c.recipient), 0i128);
}

#[test]
fn every_gated_entry_point_fails_with_no_authorization_at_all() {
    let c = Ctx::new();
    let expiry = c.future_ledger();
    auth::allow_nothing(&c.env);

    assert!(c.lp.try_pause().is_err());
    assert!(c.lp.try_unpause().is_err());
    assert!(c.lp.try_admin_transfer(&c.stranger).is_err());
    assert!(c.lp.try_mint(&c.stranger, &1).is_err());
    assert!(c.lp.try_burn(&c.holder, &1).is_err());
    assert!(c.lp.try_burn_from(&c.spender, &c.holder, &1).is_err());
    assert!(c.lp.try_approve(&c.holder, &c.spender, &1, &expiry).is_err());
    assert!(c.lp.try_transfer(&c.holder, &c.recipient, &1).is_err());
    assert!(c.lp.try_transfer_from(&c.spender, &c.holder, &c.recipient, &1).is_err());

    assert!(!c.lp.is_paused());
    assert_eq!(c.lp.admin(), c.admin);
    assert_eq!(c.lp.total_supply(), 1_000i128);
    assert_eq!(c.lp.balance(&c.holder), 1_000i128);
    assert_eq!(c.lp.balance(&c.recipient), 0i128);
}

/// An authorization bound to the wrong *arguments* must not satisfy the call.
/// This is the case a blanket mock cannot catch: the address and the function
/// are both right, and only the amount differs.
///
/// `require_auth()` infers the current invocation's arguments, so it is
/// argument-bound even though it is not written as `require_auth_for_args(..)`.
#[test]
fn an_authorization_for_a_different_amount_is_rejected() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.holder,
        &c.lp_id,
        "transfer",
        auth_args!(&c.env, c.holder.clone(), c.recipient.clone(), 1_000i128),
    );
    assert!(
        c.lp.try_transfer(&c.holder, &c.recipient, &1).is_err(),
        "an authorization for 1000 must not authorize a 1-token transfer"
    );
    assert_eq!(c.lp.balance(&c.recipient), 0i128);
    assert_eq!(c.lp.balance(&c.holder), 1_000i128);

    // The matching amount does go through.
    c.lp.transfer(&c.holder, &c.recipient, &1_000);
    assert_eq!(c.lp.balance(&c.recipient), 1_000i128);
}

// ─────────────────────────────────────────────
// The permissionless surface stays permissionless
// ─────────────────────────────────────────────

#[test]
fn a_view_call_needs_no_authorization() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    let _ = c.lp.balance(&c.holder);
    let _ = c.lp.total_supply();
    let _ = c.lp.allowance(&c.holder, &c.spender);
    let _ = c.lp.is_paused();
    let _ = c.lp.admin();

    auth::assert_unauthorized(&c.env);
}
