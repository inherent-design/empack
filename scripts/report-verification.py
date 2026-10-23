#!/usr/bin/env python3
"""Recheck explicit suite attempts, retaining failed, blocked and unstarted work."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re

from verification.evidence import aggregate_outcomes, sha256, write_json
from verification.receipts import inspect_suite_receipt
from verification.python_tests import inspect_python_evidence
from verification.reporting import render_attempts
from verification.requirements import case_results

ROOT = Path(__file__).resolve().parents[1]


def report(paths, revision, version):
    if not re.fullmatch(r"[a-f0-9]{40}", revision):
        raise ValueError("Expected one immutable source revision")
    if not paths or len(paths) > 1000:
        raise ValueError("Select between one and 1000 explicit receipts")
    requirements_path = ROOT / "tests/verification/requirements.json"
    classification_path = ROOT / "tests/verification/test-bindings.json"
    classification = json.loads(classification_path.read_text())
    admitted, tooling, attempts, seen = [], [], [], set()
    for path in paths:
        digest = None
        try:
            path = Path(path)
            digest = sha256(path)
            if digest in seen:
                continue
            seen.add(digest)
            receipt = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(receipt, dict):
                raise ValueError("Expected a suite receipt object")
            if receipt.get("kind") == "python-suite-run":
                result = inspect_python_evidence(path, revision)
                tooling.append(result)
                attempts.append({"admission": "ADMITTED", **{key: value for key, value in result.items() if key != "tests"}})
                continue
            artifact = receipt.get("artifact")
            # Preflight may stop before producing a binary. The compiler identity
            # can identify its host but cannot manufacture executable evidence.
            target = artifact["target"] if artifact else next(
                line.removeprefix("host: ") for line in receipt.get("rustc", "").splitlines()
                if line.startswith("host: "))
            features = artifact["features"] if artifact else receipt.get("requested_product_features")
            profile = artifact["profile"] if artifact else receipt.get("requested_product_profile")
            if (target not in classification["targets"] or features not in {"default", "telemetry"}
                    or profile not in {"dev", "release"} or receipt.get("test_features") not in {"minimal", "all"}):
                raise ValueError("Attempt lacks a supported host/product configuration")
            result = inspect_suite_receipt(path, receipt["requested_suites"], target, revision,
                                           features, version, profile, receipt["test_features"])
            admitted.append(result)
            attempts.append({"admission": "ADMITTED", **{key: value for key, value in result.items() if key != "tests"}})
        except (ValueError, OSError, KeyError, TypeError, AttributeError, IndexError, StopIteration) as error:
            attempts.append({"admission": "REJECTED", "receipt_sha256": digest,
                             "path": str(path), "status": "FAIL", "reason": str(error) or "Missing attempt identity"})
    acceptance = case_results(json.loads(requirements_path.read_text()), classification, admitted, revision, tooling_evidence=tooling)
    acceptance.update(requirements_sha256=sha256(requirements_path), classification_sha256=sha256(classification_path))
    return {"schema": 1, "kind": "verification-attempt-report", "qualification": False,
            "status": aggregate_outcomes(row["status"] for row in attempts), "source_revision": revision,
            "version": version, "created_at": datetime.now(timezone.utc).isoformat(),
            "attempts": attempts, "acceptance": acceptance, "tooling_evidence": tooling,
            "suite_evidence": [{key: value for key, value in row.items() if key != "tests"} for row in admitted]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, action="append", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = report(args.receipt, args.revision, args.version)
    args.output.mkdir(parents=True, exist_ok=False)
    write_json(args.output / "attempts.json", result)
    (args.output / "attempts.md").write_text(render_attempts(result), encoding="utf-8", newline="\n")
    print(f"{result['status']}: {args.output / 'attempts.json'} (not release qualification)")
    return 0 if result["status"] == "PASS" else 2 if result["status"] == "BLOCKED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
