#!/usr/bin/env python3
"""Run the pinned archive corpus through the shared product verification runner."""
from pathlib import Path
import subprocess
import sys

if __name__ == "__main__":
    raise SystemExit(subprocess.call([
        sys.executable, str(Path(__file__).with_name("verify.py")), "import-corpus",
        *sys.argv[1:],
    ]))
