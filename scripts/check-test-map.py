#!/usr/bin/env python3
"""Validate acceptance bindings against actual compiler discovery; never infer passes."""
import argparse
import json
import os
import sys
from pathlib import Path

from verification.classification import BOUNDARIES, compare_rust_inventory
from verification.consumer import PRISM_TEST
from verification.python_tests import compare_inventory as compare_python_inventory
from verification.suites import TEST_FEATURES
from verification.discovery import rustdoc_tests
from verification.requirements import case_results, validate_requirements
from verification.process import run

ROOT = Path(__file__).resolve().parents[1]


def discovered_tests(inventory):
    result = {}
    for binary_id, suite in inventory["rust-suites"].items():
        if suite["status"] != "listed":
            raise ValueError(f"Test binary was not listed: {binary_id}")
        for name, case in suite["testcases"].items():
            identity = f"{binary_id}::{name}"
            if identity in result:
                raise ValueError(f"Duplicate test identity: {identity}")
            result[identity] = {"binary": binary_id, "name": name,
                                "ignored": case["ignored"]}
    if not result:
        raise ValueError("Compiler discovery must be nonempty")
    if len(result) != inventory["test-count"]:
        raise ValueError("Compiler inventory count does not match discovered tests")
    return result


def compare_inventory(inventory, bindings):
    discovered = discovered_tests(inventory)
    classified = {}
    for entry in bindings["tests"]:
        identity = entry["id"]
        if identity in classified:
            raise ValueError(f"Duplicate test classification: {identity}")
        if entry["boundary"] not in BOUNDARIES:
            raise ValueError(f"Unknown test boundary: {identity}")
        classified[identity] = entry
    unknown = sorted(discovered.keys() - classified.keys())
    missing = sorted(classified.keys() - discovered.keys())
    if unknown or missing:
        raise ValueError(json.dumps({"unclassified_tests": unknown,
                                     "missing_tests": missing}, indent=2))
    return discovered


def compare_python(inventory, bindings, requirements=None):
    result = compare_python_inventory(inventory, bindings)
    if requirements is not None:
        for entry in bindings["tests"]:
            if not entry.get("requirements") or not set(entry["requirements"]) <= requirements:
                raise ValueError(f"Python test has no valid requirement owner: {entry['id']}")
    return result


def compare_surfaces(inventory, bindings, requirements):
    if inventory.get("source") != "clap-command-factory":
        raise ValueError("Expected compiled Clap discovery")
    discovered = {row["id"]: row for row in inventory["surfaces"]}
    classified = {row["id"]: row for row in bindings["surfaces"]}
    if not discovered or len(discovered) != len(inventory["surfaces"]):
        raise ValueError("CLI surfaces must be unique and nonempty")
    if len(classified) != len(bindings["surfaces"]):
        raise ValueError("Duplicate CLI surface classification")
    if discovered.keys() != classified.keys():
        raise ValueError(json.dumps({
            "unclassified_surfaces": sorted(discovered.keys() - classified.keys()),
            "missing_surfaces": sorted(classified.keys() - discovered.keys()),
        }, indent=2))
    for identity, entry in classified.items():
        if not entry["requirements"] or not set(entry["requirements"]) <= requirements:
            raise ValueError(f"Surface has no valid requirement owner: {identity}")
        if entry["definition"] != discovered[identity]:
            raise ValueError(f"CLI surface definition changed: {identity}")
    return discovered


def compare_doctests(inventory, bindings, requirements):
    if inventory.get("source") != "rustdoc-list":
        raise ValueError("Expected rustdoc discovery")
    discovered = {row["id"]: row for row in inventory["tests"]}
    if not discovered or len(discovered) != len(inventory["tests"]):
        raise ValueError("Doctest identities must be unique and nonempty")
    for entry in bindings["tests"]:
        if not entry.get("requirements") or not set(entry["requirements"]) <= requirements:
            raise ValueError(f"Doctest has no valid requirement owner: {entry['id']}")
        name = entry["id"].removeprefix("doctest::")
        if name in discovered and entry["ignored"] != discovered[name]["ignored"]:
            raise ValueError(f"Doctest ignore policy changed: {entry['id']}")
    return compare_inventory({"test-count": len(discovered), "rust-suites": {
        "doctest": {"status": "listed", "testcases": discovered}}}, bindings)


