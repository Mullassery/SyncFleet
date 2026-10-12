#!/usr/bin/env python3
"""Bump the SyncFleet version across every manifest in one shot.

The Rust crate (``Cargo.toml``) is the source of truth. This script keeps the
Python wrapper and the changelog in lockstep with it, so a release is a single
command instead of a hand-edited set of files that drift apart.

Usage::

    python scripts/release.py patch            # 0.1.0 -> 0.1.1
    python scripts/release.py minor            # 0.1.0 -> 0.2.0
    python scripts/release.py major            # 0.1.0 -> 1.0.0
    python scripts/release.py 1.2.3            # explicit version
    python scripts/release.py patch --tag      # also create the git tag
    python scripts/release.py patch --commit   # also commit the bump

Files updated:

* ``Cargo.toml``                  - ``[package].version``
* ``python/syncfleet/__init__.py`` - ``__version__``
* ``CHANGELOG.md``                 - a new version section is inserted

``Cargo.lock`` is refreshed by the next ``cargo`` invocation; run
``cargo build`` (or ``cargo update -p syncfleet``) afterwards to commit it.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
INIT_PY = ROOT / "python" / "syncfleet" / "__init__.py"
CHANGELOG = ROOT / "CHANGELOG.md"

_SEMVER = re.compile(r"^\d+\.\d+\.\d+$")


def read_cargo_version() -> str:
    text = CARGO_TOML.read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        raise SystemExit("could not find version in Cargo.toml")
    return match.group(1)


def bump(current: str, part: str) -> str:
    if _SEMVER.match(part):
        return part
    major, minor, patch = (int(x) for x in current.split("."))
    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    if part == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise SystemExit(f"invalid bump: {part!r} (use major|minor|patch|x.y.z)")


def write_cargo(new: str) -> None:
    text = CARGO_TOML.read_text(encoding="utf-8")
    text, count = re.subn(
        r'^version\s*=\s*"[^"]+"',
        f'version = "{new}"',
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        raise SystemExit("failed to update Cargo.toml version")
    CARGO_TOML.write_text(text, encoding="utf-8")


def write_init(new: str) -> None:
    text = INIT_PY.read_text(encoding="utf-8")
    text, count = re.subn(
        r'__version__\s*=\s*"[^"]+"',
        f'__version__ = "{new}"',
        text,
        count=1,
    )
    if count != 1:
        raise SystemExit("failed to update python/syncfleet/__init__.py version")
    INIT_PY.write_text(text, encoding="utf-8")


def write_changelog(new: str, old: str) -> None:
    text = CHANGELOG.read_text(encoding="utf-8")
    today = _dt.date.today().isoformat()
    header = f"## [{new}] - {today}\n\n### Changed\n\n- Release {new}.\n\n"
    marker = "## [Unreleased]"
    if marker in text:
        head, _, tail = text.partition(marker)
        newline_idx = tail.find("\n")
        rest = tail[newline_idx + 1 :] if newline_idx != -1 else tail
        text = f"{head}{marker}{tail[:newline_idx + 1]}\n{header}{rest}".replace(
            "\n\n\n", "\n\n"
        )
    else:
        text = f"{header}{text}"
        _ = old
    CHANGELOG.write_text(text, encoding="utf-8")


def git(*args: str) -> None:
    subprocess.run(["git", *args], cwd=ROOT, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="Bump the SyncFleet version")
    parser.add_argument("part", help="major | minor | patch | an explicit x.y.z")
    parser.add_argument("--tag", action="store_true", help="create a git tag vX.Y.Z")
    parser.add_argument("--commit", action="store_true", help="git commit the bump")
    args = parser.parse_args()

    old = read_cargo_version()
    new = bump(old, args.part)
    if new == old:
        raise SystemExit(f"version already {old}")

    write_cargo(new)
    write_init(new)
    write_changelog(new, old)

    print(f"Bumped {old} -> {new}")
    print("Reminder: run `cargo build` (or `cargo update -p syncfleet`) to refresh Cargo.lock.")

    if args.commit:
        git("add", "Cargo.toml", "Cargo.lock", "python/syncfleet/__init__.py", "CHANGELOG.md")
        git("commit", "-m", f"chore(release): v{new}")
        print(f"Committed v{new}")
    if args.tag:
        git("tag", "-a", f"v{new}", "-m", f"SyncFleet v{new}")
        print(f"Tagged v{new}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
