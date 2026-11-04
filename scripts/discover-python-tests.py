#!/usr/bin/env python3
"""List Python unittest identities through the loader, without executing tests."""
import argparse
import json
from pathlib import Path
from verification.python_tests import load_suite


def discover(directory: Path):
    return load_suite(directory)[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path,
                        default=Path(__file__).resolve().parent / "tests")
    args = parser.parse_args()
    print(json.dumps(discover(args.directory), indent=2))


if __name__ == "__main__":
    main()
