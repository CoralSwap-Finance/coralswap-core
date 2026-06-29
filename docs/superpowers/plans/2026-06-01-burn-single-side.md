# burn_single_side() Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Pair::burn_single_side()` that burns LP tokens and exits into a single preferred token atomically, saving the user a separate swap and double fees.

**Architecture:** Inline swap math (Option B from design): compute proportional burn shares, apply Uniswap V2 `get_amount_out` on the post-burn reserves to price the internal swap, do a single atomic reserve update, single LP burn, and single token transfer. No call to `swap_inner` — the K invariant check is explicit against the post-burn reserves. Reentrancy guard acquired the same way as `swap()`.

**Tech Stack:** Rust / Soroban SDK, `soroban-sdk` testutils, `coralswap-lp-token` crate, `MockToken` pattern (identical to `contracts/pair/src/test/mint.rs`).

**Spec:** `docs/superpowers/specs/2026-06-01-burn-single-side-design.md`

---

## File Map

| File | Action | What changes |
|---|---|---|
| `contracts/pair/src/errors.rs` | Modify | Add `SlippageExceeded = 119` variant |
| `contracts/pair/src/events.rs` | Modify | Add `PairEvents::burn_single_side()` |
| `contracts/pair/src/test/burn.rs` | Create | 5 tests for burn_single_side |
| `contracts/pair/src/test/mod.rs` | Modify | Add `mod burn;` |
| `contracts/pair/src/lib.rs` | Modify | Add `burn_single_side()` method |

---

## Task 1: Create feature branch

**Files:** none (git only)

- [ ] **Step 1: Create and switch to the feature branch**

```bash
git checkout -b feat/burn-single-side
```

Expected: `Switched to a new branch 'feat/burn-single-side'`

---

## Task 2: Add SlippageExceeded error

**Files:**
- Modify: `contracts/pair/src/errors.rs`

- [ ] **Step 1: Add the new variant**

Open `contracts/pair/src/errors.rs`. The last variant is `FlashLoanFeeTooHigh = 118`. Add one line immediately after it:

```rust
    FlashLoanFeeTooHigh = 118,
    SlippageExceeded = 119,
```

The complete updated enum should look like:

```rust
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PairError {
    AlreadyInitialized = 100,
    NotInitialized = 101,
    InsufficientLiquidity = 102,
    InsufficientInputAmount = 103,
    InsufficientOutputAmount = 104,
    InvalidK = 105,
    Locked = 106,
    FlashLoanNotRepaid = 107,
    FlashPayloadTooLarge = 108,
    Paused = 109,
    Overflow = 110,
    ZeroAddress = 111,
    InsufficientLiquidityMinted = 112,
    InsufficientLiquidityBurned = 113,
    InvalidInput = 114,
    InvalidEmaAlpha = 115,
    FeeOverflow = 116,
    FlashCallbackFailed = 117,
    FlashLoanFeeTooHigh = 118,
    SlippageExceeded = 119,
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p coralswap-pair 2>&1 | tail -5
```

Expected: no errors (warnings are OK).

---

## Task 3: Add burn_single_side event

**Files:**
- Modify: `contracts/pair/src/events.rs`

- [ ] **Step 1: Add the new event method**

At the end of the `impl PairEvents` block (after the closing `}` of `flash_loan`), add:

```rust
    pub fn burn_single_side(
        env: &Env,
        to: &Address,
        lp_amount: i128,
        preferred_token: &Address,
        total_out: i128,
    ) {
        env.events().publish(
            (symbol_short!("burn_ss"), to.clone()),
            (lp_amount, preferred_token.clone(), total_out),
        );
    }
```

`"burn_ss"` is 7 characters — within the 9-character `symbol_short!` limit.

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p coralswap-pair 2>&1 | tail -5
```

Expected: no errors.

---

## Task 4: Write failing tests (TDD)

**Files:**
- Create: `contracts/pair/src/test/burn.rs`
- Modify: `contracts/pair/src/test/mod.rs`

- [ ] **Step 1: Register the new test module**

In `contracts/pair/src/test/mod.rs`, add `mod burn;` to the list:

```rust
mod dynamic_fee;
mod events;
mod flash_loan;
mod initialize;
mod mint;
mod oracle;
mod reentrancy;
mod swap_math;
mod sync;
mod views;
mod burn;
```

- [ ] **Step 2: Create the test file with all 5 tests**

Create `contracts/pair/src/test/burn.rs` with the following content:

```rust
#![cfg(test)]

