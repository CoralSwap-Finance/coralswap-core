#!/usr/bin/env python3
import argparse
import hashlib
import json
import sys
from pathlib import Path


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--release-dir", required=True)
    parser.add_argument("--manifest", required=True)
    args = parser.parse_args()

    release_dir = Path(args.release_dir)
    local_manifest = load(args.manifest)
    attached_manifest_path = release_dir / "release-manifest.json"
    if not attached_manifest_path.is_file():
        print(f"error: attached release manifest not found: {attached_manifest_path}", file=sys.stderr)
        return 1
    attached_manifest = load(attached_manifest_path)

    if attached_manifest != local_manifest:
        print("error: attached release manifest differs from the rebuilt release manifest", file=sys.stderr)
        return 1

    errors = []
    for name, entry in local_manifest.get("contracts", {}).items():
        artifact = release_dir / entry["artifact"]
        if not artifact.is_file():
            errors.append(f"{name}: missing attached artifact {entry['artifact']}")
            continue
        actual = sha256(artifact)
        if actual != entry["sha256"]:
            errors.append(f"{name}: attached hash {actual} does not match {entry['sha256']}")
        if artifact.stat().st_size != entry["size_bytes"]:
            errors.append(f"{name}: attached size {artifact.stat().st_size} does not match {entry['size_bytes']}")

    if errors:
        print("error: release asset verification failed.", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1

    print(f"Release assets match the rebuilt release for {len(local_manifest['contracts'])} contract(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
