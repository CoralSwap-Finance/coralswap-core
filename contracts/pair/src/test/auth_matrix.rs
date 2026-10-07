//! Scoped-authorization matrix for the pair contract (issue #314).
//!
//! `Env::mock_all_auths()` authorizes every address in every invocation tree,
//! so a test written that way cannot tell the difference between "the contract
//! gates this call" and "the contract dropped its guard and the blanket mock
//! quietly covered it". Every test here installs an *exact* authorization tree
//! instead, so each one fails both when the guard is missing and when the
//! contract starts over-authorizing.
//!
//! The matrix is exhaustive over the pair's gated surface:
//!
//! | Entry point            | Authorizer | Who must *not* be able to call it |
//! |------------------------|------------|------------------------------------|
//! | `mint`                 | `to`       | any other address                  |
//! | `mint_with_one_token`  | `to`       | any other address                  |
//! | `burn`                 | `to`       | any other address                  |
//! | `burn_single_side`     | `to`       | any other address                  |
//! | `set_stale_threshold`  | `factory`  | anyone else, incl. `to`           |
//! | `set_lp_token_paused`  | `factory`  | anyone else, incl. `to`           |
//! | `set_frozen`           | `factory`  | anyone else, incl. `to`           |
//!
//! and pins the permissionless surface (`swap`, `sync`, `flash_loan`) with
//! [`auth::assert_unauthorized`] so adding a `require_auth` later becomes a
//! deliberate, reviewable change instead of an accident.
//!
//! # Why the positive tests are not fully scoped
//!
//! `Env::mock_auths` implements each authorizer by registering a mock checker
//! contract *at the author's own address* (`register_at`). Every pair entry
//! point that moves LP has the pair itself as an authorizer, because the pair
//! is the LP token's `admin` and the LP token calls `admin.require_auth()`.
//! Registering the mock checker at the pair's address would overwrite the pair
//! contract, so a contract can never be its own authorizer under scoped auths.
//!
//! The positive tests therefore use `Env::mock_all_auths()` and close the gap
//! the other way round: [`auth::assert_authorized`] inspects the recorded tree
//! afterwards. That still fails if a `require_auth` is deleted (the tree comes
//! back empty) and still fails if a guard is rebound to the wrong address or
//! the wrong arguments. The negative tests need no such caveat, because every
//! pair guard runs before any sub-invocation, so `allow_nothing` reliably
//! reaches the EOA/factory check.

#![cfg(test)]

use coralswap_lp_token::{LpToken, LpTokenClient};
use coralswap_mock_flash_receiver::MockFlashReceiver;
use coralswap_shared::auth_args;
use coralswap_shared::test_support as auth;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Bytes, Env, String as SdkString};

use crate::{Pair, PairClient};

const RESERVE: i128 = 1_000_000;

struct Ctx {
    env: Env,
    pair: PairClient<'static>,
    pair_id: Address,
    token_a_id: Address,
    token_b_id: Address,
    lp_id: Address,
    asset_admin: Address,
    factory: Address,
    user: Address,
    other: Address,
}

impl Ctx {
    fn new() -> Self {
        let env = Env::default();
        let pair_id = env.register(Pair, ());
        let lp_id = env.register(LpToken, ());
        let asset_admin = Address::generate(&env);
        let token_a_id = env.register_stellar_asset_contract_v2(asset_admin.clone()).address();
        let token_b_id = env.register_stellar_asset_contract_v2(asset_admin.clone()).address();
        let factory = Address::generate(&env);
        let user = Address::generate(&env);
        let other = Address::generate(&env);

        LpTokenClient::new(&env, &lp_id).initialize(
            &pair_id,
            &7u32,
            &SdkString::from_str(&env, "Coral LP"),
            &SdkString::from_str(&env, "CORAL-LP"),
        );

        let pair = PairClient::new(&env, &pair_id);
        pair.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

        let ctx = Self {
            env,
            pair,
            pair_id,
            token_a_id,
            token_b_id,
            lp_id,
            asset_admin,
            factory,
            user,
            other,
        };
        ctx.seed_pool();
        ctx
    }

