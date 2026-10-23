"""Acceptance cases and required variants, independent of suite scheduling."""
from .classification import BOUNDARIES
from .evidence import aggregate_outcomes


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
            if "required_targets" in case:
                targets = case["required_targets"]
                if (case["boundary"] != "P" or not targets or len(set(targets)) != len(targets)):
                    raise ValueError(f"Exact target variants require a package case: {case['id']}")
            if not case["test_ids"] or len(set(case["test_ids"])) != len(case["test_ids"]):
                raise ValueError(f"Case has no concrete tests: {case['id']}")
    return requirements, cases


def case_results(document, classification, evidence, revision, *, package_receipts=()):
    """Join already validated suite evidence; no test counts imply group coverage.

    The CI gate authenticates revision/cleanliness and re-reads inventory/JUnit
    before calling this function. This function is not an evidence-file loader.
    Missing cases, variants and incomplete decomposition remain NOT_RUN. Only the
    archive gate may supply package_receipts after binding archive/build/execution
    identities; ordinary suite receipts cannot assert their own package context.
    """
    validate_requirements(document)
    package_receipts = set(package_receipts)
    if not package_receipts <= {record["receipt_sha256"] for record in evidence}:
        raise ValueError("Package proof lacks its admitted suite receipt")
    bindings = {row["id"]: row for row in classification["tests"]}
    targets = classification["targets"]
    observed = {}
    receipts = {}
    for record in evidence:
        if record["source_revision"] != revision:
            raise ValueError("Acceptance evidence belongs to another revision")
        digest = record["receipt_sha256"]
        if digest in receipts and receipts[digest] != record:
            raise ValueError(f"Conflicting receipt evidence: {digest}")
        receipts[digest] = record
        seen_tests = set()
        platform = targets[record["target"]]
        features = record["features"]
        if features not in {"default", "telemetry"}:
            raise ValueError("Unknown acceptance product feature variant")
        graph = record["test_features"]
        if graph not in {"minimal", "all"}:
            raise ValueError("Unknown acceptance test feature graph")
        for test in record["tests"]:
            identity = test["id"]
            run_key = (identity, test.get("suite", ""))
            if run_key in seen_tests:
                raise ValueError(f"Duplicate test in receipt: {identity}")
            seen_tests.add(run_key)
            if (identity not in bindings or platform not in bindings[identity]["platforms"]
                    or graph not in bindings[identity]["test_features"]):
                raise ValueError(f"Acceptance evidence has an unclassified test variant: {identity}")
            if test["status"] not in {"PASS", "FAIL", "BLOCKED", "NOT_RUN"}:
                raise ValueError(f"Unexpected acceptance test status: {identity}")
            key = (identity, platform, features)
            outcome = {"status": test["status"], "artifact_sha256": record["artifact_sha256"],
                       "test_features": graph, "receipt_sha256": digest, "target": record["target"]}
            if "created_at" in record:
                outcome["created_at"] = record["created_at"]
            if "suite" in test:
                outcome["suite"] = test["suite"]
            observed.setdefault(key, {})[(digest, test.get("suite", ""))] = outcome

    def test_result(identity, platform, features, target, packaged):
        entries = {key: value for key, value in observed.get((identity, platform, features), {}).items()
                   if (not target or value["target"] == target)
                   and (not packaged or key[0] in package_receipts)}
        attempts = [entries[key] for key in sorted(entries)]
        result = {"id": identity, "status": aggregate_outcomes(a["status"] for a in attempts),
                  "receipts": sorted({key[0] for key in entries}), "attempts": attempts}
        # Retain the compact representation only when every attempt agrees on
        # product identity and graph. Reports must not select an arbitrary winner.
        for field in ("artifact_sha256", "test_features"):
            values = {a[field] for a in attempts}
            if len(values) == 1:
                result[field] = values.pop()
        return result

    requirements = []
    for row in document["requirements"]:
        cases = []
        for case in row["cases"]:
            for identity in case["test_ids"]:
                binding = bindings.get(identity)
                if binding is None or not (binding["boundary"] == case["boundary"]
                        or case["boundary"] == "P" and binding["boundary"] == "X"):
                    raise ValueError(f"Acceptance case lacks its required test boundary: {case['id']} / {identity}")
                if not set(case["required_platforms"]) <= set(binding["platforms"]):
                    raise ValueError(f"Acceptance case requires an unclassified platform: {case['id']}")
            variants = []
            exact_targets = case.get("required_targets", [])
            if exact_targets and (not set(exact_targets) <= targets.keys()
                                  or {targets[t] for t in exact_targets} != set(case["required_platforms"])):
                raise ValueError(f"Package targets disagree with required platforms: {case['id']}")
            hosts = [(targets[t], t) for t in exact_targets] if exact_targets else [
                (platform, None) for platform in case["required_platforms"]]
            for platform, target in hosts:
                for features in case["required_product_features"]:
                    tests = [test_result(identity, platform, features, target, case["boundary"] == "P")
                             for identity in case["test_ids"]]
                    variant = {"platform": platform, "features": features, "tests": tests,
                               "status": aggregate_outcomes(test["status"] for test in tests)}
                    if target:
                        variant["target"] = target
                    variants.append(variant)
            cases.append({"id": case["id"], "boundary": case["boundary"], "variants": variants,
                          "status": aggregate_outcomes([variant["status"] for variant in variants])})
        states = [case["status"] for case in cases]
        if row["case_bindings_status"] != "complete":
            states.append("NOT_RUN")
        requirements.append({"id": row["id"], "title": row.get("title", row["id"]), "scope": row["scope"], "risk": row["risk"],
                             "case_bindings_status": row["case_bindings_status"],
                             "status": aggregate_outcomes(states), "cases": cases})
    return {"source_revision": revision, "qualification": False, "requirements": requirements}

