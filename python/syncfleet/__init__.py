"""SyncFleet Python wrapper.

A dependency-free Python wrapper around the native SyncFleet CLI. See
:func:`syncfleet.cli.main` for details on how the native binary is located.
"""

from __future__ import annotations

from .cli import binary_name, find_binary, main

__all__ = ["__version__", "binary_name", "find_binary", "main"]

__version__ = "0.2.0"
