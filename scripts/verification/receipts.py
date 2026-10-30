"""Revalidate suite attempts; strict gates still require a complete passing run."""
from datetime import datetime
import json
from pathlib import Path
import re

from .classification import compare_rust_inventory
from .evidence import aggregate_outcomes, read_results, selected_tests, sha256
from .suites import SUITES, nextest_command

ROOT = Path(__file__).resolve().parents[2]
CLASSIFICATION = ROOT / "tests/verification/test-bindings.json"
REQUIREMENTS = ROOT / "tests/verification/requirements.json"


def inspect_suite_receipt(path, expected_suites, target, revision, features="telemetry",
                          version=None, profile="dev", test_features="all"):
    """Read one explicit attempt without replacing failures with a passing retry.

    A blocked or unstarted suite supplies no test execution. Invalid input identity,
    changed evidence and failures of the runner's final source/product check remain
    admission errors; their embedded PASS rows cannot be reused as acceptance.
    """
    path = Path(path)
    receipt = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(receipt, dict):
        raise ValueError("Expected a suite receipt object")
    if (receipt.get("source_revision") != revision or receipt.get("worktree_dirty") is not False
            or receipt.get("kind") != "test-suite-run" or receipt.get("schema") != 1
            or receipt.get("status") not in {"PASS", "FAIL", "BLOCKED", "NOT_RUN", "NOT_APPLICABLE"}):
        raise ValueError(f"Failed or unrelated suite attempt: {path}")
    if receipt.get("test_features") != test_features:
        raise ValueError(f"Wrong workspace test feature graph: {path}")
    if sha256(path.parent / "source-snapshot.json") != receipt["source_snapshot_sha256"]:
        raise ValueError(f"Changed source snapshot: {path}")
    snapshot = json.loads((path.parent / "source-snapshot.json").read_text(encoding="utf-8"))
    requirement_input = snapshot.get("tests/verification/requirements.json") if isinstance(snapshot, dict) else None
    if not isinstance(requirement_input, dict) or requirement_input.get("sha256") != sha256(REQUIREMENTS):
        raise ValueError(f"Requirements differ from the executed source snapshot: {path}")
    created_at = receipt.get("created_at")
    if not isinstance(created_at, str) or datetime.fromisoformat(created_at).utcoffset() is None:
        raise ValueError(f"Missing timezone-aware attempt date: {path}")
    artifact = receipt.get("artifact")
    if artifact is not None:
        if artifact["target"] != target or artifact["features"] != features:
            raise ValueError(f"Wrong target or product features: {path}")
        if artifact.get("profile") != profile:
            raise ValueError(f"Wrong product build profile: {path}")
        if version is not None and artifact.get("version") != version:
            raise ValueError(f"Wrong product version: {path}")
        if not re.fullmatch("[0-9a-f]{64}", artifact["sha256"]):
            raise ValueError(f"Invalid product identity: {path}")
    suites = receipt["suites"]
    if (not expected_suites or len(set(expected_suites)) != len(expected_suites)
            or not set(expected_suites) <= SUITES.keys()
            or receipt["requested_suites"] != expected_suites
            or [row["suite"] for row in suites] != expected_suites):
        raise ValueError(f"Unexpected suite selection: {path}")
    # Final input verification failures invalidate all earlier outcomes. A blocked
    # preflight without a product can still explain why no tests ran.
    preflight = artifact is None and receipt["status"] == "BLOCKED" and bool(receipt.get("reason"))
    if receipt.get("reason") and not preflight:
        raise ValueError(f"Attempt did not establish stable execution inputs: {receipt['reason']}")
    if artifact is None and not preflight:
        raise ValueError(f"Attempt has no verified product: {path}")
    if preflight:
        hosts = [line.removeprefix("host: ") for line in receipt.get("rustc", "").splitlines()
                 if line.startswith("host: ")]
        if (hosts != [target] or receipt.get("requested_product_features") != features
                or receipt.get("requested_product_profile") != profile
                or any(row.get("status") != "NOT_RUN" for row in suites)):
            raise ValueError("Blocked preflight disagrees with requested configuration or unstarted suites")
    tests, outcomes = [], []
    for row in suites:
        state = row["status"]
        summary = {key: row[key] for key in ("suite", "status", "attempt", "reason", "exit_code", "timed_out") if key in row}
        outcomes.append(summary)
        if state in {"BLOCKED", "NOT_RUN", "NOT_APPLICABLE"} or (state == "FAIL" and row.get("reason")):
            if row.get("tests") or row.get("junit_sha256"):
                raise ValueError(f"Unexecuted suite cannot supply test outcomes: {row['suite']}")
            if state != "NOT_RUN" and not row.get("reason"):
                raise ValueError(f"Missing suite failure/prerequisite reason: {row['suite']}")
            if state == "NOT_APPLICABLE" and not SUITES[row["suite"]].inapplicable_reason(target):
                raise ValueError(f"Unreviewed suite applicability: {row['suite']}")
            continue
        if artifact is None or state not in {"PASS", "FAIL"} or row.get("attempt") != 1:
            raise ValueError(f"Suite has no single recorded execution: {row['suite']}")
        suite = SUITES[row["suite"]]
        if suite.inapplicable_reason(target):
            raise ValueError(f"Inapplicable suite cannot supply execution evidence: {row['suite']}")
        for action, key in (("list", "discovery_command"), ("run", "command")):
            command = row.get(key, [])
            if len(command) < 5 or command != nextest_command(action, suite, command[4], test_features):
                raise ValueError(f"Test command does not establish its feature graph/selection: {row['suite']}")
        inventory = path.parent / row["suite"] / "inventory.json"
        junit = path.parent / row["suite"] / "junit.xml"
        if sha256(inventory) != row["inventory_sha256"]:
            raise ValueError(f"Changed test evidence: {row['suite']}")
        if row.get("classification_sha256") != sha256(CLASSIFICATION):
            raise ValueError(f"Changed Rust classification: {row['suite']}")
        discovered = json.loads(inventory.read_text())
        compare_rust_inventory(discovered, json.loads(CLASSIFICATION.read_text()),
                               json.loads(REQUIREMENTS.read_text())["api_routes"], suite.binaries, test_features)
        if discovered["rust-build-meta"]["platforms"]["host"]["platform"]["triple"] != target:
            raise ValueError(f"Inventory target disagrees with required target: {row['suite']}")
        selected, all_ids = selected_tests(discovered, suite.ignored, suite.binaries, suite.cases)
        if "junit_sha256" in row:
            if sha256(junit) != row["junit_sha256"]:
                raise ValueError(f"Changed test evidence: {row['suite']}")
            results = read_results(junit, selected, all_ids)
        else:
            if junit.exists() or state != "FAIL":
                raise ValueError(f"Unbound or missing test execution evidence: {row['suite']}")
            results = [{"id": identity, "status": "NOT_RUN"} for identity in selected]
        if type(row.get("exit_code")) is not int or type(row.get("timed_out")) is not bool:
            raise ValueError(f"Missing process outcome: {row['suite']}")
        expected = "PASS" if row["exit_code"] == 0 and not row["timed_out"] and all(
            result["status"] == "PASS" for result in results) else "FAIL"
        if selected != row["selected"] or results != row["tests"] or state != expected:
            raise ValueError(f"Incomplete or inconsistent test outcomes: {row['suite']}")
        tests.extend(dict(result, suite=row["suite"]) for result in results)
    overall = "BLOCKED" if preflight else aggregate_outcomes(row["status"] for row in suites)
    if receipt["status"] != overall:
        raise ValueError(f"Attempt summary disagrees with suite outcomes: {path}")
    return {"receipt_sha256": sha256(path), "status": overall, "created_at": created_at,
            "reason": receipt.get("reason"), "suite_outcomes": outcomes,
            "target": target, "features": features, "profile": profile,
            "artifact_sha256": artifact["sha256"] if artifact else None,
            "suites": expected_suites, "test_features": test_features,
            "source_snapshot_sha256": receipt["source_snapshot_sha256"],
            "source_revision": revision, "tests": tests}


def check_suite_receipt(directory, expected_suites, target, revision, features="telemetry", version=None, profile="dev", test_features="all"):
    receipts = list(directory.glob("*/receipt.json"))
    if len(receipts) != 1:
        raise ValueError(f"Expected exactly one attempt in {directory.name}; found {len(receipts)}")
    record = inspect_suite_receipt(receipts[0], expected_suites, target, revision, features,
                                   version, profile, test_features)
    if record["status"] != "PASS":
        raise ValueError(f"Failed or unrelated suite attempt: {receipts[0]}")
    return record
