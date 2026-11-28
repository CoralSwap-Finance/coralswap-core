#!/usr/bin/env python3
import argparse
import json
import os
import sys
from pathlib import Path


def annotate(level, message):
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level} title=WASM size budget::{message}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("results")
    parser.add_argument("budget")
    args = parser.parse_args()

    results_path = Path(args.results)
    budget_path = Path(args.budget)
    if not results_path.is_file():
        print(f"error: measurements not found: {results_path}", file=sys.stderr)
        return 1
    if not budget_path.is_file():
        print(f"error: size budget not found: {budget_path}", file=sys.stderr)
        return 1

    results = json.loads(results_path.read_text(encoding="utf-8"))
    budget = json.loads(budget_path.read_text(encoding="utf-8"))
    warn_ratio = float(budget.get("warn_ratio", 0.8))
    measurements = results.get("contracts", {})
    contracts = budget.get("contracts", {})

    print(f"Contract size budget ({budget_path}):")
    print("  contract    size        budget     used")
    print("  ---------   ---------   ---------  ------")

    status = 0
    for name, policy in contracts.items():
        max_bytes = int(policy["max_bytes"])
        measurement = measurements.get(name)
        if not isinstance(measurement, dict) or "size_bytes" not in measurement:
            print(f"  {name}: no measurement in {results_path}")
            annotate("error", f"{name} has a size budget of {max_bytes} bytes but no measurement in {results_path}")
            status = 1
            continue

        size_bytes = int(measurement["size_bytes"])
        used = (size_bytes * 100) / max_bytes
        if size_bytes > max_bytes:
            over = size_bytes - max_bytes
            print(f"  {name}: {size_bytes} bytes exceeds its budget of {max_bytes} bytes (+{over}, {used:.1f}%)")
            annotate("error", f"{name} is {size_bytes} bytes, {over} bytes over its {max_bytes} byte budget")
            status = 1
            continue

        print(f"  {name:<10}  {size_bytes:<10} {max_bytes:<10} {used:.1f}%")
        if size_bytes > max_bytes * warn_ratio:
            annotate(
                "warning",
                f"{name} is {size_bytes} bytes, above {warn_ratio} of its {max_bytes} byte budget",
            )

    if status:
        print("error: contract size budget exceeded — see the table above", file=sys.stderr)
        return 1

    print("All contracts are within budget.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
