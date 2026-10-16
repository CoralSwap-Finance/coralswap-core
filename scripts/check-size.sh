#!/usr/bin/env bash
# Enforces the per-contract WASM size budget recorded in scripts/size-budget.json
# against the measurements produced by scripts/benchmark.sh.
#
# Contract size drives upload/deployment cost and the per-invocation resource
# budget on Soroban, and a contract above the network's 64 KiB (65536 byte) WASM
# limit can no longer be deployed at all. CI therefore fails the build instead of
# letting the binaries creep up silently.
#
# Usage:
#   ./scripts/benchmark.sh            # writes benchmark-results.json
#   ./scripts/check-size.sh [results] [budget]
#
# `results` defaults to benchmark-results.json, `budget` to
# scripts/size-budget.json (or $SIZE_BUDGET when set).
#
# Exit codes: 0 = every contract within budget, 1 = at least one breach.

set -euo pipefail

cd "$(dirname "$0")/.."

RESULTS_FILE="${1:-benchmark-results.json}"
BUDGET_FILE="${2:-${SIZE_BUDGET:-scripts/size-budget.json}}"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required to run $0" >&2
  exit 1
fi

if [ ! -f "$RESULTS_FILE" ]; then
  echo "error: measurements not found: $RESULTS_FILE (run ./scripts/benchmark.sh first)" >&2
  exit 1
fi

if [ ! -f "$BUDGET_FILE" ]; then
  echo "error: size budget not found: $BUDGET_FILE" >&2
  exit 1
fi

# Fraction of a contract's budget at which we start warning about headroom.
WARN_RATIO=$(jq -r '.warn_ratio // 0.8' "$BUDGET_FILE")

annotate() {
  # annotate <level> <title> <message>
  if [ "${GITHUB_ACTIONS:-}" = "true" ]; then
    echo "::$1 title=WASM size budget::$3"
  fi
}

echo "Contract size budget ($BUDGET_FILE):"
echo "  contract    size        budget     used"
echo "  ---------   ---------   ---------  ------"

status=0
for name in $(jq -r '.contracts | keys[]' "$BUDGET_FILE"); do
  max_bytes=$(jq -r --arg name "$name" '.contracts[$name].max_bytes' "$BUDGET_FILE")

  if ! size_bytes=$(jq -e --arg name "$name" '.contracts[$name].size_bytes' "$RESULTS_FILE"); then
    echo "  $name: no measurement in $RESULTS_FILE"
    annotate error "missing measurement" \
      "$name has a size budget of $max_bytes bytes but no measurement in $RESULTS_FILE"
    status=1
    continue
  fi

  if [ "$size_bytes" -gt "$max_bytes" ]; then
    over=$((size_bytes - max_bytes))
    used=$(awk -v s="$size_bytes" -v m="$max_bytes" 'BEGIN { printf "%.1f%%", (s * 100) / m }')
    echo "  $name: $size_bytes bytes exceeds its budget of $max_bytes bytes (+$over, $used)"
    annotate error "size budget exceeded" \
      "$name is $size_bytes bytes, $over bytes over its $max_bytes byte budget"
    status=1
    continue
  fi

  warn_bytes=$(awk -v m="$max_bytes" -v r="$WARN_RATIO" 'BEGIN { printf "%d", m * r }')
  used=$(awk -v s="$size_bytes" -v m="$max_bytes" 'BEGIN { printf "%.1f%%", (s * 100) / m }')
  echo "  $name      $size_bytes  $max_bytes  $used"

  if [ "$size_bytes" -gt "$warn_bytes" ]; then
    annotate warning "limited headroom" \
      "$name is $size_bytes bytes, above ${WARN_RATIO} of its $max_bytes byte budget"
  fi
done

if [ "$status" -ne 0 ]; then
  echo "error: contract size budget exceeded — see the table above" >&2
  exit "$status"
fi

echo "All contracts are within budget."