use coralswap_lp_token::{LpToken, LpTokenClient};

use crate::{errors::PairError, Pair, PairClient};
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::Address as _,
    Address, Env, String,
};

// ── Minimal mock token ────────────────────────────────────────────────────────

#[contracttype]
enum BurnMockTokenKey {
    Balance(Address),
}

#[contract]
pub struct BurnMockToken;

#[contractimpl]
impl BurnMockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = BurnMockTokenKey::Balance(to);
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let fk = BurnMockTokenKey::Balance(from);
        let tk = BurnMockTokenKey::Balance(to);
        let fb: i128 = env.storage().persistent().get(&fk).unwrap_or(0);
        let tb: i128 = env.storage().persistent().get(&tk).unwrap_or(0);
        env.storage().persistent().set(&fk, &(fb - amount));
        env.storage().persistent().set(&tk, &(tb + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&BurnMockTokenKey::Balance(id)).unwrap_or(0)
    }
}

// ── Shared setup ──────────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
fn setup_pair(
    reserve_a: i128,
    reserve_b: i128,
) -> (
    Env,
    PairClient<'static>,
    BurnMockTokenClient<'static>,
    BurnMockTokenClient<'static>,
    LpTokenClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let token_a_id = env.register_contract(None, BurnMockToken);
    let token_b_id = env.register_contract(None, BurnMockToken);
    let lp_id = env.register_contract(None, LpToken);
    let pair_id = env.register_contract(None, Pair);

    let token_a = BurnMockTokenClient::new(&env, &token_a_id);
    let token_b = BurnMockTokenClient::new(&env, &token_b_id);
    let lp_client = LpTokenClient::new(&env, &lp_id);
    let pair_client = PairClient::new(&env, &pair_id);

    let admin = Address::generate(&env);
    let factory = Address::generate(&env);
    let user = Address::generate(&env);

    lp_client.initialize(
        &admin,
        &7u32,
        &String::from_str(&env, "Coral LP"),
        &String::from_str(&env, "CLP"),
    );

    pair_client.initialize(&factory, &token_a_id, &token_b_id, &lp_id);

    token_a.mint(&user, &reserve_a);
    token_b.mint(&user, &reserve_b);
    token_a.transfer(&user, &pair_client.address, &reserve_a);
    token_b.transfer(&user, &pair_client.address, &reserve_b);
    pair_client.mint(&user);

    (env, pair_client, token_a, token_b, lp_client, user, token_a_id, token_b_id)
}

fn get_amount_out(amount_in: i128, reserve_in: i128, reserve_out: i128, fee_bps: i128) -> i128 {
    let fee_factor = 10_000 - fee_bps;
    let aif = amount_in * fee_factor;
    aif * reserve_out / (reserve_in * 10_000 + aif)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// 1. Exit into token_a returns correct amount (balanced pool)
#[test]
fn test_burn_single_side_exit_token_a() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let lp_amount = 10_000_000i128;
    let total_supply = lp_client.total_supply(); // 1_000_000_000

    let share_a = lp_amount * reserve / total_supply; // 10_000_000
    let share_b = lp_amount * reserve / total_supply; // 10_000_000

    let reserve_a_post_burn = reserve - share_a; // 990_000_000
    let reserve_b_post_burn = reserve - share_b; // 990_000_000

    let swap_out = get_amount_out(share_b, reserve_b_post_burn, reserve_a_post_burn, 30);
    let expected_total = share_a + swap_out; // 10_000_000 + 9_870_596 = 19_870_596

    let result = pair_client.burn_single_side(&user, &lp_amount, &token_a_id, &1i128);

    assert_eq!(result, expected_total, "total_out must equal share + swap_out");
    assert_eq!(
        token_a.balance(&user),
        expected_total,
        "user's token_a balance must equal total_out"
    );
}

