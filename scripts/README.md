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
