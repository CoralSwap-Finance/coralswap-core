#!/usr/bin/env bash
# Runs the whole test suite under cargo-llvm-cov and enforces the per-member
# coverage floor recorded in coverage.toml.
#
# Coverage is what stops the high-risk paths (flash loan, single-sided mint,
# reentrancy guard, auth) from silently losing tests: every workspace member
# has a floor, and a PR that lands below it fails this check with a report.
# Justified-but-uncovered regions go through the allowlist in coverage.toml,
# each with a reason and an `# expires:` date that the gate enforces.
#
# Usage:
#   ./scripts/check-coverage.sh                 # build, measure, enforce
#   COVERAGE_REUSE=1 ./scripts/check-coverage.sh  # re-check an existing report
#   ./scripts/check-coverage.sh --files         # also print per-file numbers
#
# Environment:
#   COVERAGE_POLICY       policy file            (default: coverage.toml)
#   COVERAGE_LCOV         LCOV report to write   (default: target/llvm-cov/coverage.lcov)
#   COVERAGE_REUSE=1      skip cargo-llvm-cov and check the existing report
#   COVERAGE_NO_CLEAN=1   keep the previous instrumented build (faster locally)
#   COVERAGE_HTML=1       also render target/llvm-cov/html for artifacts
#   COVERAGE_CARGO_FLAGS  extra flags for cargo-llvm-cov (e.g. "-p coralswap-pair")
#   CARGO_PROFILE_DEV_DEBUG  debug info level (default: line-tables-only — enough
#                         for line coverage and far cheaper than the default)
#
# Exit codes: 0 = policy satisfied, 1 = floor breached / exemption expired,
# 2 = unusable policy or report, any other = the coverage run itself failed.

set -euo pipefail

cd "$(dirname "$0")/.."

POLICY="${COVERAGE_POLICY:-coverage.toml}"
LCOV="${COVERAGE_LCOV:-target/llvm-cov/coverage.lcov}"
REUSE="${COVERAGE_REUSE:-0}"
NO_CLEAN="${COVERAGE_NO_CLEAN:-0}"
HTML="${COVERAGE_HTML:-0}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}"

die() {
  echo "error: $*" >&2
  exit 1
}

annotate() {
  # annotate <level> <message>
  if [ "${GITHUB_ACTIONS:-}" = "true" ]; then
    echo "::$1 title=Coverage::$2"
  fi
}

command -v cargo >/dev/null 2>&1 || die "cargo not found on PATH"
command -v python3 >/dev/null 2>&1 || die "python3 not found on PATH (3.11+ is required)"
python3 -c 'import tomllib' >/dev/null 2>&1 ||
  die "python3 cannot import tomllib — Python 3.11+ is required"
command -v cargo-llvm-cov >/dev/null 2>&1 ||
  die "cargo-llvm-cov not found — install it with:
    cargo install cargo-llvm-cov --locked
  and make sure the llvm-tools-preview component is available (rustup component add llvm-tools-preview)"

if [ "$REUSE" != "1" ]; then
  # The factory and lp-token tests upload the real release WASM artifacts
  # (see load_wasm() in contracts/factory/src/test/mod.rs), so the coverage
  # run cannot start before they exist.
  echo "==> building release WASM test artifacts"
  cargo build --release --target wasm32v1-none

  mkdir -p "$(dirname "$LCOV")"

  flags=(--workspace --all-features --lcov --output-path "$LCOV")
  if [ "$NO_CLEAN" = "1" ]; then
    flags+=(--no-clean)
  fi
  extra=()
  if [ -n "${COVERAGE_CARGO_FLAGS:-}" ]; then
    read -r -a extra <<<"$COVERAGE_CARGO_FLAGS"
  fi

  echo "==> cargo llvm-cov ${flags[*]} ${extra[*]:-}"
  if ! cargo llvm-cov "${flags[@]}" "${extra[@]}"; then
    annotate error \
      "the coverage run failed — no report produced (test failure or build error, see the log above)"
    exit 1
  fi
fi

if [ "$HTML" = "1" ]; then
  echo "==> rendering HTML report"
  # Default output is target/llvm-cov/html/index.html
  cargo llvm-cov report --html ||
    echo "warning: HTML report not generated" >&2
fi

echo "==> enforcing coverage policy ($POLICY)"
python3 scripts/coverage_gate.py --policy "$POLICY" --lcov "$LCOV" "$@"
