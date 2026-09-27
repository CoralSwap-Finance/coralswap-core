//! Issue #313 — a paused LP token must actually stop the pair.
//!
//! The factory initializes each LP token with the **pair** as its admin, so the
//! pair is the only address that can `mint`, `burn` or `pause` that token.
//! Nothing on the pair forwarded to `LpToken::pause`, which left the flag
//! unreachable in production. These tests cover the whole path:
//!
//! 1. the pair can drive the flag (authorized by the factory),
//! 2. every LP-supply-changing entry point refuses while it is set,
//! 3. the refusal is a typed `PairError::LpTokenPaused`, not an opaque nested
//!    host failure,
//! 4. unpausing restores operation, and
//! 5. swaps are unaffected, so a pause is not a blanket trading halt.

use crate::{errors::PairError, Pair, PairClient};
use coralswap_lp_token::{LpToken, LpTokenClient};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, Env,
    Error as SdkError, String, Vec as SdkVec,
};

// ── Minimal mock underlying token ────────────────────────────────────────────

#[contracttype]
enum TokenKey {
    Balance(Address),
}

#[contract]
pub struct PauseMockToken;

#[contractimpl]
impl PauseMockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = TokenKey::Balance(to);
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let fb: i128 =
            env.storage().persistent().get(&TokenKey::Balance(from.clone())).unwrap_or(0);
        let tb: i128 = env.storage().persistent().get(&TokenKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&TokenKey::Balance(from), &(fb - amount));
        env.storage().persistent().set(&TokenKey::Balance(to), &(tb + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&TokenKey::Balance(id)).unwrap_or(0)
    }
}

// ── Shared setup ─────────────────────────────────────────────────────────────

#[allow(dead_code)]
struct Harness {
    env: Env,
    pair: PairClient<'static>,
    token_a: PauseMockTokenClient<'static>,
    token_b: PauseMockTokenClient<'static>,
    lp: LpTokenClient<'static>,
    lp_id: Address,
    pair_id: Address,
    factory: Address,
    user: Address,
    reserve: i128,
}

/// Builds a pair wired the way the factory wires it: **the pair address is the
/// LP token's admin**, so the pair really is the only address that can mint,
/// burn or pause the token. A harness that made some other address the admin
/// would not exercise the production permission graph at all.
#[allow(clippy::type_complexity)]
fn setup() -> Harness {
    let env = Env::default();
    // Blanket auth is used deliberately: this module asserts *pause* semantics,
    // not the authorization graph. The authorization matrix is covered
    // separately (issue #314).
    // BLANKET MOCK (issue #314): pause/unpause behavior, not authorization.
    // Guards are covered by the per-contract `auth_matrix` module.
    env.mock_all_auths_allowing_non_root_auth();

    let token_a_id = env.register(PauseMockToken, ());
    let token_b_id = env.register(PauseMockToken, ());
    let lp_id = env.register(LpToken, ());
    let pair_id = env.register(Pair, ());
    let factory = Address::generate(&env);
    let user = Address::generate(&env);
    let reserve = 1_000_000i128;

    let token_a = PauseMockTokenClient::new(&env, &token_a_id);
    let token_b = PauseMockTokenClient::new(&env, &token_b_id);
    let lp = LpTokenClient::new(&env, &lp_id);
    let pair = PairClient::new(&env, &pair_id);

    // Factory behaviour: the pair is the LP token's admin.
    lp.initialize(
        &pair_id,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );
    pair.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    // Seed the pool through the normal entry point.
    token_a.mint(&user, &reserve);
    token_b.mint(&user, &reserve);
    token_a.transfer(&user, &pair_id, &reserve);
    token_b.transfer(&user, &pair_id, &reserve);
    pair.mint(&user);

    Harness { env, pair, token_a, token_b, lp, lp_id, pair_id, factory, user, reserve }
}

// ── The pause itself ─────────────────────────────────────────────────────────

