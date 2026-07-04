//! Integration test for the full reward distribution lifecycle — Issue #239.
//!
//! STATUS: BLOCKED. This test cannot be implemented yet because the
//! `RewardDistributor` contract referenced in #239 does not exist anywhere
//! in this repository (checked `main`, all branches, and open PRs #267/#268
//! as of 2026-07-02). PR #267 adds `RewardEvent` emissions to `contracts/pair`
//! but does not add a staking/distributor contract.
//!
//! Intended flow once the contract exists:
//!   1. User provides liquidity (via Router -> Pair, mints LP tokens)
//!   2. User stakes LP tokens into RewardDistributor
//!   3. Time passes
//!   4. User claims rewards
//!   5. Admin changes reward rate
//!   6. Time passes
//!   7. User unstakes and claims final rewards
//!
//! Acceptance criteria to cover once unblocked:
//!   - Simulates a realistic liquidity mining campaign
//!   - Verifies exact user token balances at the end
//!   - Proves no funds are locked or leaked
//!
//! See issue #239 for full context.

#[test]
#[ignore = "blocked on RewardDistributor contract, which does not exist yet — see #239"]
fn test_full_reward_distribution_lifecycle() {
    todo!("implement once contracts/reward_distributor exists");
}
