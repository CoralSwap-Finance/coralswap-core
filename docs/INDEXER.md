# CoralSwap Indexer Guide

This document describes the event shapes, emission semantics, and indexing recommendations for off-chain indexers, analytics services, and client applications interacting with CoralSwap.

---

## 1. Overview

CoralSwap contracts emit structured events on the Stellar Soroban network using the contract event system. Events are organized by contract:

- **Factory Contract**: Emits protocol lifecycle events, pause/resume state transitions, per-pair freeze status, pair registrations, and contract upgrades.
- **Pair Contracts**: Emits liquidity pool operations (swap, mint, burn, sync, flash loan) and protocol fee collections.

---

## 2. Factory Events

### 2.1 Protocol Pause & Resume

When the multisig admin pauses or resumes protocol trading, events are emitted to inform indexers:

| Event | Topic 0 | Topic 1 | Data Tuple | Description |
|---|---|---|---|---|
| `paused` | `Symbol("paused")` | — | `()` | Factory trading and new pair creation paused. |
| `unpaused` | `Symbol("unpaused")` | — | `()` | Factory unpaused via `unpause()`. |
| `resumed` | `Symbol("resumed")` | — | `()` | Factory resumed via `resume()` or `unpause()`. |

> **Indexer Note**: Pollers caching a `paused` status should subscribe to both `unpaused` and `resumed` topics to immediately clear paused views.

### 2.2 Factory Sync (Heartbeat)

To prevent indexers from holding stale views or polling individual storage keys during downtime or node restarts, the Factory exposes a public `sync()` entrypoint that emits a heartbeat event:

| Event | Topic 0 | Topic 1 | Data Tuple | Description |
|---|---|---|---|---|
| `sync` | `Symbol("sync")` | — | `(paused: bool, pair_count: u32)` | Current factory pause state and total registered pairs. |

- **`paused`** (`bool`): `true` if protocol operations are paused, `false` otherwise.
- **`pair_count`** (`u32`): Total number of liquidity pairs instantiated by the factory.

### 2.3 Per-Pair Freeze State

The `fee_to_setter` address can freeze or unfreeze a single pair without pausing the entire protocol — the incident-response path for a compromised pool. `freeze_pair` requires the target to be a registered pair and authorizes with the invoking address alone (no multisig). While frozen, the pair rejects `swap`, `mint`, `mint_with_one_token`, `burn`, `burn_single_side`, and `flash_loan` with `PairError::ContractFrozen` (125); views and `sync()` stay open.

| Event | Topic 0 | Topic 1 | Data Map | Description |
|---|---|---|---|---|
| `pair_frozen_event` | `Symbol("pair_frozen_event")` | `pair: Address` | `{ by: Address, ledger: u32 }` | Specific pair frozen; all value-moving paths refuse. |
| `pair_unfrozen_event` | `Symbol("pair_unfrozen_event")` | `pair: Address` | `{ by: Address, ledger: u32 }` | Specific pair unfrozen; normal operations resumed. |

- **`by`** (`Address`): the authorizing `fee_to_setter` that invoked `freeze_pair` / `unfreeze_pair`.
- **`ledger`** (`u32`): the ledger sequence on which the event fired.

> **Indexer Note**: `by` and `ledger` are a **map**, not a tuple — the `#[contractevent]` macro serializes all non-topic fields as a name-sorted `ScMap`. Subscribe to the full `pair_frozen_event` symbol (17 chars, so emitted via `Symbol::new`, not `symbol_short!`), keyed on topic 1 for per-pair status views.

### 2.4 Pair Creation

Emitted whenever a new liquidity pair is deployed:

| Event | Topic 0 | Topic 1 | Topic 2 | Data Tuple |
|---|---|---|---|---|
| `created` | `Symbol("created")` | `token_a: Address` | `token_b: Address` | `(pair: Address, all_pairs_length: u32)` |

