"""Read attributable consumer/manual observations; never perform a task by admission."""
from datetime import datetime
import hashlib
import json
from pathlib import Path, PurePosixPath
import re

from .evidence import aggregate_outcomes, sha256

ROOT = Path(__file__).resolve().parents[2]
PROCEDURES = "tests/verification/acceptance-procedures.json"
MAX_RECORD = 1024 * 1024
MAX_FILE = 16 * 1024 * 1024


def procedures(root=ROOT):
    document = json.loads((Path(root) / PROCEDURES).read_text(encoding="utf-8"))
    if document.get("schema") != 1:
        raise ValueError("Unsupported acceptance procedure schema")
    result = {}
    for row in document["procedures"]:
        if (not row["id"].startswith("acceptance::") or row["id"] in result
                or row["boundary"] not in {"C", "M"}
                or not row["steps"] or len(set(row["steps"])) != len(row["steps"])
                or not row["platforms"] or not set(row["platforms"]) <= {"linux", "macos", "windows"}):
            raise ValueError("Invalid or duplicate acceptance procedure")
        result[row["id"]] = row
    if not result:
        raise ValueError("Acceptance procedures must be nonempty")
    return result


def _bounded(path, maximum):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError("Acceptance evidence must be a regular file")
    with path.open("rb") as stream:
        data = stream.read(maximum + 1)
    if not 0 < len(data) <= maximum:
        raise ValueError("Acceptance evidence is empty or exceeds its admitted size")
    return data


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate acceptance JSON key")
        result[key] = value
    return result


def _text(value):
    return isinstance(value, str) and bool(value.strip())


def inspect_acceptance_evidence(path, revision, version, archive_digest, products, root=ROOT):
    """Bind a human-supplied record to already admitted candidate product identities.

    The caller supplies (target, features) -> executable SHA-256 from archive or
    auxiliary-build admission. A record cannot grant itself an executable identity.
    Attribution and observations remain operator reports, not automated proof.
    """
    try:
        return _inspect(Path(path), revision, version, archive_digest, products, Path(root))
    except (KeyError, TypeError, AttributeError, UnicodeError) as error:
        raise ValueError("Malformed operator acceptance evidence") from error


def _inspect(path, revision, version, archive_digest, products, root):
    data = _bounded(path, MAX_RECORD)
    record = json.loads(data, object_pairs_hook=_object)
    definition = procedures(root).get(record["procedure"])
    if (type(record.get("schema")) is not int or record["schema"] != 1 or record.get("kind") != "operator-acceptance"
            or record["source_revision"] != revision or record["version"] != version
            or record["archive_evidence_sha256"] != archive_digest or definition is None
            or record["procedures_sha256"] != sha256(root / PROCEDURES)
            or record["features"] not in {"default", "telemetry"}
            or (record["target"], record["features"]) not in products
            or products.get((record["target"], record["features"])) != record["artifact_sha256"]):
        raise ValueError("Acceptance evidence differs from the admitted candidate or procedures")
    platforms = {"linux": "linux", "darwin": "macos", "windows": "windows"}
    target_platform = next((platform for token, platform in platforms.items() if token in record["target"]), None)
    if target_platform not in definition["platforms"] or record["platform"] != target_platform:
        raise ValueError("Acceptance platform differs from the tested executable")
    started = datetime.fromisoformat(record["started_at"])
    completed = datetime.fromisoformat(record["completed_at"])
    if not started.tzinfo or not completed.tzinfo or completed < started:
        raise ValueError("Acceptance observations need ordered, timezone-aware dates")
    operator, environment = record["operator"], record["environment"]
    if (not _text(operator["id"]) or operator["role"] not in definition["operator_roles"]
            or any(not _text(environment[key]) for key in ("os_version", "architecture", "tool", "tool_version", "runtime"))):
        raise ValueError("Acceptance evidence lacks operator or execution environment")
    files = {}
    if len(record["files"]) > 8:
        raise ValueError("Too many acceptance evidence files")
    for entry in record["files"]:
        name = entry["path"]
        relative = PurePosixPath(name)
        if (not name or relative.is_absolute() or relative.as_posix() != name
                or any(part in {".", ".."} for part in relative.parts)
                or "\\" in name or ":" in name or name in files or name == path.name):
            raise ValueError("Unsafe or repeated acceptance evidence path")
        selected = path.parent
        for part in relative.parts:
            selected = selected / part
            if selected.is_symlink():
                raise ValueError("Acceptance evidence cannot follow symbolic links")
        if not selected.resolve().is_relative_to(path.parent.resolve()):
            raise ValueError("Acceptance evidence leaves its record directory")
        raw = _bounded(selected, MAX_FILE)
        digest = hashlib.sha256(raw).hexdigest()
        if type(entry["bytes"]) is not int or entry["bytes"] != len(raw) or entry["sha256"] != digest:
            raise ValueError("Acceptance supporting evidence changed")
        files[name] = {"sha256": digest, "bytes": len(raw)}
    observations = record["observations"]
    if [row["step"] for row in observations] != definition["steps"]:
        raise ValueError("Acceptance record omits, reorders or invents procedure steps")
    for row in observations:
        if (row["status"] not in {"PASS", "FAIL", "BLOCKED", "NOT_RUN"}
                or not _text(row["observed"]) or len(set(row["evidence"])) != len(row["evidence"])
                or not set(row["evidence"]) <= files.keys()
                or row["status"] in {"PASS", "FAIL"} and not row["evidence"]):
            raise ValueError("Acceptance step lacks an outcome, observation or supporting evidence")
    status = aggregate_outcomes(row["status"] for row in observations)
    if record["status"] != status:
        raise ValueError("Acceptance status differs from its observed steps")
    subject = record["subject"]
    if (not _text(subject["project_revision"])
            or status == "PASS" and any(not isinstance(subject[key], str)
                or not re.fullmatch(r"[a-f0-9]{64}", subject[key])
                for key in ("distribution_sha256", "expected_inventory_sha256"))):
        raise ValueError("Passing acceptance must identify the project, distribution and expected inventory")
    return dict(kind="operator-acceptance", source_revision=revision, version=version,
                archive_evidence_sha256=archive_digest, test_id=definition["id"],
                boundary=definition["boundary"], platform=target_platform, target=record["target"],
                features=record["features"], artifact_sha256=record["artifact_sha256"], status=status,
                created_at=record["completed_at"], operator=operator, environment=environment,
                observations=observations, subject=subject, files=files, procedures_sha256=record["procedures_sha256"],
                evidence_scope="operator-reported observations with retained supporting files",
                receipt_sha256=hashlib.sha256(data).hexdigest())
