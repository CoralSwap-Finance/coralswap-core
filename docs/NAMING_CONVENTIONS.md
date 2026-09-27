# Naming Conventions — Errors & Events (issue #382)

This document is the convention every CoralSwap contract must follow for
error variants and event topic symbols. Compile-time checks enforce the
event-symbol half of it (see "Enforcement" below).

## Error variant naming

`#[contracterror]` enums are PascalCase; variants are PascalCase condition
phrases drawn from a shared vocabulary — never prefixed with the contract or
enum name (the enum itself already scopes them).

### Shared vocabulary

| Variant             | Meaning                                                        | Used by |
|---------------------|----------------------------------------------------------------|---------|
| `AlreadyInitialized`| lifecycle: double-init guard                                   | Factory, Pair, LpToken |
| `NotInitialized`    | lifecycle: storage missing                                     | Factory, Pair, LpToken |
| `Unauthorized`      | caller is not allowed to perform the action                    | Factory, LpToken |
| `ContractPaused`    | this contract's own pause switch is engaged                    | Pair, LpToken |
| `ProtocolPaused`    | the factory-level protocol pause is engaged                    | Factory |
| `IdenticalTokens`   | the two token addresses are equal                              | Factory, Router |
| `InsufficientLiquidity` / `InsufficientInputAmount` / `InsufficientOutputAmount` | AMM amount conditions | Pair, Router |
| `SlippageExceeded`  | output/input outside the user's tolerance                      | Pair, Router |
| `DustAmount`        | below the shared minimum-reserve floor (issue #393)            | Pair, Router |

Rules:

- Use an existing vocabulary word before inventing a new one; add new words to
  this table in the same PR.
- State-condition variants (`ContractPaused`, `AlreadyInitialized`, …) are
  named `<State><Condition>`; factory-level protocol governance uses the
  `Protocol` prefix (`ProtocolPaused`) to distinguish the factory-wide pause
  from a per-contract pause.
- Discriminant ranges: `FactoryError` 1–99, `PairError` 100–199,
  `OracleError` / `LpTokenError` 200–299, `RouterError` 300–399. Renaming a
  variant changes its client-visible string but **never** its numeric code —
  keep codes stable.

## Event topic symbols

- Topic symbols are **lowercase snake_case** (`[a-z0-9_]`, non-empty).
- If the full event name is ≤ 9 characters, emit it verbatim via
  `symbol_short!("name")` (the macro enforces the length limit at compile
  time).
- Longer names use the **full snake_case spelling** via `Symbol::new` (e.g.
  `flash_loan`, `protocol_fee_collected`).
- When a > 9-char name must be abbreviated for `symbol_short!`, the
  abbreviation must come from the canonical table below so the same concept
  abbreviates the same way everywhere.

### Canonical abbreviation table

| Abbreviation | Full word   |
|--------------|-------------|
| `_ss`        | single side |
| `rwd`        | reward      |
| `upg`        | upgrade     |
| `upd`        | update      |
| `stl`        | stale       |
| `thrsh`      | threshold   |
| `adm`        | admin       |
| `prop`       | propose     |

### Canonical symbol registry

| Contract | Symbol                     | Full meaning            |
|----------|----------------------------|-------------------------|
| Factory  | `created`                  | pair_created            |
| Factory  | `paused` / `unpaused`      | protocol pause state    |
| Factory  | `prop_upg`                 | upgrade proposed        |
| Factory  | `upgraded`                 | upgrade executed        |
| Factory  | `fee_to` / `setter`        | fee config changes      |
| Factory  | `fee_upd`                  | protocol fee updated    |
| Factory  | `pair_fee`                 | per-pair fee override   |
| Factory  | `protocol_fee_collected`   | protocol fee swept      |
| Pair     | `swap` / `mint` / `burn` / `sync` | core AMM events   |
| Pair     | `burn_ss` / `mint_ss`      | single-side burn / mint |
| Pair     | `rwd_added` / `rwd_rate` / `rwd_claim` | reward module |
| Pair     | `stl_thrsh`                | stale threshold updated |
| Pair     | `protocol_fee`             | protocol fee collected on pair |
| Pair     | `flash_loan`               | flash loan executed     |
| LpToken  | `approve` / `transfer` / `mint` / `burn` | SEP-41 standard events |
| LpToken  | `paused` / `unpaused` / `adm_xfer` | admin lifecycle |

`mint_1t` was renamed to `mint_ss` in issue #382 so both single-side events
share the `_ss` suffix.

## Storage keys are not event symbols

Storage-key symbols (e.g. the pair's `ProtocolFeeA` / `ProtocolFeeB`
instance-storage keys) are deliberately excluded from this convention:
renaming a storage key orphans existing on-chain state. Never "normalize" a
storage-key symbol without a state migration.

## Enforcement

The compile-time test lives in:

- `contracts/factory/src/events.rs`
- `contracts/pair/src/events.rs`
- `contracts/lp_token/src/lib.rs`

Each declares a `const fn is_convention_symbol` assertion list mirroring the
emitters; a symbol that violates the convention fails the **build**, not just
`cargo test`. When you add or rename a topic symbol, update the corresponding
assert list in the same commit.
