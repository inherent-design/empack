"""Fixed supported Prism parser evidence; not launcher import or gameplay proof."""
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re

from .evidence import sha256

ROOT = Path(__file__).resolve().parents[2]
PRISM_COMMIT = "7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4"  # Prism 11.1.1
FIXTURE_PATH = "crates/empack-lib/src/engine/build/client/fixtures.json"
PRISM_TEST = "consumer::prism::managed-config-grammar"
PRISM_BINDING = {"id": PRISM_TEST, "boundary": "C", "platforms": ["linux", "macos"]}
INPUTS = (FIXTURE_PATH, "scripts/consumer/prism-config-probe.cpp",
          "scripts/check-prism-config.py", "scripts/verification/consumer.py")
PREPARE = ('empack --workdir "$INST_DIR" --yes instance prepare '
           '"$INST_DIR/.minecraft/.empack-consumer/release.json" --sha256 '
           + '0' * 64 + ' --layout prism --side client')
WRAPPER = 'empack --workdir "$INST_DIR" --yes instance launch --'


def require_supported_qt(version):
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match or int(match[1]) != 6 or int(match[2]) < 4:
        raise ValueError(f"Prism 11.1.1 requires Qt 6.4+ within major 6; got {version}. "
                         "Unsupported builds are supplemental only; choose a supported --qmake.")


def input_hashes(root=ROOT):
    return {name: sha256(Path(root) / name) for name in INPUTS}


def observation_failures(fixtures, observations):
    """Recheck actual settings, not an asserted PASS or matching fixture names."""
    expected = [(case["name"], case["accepted"]) for case in fixtures]
    actual = [(case["fixture"], case["supported"]) for case in observations]
    if (actual != expected or len({name for name, _ in expected}) != len(expected)
            or not expected or any(type(case["supported"]) is not bool for case in observations)):
        raise ValueError("Consumer fixture set differs from the required source fixtures")
    failed = []
    for case, observed in zip(fixtures, observations, strict=True):
        value = observed["consumer"]
        if (any(type(value[key]) is not bool for key in ("loaded", "override"))
                or any(not isinstance(value[key], str) for key in ("version", "prepare", "wrapper"))):
            raise ValueError("Malformed consumer settings observation")
        integrated = (value["loaded"] and value["override"]
                      and value["prepare"] == PREPARE and value["wrapper"] == WRAPPER)
        if (case["accepted"] and not integrated
                or case["name"] == "legacy-shadow" and integrated
                or "consumer_override" in case and value["override"] != case["consumer_override"]):
            failed.append(case["name"])
    return failed


def inspect_prism_record(record, revision, root=ROOT):
    try:
        return _inspect_prism_record(record, revision, root)
    except (KeyError, TypeError, AttributeError) as error:
        raise ValueError("Malformed consumer parser evidence") from error


def _inspect_prism_record(record, revision, root):
    if (record.get("schema") != 2 or record.get("kind") != "prism-parser-observations"
            or record["empack_commit"] != revision or record["empack_dirty"] is not False
            or record["prism_commit"] != PRISM_COMMIT
            or record["evidence_scope"] != "supported-dependency headless parser probe"
            or record["gameplay_tested"] is not False
            or record["inputs"] != input_hashes(root)
            or record["fixture_sha256"] != sha256(Path(root) / FIXTURE_PATH)):
        raise ValueError("Missing or unrelated supported Prism parser evidence")
    if (not re.fullmatch(r"[a-f0-9]{64}", record["probe_sha256"])
            or not datetime.fromisoformat(record["created_at"]).tzinfo):
        raise ValueError("Consumer evidence lacks probe identity or dated observations")
    qt, environment, build = record["qt_version"], record["environment"], record["build_configuration"]
    require_supported_qt(qt)
    if any(environment[key] != qt for key in ("qt_compiled", "qt_runtime")):
        raise ValueError("Consumer Qt runtime and build identity disagree")
    if (environment["cplusplus"] < 202002 or build["cxx_standard"] != 20 or build["mode"] != "release"
            or not environment["compiler"] or not build["CXX"]):
        raise ValueError("Consumer evidence lacks the supported compiler/build configuration")
    hosts = {"Linux": "linux", "Darwin": "macos"}
    if record["host_system"] not in hosts or not record["platform"] or not record["host_architecture"]:
        raise ValueError("Consumer evidence lacks a supported execution host")
    if not environment["target_architecture"] or not environment["runtime_architecture"]:
        raise ValueError("Consumer evidence lacks target/runtime architecture")
    failures = observation_failures(json.loads((Path(root) / FIXTURE_PATH).read_text(encoding="utf-8")), record["fixtures"])
    status = "FAIL" if failures else "PASS"
    if record["status"] != status:
        raise ValueError("Consumer status disagrees with observed settings")
    return dict(kind="prism-parser", source_revision=revision, platform=hosts[record["host_system"]],
                features="independent", status=status, test_id=PRISM_TEST, failed_fixtures=failures,
                created_at=record["created_at"], probe_sha256=record["probe_sha256"],
                prism_commit=PRISM_COMMIT, qt_version=qt, environment=environment,
                build_configuration=build, fixture_sha256=record["fixture_sha256"], inputs=record["inputs"])


def inspect_prism_evidence(path, revision, root=ROOT):
    path = Path(path)
    with path.open("rb") as stream:
        data = stream.read(1024 * 1024 + 1)
    if not 0 < len(data) <= 1024 * 1024:
        raise ValueError("Consumer record must be nonempty and at most 1 MiB")
    result = inspect_prism_record(json.loads(data), revision, root)
    result["receipt_sha256"] = hashlib.sha256(data).hexdigest()
    return result
