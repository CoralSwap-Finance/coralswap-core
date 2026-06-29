# CoralSwap Deployment Guide

Step-by-step instructions for deploying CoralSwap contracts to Stellar testnet and mainnet.

## Prerequisites

| Requirement | Version / Notes |
|---|---|
| **Rust** | Stable toolchain (managed by `rust-toolchain.toml`) |
| **wasm32-unknown-unknown target** | Installed automatically via `rust-toolchain.toml` |
| **Soroban CLI** | `stellar` CLI with Soroban support (>= 21.x recommended) |
| **Stellar account** | Funded account with signing key for deployment |

Install the Soroban CLI if you haven't already:

```bash
cargo install --locked stellar-cli
```

Verify setup:

```bash
stellar --version
rustup target list --installed | grep wasm32
```

## Network Configuration

The repository ships with network presets in `soroban-deploy.toml`:

| Network | RPC URL | Passphrase |
|---|---|---|
| **Testnet** | `https://soroban-testnet.stellar.org` | `Test SDF Network ; September 2015` |
| **Mainnet** | `https://soroban-rpc.stellar.org` | `Public Global Stellar Network ; September 2015` |

Configure your identity (one-time):

```bash
# Generate a new identity for testnet
stellar keys generate deployer --network testnet

# Or import an existing secret key
stellar keys add deployer --secret-key
```

Fund the testnet account:

```bash
stellar keys fund deployer --network testnet
```

## Build Contracts

```bash
# Build all contracts for wasm32 (release profile with overflow checks)
make build

# Or manually:
soroban contract build
```

The compiled WASM files are output to `target/wasm32-unknown-unknown/release/`.

Verify the build artifacts exist:

```bash
ls target/wasm32-unknown-unknown/release/*.wasm
```

Expected outputs:

- `coralswap_factory.wasm`
- `coralswap_pair.wasm`
- `coralswap_lp_token.wasm`
- `coralswap_router.wasm`

## Verify WASM Hashes

Before deploying, record the WASM hash of each contract. These hashes are used during initialization and for verifying that deployed code matches your build.

```bash
stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_factory.wasm \
  --source deployer \
  --network testnet
```

This prints the WASM hash (a 64-character hex string). Repeat for each contract. Save these hashes — you will need the `pair` and `lp_token` hashes when initializing the Factory.

## Deploy to Testnet

### 1. Install WASM code on-chain

```bash
# Install each contract's WASM and note the returned hash
PAIR_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_pair.wasm \
  --source deployer \
  --network testnet)

LP_TOKEN_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_lp_token.wasm \
  --source deployer \
  --network testnet)

FACTORY_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_factory.wasm \
  --source deployer \
  --network testnet)

ROUTER_HASH=$(stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_router.wasm \
  --source deployer \
  --network testnet)
```

### 2. Deploy the Factory

```bash
FACTORY_ID=$(stellar contract deploy \
  --wasm-hash $FACTORY_HASH \
  --source deployer \
  --network testnet)

echo "Factory deployed at: $FACTORY_ID"
```

### 3. Initialize the Factory

```bash
stellar contract invoke \
  --id $FACTORY_ID \
  --source deployer \
  --network testnet \
  -- \
  initialize \
  --signers '["<SIGNER_ADDRESS_1>"]' \
  --pair_wasm_hash $PAIR_HASH \
  --lp_token_wasm_hash $LP_TOKEN_HASH \
  --fee_to_setter <FEE_SETTER_ADDRESS>
```

The `signers` array accepts 1–10 addresses that form the multisig set for governance operations (pause, upgrade). The threshold is `ceil(n/2)`.

### 4. Deploy and Initialize the Router

```bash
ROUTER_ID=$(stellar contract deploy \
  --wasm-hash $ROUTER_HASH \
  --source deployer \
  --network testnet)

stellar contract invoke \
  --id $ROUTER_ID \
  --source deployer \
  --network testnet \
  -- \
  initialize \
  --factory $FACTORY_ID \
  --hubs '["<HUB_TOKEN_1>", "<HUB_TOKEN_2>"]'
```

Hub tokens are used for multi-hop path discovery (e.g., USDC, XLM native).

### 5. Create a Pair

```bash
stellar contract invoke \
  --id $FACTORY_ID \
  --source deployer \
  --network testnet \
  -- \
  create_pair \
  --token_a <TOKEN_A_ADDRESS> \
  --token_b <TOKEN_B_ADDRESS>
```

This deploys a new Pair contract and its LP Token contract automatically.

## Deploy to Mainnet

The process is identical to testnet with the following changes:

1. Replace `--network testnet` with `--network mainnet` in all commands.
2. Use a **dedicated mainnet identity** with real XLM for gas fees.
3. **Do not reuse testnet keys on mainnet.**

```bash
# Add your mainnet deployer key
stellar keys add mainnet-deployer --secret-key

# Repeat all install / deploy / invoke steps with --network mainnet
```

### Mainnet Checklist

- [ ] All contracts pass `cargo test` and `cargo clippy`
- [ ] WASM hashes match your audited build (compare `stellar contract install` output)
- [ ] Multisig signer set includes at least 2 addresses for production governance
- [ ] `fee_to_setter` is set to an appropriate operational address
- [ ] Router hub tokens are set to liquid, widely-held assets
- [ ] Test the full flow on testnet first: create pair → add liquidity → swap → remove liquidity

## Verifying a Deployed Contract

After deployment, verify the on-chain WASM matches your local build:

```bash
# Get the hash of the deployed contract's WASM
stellar contract info \
  --id $FACTORY_ID \
  --network testnet

# Compare against your local build hash
stellar contract install \
  --wasm target/wasm32-unknown-unknown/release/coralswap_factory.wasm \
  --source deployer \
  --network testnet
```

Both should return the same WASM hash. If they differ, the on-chain code does not match your local source.

## Troubleshooting

### `error: HostError ... storage`

The deployer account may be out of XLM for storage rent. Fund it with more XLM:

```bash
stellar keys fund deployer --network testnet
```

### `error: transaction simulation failed`

Common causes:

- **Already initialized**: The Factory or Router `initialize` function has a double-init guard. Check if the contract is already initialized before calling.
- **Insufficient gas**: Increase the fee with `--fee <stroops>` (e.g., `--fee 1000000`).
- **Wrong network**: Verify you're targeting the correct network in your command.

### `error: wasm32-unknown-unknown target not installed`

```bash
rustup target add wasm32-unknown-unknown
```

### Build fails with dependency errors

```bash
cargo clean
make build
```

### Contract invocation returns `NotInitialized`

Ensure you called the `initialize` function on the contract before any other operation. Check the transaction status on a block explorer (e.g., [stellar.expert](https://stellar.expert) for mainnet or [Stellar Laboratory](https://laboratory.stellar.org) for testnet).
