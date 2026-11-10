#!/usr/bin/env python3
"""Refresh the finite Java policy from a pinned, hash-checked Prism metadata snapshot.

This contributor command is never run during project creation or launch. Historical
minimums precede Prism's Java 8 legacy baseline; see docs/design/java.md.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import urllib.request

REVISION = "b8e2a3e1ba241414f8b861f13c9a6fd6bc5d922d"
SNAPSHOTS = {"21w18a", "21w19a", "1.18-pre1", "1.18-pre2", "24w13a",
             "24w14potato", "24w14a", "26.1-snapshot-1", "26.4-snapshot-8"}
ROOT = Path(__file__).resolve().parents[1]


def receive(url):
    with urllib.request.urlopen(url, timeout=30) as response:
        data = response.read((2 << 20) + 1)
    if len(data) > 2 << 20:
        raise ValueError("Metadata exceeds two MiB")
    return data


def historical_minimum(version, majors):
    parts = version.split(".")
    if len(parts) in (2, 3) and all(part.isdecimal() for part in parts):
        parts = tuple(map(int, parts))
        if parts[0] == 1 and parts[1] <= 5:
            return 5
        if parts[0] == 1 and parts[1] <= 11:
            return 6
    return min(majors)


def generate(revision):
    if len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise ValueError("Use an exact metadata commit")
    base = f"https://raw.githubusercontent.com/PrismLauncher/meta-launcher/{revision}/net.minecraft/"
    index_bytes = receive(base + "index.json")
    index = json.loads(index_bytes)
    versions = [entry for entry in index["versions"]
                if entry["type"] == "release" or entry["version"] in SNAPSHOTS]
    present = {entry["version"] for entry in versions}
    if missing := SNAPSHOTS - present:
        raise ValueError(f"Selected exact snapshot absent from index: {sorted(missing)}")

    def row(entry):
        version = entry["version"]
        if not version or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-_" for c in version):
            raise ValueError("Invalid metadata version identity")
        data = receive(base + version + ".json")
        digest = hashlib.sha256(data).hexdigest()
        if digest != entry["sha256"]:
            raise ValueError(f"Index hash mismatch: {version}")
        profile = json.loads(data)
        majors = profile["compatibleJavaMajors"]
        if (not majors or any(type(n) is not int or not 8 <= n <= 65535 for n in majors)
                or majors != sorted(set(majors))):
            raise ValueError(f"Unsupported Java profile: {version}")
        return f"{version}|{historical_minimum(version, majors)}|{','.join(map(str, majors))}|{digest}"

    with ThreadPoolExecutor(max_workers=4) as executor:
        rows = sorted(executor.map(row, versions))
    return (f"# PrismLauncher/meta-launcher {revision}\n"
            f"# index.json sha256 {hashlib.sha256(index_bytes).hexdigest()}\n"
            "# Exact Minecraft identity|historical minimum|admitted Java majors|profile sha256\n"
            + "\n".join(rows) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", default=REVISION)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generated = generate(args.revision)
    destination = ROOT / "crates/empack-core/src/java_profiles.csv"
    if args.check:
        if destination.read_text() != generated:
            raise ValueError("Java policy differs from the pinned metadata")
    else:
        destination.write_text(generated, encoding="utf-8", newline="\n")
    print(f"Verified {len(generated.splitlines()) - 3} exact Java profiles at {args.revision}")


if __name__ == "__main__":
    main()
