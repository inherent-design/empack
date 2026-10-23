"""Present admitted CI evidence without turning it into release qualification."""
from collections import Counter
from html import escape


def cell(value):
    # Titles and diagnostics are data, not executable HTML or table structure.
    return "<code>" + escape(str(value)).replace("|", "&#124;").replace("\n", "<br>") + "</code>"


def render_gate(record):
    if record.get("schema") != 1 or record.get("qualification") is not False:
        raise ValueError("Expected a nonqualifying CI gate record")
    if record.get("status") not in {"PASS", "FAIL"}:
        raise ValueError("Unknown CI gate outcome")
    lines = ["# Verification evidence", "",
             f"CI gate: **{record['status']}**. Release qualification has **not** been established.", "",
             "This view summarizes the gate JSON. It does not rerun checks or admit new evidence.", "",
             f"Source: {cell(record.get('source_revision', 'not recorded'))}", "",
             f"Tree: {cell(record.get('source_tree', 'not recorded'))}", "",
             f"Report generated: {cell(record.get('created_at', 'not recorded'))}", ""]
    if record["status"] == "FAIL":
        lines += ["## Evidence admission failed", "", cell(record.get("reason", "No diagnostic recorded")), "",
                  "No complete acceptance join was admitted. A missing or rejected record is not a pass.", "",
                  "Retain the job artifacts for failed attempts, skipped prerequisites and detailed diagnostics.", ""]
        return "\n".join(lines)

    return "\n".join(lines + _acceptance_lines(record))


def _acceptance_lines(record):
    lines = []
    acceptance = record["acceptance"]
    if acceptance.get("source_revision") != record.get("source_revision") or acceptance.get("qualification") is not False:
        raise ValueError("Acceptance view disagrees with the CI source or qualification state")
    requirements = acceptance["requirements"]
    counts = Counter(row["status"] for row in requirements)
    if not set(counts) <= {"PASS", "FAIL", "BLOCKED", "NOT_RUN"}:
        raise ValueError("Unsupported acceptance status; waivers require a separate approval policy")
    lines += ["## Requirement status", "",
              "Counts describe requirement groups, not source-line coverage or test-function coverage.", "",
              "| PASS | FAIL | BLOCKED | NOT_RUN |", "| ---: | ---: | ---: | ---: |",
              "| " + " | ".join(str(counts[s]) for s in ("PASS", "FAIL", "BLOCKED", "NOT_RUN")) + " |", "",
              "Pending or partial decomposition remains NOT_RUN even when every currently bound case passes.", "",
              f"Requirements SHA-256: {cell(acceptance['requirements_sha256'])}", "",
              f"Classification SHA-256: {cell(acceptance['classification_sha256'])}", "",
              "| Requirement | Title | Scope | Risk | Bindings | Result |",
              "| --- | --- | --- | --- | --- | --- |"]
    for row in requirements:
        lines.append("| " + " | ".join(cell(row.get(k, row['id'])) for k in
                     ("id", "title", "scope", "risk", "case_bindings_status", "status")) + " |")

    lines += ["", "## Admitted suite receipts", "",
              "Executed test rows were checked against compiler discovery and JUnit. Preflight failures supply no passing tests.", ""]
    for suite in record["suite_evidence"]:
        lines += [f"### {cell(suite['target'])} / {cell(suite['features'])}", "",
                  f"Product profile: {cell(suite['profile'])}; test graph: {cell(suite['test_features'])}.", "",
                  f"Suites: {cell(', '.join(suite['suites']))}", "",
                  f"Executable SHA-256: {cell(suite['artifact_sha256'])}", "",
                  f"Receipt SHA-256: {cell(suite['receipt_sha256'])}", "",
                  f"Source snapshot SHA-256: {cell(suite['source_snapshot_sha256'])}", ""]
    if not record["suite_evidence"]:
        lines += ["No native suite receipts were selected or admitted by this gate.", ""]

    if record.get("tooling_evidence"):
        lines += ["## Python tooling observations", "",
                  "These unittest observations exercise verification helpers and scripts, independently of product builds.", ""]
        for tooling in record["tooling_evidence"]:
            lines += [f"- {cell(tooling['platform'])}: {cell(tooling['status'])}; Python {cell(tooling['python_version'])}",
                      f"  - Interpreter SHA-256: {cell(tooling['interpreter_sha256'])}",
                      f"  - Receipt SHA-256: {cell(tooling['receipt_sha256'])}"]
        lines += [""]

    if record.get("consumer_evidence"):
        lines += ["## Consumer parser observations", "",
                  "These probes execute pinned consumer source. They do not execute empack or establish launcher import/startup.", ""]
        for consumer in record["consumer_evidence"]:
            lines += [f"- {cell(consumer['platform'])}: {cell(consumer['status'])}; Qt {cell(consumer['qt_version'])}",
                      f"  - Prism source: {cell(consumer['prism_commit'])}; probe SHA-256: {cell(consumer['probe_sha256'])}",
                      f"  - Receipt SHA-256: {cell(consumer['receipt_sha256'])}"]
        lines += [""]

    lines += ["## Bound cases and execution variants", "",
              "Missing evidence stays NOT_RUN. A pure, host, or parser check cannot replace another case's required boundary.", ""]
    for row in requirements:
        for case in row["cases"]:
            lines += [f"### {cell(case['id'])}", "",
                      f"Boundary: {cell(case['boundary'])}; case result: {cell(case['status'])}.", "",
                      "| Platform | Target | Product features | Result |", "| --- | --- | --- | --- |"]
            for variant in case["variants"]:
                lines.append("| " + " | ".join(cell(variant.get(k, "any classified target"))
                             for k in ("platform", "target", "features", "status")) + " |")
            lines += ["", "<details>", "<summary>Exact tests and evidence identities</summary>", ""]
            for variant in case["variants"]:
                for test in variant["tests"]:
                    lines += [f"- {cell(variant['platform'])} / {cell(variant['features'])}: {cell(test['id'])} — {cell(test['status'])}"]
                    if test.get("attempts"):
                        for attempt in test["attempts"]:
                            lines += [f"  - Attempt: {cell(attempt['status'])}; receipt: {cell(attempt['receipt_sha256'])}",
                                      f"    - Test graph: {cell(attempt['test_features'])}; " +
                                      (f"Python interpreter: {cell(attempt['interpreter_sha256'])}" if "interpreter_sha256" in attempt
                                       else f"consumer probe: {cell(attempt['probe_sha256'])}" if "probe_sha256" in attempt
                                       else f"empack executable: {cell(attempt['artifact_sha256'])}")]
                            if attempt.get("target"):
                                lines += [f"    - Target: {cell(attempt['target'])}"]
                            if attempt.get("suite"):
                                lines += [f"    - Suite: {cell(attempt['suite'])}"]
                            if attempt.get("created_at"):
                                lines += [f"    - Recorded: {cell(attempt['created_at'])}"]
                    elif test["receipts"]:
                        # Render earlier admitted schema-1 reports too.
                        lines += [f"  - Test graph: {cell(test['test_features'])}; executable: {cell(test['artifact_sha256'])}",
                                  f"  - Receipts: {cell(', '.join(test['receipts']))}"]
                    else:
                        lines += ["  - No admitted execution for this test and variant."]
            lines += ["", "</details>", ""]
    lines += ["## Remaining qualification", "",
              "This CI report does not apply waivers or certify the release. Complete case decomposition,",
              "non-Rust and actual-consumer evidence, required devices and final archive execution remain",
              "separate obligations. No result is inferred from the number or names of discovered tests.", ""]
    return lines