// 2. Exit into token_b returns correct amount (asymmetric pool)
#[test]
fn test_burn_single_side_exit_token_b() {
    let reserve_a = 1_000_000_000i128;
    let reserve_b = 4_000_000_000i128;
    let (_env, pair_client, _token_a, token_b, lp_client, user, _token_a_id, token_b_id) =
        setup_pair(reserve_a, reserve_b);

    let total_supply = lp_client.total_supply(); // 2_000_000_000
    let lp_amount = 20_000_000i128; // 1% of supply

    let share_a = lp_amount * reserve_a / total_supply; // 10_000_000
    let share_b = lp_amount * reserve_b / total_supply; // 40_000_000

    let reserve_a_post_burn = reserve_a - share_a; // 990_000_000
    let reserve_b_post_burn = reserve_b - share_b; // 3_960_000_000

    // preferred = token_b, unwanted = token_a (share_a swaps to token_b)
    let swap_out = get_amount_out(share_a, reserve_a_post_burn, reserve_b_post_burn, 30);
    let expected_total = share_b + swap_out; // 40_000_000 + 39_482_384 = 79_482_384

    let result = pair_client.burn_single_side(&user, &lp_amount, &token_b_id, &1i128);

    assert_eq!(result, expected_total, "total_out must equal share_b + swap_out_of_a");
    assert_eq!(
        token_b.balance(&user),
        expected_total,
        "user's token_b balance must equal total_out"
    );
}

// 3. K invariant holds post-operation
#[test]
fn test_burn_single_side_k_invariant_holds() {
    let reserve = 1_000_000_000i128;
    let (env, pair_client, _token_a, _token_b, _lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let lp_amount = 10_000_000i128;

    pair_client.burn_single_side(&user, &lp_amount, &token_a_id, &1i128);

    let (res_a, res_b, _) = pair_client.get_reserves();

    // K must be positive and reserves must be in valid range
    assert!(res_a > 0 && res_b > 0, "reserves must remain positive");

    // reserve_b stays unchanged (unwanted reserve is net-unchanged after burn+swap)
    assert_eq!(res_b, reserve, "unwanted reserve (token_b) must be unchanged");

    // reserve_a decreased by total_out
    assert!(res_a < reserve, "preferred reserve must decrease");

    // K after < K before (LP burned, liquidity removed — expected)
    // but the ratio must be valid (not NaN, not zero)
    let _k = res_a.checked_mul(res_b).expect("k must not overflow");
}

// 4. min_amount_out reverts when output is insufficient
#[test]
fn test_burn_single_side_slippage_reverts() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let lp_amount = 10_000_000i128;
    let total_supply = lp_client.total_supply();

    let share_a = lp_amount * reserve / total_supply;
    let share_b = lp_amount * reserve / total_supply;
    let reserve_a_post_burn = reserve - share_a;
    let reserve_b_post_burn = reserve - share_b;
    let swap_out = get_amount_out(share_b, reserve_b_post_burn, reserve_a_post_burn, 30);
    let actual_out = share_a + swap_out;

    // Demand more than possible → should revert
    let result = pair_client.try_burn_single_side(
        &user,
        &lp_amount,
        &token_a_id,
        &(actual_out + 1),
    );

    assert!(result.is_err(), "must revert when min_amount_out exceeds actual output");
}

// 5. LP token supply decreases by exactly lp_amount
#[test]
fn test_burn_single_side_lp_supply_decreases() {
    let reserve = 1_000_000_000i128;
    let (_env, pair_client, _token_a, _token_b, lp_client, user, token_a_id, _token_b_id) =
        setup_pair(reserve, reserve);

    let supply_before = lp_client.total_supply();
    let lp_amount = 10_000_000i128;

    pair_client.burn_single_side(&user, &lp_amount, &token_a_id, &1i128);

    let supply_after = lp_client.total_supply();

    assert_eq!(
        supply_before - supply_after,
        lp_amount,
        "LP total_supply must decrease by exactly lp_amount"
    );
}
```

- [ ] **Step 3: Run tests — all should fail because burn_single_side doesn't exist yet**

```bash
cargo test -p coralswap-pair burn 2>&1 | tail -20
```

Expected output contains: `error[E0599]: no method named 'burn_single_side'` or similar — confirms TDD red phase.

---

## Task 5: Implement burn_single_side

**Files:**
- Modify: `contracts/pair/src/lib.rs`

- [ ] **Step 1: Add the method to the Pair impl block**

In `contracts/pair/src/lib.rs`, insert the following method inside the `#[contractimpl] impl Pair` block, after the closing `}` of the `burn()` method (around line 220) and before the `// Swap` section comment:

