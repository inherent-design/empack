#!/usr/bin/env python3
"""Bind production builds to executed archives before staging publication bytes."""
import argparse
import json
import os
from pathlib import Path

from verification.release import TARGETS, admit_archives, check_payload, record_build
from verification.product_build import compile_product


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    compile = commands.add_parser("build")
    compile.add_argument("--target", choices=TARGETS, required=True)
    compile.add_argument("--builder", choices=("cargo", "cross"), required=True)
    compile.add_argument("--output", type=Path, required=True)
    build = commands.add_parser("record")
    build.add_argument("--inputs", type=Path, required=True)
    build.add_argument("--archive", type=Path, required=True)
    build.add_argument("--target", choices=TARGETS, required=True)
    build.add_argument("--builder", choices=("cargo", "cross"), required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--compilation", type=Path, required=True)
    gate = commands.add_parser("gate")
    gate.add_argument("--artifacts", type=Path, required=True)
    gate.add_argument("--version", required=True)
    gate.add_argument("--revision", required=True)
    gate.add_argument("--output", type=Path, required=True)
    payload = commands.add_parser("verify-payload")
    payload.add_argument("--directory", type=Path, required=True)
    payload.add_argument("--version", required=True)
    payload.add_argument("--revision", required=True)
    payload.add_argument("--evidence-sha256", required=True)
    args = parser.parse_args()
    if args.command == "build":
        if args.builder == "cross" and args.target != "aarch64-unknown-linux-gnu":
            parser.error("Cross is only qualified for the Linux ARM64 archive")
        result = compile_product(Path.cwd(), args.target, args.builder, args.output, os.environ.copy())
    elif args.command == "record":
        result = record_build(Path.cwd(), args.inputs, args.archive, args.target, args.builder, args.output, args.compilation)
    elif args.command == "gate":
        result = admit_archives(Path.cwd(), args.artifacts, args.version, args.revision, args.output)
    else:
        result = check_payload(args.directory, args.version, args.revision, args.evidence_sha256)
    print(json.dumps({key: value for key, value in result.items() if key != "targets"}, indent=2))


if __name__ == "__main__":
    main()
