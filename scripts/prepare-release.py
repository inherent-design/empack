#!/usr/bin/env python3
"""Stamp workspace package versions without resolving or changing dependencies."""
import argparse
import json
from pathlib import Path

from verification.evidence import write_json
from verification.release_inputs import prepare, release_version, stamped_sources


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    record = prepare(Path.cwd(), args.version)
    write_json(args.output, record)
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
