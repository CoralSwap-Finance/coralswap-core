//! Scoped-authorization matrix for the factory contract (issue #314).
//!
//! Every governance entry point here is authorized by an externally-signed
//! address — a `fee_to` setter, or the calling pair — so unlike the pair these
//! tests can use fully scoped authorization trees. A test written this way
//! fails if a `require_auth` is deleted, if it is rebound to the wrong address,
//! or if the factory starts demanding authorizations it does not need.
//!
//! | Entry point                | Signature                                                     | Authorizer          |
//! |----------------------------|---------------------------------------------------------------|---------------------|
//! | `set_fee_to`               | `(setter, fee_to, fee_bps)`                                   | the `fee_to_setter` |
//! | `set_fee_to_setter`        | `(setter, new_setter)`                                        | the `fee_to_setter` |
//! | `set_pair_fee`             | `(setter, pair, fee_bps)`                                     | the `fee_to_setter` |
//! | `freeze_pair` / `unfreeze_pair` | `(pair)`                                                 | the `fee_to_setter` |
//! | `deposit_protocol_fee`     | `(pair, token, amount)`                                       | the `pair`          |
//!
//! Each setter entry point takes the claimed setter as an explicit argument and
//! then checks it against storage, so the authorization is bound to a specific
//! address *and* a specific argument list. `freeze_pair` / `unfreeze_pair`
//! are the exception: they take no setter argument and read it from storage,
//! so only the rejection half belongs here — the positive half needs a real
//! deployed pair (the relay invokes `Pair::set_frozen`) and lives in
//! `test/mod.rs`. `deposit_protocol_fee` is covered
//! negatively only: its authorizer is a deployed contract, and
//! `Env::mock_auths` would overwrite that contract with its mock checker (see
//! the pair matrix for the full explanation).

#![cfg(test)]

use coralswap_shared::auth_args;
use coralswap_shared::test_support as auth;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, Vec};

use crate::{Factory, FactoryClient};

struct Ctx {
    env: Env,
    factory: FactoryClient<'static>,
    factory_id: Address,
    setter: Address,
    stranger: Address,
    /// A stand-in for a deployed pair. `set_pair_fee` and
    /// `deposit_protocol_fee` only compare addresses, so the authorization
    /// tests do not need a real pair contract.
    pair: Address,
    token: Address,
    recipient: Address,
}

impl Ctx {
    fn new() -> Self {
        let env = Env::default();
        // Initialization is fixture work, not the thing under test.
        env.mock_all_auths();

        let factory_id = env.register(Factory, ());
        let setter = Address::generate(&env);
        let signers = Vec::from_array(&env, [Address::generate(&env)]);
        let empty_wasm = BytesN::from_array(&env, &[0u8; 32]);
        let stranger = Address::generate(&env);
        let pair = Address::generate(&env);
        let token = Address::generate(&env);
        let recipient = Address::generate(&env);

        let factory = FactoryClient::new(&env, &factory_id);
        factory.initialize(&signers, &empty_wasm, &empty_wasm, &empty_wasm, &setter);

        Self { env, factory, factory_id, setter, stranger, pair, token, recipient }
    }

    /// Sets `fee_to` through a scoped authorization, so fee-dependent entry
    /// points can be exercised.
    fn set_fee_to(&self, fee_to: &Address, fee_bps: u32) {
        auth::allow(
            &self.env,
            &self.setter,
            &self.factory_id,
            "set_fee_to",
            auth_args!(&self.env, self.setter.clone(), fee_to.clone(), fee_bps),
        );
        self.factory.set_fee_to(&self.setter, &Some(fee_to.clone()), &fee_bps);
    }
}

// ─────────────────────────────────────────────
// Positive: the right authorizer is accepted
// ─────────────────────────────────────────────

#[test]
fn set_fee_to_is_authorized_by_the_setter() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_fee_to",
        auth_args!(&c.env, c.setter.clone(), c.recipient.clone(), 30u32),
    );
    c.factory.set_fee_to(&c.setter, &Some(c.recipient.clone()), &30);

    auth::assert_authorized(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_fee_to",
        auth_args!(&c.env, c.setter.clone(), c.recipient.clone(), 30u32),
    );
    assert_eq!(c.factory.fee_to(), Some(c.recipient.clone()));
    assert_eq!(c.factory.fee_bps(), 30u32);
}

#[test]
fn set_fee_config_is_atomic_and_rejects_invalid_combinations() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_fee_config",
        auth_args!(&c.env, c.setter.clone(), c.recipient.clone(), 20u32),
    );
    c.factory.set_fee_config(&c.setter, &Some(c.recipient.clone()), &20);
    assert_eq!(c.factory.fee_to(), Some(c.recipient.clone()));
    assert_eq!(c.factory.fee_bps(), 20);

    assert!(c.factory.try_set_fee_config(&c.setter, &Some(c.recipient.clone()), &0).is_err());
    assert_eq!(c.factory.fee_bps(), 20);
}

