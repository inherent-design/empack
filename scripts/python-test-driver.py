#!/usr/bin/env python3
"""Fixed unittest worker for scripts/verify.py; no product is executed implicitly."""
import argparse
import json
from pathlib import Path
import platform
import unittest

from verification.evidence import write_json
from verification.python_tests import BINDINGS, ROOT, ObservedResult, load_suite, platform_outcomes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    suite, inventory = load_suite(ROOT / "scripts/tests")
    write_json(args.output / "inventory.json", inventory)
    result = unittest.TextTestRunner(verbosity=2, resultclass=ObservedResult).run(suite)
    write_json(args.output / "events.json", result.events)
    classification = json.loads((ROOT / BINDINGS).read_text(encoding="utf-8"))
    host = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}[platform.system()]
    _, status = platform_outcomes(inventory, result.events, classification, host)
    return 0 if status == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
