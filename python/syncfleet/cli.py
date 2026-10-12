"""Command-line wrapper around the native ``syncfleet`` binary.

SyncFleet's core is written in Rust and shipped as a self-contained native
executable. This package is a thin, dependency-free Python wrapper that locates
that executable and forwards the process arguments, standard streams and exit
code to it, so the tool can be installed and invoked the usual Python way::

    pip install syncfleet
    syncfleet --help

Binary resolution order:

1. ``SYNCFLEET_BIN`` environment variable (absolute path to the executable).
2. A binary vendored inside this package at ``syncfleet/bin/syncfleet``.
3. ``syncfleet`` (or ``syncfleet.exe`` on Windows) found on ``PATH``, skipping
   this wrapper itself so that a Python console script named ``syncfleet`` can
   never recurse into itself.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

__all__ = ["binary_name", "find_binary", "main"]

_ENV_OVERRIDE = "SYNCFLEET_BIN"
_RECURSION_GUARD = "SYNCFLEET_WRAPPER_DEPTH"


def binary_name() -> str:
    """Return the platform-specific native executable name."""
    return "syncfleet.exe" if os.name == "nt" else "syncfleet"


def _bundled_binary() -> Path:
    return Path(__file__).resolve().parent / "bin" / binary_name()


def _is_executable(path: Path) -> bool:
    return path.is_file() and os.access(path, os.X_OK)


def _self_paths() -> set[Path]:
    """Paths that correspond to this wrapper and must never be re-executed."""
    candidates: set[Path] = set()
    for raw in (sys.argv[0] if sys.argv else "", sys.executable or ""):
        if not raw:
            continue
        try:
            candidates.add(Path(raw).resolve())
        except OSError:  # pragma: no cover - defensive
            continue
    return candidates


def find_binary() -> str:
    """Locate the native ``syncfleet`` executable.

    Raises:
        FileNotFoundError: if no executable could be found. The message lists
            every location that was probed so the problem is actionable.
    """
    override = os.environ.get(_ENV_OVERRIDE)
    if override:
        if _is_executable(Path(override)):
            return override
        raise FileNotFoundError(
            f"{_ENV_OVERRIDE} is set to {override!r} but it is not an "
            f"executable file"
        )

    bundled = _bundled_binary()
    if _is_executable(bundled):
        return str(bundled)

    name = binary_name()
    blocked = _self_paths()
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if not entry:
            continue
        candidate = Path(entry) / name
        try:
            resolved = candidate.resolve()
        except OSError:  # pragma: no cover - defensive
            continue
        if resolved in blocked:
            continue
        if _is_executable(candidate):
            return str(candidate)

    raise FileNotFoundError(
        "could not locate the native 'syncfleet' executable.\n"
        "Install the SyncFleet CLI and ensure it is on your PATH, or point "
        f"{_ENV_OVERRIDE} at the binary.\n"
        f"Probed: {_ENV_OVERRIDE}, {bundled}, and PATH."
    )


def main(argv: list[str] | None = None) -> int:
    """Run the native binary, forwarding ``argv`` and stdio.

    On POSIX systems the native process replaces this one via ``execv`` so
    signals and exit codes behave exactly as if the binary were invoked
    directly. On Windows the child process is awaited and its return code is
    propagated.
    """
    if argv is None:
        argv = sys.argv[1:]

    depth = int(os.environ.get(_RECURSION_GUARD, "0"))
    if depth > 0:
        print(
            "syncfleet: refusing to re-run the Python wrapper recursively; "
            f"set {_ENV_OVERRIDE} to the native binary path",
            file=sys.stderr,
        )
        return 1

    try:
        binary = find_binary()
    except FileNotFoundError as exc:
        print(f"syncfleet: {exc}", file=sys.stderr)
        return 127

    args = [binary, *argv]
    env = dict(os.environ, **{_RECURSION_GUARD: str(depth + 1)})

    if os.name == "nt":
        completed = subprocess.run(args, env=env)
        return completed.returncode

    os.execve(binary, args, env)
    return 0  # pragma: no cover - execve never returns on success
