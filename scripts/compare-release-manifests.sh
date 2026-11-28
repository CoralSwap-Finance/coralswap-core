#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
PYTHON="${PYTHON:-python3}"

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <manifest-a> <manifest-b>" >&2
  exit 2
fi

exec "$PYTHON" scripts/compare-release-manifests.py "$1" "$2"
