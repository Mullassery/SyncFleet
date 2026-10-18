from __future__ import annotations

import os

from setuptools import Distribution, setup
from setuptools.command.bdist_wheel import bdist_wheel


class BinaryDistribution(Distribution):
    def has_ext_modules(self) -> bool:
        return True


class PlatformWheel(bdist_wheel):
    def get_tag(self) -> tuple[str, str, str]:
        _, _, plat = super().get_tag()
        # ``SYNCFLEET_WHEEL_PLAT`` lets the vendoring script pin an accurate tag
        # (e.g. macosx_11_0_arm64); setuptools' default would otherwise claim
        # universal2 for a single-arch native binary.
        plat = os.environ.get("SYNCFLEET_WHEEL_PLAT") or plat
        return "py3", "none", plat


setup(distclass=BinaryDistribution, cmdclass={"bdist_wheel": PlatformWheel})
