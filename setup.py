from __future__ import annotations

from setuptools import Distribution, setup
from setuptools.command.bdist_wheel import bdist_wheel


class BinaryDistribution(Distribution):
    def has_ext_modules(self) -> bool:
        return True


class PlatformWheel(bdist_wheel):
    def get_tag(self) -> tuple[str, str, str]:
        _, _, plat = super().get_tag()
        return "py3", "none", plat


setup(distclass=BinaryDistribution, cmdclass={"bdist_wheel": PlatformWheel})
