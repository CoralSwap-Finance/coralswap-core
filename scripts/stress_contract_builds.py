#!/usr/bin/env python3
"""Relink every workspace cdylib twice in seeded random order (Python 3.11+)."""

import argparse
import json
import random
import secrets
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TARGET = "wasm32v1-none"


def contract_packages(metadata):
    """Discover deployable targets, including future workspace members."""
    members = set(metadata["workspace_members"])
    packages = sorted(
        package["name"]
        for package in metadata["packages"]
        if package["id"] in members
        and any("cdylib" in target["crate_types"] for target in package["targets"])
    )
    if not packages:
        raise ValueError("No workspace cdylib contract targets found")
    return packages


def build_orders(packages, seed):
    """Visit every target once per pass, with distinct orders when possible."""
    rng = random.Random(seed)
    first = list(packages)
    second = list(packages)
    rng.shuffle(first)
    rng.shuffle(second)
    if len(first) > 1 and second == first:
        second = second[1:] + second[:1]
    return first, second


def stress_build(packages, seed, target_dir, run=subprocess.run):
    """Clean each root artifact before building; cached dependencies may remain."""
    for pass_number, order in enumerate(build_orders(packages, seed), start=1):
        print(f"Pass {pass_number}/2: {', '.join(order)}", flush=True)
        for package in order:
            common = [
                "--package", package, "--release", "--target", TARGET,
                "--target-dir", str(target_dir),
            ]
            # Without this, the second pass can be entirely Fresh and never
            # exercise rustc/wasm-ld. Only this isolated stress target is cleaned.
            run(["cargo", "clean", *common], cwd=ROOT, check=True)
            run(["cargo", "build", "--locked", "--lib", *common], cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, help="Replay the seed printed by a previous run")
    args = parser.parse_args()
    seed = args.seed if args.seed is not None else secrets.randbits(64)
    print(f"Contract build stress seed: {seed}", flush=True)
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout)
    packages = contract_packages(metadata)
    # Keep stress artifacts separate from WASM fixtures used by native tests.
    target_dir = ROOT / "target" / "contract-build-stress"
    stress_build(packages, seed, target_dir)
    print(f"Passed: {len(packages)} contracts, 2 builds each (seed {seed})", flush=True)


if __name__ == "__main__":
    main()
