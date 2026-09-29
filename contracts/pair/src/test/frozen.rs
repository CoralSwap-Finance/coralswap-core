//! The factory-admin freeze: a pool that must stop moving, and start moving
//! again, on one address's word.
//!
//! `Factory::freeze_pair` reaches the pool through `Pair::set_frozen`, so these
//! tests pin the pair-side half of that contract:
//!
//! 1. every value-moving entry point refuses while frozen, with the typed
//!    [`PairError::ContractFrozen`] rather than an opaque failure,
//! 2. the refusal happens before any funds move,
//! 3. unfreezing restores full functionality — the acceptance round trip
//!    (swap reverts → unfreeze → swap succeeds), and
//! 4. observation is never blocked: an incident responder must still be able to
//!    read reserves and `sync` a pool whose balances drifted.

use crate::{errors::PairError, Pair, PairClient};
use coralswap_lp_token::{LpToken, LpTokenClient};
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, Env, String,
};

const RESERVE: i128 = 1_000_000;
/// Comfortably above `MINIMUM_RESERVE` and large enough that the fee-adjusted
/// k check passes with room to spare on a 1_000_000 / 1_000_000 pool.
const SWAP_IN: i128 = 1_000;
const SWAP_OUT: i128 = 900;

struct Harness {
    env: Env,
    pair: PairClient<'static>,
    pair_id: Address,
    lp: LpTokenClient<'static>,
    token_a_id: Address,
    token_b_id: Address,
    token_a_admin: StellarAssetClient<'static>,
    token_b_admin: StellarAssetClient<'static>,
    user: Address,
}

impl Harness {
    fn token(&self, id: &Address) -> TokenClient<'_> {
        TokenClient::new(&self.env, id)
    }

    /// Moves `amount` of `token_id` from the freshly minted user balance into
    /// the pair, exactly as a trader would before a swap.
    fn deposit_input(&self, token_id: &Address, amount: i128) {
        self.token(token_id).transfer(&self.user, &self.pair_id, &amount);
    }

    fn mint_user(&self, token_id: &Address, amount: i128) {
        let admin =
            if *token_id == self.token_a_id { &self.token_a_admin } else { &self.token_b_admin };
        admin.mint(&self.user, &amount);
    }
}

/// Wires a pair the way the factory wires one: real LP token with the pair as
/// its admin, real Stellar-asset tokens, and a seeded 1:1 pool so every entry
/// point has something to act on.
fn setup() -> Harness {
    let env = Env::default();
    // BLANKET MOCK (issue #314): freeze semantics, not authorization. The
    // authorization matrix covers who may flip the flag.
    env.mock_all_auths_allowing_non_root_auth();

    let asset_admin = Address::generate(&env);
    let token_a_id = env.register_stellar_asset_contract_v2(asset_admin.clone()).address();
    let token_b_id = env.register_stellar_asset_contract_v2(asset_admin).address();
    let lp_id = env.register(LpToken, ());
    let pair_id = env.register(Pair, ());
    let user = Address::generate(&env);

    LpTokenClient::new(&env, &lp_id).initialize(
        &pair_id,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );

    let pair = PairClient::new(&env, &pair_id);
    pair.initialize(&Address::generate(&env), &token_a_id, &token_b_id, &lp_id);

    let token_a_admin = StellarAssetClient::new(&env, &token_a_id);
    let token_b_admin = StellarAssetClient::new(&env, &token_b_id);
    token_a_admin.mint(&user, &RESERVE);
    token_b_admin.mint(&user, &RESERVE);
    TokenClient::new(&env, &token_a_id).transfer(&user, &pair_id, &RESERVE);
    TokenClient::new(&env, &token_b_id).transfer(&user, &pair_id, &RESERVE);
    pair.mint(&user);

    let lp = LpTokenClient::new(&env, &lp_id);

    Harness { env, pair, lp, pair_id, token_a_id, token_b_id, token_a_admin, token_b_admin, user }
}

// ─────────────────────────────────────────────────────────────────────────
// Every value-moving path refuses while frozen
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn frozen_pool_rejects_swap() {
    let h = setup();
    h.pair.set_frozen(&true);

    // The input is deposited first, so only the freeze can be what refuses.
    h.mint_user(&h.token_a_id, SWAP_IN);
    h.deposit_input(&h.token_a_id, SWAP_IN);

    let err = h.pair.try_swap(&0, &SWAP_OUT, &h.user).expect_err("swap must revert while frozen");
    assert_eq!(err, Ok(PairError::ContractFrozen), "failure must be a typed pair error");
}

#[test]
fn frozen_pool_rejects_mint() {
    let h = setup();
    h.mint_user(&h.token_a_id, SWAP_IN);
    h.mint_user(&h.token_b_id, SWAP_IN);
    h.deposit_input(&h.token_a_id, SWAP_IN);
    h.deposit_input(&h.token_b_id, SWAP_IN);

    h.pair.set_frozen(&true);

    let err = h.pair.try_mint(&h.user).expect_err("mint must revert while frozen");
    assert_eq!(err, Ok(PairError::ContractFrozen));
}

#[test]
fn frozen_pool_rejects_burn() {
    let h = setup();
    // `Pair::burn` redeems the LP the pair custodies, so the holder transfers
    // their position in first — otherwise the freeze would only be beating an
    // earlier failure.
    let user_lp = h.lp.balance(&h.user);
    h.lp.transfer(&h.user, &h.pair_id, &user_lp);

    h.pair.set_frozen(&true);

    let err = h.pair.try_burn(&h.user).expect_err("burn must revert while frozen");
    assert_eq!(err, Ok(PairError::ContractFrozen));
}