def discover_tooling(output, requirements):
    """Compile/list public surfaces and verify ownership, without executing cases."""
    output.mkdir(parents=True, exist_ok=True)
    def invoke(command, name):
        log = output / f"{name}-discovery.log"
        code, stdout, timed_out = run(command, cwd=ROOT, env=dict(os.environ),
            log=log, timeout=900, separate_stdout=True)
        if code or timed_out:
            raise ValueError(f"{name} discovery failed (exit {code}, timeout={timed_out}); see {log}")
        return stdout

    commands = {
        "python": [sys.executable, str(ROOT / "scripts/discover-python-tests.py")],
        "cli": ["cargo", "run", "--locked", "--quiet", "-p", "empack-tests",
                "--example", "cli-inventory"],
    }
    inventories = {name: json.loads(invoke(command, name)) for name, command in commands.items()}
    command = ["cargo", "test", "--locked", "--workspace", "--all-features", "--doc", "--", "--list"]
    tests = rustdoc_tests(invoke(command, "doctest"))
    ignored = rustdoc_tests(invoke(command + ["--ignored"], "doctest-ignored"), allow_empty=True)
    if ignored - tests:
        raise ValueError("Ignored rustdoc identities were absent from complete discovery")
    inventories["doctest"] = {"schema": 1, "source": "rustdoc-list",
        "selection": "workspace-all-features", "tests": [
            {"id": identity, "ignored": identity in ignored} for identity in sorted(tests)]}
    counts = {}
    for name, inventory in inventories.items():
        (output / f"{name}-inventory.json").write_text(
            json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
        bindings = json.loads((ROOT / f"tests/verification/{name}-bindings.json").read_text(encoding="utf-8"))
        compare = {"python": compare_python, "cli": compare_surfaces, "doctest": compare_doctests}[name]
        counts[name] = len(compare(inventory, bindings, requirements))
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--requirements", type=Path,
                        default=ROOT / "tests/verification/requirements.json")
    parser.add_argument("--test-features", choices=TEST_FEATURES, default="all")
    parser.add_argument("--inventory", type=Path)
    parser.add_argument("--bindings", type=Path)
    parser.add_argument("--python-inventory", type=Path)
    parser.add_argument("--python-bindings", type=Path)
    parser.add_argument("--doctest-inventory", type=Path)
    parser.add_argument("--doctest-bindings", type=Path)
    parser.add_argument("--cli-inventory", type=Path)
    parser.add_argument("--cli-bindings", type=Path)
    parser.add_argument("--discover", action="store_true",
                        help="Print compiler IDs for review, without validating classification")
    parser.add_argument("--discover-tooling", type=Path, metavar="OUTPUT_DIR",
                        help="Compile/list CLI, Python and doctests; verify checked-in bindings")
    args = parser.parse_args()
    requirements = json.loads(args.requirements.read_text(encoding="utf-8"))
    owners, cases = validate_requirements(requirements)
    # Validate every declared boundary and platform even before selecting a host
    # inventory. An empty evidence set deliberately cannot qualify any case.
    case_results(requirements,
                 json.loads((ROOT / "tests/verification/test-bindings.json").read_text(encoding="utf-8")),
                 [], "unexecuted-structural-check")
    if args.discover_tooling:
        counts = discover_tooling(args.discover_tooling, owners)
        print(f"Classified {counts['cli']} CLI surfaces, {counts['python']} Python tests "
              f"and {counts['doctest']} doctests")
    if args.inventory:
        inventory = json.loads(args.inventory.read_text(encoding="utf-8"))
        if args.discover:
            print(json.dumps(discovered_tests(inventory), indent=2))
            return
        if not args.bindings:
            parser.error("--inventory requires --bindings or explicit --discover")
        bindings = json.loads(args.bindings.read_text(encoding="utf-8"))
        discovered = (compare_rust_inventory(inventory, bindings, requirements["api_routes"], test_features=args.test_features)
                      if bindings.get("schema") == 3 else compare_inventory(inventory, bindings))
        # Compiler comparison above enforces this host/feature graph. Cases can
        # also require other hosts; check their names against the complete Rust
        # classification, not this host's conditional compilation result.
        known_rust = ({row["id"] for row in bindings["tests"]}
                      if bindings.get("schema") == 3 else discovered.keys())
        for requirement in requirements["requirements"]:
            for case in requirement["cases"]:
                # Python, rustdoc and Qt have independent discovery/validation.
                # case_results above still rejects unknown or mismatched bindings.
                missing = {identity for identity in case["test_ids"]
                           if not identity.startswith(("python::", "doctest::"))} - {PRISM_TEST} - known_rust
                if missing:
                    raise ValueError(f"Case {case['id']} selects absent tests: {sorted(missing)}")
    for inventory_path, binding_path, label, compare in [
        (args.python_inventory, args.python_bindings, "Python", compare_python),
        (args.cli_inventory, args.cli_bindings, "CLI", compare_surfaces),
        (args.doctest_inventory, args.doctest_bindings, "Doctest", compare_doctests),
    ]:
        if bool(inventory_path) != bool(binding_path):
            parser.error(f"{label} validation requires both inventory and bindings")
        if inventory_path:
            inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
            bindings = json.loads(binding_path.read_text(encoding="utf-8"))
            compare(inventory, bindings, owners)
    print(f"{len(requirements['requirements'])} requirements, {len(cases)} bound cases; "
          "structure checked only, no execution or qualification claim")


if __name__ == "__main__":
    main()
