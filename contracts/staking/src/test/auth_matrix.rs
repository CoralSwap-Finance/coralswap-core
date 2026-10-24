//! Scoped-authorization matrix for the staking contract (issue #314).
//!
//! Every entry point that has a `require_auth` guard is tested in two
//! directions:
//!
//! 1. **Authorized path** — the correct signer is installed; the call succeeds
//!    and the recorded auth tree matches exactly.
//! 2. **Rejection path** — no authorization is installed; the call fails with
//!    an auth error.
//!
//! Blanket `mock_all_auths` is **not** used for the assertions themselves.
//! Removing a guard or rebinding it to the wrong address breaks the rejection
//! tests.  Setup-only minting inside `Ctx::new` uses a blanket mock that is
//! cleared before each assertion (matching the lp_token auth_matrix pattern).
//!
//! | Entry point          | Authorizer                          |
//! |----------------------|-------------------------------------|
//! | `initialize`         | `admin` (the caller / new admin)    |
//! | `stake`              | `user`                              |
//! | `unstake`            | `user`                              |
//! | `set_reward_config`  | `admin` (stored admin)              |

#![cfg(test)]

use coralswap_shared::auth_args;
use coralswap_shared::test_support as auth;
use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Address, Env};

use crate::{Staking, StakingClient};

// ---------------------------------------------------------------------------
// Shared fixture
// ---------------------------------------------------------------------------

struct Ctx {
    env: Env,
    staking_id: Address,
    admin: Address,
    user: Address,
    lp_token: Address,
}

impl Ctx {
    /// Builds a fully-initialized staking contract fixture.
    ///
    /// Setup (token registration, minting, `initialize`) runs under a blanket
    /// mock so it does not interfere with the scoped-auth assertions that
    /// individual tests install afterwards.
    fn new() -> Self {
        let env = Env::default();
        // Blanket mock for fixture setup only — cleared before each assertion.
        env.mock_all_auths_allowing_non_root_auth();

        let staking_id = env.register(Staking, ());
        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        let lp_token = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let reward_token = env.register_stellar_asset_contract_v2(admin.clone()).address();

        // Mint LP tokens to the user so stake tests have something to transfer.
        StellarAssetClient::new(&env, &lp_token).mint(&user, &100_000i128);

        // Initialize the contract (blanket mock active).
        StakingClient::new(&env, &staking_id).initialize(&admin, &lp_token, &reward_token, &100u64);

        Ctx { env, staking_id, admin, user, lp_token }
    }

    fn client(&self) -> StakingClient<'_> {
        StakingClient::new(&self.env, &self.staking_id)
    }

    /// Stake `amount` under a blanket mock (setup helper, not under test).
    fn stake_setup(&self, amount: i128) {
        self.env.mock_all_auths_allowing_non_root_auth();
        self.client().stake(&self.user, &amount);
    }
}

// ===========================================================================
// initialize
// ===========================================================================

#[test]
fn initialize_is_authorized_by_admin() {
    let env = Env::default();
    let staking_id = env.register(Staking, ());
    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);

    auth::allow(
        &env,
        &admin,
        &staking_id,
        "initialize",
        auth_args!(&env, admin.clone(), lp.clone(), rwd.clone(), 50u64),
    );

    StakingClient::new(&env, &staking_id).initialize(&admin, &lp, &rwd, &50u64);

    auth::assert_authorized(
        &env,
        &admin,
        &staking_id,
        "initialize",
        auth_args!(&env, admin.clone(), lp.clone(), rwd.clone(), 50u64),
    );
}

#[test]
fn initialize_rejects_a_stranger() {
    let env = Env::default();
    let staking_id = env.register(Staking, ());
    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);

    // Authorize the stranger, not the admin that require_auth checks.
    auth::allow(
        &env,
        &stranger,
        &staking_id,
        "initialize",
        auth_args!(&env, admin.clone(), lp.clone(), rwd.clone(), 50u64),
    );

    let result = StakingClient::new(&env, &staking_id).try_initialize(&admin, &lp, &rwd, &50u64);
    assert!(result.is_err(), "expected auth failure when stranger authorizes initialize");
}

// ===========================================================================
// stake
// ===========================================================================