    fn token(&self, id: &Address) -> TokenClient<'_> {
        TokenClient::new(&self.env, id)
    }

    /// `mint` is an admin-only entry point, so it needs the asset client
    /// rather than the holder `TokenClient`.
    fn token_admin(&self, id: &Address) -> StellarAssetClient<'_> {
        StellarAssetClient::new(&self.env, id)
    }

    fn lp(&self) -> LpTokenClient<'_> {
        LpTokenClient::new(&self.env, &self.lp_id)
    }

    /// Mints test tokens, scoping the authorization to exactly the asset
    /// admin's `mint` call. Even fixture setup is scoped rather than blanket
    /// mocked, so a test cannot accidentally rely on a global bypass.
    fn mint(&self, token_id: &Address, to: &Address, amount: i128) {
        auth::allow(
            &self.env,
            &self.asset_admin,
            token_id,
            "mint",
            auth_args!(&self.env, to.clone(), amount),
        );
        self.token_admin(token_id).mint(to, &amount);
    }

    /// Mints to `user` and moves the deposit into the pair, scoping both
    /// authorizations. The user's `transfer` is a genuine off-chain
    /// authorization, so it is always scoped rather than blanket-mocked.
    fn deposit(&self, token_id: &Address, amount: i128) {
        self.mint(token_id, &self.user, amount);
        auth::allow(
            &self.env,
            &self.user,
            token_id,
            "transfer",
            auth_args!(&self.env, self.user.clone(), self.pair_id.clone(), amount),
        );
        self.token(token_id).transfer(&self.user, &self.pair_id, &amount);
    }

    /// Moves LP from `user` to the pair, which is how `Pair::burn` redeems.
    fn deposit_lp(&self, amount: i128) {
        auth::allow(
            &self.env,
            &self.user,
            &self.lp_id,
            "transfer",
            auth_args!(&self.env, self.user.clone(), self.pair_id.clone(), amount),
        );
        self.lp().transfer(&self.user, &self.pair_id, &amount);
    }

    /// Gives the pair a live position. The pair's own LP mint needs no external
    /// signature because the pair is already inside the invocation tree.
    fn seed_pool(&self) {
        for token_id in [self.token_a_id.clone(), self.token_b_id.clone()] {
            self.deposit(&token_id, RESERVE);
        }
        // Fixture seeding uses a blanket mock on purpose — see the module
        // docs for why a pair entry point cannot be fully scoped.
        self.env.mock_all_auths();
        self.pair.mint(&self.user);
    }
}

// ─────────────────────────────────────────────
// Positive: each gated call authorizes exactly one actor
// ─────────────────────────────────────────────

#[test]
fn mint_is_authorized_by_the_recipient_and_nobody_else() {
    let c = Ctx::new();
    c.deposit(&c.token_a_id, 10_000i128);
    c.deposit(&c.token_b_id, 10_000i128);

    // A blanket mock is unavoidable here (see module docs), so the guard is
    // proven by inspecting the recorded tree instead. If `to.require_auth()`
    // were deleted from `Pair::mint`, `env.auths()` would come back empty and
    // this assertion would fail.
    c.env.mock_all_auths();
    c.pair.mint(&c.user);

    auth::assert_authorized(
        &c.env,
        &c.user,
        &c.pair_id,
        "mint",
        auth_args!(&c.env, c.user.clone()),
    );
}

#[test]
fn burn_is_authorized_by_the_recipient_and_nobody_else() {
    let c = Ctx::new();
    let held = c.lp().balance(&c.user);
    c.deposit_lp(held);

    c.env.mock_all_auths();
    c.pair.burn(&c.user);

    auth::assert_authorized(
        &c.env,
        &c.user,
        &c.pair_id,
        "burn",
        auth_args!(&c.env, c.user.clone()),
    );
}

#[test]
fn mint_with_one_token_authorizes_the_token_pull_as_a_sub_invocation() {
    let c = Ctx::new();
    let amount = 50_000i128;
    c.mint(&c.token_a_id, &c.user, amount);

    // The pair pulls the deposit with a token `transfer`, so the payer's tree
    // carries a genuine nested authorization. This is the realistic reentrancy
    // shape a blanket mock used to hide.
    c.env.mock_all_auths();
    c.pair.mint_with_one_token(&c.user, &c.token_a_id, &amount, &1);

    let auths = c.env.auths();
    assert_eq!(auths.len(), 1, "expected exactly one authorized actor");
    assert_eq!(auths[0].0, c.user);
    assert_eq!(
        auths[0].1.sub_invocations.len(),
        1,
        "the token pull must be recorded as an authorized sub-invocation"
    );
}

