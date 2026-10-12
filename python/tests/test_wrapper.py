"""Tests for the SyncFleet Python wrapper's binary discovery."""

from __future__ import annotations

import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from syncfleet import cli  # noqa: E402


def _make_executable(path: Path) -> None:
    path.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


class FindBinaryTests(unittest.TestCase):
    def test_env_override_wins(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / cli.binary_name()
            _make_executable(binary)
            with mock.patch.dict(os.environ, {"SYNCFLEET_BIN": str(binary)}):
                self.assertEqual(cli.find_binary(), str(binary))

    def test_env_override_missing_is_reported(self) -> None:
        with mock.patch.dict(os.environ, {"SYNCFLEET_BIN": "/nope/syncfleet"}):
            with self.assertRaises(FileNotFoundError) as ctx:
                cli.find_binary()
            self.assertIn("SYNCFLEET_BIN", str(ctx.exception))

    def test_bundled_binary_is_used(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / cli.binary_name()
            _make_executable(binary)
            with mock.patch.dict(os.environ, {}, clear=True):
                with mock.patch.object(cli, "_bundled_binary", return_value=binary):
                    self.assertEqual(cli.find_binary(), str(binary))

    def test_path_lookup_skips_wrapper_itself(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            wrapper = Path(tmp) / cli.binary_name()
            _make_executable(wrapper)
            native = Path(tmp) / "native" / cli.binary_name()
            native.parent.mkdir()
            _make_executable(native)
            env = {"PATH": f"{wrapper.parent}{os.pathsep}{native.parent}"}
            with mock.patch.dict(os.environ, env, clear=True):
                with mock.patch.object(cli, "_bundled_binary", return_value=Path("/nope")):
                    with mock.patch.object(cli, "_self_paths", return_value={wrapper.resolve()}):
                        self.assertEqual(cli.find_binary(), str(native))

    def test_nothing_found_raises(self) -> None:
        with mock.patch.dict(os.environ, {"PATH": ""}, clear=True):
            with mock.patch.object(cli, "_bundled_binary", return_value=Path("/nope")):
                with self.assertRaises(FileNotFoundError):
                    cli.find_binary()


if __name__ == "__main__":
    unittest.main()