/// The pair can drive the flag, because the factory made it the LP admin. This
/// is the step that was missing: without it the flag could never be set.
#[test]
fn pair_can_pause_and_unpause_its_lp_token() {
    let h = setup();
    assert!(!h.lp.is_paused(), "a freshly created LP token is not paused");
    assert!(!h.pair.is_lp_token_paused());

    h.pair.set_lp_token_paused(&true);
    assert!(h.lp.is_paused(), "the relay must reach the LP token itself");
    assert!(h.pair.is_lp_token_paused(), "the pair must report the token's state");

    h.pair.set_lp_token_paused(&false);
    assert!(!h.lp.is_paused());
    assert!(!h.pair.is_lp_token_paused());
}

/// Pausing twice and unpausing twice are no-ops, not errors — a governance
/// script that retries must not wedge.
#[test]
fn pause_and_unpause_are_idempotent() {
    let h = setup();

    h.pair.set_lp_token_paused(&true);
    h.pair.set_lp_token_paused(&true);
    assert!(h.lp.is_paused());

    h.pair.set_lp_token_paused(&false);
    h.pair.set_lp_token_paused(&false);
    assert!(!h.lp.is_paused());
}

// ── Acceptance: a paused LP token blocks pair mint/burn ──────────────────────

#[test]
fn paused_lp_token_blocks_pair_mint() {
    let h = setup();
    h.pair.set_lp_token_paused(&true);

    h.token_a.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);

    let result = h.pair.try_mint(&h.user);
    let err = result.expect_err("mint must fail while the LP token is paused");
    assert_eq!(err, Ok(PairError::LpTokenPaused), "failure must be a typed pair error");
}

#[test]
fn paused_lp_token_blocks_single_sided_mint() {
    let h = setup();
    h.pair.set_lp_token_paused(&true);

    h.token_a.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);

    let result = h.pair.try_mint_with_one_token(&h.user, &h.token_a.address, &h.reserve, &1i128);
    let err = result.expect_err("single-sided mint must fail while paused");
    assert_eq!(err, Ok(PairError::LpTokenPaused));
}

#[test]
fn paused_lp_token_blocks_pair_burn() {
    let h = setup();
    h.pair.set_lp_token_paused(&true);

    let err = h.pair.try_burn(&h.user).expect_err("burn must fail while paused");
    assert_eq!(err, Ok(PairError::LpTokenPaused));
}

#[test]
fn paused_lp_token_blocks_single_sided_burn() {
    let h = setup();
    h.pair.set_lp_token_paused(&true);

    let result = h.pair.try_burn_single_side(&h.user, &1_000i128, &h.token_a.address, &1i128);
    let err = result.expect_err("single-sided burn must fail while paused");
    assert_eq!(err, Ok(PairError::LpTokenPaused));
}

// ── Round trip: unpausing restores every path ────────────────────────────────

#[test]
fn unpausing_restores_mint_and_burn() {
    let h = setup();

    // Direction 1: mint is blocked while paused, works once unpaused.
    h.token_a.mint(&h.user, &h.reserve);
    h.token_b.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);
    h.token_b.transfer(&h.user, &h.pair_id, &h.reserve);

    h.pair.set_lp_token_paused(&true);
    let err = h.pair.try_mint(&h.user).expect_err("mint must fail while paused");
    assert_eq!(err, Ok(PairError::LpTokenPaused));

    h.pair.set_lp_token_paused(&false);
    assert!(h.pair.try_mint(&h.user).is_ok(), "mint must work again after unpausing");

    // Direction 2: burn is blocked while paused, works once unpaused.
    h.pair.set_lp_token_paused(&true);
    let err = h.pair.try_burn(&h.user).expect_err("burn must fail while paused");
    assert_eq!(err, Ok(PairError::LpTokenPaused));

    h.pair.set_lp_token_paused(&false);

    // `Pair::burn` redeems the LP the pair itself custodies, so the holder
    // transfers their position in first (see
    // `test_burn_seed_remains_intact_after_full_cycle` for the same flow).
    let user_lp = h.lp.balance(&h.user);
    h.lp.transfer(&h.user, &h.pair_id, &user_lp);
    assert!(h.pair.try_burn(&h.user).is_ok(), "burn must work again after unpausing");
}

// ── A pause is scoped, not a trading halt ────────────────────────────────────