#[test]
fn frozen_pool_rejects_flash_loan() {
    let h = setup();
    h.pair.set_frozen(&true);

    // The guard fires before the receiver is ever invoked, so a bare address
    // stands in for one.
    let receiver = Address::generate(&h.env);
    let err = h
        .pair
        .try_flash_loan(&receiver, &1_000i128, &0i128, &Bytes::new(&h.env))
        .expect_err("flash loan must revert while frozen");
    assert_eq!(err, Ok(PairError::ContractFrozen));
}

#[test]
fn frozen_pool_rejects_single_sided_mint_and_burn() {
    let h = setup();
    h.mint_user(&h.token_a_id, 50_000);
    h.pair.set_frozen(&true);

    let mint_err = h
        .pair
        .try_mint_with_one_token(&h.user, &h.token_a_id, &50_000i128, &1i128)
        .expect_err("single-sided mint must revert while frozen");
    assert_eq!(mint_err, Ok(PairError::ContractFrozen));

    let burn_err = h
        .pair
        .try_burn_single_side(&h.user, &1_000i128, &h.token_b_id, &1i128)
        .expect_err("single-sided burn must revert while frozen");
    assert_eq!(burn_err, Ok(PairError::ContractFrozen));
}

// ─────────────────────────────────────────────────────────────────────────
// Nothing half-executes, and observation stays open
// ─────────────────────────────────────────────────────────────────────────

/// A refused swap must leave the pool exactly as it was: the deposit that
/// arrived beforehand stays a balance (recoverable via `sync`), and reserves
/// are untouched.
#[test]
fn a_refused_swap_leaves_reserves_untouched() {
    let h = setup();
    let reserves_before = h.pair.get_reserves();

    h.pair.set_frozen(&true);
    h.mint_user(&h.token_a_id, SWAP_IN);
    h.deposit_input(&h.token_a_id, SWAP_IN);

    assert!(h.pair.try_swap(&0, &SWAP_OUT, &h.user).is_err());
    assert_eq!(h.pair.get_reserves(), reserves_before, "reserves must not move");
}

/// A freeze is a trading halt, not a blackout: an incident responder still
/// needs reads and `sync` to see what a compromised pool actually holds.
#[test]
fn views_and_sync_keep_working_while_frozen() {
    let h = setup();
    h.pair.set_frozen(&true);

    assert!(h.pair.is_frozen());
    let (_a, _b, _t) = h.pair.get_reserves();
    let _ = h.pair.get_current_fee_bps();
    h.pair.sync();

    // LP tokens are a separate layer: freezing the pool must not trap holders.
    let user_lp = h.lp.balance(&h.user);
    assert!(user_lp > 0, "the holder keeps their position");
    h.lp.transfer(&h.user, &Address::generate(&h.env), &(user_lp / 2));
}

// ─────────────────────────────────────────────────────────────────────────
// Acceptance: freeze → swap reverts → unfreeze → swap succeeds
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn freeze_then_unfreeze_round_trip_restores_swaps() {
    let h = setup();

    // Baseline: the pool trades normally before any freeze.
    h.mint_user(&h.token_a_id, SWAP_IN);
    h.deposit_input(&h.token_a_id, SWAP_IN);
    h.pair.swap(&0, &SWAP_OUT, &h.user);

    // Deposit the next trade's input, then freeze mid-flight.
    h.mint_user(&h.token_a_id, SWAP_IN);
    h.deposit_input(&h.token_a_id, SWAP_IN);
    h.pair.set_frozen(&true);

    let err = h.pair.try_swap(&0, &SWAP_OUT, &h.user).expect_err("swap must revert while frozen");
    assert_eq!(err, Ok(PairError::ContractFrozen));

    // Thaw: the very same swap, with the very same input already sitting in
    // the pool, must go through.
    h.pair.set_frozen(&false);
    assert!(!h.pair.is_frozen());
    h.pair
        .try_swap(&0, &SWAP_OUT, &h.user)
        .expect("swap invocation must succeed")
        .expect("swap must succeed again after unfreezing");

    // And liquidity keeps working too, so the unfreeze is a full restore.
    // Deposited at a size that clears the `MIN_LIQUIDITY_FOR_USER` floor on a
    // ~1_000_000-share supply, so the mint is not refused as dust.
    const MINT_IN: i128 = 100_000;
    h.mint_user(&h.token_a_id, MINT_IN);
    h.mint_user(&h.token_b_id, MINT_IN);
    h.deposit_input(&h.token_a_id, MINT_IN);
    h.deposit_input(&h.token_b_id, MINT_IN);
    assert!(h.pair.try_mint(&h.user).is_ok(), "mint must work again after unfreezing");
}

/// Flipping the flag twice in the same direction is a no-op, not an error: a
/// responder retrying an incident runbook must not wedge the pool.
#[test]
fn freeze_and_unfreeze_are_idempotent() {
    let h = setup();

    h.pair.set_frozen(&true);
    h.pair.set_frozen(&true);
    assert!(h.pair.is_frozen());

    h.pair.set_frozen(&false);
    h.pair.set_frozen(&false);
    assert!(!h.pair.is_frozen());

    h.mint_user(&h.token_a_id, SWAP_IN);
    h.deposit_input(&h.token_a_id, SWAP_IN);
    assert!(h.pair.try_swap(&0, &SWAP_OUT, &h.user).is_ok());
}
