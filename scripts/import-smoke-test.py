#!/usr/bin/env python3
"""Compatibility entry point for the rolling import survey; not a pinned corpus."""
from pathlib import Path
import runpy

if __name__ == "__main__":
    runpy.run_path(str(Path(__file__).with_name("survey-imports.py")), run_name="__main__")
