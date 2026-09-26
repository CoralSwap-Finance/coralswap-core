#!/usr/bin/env bash
# Asserts that built release WASM artifact hashes match recorded deployment metadata.
# Used in CI to prevent config/code drift between deployments.json / soroban-deploy.toml
# and compiled contract bytecode.

set -euo pipefail

cd "$(dirname "$0")/.."

WASM_TARGET="${WASM_TARGET:-wasm32v1-none}"
export WASM_DIR="${WASM_DIR:-target/$WASM_TARGET/release}"
export DEPLOYMENTS_FILE="${DEPLOYMENTS_FILE:-deployments.json}"
export CONFIG_FILE="${CONFIG_FILE:-soroban-deploy.toml}"

python3 - << 'EOF'
import hashlib
import json
import os
import re
import sys

wasm_dir = os.environ.get("WASM_DIR", "target/wasm32v1-none/release")
deployments_file = os.environ.get("DEPLOYMENTS_FILE", "deployments.json")
config_file = os.environ.get("CONFIG_FILE", "soroban-deploy.toml")

def normalize_name(name):
    return name.lower().replace("-", "_")

def find_wasm_file(contract_name):
    norm = normalize_name(contract_name)
    candidates = [
        f"coralswap_{norm}.wasm",
        f"{norm}.wasm",
        f"coralswap-{contract_name}.wasm",
    ]
    for c in candidates:
        path = os.path.join(wasm_dir, c)
        if os.path.isfile(path):
            return path
    return None

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest().lower()

# Gather expected hashes: list of (source, network, contract, expected_hash)
targets = []

# 1. Parse deployments.json
if os.path.isfile(deployments_file):
    try:
        with open(deployments_file, "r") as f:
            data = json.load(f)
    except Exception as e:
        print(f"Error parsing {deployments_file}: {e}", file=sys.stderr)
        sys.exit(1)

    if isinstance(data, dict):
        for net_or_contract, val in data.items():
            if isinstance(val, dict):
                # Could be a network section or a contract section
                for sub_key, sub_val in val.items():
                    if sub_key in ("wasm_hashes", "wasmHashes") and isinstance(sub_val, dict):
                        for c_name, h in sub_val.items():
                            if isinstance(h, str) and len(h.strip()) == 64:
                                targets.append((deployments_file, net_or_contract, c_name, h.strip().lower()))
                    elif isinstance(sub_val, dict):
                        h = (
                            sub_val.get("wasm_hash")
                            or sub_val.get("wasmHash")
                            or sub_val.get("hash")
                        )
                        if h and isinstance(h, str) and len(h.strip()) == 64:
                            targets.append((deployments_file, net_or_contract, sub_key, h.strip().lower()))
                    elif isinstance(sub_val, str) and len(sub_val.strip()) == 64:
                        targets.append((deployments_file, net_or_contract, sub_key, sub_val.strip().lower()))
            elif isinstance(val, str) and len(val.strip()) == 64:
                targets.append((deployments_file, "global", net_or_contract, val.strip().lower()))

# 2. Check soroban-deploy.toml (if any wasm_hash defined)
if os.path.isfile(config_file):
    try:
        with open(config_file, "r") as f:
            lines = f.readlines()
        current_contract = None
        for line in lines:
            line = line.strip()
            sec_match = re.match(r"^\[contracts\.([a-zA-Z0-9_\-]+)\]", line)
            if sec_match:
                current_contract = sec_match.group(1)
            hash_match = re.match(r'^(?:wasm_hash|hash)\s*=\s*["\']([a-fA-F0-9]{64})["\']', line)
            if hash_match and current_contract:
                targets.append((config_file, "config", current_contract, hash_match.group(1).lower()))
    except Exception as e:
        print(f"Warning reading {config_file}: {e}", file=sys.stderr)

if not targets:
    print(f"assert-wasm-hashes: No WASM hashes found in {deployments_file} or {config_file}.")
    print("No deployment pinned hashes to verify against built artifacts. Assertion skipped.")
    sys.exit(0)

print(f"assert-wasm-hashes: Verifying {len(targets)} pinned WASM hash(es) against built artifacts...")
errors = 0
for source, network, contract, expected in targets:
    wasm_path = find_wasm_file(contract)
    if not wasm_path:
        print(f"[ERROR] {source} ({network}.{contract}): Built WASM artifact not found in {wasm_dir}")
        errors += 1
        continue

    actual = compute_sha256(wasm_path)
    if actual != expected:
        print(f"[DRIFT DETECTED] {source} ({network}.{contract}):")
        print(f"  Expected: {expected}")
        print(f"  Actual:   {actual}")
        print(f"  Artifact: {wasm_path}")
        errors += 1
    else:
        print(f"[OK] {network}.{contract} matches artifact ({wasm_path}): {actual}")

if errors > 0:
    print(f"\nAssertion failed: {errors} WASM hash drift(s) detected.", file=sys.stderr)
    sys.exit(1)

print(f"\nAll {len(targets)} WASM hash assertions passed successfully.")
sys.exit(0)
EOF
