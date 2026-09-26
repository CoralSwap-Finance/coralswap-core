# Storage-Lifetime Contract (issue #385)

Soroban ledger storage is rent-based: every entry carries an expiry ledger,
and once it expires the data is gone. The TTL fixes (storage-expiry
hardening, PR #406 and the shared TTL policy in `contracts/shared`) added
`extend_ttl` calls across the codebase. This document is the **written
guarantee** of entry lifetimes so that other teams writing to the same
storage (rewards modules, future staking) cannot silently skip extension
and let live state expire.

The single source of truth for the numeric policy is
`contracts/shared/src/lib.rs`. The numbers below assume ~5s ledger close
times (cadence math is documented in the shared crate).

## Policy summary

| Policy constant(s)                  | Applies to                  | Threshold | Extend to |
|-------------------------------------|-----------------------------|-----------|-----------|
| `INSTANCE_TTL_THRESHOLD` / `INSTANCE_TTL_EXTEND_TO` | pair + router instance storage | 3.5 days (60_480 ledgers) | 7 days (120_960 ledgers) |
| `FACTORY_INSTANCE_THRESHOLD` / `FACTORY_INSTANCE_BUMP_AMOUNT` | factory instance storage | 1 day (17_280) | 30 days (518_400) |
| `REENTRANCY_TTL_THRESHOLD` / `REENTRANCY_TTL_EXTEND_TO` | pair reentrancy-lock flips | 5_000 ledgers | 7 days (120_960) |
| `LP_PERSISTENT_THRESHOLD` / `LP_PERSISTENT_EXTEND_TO` | LP-token persistent entries (balances, allowances, nonces) | 30 days (518_400) | 60 days (1_036_800) |

## Storage map by contract

### Factory (all instance storage)

| Entry | Type | Written by | Refresh point |
|---|---|---|---|
| `Factory` (signers, wasm hashes, version, pause, fee config) | instance | `initialize`, `pause`, `unpause`, `set_fee_to`, `set_fee_to_setter`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` | every write (`storage::extend_instance_ttl`) |
| `Pair(t0, t1)` registry | instance | `create_pair` | `create_pair` |
| `PairList`, `TotalPairs` | instance | `create_pair` | `create_pair` |
| `PendingUpgrade` | instance | `propose_upgrade`, `cancel_upgrade` | `propose_upgrade` |
| `PairFeeOverride(pair)` | instance | `set_pair_fee` | `set_pair_fee` |
| `protocol_fee_balance` accumulator | instance | `deposit_protocol_fee` | `deposit_protocol_fee` |

Lifetime guarantee: instance entries are extended to ≥ 30 days on every
mutating entry point, so idle-but-initialized factories never expire. Reads
(`get_pair`, views) do **not** extend TTL — they may be called at any time
and simply return `None` / defaults if storage is gone.

### Pair (all instance storage)

| Entry | Type | Written by | Refresh point |
|---|---|---|---|
| `Pair` state (reserves, tokens, factory, lp_token, k_last) | instance | `initialize`, `mint`, `burn`, `burn_single_side`, `swap`, `sync`, `flash_loan` | every mutating entrypoint (`extend_instance_ttl`) |
| `ProtocolFeeA` / `ProtocolFeeB` accumulators | instance | `swap` (when protocol fee is active) | `swap` |
| fee-state / dynamic-fee accumulators | instance | `sync`, `swap` | `sync` |
| reentrancy lock flag | instance | `flash_loan` guard flip | `extend_reentrancy_ttl` at flip time (5_000-ledger window is intentional — the lock must die quickly) |
| oracle observations | instance | oracle update paths | same write |

### Router (all instance storage)

| Entry | Type | Written by | Refresh point |
|---|---|---|---|
| `CommitConfig`, `CommitCount`, `Factory` | instance | `set_commit_config`, `set_factory` (admin) | write paths |
| commits (hash, nonce, ledger) | instance | `commit`, `reveal` (prune/overwrite) | `commit` |

### LpToken (persistent entries + instance config)

| Entry | Type | Written by | Refresh point |
|---|---|---|---|
| balances (`Balance(owner)`) | **persistent** | `mint`, `burn`, `transfer` | every write (`LP_PERSISTENT_THRESHOLD`/`EXTEND_TO`) — balances MUST survive idle wallets |
| allowances (`Allowance(from, spender)`) | **persistent** | `approve` | `approve` (extended to the requested expiration ledger) |
| permit nonces | **persistent** | `permit` | `permit` |
| admin / metadata / pause config | instance | `initialize`, admin setters | write paths |

Persistent balances deliberately outlive instance storage: an LP holder who
never transacts again must still find their balance after 30 days. The
60-day extend-to window means an entry touched once every ~2 months is
effectively immortal.

## The contract (rules other teams must follow)

1. **Every `storage().set` pairs with an `extend_ttl` call** on the same
   entry (or a documented group extension covering it) in the same
   call path. Instance writes inside the four core contracts pair with
   `coralswap_shared::extend_instance_ttl` / `extend_factory_instance_ttl` /
   `extend_reentrancy_ttl`; LP persistent writes pair with the
   `LP_PERSISTENT_*` policy.
2. **Choose the storage type deliberately**: per-contract configuration and
   hot state → instance; user-owned assets/authorizations that must outlive
   idle periods → persistent.
3. **Views never extend TTL.** If a view returns defaults because storage
   expired, that is the documented behaviour — do not "fix" it by extending
   in read paths.
4. **New storage maps added by other teams** (rewards, staking) must publish
   their threshold/extend-to constants in `contracts/shared` (or document
   their own policy here) and follow rule 1.
5. **Storage-key symbols are stable forever** — see
   `docs/NAMING_CONVENTIONS.md`; renaming a key orphans on-chain state.

Enforcement: the PR-template checklist item "Any new `storage().set` writes
are paired with an `extend_ttl` call" must be satisfied (or explicitly
waived with a reason) in every review.
