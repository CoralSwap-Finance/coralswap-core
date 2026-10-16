# Architecture Update (Issues #358-361)

## Issue #361: Bounded Pair List Growth Model

### Problem
Plain Vec in factory grows with create/delete cycles, no API to prune tombstones or cap growth.

### Solution
**Bounded Pair Count with Governance Prune**

```rust
pub const MAX_ACTIVE_PAIRS: u32 = 10_000;

pub fn can_create_pair(env: &Env) -> bool {
    get_active_pair_count(env) < MAX_ACTIVE_PAIRS
}

pub fn prune_inactive_pairs(env: &Env, pairs: Vec<Address>) -> u32 {
    // Governance-only: Remove inactive pairs (zero liquidity, 90+ days idle)
    // Reduces active_count, allows new pair creation
}
```

**Growth Bound**: Maximum 10,000 active pairs
**Prune Criteria**: Zero liquidity + 90 days no activity
**Governance Path**: `prune_inactive_pairs()` callable by factory admin

### State Growth Analysis
- Per pair: ~500 bytes (addresses + metadata)
- Max pairs: 10,000
- Total storage: ~5 MB (bounded)
- Without pruning: Unbounded growth via create/delete cycles

## Issue #360: Router Path Length Gas Griefing Protection

### Problem
Path length capped at 3 hops in some entry points, but not uniformly enforced.
Each hop = ~12,500 gas (cross-contract calls, transfers, callbacks).

### Solution
**Central Path Guard**

```rust
pub const MAX_PATH_LENGTH: usize = 3; // Max hops

pub fn validate_path_length(path: &Vec<Address>) -> Result<(), Error> {
    if path.len() - 1 > MAX_PATH_LENGTH {
        return Err(Error::PathTooLong);
    }
    Ok(())
}
```

**Applied to all router entry points**:
- `swap_exact_tokens_for_tokens()`
- `swap_tokens_for_exact_tokens()`
- `get_amounts_out()`
- `get_amounts_in()`
- `get_best_path()`

**Gas Bounds**:
- 1 hop: ~12,500 gas
- 2 hops: ~25,000 gas
- 3 hops: ~37,500 gas (MAX)

## Issue #359: TWAP Snapshot View

### Problem
TWAP consumers need reserves + block_timestamp_last + cumulative_prices.
Current API requires 3 separate calls to stitch data together.

### Solution
**New Snapshot View**

```rust
pub struct PairSnapshot {
    pub reserve0: i128,
    pub reserve1: i128,
    pub block_timestamp_last: u64,
    pub price0_cumulative: u128,
    pub price1_cumulative: u128,
    pub k_last: i128,
}

pub fn get_snapshot(env: &Env) -> PairSnapshot {
    // Returns all TWAP data in one call
}
```

**Backward Compatibility**: `get_reserves()` unchanged, returns (reserve0, reserve1)

**TWAP Calculation**:
```rust
let snapshot1 = pair.get_snapshot();
// ... wait N blocks ...
let snapshot2 = pair.get_snapshot();

let price_avg = (snapshot2.price0_cumulative - snapshot1.price0_cumulative)
    / (snapshot2.block_timestamp_last - snapshot1.block_timestamp_last);
```

## Issue #358: TTL Maintenance on Read-Only Paths

### Problem
FeeState, OracleState, GuardState are instance storage entries.
Read-only governance/view paths don't extend TTL → state can expire.

### Solution
**TTL Extension on All Operations**

```rust
const TTL_EXTENSION_LEDGERS: u32 = 518_400; // 30 days

pub fn extend_state_ttl(env: &Env) {
    for key in ["FeeState", "OracleState", "GuardState", "Reserves", ...] {
        env.storage().persistent().extend_ttl(key, TTL_EXTENSION_LEDGERS);
    }
}
```

**Called in**:
- Write operations: `swap()`, `mint()`, `burn()`
- View operations: `get_reserves()`, `get_snapshot()`
- Governance operations: `set_fee_decay()`, `set_oracle_config()`

**Guarantee**: No read-only path lets state expire

**Expiration Warning**: `is_state_approaching_expiration()` checks if TTL < 7 days

## Integration Guide

### Factory: Check Pair Bound Before Creation
```rust
if !can_create_pair(&env) {
    return Err(Error::MaxPairsReached);
}
```

### Router: Validate Path in All Entry Points
```rust
pub fn swap_exact_tokens_for_tokens(path: Vec<Address>, ...) {
    validate_path_length(&env, &path)?;
    // ... proceed with swap
}
```

### Pair: Use Snapshot View for TWAP
```rust
// Old way (3 calls)
let (r0, r1) = pair.get_reserves();
let timestamp = pair.get_block_timestamp_last();
let prices = pair.get_cumulative_prices();

// New way (1 call)
let snapshot = pair.get_snapshot();
```

### Pair: Extend TTL on All Operations
```rust
pub fn get_reserves(env: &Env) -> (i128, i128) {
    extend_ttl_on_view(&env); // <-- Add this
    // ... return reserves
}

pub fn set_fee_decay(env: &Env, decay_rate: u32) {
    extend_ttl_on_governance(&env); // <-- Add this
    // ... update fee state
}
```

## Testing

### Pair Bound Test
```rust
#[test]
fn test_max_pairs_enforced() {
    // Create MAX_ACTIVE_PAIRS pairs
    // Assert next creation fails
}
```

### Gas Griefing Test
```rust
#[test]
fn test_worst_case_path_gas() {
    let path = vec![token0, token1, token2, token3]; // 3 hops
    let gas = estimate_gas(&path);
    assert!(gas < 50_000); // Bounded
}
```

### TTL Maintenance Test
```rust
#[test]
fn test_view_extends_ttl() {
    pair.get_reserves(); // Read-only
    let ttl = get_fee_state_ttl();
    assert!(ttl >= 30_days);
}
```