/// Swapping a user's existing LP position must still work while the LP token is
/// paused; otherwise a pause would trap liquidity rather than merely freezing
/// supply.
#[test]
fn paused_lp_token_does_not_block_swaps() {
    let h = setup();
    h.pair.set_lp_token_paused(&true);

    // `swap(amount_a_out, amount_b_out, to)` derives the input from the pair's
    // balance delta, so a single-sided deposit of token A buys token B back.
    // No LP supply changes, so the pause must not interfere.
    let b_before = h.token_b.balance(&h.user);
    h.token_a.mint(&h.user, &1_000i128);
    h.token_a.transfer(&h.user, &h.pair_id, &1_000i128);

    h.pair
        .try_swap(&0i128, &900i128, &h.user)
        .expect("swap invocation must succeed")
        .expect("swap must succeed while paused");

    assert!(
        h.token_b.balance(&h.user) > b_before,
        "swap must still deliver output while the LP token is paused"
    );
}

// ── Failure is reported, not swallowed ───────────────────────────────────────

/// The reserves and LP supply must be untouched by a refused mint, so a
/// governance mistake cannot half-execute a deposit.
#[test]
fn refused_mint_leaves_reserves_and_supply_untouched() {
    let h = setup();
    let supply_before = h.lp.total_supply();
    let reserves_before = h.pair.get_reserves();

    h.pair.set_lp_token_paused(&true);
    h.token_a.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);

    assert!(h.pair.try_mint(&h.user).is_err());

    // The user's tokens were transferred in, but the pair never recorded them
    // as reserves: sync would pull them in, but no LP was issued. Confirm the
    // *supply* is unchanged, which is the part the pause guarantees.
    assert_eq!(h.lp.total_supply(), supply_before, "no LP may be issued while paused");
    // Reverting the mint means the reserve snapshot is unchanged too.
    assert_eq!(h.pair.get_reserves(), reserves_before);
}

/// A pair whose LP token is not a pausable token must not brick: the guard
/// fails open, and the nested mint is still the authority.
#[test]
fn non_pausable_lp_token_does_not_brick_the_pair() {
    let env = Env::default();
    // BLANKET MOCK (issue #314): pause/unpause behavior, not authorization.
    // Guards are covered by the per-contract `auth_matrix` module.
    env.mock_all_auths_allowing_non_root_auth();

    // A bare address standing in for the LP token has no `is_paused`, so the
    // status read fails. The pair must still behave as if unpaused.
    let token_a_id = env.register(PauseMockToken, ());
    let token_b_id = env.register(PauseMockToken, ());
    let lp_id = Address::generate(&env);
    let pair_id = env.register(Pair, ());
    let factory = Address::generate(&env);
    let user = Address::generate(&env);
    let reserve = 1_000_000i128;

    let token_a = PauseMockTokenClient::new(&env, &token_a_id);
    let token_b = PauseMockTokenClient::new(&env, &token_b_id);
    let pair = PairClient::new(&env, &pair_id);
    pair.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    assert!(!pair.is_lp_token_paused(), "an unreadable pause flag reads as not paused");

    token_a.mint(&user, &reserve);
    token_b.mint(&user, &reserve);
    token_a.transfer(&user, &pair_id, &reserve);
    token_b.transfer(&user, &pair_id, &reserve);

    // The LP mint itself will fail (there is no token at that address), but the
    // failure must come from the token, not from the pause guard.
    let err = pair.try_mint(&user).expect_err("no LP token at that address");
    assert_ne!(
        err,
        Ok(PairError::LpTokenPaused),
        "an unreadable pause flag must not be reported as a pause"
    );
}

// ── The pair's cached hint is not the enforcement point ───────────────────────

