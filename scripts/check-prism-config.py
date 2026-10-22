#!/usr/bin/env python3
"""Exercise shared managed-config fixtures with an unmodified Prism INIFile parser."""
import argparse
import hashlib
import json
import platform
import re
import shutil
from pathlib import Path
import subprocess
import tempfile

PRISM_COMMIT = "7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4"  # Prism 11.1.1
ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/empack-lib/src/engine/build/client/fixtures.json"


def checked(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def require_supported_qt(version):
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match or int(match[1]) != 6 or int(match[2]) < 4:
        raise ValueError(f"Prism 11.1.1 requires Qt 6.4+ within major 6; got {version}. "
                         "Unsupported builds are supplemental only; choose a supported --qmake.")


def qpath(path):
    value = str(path)
    if any(ch in value for ch in '\n\r"$'):
        raise ValueError("Probe paths cannot contain qmake metacharacters")
    return '"' + value + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prism-source", type=Path, required=True)
    parser.add_argument("--qmake", default="qmake")
    args = parser.parse_args()
    qmake = shutil.which(args.qmake)
    if not qmake:
        raise ValueError(f"qmake not found: {args.qmake}")
    qt = checked(qmake, "-query", "QT_VERSION")
    require_supported_qt(qt)
    source = args.prism_source.resolve()
    commit = checked("git", "-C", str(source), "rev-parse", "HEAD")
    if commit != PRISM_COMMIT:
        raise ValueError(f"Expected Prism 11.1.1 source at {PRISM_COMMIT}; got {commit}")
    if checked("git", "-C", str(source), "status", "--porcelain"):
        raise ValueError("Prism source checkout must be clean")
    empack_commit = checked("git", "-C", str(ROOT), "rev-parse", "HEAD")
    empack_status = checked("git", "-C", str(ROOT), "status", "--porcelain")
    fixture_bytes = FIXTURES.read_bytes()
    with tempfile.TemporaryDirectory(prefix="empack-prism-parser-") as temporary:
        work = Path(temporary)
        project = f"""QT = core network
CONFIG += console c++20 release
CONFIG -= app_bundle debug debug_and_release
DEFINES += QT_NO_DEBUG QT_WARN_DEPRECATED_UP_TO=0x060400 QT_DISABLE_DEPRECATED_UP_TO=0x060400
TEMPLATE = app
TARGET = prism-config-probe
INCLUDEPATH += {qpath(source / 'launcher')}
SOURCES += {qpath(ROOT / 'scripts/consumer/prism-config-probe.cpp')} {qpath(source / 'launcher/settings/INIFile.cpp')} {qpath(source / 'launcher/Json.cpp')}
QMAKE_CXXFLAGS += -ffunction-sections -fdata-sections
macx:QMAKE_LFLAGS += -Wl,-dead_strip
unix:!macx:QMAKE_LFLAGS += -Wl,--gc-sections
"""
        (work / "probe.pro").write_text(project)
        for command in ([qmake, "probe.pro"], ["make", "-j2"]):
            result = subprocess.run(command, cwd=work, text=True, capture_output=True)
            if result.returncode:
                raise RuntimeError(result.stdout + result.stderr)
        environment = json.loads(checked(str(work / "prism-config-probe"), "--environment"))
        require_supported_qt(environment["qt_runtime"])
        if environment["cplusplus"] < 202002:
            raise ValueError("Pinned Prism source requires C++20")
        if environment["qt_compiled"] != qt or environment["qt_runtime"] != qt:
            raise ValueError(f"qmake/compiled/runtime Qt mismatch: {qt}, {environment}")
        build = dict(re.findall(r"^(CXX|CXXFLAGS|DEFINES|LFLAGS)\s*=\s*(.*)$",
                                (work / "Makefile").read_text(), re.MULTILINE))
        prepare = ('empack --workdir "$INST_DIR" --yes instance prepare '
                   '"$INST_DIR/.minecraft/.empack-consumer/release.json" --sha256 '
                   + '0' * 64 + ' --layout prism --side client')
        wrapper = 'empack --workdir "$INST_DIR" --yes instance launch --'
        observations = []
        for index, case in enumerate(json.loads(fixture_bytes)):
            config = case["configuration"].replace("@PREPARE@", json.dumps(prepare)).replace("@WRAPPER@", json.dumps(wrapper))
            path = work / f"{index}.ini"
            path.write_bytes(config.encode("utf-8"))
            actual = json.loads(checked(str(work / "prism-config-probe"), str(path)))
            integrated = (actual["loaded"] and actual["override"] and
                          actual["prepare"] == prepare and actual["wrapper"] == wrapper)
            if case["accepted"] and not integrated:
                raise AssertionError((case["name"], actual))
            if case["name"] == "legacy-shadow" and integrated:
                raise AssertionError("The legacy shadow counterexample was not reproduced")
            if "consumer_override" in case and actual["override"] != case["consumer_override"]:
                raise AssertionError((case["name"], "unexpected consumer override", actual))
            observations.append({"fixture": case["name"], "supported": case["accepted"], "consumer": actual})
        if (checked("git", "-C", str(ROOT), "rev-parse", "HEAD") != empack_commit or
                checked("git", "-C", str(ROOT), "status", "--porcelain") != empack_status or
                FIXTURES.read_bytes() != fixture_bytes):
            raise ValueError("Empack source changed during probe; rerun against a stable checkout")
        print(json.dumps({"prism_commit": commit, "qt_version": qt,
                          "evidence_scope": "supported-dependency headless parser probe",
                          "empack_commit": empack_commit, "empack_dirty": bool(empack_status),
                          "fixture_sha256": hashlib.sha256(fixture_bytes).hexdigest(),
                          "platform": platform.platform(), "host_architecture": platform.machine(),
                          "environment": environment, "qmake": qmake,
                          "build_configuration": {"mode": "release", "cxx_standard": 20, **build},
                          "fixtures": observations, "gameplay_tested": False}, indent=2))


if __name__ == "__main__":
    main()