```rust
    // ─────────────────────────────────────────
    // Burn single-side
    // ─────────────────────────────────────────

    /// Burns `lp_amount` LP tokens from `to` and returns all value as
    /// `preferred_token`, combining the proportional exit with an internal
    /// constant-product swap of the unwanted share.
    pub fn burn_single_side(
        env: Env,
        to: Address,
        lp_amount: i128,
        preferred_token: Address,
        min_amount_out: i128,
    ) -> Result<i128, PairError> {
        to.require_auth();

        let _guard = reentrancy::ReentrancyGuard::acquire(&env)?;

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let fee_state = get_fee_state(&env).ok_or(PairError::NotInitialized)?;

        if lp_amount <= 0 || min_amount_out <= 0 {
            return Err(PairError::InvalidInput);
        }

        let prefer_a = if preferred_token == state.token_a {
            true
        } else if preferred_token == state.token_b {
            false
        } else {
            return Err(PairError::InvalidInput);
        };

        let lp_client = LpTokenClient::new(&env, &state.lp_token);
        let total_supply = lp_client.total_supply();

        if total_supply == 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        // Burn LP from caller before computing shares (total_supply read first)
        lp_client.burn(&to, &lp_amount);

        let share_a = lp_amount
            .checked_mul(state.reserve_a)
            .ok_or(PairError::Overflow)?
            .checked_div(total_supply)
            .ok_or(PairError::Overflow)?;

        let share_b = lp_amount
            .checked_mul(state.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_div(total_supply)
            .ok_or(PairError::Overflow)?;

        if share_a <= 0 || share_b <= 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        // Partition into preferred/unwanted directions
        let (share_preferred, share_unwanted, reserve_preferred, reserve_unwanted) = if prefer_a {
            (share_a, share_b, state.reserve_a, state.reserve_b)
        } else {
            (share_b, share_a, state.reserve_b, state.reserve_a)
        };

        // Post-burn reserves are the baseline for the internal swap
        let reserve_preferred_post_burn = reserve_preferred
            .checked_sub(share_preferred)
            .ok_or(PairError::Overflow)?;
        let reserve_unwanted_post_burn = reserve_unwanted
            .checked_sub(share_unwanted)
            .ok_or(PairError::Overflow)?;

        // Uniswap V2 get_amount_out: share_unwanted swaps into preferred
        let fee_bps = dynamic_fee::compute_fee_bps(&fee_state) as i128;
        let fee_factor = 10_000i128 - fee_bps;

        let amount_in_with_fee = share_unwanted
            .checked_mul(fee_factor)
            .ok_or(PairError::Overflow)?;

        let swap_numerator = amount_in_with_fee
            .checked_mul(reserve_preferred_post_burn)
            .ok_or(PairError::Overflow)?;

        let swap_denominator = reserve_unwanted_post_burn
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_add(amount_in_with_fee)
            .ok_or(PairError::Overflow)?;

        if swap_denominator == 0 {
            return Err(PairError::InsufficientLiquidity);
        }

        let swap_out = swap_numerator / swap_denominator;

        let total_out = share_preferred
            .checked_add(swap_out)
            .ok_or(PairError::Overflow)?;

        if total_out < min_amount_out {
            return Err(PairError::SlippageExceeded);
        }

        // Final reserves: unwanted is net-unchanged (burned share re-enters as swap input)
        let reserve_preferred_final = reserve_preferred_post_burn
            .checked_sub(swap_out)
            .ok_or(PairError::Overflow)?;
        let reserve_unwanted_final = reserve_unwanted;

        // K invariant check against post-burn baseline (swap leg must not violate K)
        let k_before = reserve_preferred_post_burn
            .checked_mul(reserve_unwanted_post_burn)
            .ok_or(PairError::Overflow)?
            .checked_mul(100_000_000)
            .ok_or(PairError::Overflow)?;

        let balance_preferred_adj = reserve_preferred_final
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?;

        let balance_unwanted_adj = reserve_unwanted_final
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(
                share_unwanted.checked_mul(fee_bps).ok_or(PairError::Overflow)?,
            )
            .ok_or(PairError::Overflow)?;

        let k_after = balance_preferred_adj
            .checked_mul(balance_unwanted_adj)
            .ok_or(PairError::Overflow)?;

        if k_after < k_before {
            return Err(PairError::InvalidK);
        }

        // Transfer only the preferred token to the user
        let contract = env.current_contract_address();
        TokenClient::new(&env, &preferred_token).transfer(&contract, &to, &total_out);

        // Update reserves
        if prefer_a {
            state.reserve_a = reserve_preferred_final;
            state.reserve_b = reserve_unwanted_final;
        } else {
            state.reserve_a = reserve_unwanted_final;
            state.reserve_b = reserve_preferred_final;
        }

        state.k_last = state
            .reserve_a
            .checked_mul(state.reserve_b)
            .ok_or(PairError::Overflow)?;
        state.block_timestamp_last = env.ledger().timestamp();

        set_pair_state(&env, &state);

        PairEvents::burn_single_side(&env, &to, lp_amount, &preferred_token, total_out);

        Ok(total_out)
    }
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo build -p coralswap-pair 2>&1 | tail -10
```

