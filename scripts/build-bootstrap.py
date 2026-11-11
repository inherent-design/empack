#!/usr/bin/env python3
"""Build the Java 8-compatible bootstrap once before compiling native empack."""
import argparse
import json
from pathlib import Path

from verification.bootstrap import admit, build


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Validate existing bytes without building or downloading")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    print(json.dumps((admit if args.check else build)(root), indent=2))


if __name__ == "__main__":
    main()
