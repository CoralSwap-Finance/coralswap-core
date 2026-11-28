#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
PYTHON="${PYTHON:-python3}"

if [ "$#" -gt 2 ]; then
  echo "usage: $0 [results] [budget]" >&2
  exit 2
fi

RESULTS_FILE="${1:-benchmark-results.json}"
BUDGET_FILE="${2:-${SIZE_BUDGET:-scripts/size-budget.json}}"

command -v "$PYTHON" >/dev/null 2>&1 || {
  echo "error: $PYTHON not found on PATH" >&2
  exit 1
}

exec "$PYTHON" scripts/size_gate.py "$RESULTS_FILE" "$BUDGET_FILE"
