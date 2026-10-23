#!/usr/bin/env python3
"""Require complete candidate observations before publishing tested release archives."""
import argparse
from datetime import datetime, timezone
from pathlib import Path

from verification.evidence import write_json
from verification.qualification import check_qualification, qualify
from verification.reporting import cell


def render(record):
    lines = ["# Release candidate", "", f"Qualification: **{record['status']}**.", "",
             f"Source: {cell(record['source_revision'])}; version: {cell(record['version'])}.", "",
             f"Archive evidence: {cell(record['archive_evidence_sha256'])}", ""]
    if record.get("reason"):
        return "\n".join(lines + [cell(record["reason"]), "", "No qualification was established.", ""])
    lines += [record["waiver_policy"], "", "| Requirement | Applicable | Observations |",
              "| --- | --- | --- |"]
    for row in record["acceptance"]["requirements"]:
        lines.append(f"| {cell(row['id'])} | {cell(row['applicable'])} | {cell(row['status'])} |")
    lines += ["", "## Outstanding obligations", ""]
    for blocker in record["blockers"]:
        lines.append(f"- {cell(blocker['requirement'])}: {cell('; '.join(blocker['reasons']))}")
    if not record["blockers"]:
        lines.append("All applicable obligations passed at their required evidence boundaries.")
    lines += ["", "Detailed case, target, feature and receipt identities remain in the companion JSON.",
              "An archive test or parser probe does not establish consumer import, startup or gameplay.", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    gate = commands.add_parser("gate")
    gate.add_argument("--payload", type=Path, required=True)
    gate.add_argument("--receipt", type=Path, action="append", default=[])
    gate.add_argument("--consumer", type=Path, action="append", default=[])
    gate.add_argument("--tooling", type=Path, action="append", default=[])
    gate.add_argument("--auxiliary", type=Path, action="append", default=[],
                      help="Telemetry build.json with its retained executable and suite evidence")
    gate.add_argument("--output", type=Path, required=True)
    publish = commands.add_parser("verify")
    publish.add_argument("--record", type=Path, required=True)
    publish.add_argument("--sha256", required=True)
    for command in (gate, publish):
        command.add_argument("--archive-evidence-sha256", required=True)
        command.add_argument("--revision", required=True)
        command.add_argument("--version", required=True)
    args = parser.parse_args()
    if args.command == "verify":
        check_qualification(args.record, args.sha256, args.revision, args.version, args.archive_evidence_sha256)
        print("Candidate qualification matches the publication payload")
        return 0
    try:
        record = qualify(Path.cwd(), args.payload, args.archive_evidence_sha256,
                         args.revision, args.version, args.receipt, args.consumer, args.tooling, args.auxiliary)
    except (ValueError, OSError, KeyError, TypeError) as error:
        record = dict(schema=1, kind="release-candidate-qualification", status="FAIL", qualification=False,
                      source_revision=args.revision, version=args.version,
                      archive_evidence_sha256=args.archive_evidence_sha256,
                      created_at=datetime.now(timezone.utc).isoformat(), reason=str(error))
    write_json(args.output, record)
    args.output.with_suffix(".md").write_text(render(record), encoding="utf-8", newline="\n")
    print(f"Candidate qualification: {record['status']}; {args.output}")
    return 0 if record["qualification"] else 2 if record["status"] == "BLOCKED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
