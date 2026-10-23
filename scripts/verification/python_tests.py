"""Unittest lifecycle observations; independent of empack product execution."""
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import unittest

from .evidence import aggregate_outcomes, sha256

ROOT = Path(__file__).resolve().parents[2]
BINDINGS = "tests/verification/python-bindings.json"
PLATFORMS = ("linux", "macos", "windows")
PRODUCER_INPUTS = ("scripts/verify.py", "scripts/python-test-driver.py", "scripts/verification/python_tests.py")


def inputs(root=ROOT):
    root = Path(root)
    names = {BINDINGS, "tests/verification/requirements.json", *PRODUCER_INPUTS}
    names.update(p.relative_to(root).as_posix() for p in (root / "scripts").rglob("*.py"))
    return {name: sha256(root / name) for name in sorted(names)}


def load_suite(directory):
    loader = unittest.TestLoader()
    suite = loader.discover(str(directory))
    if loader.errors:
        raise ValueError("Python test discovery failed:\n" + "\n".join(loader.errors))

    def identities(node):
        if isinstance(node, unittest.TestSuite):
            for child in node:
                yield from identities(child)
        else:
            method = getattr(node, node._testMethodName)
            ignored = getattr(node, "__unittest_skip__", False) or getattr(method, "__unittest_skip__", False)
            yield {"id": node.id(), "ignored": bool(ignored)}

    tests = sorted(identities(suite), key=lambda row: row["id"])
    if not tests or len({row["id"] for row in tests}) != len(tests):
        raise ValueError("Python discovery must contain unique, nonempty test identities")
    return suite, {"schema": 1, "source": "unittest-loader", "tests": tests}


def compare_inventory(inventory, classification):
    if inventory.get("schema") != 1 or inventory.get("source") != "unittest-loader":
        raise ValueError("Expected unittest loader inventory")
    actual = {"python::" + row["id"]: row for row in inventory["tests"]}
    expected = {row["id"]: row for row in classification["tests"]}
    if (not actual or len(actual) != len(inventory["tests"])
            or len(expected) != len(classification["tests"]) or actual.keys() != expected.keys()
            or any(type(row["ignored"]) is not bool for row in actual.values())):
        raise ValueError("Python inventory differs from its unique classified identities")
    if any(not key.startswith("python::") or row["boundary"] not in {"U", "H", "N", "X"}
           for key, row in expected.items()):
        raise ValueError("Python helper tests cannot claim consumer or product qualification")
    return actual


class ObservedResult(unittest.TextTestResult):
    """Retain every callback; subtest failure cannot be overwritten by success."""
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.events = []

    def event(self, test, event, status=None):
        self.events.append(dict(id=test.id(), event=event, **({"status": status} if status else {})))

    def startTest(self, test):
        self.event(test, "start")
        super().startTest(test)

    def stopTest(self, test):
        self.event(test, "stop")
        super().stopTest(test)

    def addSuccess(self, test):
        self.event(test, "result", "PASS")
        super().addSuccess(test)

    def addFailure(self, test, err):
        self.event(test, "result", "FAIL")
        super().addFailure(test, err)

    def addError(self, test, err):
        self.event(test, "result", "FAIL")
        super().addError(test, err)

    def addSkip(self, test, reason):
        self.event(test, "result", "NOT_RUN")
        super().addSkip(test, reason)

    def addExpectedFailure(self, test, err):
        self.event(test, "result", "FAIL")
        super().addExpectedFailure(test, err)

    def addUnexpectedSuccess(self, test):
        self.event(test, "result", "FAIL")
        super().addUnexpectedSuccess(test)

    def addSubTest(self, test, subtest, err):
        self.event(test, "subtest", "FAIL" if err else "PASS")
        super().addSubTest(test, subtest, err)


