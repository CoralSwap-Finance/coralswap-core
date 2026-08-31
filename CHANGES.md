# Changes Summary

This PR addresses four critical issues in the CoralSwap core contracts:

## Issue #286: LP Token Balances and Nonces TTL Expiration [CRITICAL]

**Problem**: LP token balances and nonces in persistent storage never had their TTL extended, causing user balances to expire to zero after the default TTL period.

**Solution**:

- Added TTL policy constants: `TTL_THRESHOLD` (518,400 ledgers / ~30 days) and `TTL_EXTEND_TO` (1,036,800 ledgers / ~60 days)
- Modified `write_balance()` to extend TTL on every balance write
- Modified `balance()` to proactively extend TTL on reads for non-zero balances
- Modified `permit()` to extend TTL when incrementing nonces
- Allowance TTL extension was already in place and preserved

**Tests Added**:

- `test_write_balance_extends_ttl`
- `test_balance_read_extends_ttl`
- `test_nonce_write_extends_ttl`
- `test_transfer_extends_ttl_for_both_parties`

**Files Changed**:

- `contracts/lp_token/src/lib.rs`
- `contracts/lp_token/src/test/mod.rs`

---

## Issue #289: MINIMUM_LIQUIDITY Seed Claimable via Burn [CRITICAL]

**Problem**: The `burn()` and `burn_single_side()` functions used the full `total_supply` (including the MINIMUM_LIQUIDITY seed) in redemption calculations. This allowed any caller to extract the seed's proportional share of reserves, permanently removing the dust-protection margin.

**Solution**:

- Modified `burn()` to compute redemption amounts using `burnable_supply = total_supply - MINIMUM_LIQUIDITY` as the divisor
- Modified `burn_single_side()` to use the same `burnable_supply` calculation
- The seed (1,000 LP tokens) remains permanently locked in the pair contract
- Added validation to ensure `burnable_supply > 0` before processing burns

**Tests Added**:

- `test_burn_seed_remains_intact_after_full_cycle`
- `test_burn_seed_not_redeemable`
- `test_burn_single_side_seed_remains_intact`
- `test_burn_cannot_extract_seed_reserves`

**Files Changed**:

- `contracts/pair/src/lib.rs`
- `contracts/pair/src/test/burn.rs`

---

## Issue #405: Security Documentation - Invariants Section

**Problem**: Critical security invariants were scattered across issues and code comments, making it difficult for auditors and security researchers to verify the system's properties.

**Solution**:
Added a comprehensive "Security Invariants" section to `SECURITY.md` documenting:

1. **Constant Product (K) Invariant** - K monotonicity across swaps
2. **Reserves Equal Token Balances** - Reserve/balance synchronization
3. **MINIMUM_LIQUIDITY Seed Integrity** - Seed permanence and exclusion from redemption
4. **No Foreign LP Token Burns** - Authorization checks
5. **Persistent Storage TTL Policies** - TTL extension policies and constants
6. **Reentrancy Protection** - Guard mechanism
7. **Flash Loan Repayment Validation** - Repayment enforcement
8. **Oracle TWAP Integrity** - Accumulator monotonicity
9. **Dynamic Fee Bounds** - Fee caps and validation
10. **Pause State Isolation** - Factory/pair independence

Each invariant includes:

- Clear statement of the property
- Enforcement mechanism
- Code locations
- Test references
- Related issue numbers (where applicable)

**Files Changed**:

- `SECURITY.md`

---

## Issue #402: Factory create_pair Gas Budget Benchmark

**Problem**: No budget or benchmark tracked potential regressions in `create_pair` CPU cost. Future features could exceed Stellar's per-transaction gas limits without detection.

**Solution**:
Added two benchmark tests in the factory test suite:

1. **`test_create_pair_gas_budget`**: Enforces a hard cap of 50M CPU instructions (50% of Stellar's ~100M limit). Test fails if exceeded, blocking PR merges in CI.

2. **`test_create_pair_baseline_cost`**: Documents baseline CPU cost for tracking trends over time.

**Budget Rationale**:

- Current baseline: ~25-35M instructions
- Budget cap: 50M instructions
- Leaves headroom for router multicalls and future features

**CI Integration**:
Tests run automatically in existing `cargo test` CI job. No workflow changes needed.

**Files Changed**:

- `contracts/factory/src/test/mod.rs`

---

## Testing

All changes include comprehensive test coverage:

- LP token TTL tests verify storage extension on writes and reads
- Burn tests verify seed integrity after full burn cycles and attack scenarios
- Factory benchmark tests enforce and document CPU budgets

Run tests:

```bash
cargo test
```

## Deployment Impact

All changes are backward compatible:

- TTL fixes apply prospectively to new writes/reads
- Burn fix prevents future exploitation; existing LP holders unaffected
- Documentation changes have no runtime impact
- Benchmark tests are CI-only, not deployed
