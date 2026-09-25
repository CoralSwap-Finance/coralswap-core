#![no_std]

//! Shared policy constants for CoralSwap contracts (issue #390).
//!
//! Single source of truth for storage TTL policy, dust guards, LP metadata
//! defaults, and router commit-reveal bounds. All four core contracts
//! (`factory`, `pair`, `router`, `lp_token`) must import from here instead of
//! defining local magic numbers.
//!
//! # Cadence math (Stellar ~5s/ledger close time)
//!
//! - 1 hour      ~= 720 ledgers
//! - 1 day       ~= 17_280 ledgers
//! - 3.5 days    ~= 60_480 ledgers (half of [`INSTANCE_TTL_EXTEND_TO`])
//! - 7 days      ~= 120_960 ledgers
//! - 30 days     ~= 518_400 ledgers
//! - 60 days     ~= 1_036_800 ledgers
//!
//! The instance-TTL policy extends when remaining TTL falls below
//! [`INSTANCE_TTL_THRESHOLD`] (3.5 days) back out to
//! [`INSTANCE_TTL_EXTEND_TO`] (7 days). Persistent LP entries live longer
//! ([`LP_PERSISTENT_THRESHOLD`] / [`LP_PERSISTENT_EXTEND_TO`]) because wallet
//! balances must survive idle periods. The factory uses its own longer bump
//! ([`FACTORY_INSTANCE_THRESHOLD`] / [`FACTORY_INSTANCE_BUMP_AMOUNT`]) since
//! it stores the pair registry.

use soroban_sdk::Env;

// ─────────────────────────────────────────────
// Instance TTL policy (pair + router)
// ─────────────────────────────────────────────

/// Extend instance TTL when it falls below this (3.5 days).
/// Replaces legacy `TTL_THRESHOLD` magic (`60_480` in pair, `50_000` in
/// router — now unified).
pub const INSTANCE_TTL_THRESHOLD: u32 = 60_480;

/// Extend instance storage out to this many ledgers (7 days).
/// Replaces legacy `TTL_EXTEND_TO = 120_960` magic in pair/router.
pub const INSTANCE_TTL_EXTEND_TO: u32 = 120_960;

/// Extend instance TTL using the canonical pair/router policy.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}

// ─────────────────────────────────────────────
// Reentrancy-guard TTL (pair)
// ─────────────────────────────────────────────

/// Threshold used when the reentrancy guard flips the lock flag.
/// Previously bare `5_000` magic in `pair::reentrancy`.
pub const REENTRANCY_TTL_THRESHOLD: u32 = 5_000;

/// Extend-to used when the reentrancy guard flips the lock flag.
pub const REENTRANCY_TTL_EXTEND_TO: u32 = 120_960;

/// Extend instance TTL for reentrancy-guard lock flips.
pub fn extend_reentrancy_ttl(env: &Env) {
    env.storage().instance().extend_ttl(
        REENTRANCY_TTL_THRESHOLD,
        REENTRANCY_TTL_EXTEND_TO,
    );
}

// ─────────────────────────────────────────────
// Factory instance TTL
// ─────────────────────────────────────────────

/// Factory threshold: ~1 day. Previously `INSTANCE_LIFETIME_THRESHOLD`.
pub const FACTORY_INSTANCE_THRESHOLD: u32 = 17_280;

/// Factory bump amount: ~30 days. Previously `INSTANCE_BUMP_AMOUNT`.
pub const FACTORY_INSTANCE_BUMP_AMOUNT: u32 = 518_400;

/// Extend instance TTL using the factory policy.
pub fn extend_factory_instance_ttl(env: &Env) {
    env.storage().instance().extend_ttl(
        FACTORY_INSTANCE_THRESHOLD,
        FACTORY_INSTANCE_BUMP_AMOUNT,
    );
}

// ─────────────────────────────────────────────
// LP-token persistent TTL
// ─────────────────────────────────────────────

/// LP persistent-entry threshold: ~30 days.
pub const LP_PERSISTENT_THRESHOLD: u32 = 518_400;

/// LP persistent-entry extend-to: ~60 days.
pub const LP_PERSISTENT_EXTEND_TO: u32 = 1_036_800;

// ─────────────────────────────────────────────
// Minimum-reserve / dust policy (issue #393)
// ─────────────────────────────────────────────

/// Minimum reserve floor (in stroops) that must remain in the pool after
/// any `mint` / `burn` / `swap`.
///
/// Chosen to match `MINIMUM_LIQUIDITY = 1_000`: any operation that would
/// leave a reserve below this, or that moves less than this, is rejected
/// with a typed dust error. This prevents dozens of 1-stroop ops from
/// fragmenting state and increasing storage churn.
pub const MINIMUM_RESERVE: i128 = 1_000;

/// Minimum LP tokens that must be minted to the user on any mint path.
/// Mirrors `MINIMUM_LIQUIDITY` so dust deposits cannot create dust LP
/// positions.
pub const MIN_LIQUIDITY_FOR_USER: i128 = 1_000;

// ─────────────────────────────────────────────
// LP metadata defaults (issue #392)
// ─────────────────────────────────────────────

/// Default LP-token decimals. Matches SAC Stellar-asset precision (7) so
/// wallets render LP balances consistently.
pub const LP_DECIMALS: u32 = 7;

/// Default LP-token name.
pub const LP_NAME: &str = "Coral LP";

/// Default LP-token symbol.
pub const LP_SYMBOL: &str = "CLP";

// ─────────────────────────────────────────────
// Router commit-reveal bounds (issue #389)
// ─────────────────────────────────────────────

/// Default cap on the number of simultaneously live commits tracked by the
/// router. Bounds instance-storage growth from distinct committers.
pub const DEFAULT_MAX_COMMITS: u32 = 32;

/// Default commit expiry in ledgers (~1.4 hours at 5s/ledger).
/// A commit older than `commit_ledger + expiry` can no longer be revealed
/// and may be overwritten/pruned.
pub const DEFAULT_COMMIT_EXPIRY_LEDGERS: u32 = 1_000;

/// Hard cap for admin-configured `max_commits` (prevents unbounded config).
pub const MAX_COMMITS_HARD_CAP: u32 = 1_000;

/// Hard cap for admin-configured commit expiry (~30 days).
pub const COMMIT_EXPIRY_HARD_CAP: u32 = 518_400;