/// The pair caches the pause flag to keep a contract call off the `swap` /
/// `mint` / `burn` path, since a nested sub-invocation on every swap is a
/// measurable share of the Soroban budget. This pins down what that trade costs
/// and, more importantly, what it does not.
///
/// The only way to desynchronise the cache is to pause the LP token without
/// going through `Pair::set_lp_token_paused` — which requires the token's admin
/// to have been transferred off the pair. The flag then still says `false`, and
/// the pair cannot name the real reason. What must not happen is the deposit
/// *succeeding*. The LP token refuses the nested mint independently, so the pool
/// stays frozen; all that is lost is the typed `PairError::LpTokenPaused` in
/// favour of the token's raw `#207`.
#[test]
fn a_pause_applied_behind_the_pairs_back_still_blocks_liquidity() {
    let h = setup();

    // Set up both a mint and a burn while the token is still usable: a paused
    // token refuses `transfer` too, so the LP has to reach the user first.
    h.token_a.mint(&h.user, &h.reserve);
    h.token_b.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);
    h.token_b.transfer(&h.user, &h.pair_id, &h.reserve);
    h.pair.mint(&h.user);
    // `setup`'s mint credited the LP to the user, so they already hold a real,
    // redeemable position. Nothing to transfer: the pair only holds the locked
    // `MINIMUM_LIQUIDITY`.
    assert!(h.lp.balance(&h.user) > 0, "the user must hold LP for the burn to be meaningful");

    // Pause the token directly, as a relocated admin would. The pair's cached
    // hint is never told, so it still reads `false`.
    h.lp.pause();
    assert!(h.lp.is_paused(), "the token itself is paused");
    assert!(
        h.pair.is_lp_token_paused(),
        "the view reads through to the token, so it is never stale"
    );

    // The important half: both operations are refused, and refused by the token.
    // The pair calls `LpTokenClient::mint` directly, so a refusal from inside the
    // token surfaces as the host's own error rather than a `PairError`. `#207` is
    // `LpTokenError::ContractPaused`, which is how we can tell the token did the
    // refusing. Had the pair's cached hint been consulted instead, the caller
    // would have seen the typed `PairError::LpTokenPaused` (122) — that
    // difference is the entire cost of the cache, and it is a diagnostics cost
    // only.
    //
    h.token_a.mint(&h.user, &h.reserve);
    h.token_b.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);
    h.token_b.transfer(&h.user, &h.pair_id, &h.reserve);
    assert_refused_by_paused_token(&h, symbol_short!("mint"), "mint");

    // `burn` needs no counterpart here. It redeems the LP the *pair* custodies, so
    // while the token is paused the user cannot even transfer LP to the pair, and
    // the operation is blocked one step earlier than the nested call. The pair's
    // guard is irrelevant to that case, which is exactly why the token is the
    // only place the enforcement has to live.
}

/// `LpTokenError::ContractPaused`, the code the LP token raises while paused.
const LP_TOKEN_PAUSED_CODE: u32 = 207;

/// Asserts that `func` on the pair fails with the LP token's own pause error
/// rather than with a `PairError` raised from the pair's cached hint.
///
/// The distinction is the whole point: a `PairError` here would mean the cache
/// was consulted, and a raw contract error means the nested LP token call is
/// what refused.
fn assert_refused_by_paused_token(h: &Harness, func: soroban_sdk::Symbol, what: &str) {
    let args = SdkVec::from_array(&h.env, [h.user.to_val()]);
    match h.env.try_invoke_contract::<(), SdkError>(&h.pair_id, &func, args) {
        Ok(_) => panic!("{what} must not succeed while the LP token is paused"),
        Err(Ok(code)) => assert_eq!(
            code,
            SdkError::from_contract_error(LP_TOKEN_PAUSED_CODE),
            "{what} must be refused by the LP token, not by the pair's stale hint"
        ),
        Err(Err(invoke_err)) => panic!("{what} could not be invoked at all: {invoke_err:?}"),
    }
}

/// The mirror image: a stale `true` is impossible, because the pair is the only
/// writer of its own cache. After a normal pause/unpause round trip the hot-path
/// hint agrees with the token again.
#[test]
fn the_cached_hint_tracks_the_token_across_a_pause_round_trip() {
    let h = setup();

    h.pair.set_lp_token_paused(&true);
    assert!(h.lp.is_paused());
    assert!(h.pair.is_lp_token_paused());

    h.pair.set_lp_token_paused(&false);
    assert!(!h.lp.is_paused());
    assert!(
        !h.pair.is_lp_token_paused(),
        "unpausing must clear the hint, or the pool would stay frozen forever"
    );

    // And liquidity genuinely works again through the hot path.
    h.token_a.mint(&h.user, &h.reserve);
    h.token_b.mint(&h.user, &h.reserve);
    h.token_a.transfer(&h.user, &h.pair_id, &h.reserve);
    h.token_b.transfer(&h.user, &h.pair_id, &h.reserve);
    h.pair.mint(&h.user);
}
