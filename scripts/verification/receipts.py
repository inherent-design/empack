"""Revalidate shared suite receipts from compiler inventory and JUnit."""
import json
from pathlib import Path
import re

from .classification import compare_rust_inventory
from .evidence import read_results, selected_tests, sha256
from .suites import SUITES

ROOT = Path(__file__).resolve().parents[2]
CLASSIFICATION = ROOT / "tests/verification/test-bindings.json"
REQUIREMENTS = ROOT / "tests/verification/requirements.json"


def check_suite_receipt(directory, expected_suites, target, revision, features="telemetry", version=None, profile="dev"):
    receipts = list(directory.glob("*/receipt.json"))
    if len(receipts) != 1:
        raise ValueError(f"Expected exactly one attempt in {directory.name}; found {len(receipts)}")
    path = receipts[0]
    receipt = json.loads(path.read_text())
    if (receipt.get("status") != "PASS" or receipt.get("source_revision") != revision
            or receipt.get("worktree_dirty") is not False
            or receipt.get("kind") != "test-suite-run" or receipt.get("schema") != 1):
        raise ValueError(f"Failed or unrelated suite attempt: {path}")
    artifact = receipt["artifact"]
    if artifact["target"] != target or artifact["features"] != features:
        raise ValueError(f"Wrong target or product features: {path}")
    if artifact.get("profile") != profile:
        raise ValueError(f"Wrong product build profile: {path}")
    if version is not None and artifact.get("version") != version:
        raise ValueError(f"Wrong product version: {path}")
    if not re.fullmatch("[0-9a-f]{64}", artifact["sha256"]):
        raise ValueError(f"Invalid product identity: {path}")
    if sha256(path.parent / "source-snapshot.json") != receipt["source_snapshot_sha256"]:
        raise ValueError(f"Changed source snapshot: {path}")
    suites = receipt["suites"]
    if receipt["requested_suites"] != expected_suites or [row["suite"] for row in suites] != expected_suites:
        raise ValueError(f"Unexpected suite selection: {path}")
    for row in suites:
        if row["status"] != "PASS" or row["exit_code"] != 0 or row["timed_out"] or row["attempt"] != 1:
            raise ValueError(f"Suite did not pass its first attempt: {row['suite']}")
        suite = SUITES[row["suite"]]
        inventory = path.parent / row["suite"] / "inventory.json"
        junit = path.parent / row["suite"] / "junit.xml"
        if sha256(inventory) != row["inventory_sha256"] or sha256(junit) != row["junit_sha256"]:
            raise ValueError(f"Changed test evidence: {row['suite']}")
        if row.get("classification_sha256") != sha256(CLASSIFICATION):
            raise ValueError(f"Changed Rust classification: {row['suite']}")
        discovered = json.loads(inventory.read_text())
        compare_rust_inventory(discovered, json.loads(CLASSIFICATION.read_text()),
                               json.loads(REQUIREMENTS.read_text())["api_routes"], suite.binaries)
        if discovered["rust-build-meta"]["platforms"]["host"]["platform"]["triple"] != target:
            raise ValueError(f"Inventory target disagrees with required target: {row['suite']}")
        selected, all_ids = selected_tests(discovered, suite.ignored, suite.binaries, suite.cases)
        results = read_results(junit, selected, all_ids)
        if selected != row["selected"] or results != row["tests"] or any(r["status"] != "PASS" for r in results):
            raise ValueError(f"Incomplete or inconsistent test outcomes: {row['suite']}")
    return {"receipt_sha256": sha256(path), "target": target, "features": features, "profile": profile,
            "artifact_sha256": artifact["sha256"], "suites": expected_suites,
            "source_snapshot_sha256": receipt["source_snapshot_sha256"],
            "source_revision": revision, "tests": [test for row in suites for test in row["tests"]]}

