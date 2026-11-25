#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
. scripts/release.env

OUTPUT_DIR="${OUTPUT_DIR:-release}"
OPTIMIZED_DIR="$OUTPUT_DIR/wasm"
MANIFEST_DIR="$OUTPUT_DIR/manifests"
MANIFEST_FILE="$OUTPUT_DIR/release-manifest.json"
TARGET_DIR="${TARGET_DIR:-target}"
CLEAN_TARGET="${CLEAN_TARGET:-0}"
PYTHON="${PYTHON:-python3}"
STELLAR="${STELLAR:-stellar}"

die() {
  echo "error: $*" >&2
  exit 1
}

command -v cargo >/dev/null 2>&1 || die "cargo not found on PATH"
command -v "$STELLAR" >/dev/null 2>&1 || die "$STELLAR not found on PATH"
command -v "$PYTHON" >/dev/null 2>&1 || die "$PYTHON not found on PATH"

if [ "$CLEAN_TARGET" = "1" ]; then
  rm -rf "$TARGET_DIR"
fi

cli_version=$("$STELLAR" --version | awk 'NR == 1 { print $2 }')
[ "$cli_version" = "$STELLAR_CLI_VERSION" ] || die "expected stellar CLI $STELLAR_CLI_VERSION, found $cli_version"

commit=$(git rev-parse HEAD)

rm -rf "$OUTPUT_DIR"
mkdir -p "$OPTIMIZED_DIR" "$MANIFEST_DIR"

echo "==> building release WASM artifacts for $commit"
export CARGO_TARGET_DIR="$TARGET_DIR"
WASM_DIR="$TARGET_DIR/$WASM_TARGET/release"
cargo build --locked --workspace --exclude coralswap-fuzz --exclude coralswap-integration-tests --release --target "$WASM_TARGET"

echo "==> optimizing release WASM artifacts with stellar CLI $cli_version"
for name in $RELEASE_CONTRACTS; do
  artifact="coralswap_${name}.wasm"
  source="$WASM_DIR/$artifact"
  destination="$OPTIMIZED_DIR/$artifact"
  [ -f "$source" ] || die "expected WASM artifact not found: $source"
  "$STELLAR" contract optimize --wasm "$source" --wasm-out "$destination"
done

"$PYTHON" scripts/release_manifest.py \
  --output "$MANIFEST_FILE" \
  --manifest-dir "$MANIFEST_DIR" \
  --wasm-dir "$OPTIMIZED_DIR" \
  --commit "$commit" \
  --toolchain "$RUST_TOOLCHAIN" \
  --cli-version "$cli_version" \
  --target "$WASM_TARGET" \
  --contracts $RELEASE_CONTRACTS

echo "Wrote $MANIFEST_FILE"
