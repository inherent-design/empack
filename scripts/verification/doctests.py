"""Observed rustdoc outcomes, distinct from listing and product execution."""
from collections import Counter
from datetime import datetime
import json
from pathlib import Path
import re
import tomllib

from .discovery import rustdoc_identity, rustdoc_tests
from .evidence import aggregate_outcomes, sha256
from .suites import TEST_FEATURES

ROOT = Path(__file__).resolve().parents[2]
BINDINGS = "tests/verification/doctest-bindings.json"
MODES = {"": "run", " - compile": "compile", " - compile fail": "compile_fail"}
TARGETS = {"x86_64-unknown-linux-gnu": "linux", "aarch64-apple-darwin": "macos",
           "x86_64-apple-darwin": "macos", "x86_64-pc-windows-msvc": "windows"}


def command(features, action):
    if features not in TEST_FEATURES or action not in {"list", "ignored", "run"}:
        raise ValueError("Unknown rustdoc graph or action")
    base = ["cargo", "test", "--locked", "--workspace", *TEST_FEATURES[features], "--doc", "--no-fail-fast", "--"]
    return base + ({"list": ["--list"], "ignored": ["--list", "--ignored"],
                   "run": ["--test-threads=1", "--color=never"]}[action])


def inputs(root=ROOT):
    names = (BINDINGS, "Cargo.lock", "rust-toolchain.toml", "scripts/verify.py",
             "scripts/verification/doctests.py", "scripts/verification/discovery.py",
             "scripts/verification/process.py", "scripts/verification/suites.py")
    return {name: sha256(Path(root) / name) for name in names}


def inventory(listing, ignored_listing, classification):
    identities = rustdoc_tests(listing)
    ignored = rustdoc_tests(ignored_listing, allow_empty=True)
    bindings = classification["tests"]
    expected = {row["id"].removeprefix("doctest::"): row for row in bindings}
    if (len(expected) != len(bindings) or identities != expected.keys()
            or not ignored <= identities or any(row.get("mode") not in MODES.values() for row in bindings)
            or ignored != {key for key, row in expected.items() if row["ignored"]}):
        raise ValueError("Rustdoc listing differs from reviewed identities, modes or ignore policy")
    return expected


def outcomes(output, expected):
    """Strict stable pretty format from the pinned toolchain; unknown output fails closed.

    Failure diagnostics may exceed this grammar. Such output remains a failed
    attempt with retained logs, and supplies no passing rows for qualification.
    """
    rows, group, count, summaries = {}, [], None, 0
    for line in output.splitlines():
        if not line:
            continue
        if match := re.fullmatch(r"running ([0-9]+) tests?", line):
            if count is not None:
                raise ValueError("Missing rustdoc result summary")
            count = int(match[1])
            group = []
        elif match := re.fullmatch(r"test (.+ - .*\(line [1-9][0-9]*\))( - compile fail| - compile)? \.\.\. (ok|FAILED|ignored)", line):
            identity, suffix, result = match.groups()
            identity = rustdoc_identity(identity)
            mode = MODES[suffix or ""]
            if count is None or identity not in expected or identity in rows:
                raise ValueError("Unknown, duplicate or ungrouped rustdoc result")
            if expected[identity]["mode"] != mode or expected[identity]["ignored"] != (result == "ignored"):
                raise ValueError("Rustdoc mode or ignore result differs from reviewed classification")
            row = dict(id="doctest::" + identity, mode=mode,
                       status={"ok": "PASS", "FAILED": "FAIL", "ignored": "NOT_RUN"}[result])
            rows[identity] = row
            group.append(row)
        elif match := re.fullmatch(r"test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out; finished in [0-9.]+s", line):
            counts = Counter(row["status"] for row in group)
            if (count is None or len(group) != count
                    or [counts[key] for key in ("PASS", "FAIL", "NOT_RUN")] != [int(match[i]) for i in (2, 3, 4)]
                    or int(match[5]) or int(match[6]) or (match[1] == "FAILED") != bool(counts["FAIL"])):
                raise ValueError("Rustdoc summary disagrees with observed results")
            summaries += 1
            count = None
        elif re.fullmatch(r"all doctests ran in [0-9.]+s; merged doctests compilation took [0-9.]+s", line):
            if count is not None or not summaries:
                raise ValueError("Premature merged rustdoc summary")
        else:
            raise ValueError("Unsupported rustdoc output; inspect retained run log")
    if count is not None or not summaries or rows.keys() != expected.keys():
        raise ValueError("Incomplete rustdoc execution")
    tests = [rows[key] for key in sorted(rows)]
    return tests, aggregate_outcomes(row["status"] for row in tests)


def read_text(path):
    with Path(path).open("rb") as stream:
        data = stream.read(4 * 1024 * 1024 + 1)
    if not 0 < len(data) <= 4 * 1024 * 1024:
        raise ValueError("Rustdoc evidence must be nonempty and at most 4 MiB")
    return data.decode("utf-8")


def inspect_doctest_evidence(path, revision, root=ROOT):
    path, root = Path(path), Path(root)
    try:
        record = json.loads(read_text(path))
        pin = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        if (record["schema"] != 1 or record["kind"] != "doctest-suite-run"
                or record["qualification"] is not False or record["source_revision"] != revision
                or record["worktree_dirty"] is not False or record.get("reason")
                or record["inputs"] != inputs(root) or record["target"] not in TARGETS
                or record["platform"] != TARGETS[record["target"]]
                or not datetime.fromisoformat(record["created_at"]).tzinfo
                or type(record["exit_code"]) is not int or type(record["timed_out"]) is not bool
                or f"release: {pin}" not in record["rustc"].splitlines()
                or f"host: {record['target']}" not in record["rustc"].splitlines()
                or not record["rustdoc"].startswith(f"rustdoc {pin} ")):
            raise ValueError("Unrelated or unsupported rustdoc evidence")
        for action in ("list", "ignored", "run"):
            if record["commands"][action] != command(record["test_features"], action):
                raise ValueError("Rustdoc command changed its selection or feature graph")
        for name in ("list.stdout", "ignored.stdout", "run.stdout", "run.log", "source-snapshot.json"):
            if record["evidence"][name] != sha256(path.parent / name):
                raise ValueError("Rustdoc observations or source snapshot changed")
        expected = inventory(read_text(path.parent / "list.stdout"), read_text(path.parent / "ignored.stdout"),
                             json.loads((root / BINDINGS).read_text()))
        tests, status, parse_error = evaluate(read_text(path.parent / "run.stdout"), expected,
                                              record["exit_code"], record["timed_out"])
        if record["tests"] != tests or record["status"] != status or record.get("parse_error") != parse_error:
            raise ValueError("Rustdoc receipt disagrees with observed output")
        return dict(kind="doctest", source_revision=revision, status=status, tests=tests,
                    target=record["target"], platform=record["platform"], test_features=record["test_features"],
                    rustc=record["rustc"], rustdoc=record["rustdoc"], created_at=record["created_at"],
                    receipt_sha256=sha256(path), source_snapshot_sha256=record["evidence"]["source-snapshot.json"],
                    parse_error=parse_error)
    except (KeyError, TypeError, AttributeError) as error:
        raise ValueError("Malformed rustdoc execution evidence") from error


def evaluate(output, expected, code, timed_out):
    try:
        tests, status = outcomes(output, expected)
        return tests, "FAIL" if code or timed_out else status, None
    except ValueError as error:
        return [], "FAIL", str(error)
