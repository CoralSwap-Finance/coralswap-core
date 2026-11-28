#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    parser.add_argument("--manifest-dir")
    parser.add_argument("--wasm-dir", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--toolchain", required=True)
    parser.add_argument("--cli-version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--contracts", nargs="+", required=True)
    args = parser.parse_args()

    wasm_dir = Path(args.wasm_dir)
    output = Path(args.output)
    contracts = {}

    for name in args.contracts:
        artifact = f"coralswap_{name}.wasm"
        wasm_path = wasm_dir / artifact
        if not wasm_path.is_file():
            raise SystemExit(f"error: optimized WASM artifact not found: {wasm_path}")
        entry = {
            "artifact": artifact,
            "cli_version": args.cli_version,
            "commit": args.commit,
            "profile": "release",
            "sha256": sha256(wasm_path),
            "size_bytes": wasm_path.stat().st_size,
            "target": args.target,
            "toolchain": args.toolchain,
        }
        contracts[name] = entry
        if args.manifest_dir:
            write_json(Path(args.manifest_dir) / f"{name}.json", {"contract": name, **entry})

    manifest = {
        "cli_version": args.cli_version,
        "commit": args.commit,
        "contracts": contracts,
        "profile": "release",
        "target": args.target,
        "toolchain": args.toolchain,
    }
    write_json(output, manifest)


if __name__ == "__main__":
    main()