- `token_a` and `token_b` are canonical sorted addresses (`token_a < token_b`).
- `pair` is the deployed contract address of the new pair.
- `all_pairs_length` is the total count of pairs including the newly deployed pair.

### 2.5 Contract Upgrade

| Event | Topic 0 | Topic 1 | Data Tuple | Description |
|---|---|---|---|---|
| `upgrade_proposed` | `Symbol("upgrade_proposed")` | `new_wasm_hash: BytesN<32>` | `()` | Upgrade proposed by multisig. |
| `upgraded` | `Symbol("upgraded")` | — | `()` | Contract WASM updated. |

---

## 3. Pair Events

| Event | Topic 0 | Topic 1 | Data Tuple | Description |
|---|---|---|---|---|
| `swap` | `Symbol("swap")` | `sender: Address` (+ Topic 2 `token_a: Address`, Topic 3 `token_b: Address`, see 3.1) | `(amount_a_in: i128, amount_b_in: i128, amount_a_out: i128, amount_b_out: i128, fee_bps: u32, to: Address)` | Token swap executed. |
| `mint` | `Symbol("mint")` | `sender: Address` | `(amount_a: i128, amount_b: i128)` | Liquidity deposited; LP tokens minted. |
| `burn` | `Symbol("burn")` | `sender: Address` | `(amount_a: i128, amount_b: i128, to: Address)` | Liquidity removed; LP tokens burned. |
| `sync` | `Symbol("sync")` | — | `(reserve_a: i128, reserve_b: i128)` | Pair reserves updated or synced. |
| `flash_loan` | `Symbol("flash_loan")` | `receiver: Address` | `(amount_a: i128, amount_b: i128, fee_a: i128, fee_b: i128, fee_bps: u32)` | Flash loan completed. Zero-amount calls no-op without emitting. |
| `burn_ss` | `Symbol("burn_ss")` | `to: Address` | `(lp_amount: i128, preferred_token: Address, total_out: i128)` | Single-sided liquidity burn. |
| `mint_1t` | `Symbol("mint_1t")` | `sender: Address` | `(token_in: Address, amount_in: i128, swap_amount: i128, lp_minted: i128)` | Single-sided liquidity mint. |
| `protocol_fee` | `Symbol("protocol_fee")` | `fee_to: Address` | `(amount_a: i128, amount_b: i128)` | Accumulated protocol fees distributed. |

### 3.1 Swap Topic Shape

The `swap` event carries four topics:

| Index | Value | Notes |
|---|---|---|
| 0 | `Symbol("swap")` | Event name. |
| 1 | `sender: Address` | Unchanged from the original shape. |
| 2 | `token_a: Address` | Pair's canonical first token (lower address). |
| 3 | `token_b: Address` | Pair's canonical second token (higher address). |

- `amount_a_*` in the data tuple always refers to `token_a` (topic 2) and `amount_b_*` to `token_b` (topic 3), so a swap can be decoded without reading the pair's `token_a()`/`token_b()` views.
- Topics 0 and 1 keep their original positions; the token topics are appended, so filters that match on `(Symbol("swap"), sender)` prefixes keep working. Indexers that asserted an exact topic count of 2 must accept 4.
- To index all swaps touching a token, filter on topic 2 **or** topic 3 equal to that token address.

---

## 4. Indexer Integration Recommendations

1. **Heartbeat & Resumption Detection**:
   - Subscribe to Factory topics `Symbol("resumed")` and `Symbol("unpaused")`.
   - Call `factory.sync()` periodically or on restart to refresh state cache without full contract introspection.
2. **Deterministic LP Token Metadata**:
   - LP tokens generated by the Factory follow deterministic metadata: `name = CORAL-SWAP-LP-<HEX8>` and `symbol = CLP-<HEX8>` (derived from SHA-256 of canonical pair addresses).
3. **Flash Loan Ingestion**:
   - `flash_loan(0, 0)` is a clean no-op and does not emit a `flash_loan` event. Only non-zero executed loans emit events.
