#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
. scripts/release.env

OUTPUT_DIR="${OUTPUT_DIR:-release}"
OUTPUT_FILE="${OUTPUT_FILE:-benchmark-results.json}"
PYTHON="${PYTHON:-python3}"
BUILD_RELEASE="${BUILD_RELEASE:-$ROOT/scripts/build-release-container.sh}"
BUILD_RELEASE_SHELL="${BUILD_RELEASE_SHELL:-bash}"

command -v "$PYTHON" >/dev/null 2>&1 || {
  echo "error: $PYTHON not found on PATH" >&2
  exit 1
}

echo "==> building optimized release artifacts"
OUTPUT_DIR="$OUTPUT_DIR" TARGET_DIR="$OUTPUT_DIR/target" "$BUILD_RELEASE_SHELL" "$BUILD_RELEASE"

"$PYTHON" scripts/benchmark_results.py \
  --manifest "$OUTPUT_DIR/release-manifest.json" \
  --output "$OUTPUT_FILE"