#[test]
fn set_pair_fee_is_authorized_by_the_setter() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_pair_fee",
        auth_args!(&c.env, c.setter.clone(), c.pair.clone(), 30u32),
    );
    c.factory.set_pair_fee(&c.setter, &c.pair, &30);

    auth::assert_authorized(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_pair_fee",
        auth_args!(&c.env, c.setter.clone(), c.pair.clone(), 30u32),
    );
    assert_eq!(c.factory.get_pair_fee_override(&c.pair), Some(30u32));
}

#[test]
fn set_fee_to_setter_is_authorized_by_the_current_setter() {
    let c = Ctx::new();
    let next = Address::generate(&c.env);

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_fee_to_setter",
        auth_args!(&c.env, c.setter.clone(), next.clone()),
    );
    c.factory.set_fee_to_setter(&c.setter, &next);

    auth::assert_authorized(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_fee_to_setter",
        auth_args!(&c.env, c.setter.clone(), next.clone()),
    );
    assert_eq!(c.factory.fee_to_setter(), Some(next));
}

// ─────────────────────────────────────────────
// Negative: the matrix
// ─────────────────────────────────────────────

#[test]
fn set_fee_to_rejects_a_stranger() {
    let c = Ctx::new();

    // A stranger signs, and also passes its own address as the `setter`
    // argument. The storage check must still refuse it.
    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "set_fee_to",
        auth_args!(&c.env, c.stranger.clone(), c.recipient.clone(), 30u32),
    );
    assert!(
        c.factory.try_set_fee_to(&c.stranger, &Some(c.recipient.clone()), &30).is_err(),
        "a stranger must not set the protocol fee recipient"
    );
    assert_eq!(c.factory.fee_to(), None, "state must be unchanged");
}

#[test]
fn set_pair_fee_rejects_a_stranger() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "set_pair_fee",
        auth_args!(&c.env, c.stranger.clone(), c.pair.clone(), 30u32),
    );
    assert!(
        c.factory.try_set_pair_fee(&c.stranger, &c.pair, &30).is_err(),
        "a stranger must not set a pair fee override"
    );
    assert_eq!(c.factory.get_pair_fee_override(&c.pair), None, "state must be unchanged");
}

#[test]
fn set_fee_to_setter_rejects_a_stranger() {
    let c = Ctx::new();
    let next = Address::generate(&c.env);

    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "set_fee_to_setter",
        auth_args!(&c.env, c.stranger.clone(), next.clone()),
    );
    assert!(
        c.factory.try_set_fee_to_setter(&c.stranger, &next).is_err(),
        "a stranger must not reassign the setter role"
    );
    assert_eq!(c.factory.fee_to_setter(), Some(c.setter));
}

/// `freeze_pair` reads its admin from storage rather than taking it as an
/// argument, so the only way for a stranger's signature to be refused is the
/// `fee_to_setter.require_auth()` guard itself.
#[test]
fn freeze_pair_rejects_a_signer_that_is_not_the_setter() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "freeze_pair",
        auth_args!(&c.env, c.pair.clone()),
    );
    assert!(c.factory.try_freeze_pair(&c.pair).is_err(), "a stranger must not freeze a pair");
    assert!(!c.factory.is_pair_frozen(&c.pair), "state must be unchanged");
}

#[test]
fn unfreeze_pair_rejects_a_signer_that_is_not_the_setter() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "unfreeze_pair",
        auth_args!(&c.env, c.pair.clone()),
    );
    assert!(c.factory.try_unfreeze_pair(&c.pair).is_err(), "a stranger must not unfreeze a pair");
}

/// A correct authorizer bound to the wrong *function* must not satisfy the
/// call: authorizations are bound to a specific invocation, not to an address
/// alone.
#[test]
fn set_pair_fee_rejects_an_authorization_for_different_arguments() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_pair_fee",
        auth_args!(&c.env, c.setter.clone(), c.pair.clone(), 99u32),
    );
    assert!(
        c.factory.try_set_pair_fee(&c.setter, &c.pair, &30).is_err(),
        "an authorization naming fee_bps=99 must not authorize a fee_bps=30 call"
    );
    assert_eq!(c.factory.get_pair_fee_override(&c.pair), None);
}

/// A correct authorizer bound to the wrong *function* must not satisfy the call.
#[test]
fn set_fee_to_rejects_an_authorization_for_a_different_function() {
    let c = Ctx::new();

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "set_pair_fee",
        auth_args!(&c.env, c.setter.clone(), c.pair.clone(), 30u32),
    );
    assert!(
        c.factory.try_set_fee_to(&c.setter, &Some(c.recipient.clone()), &30).is_err(),
        "an authorization for set_pair_fee must not authorize set_fee_to"
    );
    assert_eq!(c.factory.fee_to(), None);
}

