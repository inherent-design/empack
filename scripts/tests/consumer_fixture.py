"""Synthetic parser records for admission tests; never consumer execution evidence."""
import json

from verification.consumer import (FIXTURE_PATH, PREPARE, PRISM_COMMIT, ROOT,
                                   WRAPPER, input_hashes)
from verification.evidence import sha256


def prism_record(revision, root=ROOT, system="Linux"):
    fixtures = json.loads((root / FIXTURE_PATH).read_text(encoding="utf-8"))
    return dict(schema=2, kind="prism-parser-observations", empack_commit=revision,
        empack_dirty=False, prism_commit=PRISM_COMMIT, status="PASS", inputs=input_hashes(root),
        created_at="2026-10-23T00:00:00+00:00", probe_sha256="f" * 64,
        evidence_scope="supported-dependency headless parser probe", fixture_sha256=sha256(root / FIXTURE_PATH),
        gameplay_tested=False, host_system=system, platform="synthetic host", host_architecture="x86_64",
        qt_version="6.4.2", environment=dict(qt_compiled="6.4.2", qt_runtime="6.4.2", cplusplus=202002,
            compiler="synthetic compiler", target_architecture="x86_64", runtime_architecture="x86_64"),
        build_configuration=dict(mode="release", cxx_standard=20, CXX="synthetic c++"),
        fixtures=[dict(fixture=case["name"], supported=case["accepted"], consumer=dict(
            loaded=case["accepted"], override=case.get("consumer_override", case["accepted"]),
            prepare=PREPARE if case["accepted"] else "", wrapper=WRAPPER if case["accepted"] else "", version="1.2"))
            for case in fixtures])
