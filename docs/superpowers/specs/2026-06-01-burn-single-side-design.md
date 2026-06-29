# burn_single_side() Design

**Date:** 2026-06-01
**Issue:** CoralSwap-Finance/coralswap-core#137
**Contract:** `contracts/pair/src/lib.rs` — `Pair` struct

---

## Problem

`Pair::burn()` returns both tokens proportionally. An LP who wants to exit into a single token must burn, then swap — paying fees twice and incurring additional slippage. `burn_single_side()` combines both operations atomically.

---

## Function Signature

```rust
pub fn burn_single_side(
    env: Env,
    to: Address,
    lp_amount: i128,
    preferred_token: Address,
    min_amount_out: i128,
) -> Result<i128, PairError>
```

| Parameter | Description |
|---|---|
| `to` | Recipient of LP burn authorization and output tokens |
| `lp_amount` | Number of LP tokens to burn (pulled from `to`) |
| `preferred_token` | The token address the caller wants to receive (`token_a` or `token_b`) |
| `min_amount_out` | Minimum total output accepted; reverts if not met (slippage protection) |

Returns `i128` — the total amount of `preferred_token` sent to `to`.

---

## Algorithm

The operation is fully atomic — a single state write at the end.

1. `to.require_auth()` + acquire reentrancy guard
2. Load `state` (PairStorage) and `fee_state` (FeeState)
3. **Validate inputs:**
   - `lp_amount > 0` → `PairError::InvalidInput`
   - `preferred_token` must be `state.token_a` or `state.token_b` → `PairError::InvalidInput`
   - `min_amount_out > 0` → `PairError::InvalidInput`
4. Read `total_supply` from LP token contract **before** burning
5. Burn `lp_amount` LP tokens from `to` via `LpTokenClient::burn(&to, &lp_amount)`
6. Compute proportional shares:
   ```
   share_a = lp_amount * reserve_a / total_supply
   share_b = lp_amount * reserve_b / total_supply
   ```
   Revert with `InsufficientLiquidityBurned` if either share is 0.
7. Determine swap direction:
   - `preferred = token_a` → unwanted token is B; swap `share_b` B→A
   - `preferred = token_b` → unwanted token is A; swap `share_a` A→B
8. Compute **post-burn** reserves for the internal swap:
   ```
   reserve_in_post  = reserve_unwanted - share_unwanted
   reserve_out_post = reserve_preferred - share_preferred
   ```
9. Apply Uniswap V2 `get_amount_out` formula with dynamic `fee_bps`:
   ```
   swap_out = (share_unwanted * (10000 - fee_bps) * reserve_out_post)
            / (reserve_in_post * 10000 + share_unwanted * (10000 - fee_bps))
   ```
10. `total_out = share_preferred + swap_out`
11. **Slippage check:** revert with `InsufficientOutputAmount` if `total_out < min_amount_out`
12. **K invariant check** on the final post-operation reserves:
    ```
    new_reserve_preferred = reserve_out_post - swap_out
    new_reserve_unwanted  = reserve_in_post + share_unwanted   (nets to reserve_unwanted unchanged)
    ```
    The fee-adjusted K must be ≥ pre-operation K (same adjusted-balance check used in `swap_inner`).
13. Transfer `total_out` of `preferred_token` to `to`
14. Write updated state: reserves, `k_last`, `block_timestamp_last`
15. Emit `PairEvents::burn_single_side` event
16. Return `total_out`

---

## Reserve Accounting Detail

After proportional burn + internal swap the unwanted-token reserve is net-unchanged:

```
reserve_unwanted_final = (reserve_unwanted - share_unwanted) + share_unwanted
                       = reserve_unwanted
```

The preferred-token reserve decreases by both the burned share and the swap output:

```
reserve_preferred_final = reserve_preferred - share_preferred - swap_out
```

---

## K Invariant Verification

Reuse the fee-adjusted K check from `swap_inner`, applied to the **swap leg** only (post-burn reserves are the baseline):

```rust
// Baseline: post-burn reserves before the internal swap step
let reserve_preferred_post_burn = reserve_preferred - share_preferred;
let reserve_unwanted_post_burn  = reserve_unwanted  - share_unwanted;

let k_before = reserve_preferred_post_burn
    .checked_mul(reserve_unwanted_post_burn)
    .ok_or(PairError::Overflow)?
    .checked_mul(100_000_000)
    .ok_or(PairError::Overflow)?;

// amount_preferred_in = 0 (preferred only exits), amount_unwanted_in = share_unwanted
let balance_preferred_adj = reserve_preferred_final
    .checked_mul(10_000)
    .ok_or(PairError::Overflow)?;   // - 0 * fee_bps

let balance_unwanted_adj = reserve_unwanted_final
    .checked_mul(10_000)
    .ok_or(PairError::Overflow)?
    .checked_sub(share_unwanted * fee_bps as i128)
    .ok_or(PairError::Overflow)?;

let k_after = balance_preferred_adj.checked_mul(balance_unwanted_adj).ok_or(PairError::Overflow)?;

if k_after < k_before { return Err(PairError::InvalidK); }
```

---

## New Event

```rust
// PairEvents::burn_single_side
// Topics: ("burn_ss", to)
// Data:   (lp_amount, preferred_token, total_out)
pub fn burn_single_side(env: &Env, to: &Address, lp_amount: i128, preferred_token: &Address, total_out: i128)
```

`"burn_ss"` fits the 9-character `symbol_short!` limit.

---

## New Error

Add `SlippageExceeded = 119` to `PairError` — more descriptive than reusing `InsufficientOutputAmount` for the `min_amount_out` check.

---

## Tests

New file: `contracts/pair/src/test/burn.rs`

| Test | What it verifies |
|---|---|
| `test_burn_single_side_exit_token_a` | Exit into token_a returns correct amount |
| `test_burn_single_side_exit_token_b` | Exit into token_b returns correct amount |
| `test_burn_single_side_k_invariant` | K invariant holds post-operation |
| `test_burn_single_side_slippage_reverts` | Reverts when `total_out < min_amount_out` |
| `test_burn_single_side_lp_supply_decreases` | LP total_supply decreases by exactly `lp_amount` |

The test file follows the same `setup_pair()` helper pattern used in `test/mint.rs`.

---

## Files Changed

| File | Change |
|---|---|
| `contracts/pair/src/lib.rs` | Add `burn_single_side()` method |
| `contracts/pair/src/errors.rs` | Add `SlippageExceeded = 119` |
| `contracts/pair/src/events.rs` | Add `PairEvents::burn_single_side()` |
| `contracts/pair/src/test/mod.rs` | Add `mod burn;` |
| `contracts/pair/src/test/burn.rs` | New — 5 tests |
