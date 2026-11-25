#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

WASM_TARGET="${WASM_TARGET:-wasm32v1-none}"
WASM_DIR="${WASM_DIR:-target/$WASM_TARGET/release}"
OUTPUT_DIR="${OUTPUT_DIR:-interfaces}"
STELLAR="${STELLAR:-stellar}"

if ! command -v "$STELLAR" >/dev/null 2>&1; then
  echo "generate-contract-interfaces: Stellar CLI not found: $STELLAR" >&2
  echo "Install the Stellar CLI and retry." >&2
  exit 1
fi

cargo build --workspace \
  --exclude coralswap-fuzz \
  --exclude coralswap-integration-tests \
  --release \
  --target "$WASM_TARGET"

mkdir -p "$OUTPUT_DIR"

for contract in factory pair lp_token router; do
  wasm="$WASM_DIR/coralswap_$contract.wasm"
  output="$OUTPUT_DIR/$contract.json"

  if [[ ! -f "$wasm" ]]; then
    echo "generate-contract-interfaces: WASM not found: $wasm" >&2
    exit 1
  fi

  "$STELLAR" contract info interface --wasm "$wasm" --output json > "$output"
  echo "Wrote $output"
done