def outcomes(inventory, events):
    """Replay lifecycle evidence, requiring exactly one complete run per identity."""
    rows = {row["id"]: row for row in inventory["tests"]}
    if not rows or len(rows) != len(inventory["tests"]) or len(events) > 100000:
        raise ValueError("Invalid Python discovery or oversized lifecycle record")
    states, results, active, fixture_failure = {}, {}, None, False
    for event in events:
        identity, kind = event["id"], event["event"]
        if identity not in rows:
            # unittest reports class/module setup/teardown failures as error holders.
            # They may stop discovered tests from starting; never create new tests.
            if kind != "result" or event.get("status") not in {"FAIL", "NOT_RUN"}:
                raise ValueError("Unclassified Python lifecycle observation")
            fixture_failure = True
            continue
        if kind == "start":
            if active is not None or identity in states or "status" in event:
                raise ValueError("Repeated or overlapping Python test execution")
            active = identity
            states[identity] = "running"
            results[identity] = []
        elif kind in {"result", "subtest"}:
            if active != identity or event.get("status") not in {"PASS", "FAIL", "NOT_RUN"}:
                raise ValueError("Python outcome lacks its active test")
            results[identity].append(event["status"])
        elif kind == "stop":
            if active != identity or not results[identity] or "status" in event:
                raise ValueError("Python test did not establish a terminal result")
            states[identity] = "finished"
            active = None
        else:
            raise ValueError("Unknown Python lifecycle event")
    tests = []
    for identity in sorted(rows):
        status = aggregate_outcomes(results.get(identity, [])) if states.get(identity) == "finished" else "NOT_RUN"
        if rows[identity]["ignored"] and status == "PASS":
            raise ValueError("Skipped Python test cannot supply passing evidence")
        tests.append(dict(id="python::" + identity, status=status))
    status = "FAIL" if fixture_failure else aggregate_outcomes(row["status"] for row in tests)
    return tests, status


def read_json(path):
    with Path(path).open("rb") as stream:
        data = stream.read(4 * 1024 * 1024 + 1)
    if not 0 < len(data) <= 4 * 1024 * 1024:
        raise ValueError("Python evidence must be nonempty and at most 4 MiB")
    return json.loads(data), hashlib.sha256(data).hexdigest()


def inspect_python_evidence(path, revision, root=ROOT):
    try:
        record, digest = read_json(path)
        if (record.get("schema") != 1 or record.get("kind") != "python-suite-run"
                or record["source_revision"] != revision or record["worktree_dirty"] is not False
                or record["inputs"] != inputs(root) or record["features"] != "independent"
                or record["platform"] not in PLATFORMS or not record["architecture"]
                or not datetime.fromisoformat(record["created_at"]).tzinfo
                or not re.fullmatch(r"3\.(?:1[1-9]|[2-9][0-9])\.\d+", record["python_version"])
                or type(record["timed_out"]) is not bool or type(record["exit_code"]) is not int
                or not re.fullmatch(r"[a-f0-9]{64}", record["interpreter_sha256"])):
            raise ValueError("Unrelated or unsupported Python execution evidence")
        inventory, inventory_digest = read_json(Path(path).parent / "inventory.json")
        events, events_digest = read_json(Path(path).parent / "events.json")
        classification = json.loads((Path(root) / BINDINGS).read_text(encoding="utf-8"))
        compare_inventory(inventory, classification)
        if (record["inventory_sha256"] != inventory_digest or record["events_sha256"] != events_digest
                or record["classification_sha256"] != sha256(Path(root) / BINDINGS)):
            raise ValueError("Python discovery, observations or classification changed")
        tests, status = outcomes(inventory, events)
        if record["timed_out"] or record["exit_code"] != 0:
            status = "FAIL"
        if record["tests"] != tests or record["status"] != status:
            raise ValueError("Python result differs from its observed lifecycle")
        return dict(kind="python", source_revision=revision, features="independent", tests=tests,
                    status=status, platform=record["platform"], architecture=record["architecture"],
                    python_version=record["python_version"], interpreter_sha256=record["interpreter_sha256"],
                    created_at=record["created_at"], receipt_sha256=digest,
                    inventory_sha256=inventory_digest, events_sha256=events_digest)
    except (KeyError, TypeError, AttributeError) as error:
        raise ValueError("Malformed Python execution evidence") from error
