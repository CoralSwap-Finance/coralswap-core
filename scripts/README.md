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

## check-coverage.sh

Runs the whole test suite under [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) and enforces the per-workspace-member line-coverage floors recorded in [`coverage.toml`](../coverage.toml). The policy check itself lives in `coverage_gate.py`.

### Prerequisites

- `cargo install cargo-llvm-cov --locked`
- the `llvm-tools-preview` rustup component (already listed in `rust-toolchain.toml`)
- Python 3.11+ (`tomllib` is used to read the policy)
- release WASM artifacts — the script builds them first because the factory and lp-token tests upload `coralswap_pair.wasm` / `coralswap_lp_token.wasm`

### Usage

```bash
# Build, measure, enforce (what CI runs)
./scripts/check-coverage.sh

# Re-check an existing report without re-running the tests
COVERAGE_REUSE=1 ./scripts/check-coverage.sh

# Keep the previous instrumented build (much faster local iteration)
COVERAGE_NO_CLEAN=1 ./scripts/check-coverage.sh

# Per-file coverage numbers in addition to the per-member table
./scripts/check-coverage.sh --files
```

### Environment variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `COVERAGE_POLICY` | `coverage.toml` | policy file to enforce |
| `COVERAGE_LCOV` | `target/llvm-cov/coverage.lcov` | LCOV report to write/read |
| `COVERAGE_REUSE` | `0` | `1` = skip `cargo llvm-cov`, check the existing report |
| `COVERAGE_NO_CLEAN` | `0` | `1` = reuse the previous instrumented build |
| `COVERAGE_HTML` | `0` | `1` = also render `target/llvm-cov/html/index.html` |
| `COVERAGE_CARGO_FLAGS` | – | extra flags for `cargo llvm-cov` (e.g. `-p coralswap-pair`) |
| `CARGO_PROFILE_DEV_DEBUG` | `line-tables-only` | debug info level; line tables are enough for line coverage and far cheaper than the default |

### Exit codes

`0` policy satisfied · `1` floor breached or an exemption past its expiry · `2` unusable policy or report · anything else `cargo llvm-cov` failed (test/build failure).

### Policy file (`coverage.toml`)

- `[measurement] exclude` — globs for sources that are not production code (`src/test/**`, dev-only helpers).
- `[floors]` — minimum line coverage per workspace member. Members must appear here or under `[no_floor.<member>]` with a `reason`.
- `[[exemptions]]` — allowlist entry for an uncovered-but-justified region: `path`, `lines` (e.g. `"3, 16-20"`), `reason`, and a mandatory `# expires: YYYY-MM-DD` comment inside the entry. The gate fails the PR when an exemption expires, warns 14 days ahead, and errors if the line numbers no longer match.