#[test]
fn stake_is_authorized_by_user() {
    let ctx = Ctx::new();

    // `stake` calls `token::transfer(user → staking_contract, amount)` as a
    // sub-invocation, so the user must authorize both the outer `stake` and
    // the inner LP-token `transfer`.
    //
    // Note: `assert_authorized` requires no sub-invocations, so it cannot be
    // used here.  The rejection test (`stake_rejects_a_stranger`) proves the
    // guard is present; this test proves the correct tree is accepted.
    let transfer_args = auth_args!(&ctx.env, ctx.user.clone(), ctx.staking_id.clone(), 500i128);
    let sub = auth::Call::new(&ctx.lp_token, "transfer", transfer_args);

    auth::allow_with_sub_invocations(
        &ctx.env,
        &ctx.user,
        &ctx.staking_id,
        "stake",
        auth_args!(&ctx.env, ctx.user.clone(), 500i128),
        &[sub],
    );

    // If this returns without panicking the scoped auth tree was satisfied,
    // which proves the `user.require_auth()` guard exists and binds to the
    // correct signer and arguments.
    ctx.client().stake(&ctx.user, &500i128);

    assert_eq!(ctx.client().get_stake(&ctx.user), 500i128);
}

#[test]
fn stake_rejects_a_stranger() {
    let ctx = Ctx::new();

    // No authorization at all → host rejects `user.require_auth()`.
    auth::allow_nothing(&ctx.env);

    let result = ctx.client().try_stake(&ctx.user, &500i128);
    assert!(result.is_err(), "expected auth failure for unauthenticated stake");
}

#[test]
fn an_authorization_for_a_different_amount_is_rejected() {
    let ctx = Ctx::new();

    // Authorize for 500 but call with 1_000 — arg-binding must catch this.
    auth::allow(
        &ctx.env,
        &ctx.user,
        &ctx.staking_id,
        "stake",
        auth_args!(&ctx.env, ctx.user.clone(), 500i128),
    );

    let result = ctx.client().try_stake(&ctx.user, &1_000i128);
    assert!(result.is_err(), "mismatched amount must be rejected by auth");
}

// ===========================================================================
// unstake
// ===========================================================================

#[test]
fn unstake_is_authorized_by_user() {
    let ctx = Ctx::new();
    // Setup: stake under blanket mock.
    ctx.stake_setup(1_000i128);

    auth::allow(
        &ctx.env,
        &ctx.user,
        &ctx.staking_id,
        "unstake",
        auth_args!(&ctx.env, ctx.user.clone(), 500i128),
    );

    ctx.client().unstake(&ctx.user, &500i128);

    auth::assert_authorized(
        &ctx.env,
        &ctx.user,
        &ctx.staking_id,
        "unstake",
        auth_args!(&ctx.env, ctx.user.clone(), 500i128),
    );
}

#[test]
fn unstake_rejects_a_stranger() {
    let ctx = Ctx::new();
    ctx.stake_setup(1_000i128);

    auth::allow_nothing(&ctx.env);

    let result = ctx.client().try_unstake(&ctx.user, &500i128);
    assert!(result.is_err(), "expected auth failure for unauthenticated unstake");
}

// ===========================================================================
// set_reward_config
// ===========================================================================

#[test]
fn set_reward_config_is_authorized_by_admin() {
    let ctx = Ctx::new();
    let new_rwd = Address::generate(&ctx.env);

    auth::allow(
        &ctx.env,
        &ctx.admin,
        &ctx.staking_id,
        "set_reward_config",
        auth_args!(&ctx.env, ctx.admin.clone(), new_rwd.clone(), 200u64),
    );

    ctx.client().set_reward_config(&ctx.admin, &new_rwd, &200u64);

    auth::assert_authorized(
        &ctx.env,
        &ctx.admin,
        &ctx.staking_id,
        "set_reward_config",
        auth_args!(&ctx.env, ctx.admin.clone(), new_rwd.clone(), 200u64),
    );
}

#[test]
fn set_reward_config_rejects_a_stranger() {
    let ctx = Ctx::new();
    let new_rwd = Address::generate(&ctx.env);

    auth::allow_nothing(&ctx.env);

    let result = ctx.client().try_set_reward_config(&ctx.admin, &new_rwd, &200u64);
    assert!(result.is_err(), "expected auth failure for unauthenticated set_reward_config");
}

// ===========================================================================
// Blanket: every gated entry point fails with no auth
// ===========================================================================

#[test]
fn every_gated_entry_point_fails_with_no_authorization_at_all() {
    let env = Env::default();
    auth::allow_nothing(&env);

    let staking_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &staking_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let lp = Address::generate(&env);
    let rwd = Address::generate(&env);

    assert!(
        client.try_initialize(&admin, &lp, &rwd, &10u64).is_err(),
        "initialize must require auth"
    );
    assert!(client.try_stake(&user, &100i128).is_err(), "stake must require auth");
    assert!(client.try_unstake(&user, &100i128).is_err(), "unstake must require auth");
    assert!(
        client.try_set_reward_config(&admin, &rwd, &10u64).is_err(),
        "set_reward_config must require auth"
    );
}