#[test]
fn burn_single_side_authorizes_the_lp_burn_as_a_sub_invocation() {
    let c = Ctx::new();
    let amount = 1_000i128;
    c.deposit_lp(amount);

    // The pair burns LP the holder left with it, so the root call is authorized
    // by the holder and the LP burn sits beneath it.
    c.env.mock_all_auths();
    c.pair.burn_single_side(&c.user, &amount, &c.token_b_id, &1);

    let auths = c.env.auths();
    assert_eq!(auths.len(), 1, "expected exactly one authorized actor");
    assert_eq!(auths[0].0, c.user);
    assert_eq!(auths[0].1.sub_invocations.len(), 1, "the LP burn must be authorized");
}

#[test]
fn factory_only_may_retune_the_oracle() {
    let c = Ctx::new();

    auth::allow(&c.env, &c.factory, &c.pair_id, "set_stale_threshold", auth_args!(&c.env, 77u32));
    c.pair.set_stale_threshold(&77);

    auth::assert_authorized(
        &c.env,
        &c.factory,
        &c.pair_id,
        "set_stale_threshold",
        auth_args!(&c.env, 77u32),
    );
}

#[test]
fn factory_only_may_pause_the_lp_token() {
    let c = Ctx::new();

    auth::allow(&c.env, &c.factory, &c.pair_id, "set_lp_token_paused", auth_args!(&c.env, true));
    c.pair.set_lp_token_paused(&true);

    auth::assert_authorized(
        &c.env,
        &c.factory,
        &c.pair_id,
        "set_lp_token_paused",
        auth_args!(&c.env, true),
    );
    assert!(c.pair.is_lp_token_paused());
}

#[test]
fn factory_only_may_freeze_the_pair() {
    let c = Ctx::new();

    auth::allow(&c.env, &c.factory, &c.pair_id, "set_frozen", auth_args!(&c.env, true));
    c.pair.set_frozen(&true);

    auth::assert_authorized(&c.env, &c.factory, &c.pair_id, "set_frozen", auth_args!(&c.env, true));
    assert!(c.pair.is_frozen());
}

// ─────────────────────────────────────────────
// Negative: the matrix. Wrong authorizer ⇒ auth error
// ─────────────────────────────────────────────

#[test]
fn mint_rejects_a_signer_that_is_not_the_recipient() {
    let c = Ctx::new();
    // `other` signs, but the pair demands `user`.
    auth::allow(&c.env, &c.other, &c.pair_id, "mint", auth_args!(&c.env, c.user.clone()));

    assert!(
        c.pair.try_mint(&c.user).is_err(),
        "mint must not succeed when only an unrelated address authorized it"
    );
}

#[test]
fn burn_rejects_a_signer_that_is_not_the_recipient() {
    let c = Ctx::new();
    auth::allow(&c.env, &c.other, &c.pair_id, "burn", auth_args!(&c.env, c.user.clone()));

    assert!(c.pair.try_burn(&c.user).is_err(), "burn must not succeed for a non-recipient signer");
}

#[test]
fn mint_with_one_token_rejects_a_signer_that_is_not_the_payer() {
    let c = Ctx::new();
    let amount = 50_000i128;
    auth::allow(
        &c.env,
        &c.other,
        &c.pair_id,
        "mint_with_one_token",
        auth_args!(&c.env, c.user.clone(), c.token_a_id.clone(), amount, 1i128),
    );

    assert!(
        c.pair.try_mint_with_one_token(&c.user, &c.token_a_id, &amount, &1).is_err(),
        "single-sided mint must not succeed for a non-payer signer"
    );
}

#[test]
fn burn_single_side_rejects_a_signer_that_is_not_the_holder() {
    let c = Ctx::new();
    let amount = 1_000i128;
    auth::allow(
        &c.env,
        &c.other,
        &c.pair_id,
        "burn_single_side",
        auth_args!(&c.env, c.user.clone(), amount, c.token_b_id.clone(), 1i128),
    );

    assert!(
        c.pair.try_burn_single_side(&c.user, &amount, &c.token_b_id, &1).is_err(),
        "single-sided burn must not succeed for a non-holder signer"
    );
}

#[test]
fn set_stale_threshold_rejects_a_signer_that_is_not_the_factory() {
    let c = Ctx::new();
    auth::allow(&c.env, &c.user, &c.pair_id, "set_stale_threshold", auth_args!(&c.env, 77u32));

    assert!(
        c.pair.try_set_stale_threshold(&77).is_err(),
        "an LP holder must not retune the oracle"
    );
}

