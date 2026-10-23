#!/usr/bin/env python3
"""Validate acceptance bindings against actual compiler discovery; never infer passes."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BOUNDARIES = {"U", "H", "N", "X", "I", "L", "C", "P", "W", "M"}


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
    if len(result) != inventory["test-count"]:
        raise ValueError("Compiler inventory count does not match discovered tests")
    return result


def validate_requirements(document):
    if document["schema"] != 1:
        raise ValueError("Unsupported requirement schema")
    requirements, cases = set(), set()
    for row in document["requirements"]:
        if row["id"] in requirements:
            raise ValueError(f"Duplicate requirement: {row['id']}")
        requirements.add(row["id"])
        if not row["evidence_tiers"] or not set(row["evidence_tiers"]) <= BOUNDARIES:
            raise ValueError(f"Unknown evidence boundary: {row['id']}")
        for case in row["cases"]:
            if case["id"] in cases:
                raise ValueError(f"Duplicate case: {case['id']}")
            cases.add(case["id"])
            if not case["test_ids"]:
                raise ValueError(f"Case has no concrete tests: {case['id']}")
    return requirements, cases


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


def compare_python(inventory, bindings):
    if inventory.get("source") != "unittest-loader":
        raise ValueError("Expected unittest loader discovery")
    discovered = {row["id"]: row for row in inventory["tests"]}
    if not discovered or len(discovered) != len(inventory["tests"]):
        raise ValueError("Python identities must be unique and nonempty")
    return compare_inventory({"test-count": len(discovered), "rust-suites": {
        "python": {"status": "listed", "testcases": discovered}}}, bindings)


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--requirements", type=Path,
                        default=ROOT / "tests/verification/requirements.json")
    parser.add_argument("--inventory", type=Path)
    parser.add_argument("--bindings", type=Path)
    parser.add_argument("--python-inventory", type=Path)
    parser.add_argument("--python-bindings", type=Path)
    parser.add_argument("--cli-inventory", type=Path)
    parser.add_argument("--cli-bindings", type=Path)
    parser.add_argument("--discover", action="store_true",
                        help="Print compiler IDs for review, without validating classification")
    args = parser.parse_args()
    requirements = json.loads(args.requirements.read_text())
    owners, cases = validate_requirements(requirements)
    if args.inventory:
        inventory = json.loads(args.inventory.read_text())
        if args.discover:
            print(json.dumps(discovered_tests(inventory), indent=2))
            return
        if not args.bindings:
            parser.error("--inventory requires --bindings or explicit --discover")
        bindings = json.loads(args.bindings.read_text())
        discovered = compare_inventory(inventory, bindings)
        for requirement in requirements["requirements"]:
            for case in requirement["cases"]:
                missing = set(case["test_ids"]) - discovered.keys()
                if missing:
                    raise ValueError(f"Case {case['id']} selects absent tests: {sorted(missing)}")
    for inventory_path, binding_path, label, compare in [
        (args.python_inventory, args.python_bindings, "Python", compare_python),
        (args.cli_inventory, args.cli_bindings, "CLI", compare_surfaces),
    ]:
        if bool(inventory_path) != bool(binding_path):
            parser.error(f"{label} validation requires both inventory and bindings")
        if inventory_path:
            inventory = json.loads(inventory_path.read_text())
            bindings = json.loads(binding_path.read_text())
            if label == "CLI":
                compare(inventory, bindings, owners)
            else:
                compare(inventory, bindings)
    print(f"{len(requirements['requirements'])} requirements, {len(cases)} bound cases; "
          "structure checked only, no execution or qualification claim")


if __name__ == "__main__":
    main()
