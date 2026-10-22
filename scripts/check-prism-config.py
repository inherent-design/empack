#!/usr/bin/env python3
"""Exercise shared managed-config fixtures with an unmodified Prism INIFile parser."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

PRISM_COMMIT = "7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4"  # Prism 11.1.1
ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/empack-lib/src/engine/build/client/fixtures.json"


def checked(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


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
    source = args.prism_source.resolve()
    commit = checked("git", "-C", str(source), "rev-parse", "HEAD")
    if commit != PRISM_COMMIT:
        raise ValueError(f"Expected Prism 11.1.1 source at {PRISM_COMMIT}; got {commit}")
    subprocess.run(["git", "-C", str(source), "diff", "--exit-code", "HEAD", "--", "launcher"], check=True)
    qt = checked(args.qmake, "-query", "QT_VERSION")
    with tempfile.TemporaryDirectory(prefix="empack-prism-parser-") as temporary:
        work = Path(temporary)
        project = f"""QT = core network
CONFIG += console c++17
CONFIG -= app_bundle
TEMPLATE = app
TARGET = prism-config-probe
INCLUDEPATH += {qpath(source / 'launcher')}
SOURCES += {qpath(ROOT / 'scripts/consumer/prism-config-probe.cpp')} {qpath(source / 'launcher/settings/INIFile.cpp')} {qpath(source / 'launcher/Json.cpp')}
QMAKE_CXXFLAGS += -ffunction-sections -fdata-sections
macx:QMAKE_LFLAGS += -Wl,-dead_strip
unix:!macx:QMAKE_LFLAGS += -Wl,--gc-sections
"""
        (work / "probe.pro").write_text(project)
        for command in ([args.qmake, "probe.pro"], ["make", "-j2"]):
            result = subprocess.run(command, cwd=work, text=True, capture_output=True)
            if result.returncode:
                raise RuntimeError(result.stdout + result.stderr)
        prepare = ('empack --workdir "$INST_DIR" --yes instance prepare '
                   '"$INST_DIR/.minecraft/.empack-consumer/release.json" --sha256 '
                   + '0' * 64 + ' --layout prism --side client')
        wrapper = 'empack --workdir "$INST_DIR" --yes instance launch --'
        observations = []
        for index, case in enumerate(json.loads(FIXTURES.read_text())):
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
        print(json.dumps({"prism_commit": commit, "qt_version": qt,
                          "fixtures": observations, "gameplay_tested": False}, indent=2))


if __name__ == "__main__":
    main()
