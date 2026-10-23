#!/usr/bin/env python3
"""Select fixed PR checks and reject missing job or suite evidence at the final gate."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

from verification.evidence import read_results, selected_tests, sha256, write_json
from verification.suites import SUITES
from verification.consumer import PRISM_COMMIT, require_supported_qt

TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc")
NATIVE_SUITES = {"default": ["deterministic"], "telemetry": ["cli", "telemetry"]}
JOBS = ("tooling", "lint", "test", "cross-build", "coverage", "live", "consumer", "survey")
FIXTURES = Path(__file__).resolve().parents[1] / "crates/empack-lib/src/engine/build/client/fixtures.json"


def selection(paths, *, trusted, full=False):
    """Deleted paths count too; unknown non-document changes take the native path."""
    paths = sorted(set(paths))
    code = [p for p in paths if not (p.endswith(".md") or (p.startswith("docs/schemas/") and p.endswith(".json")))]
    native = full or bool(code)
    shared = full or any(p in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
                        or p.startswith(("crates/empack-core/", ".github/", "scripts/verification/"))
                        or p in {"scripts/verify.py", "scripts/ci-verification.py"} for p in code)
    provider = shared or any(p.startswith((
        "crates/empack-lib/src/engine/providers", "crates/empack-lib/src/engine/acquisition",
        "crates/empack-lib/src/engine/import", "crates/empack-lib/src/networking",
        "crates/empack-lib/src/application/engine_host/dependencies",
        "crates/empack-tests/tests/live_provider", "crates/empack-tests/tests/provider_catalog",
        "crates/empack-lib/tests/provider_catalog",
    )) for p in code)
    runtime = shared or any(p.startswith((
        "crates/empack-lib/src/engine/runtime", "crates/empack-lib/src/engine/build/server",
        "crates/empack-lib/src/application/process_runtime", "crates/empack-lib/src/application/java",
        "crates/empack-lib/src/engine/api/launch", "crates/empack-lib/src/engine/instance",
        "crates/empack-tests/tests/live_cli_init", "crates/empack-tests/tests/live_server",
        "crates/empack-tests/tests/runtime_", "crates/empack-lib/templates/server/",
        "crates/empack-lib/tests/runtime_",
    )) for p in code)
    consumer = shared or any(p.startswith((
        "crates/empack-lib/src/engine/build/client", "crates/empack-lib/templates/client/",
        "scripts/consumer/", "scripts/check-prism-config.py", "scripts/tests/test_consumer_evidence.py",
    )) for p in code)
    live = []
    if provider:
        live += ["live-provider-cli", "live-modrinth"]
    if full or any(p.startswith(("crates/empack-lib/src/engine/providers/curseforge",
                                "crates/empack-lib/tests/provider_catalog_smoke.rs")) for p in code):
        live.append("live-curseforge")
    if runtime:
        live += ["live-catalog", "server-cli", "runtime-current"]
    required = ["tooling"]
    if native:
        required += ["lint", "test", "cross-build", "coverage"]
    if live:
        required.append("live")
    if consumer:
        required.append("consumer")
    if full:
        required.append("survey")
    return {"schema": 1, "qualification": False, "changed_paths": paths,
            "trusted": trusted, "native": native, "live_suites": live,
            "consumer": consumer, "survey": full, "required_jobs": required,
            "native_targets": list(TARGETS) if native else [],
            "blocked_jobs": [job for job in required if job in {"live", "consumer", "survey"} and not trusted]}


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def make_plan(event, event_name, repository):
    if event_name == "pull_request":
        pr = event["pull_request"]
        base, head = pr["base"]["sha"], pr["head"]["sha"]
        for revision in (base, head):
            if not re.fullmatch("[0-9a-f]{40}", revision):
                raise ValueError("Expected immutable PR revisions")
        paths = subprocess.check_output([
            "git", "diff", "--name-only", "--no-renames", "-z", f"{base}...{head}", "--"
        ]).decode("utf-8").split("\0")
        trusted = pr["head"]["repo"]["full_name"] == repository
        plan = selection(filter(None, paths), trusted=trusted)
        plan.update(base_revision=base, head_revision=head)
    elif event_name == "workflow_dispatch":
        # Dispatch executes a repository ref selected by a maintainer, never a PR checkout.
        plan = selection([], trusted=True, full=True)
    else:
        raise ValueError(f"Unsupported CI planning event: {event_name}")
    plan.update(source_revision=git("rev-parse", "HEAD"), source_tree=git("rev-parse", "HEAD^{tree}"))
    return plan


def check_suite_receipt(directory, expected_suites, target, revision, features="telemetry"):
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
        selected, all_ids = selected_tests(json.loads(inventory.read_text()), suite.ignored, suite.binaries, suite.cases)
        results = read_results(junit, selected, all_ids)
        if selected != row["selected"] or results != row["tests"] or any(r["status"] != "PASS" for r in results):
            raise ValueError(f"Incomplete or inconsistent test outcomes: {row['suite']}")
    return {"receipt_sha256": sha256(path), "target": target, "features": features,
            "artifact_sha256": artifact["sha256"], "suites": expected_suites}


def check_consumer(record, revision):
    if (record["empack_commit"] != revision or record["empack_dirty"]
            or record["prism_commit"] != PRISM_COMMIT
            or record["evidence_scope"] != "supported-dependency headless parser probe"
            or record["fixture_sha256"] != sha256(FIXTURES)):
        raise ValueError("Missing or unrelated supported Prism parser evidence")
    qt = record["qt_version"]
    require_supported_qt(qt)
    if any(record["environment"][key] != qt for key in ("qt_compiled", "qt_runtime")):
        raise ValueError("Consumer Qt runtime and build identity disagree")
    expected = [(case["name"], case["accepted"]) for case in json.loads(FIXTURES.read_text())]
    actual = [(case["fixture"], case["supported"]) for case in record["fixtures"]]
    if actual != expected:
        raise ValueError("Consumer fixture set differs from the required source fixtures")


def check_gate(plan, needs, artifacts):
    if plan["schema"] != 1 or not re.fullmatch("[0-9a-f]{40}", plan["source_revision"]):
        raise ValueError("Malformed CI plan")
    expected = selection(plan["changed_paths"], trusted=plan["trusted"], full=plan["survey"])
    if any(plan.get(key) != value for key, value in expected.items()):
        raise ValueError("CI plan no longer matches its changed paths and trust boundary")
    if plan["blocked_jobs"]:
        raise ValueError("Trusted execution required for: " + ", ".join(plan["blocked_jobs"]))
    for job in ["plan", *plan["required_jobs"]]:
        if job not in {"plan", *JOBS} or needs.get(job, {}).get("result") != "success":
            raise ValueError(f"Required job {job} is missing, failed, cancelled or skipped")
    evidence = []
    for target in plan["native_targets"]:
        for features, suites in NATIVE_SUITES.items():
            evidence.append(check_suite_receipt(artifacts / f"verification-{target}" / features,
                            suites, target, plan["source_revision"], features))
    if plan["live_suites"]:
        evidence.append(check_suite_receipt(artifacts / "verification-live", plan["live_suites"],
                        TARGETS[0], plan["source_revision"]))
    if plan["survey"]:
        evidence.append(check_suite_receipt(artifacts / "verification-survey", ["import-survey"],
                        TARGETS[0], plan["source_revision"]))
    if plan["consumer"]:
        record = json.loads((artifacts / "consumer-prism" / "prism.json").read_text())
        check_consumer(record, plan["source_revision"])
    return {"schema": 1, "status": "PASS", "qualification": False,
            "source_revision": plan["source_revision"], "source_tree": plan["source_tree"],
            "required_jobs": plan["required_jobs"], "suite_evidence": evidence}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    planner = commands.add_parser("plan")
    planner.add_argument("--output", type=Path, required=True)
    gate = commands.add_parser("gate")
    gate.add_argument("--artifacts", type=Path, required=True)
    gate.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "plan":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        plan = make_plan(event, os.environ["GITHUB_EVENT_NAME"], os.environ["GITHUB_REPOSITORY"])
        write_json(args.output, plan)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            for name, value in {"native": plan["native"], "trusted": plan["trusted"],
                                "live": bool(plan["live_suites"]), "consumer": plan["consumer"],
                                "survey": plan["survey"], "live_suites": plan["live_suites"]}.items():
                print(name + "=" + json.dumps(value), file=output)
        print(json.dumps(plan, indent=2))
    else:
        plan = json.loads((args.artifacts / "verification-plan" / "plan.json").read_text())
        if git("rev-parse", "HEAD") != plan["source_revision"]:
            raise ValueError("Gate checkout differs from the planned source")
        try:
            result = check_gate(plan, json.loads(os.environ["CI_NEEDS_JSON"]), args.artifacts)
        except (ValueError, OSError, KeyError) as error:
            write_json(args.output, {"status": "FAIL", "qualification": False, "reason": str(error)})
            raise
        write_json(args.output, result)
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
