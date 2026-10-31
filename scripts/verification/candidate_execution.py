"""Fixed native/live execution against admitted default and telemetry binaries."""
from datetime import datetime
import json
from pathlib import Path

from .evidence import aggregate_outcomes, sha256
from .receipts import inspect_suite_receipt
from .authoring_schema import ROOT, inspect_schema_evidence

NATIVE_TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc")
GRAPHS = {"default": "minimal", "telemetry": "all"}
PHASES = {
    "native": ["candidate-native", "native-remount"],
    "live": ["pinned-import", "live-provider-cli", "live-modrinth", "live-curseforge",
             "live-catalog", "publisher-cli", "server-cli", "live-runtime", "live-assistance"],
}


def inspect_runs(directory, target, artifacts, revision, version, compiler):
    """Re-read all four attempts, including failures; paths cannot select a winner."""
    directory = Path(directory)
    summaries, suites = [], []
    for features, graph in GRAPHS.items():
        for phase, selection in PHASES.items():
            paths = list((directory / features / phase).glob("*/receipt.json"))
            if (len(paths) != 1 or paths[0].is_symlink()
                    or not paths[0].resolve().is_relative_to(directory.resolve())):
                raise ValueError("Candidate execution needs one contained attempt per host/feature/phase")
            path = paths[0]
            raw = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(raw, dict):
                raise ValueError("Expected a candidate suite receipt object")
            if raw.get("rustc") != compiler:
                raise ValueError("Candidate suite compiler differs from the admitted native toolchain")
            suite = inspect_suite_receipt(path, selection, target, revision, features, version, "release", graph)
            if suite["artifact_sha256"] != artifacts[features]:
                raise ValueError("Candidate execution used another default or telemetry executable")
            summaries.append(dict(features=features, phase=phase, test_features=graph,
                                  receipt_sha256=suite["receipt_sha256"]))
            suites.append(suite)
    return summaries, suites


def inspect_schema_runs(directory, target, artifacts, revision, version, root=ROOT):
    """Only Linux owns the editor conformance case; bind it to extracted shipped bytes."""
    directory = Path(directory)
    paths = list((directory / "authoring-schema").glob("*/receipt.json"))
    if target != NATIVE_TARGETS[0]:
        if paths:
            raise ValueError("Unexpected schema attempt on a host without that candidate obligation")
        return []
    if (len(paths) != 1 or paths[0].is_symlink()
            or not paths[0].resolve().is_relative_to(directory.resolve())):
        raise ValueError("Candidate needs one contained schema conformance attempt")
    expected = dict(target=target, features="default", profile="release", version=version,
                    sha256=artifacts["default"])
    return [inspect_schema_evidence(paths[0], revision, root, expected_artifact=expected)]


def inspect_execution(path, archives, auxiliary, archive_digest, revision, version, root=ROOT):
    """Bind supplemental native/live results to already admitted build identities."""
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError("Missing or redirected candidate execution record")
    record = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(record, dict):
        raise ValueError("Expected a candidate execution record object")
    target = record.get("target")
    if (record.get("schema") != 1 or record.get("kind") != "candidate-native-execution"
            or record.get("source_revision") != revision or record.get("version") != version
            or record.get("archive_evidence_sha256") != archive_digest or target not in NATIVE_TARGETS
            or record.get("qualification") is not False):
        raise ValueError("Candidate execution differs from the admitted source, payload or native host")
    if record.get("reason"):
        raise ValueError("Candidate execution did not establish stable inputs: " + str(record["reason"]))
    if datetime.fromisoformat(record["created_at"]).utcoffset() is None:
        raise ValueError("Candidate execution needs a timezone-aware date")
    builds = [entry for entry in auxiliary if entry["suite"]["target"] == target]
    if len(builds) != 1:
        raise ValueError("Candidate execution needs one admitted telemetry build for its host")
    default = next(entry for entry in archives["targets"] if entry["suite"]["target"] == target)
    artifacts = dict(default=default["executable"]["sha256"], telemetry=builds[0]["executable"]["sha256"])
    if record.get("artifacts") != artifacts or record.get("auxiliary_build_sha256") != builds[0]["build_sha256"]:
        raise ValueError("Candidate execution is not bound to the admitted default and telemetry builds")
    if record.get("compiler") != builds[0]["compiler"]:
        raise ValueError("Candidate compiler differs from the admitted native toolchain")
    summaries, suites = inspect_runs(path.parent, target, artifacts, revision, version, record["compiler"])
    schemas = inspect_schema_runs(path.parent, target, artifacts, revision, version, root)
    status = aggregate_outcomes(suite["status"] for suite in [*suites, *schemas])
    if (record.get("runs") != summaries or record.get("status") != status
            or record.get("schema_evidence") != schemas):
        raise ValueError("Candidate execution summary differs from its retained attempts")
    return dict(target=target, status=status, execution_sha256=sha256(path), artifacts=artifacts,
                source_revision=revision, archive_evidence_sha256=archive_digest, suites=suites,
                schema_evidence=schemas)
