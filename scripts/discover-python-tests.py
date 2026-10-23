#!/usr/bin/env python3
"""List Python unittest identities through the loader, without executing tests."""
import argparse
import json
from pathlib import Path
import unittest


def discover(directory: Path):
    loader = unittest.TestLoader()
    suite = loader.discover(str(directory))
    if loader.errors:
        raise ValueError("Python test discovery failed:\n" + "\n".join(loader.errors))

    def identities(node):
        if isinstance(node, unittest.TestSuite):
            for child in node:
                yield from identities(child)
        else:
            method = getattr(node, node._testMethodName)
            skipped = getattr(node, "__unittest_skip__", False) or getattr(
                method, "__unittest_skip__", False)
            yield {"id": node.id(), "ignored": bool(skipped)}

    tests = sorted(identities(suite), key=lambda row: row["id"])
    if not tests or len({row["id"] for row in tests}) != len(tests):
        raise ValueError("Python discovery must contain unique, nonempty test identities")
    return {"schema": 1, "source": "unittest-loader", "tests": tests}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path,
                        default=Path(__file__).resolve().parent / "tests")
    args = parser.parse_args()
    print(json.dumps(discover(args.directory), indent=2))


if __name__ == "__main__":
    main()
