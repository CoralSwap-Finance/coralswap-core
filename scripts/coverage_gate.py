#!/usr/bin/env python3
"""Coverage floor gate for the CoralSwap workspace.

Reads the LCOV report produced by `cargo llvm-cov --lcov` together with the
policy in `coverage.toml` and decides whether every workspace member meets its
line-coverage floor. Emits a human-readable report, a GitHub Actions job
summary and `::error` / `::warning` annotations, then exits non-zero when the
policy is breached so the PR fails.

Policy (coverage.toml):

  [measurement]      exclude globs for sources that are not production code
  [floors]           <member> = minimum line coverage in percent
  [no_floor.<m>]     reason = why <m> cannot have a floor (test harnesses ...)
  [[exemptions]]     allowlist entry for an uncovered-but-justified region:
                     path + lines + reason + a mandatory `# expires: YYYY-MM-DD`
                     comment inside the entry.

Exit codes:
  0  every member at or above its floor, no expired exemption
  1  coverage floor breached or an exemption past its expiry date
  2  bad policy / unusable report (nothing meaningful was measured)
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import json
import os
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

EXIT_OK = 0
EXIT_VIOLATION = 1
EXIT_USAGE = 2

# Exemptions this close to their expiry date produce a warning annotation.
EXPIRY_WARN_DAYS = 14

RANGE_RE = re.compile(r"^\s*(\d+)(?:\s*-\s*(\d+))?\s*$")
EXPIRY_RE = re.compile(r"#\s*expires:\s*(\d{4}-\d{2}-\d{2})")
EXEMPTION_HEADER_RE = re.compile(r"(?m)^\s*\[\[exemptions\]\][^\n]*$")


class PolicyError(Exception):
    """The policy file cannot be parsed or is self-inconsistent."""


@dataclass
class Exemption:
    path: str
    ranges: list[tuple[int, int]]
    reason: str
    expires: dt.date | None
    index: int

    def label(self) -> str:
        return ", ".join(str(a) if a == b else f"{a}-{b}" for a, b in self.ranges)

    def covers(self, line: int) -> bool:
        return any(a <= line <= b for a, b in self.ranges)


@dataclass
class Policy:
    exclude: list[str]
    floors: dict[str, float]
    no_floor: dict[str, str]
    exemptions: list[Exemption] = field(default_factory=list)


@dataclass
class MemberStats:
    covered: int = 0
    total: int = 0
    files: dict[str, tuple[int, int]] = field(default_factory=dict)

    @property
    def percent(self) -> float:
        return 100.0 * self.covered / self.total if self.total else 0.0


# ── policy ──────────────────────────────────────────────────────────────────


def normalize_path(value: str) -> str:
    """Strip whitespace and any leading `./` from a policy source path."""
    text = value.strip()
    while text.startswith("./"):
        text = text[2:]
    return text


def parse_ranges(text: str) -> list[tuple[int, int]]:
    """Parse `"9, 23-24"` into `[(9, 9), (23, 24)]`."""
    ranges: list[tuple[int, int]] = []
    for part in text.split(","):
        if not part.strip():
            continue
        m = RANGE_RE.match(part)
        if not m:
            raise PolicyError(f"cannot parse line range {part!r} in `lines = {text!r}`")
        start = int(m.group(1))
        end = int(m.group(2) or start)
        if end < start:
            raise PolicyError(f"descending line range {start}-{end} in `lines = {text!r}`")
        ranges.append((start, end))
    if not ranges:
        raise PolicyError("`lines` must list at least one line or line range")
    return ranges


def parse_expiries(raw: str, expected: int) -> list[dt.date | None]:
    """Extract the `# expires: YYYY-MM-DD` comment from every `[[exemptions]]`.

    The comment must sit *inside* the entry it belongs to (after its header and
    before the next one) — that is where a TOML comment documents that table.
    """
    blocks = EXEMPTION_HEADER_RE.split(raw)[1:]
    if len(blocks) != expected:
        raise PolicyError(
            "cannot correlate `[[exemptions]]` headers with parsed entries "
            f"({len(blocks)} headers vs {expected} entries)"
        )
    expiries: list[dt.date | None] = []
    for block in blocks:
        matches = EXPIRY_RE.findall(block)
        if not matches:
            expiries.append(None)
            continue
        try:
            expiries.append(dt.date.fromisoformat(matches[-1]))
        except ValueError as exc:
            raise PolicyError(f"invalid `# expires:` date {matches[-1]!r}: {exc}") from None
    return expiries


def load_policy(path: Path) -> Policy:
    if not path.is_file():
        raise PolicyError(f"policy file not found: {path}")
    raw = path.read_text(encoding="utf-8")
    try:
        data = tomllib.loads(raw)
    except tomllib.TOMLDecodeError as exc:
        raise PolicyError(f"{path}: {exc}") from None

    exclude = data.get("measurement", {}).get("exclude", [])
    if not isinstance(exclude, list) or not all(isinstance(g, str) for g in exclude):
        raise PolicyError("`measurement.exclude` must be a list of glob strings")

    floors_raw = data.get("floors", {})
    if not isinstance(floors_raw, dict):
        raise PolicyError("[floors] must be a table of <member> = <percent>")
    floors: dict[str, float] = {}
    for name, value in floors_raw.items():
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise PolicyError(f"floor for {name} must be a percentage, got {value!r}")
        if not 0 <= float(value) <= 100:
            raise PolicyError(f"floor for {name} must be in [0, 100], got {value}")
        floors[name] = float(value)

    no_floor: dict[str, str] = {}
    for name, entry in data.get("no_floor", {}).items():
        if not isinstance(entry, dict) or not str(entry.get("reason", "")).strip():
            raise PolicyError(f"[no_floor.{name}] needs a non-empty `reason`")
        no_floor[name] = str(entry["reason"]).strip()

    if not floors and not no_floor:
        raise PolicyError("policy defines neither [floors] nor [no_floor.*]")

    rows = data.get("exemptions", [])
    if not isinstance(rows, list):
        raise PolicyError("exemptions must be an array of `[[exemptions]]` tables")
    expiries = parse_expiries(raw, len(rows))

    exemptions: list[Exemption] = []
    for i, (row, expires) in enumerate(zip(rows, expiries), start=1):
        if not isinstance(row, dict):
            raise PolicyError(f"exemption #{i} must be a table")
        missing = [k for k in ("path", "lines", "reason") if not str(row.get(k, "")).strip()]
        if missing:
            raise PolicyError(f"exemption #{i} is missing {', '.join(missing)}")
        exemptions.append(
            Exemption(
                path=normalize_path(str(row["path"])),
                ranges=parse_ranges(str(row["lines"])),
                reason=str(row["reason"]).strip(),
                expires=expires,
                index=i,
            )
        )
    return Policy(exclude=exclude, floors=floors, no_floor=no_floor, exemptions=exemptions)


# ── coverage data ───────────────────────────────────────────────────────────


def parse_lcov(path: Path) -> dict[str, dict[int, int]]:
    """Return `{source file: {line: hit count}}` for every `DA:` record."""
    if not path.is_file():
        raise FileNotFoundError(
            f"coverage report not found: {path}\n"
            "run ./scripts/check-coverage.sh first (or set COVERAGE_REUSE=1)"
        )
    files: dict[str, dict[int, int]] = {}
    current: str | None = None
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("SF:"):
            current = line[3:]
            files.setdefault(current, {})
        elif line.startswith("DA:") and current is not None:
            parts = line[3:].split(",")
            if len(parts) < 2:
                continue
            try:
                lineno, count = int(parts[0]), int(parts[1])
            except ValueError:
                continue
            # A line can appear more than once (macro expansion); keep the best hit.
            files[current][lineno] = max(files[current].get(lineno, 0), count)
    return files


def workspace_members(root: Path) -> dict[str, str]:
    """Return `{manifest directory: package name}` for every workspace member."""
    try:
        proc = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=root,
            capture_output=True,
            text=True,
            check=True,
        )
    except FileNotFoundError as exc:
        raise PolicyError("cargo not found on PATH (needed to map sources to members)") from exc
    except subprocess.CalledProcessError as exc:
        raise PolicyError(f"cargo metadata failed: {exc.stderr.strip()}") from None
    meta = json.loads(proc.stdout)
    return {str(Path(p["manifest_path"]).parent): p["name"] for p in meta["packages"]}


def select_sources(
    lcov_files: dict[str, dict[int, int]],
    root: Path,
    dirs: dict[str, str],
    policy: Policy,
) -> tuple[dict[str, dict[int, int]], set[str], list[str]]:
    """Keep workspace sources that are not excluded by `measurement.exclude`.

    Returns (measured sources keyed by repo-relative path, every repo-relative
    path present in the report, warnings for ignored files).
    """
    surviving: dict[str, dict[int, int]] = {}
    seen: set[str] = set()
    ignored: list[str] = []
    for raw_path, lines in sorted(lcov_files.items()):
        path = Path(raw_path)
        if not path.is_absolute():
            path = root / path
        try:
            rel = str(path.resolve().relative_to(root))
        except ValueError:
            ignored.append(raw_path)
            continue
        seen.add(rel)

        owner_dir = None
        for directory in dirs:
            if str(path) == directory or str(path).startswith(directory + os.sep):
                if owner_dir is None or len(directory) > len(owner_dir):
                    owner_dir = directory
        if owner_dir is None:
            ignored.append(rel)
            continue

        member_rel = str(path.relative_to(Path(owner_dir)))
        if any(
            fnmatch.fnmatch(rel, glob) or fnmatch.fnmatch(member_rel, glob)
            for glob in policy.exclude
        ):
            continue
        surviving[rel] = lines

    warnings = []
    if ignored:
        warnings.append(
            "ignored coverage outside the workspace: "
            + ", ".join(sorted(ignored)[:5])
            + (" …" if len(ignored) > 5 else "")
        )
    return surviving, seen, warnings


def apply_exemptions(
    policy: Policy,
    surviving: dict[str, dict[int, int]],
    seen: set[str],
    today: dt.date,
) -> tuple[list[str], list[str], list[str]]:
    """Remove exempt lines from the measurement and validate every entry.

    Returns (violations, usage errors, warnings).
    """
    violations: list[str] = []
    errors: list[str] = []
    warnings: list[str] = []

    for ex in policy.exemptions:
        where = f"exemption #{ex.index} ({ex.path} {ex.label()})"

        if ex.path not in surviving:
            if ex.path in seen:
                errors.append(f"{where}: file is excluded from measurement")
            else:
                errors.append(
                    f"{where}: file not found in the coverage report — wrong path "
                    "or the source no longer exists"
                )
            continue

        matched = [line for line in surviving[ex.path] if ex.covers(line)]
        if not matched:
            errors.append(
                f"{where}: matches no executable line — line numbers drifted, "
                "update or delete the entry"
            )
            continue

        uncovered = [line for line in matched if surviving[ex.path][line] == 0]
        for line in matched:
            del surviving[ex.path][line]

        if ex.expires is None:
            errors.append(
                f"{where}: missing expiry comment — add `# expires: YYYY-MM-DD` "
                "inside the entry"
            )
            continue

        if today >= ex.expires:
            violations.append(
                f"{where}: expired on {ex.expires.isoformat()} — add tests, "
                "delete the dead code, or re-review and move the date forward"
            )
        elif (ex.expires - today).days <= EXPIRY_WARN_DAYS:
            warnings.append(
                f"{where}: expires {ex.expires.isoformat()} "
                f"in {(ex.expires - today).days} day(s)"
            )

        if not uncovered:
            warnings.append(
                f"{where}: every listed line is now covered — drop the exemption"
            )

    return violations, errors, warnings


def compute_stats(
    surviving: dict[str, dict[int, int]], dirs: dict[str, str], root: Path
) -> dict[str, MemberStats]:
    """Attribute every measured source file to its workspace member."""
    stats: dict[str, MemberStats] = {}
    for rel, lines in surviving.items():
        if not lines:
            continue
        absolute = str(root / rel)
        best: tuple[int, str] | None = None
        for directory, member in dirs.items():
            if absolute == directory or absolute.startswith(directory + os.sep):
                if best is None or len(directory) > best[0]:
                    best = (len(directory), member)
        if best is None:
            continue
        member = stats.setdefault(best[1], MemberStats())
        covered = sum(1 for count in lines.values() if count > 0)
        member.covered += covered
        member.total += len(lines)
        member.files[rel] = (covered, len(lines))
    return stats


# ── policy checks ───────────────────────────────────────────────────────────


def check_membership(
    policy: Policy, dirs: dict[str, str], measured: dict[str, MemberStats]
) -> list[str]:
    """Every workspace member must be floored or explicitly exempted."""
    errors: list[str] = []
    known = set(dirs.values())
    for name in sorted(known - set(policy.floors) - set(policy.no_floor)):
        errors.append(
            f"workspace member {name} has no entry in coverage.toml — add it to "
            f"[floors] or to [no_floor.{name}] with a reason"
        )
    for name in sorted(set(policy.floors) - known):
        errors.append(f"[floors] lists unknown member {name}")
    for name in sorted(set(policy.no_floor) - known):
        errors.append(f"[no_floor] lists unknown member {name}")
    for name in sorted(policy.floors):
        if name not in measured or measured[name].total == 0:
            errors.append(
                f"{name} has a floor but no measured source lines — move it to "
                f"[no_floor.{name}] with a reason or adjust measurement.exclude"
            )
    return errors


def floor_failures(policy: Policy, stats: dict[str, MemberStats]) -> list[str]:
    failures = []
    for name, floor in sorted(policy.floors.items()):
        member = stats.get(name, MemberStats())
        if member.total and member.percent + 1e-9 < floor:
            failures.append(
                f"{name}: {member.percent:.2f}% < floor {floor:.1f}% "
                f"({member.covered}/{member.total} lines)"
            )
    return failures


# ── reporting ───────────────────────────────────────────────────────────────


def render_report(policy: Policy, stats: dict[str, MemberStats], today: dt.date, lcov_path: str) -> str:
    rows: list[tuple[str, MemberStats, float | None, str]] = []
    for name in sorted(set(policy.floors) | set(stats)):
        member = stats.get(name, MemberStats())
        floor = policy.floors.get(name)
        if floor is None:
            result = "n/a"
        elif member.percent + 1e-9 >= floor:
            result = "ok"
        else:
            result = "FAIL"
        rows.append((name, member, floor, result))

    width = max([len(r[0]) for r in rows] + [6])
    lines = [
        f"coverage report — {lcov_path}",
        f"measured {sum(m.total for m in stats.values())} executable lines in "
        f"{sum(len(m.files) for m in stats.values())} source files",
        "",
        f"{'member'.ljust(width)}  {'covered':>9}  {'coverage':>9}  {'floor':>7}  result",
        f"{'-' * width}  {'-' * 9}  {'-' * 9}  {'-' * 7}  {'-' * 6}",
    ]
    for name, member, floor, result in rows:
        floor_s = "-" if floor is None else f"{floor:.1f}%"
        lines.append(
            f"{name.ljust(width)}  {f'{member.covered}/{member.total}':>9}  "
            f"{member.percent:>8.2f}%  {floor_s:>7}  {result}"
        )

    if policy.no_floor:
        lines += ["", "members without a floor:"]
        for name in sorted(policy.no_floor):
            lines.append(f"  {name}: {policy.no_floor[name]}")

    if policy.exemptions:
        lines += ["", "allowlist exemptions:"]
        for ex in policy.exemptions:
            if ex.expires is None:
                detail = "NO EXPIRY"
            elif today >= ex.expires:
                detail = f"EXPIRED {ex.expires.isoformat()}"
            else:
                detail = f"expires {ex.expires.isoformat()} ({(ex.expires - today).days}d)"
            lines.append(f"  {ex.path} {ex.label()}  [{detail}]")
            lines.append(f"      {ex.reason}")
    return "\n".join(lines)


def render_gaps(stats: dict[str, MemberStats], policy: Policy) -> str:
    out: list[str] = []
    for name in sorted(policy.floors):
        member = stats.get(name, MemberStats())
        floor = policy.floors[name]
        if not member.total or member.percent + 1e-9 >= floor:
            continue
        out.append(
            f"{name}: {member.percent:.2f}% is below the {floor:.1f}% floor "
            f"({member.covered}/{member.total} lines, short by "
            f"{floor - member.percent:.2f} pp)"
        )
        gaps = sorted(
            ((path, total - cov, total) for path, (cov, total) in member.files.items() if cov < total),
            key=lambda item: (-item[1], item[0]),
        )
        for path, uncovered, total in gaps[:8]:
            out.append(f"    {path}: {uncovered}/{total} lines uncovered")
    return "\n".join(out)


def render_markdown(
    policy: Policy,
    stats: dict[str, MemberStats],
    failures: list[str],
    warnings: list[str],
    violations: list[str],
) -> str:
    lines = ["## Coverage floor", ""]
    lines.append("**Result: FAILED**" if failures or violations else "**Result: passed**")
    lines += [
        "",
        "| member | coverage | floor | result |",
        "| --- | ---: | ---: | :---: |",
    ]
    for name in sorted(set(policy.floors) | set(stats)):
        member = stats.get(name, MemberStats())
        floor = policy.floors.get(name)
        if floor is None:
            result, floor_s = "no floor", "-"
        elif member.percent + 1e-9 >= floor:
            result, floor_s = "ok", f"{floor:.1f}%"
        else:
            result, floor_s = "**FAIL**", f"{floor:.1f}%"
        lines.append(
            f"| `{name}` | {member.percent:.2f}% ({member.covered}/{member.total}) "
            f"| {floor_s} | {result} |"
        )

    if policy.exemptions:
        lines += [
            "",
            f"<details><summary>allowlist exemptions ({len(policy.exemptions)})</summary>",
            "",
        ]
        for ex in policy.exemptions:
            expiry = ex.expires.isoformat() if ex.expires else "missing"
            lines.append(f"- `{ex.path}` {ex.label()} — expires {expiry}: {ex.reason}")
        lines += ["", "</details>"]

    for title, items in (("Failures", failures + violations), ("Warnings", warnings)):
        if items:
            lines += ["", f"**{title}:**"]
            lines += [f"- {item}" for item in items]
    return "\n".join(lines)


def annotate(kind: str, title: str, message: str) -> None:
    """Emit a GitHub Actions annotation (no-op outside of Actions)."""
    if os.environ.get("GITHUB_ACTIONS") != "true":
        return
    escaped = message.replace("%", "%25").replace("\n", "%0A").replace("\r", "%0D")
    print(f"::{kind} title={title}::{escaped}")


# ── entry point ─────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="enforce coverage.toml floors")
    parser.add_argument("--policy", default="coverage.toml", help="policy TOML file")
    parser.add_argument(
        "--lcov", default="target/llvm-cov/coverage.lcov", help="LCOV report to check"
    )
    parser.add_argument("--root", default=".", help="workspace root (default: cwd)")
    parser.add_argument(
        "--today",
        default=None,
        help="override today's UTC date (YYYY-MM-DD) to test expiry handling",
    )
    parser.add_argument(
        "--files", action="store_true", help="print per-file coverage for every member"
    )
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()

    def resolve(value: str) -> Path:
        path = Path(value)
        return path if path.is_absolute() else root / path

    try:
        today = dt.date.fromisoformat(args.today) if args.today else dt.datetime.now(dt.UTC).date()
    except ValueError:
        print(f"error: --today must be YYYY-MM-DD, got {args.today!r}", file=sys.stderr)
        return EXIT_USAGE

    try:
        policy = load_policy(resolve(args.policy))
        lcov_files = parse_lcov(resolve(args.lcov))
        dirs = workspace_members(root)
    except (PolicyError, FileNotFoundError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return EXIT_USAGE

    surviving, seen, warnings = select_sources(lcov_files, root, dirs, policy)
    violations, usage_errors, exemption_warnings = apply_exemptions(
        policy, surviving, seen, today
    )
    warnings += exemption_warnings

    stats = compute_stats(surviving, dirs, root)
    usage_errors += check_membership(policy, dirs, stats)
    failures = floor_failures(policy, stats)

    print(render_report(policy, stats, today, args.lcov))
    if args.files:
        print()
        for name in sorted(stats):
            print(f"{name}:")
            for path, (cov, total) in sorted(stats[name].files.items()):
                pct = 100.0 * cov / total if total else 0.0
                print(f"    {cov:4d}/{total:<4d} {pct:6.2f}%  {path}")

    gaps = render_gaps(stats, policy)
    if gaps:
        print("\nbelow floor:\n" + gaps)

    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as handle:
            handle.write(render_markdown(policy, stats, failures, warnings, violations) + "\n")

    for message in warnings:
        annotate("warning", "coverage policy", message)
        print(f"warning: {message}", file=sys.stderr)
    for message in usage_errors:
        annotate("error", "coverage policy", message)
        print(f"error: {message}", file=sys.stderr)
    for message in failures + violations:
        annotate("error", "coverage floor", message)
        print(f"error: {message}", file=sys.stderr)

    if usage_errors:
        return EXIT_USAGE
    if failures or violations:
        print(
            f"\ncoverage floor breached: {len(failures)} member(s) below floor, "
            f"{len(violations)} expired exemption(s) — see the report above",
            file=sys.stderr,
        )
        return EXIT_VIOLATION

    total = sum(m.total for m in stats.values())
    covered = sum(m.covered for m in stats.values())
    overall = 100.0 * covered / total if total else 0.0
    print(
        f"\nall {len(policy.floors)} floored member(s) at or above their floor "
        f"({covered}/{total} lines, {overall:.2f}% overall)"
    )
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
