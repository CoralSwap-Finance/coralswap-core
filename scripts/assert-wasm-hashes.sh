#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

PYTHON="${PYTHON:-python3}"
export ROOT
export DEPLOYMENTS_FILE="${DEPLOYMENTS_FILE:-deployments.json}"

command -v "$PYTHON" >/dev/null 2>&1 || {
  echo "error: $PYTHON not found on PATH" >&2
  exit 1
}

exec "$PYTHON" scripts/deployment_hash_gate.py
