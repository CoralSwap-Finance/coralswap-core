**Updated `contracts/pair/src/lib.rs`**

```rust
#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod dynamic_fee;
pub mod errors;
mod events;
mod factory_client;
mod fee_decay;
mod flash_loan;
pub mod math;
mod oracle;
mod reentrancy;
mod storage;

#[cfg(test)]
mod test;

use dynamic_fee::{compute_fee_bps, DEFAULT_STALE_LEDGER_THRESHOLD};
use errors::PairError;
use events::PairEvents;
use factory_client::FactoryClient;
use math::MINIMUM_LIQUIDITY;
use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, token::TokenClient,
    Address, Bytes, Env, Symbol,
};
use storage::{
    get_fee_state, get_frozen, get_lp_token_paused, get_pair_state, get_pending_lp,
    get_pending_lp_total, set_fee_state, set_frozen, set_lp_token_paused, set_pair_state,
    set_pending_lp, set_pending_lp_total, set_reentrancy_guard, FeeState, ReentrancyGuard,
};

/// Error surface of the LP-token client declared above.
///
/// The pair never inspects these variants — [`set_lp_token_paused`] maps any
/// inner error to [`PairError::LpTokenRejected`]. They exist only to satisfy the
/// `contractclient` macro, and are declared separately from `PairError` so the
/// two contracts' numeric codes can never be conflated at a call site.
///
/// The discriminants mirror `coralswap_lp_token::LpTokenError` for readability;
/// the pair cannot depend on that crate's error type without importing the
/// whole token contract into the pair's build graph.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LpTokenClientError {
    NotInitialized = 201,
    Unauthorized = 202,
    ContractPaused = 207,
}

#[contractclient(name = "LpTokenClient")]
pub trait LpTokenInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn total_supply(env: Env) -> i128;
    fn is_paused(env: Env) -> bool;
    fn pause(env: Env) -> Result<(), LpTokenClientError>;
    fn unpause(env: Env) -> Result<(), LpTokenClientError>;
}

/// Returns `true` when the pair's LP token is currently paused (issue #313).
///
/// This reads through to the LP token, so it is authoritative. It is a view and
/// not on the mint/burn/swap path, so the nested call is affordable here; the
/// hot path uses the cached flag instead — see [`ensure_lp_token_not_paused`].
fn lp_token_is_paused(env: &Env, lp_token: &Address) -> bool {
    LpTokenClient::new(env, lp_token).try_is_paused().unwrap_or(Ok(false)).unwrap_or(false)
}

/// Guard for every path that changes LP supply, and for `swap`.
///
/// Returns [`PairError::LpTokenPaused`] so a caller can tell an intentional
/// pause apart from an arithmetic failure, which is worth a lot more to clients
/// and indexers than the raw nested error the LP token would otherwise produce.
///
/// # This is a hint; the LP token is the authority
///
/// The flag is read from the pair's own storage rather than from the token. That
/// matters for cost: a contract call is a nested sub-invocation, and paying for
/// one on every `swap` — the hottest function in the protocol — is a real
/// fraction of its budget. So the pair caches what it last applied in
/// [`set_lp_token_paused`] and reads that.
///
/// The cache can only go stale in one direction, and only if the LP token's
/// admin is transferred off the pair: the new admin can pause the token directly
/// without the pair learning. Even then nothing unsafe happens — the nested
/// `mint` / `burn` still reverts inside the LP token, and this guard merely
/// reports it as [`PairError::LpTokenRejected`] instead of
/// [`PairError::LpTokenPaused`]. A stale `true` is likewise impossible, since
/// only the pair writes this key.
fn ensure_lp_token_not_paused(env: &Env) -> Result<(), PairError> {
    if get_lp_token_paused(env) {
        return Err(PairError::LpTokenPaused);
    }
    Ok(())
}

/// Guard for the price-moving and value-creating entry points (`swap`, `mint`,
/// `mint_with_one_token`, `burn_single_side`, `flash_loan`) while the pool is
/// frozen. A proportional `burn` is deliberately not guarded: it moves no price
/// and lets LPs withdraw their share during an incident, so a freeze can never
/// lock funds in the pool.
///
/// Returns [`PairError::ContractFrozen`] so an incident-response freeze is
/// distinguishable at the call site from an LP-token pause, an authorization
/// failure, or an arithmetic error.
///
/// The flag is the pair's own copy: `Factory::freeze_pair` sets it through
/// [`Pair::set_frozen`], and both writes land in the same transaction, so the
/// copy can never disagree with the factory's registry. Reading it locally
/// keeps a nested sub-invocation off the `swap` / `mint` hot path.
fn ensure_not_frozen(env: &Env) -> Result<(), PairError> {
    if get_frozen(env) {
        return Err(PairError::ContractFrozen);
    }
    Ok(())
}

/// Version of the fee model reported by `Pair::get_fee_state`.
///
/// Version 1: every fee value is an integer number of basis points
/// (1 bps = 0.01%).
pub const FEE_MODEL_VERSION: u32 = 1;

/// Fee configuration returned by `Pair::get_fee_state`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeStateView {
    /// Fee model version; tells clients how to interpret the fee fields.
    pub fee_model_version: u32,
    pub current_fee_bps: u32,
    pub baseline_fee_bps: u32,
    pub min_fee_bps: u32,
    pub max_fee_bps: u32,
}

#[contract]
pub struct Pair;

#[contractimpl]
impl Pair {
    // ─────────────────────────────────────────
    // Initialize
    // ─────────────────────────────────────────

    pub fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
    ) -> Result<(), PairError> {
        // 1. Double-init guard
        if get_pair_state(&env).is_some() {
            return Err(PairError::AlreadyInitialized);
        }

        // 2. Identical-token guard
        if token_a == token_b {
            return Err(PairError::InvalidInput);
        }

        // 3. Zero‑address / nonsense‑address guard
        //    The Soroban SDK treats an address consisting entirely of zeroes as
        //    the “null” address. We also reject any address that cannot be
        //    deserialized into a valid `Address` (e.g., malformed bytes). The
        //    `Address::try_from` conversion is used by the SDK internally; we
        //    mimic that check by ensuring the address is not the default.
        let zero_address = Address::from_bytes(&Bytes::from_array(&env, &[0; 32]));
        if token_a == zero_address || token_b == zero_address || lp_token == zero_address {
            return Err(PairError::InvalidInput);
        }

        // 4. A pool token must never be a contract identity controlled by this
        //    deployment or by an existing pool from the same factory. Otherwise
        //    reserve accounting can become circular or self-referential.
        if token_a == factory
            || token_b == factory
            || token_a == env.current_contract_address()
            || token_b == env.current_contract_address()
            || token_a == lp_token
            || token_b == lp_token
        {
            return Err(PairError::InvalidInput);
        }

        let factory_client = FactoryClient::new(&env, &factory);
        let token_is_known_contract = |token: &Address| {
            factory_client.try_is_pair(token).is_ok_and(|result| result.unwrap_or(false))
                || factory_client.try_is_lp_token(token).is_ok_and(|result| result.unwrap_or(false))
        };
        if token_is_known_contract(&token_a) || token_is_known_contract(&token_b) {
            return Err(PairError::InvalidInput);
        }

        let state = storage::PairStorage {
            factory,
            token_a,
            token_b,
            lp_token,
            reserve_a: 0,
            reserve_b: 0,
            block_timestamp_last: env.ledger().timestamp(),
            k_last: 0,
        };

        set_pair_state(&env, &state);

        // 5. Initialize FeeState with sane defaults
        let fee_state = FeeState {
            vol_accumulator: 0,
            ema_alpha: 10_000_000_000_000, // 10% of SCALE (1e14)
            baseline_fee_bps: 30,
            min_fee_bps: 10,
            max_fee_bps: 100,
            ramp_up_multiplier: 2,
            cooldown_divisor: 2,
            last_fee_update: 0,
            decay_threshold_blocks: 100,
            stale_threshold: DEFAULT_STALE_LEDGER_THRESHOLD,
        };
        set_fee_state(&env, &fee_state);

        // 6. Initialize ReentrancyGuard as unlocked
        set_reentrancy_guard(&env, &ReentrancyGuard { locked: false });

        // 7. Extend instance storage TTL (~7 days at 5s/ledger)
        Self::extend_instance_ttl(&env);

        Ok(())
    }

    fn extend_instance_ttl(env: &Env) {
        // Single source of truth: shared TTL policy (issue #390).
        // INSTANCE_TTL_THRESHOLD = 60_480 (~3.5 days), EXTEND_TO = 120_960 (~7 days).
        coralswap_shared::extend_instance_ttl(env);
    }

    // ─────────────────────────────────────────
    // Mint
    // ─────────────────────────────────────────

    pub fn mint(env: Env, to: Address) -> Result<i128, PairError> {
        to.require_auth();

        let _guard = reentrancy::ReentrancyGuard::acquire(&env)?;

        // A frozen pool accepts no deposits at all (incident response).
        ensure_not_frozen(&env)?;

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();

        // A paused LP token must not be able to accept deposits (issue #313).
        ensure_lp_token_not_paused(&env)?;

        let balance_a = TokenClient::new(&env, &state.token_a).balance(&contract);
        let balance_b = TokenClient::new(&env, &state.token_b).balance(&contract);

        let amount_a = balance_a.checked_sub(state.reserve_a).ok_or(PairError::InvalidInput)?;

        let amount_b = balance_b.checked_sub(state.reserve_b).ok_or(PairError::InvalidInput)?;

        // Minimum-reserve / dust policy (issue 393): reject 1-stroop style
        // deposits that fragment state. Both sides must meet MINIMUM_RESERVE.
        if amount_a < coralswap_shared::MINIMUM_RESERVE
            || amount_b < coralswap_shared::MINIMUM_RESERVE
        {
            return Err(PairError::DustAmount);
        }

        let lp_client = LpTokenClient::new(&env, &state.lp_token);
        let total_supply = lp_client.total_supply();

        let liquidity;

        if total_supply == 0 {
            liquidity = math::initial_liquidity(amount_a, amount_b)?;

            lp_client.mint(&contract, &MINIMUM