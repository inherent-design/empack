#!/usr/bin/env python3
"""Stage or retrieve reviewed operator files for the existing candidate gate."""
import argparse
from pathlib import Path
import subprocess

from verification.acceptance_assets import collect, stage


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    staging = commands.add_parser("stage")
    staging.add_argument("--record", action="append", type=Path, required=True)
    fetch = commands.add_parser("collect")
    fetch.add_argument("--repository", required=True)
    for command in (staging, fetch):
        command.add_argument("--revision", required=True)
        command.add_argument("--version", required=True)
        command.add_argument("--archive-evidence-sha256", required=True)
        command.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "stage":
            result = stage(args.record, args.output, args.revision, args.version, args.archive_evidence_sha256)
            print(f"Staged {len(result['assets'])} data files for {result['release_tag']}; no acceptance claimed")
        else:
            result = collect(args.repository, args.output, args.revision, args.version, args.archive_evidence_sha256)
            print(f"Operator evidence: {result['status']}; {len(result['records'])} records; {args.output / 'intake.json'}")
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Operator evidence transfer failed: {type(error).__name__}: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
