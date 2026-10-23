"""Acceptance cases and required variants, independent of suite scheduling."""
from .classification import BOUNDARIES


def validate_requirements(document):
    if document["schema"] != 1:
        raise ValueError("Unsupported requirement schema")
    requirements, cases = set(), set()
    for row in document["requirements"]:
        if row["id"] in requirements:
            raise ValueError(f"Duplicate requirement: {row['id']}")
        requirements.add(row["id"])
        state = row["case_bindings_status"]
        if state not in {"pending", "partial", "complete"}:
            raise ValueError(f"Unknown binding status: {row['id']}")
        if (state == "pending") != (not row["cases"]):
            raise ValueError(f"Binding status disagrees with concrete cases: {row['id']}")
        if not row["evidence_tiers"] or not set(row["evidence_tiers"]) <= BOUNDARIES:
            raise ValueError(f"Unknown evidence boundary: {row['id']}")
        for case in row["cases"]:
            if case["id"] in cases:
                raise ValueError(f"Duplicate case: {case['id']}")
            cases.add(case["id"])
            if case["boundary"] not in row["evidence_tiers"]:
                raise ValueError(f"Case uses an unrelated evidence boundary: {case['id']}")
            platforms, features = case["required_platforms"], case["required_product_features"]
            if (not platforms or not features or len(set(platforms)) != len(platforms)
                    or len(set(features)) != len(features)
                    or not set(platforms) <= {"linux", "macos", "windows"}
                    or not set(features) <= {"default", "telemetry"}):
                raise ValueError(f"Case has no required execution variants: {case['id']}")
            if not case["test_ids"] or len(set(case["test_ids"])) != len(case["test_ids"]):
                raise ValueError(f"Case has no concrete tests: {case['id']}")
    return requirements, cases


def case_results(document, classification, evidence, revision):
    """Join already validated suite evidence; no test counts imply group coverage.

    The CI gate authenticates revision/cleanliness and re-reads inventory/JUnit
    before calling this function. This function is not an evidence-file loader.
    Missing cases, variants and incomplete decomposition remain NOT_RUN.
    """
    validate_requirements(document)
    bindings = {row["id"]: row for row in classification["tests"]}
    targets = classification["targets"]
    observed = {}
    for record in evidence:
        if record["source_revision"] != revision:
            raise ValueError("Acceptance evidence belongs to another revision")
        platform = targets[record["target"]]
        features = record["features"]
        if features not in {"default", "telemetry"}:
            raise ValueError("Unknown acceptance product feature variant")
        for test in record["tests"]:
            identity = test["id"]
            if identity not in bindings or platform not in bindings[identity]["platforms"]:
                raise ValueError(f"Acceptance evidence has an unclassified test variant: {identity}")
            if test["status"] not in {"PASS", "FAIL", "BLOCKED", "NOT_RUN"}:
                raise ValueError(f"Unexpected acceptance test status: {identity}")
            key = (identity, platform, features)
            outcome = {"status": test["status"], "artifact_sha256": record["artifact_sha256"]}
            if key in observed and observed[key]["outcome"] != outcome:
                raise ValueError(f"Conflicting acceptance evidence: {identity}")
            entry = observed.setdefault(key, {"outcome": outcome, "receipts": set()})
            entry["receipts"].add(record["receipt_sha256"])

    def status(outcomes):
        for value in ("FAIL", "BLOCKED", "NOT_RUN"):
            if value in outcomes:
                return value
        return "PASS" if outcomes else "NOT_RUN"

    requirements = []
    for row in document["requirements"]:
        cases = []
        for case in row["cases"]:
            for identity in case["test_ids"]:
                binding = bindings.get(identity)
                if binding is None or binding["boundary"] != case["boundary"]:
                    raise ValueError(f"Acceptance case lacks its required test boundary: {case['id']} / {identity}")
                if not set(case["required_platforms"]) <= set(binding["platforms"]):
                    raise ValueError(f"Acceptance case requires an unclassified platform: {case['id']}")
            variants = []
            for platform in case["required_platforms"]:
                for features in case["required_product_features"]:
                    tests = []
                    for identity in case["test_ids"]:
                        entry = observed.get((identity, platform, features))
                        tests.append({"id": identity, **(entry["outcome"] if entry else {"status": "NOT_RUN"}),
                                      "receipts": sorted(entry["receipts"]) if entry else []})
                    variants.append({"platform": platform, "features": features, "tests": tests,
                                     "status": status([test["status"] for test in tests])})
            cases.append({"id": case["id"], "boundary": case["boundary"], "variants": variants,
                          "status": status([variant["status"] for variant in variants])})
        states = [case["status"] for case in cases]
        if row["case_bindings_status"] != "complete":
            states.append("NOT_RUN")
        requirements.append({"id": row["id"], "scope": row["scope"], "risk": row["risk"],
                             "case_bindings_status": row["case_bindings_status"],
                             "status": status(states), "cases": cases})
    return {"source_revision": revision, "qualification": False, "requirements": requirements}

