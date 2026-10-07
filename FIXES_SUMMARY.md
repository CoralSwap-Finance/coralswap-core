# CoreSwap-Core Fixes - Issues #358-361

## Issue #361: Use OrderedVec/lib-map iteration for pair list (state growth control) ✅
**Problem**: Plain Vec in factory grows with create/delete, no prune API
**Fix**: Bounded pair count with governance prune path
**Location**: contracts/factory/src/lib.rs

## Issue #360: Router path length guard against gas griefing ✅
**Problem**: Path length cap not enforced uniformly across all entry points
**Fix**: Central MAX_PATH_LENGTH check in all path-accepting functions
**Location**: contracts/router/src/lib.rs

## Issue #359: get_reserves emit historical snapshot for TWAP ✅
**Problem**: TWAP consumers need block_timestamp_last + cumulative prices
**Fix**: New get_snapshot() view with reserves + oracle data
**Location**: contracts/pair/src/lib.rs

## Issue #358: Fee_decay state TTL maintenance ✅
**Problem**: Read-only paths don't extend TTL (state expiration risk)
**Fix**: TTL extension in all view/governance paths
**Location**: contracts/pair/src/lib.rs

All fixes: production-ready, documented, tested.
