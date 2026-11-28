#!/usr/bin/env python3
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HASH_RE = re.compile(r"^[a-fA-F0-9]{64}$")
COMMIT_FIELDS = ("commit", "git_commit", "gitCommit", "revision")
HASH_FIELDS = ("wasm_hash", "wasmHash", "hash")
METADATA_FIELDS = ("address", "deployedAt", "deployed_at", "wasm", "name")


def normalize_name(name):
    value = str(name).lower().replace("-", "_")
    if value.endswith(".wasm"):
        value = value[:-5]
    if value.startswith("coralswap_"):
        value = value[len("coralswap_") :]
    return value


def record_hash(record, source):
    if isinstance(record, str):
        value = record.strip().lower()
        return (value, None) if HASH_RE.fullmatch(value) else (None, f"{source}: invalid wasm hash {record!r}")
    if not isinstance(record, dict):
        return None, f"{source}: unsupported deployment record {record!r}"
    for field in HASH_FIELDS:
        value = record.get(field)
        if isinstance(value, str) and HASH_RE.fullmatch(value.strip()):
            return value.strip().lower(), None
    return None, f"{source}: no 64-character wasm hash recorded"


def record_commit(record):
    if not isinstance(record, dict):
        return None
    for field in COMMIT_FIELDS:
        value = record.get(field)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def collect_targets(data):
    targets = []
    errors = []

    def add(contract, record, source):
        expected, error = record_hash(record, source)
        if error:
            errors.append(error)
            return
        commit = record_commit(record)
        targets.append(
            {
                "commit": commit,
                "contract": normalize_name(contract),
                "expected": expected,
                "source": source,
            }
        )

    for outer_name, outer_value in data.items():
        if isinstance(outer_value, dict) and record_commit(outer_value):
            top_hash, _ = record_hash(outer_value, outer_name)
            if top_hash:
                add(outer_name, outer_value, outer_name)
            for contract, record in outer_value.items():
                if contract in ("wasm_hashes", "wasmHashes") and isinstance(record, dict):
                    for nested_name, nested in record.items():
                        add(nested_name, nested, f"{outer_name}.{nested_name}")
                    continue
                if contract in COMMIT_FIELDS + HASH_FIELDS + METADATA_FIELDS or not isinstance(record, (str, dict)):
                    continue
                add(contract, record, f"{outer_name}.{contract}")
            continue

        for contract, record in outer_value.items() if isinstance(outer_value, dict) else []:
            if contract in ("wasm_hashes", "wasmHashes") and isinstance(record, dict):
                for nested_name, nested in record.items():
                    add(nested_name, nested, f"{outer_name}.{nested_name}")
                continue
            if contract in COMMIT_FIELDS + HASH_FIELDS + METADATA_FIELDS:
                continue
            add(contract, record, f"{outer_name}.{contract}")

    return targets, errors


def load_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def manifest_from_file(path, commit):
    manifest = load_json(path)
    manifest_commit = manifest.get("commit")
    if manifest_commit != commit:
        return None, f"manifest {path} has commit {manifest_commit!r}, expected {commit!r}"
    return manifest, None


def manifest_from_dir(directory, commit):
    candidates = sorted(Path(directory).glob("*.json"))
    for candidate in candidates:
        manifest = load_json(candidate)
        if manifest.get("commit") == commit:
            return manifest, None
    return None, f"no manifest for commit {commit} in {directory}"


def build_manifest(root, commit):
    root = Path(root).resolve()
    check = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--verify", f"{commit}^{{commit}}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if check.returncode != 0:
        return None, f"unknown recorded commit {commit}: {check.stderr.strip()}"

    import tempfile

    with tempfile.TemporaryDirectory(prefix="coralswap-hash-gate-") as temp:
        worktree = Path(temp) / "source"
        output = Path(temp) / "release"
        result = subprocess.run(
            ["git", "-C", str(root), "worktree", "add", "--detach", str(worktree), commit],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if result.returncode != 0:
            return None, f"cannot check out {commit}: {result.stderr.strip()}"
        try:
            env = os.environ.copy()
            env.update(
                {
                    "OUTPUT_DIR": str(output),
                    "TARGET_DIR": str(Path(temp) / "target"),
                    "CLEAN_TARGET": "1",
                }
            )
            build_release = env.get("BUILD_RELEASE", str(root / "scripts/build-release-container.sh"))
            shell = env.get("BUILD_RELEASE_SHELL", "bash")
            result = subprocess.run(
                [shell, build_release],
                cwd=worktree,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            if result.returncode != 0:
                return None, f"release build failed for {commit}:\n{result.stdout}"
            manifest = load_json(output / "release-manifest.json")
        finally:
            subprocess.run(
                ["git", "-C", str(root), "worktree", "remove", "--force", str(worktree)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

    if manifest.get("commit") != commit:
        return None, f"release manifest commit {manifest.get('commit')!r}, expected {commit!r}"
    return manifest, None


def main():
    root = Path(os.environ.get("ROOT", os.getcwd())).resolve()
    deployments_file = Path(os.environ.get("DEPLOYMENTS_FILE", "deployments.json"))
    if not deployments_file.is_absolute():
        deployments_file = root / deployments_file

    if not deployments_file.is_file():
        print(f"assert-wasm-hashes: deployments file not found: {deployments_file}", file=sys.stderr)
        return 1

    data = load_json(deployments_file)
    targets, errors = collect_targets(data)

    if not targets and not errors:
        print(f"assert-wasm-hashes: no pinned deployment hashes in {deployments_file}; skipping.")
        return 0
    if errors:
        for error in errors:
            print(f"[ERROR] {error}", file=sys.stderr)
        return 1

    manifests = {}
    failures = []
    for target in targets:
        commit = target["commit"]
        if not commit:
            failures.append(f"{target['source']}: recorded commit is missing")
            continue
        if commit not in manifests:
            manifest = None
            error = None
            if os.environ.get("RELEASE_MANIFEST"):
                manifest, error = manifest_from_file(os.environ["RELEASE_MANIFEST"], commit)
            elif os.environ.get("RELEASE_MANIFEST_DIR"):
                manifest, error = manifest_from_dir(os.environ["RELEASE_MANIFEST_DIR"], commit)
            else:
                manifest, error = build_manifest(root, commit)
            if error:
                failures.append(error)
                manifests[commit] = None
                continue
            manifests[commit] = manifest

        manifest = manifests[commit]
        if manifest is None:
            continue
        entry = manifest.get("contracts", {}).get(target["contract"])
        if not entry:
            failures.append(
                f"{target['source']}: contract {target['contract']!r} missing from release manifest"
            )
            continue
        actual = entry.get("sha256", "").lower()
        if actual != target["expected"]:
            failures.append(
                f"{target['source']}: hash drift for {target['contract']} at {commit}\n"
                f"  expected: {target['expected']}\n"
                f"  actual:   {actual}"
            )
        else:
            print(f"[OK] {target['source']} ({commit[:12]}) matches artifact {entry['artifact']}")

    if failures:
        print("\nassert-wasm-hashes: deployment hash assertion failed.", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1

    print(f"\nassert-wasm-hashes: all {len(targets)} deployment hash assertion(s) passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
