# Security Policy

CoralSwap core contracts secure AMM, factory, pair, LP token, router, and
flash-loan receiver behavior. Please report suspected vulnerabilities
privately so maintainers can investigate and coordinate a fix before public
disclosure.

## Scope

Security reports are in scope when they affect assets, protocol integrity, or
availability in this repository, including:

- Soroban smart contracts under `contracts/`
- Shared contract interfaces and deployment configuration
- Build, test, and release files that could affect deployed contract behavior
- Documentation that could cause unsafe deployment or integration choices

Out of scope:

- Issues that require compromised private keys or user devices
- Social engineering, spam, or denial-of-service against public infrastructure
- Vulnerabilities in third-party services unless they directly affect this
  repository's contracts or deployment flow
- Reports without enough reproduction detail to assess impact

## Reporting a Vulnerability

Do not open a public GitHub issue for an unpatched vulnerability.

Send reports to the CoralSwap security team:

- Email: security@coralswap.finance
- If that address is unavailable, contact the maintainers through the GitHub
  organization and request a private security channel.

Include:

- Affected contract, file, function, or deployment step
- Reproduction steps or a proof of concept
- Expected impact and any affected assets
- Suggested mitigation, if known
- Your preferred contact for follow-up

## Response Targets

The team aims to acknowledge new reports within 3 business days.

Typical targets after acknowledgement:

- Triage and severity assessment: 7 business days
- Fix plan for confirmed high or critical issues: 14 business days
- Coordinated disclosure once a fix or mitigation is available

Timelines may vary with severity, exploitability, and deployment status.

## Bounty Notes

If a bug bounty program is active, eligibility, reward amount, and payout method
are determined by the bounty listing or campaign terms. A report is not eligible
when it is public before maintainers have had a reasonable chance to remediate
it, duplicates a known issue, or falls outside the scope above.

## Safe Harbor

Good-faith research that follows this policy, avoids privacy violations, avoids
service disruption, and does not access or move funds without authorization will
be treated as authorized security research by this project.

## Security Invariants

This section documents the critical invariants that CoralSwap contracts must
maintain. Auditors, security researchers, and automated tools should verify that
these properties hold under all conditions.

### 1. Constant Product (K) Invariant

**Invariant**: For any swap operation, the product of reserves after fees must
be greater than or equal to the product before the swap:
`(reserve_a_after * reserve_b_after) >= (reserve_a_before * reserve_b_before)`

**Enforcement**: All swap operations (including single-sided liquidity
operations) compute fee-adjusted balances and verify K monotonicity before
updating reserves.

**Location**: `contracts/pair/src/lib.rs` - functions `swap_inner`,
`mint_with_one_token`, `burn_single_side`

**Tests**:

- `contracts/pair/src/test/mint_single_side.rs` - K invariant validation
- `contracts/pair/src/test/burn.rs` - `test_burn_single_side_k_invariant_holds`

### 2. Reserves Equal Token Balances

**Invariant**: After any liquidity or swap operation completes, the pair's
stored reserves must exactly match the actual token contract balances held by
the pair contract address.

**Enforcement**: Every mint, burn, and swap operation reads actual balances from
the token contracts, performs accounting, then writes updated reserves. The
`sync()` function provides an emergency mechanism to force alignment if
discrepancies occur due to direct token transfers.

**Location**: `contracts/pair/src/lib.rs` - `mint`, `burn`, `swap_inner`,
`sync`

**Tests**:

- All integration tests in `contracts/pair/src/test/` implicitly verify this by
  comparing computed reserves against token contract balances

### 3. MINIMUM_LIQUIDITY Seed Integrity

**Invariant**: The first liquidity deposit locks `MINIMUM_LIQUIDITY` (1,000) LP
tokens permanently in the pair contract. This seed must never be redeemed or
transferred. After the first mint, the pair contract must always hold exactly
`MINIMUM_LIQUIDITY` LP tokens, and all burn operations must exclude this seed
from proportional reserve calculations.

**Enforcement**:

- First mint: `mint()` issues `MINIMUM_LIQUIDITY` to the contract address before
  minting user LP tokens
- All burns: `burn()` and `burn_single_side()` compute redemption amounts using
  `(total_supply - MINIMUM_LIQUIDITY)` as the divisor, ensuring the seed's
  proportional reserves remain locked

**Location**:

- `contracts/pair/src/lib.rs` - `mint`, `burn`, `burn_single_side`
- `contracts/pair/src/math/mod.rs` - `MINIMUM_LIQUIDITY` constant

**Tests**: `contracts/pair/src/test/burn.rs`

- `test_burn_seed_remains_intact_after_full_cycle`
- `test_burn_seed_not_redeemable`
- `test_burn_single_side_seed_remains_intact`
- `test_burn_cannot_extract_seed_reserves`

**Fix**: Issue #289

### 4. No Foreign LP Token Burns

