#!/usr/bin/env python3
import argparse
import json
import sys
from pathlib import Path


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("left")
    parser.add_argument("right")
    args = parser.parse_args()

    left = load(args.left)
    right = load(args.right)
    errors = []

    for field in ("commit", "toolchain", "cli_version", "target", "profile"):
        if left.get(field) != right.get(field):
            errors.append(f"{field}: {left.get(field)!r} != {right.get(field)!r}")

    left_contracts = left.get("contracts", {})
    right_contracts = right.get("contracts", {})
    if left_contracts.keys() != right_contracts.keys():
        errors.append(
            "contract set: "
            f"{sorted(left_contracts)!r} != {sorted(right_contracts)!r}"
        )

    for name in sorted(left_contracts.keys() & right_contracts.keys()):
        for field in ("sha256", "size_bytes", "artifact"):
            if left_contracts[name].get(field) != right_contracts[name].get(field):
                errors.append(
                    f"{name}.{field}: "
                    f"{left_contracts[name].get(field)!r} != {right_contracts[name].get(field)!r}"
                )

    if errors:
        print("Release manifests differ:", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        raise SystemExit(1)

    print(f"Release manifests match across {len(left_contracts)} contract(s).")


if __name__ == "__main__":
    main()