def render_attempts(record):
    if (record.get("schema") != 1 or record.get("kind") != "verification-attempt-report"
            or record.get("qualification") is not False
            or record.get("status") not in {"PASS", "FAIL", "BLOCKED", "NOT_RUN", "NOT_APPLICABLE"}):
        raise ValueError("Expected a nonqualifying attempt report")
    lines = ["# Verification attempts", "",
             f"Selected attempts: **{record['status']}**. This is not release qualification.", "",
             "All supplied attempts remain visible. A later pass does not erase a failed attempt.", "",
             f"Expected source: {cell(record['source_revision'])}", "",
             f"Expected product version: {cell(record['version'])}", ""]
    for attempt in record["attempts"]:
        lines += [f"## Receipt {cell(attempt['receipt_sha256'])}", "",
                  f"Admission: {cell(attempt['admission'])}; outcome: {cell(attempt['status'])}.", ""]
        if attempt.get("path"):
            lines += [f"Input: {cell(attempt['path'])}", ""]
        if attempt.get("reason"):
            lines += [cell(attempt["reason"]), ""]
        if attempt["admission"] == "REJECTED":
            lines += ["No test outcomes from this receipt were admitted.", ""]
            continue
        if attempt.get("kind") == "python":
            lines += [f"Recorded: {cell(attempt['created_at'])}; platform: {cell(attempt['platform'])}; "
                      f"Python: {cell(attempt['python_version'])}. No empack artifact identity is established by this record.", ""]
            continue
        lines += [f"Recorded: {cell(attempt['created_at'])}; target: {cell(attempt['target'])}; product: {cell(attempt['features'])}.", ""]
        for suite in attempt["suite_outcomes"]:
            lines += [f"- {cell(suite['suite'])}: {cell(suite['status'])}"]
            if suite.get("reason"):
                lines += [f"  - {cell(suite['reason'])}"]
        lines += [""]
    lines += ["## Admitted observations", "",
              "These rows preserve observed test results, including failures. They do not approve the selected runs or the release.", ""]
    return "\n".join(lines + _acceptance_lines(record))
