#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
PYTHON="${PYTHON:-python3}"

command -v "$PYTHON" >/dev/null 2>&1 || {
  echo "error: $PYTHON not found on PATH" >&2
  exit 1
}

exec "$PYTHON" scripts/test_gates.py
