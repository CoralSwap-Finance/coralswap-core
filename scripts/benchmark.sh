#!/usr/bin/env bash
# Measures the compiled wasm size of each deployable contract and writes the
# result to benchmark-results.json, consumed by the "Contract Size Benchmark"
# CI workflow (.github/workflows/benchmark.yml) to flag size regressions on
# PRs. Contract size directly affects deployment cost and the per-invocation
# resource budget on Soroban, so this is tracked the same way build/test are.

set -euo pipefail

cd "$(dirname "$0")/.."

WASM_TARGET="wasm32v1-none"
OUTPUT_FILE="benchmark-results.json"

# contract short-name -> crate directory
declare -A CONTRACTS=(
  [factory]="factory"
  [lp_token]="lp_token"
  [pair]="pair"
  [router]="router"
)

echo "Building contracts for --target $WASM_TARGET (release)..."
cargo build --release --target "$WASM_TARGET" \
  -p coralswap-factory \
  -p coralswap-lp-token \
  -p coralswap-pair \
  -p coralswap-router

WASM_DIR="target/$WASM_TARGET/release"

result="{}"
for name in "${!CONTRACTS[@]}"; do
  crate_dir="${CONTRACTS[$name]}"
  crate_name=$(grep -m1 '^name' "contracts/$crate_dir/Cargo.toml" | sed -E 's/^name = "(.*)"$/\1/')
  wasm_file="$WASM_DIR/${crate_name//-/_}.wasm"

  if [ ! -f "$wasm_file" ]; then
    echo "error: expected wasm artifact not found: $wasm_file" >&2
    exit 1
  fi

  size_bytes=$(stat -c%s "$wasm_file" 2>/dev/null || stat -f%z "$wasm_file")
  echo "  $name: $size_bytes bytes ($wasm_file)"
  result=$(echo "$result" | jq --arg name "$name" --argjson size "$size_bytes" \
    '.contracts[$name] = {size_bytes: $size}')
done

echo "$result" | jq '.' > "$OUTPUT_FILE"
echo "Wrote $OUTPUT_FILE"