#[test]
fn set_lp_token_paused_rejects_a_signer_that_is_not_the_factory() {
    let c = Ctx::new();
    auth::allow(&c.env, &c.user, &c.pair_id, "set_lp_token_paused", auth_args!(&c.env, true));

    assert!(
        c.pair.try_set_lp_token_paused(&true).is_err(),
        "an LP holder must not be able to pause the LP token"
    );
    // And the token is demonstrably still running.
    assert!(!c.pair.is_lp_token_paused());
}

#[test]
fn set_frozen_rejects_a_signer_that_is_not_the_factory() {
    let c = Ctx::new();
    auth::allow(&c.env, &c.user, &c.pair_id, "set_frozen", auth_args!(&c.env, true));

    assert!(
        c.pair.try_set_frozen(&true).is_err(),
        "an LP holder must not be able to freeze the pool"
    );
    assert!(!c.pair.is_frozen(), "the pool must remain tradable");
}

#[test]
fn a_correct_but_unbound_signature_is_still_rejected() {
    let c = Ctx::new();
    // Right function, right argument — but the authorization is bound to the
    // wrong address. This is what proves the guard names the factory rather
    // than merely checking that *some* signature exists.
    auth::allow(&c.env, &c.other, &c.pair_id, "set_lp_token_paused", auth_args!(&c.env, true));

    assert!(c.pair.try_set_lp_token_paused(&true).is_err());
    assert!(!c.pair.is_lp_token_paused());
}

#[test]
fn every_gated_entry_point_fails_with_no_authorization_at_all() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    assert!(c.pair.try_mint(&c.user).is_err());
    assert!(c.pair.try_mint_with_one_token(&c.user, &c.token_a_id, &1_000i128, &1i128).is_err());
    assert!(c.pair.try_burn(&c.user).is_err());
    assert!(c.pair.try_burn_single_side(&c.user, &1_000i128, &c.token_b_id, &1i128).is_err());
    assert!(c.pair.try_set_stale_threshold(&77).is_err());
    assert!(c.pair.try_set_lp_token_paused(&true).is_err());
    assert!(c.pair.try_set_frozen(&true).is_err());
}

// ─────────────────────────────────────────────
// The permissionless surface stays permissionless
// ─────────────────────────────────────────────

#[test]
fn swap_and_sync_require_no_authorization() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    let before = c.token(&c.token_b_id).balance(&c.user);

    c.pair.sync();
    auth::assert_unauthorized(&c.env);

    // Deposit after the sync so the swap sees the balance delta as its input.
    c.deposit(&c.token_a_id, 1_000i128);
    c.pair.swap(&0i128, &100i128, &c.user);
    auth::assert_unauthorized(&c.env);
    assert!(c.token(&c.token_b_id).balance(&c.user) > before, "swap must still settle");
}

#[test]
fn flash_loan_repaid_by_a_contract_receiver_requires_no_external_signature() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    // The receiver repays from inside the callback. Its own transfer needs no
    // external signature because it is already inside the invocation tree.
    let receiver = c.env.register(MockFlashReceiver, ());
    let fee = crate::flash_loan::compute_flash_fee(1_000, 30).unwrap();
    let total = 1_000 + fee;
    c.mint(&c.token_a_id, &c.user, total);
    auth::allow(
        &c.env,
        &c.user,
        &c.token_a_id,
        "transfer",
        auth_args!(&c.env, c.user.clone(), receiver.clone(), total),
    );
    c.token(&c.token_a_id).transfer(&c.user, &receiver, &total);

    let action = Bytes::from_slice(&c.env, b"repay");
    c.pair.flash_loan(&receiver, &1_000i128, &0i128, &action);

    auth::assert_unauthorized(&c.env);
}

#[test]
fn a_view_call_needs_no_authorization() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    let (_a, _b, _t) = c.pair.get_reserves();
    auth::assert_unauthorized(&c.env);
    assert!(!c.pair.is_lp_token_paused());
    auth::assert_unauthorized(&c.env);
    assert!(!c.pair.is_frozen());
    auth::assert_unauthorized(&c.env);
    let _fee = c.pair.get_current_fee_bps();
    auth::assert_unauthorized(&c.env);
}
