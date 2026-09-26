# Deployment Scripts

## deploy.sh

Automated deployment script for the CoralSwap protocol on Stellar networks.

### Prerequisites

- Stellar CLI installed (`stellar` command available)
- Source account with sufficient XLM for deployment
- Network configured (testnet/futurenet/mainnet)

### Usage

```bash
# Deploy to testnet
SOURCE_ACCOUNT="YOUR_SECRET_KEY" NETWORK="testnet" ./scripts/deploy.sh

# Deploy to futurenet
SOURCE_ACCOUNT="YOUR_SECRET_KEY" NETWORK="futurenet" ./scripts/deploy.sh
```

### Environment Variables

- `SOURCE_ACCOUNT` (required): Secret key of the deploying account
- `NETWORK` (optional): Target network (default: testnet)

### Output

The script generates a `deployments.json` file containing all deployed contract addresses:

```json
{
  "testnet": {
    "factory": {
      "address": "C...",
      "deployedAt": "2026-06-26T00:00:00Z"
    },
    "router": {
      "address": "C...",
      "deployedAt": "2026-06-26T00:00:00Z"
    }
  }
}
```

### Idempotency

The script checks for existing deployments in `deployments.json` and skips already-deployed contracts, making it safe to re-run.

## assert-wasm-hashes.sh

Asserts that compiled release WASM artifact hashes match recorded deployment metadata in `deployments.json` and `soroban-deploy.toml`.

### Usage

```bash
# Verify built artifacts against deployment metadata
./scripts/assert-wasm-hashes.sh

# Or specify a custom WASM directory or deployments file
WASM_DIR="target/wasm32v1-none/release" DEPLOYMENTS_FILE="deployments.json" ./scripts/assert-wasm-hashes.sh
```

If no pinned WASM hashes are present in `deployments.json`, the check skips cleanly. When entries with hashes exist, the script calculates SHA-256 hashes of compiled artifacts and fails if code drift is detected.
