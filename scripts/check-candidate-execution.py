#!/usr/bin/env python3
"""Run fixed native/live selections on the exact candidate binaries; never build a product."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import uuid
import zipfile

import verify
from verification.auxiliary import inspect_auxiliary
from verification.candidate_execution import GRAPHS, PHASES, NATIVE_TARGETS, inspect_execution, inspect_runs
from verification.evidence import aggregate_outcomes, sha256, write_json
from verification.package_archive import capture_archive, capture_binary, extract_binary
from verification.release import check_payload, git

ROOT = Path(__file__).resolve().parents[1]


def execute(directory, binaries, artifacts, target, version):
    """Keep native/live and both feature results independent, without a retry."""
    for features, graph in GRAPHS.items():
        for phase, suites in PHASES.items():
            if sha256(binaries[features]) != artifacts[features]:
                raise ValueError("Candidate executable changed before execution")
            code = verify.main([*suites, "--binary", str(binaries[features]),
                                "--sha256", artifacts[features], "--target", target,
                                "--version", version, "--product-features", features,
                                "--product-profile", "release", "--test-features", graph,
                                "--output", str(directory / features / phase)])
            receipts = list((directory / features / phase).glob("*/receipt.json"))
            if len(receipts) != 1:
                raise ValueError("Candidate runner lost its single native/live attempt")
            status = json.loads(receipts[0].read_text(encoding="utf-8"))["status"]
            if code != {"PASS": 0, "BLOCKED": 2}.get(status, 1):
                raise ValueError("Candidate runner status differs from its terminal result")
            if sha256(binaries[features]) != artifacts[features]:
                raise ValueError("Candidate executable changed during execution")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--payload", type=Path, required=True)
    parser.add_argument("--archive-evidence-sha256", required=True)
    parser.add_argument("--auxiliary", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "target/verification-candidate")
    args = parser.parse_args()
    attempt = args.output.resolve() / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    record = dict(schema=1, kind="candidate-native-execution", qualification=False, status="NOT_RUN",
                  created_at=datetime.now(timezone.utc).isoformat(), source_revision=args.revision,
                  version=args.version, archive_evidence_sha256=args.archive_evidence_sha256)
    try:
        if git(ROOT, "rev-parse", "HEAD") != args.revision or git(ROOT, "status", "--porcelain"):
            raise ValueError("Candidate execution requires the exact clean source checkout")
        compiler = verify.checked(["rustc", "-vV"], attempt, "compiler", dict(os.environ))
        target = next(line.removeprefix("host: ") for line in compiler.splitlines() if line.startswith("host: "))
        if target not in NATIVE_TARGETS:
            raise ValueError("Unsupported native candidate execution host")
        record["target"] = target
        archives = check_payload(args.payload, args.version, args.revision, args.archive_evidence_sha256)
        auxiliary = inspect_auxiliary(args.auxiliary, ROOT, args.revision, args.version)
        if auxiliary["suite"]["target"] != target:
            raise ValueError("Candidate execution needs its native telemetry build")
        if compiler.strip() != auxiliary["compiler"]:
            raise ValueError("Candidate execution requires the admitted native compiler")
        record["compiler"] = compiler.strip()
        default = next(entry for entry in archives["targets"] if entry["suite"]["target"] == target)
        artifacts = dict(default=default["executable"]["sha256"], telemetry=auxiliary["executable"]["sha256"])
        record.update(artifacts=artifacts, auxiliary_build_sha256=auxiliary["build_sha256"])
        with tempfile.TemporaryDirectory(prefix="empack candidate ") as temporary:
            outside = Path(temporary).resolve()
            if outside.is_relative_to(ROOT):
                raise ValueError("Candidate executable must be extracted outside the checkout")
            captured = outside / "archive"
            capture_archive(args.payload / default["archive"]["name"], captured, default["archive"]["sha256"])
            binary, digest, _ = extract_binary(captured, outside, target)
            if digest != artifacts["default"]:
                raise ValueError("Extracted candidate differs from the admitted build")
            telemetry = outside / "telemetry" / binary.name
            telemetry.parent.mkdir()
            capture_binary(args.auxiliary.resolve().parent / binary.name, telemetry, artifacts["telemetry"])
            execute(attempt, dict(default=binary, telemetry=telemetry),
                    artifacts, target, args.version)
        if git(ROOT, "rev-parse", "HEAD") != args.revision or git(ROOT, "status", "--porcelain"):
            raise ValueError("Source changed during candidate execution")
        record["runs"], suites = inspect_runs(attempt, target, artifacts, args.revision, args.version, record["compiler"])
        record["status"] = aggregate_outcomes(suite["status"] for suite in suites)
        write_json(attempt / "execution.json", record)
        inspect_execution(attempt / "execution.json", archives, [auxiliary], args.archive_evidence_sha256,
                          args.revision, args.version)
    except (OSError, EOFError, ValueError, RuntimeError, KeyError, StopIteration, KeyboardInterrupt,
            tarfile.TarError, zipfile.BadZipFile) as error:
        record.update(status="FAIL", reason=str(error) or "Candidate execution interrupted")
    write_json(attempt / "execution.json", record)
    print(f"{record['status']}: {attempt / 'execution.json'}", flush=True)
    if record.get("reason"):
        print(record["reason"], file=sys.stderr)
    return {"PASS": 0, "BLOCKED": 2}.get(record["status"], 1)


if __name__ == "__main__":
    raise SystemExit(main())
