"""Supported environment for the pinned headless Prism parser probe."""
import re

PRISM_COMMIT = "7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4"  # Prism 11.1.1


def require_supported_qt(version):
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match or int(match[1]) != 6 or int(match[2]) < 4:
        raise ValueError(f"Prism 11.1.1 requires Qt 6.4+ within major 6; got {version}. "
                         "Unsupported builds are supplemental only; choose a supported --qmake.")
