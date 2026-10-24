"""Bind a non-shipped telemetry build to candidate inputs and observed suites."""
from datetime import datetime
import json
from pathlib import Path
import tomllib

from .evidence import sha256
from .receipts import inspect_suite_receipt
from .release import TARGETS, check_inputs

SUITES = ["cli", "telemetry"]


def build_command(target):
    return ["cargo", "build", "--locked", "--release", "-p", "empack", "--target", target,
            "--features", "telemetry"]


def inspect_auxiliary(path, root, revision, version):
    """Recheck build inputs, retained executable and exact suite evidence together.

    Like archive build records, this is trusted workflow-producer evidence, not a
    cryptographic attestation from an arbitrary caller. No auxiliary bytes enter
    the publication payload or establish package acceptance.
    """
    path, root = Path(path), Path(root)
    record = json.loads(path.read_text(encoding="utf-8"))
    target = record.get("target")
    if (record.get("schema") != 1 or record.get("kind") != "candidate-auxiliary-build"
            or record.get("source_revision") != revision or record.get("version") != version
            or target not in TARGETS or record.get("features") != "telemetry"
            or record.get("profile") != "release" or record.get("shipped") is not False
            or record.get("command") != build_command(target)
            or type(record.get("build_exit_code")) is not int or record["build_exit_code"] != 0
            or record.get("build_timed_out") is not False):
        raise ValueError("Unrelated, failed or unsupported auxiliary build")
    date = record.get("created_at")
    if not isinstance(date, str) or datetime.fromisoformat(date).utcoffset() is None:
        raise ValueError("Auxiliary build needs a timezone-aware date")
    check_inputs(record["inputs"], root, version, revision)
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    compiler = record["compiler"].splitlines()
    if f"release: {channel}" not in compiler or f"host: {target}" not in compiler:
        raise ValueError("Auxiliary build must use the pinned compiler on its native host")
    expected_environment = {"GIT_HASH": revision, "BUILD_DATE": record["inputs"]["build_date"],
                            "CARGO_INCREMENTAL": "0"}
    if record.get("build_environment") != expected_environment:
        raise ValueError("Auxiliary build metadata differs from the candidate")
    executable = path.parent / TARGETS[target][1]
    if (executable.is_symlink() or not executable.is_file()
            or record["executable"] != {"sha256": sha256(executable), "length": executable.stat().st_size}):
        raise ValueError("Auxiliary executable differs from its build record")
    # Fixed layout; a producer cannot point admission outside its evidence bundle.
    receipts = list((path.parent / "suite").glob("*/receipt.json"))
    if len(receipts) != 1 or receipts[0].is_symlink() or not receipts[0].resolve().is_relative_to(path.parent.resolve()):
        raise ValueError("Expected one contained auxiliary suite receipt")
    suite_path = receipts[0]
    if sha256(suite_path) != record.get("suite_receipt_sha256"):
        raise ValueError("Auxiliary suite receipt changed after execution")
    suite = inspect_suite_receipt(suite_path, SUITES, target, revision, "telemetry", version, "release", "all")
    if suite["artifact_sha256"] != record["executable"]["sha256"] or record.get("status") != suite["status"]:
        raise ValueError("Auxiliary execution does not describe the built executable or outcome")
    return {"build_sha256": sha256(path), "inputs": record["inputs"], "compiler": record["compiler"],
            "command": record["command"], "executable": record["executable"],
            "shipped": False, "suite": suite}
