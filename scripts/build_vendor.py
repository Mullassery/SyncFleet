#!/usr/bin/env python3
"""Build the native SyncFleet binary and vendor it into the Python package.

The Python wrapper is pure Python and ships as ``py3-none-any``; platform wheels
that bundle the native executable are produced locally (or in CI) with this
script. It compiles the release binary, copies it into
``python/syncfleet/bin/`` and then builds the wheel.

Usage::

    python scripts/build_vendor.py               # build binary + wheel
    python scripts/build_vendor.py --no-wheel    # only vendor the binary
    python scripts/build_vendor.py --sdist       # also build an sdist

Pass ``--plat-name`` to control the wheel platform tag (e.g.
``manylinux_2_35_x86_64``).
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN_DIR = ROOT / "python" / "syncfleet" / "bin"
NAME = "syncfleet.exe" if os.name == "nt" else "syncfleet"


def build_binary(profile: str) -> Path:
    cmd = ["cargo", "build", "--release"]
    if profile != "release":
        cmd = ["cargo", "build", "--profile", profile]
    subprocess.run(cmd, cwd=ROOT, check=True)
    built = ROOT / "target" / profile / NAME
    if not built.is_file():
        raise SystemExit(f"expected binary at {built} but it was not produced")
    return built


def vendor(built: Path) -> Path:
    BIN_DIR.mkdir(parents=True, exist_ok=True)
    dest = BIN_DIR / NAME
    shutil.copy2(built, dest)
    dest.chmod(dest.stat().st_mode | 0o755)
    print(f"Vendored {built} -> {dest}")
    return dest


def build_wheel(plat_name: str | None, sdist: bool) -> None:
    out = ROOT / "dist"
    cmd = [sys.executable, "-m", "build", "--wheel", "--outdir", str(out)]
    if plat_name:
        cmd += ["--config-setting", f"--build-option=--plat-name={plat_name}"]
    if sdist:
        cmd = [sys.executable, "-m", "build", "--outdir", str(out)]
    subprocess.run(cmd, cwd=ROOT, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", default="release", help="cargo profile (default: release)")
    parser.add_argument("--no-wheel", action="store_true", help="skip building the wheel")
    parser.add_argument("--sdist", action="store_true", help="build an sdist instead of a wheel")
    parser.add_argument("--plat-name", default=None, help="wheel platform tag")
    args = parser.parse_args()

    built = build_binary(args.profile)
    vendor(built)
    if not args.no_wheel:
        build_wheel(args.plat_name, args.sdist)
    return 0


if __name__ == "__main__":
    sys.exit(main())
