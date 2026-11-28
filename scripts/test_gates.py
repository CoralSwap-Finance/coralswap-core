#!/usr/bin/env python3
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PYTHON = sys.executable
BASH = os.environ.get("TEST_BASH", "bash")
COMMIT = "a" * 40
HASH = "b" * 64
WRONG_HASH = "c" * 64


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def manifest(hash_value, commit=COMMIT):
    return {
        "cli_version": "27.0.0",
        "commit": commit,
        "contracts": {
            "factory": {
                "artifact": "coralswap_factory.wasm",
                "sha256": hash_value,
                "size_bytes": 42272,
            }
        },
        "profile": "release",
        "target": "wasm32v1-none",
        "toolchain": "1.94.1",
    }


def run(script, args=(), env=None):
    merged = os.environ.copy()
    merged.update({"PYTHON": PYTHON, "ROOT": str(ROOT)})
    if env:
        merged.update(env)
    else:
        merged.pop("RELEASE_MANIFEST", None)
        merged.pop("RELEASE_MANIFEST_DIR", None)
    return subprocess.run(
        [BASH, script, *args],
        cwd=ROOT,
        env=merged,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def expect(label, result, success):
    passed = result.returncode == 0 if success else result.returncode != 0
    status = "PASS" if passed else "FAIL"
    print(f"[{status}] {label}")
    if not passed:
        print(result.stdout)
    return passed


def main():
    ok = True
    with tempfile.TemporaryDirectory(prefix="coralswap-script-tests-") as temp:
        temp = Path(temp)
        budget = ROOT / "scripts/size-budget.json"

        passing_results = temp / "results-pass.json"
        failing_results = temp / "results-fail.json"
        write_json(passing_results, {"contracts": {"factory": {"size_bytes": 42272}, "lp_token": {"size_bytes": 18063}, "pair": {"size_bytes": 72114}, "router": {"size_bytes": 48938}, "staking": {"size_bytes": 14923}}})
        write_json(failing_results, {"contracts": {"factory": {"size_bytes": 42272}, "lp_token": {"size_bytes": 18063}, "pair": {"size_bytes": 90000}, "router": {"size_bytes": 48938}, "staking": {"size_bytes": 14923}}})

        ok &= expect(
            "check-size.sh passes within tightened budgets",
            run("scripts/check-size.sh", [str(passing_results), str(budget)]),
            True,
        )
        ok &= expect(
            "check-size.sh fails over tightened budget",
            run("scripts/check-size.sh", [str(failing_results), str(budget)]),
            False,
        )

        deployments = temp / "deployments.json"
        write_json(deployments, {"testnet": {"factory": {"commit": COMMIT, "wasm_hash": HASH}}})

        passing_manifest = temp / "manifest.json"
        failing_manifest = temp / "manifest-wrong.json"
        write_json(passing_manifest, manifest(HASH))
        write_json(failing_manifest, manifest(WRONG_HASH))

        ok &= expect(
            "hash gate passes when recorded commit rebuild matches",
            run(
                "scripts/assert-wasm-hashes.sh",
                env={"DEPLOYMENTS_FILE": str(deployments), "RELEASE_MANIFEST": str(passing_manifest)},
            ),
            True,
        )
        ok &= expect(
            "hash gate fails when recorded hash does not match rebuild",
            run(
                "scripts/assert-wasm-hashes.sh",
                env={"DEPLOYMENTS_FILE": str(deployments), "RELEASE_MANIFEST": str(failing_manifest)},
            ),
            False,
        )

        manifest_dir = temp / "manifests"
        manifest_dir.mkdir()
        write_json(manifest_dir / "release.json", manifest(HASH))
        ok &= expect(
            "hash gate uses recorded-commit manifest on changed PR code",
            run(
                "scripts/assert-wasm-hashes.sh",
                env={"DEPLOYMENTS_FILE": str(deployments), "RELEASE_MANIFEST_DIR": str(manifest_dir)},
            ),
            True,
        )

        missing_commit = temp / "deployments-missing-commit.json"
        write_json(missing_commit, {"testnet": {"factory": {"wasm_hash": HASH}}})
        ok &= expect(
            "hash gate fails when deployment has no recorded commit",
            run(
                "scripts/assert-wasm-hashes.sh",
                env={"DEPLOYMENTS_FILE": str(missing_commit), "RELEASE_MANIFEST": str(passing_manifest)},
            ),
            False,
        )

        left = temp / "manifest-a.json"
        right = temp / "manifest-b.json"
        different = temp / "manifest-c.json"
        write_json(left, manifest(HASH))
        write_json(right, manifest(HASH))
        write_json(different, manifest(WRONG_HASH))
        ok &= expect(
            "release manifests with matching hashes compare equal",
            run("scripts/compare-release-manifests.sh", [str(left), str(right)]),
            True,
        )
        ok &= expect(
            "release manifests with different hashes fail comparison",
            run("scripts/compare-release-manifests.sh", [str(left), str(different)]),
            False,
        )

        assets = temp / "assets"
        assets.mkdir()
        payload = b"fixture wasm"
        artifact = assets / "coralswap_factory.wasm"
        artifact.write_bytes(payload)
        asset_manifest = manifest(hashlib.sha256(payload).hexdigest())
        asset_manifest["contracts"]["factory"]["size_bytes"] = len(payload)
        write_json(assets / "release-manifest.json", asset_manifest)
        local_asset_manifest = temp / "asset-release-manifest.json"
        write_json(local_asset_manifest, asset_manifest)

        def verify_assets():
            return subprocess.run(
                [
                    PYTHON,
                    "scripts/verify-release-assets.py",
                    "--release-dir",
                    str(assets),
                    "--manifest",
                    str(local_asset_manifest),
                ],
                cwd=ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )

        ok &= expect("release asset verification passes for matching artifacts", verify_assets(), True)
        artifact.write_bytes(payload + b"x")
        ok &= expect("release asset verification fails for changed artifacts", verify_assets(), False)

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