**Invariant**: Only the authorized pair contract (the LP token's admin) may mint
or burn LP tokens. Users cannot directly burn LP tokens that they did not
contribute liquidity to receive.

**Enforcement**: The LP token contract restricts `mint()` and `burn()` via
`require_auth()` checks. Only the admin (set to the pair contract address at
initialization) can mint. Burn requires authorization from the token holder
(`from.require_auth()`).

**Location**: `contracts/lp_token/src/lib.rs` - `mint`, `burn`

**Tests**:

- `contracts/lp_token/src/test/mod.rs` - authorization tests
- `contracts/pair/src/test/burn.rs` - `test_burn_cannot_extract_seed_reserves`

### 5. Persistent Storage TTL Policies

**Invariant**: All persistent storage entries (LP token balances, nonces,
allowances) must have their TTL extended on every write and on critical reads to
prevent data expiration. A user's LP balance or nonce must never expire to zero
due to TTL exhaustion.

**Enforcement**:

- All `write_balance()` calls extend TTL to `TTL_EXTEND_TO` (1,036,800 ledgers /
  ~60 days)
- `balance()` reads extend TTL for non-zero balances
- Nonce writes in `permit()` extend TTL
- Allowance writes in `approve()` and `permit()` extend TTL

**Policy Constants**:

- `TTL_THRESHOLD`: 518,400 ledgers (~30 days at 5s/ledger)
- `TTL_EXTEND_TO`: 1,036,800 ledgers (~60 days at 5s/ledger)

**Location**: `contracts/lp_token/src/lib.rs` - TTL constants, `write_balance`,
`balance`, `permit`, `approve`

**Tests**: `contracts/lp_token/src/test/mod.rs`

- `test_write_balance_extends_ttl`
- `test_balance_read_extends_ttl`
- `test_nonce_write_extends_ttl`
- `test_transfer_extends_ttl_for_both_parties`

**Fix**: Issue #286

### 6. Reentrancy Protection

**Invariant**: State-modifying operations (mint, burn, swap, flash loan) must be
non-reentrant. A contract invoked during a callback (e.g., flash loan receiver)
must not be able to re-enter the pair contract to manipulate reserves or LP
supply mid-operation.

**Enforcement**: All public state-changing functions acquire a `ReentrancyGuard`
at entry, which sets a locked flag in storage. Any attempt to re-enter while the
guard is held reverts with `PairError::Locked`.

**Location**:

- `contracts/pair/src/reentrancy.rs` - `ReentrancyGuard` implementation
- `contracts/pair/src/lib.rs` - All functions acquire guard via
  `let _guard = reentrancy::ReentrancyGuard::acquire(&env)?;`

**Tests**: `contracts/pair/src/test/reentrancy.rs`

- Comprehensive reentrancy attack scenarios for mint, burn, swap, flash loan

### 7. Flash Loan Repayment Validation

**Invariant**: Flash loan borrows must be fully repaid (principal + fee) within
the same transaction. After the borrower's callback completes, the pair's token
balances must have increased by at least the fee amount, and the K invariant
must hold.

**Enforcement**: `flash_loan()` snapshots reserves, transfers borrowed tokens,
invokes the receiver's callback, then validates that actual token balances
increased by the required fee. The operation reverts if repayment is
insufficient.

**Location**: `contracts/pair/src/flash_loan.rs` - `execute_flash_loan`

**Tests**: `contracts/pair/src/test/flash_loan.rs`

- `test_flash_loan_repayment_enforced`
- `test_flash_loan_insufficient_repayment_reverts`
- Malicious receiver scenarios

### 8. Oracle TWAP Integrity

**Invariant**: The time-weighted average price (TWAP) oracle accumulates price
observations on every reserve-changing operation. The cumulative price values
must monotonically increase (or stay constant in edge cases) and never overflow
or reset unexpectedly.

**Enforcement**: Each swap, mint, or burn updates `price_a_cumulative` and
`price_b_cumulative` by adding the current price multiplied by elapsed time
since the last update. Accumulators use checked arithmetic to detect overflow.

**Location**: `contracts/pair/src/oracle/mod.rs` - price accumulation logic
(called from reserve-updating operations)

**Tests**: TWAP oracle tests verify accumulator monotonicity and correct TWAP
calculation over multi-block windows (test locations TBD based on oracle
implementation)

### 9. Dynamic Fee Bounds

**Invariant**: The dynamic fee mechanism must constrain fees to the range
`[min_fee_bps, max_fee_bps]` as configured in `FeeState`. Fees must never
exceed 100 basis points (1%) to prevent value extraction. Per-pair fee overrides
set by governance must also respect the 100 bps cap.

**Enforcement**:

- `compute_fee_bps()` clamps computed fees to `[min_fee_bps, max_fee_bps]`
- Factory's `set_pair_fee()` rejects overrides > 100 bps
- Swap operations query the factory for overrides and fall back to dynamic fees

**Location**:

- `contracts/pair/src/dynamic_fee.rs` - `compute_fee_bps`
- `contracts/factory/src/lib.rs` - `set_pair_fee` validation

**Tests**:

- `contracts/factory/src/test/mod.rs` - `test_set_pair_fee_above_cap_reverts_with_fee_too_high`
- Dynamic fee tests verify bounds (test locations TBD)

### 10. Pause State Isolation

**Invariant**: When the factory is paused, no new pairs can be created, but
existing pairs remain fully operational. Pair contracts do not inherit the
factory's paused state — they operate independently once deployed.

**Enforcement**: `create_pair()` checks `factory_storage.paused` and reverts if
true. Pair contracts have no pause mechanism and do not query the factory's
pause state during normal operations.

**Location**: `contracts/factory/src/lib.rs` - `create_pair`, `pause`, `unpause`

**Tests**: `contracts/factory/src/test/mod.rs`

- `test_create_pair_while_paused`
- `test_create_pair_after_unpause`

---

**Note**: This list is not exhaustive. Additional invariants (e.g., governance
multisig thresholds, upgrade timelock enforcement) are documented in their
respective modules. Security researchers should also review issue-specific fixes
tagged with `security` and `GrantFox OSS` labels for context on past
vulnerabilities and mitigations.