#[test]
fn every_gated_entry_point_fails_with_no_authorization_at_all() {
    let c = Ctx::new();
    c.set_fee_to(&c.recipient, 30);
    auth::allow_nothing(&c.env);

    assert!(c.factory.try_set_fee_to(&c.setter, &Some(c.stranger.clone()), &30).is_err());
    assert!(c.factory.try_set_pair_fee(&c.setter, &c.pair, &30).is_err());
    assert!(c.factory.try_set_pair_lp_token_paused(&c.setter, &c.pair, &true).is_err());
    assert!(c.factory.try_set_pair_stale_threshold(&c.setter, &c.pair, &500u32).is_err());
    assert!(c.factory.try_set_fee_to_setter(&c.setter, &c.stranger).is_err());
    assert!(c.factory.try_deposit_protocol_fee(&c.pair, &c.token, &1_000i128).is_err());
    assert!(c.factory.try_freeze_pair(&c.pair).is_err());
    assert!(c.factory.try_unfreeze_pair(&c.pair).is_err());

    assert_eq!(c.factory.fee_to(), Some(c.recipient));
    assert_eq!(c.factory.get_pair_fee_override(&c.pair), None);
    assert_eq!(c.factory.fee_to_setter(), Some(c.setter));
    assert!(!c.factory.is_pair_frozen(&c.pair), "no authorization must mean no freeze");
}

#[test]
fn deposit_protocol_fee_rejects_a_stranger_acting_as_the_pair() {
    let c = Ctx::new();
    c.set_fee_to(&c.recipient, 30);

    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "deposit_protocol_fee",
        auth_args!(&c.env, c.pair.clone(), c.token.clone(), 1_000i128),
    );
    assert!(
        c.factory.try_deposit_protocol_fee(&c.pair, &c.token, &1_000i128).is_err(),
        "only the pair itself may deposit protocol fees"
    );
}

// ─────────────────────────────────────────────
// The permissionless surface stays permissionless
// ─────────────────────────────────────────────

#[test]
fn a_view_call_needs_no_authorization() {
    let c = Ctx::new();
    auth::allow_nothing(&c.env);

    let _ = c.factory.fee_to();
    let _ = c.factory.fee_bps();
    let _ = c.factory.fee_to_setter();
    let _ = c.factory.get_pair_fee_override(&c.pair);
    let _ = c.factory.get_all_pairs(&0, &10);
    let _ = c.factory.get_pair_count();
    let _ = c.factory.is_paused();

    auth::assert_unauthorized(&c.env);
}

#[test]
fn a_governance_call_needs_a_real_multisig_authorization() {
    let c = Ctx::new();
    let signers = Vec::from_array(&c.env, [c.stranger.clone()]);
    auth::allow_nothing(&c.env);

    // `pause`/`unpause` require a majority of the *registered* signers. A
    // stranger's signature satisfies `require_auth` but not membership, so the
    // call must fail on the membership check rather than on the auth check.
    assert!(
        c.factory.try_pause(&signers).is_err(),
        "a non-member signer must not be able to pause the factory"
    );
    assert!(!c.factory.is_paused());
}

// ---------------------------------------------------------------------------
// freeze_pair / unfreeze_pair: fast single-key freeze, quorum-only unfreeze
// ---------------------------------------------------------------------------

#[test]
fn freeze_pair_rejects_a_stranger_signing_alone() {
    let c = Ctx::new();
    let signers = Vec::from_array(&c.env, [c.stranger.clone()]);

    auth::allow(
        &c.env,
        &c.stranger,
        &c.factory_id,
        "freeze_pair",
        auth_args!(&c.env, signers.clone(), c.pair.clone()),
    );
    assert!(
        c.factory.try_freeze_pair(&signers, &c.pair).is_err(),
        "only the fee_to_setter (or the quorum) may freeze a pair"
    );
    assert!(!c.factory.is_pair_frozen(&c.pair), "state must be unchanged");
}

#[test]
fn freeze_pair_by_the_setter_requires_the_setters_signature() {
    let c = Ctx::new();
    let signers = Vec::from_array(&c.env, [c.setter.clone()]);

    auth::allow_nothing(&c.env);
    assert!(
        c.factory.try_freeze_pair(&signers, &c.pair).is_err(),
        "naming the fee_to_setter without its signature must not freeze"
    );
}

#[test]
fn unfreeze_pair_rejects_the_setter_signing_alone() {
    let c = Ctx::new();
    let signers = Vec::from_array(&c.env, [c.setter.clone()]);

    auth::allow(
        &c.env,
        &c.setter,
        &c.factory_id,
        "unfreeze_pair",
        auth_args!(&c.env, signers.clone(), c.pair.clone()),
    );
    assert!(
        c.factory.try_unfreeze_pair(&signers, &c.pair).is_err(),
        "the single freeze key must not be able to reopen a pool"
    );
}
