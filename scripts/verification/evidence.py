"""Join actual compiler discovery and test outcomes without inventing coverage."""
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def selected_tests(document, ignored, required_binaries=()):
    selected, all_ids, matched_binaries = [], set(), set()
    for binary, suite in document["rust-suites"].items():
        # Nextest omits discovery of binaries excluded by an exact binary filter.
        # That is permitted only outside this fixed suite's declared selection.
        if (suite["status"] == "skipped" and required_binaries
                and suite["binary-name"] not in required_binaries and not suite["testcases"]):
            continue
        if suite["status"] != "listed":
            raise ValueError(f"Unlisted test binary: {binary}")
        for name, case in suite["testcases"].items():
            identity = f"{binary}::{name}"
            if identity in all_ids:
                raise ValueError(f"Duplicate discovered test: {identity}")
            all_ids.add(identity)
            match = case["filter-match"]["status"]
            if match not in {"matches", "mismatch"}:
                raise ValueError(f"Unknown discovery filter status: {match}")
            if match == "matches" and case["ignored"] == ignored:
                selected.append(identity)
                matched_binaries.add(suite["binary-name"])
    if len(all_ids) != document["test-count"]:
        raise ValueError("Compiler discovery count mismatch")
    missing = set(required_binaries) - matched_binaries
    if missing or not selected:
        raise ValueError(f"Empty required selection: {sorted(missing)}")
    return sorted(selected), all_ids


def read_results(path, selected, all_ids):
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as error:
        raise ValueError("Malformed JUnit results") from error
    if root.tag != "testsuites":
        raise ValueError("Expected Nextest JUnit testsuites")
    observed = {}
    for suite in root.findall("testsuite"):
        for case in suite.findall("testcase"):
            if case.get("classname") != suite.get("name"):
                raise ValueError("JUnit test identity disagrees with its binary")
            identity = f"{case.attrib['classname']}::{case.attrib['name']}"
            if identity in observed or identity not in all_ids:
                raise ValueError(f"Duplicate or undiscovered result: {identity}")
            allowed = {"failure", "error", "flakyFailure", "flakyError", "rerunFailure",
                       "rerunError", "skipped", "system-out", "system-err", "properties"}
            if any(child.tag not in allowed for child in case):
                raise ValueError(f"Unknown test result element: {identity}")
            if any(child.tag in {"failure", "error", "flakyFailure", "flakyError", "rerunFailure", "rerunError"} for child in case):
                status = "FAIL"
            elif case.find("skipped") is not None:
                status = "NOT_RUN"
            else:
                status = "PASS"
            observed[identity] = status
    if int(root.attrib["tests"]) != len(observed):
        raise ValueError("JUnit test count mismatch")
    # An absent/ignored outcome is not proof that the selected test ran.
    return [{"id": identity, "status": observed.get(identity, "NOT_RUN")}
            for identity in selected]


def write_json(path, document):
    path = Path(path)
    staging = path.with_suffix(path.suffix + ".tmp")
    staging.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    staging.replace(path)
