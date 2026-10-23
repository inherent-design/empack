"""Candidate admission from rechecked suites and the exact tested archive payload."""
from datetime import datetime, timezone
import json
from pathlib import Path

from .evidence import aggregate_outcomes, sha256
from .receipts import inspect_suite_receipt
from .release import TARGETS, check_payload, git
from .release_inputs import release_version, stamped_sources
from .requirements import case_results

NATIVE = {f"UX-{number:03}" for number in range(1, 89)}
PORTABLE = {f"PORT-{number:03}" for number in range(1, 22)}


def applicable_requirements(version):
    release_version(version)
    major, minor, _ = map(int, version.split("-", 1)[0].split("."))
    return NATIVE | PORTABLE if (major, minor) >= (0, 9) else NATIVE


def evaluate_requirements(document, classification, evidence, revision, version, package_receipts):
    """Evaluate admitted observations, not self-reported requirement PASS labels."""
    release_version(version)
    by_id = {row["id"]: row for row in document["requirements"]}
    if set(by_id) != NATIVE | PORTABLE or len(by_id) != len(document["requirements"]):
        raise ValueError("Candidate registry must retain all 88 native and 21 portable requirements")
    for identity, row in by_id.items():
        if row["scope"] != ("stable" if identity in NATIVE else "v0.9-new"):
            raise ValueError(f"Requirement applicability changed without a candidate policy change: {identity}")
    # Portable obligations become mandatory at their planned release, never by
    # silently changing the denominator or relying on a caller's exclusions.
    applicable = applicable_requirements(version)
    acceptance = case_results(document, classification, evidence, revision, package_receipts=package_receipts)
    blockers = []
    for row in acceptance["requirements"]:
        row["applicable"] = row["id"] in applicable
        if not row["applicable"]:
            continue
        declared = by_id[row["id"]]
        reasons = []
        if row["case_bindings_status"] != "complete":
            reasons.append("Case decomposition is " + row["case_bindings_status"])
        absent = set(declared["evidence_tiers"]) - {case["boundary"] for case in declared["cases"]}
        if absent:
            reasons.append("Missing required evidence boundaries: " + ", ".join(sorted(absent)))
        for case in declared["cases"]:
            if case["boundary"] == "P" and set(case.get("required_targets", [])) != set(TARGETS):
                reasons.append("Package case must retain all five execution targets: " + case["id"])
        if row["status"] != "PASS":
            reasons.append("Required observations are " + row["status"])
        if reasons:
            blockers.append({"requirement": row["id"], "reasons": reasons})
    # No approved low-risk waiver policy is installed. Missing critical or actual
    # consumer evidence cannot be waived by setting a result field to WAIVED.
    states = [row["status"] for row in acceptance["requirements"] if row["applicable"]]
    for record in evidence:
        if record.get("status") != "PASS":
            blockers.append({"requirement": "candidate", "reasons": [
                f"Suite receipt {record['receipt_sha256']} is {record.get('status', 'missing its outcome')}"]})
    observed = aggregate_outcomes(states + [record["status"] for record in evidence])
    status = "FAIL" if observed == "FAIL" else "BLOCKED" if blockers else "PASS"
    return {"status": status, "qualification": status == "PASS", "acceptance": acceptance,
            "applicable_requirements": sorted(applicable), "blockers": blockers,
            "waivers": [], "waiver_policy": "No waiver admission; all applicable obligations must pass"}


def qualify(root, payload, payload_sha256, revision, version, receipt_paths=()):
    root = Path(root)
    if git(root, "rev-parse", "HEAD") != revision or git(root, "status", "--porcelain"):
        raise ValueError("Candidate gate requires its exact clean source checkout")
    stamped_sources(root, version)  # Check the reviewed version without mutating source.
    archives = check_payload(payload, version, revision, payload_sha256)
    registry = root / "tests/verification/requirements.json"
    bindings = root / "tests/verification/test-bindings.json"
    document, classification = (json.loads(path.read_text(encoding="utf-8")) for path in (registry, bindings))
    evidence = [entry["suite"] for entry in archives["targets"]]
    package_receipts = {record["receipt_sha256"] for record in evidence}
    binaries = {entry["suite"]["target"]: entry["executable"]["sha256"] for entry in archives["targets"]}
    for path in receipt_paths:
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
        artifact = raw["artifact"]
        record = inspect_suite_receipt(path, raw["requested_suites"], artifact["target"], revision,
                                       artifact["features"], version, "release", raw["test_features"])
        # This first producer path admits the actual shipped product only.
        # Additional feature builds need their own bound build evidence, not an
        # arbitrary binary described by a caller-supplied receipt.
        if artifact["features"] != "default" or binaries.get(artifact["target"]) != artifact["sha256"]:
            raise ValueError("Candidate suite did not execute the admitted release binary")
        evidence.append(record)
    result = evaluate_requirements(document, classification, evidence, revision, version, package_receipts)
    result["acceptance"].update(requirements_sha256=sha256(registry), classification_sha256=sha256(bindings))
    result.update(schema=1, kind="release-candidate-qualification", source_revision=revision,
                  source_tree=git(root, "rev-parse", "HEAD^{tree}"), version=version,
                  created_at=datetime.now(timezone.utc).isoformat(), archive_evidence_sha256=payload_sha256,
                  suite_evidence=[{key: value for key, value in record.items() if key != "tests"} for record in evidence])
    return result


def check_qualification(path, digest, revision, version, archive_digest):
    """Publication trusts only the immutable output of its candidate gate job."""
    if sha256(path) != digest:
        raise ValueError("Candidate evidence differs from the qualification gate output")
    record = json.loads(Path(path).read_text(encoding="utf-8"))
    if (record.get("schema") != 1 or record.get("kind") != "release-candidate-qualification"
            or record.get("status") != "PASS" or record.get("qualification") is not True
            or record.get("source_revision") != revision or record.get("version") != version
            or record.get("archive_evidence_sha256") != archive_digest or record.get("blockers")
            or record.get("waivers") != []):
        raise ValueError("Candidate is unqualified, incomplete or belongs to another archive payload")
    applicable = applicable_requirements(version)
    acceptance = record.get("acceptance", {})
    rows = acceptance.get("requirements", [])
    if (acceptance.get("source_revision") != revision
            or record.get("applicable_requirements") != sorted(applicable)
            or len(rows) != len(NATIVE | PORTABLE)
            or {row["id"] for row in rows} != NATIVE | PORTABLE
            or any(row.get("applicable") != (row["id"] in applicable)
                   or row["id"] in applicable and (row["status"] != "PASS" or row["case_bindings_status"] != "complete")
                   for row in rows)):
        raise ValueError("Candidate qualification lost required observations or applicability")
    return record
