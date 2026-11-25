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

Verifies that each pinned deployment hash in `deployments.json` matches a release rebuild of the commit recorded for that deployment. The gate checks out each recorded commit in a temporary worktree, runs the pinned release build, and compares the resulting manifest hash. PR changes to contract code do not affect old deployment entries.

### Usage

```bash
bash scripts/assert-wasm-hashes.sh

DEPLOYMENTS_FILE=deployments.json bash scripts/assert-wasm-hashes.sh

RELEASE_MANIFEST=release/release-manifest.json DEPLOYMENTS_FILE=deployments.json \
  bash scripts/assert-wasm-hashes.sh
```

`RELEASE_MANIFEST` and `RELEASE_MANIFEST_DIR` are test/release-asset overrides; normal runs rebuild each recorded commit. Entries with a hash but no recorded commit fail the gate. Entries without hashes are skipped.

## build-release.sh and build-release-container.sh

Defines the single release build: pinned Rust toolchain, `cargo build --locked --release`, and pinned `stellar contract optimize`. `build-release.sh` runs on the host; `build-release-container.sh` builds the pinned `Dockerfile.release` image and runs the same script in it.

### Usage

```bash
bash scripts/build-release.sh
bash scripts/build-release-container.sh

OUTPUT_DIR=release-b TARGET_DIR=release-b/target CLEAN_TARGET=1 \
  bash scripts/build-release-container.sh
```

Both write `release/release-manifest.json`, per-contract manifests under `release/manifests/`, and optimized WASMs under `release/wasm/`. Every contract manifest records `sha256`, `size_bytes`, `commit`, `toolchain`, `cli_version`, `target`, and `profile`.

## benchmark.sh and check-size.sh

`benchmark.sh` runs the pinned release build and writes optimized contract sizes to `benchmark-results.json`. `check-size.sh` enforces the tightened budgets in `scripts/size-budget.json` and is available as `make size-check`.

```bash
make benchmark
make size-check
bash scripts/check-size.sh custom-results.json custom-budget.json
```

## test-scripts.sh

Runs fixture-based tests for the size gate, deployment hash gate, and manifest comparison.

```bash
make test-scripts
```

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