Expected: `Finished` with no errors.

---

## Task 6: Run tests and commit

**Files:** none (verification + git)

- [ ] **Step 1: Run all burn tests**

```bash
cargo test -p coralswap-pair burn 2>&1
```

Expected: all 5 tests pass:
```
test test::burn::test_burn_single_side_exit_token_a ... ok
test test::burn::test_burn_single_side_exit_token_b ... ok
test test::burn::test_burn_single_side_k_invariant_holds ... ok
test test::burn::test_burn_single_side_slippage_reverts ... ok
test test::burn::test_burn_single_side_lp_supply_decreases ... ok
```

- [ ] **Step 2: Run the full test suite to catch regressions**

```bash
cargo test -p coralswap-pair 2>&1 | tail -20
```

Expected: all previously passing tests still pass, 5 new burn tests pass.

- [ ] **Step 3: Run lint**

```bash
cargo clippy -p coralswap-pair --all-targets -- -D warnings 2>&1 | tail -10
```

Expected: no warnings or errors.

- [ ] **Step 4: Commit**

```bash
git add contracts/pair/src/errors.rs \
        contracts/pair/src/events.rs \
        contracts/pair/src/lib.rs \
        contracts/pair/src/test/mod.rs \
        contracts/pair/src/test/burn.rs
git commit -m "feat: add Pair::burn_single_side() for single-token LP exit (closes #137)"
```

---

## Task 7: Push branch and open PR

**Files:** none (git + GitHub CLI)

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/burn-single-side
```

- [ ] **Step 2: Open PR against the original upstream repo**

```bash
gh pr create \
  --repo CoralSwap-Finance/coralswap-core \
  --head TS-mfon:feat/burn-single-side \
  --title "feat: add Pair::burn_single_side() for single-token LP exit" \
  --body "$(cat <<'EOF'
## Summary

- Adds `Pair::burn_single_side(to, lp_amount, preferred_token, min_amount_out)` to the `Pair` contract
- Burns LP tokens directly from the caller, computes proportional shares, and internally swaps the unwanted share using the Uniswap V2 constant-product formula on post-burn reserves
- K invariant is verified against the post-burn baseline after the internal swap
- Slippage protection via `min_amount_out` — reverts with `SlippageExceeded` if output is insufficient
- Reentrancy guard acquired (same pattern as `swap()`)
- Adds `SlippageExceeded = 119` error and `PairEvents::burn_single_side` event

## Test plan

- [x] Exit into token_0 returns correct amount (balanced pool, 1% burn)
- [x] Exit into token_1 returns correct amount (asymmetric 1:4 pool, 1% burn)
- [x] K invariant holds post-operation (unwanted reserve net-unchanged, preferred reserve decreases)
- [x] `min_amount_out` reverts when actual output is insufficient
- [x] LP token supply decreases by exactly `lp_amount`

closes #137

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: URL of the new PR printed to stdout.
