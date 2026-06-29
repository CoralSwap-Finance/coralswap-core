# Contract Upgrade Guide

This document explains how to safely upgrade CoralSwap Soroban contracts through the timelocked governance process.

## How Soroban Upgrades Work

Soroban contracts can be upgraded by replacing their WASM bytecode on-chain. The key mechanism is:

```rust
env.deployer().update_current_contract_wasm(new_wasm_hash);
```

This replaces the contract's executable code while preserving its **contract ID**, **address**, and **persistent storage**. The new WASM takes effect immediately after the call completes.

> **Warning**: The upgrade replaces code but not storage. If the new contract version changes data structures stored in contract storage, you must handle migration carefully. See [Storage Migrations](#storage-migrations) below.

## CoralSwap Upgrade Mechanism

CoralSwap implements a **timelocked multisig upgrade** process on the Factory contract. Upgrades cannot be applied instantly — a mandatory 72-hour waiting period protects users from malicious or rushed changes.

### Architecture

```
propose_upgrade() ──→ 72-hour timelock ──→ execute_upgrade()
       │                                          │
  multisig gate                              anyone can call
  (ceil(n/2) signers)                     (after timelock expires)
       │
 cancel_upgrade() ←── multisig gate
```

### Timelock Parameters

| Parameter | Value |
|---|---|
| Delay | 72 hours |
| Delay in ledgers | ~51,840 (assuming 5-second ledger close) |
| Multisig threshold | `ceil(n / 2)` where `n` = number of registered signers |
| Max signers | 10 |

## Step-by-Step Upgrade Process

### 1. Build and Audit the New Contract

```bash
# Build the new contract version
make build

# Run the full test suite
make test

# Run clippy for lint checks
make lint
```

Ensure the new WASM has been audited or peer-reviewed before proposing an upgrade.

### 2. Install the New WASM On-Chain

```bash
NEW_WASM_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_factory.wasm \
  --source deployer \
  --network mainnet)

echo "New WASM hash: $NEW_WASM_HASH"
```

Record this hash. It will be referenced in the proposal and can be verified independently by anyone.

### 3. Propose the Upgrade

The proposal requires authorization from `ceil(n/2)` of the registered multisig signers.

```bash
stellar contract invoke \
  --id $FACTORY_ID \
  --source signer1 \
  --network mainnet \
  -- \
  propose_upgrade \
  --signers '["<SIGNER_1>", "<SIGNER_2>"]' \
  --new_wasm_hash $NEW_WASM_HASH
```

This stores a `PendingUpgrade` record containing:
- The new WASM hash
- The ledger sequence at proposal time

An `UpgradeProposed` event is emitted on-chain.

**Constraint**: Only one proposal can be active at a time. If a proposal already exists, it must be cancelled or executed before a new one can be submitted.

### 4. Wait for the Timelock

The upgrade cannot be executed until 51,840 ledgers (~72 hours) have passed since the proposal.

Monitor the current ledger sequence:

```bash
stellar contract invoke \
  --id $FACTORY_ID \
  --network mainnet \
  -- \
  is_paused
```

Use a block explorer to track ledger progression and estimate when the timelock expires.

### 5. Execute the Upgrade

After the timelock has elapsed, anyone can trigger execution:

```bash
stellar contract invoke \
  --id $FACTORY_ID \
  --source deployer \
  --network mainnet \
  -- \
  execute_upgrade
```

This will:
1. Verify the timelock has expired (reverts with `UpgradeTimelockNotExpired` if too early)
2. Call `env.deployer().update_current_contract_wasm(new_wasm_hash)`
3. Clear the pending proposal
4. Increment `protocol_version` in Factory storage
5. Emit an `UpgradeExecuted` event

### 6. Verify the Upgrade

```bash
# Check the on-chain WASM hash matches
stellar contract info \
  --id $FACTORY_ID \
  --network mainnet
```

Compare the reported hash against `$NEW_WASM_HASH`.

## Cancelling an Upgrade

If a vulnerability is found in the proposed WASM or the upgrade is no longer needed, cancel it before execution:

```bash
stellar contract invoke \
  --id $FACTORY_ID \
  --source signer1 \
  --network mainnet \
  -- \
  cancel_upgrade \
  --signers '["<SIGNER_1>", "<SIGNER_2>"]'
```

Cancellation requires the same multisig threshold as proposing. This clears the `PendingUpgrade` record.

## Storage Migrations

Soroban upgrades replace WASM code but **do not modify existing storage**. This is the most dangerous aspect of contract upgrades.

### Storage Collision Risks

When storage types change between versions, the old data remains in storage under the same keys. The new code will attempt to deserialize this old data using the new type definitions, which can cause:

1. **Deserialization panics**: If a field is added without a default, reading old storage will fail.
2. **Silent data corruption**: If fields are reordered or types change, the deserializer may succeed but produce incorrect values.
3. **Inaccessible funds**: If the Pair storage structure changes and deserialization fails, reserves become locked.

### Safe Migration Strategies

#### Adding Fields

If you add a new field to a storage struct (e.g., adding a field to `FactoryStorage`):

- Use a new storage key for the new data rather than expanding the existing struct.
- Alternatively, include a migration function in the new WASM that reads the old struct format and writes the new format.

```rust
// New version — migration function called once after upgrade
pub fn migrate(env: Env) -> Result<(), FactoryError> {
    // Read old-format storage
    let old: OldFactoryStorage = env.storage().instance().get(&DataKey::Factory).unwrap();
    
    // Write new-format storage with default for new field
    let new = NewFactoryStorage {
        signers: old.signers,
        pair_wasm_hash: old.pair_wasm_hash,
        // ... copy all old fields ...
        new_field: default_value,
    };
    env.storage().instance().set(&DataKey::Factory, &new);
    Ok(())
}
```

#### Removing Fields

Removing a field from a `#[contracttype]` struct changes its serialization layout. The old data will fail to deserialize.

- Never remove fields from storage structs in a direct upgrade.
- Instead, mark them as deprecated and ignore them until a full migration is performed.

#### Renaming or Retyping Fields

This is equivalent to removing the old field and adding a new one — the same risks apply. Always use a migration function.

### Migration Checklist

- [ ] Compare `#[contracttype]` struct definitions between old and new versions field by field
- [ ] If any struct changed, write and test a `migrate()` function
- [ ] Test the migration against storage state exported from a testnet deployment
- [ ] Call `migrate()` immediately after `execute_upgrade` completes
- [ ] Verify all contract functions work correctly after migration on testnet before proposing a mainnet upgrade

## Upgrading Pair and LP Token Contracts

The Factory's upgrade mechanism only covers the **Factory contract itself**. Pair and LP Token contracts are deployed from WASM hashes stored in the Factory.

To deploy new versions of Pair or LP Token contracts:

1. Install the new WASM on-chain and record the hash.
2. Update the stored `pair_wasm_hash` or `lp_token_wasm_hash` in the Factory (this requires a Factory code change to expose a setter, or a Factory upgrade that updates these values).
3. **Existing pairs are not affected** — they continue running the WASM they were deployed with.
4. Only **newly created pairs** will use the updated WASM hash.

> Upgrading existing Pair contracts individually would require each Pair to have its own upgrade mechanism, which is not implemented in V1. Plan for this if per-pair upgrades are needed.

## Emergency Procedures

If a critical vulnerability is discovered in a deployed contract:

1. **Pause the protocol** immediately using `Factory::pause()` with multisig authorization.
2. **Propose the fix** via `propose_upgrade()` with the patched WASM hash.
3. The 72-hour timelock still applies — there is no emergency bypass. This is a deliberate security trade-off that prevents malicious instant upgrades at the cost of slower emergency response.
4. If the vulnerability allows fund extraction, communicate with users to withdraw liquidity while the protocol is paused.
5. Execute the upgrade after the timelock expires, then unpause.
