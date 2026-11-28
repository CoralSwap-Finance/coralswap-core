#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
. scripts/release.env

IMAGE="${RELEASE_IMAGE_TAG:-coralswap-release:${STELLAR_CLI_VERSION}-${RUST_TOOLCHAIN}}"
OUTPUT_DIR="${OUTPUT_DIR:-release}"
TARGET_DIR="${TARGET_DIR:-target}"
CLEAN_TARGET="${CLEAN_TARGET:-0}"

die() {
  echo "error: $*" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || die "docker not found on PATH"

echo "==> building release image $IMAGE"
docker build \
  -f Dockerfile.release \
  --build-arg RUST_IMAGE="$RUST_IMAGE" \
  --build-arg STELLAR_CLI_VERSION="$STELLAR_CLI_VERSION" \
  -t "$IMAGE" \
  "$ROOT"

echo "==> running release build in $IMAGE"
docker run --rm \
  -v "$ROOT:/workspace" \
  -e "OUTPUT_DIR=/workspace/$OUTPUT_DIR" \
  -e "TARGET_DIR=/workspace/$TARGET_DIR" \
  -e "CLEAN_TARGET=$CLEAN_TARGET" \
  -e "PYTHON=python3" \
  "$IMAGE" \
  bash scripts/build-release.sh
