#!/usr/bin/env python3
"""Observe synthetic JVM argument/path encoding; this is not consumer qualification."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from verification.compiler import compiler_environment

VALUES = ["ASCII", "caf\u00e9", "\u65e5\u672c\u8a9e", "\U0001f600", "", "value with spaces"]
STDIN = b"stdin stays separate\n\x00\xff"


def decode_native(output):
    position = 0

    def word():
        nonlocal position
        value = struct.unpack_from(">I", output, position)[0]
        position += 4
        return value

    count = word()
    if count != len(VALUES):
        raise ValueError("Unexpected fixture argument count")
    values = []
    for _ in range(count):
        length = word()
        value = output[position:position + length]
        if len(value) != length:
            raise ValueError("Truncated fixture value")
        values.append(value.decode("utf-8", errors="strict"))
        position += length
    return {"native_arguments": values, "input_preserved": output[position:] == STDIN,
            "original_arguments_preserved": values == VALUES}


def observe(command, **kwargs):
    result = subprocess.run(command, input=STDIN, capture_output=True, timeout=15, **kwargs)
    record = {"status": result.returncode}
    try:
        record.update(decode_native(result.stdout))
    except (ValueError, struct.error, UnicodeError):
        record["native_result"] = "No complete UTF-8 fixture record"
    for line in result.stderr.splitlines():
        if line.startswith(b"ENCODING_PROBE="):
            record["jvm"] = json.loads(line.removeprefix(b"ENCODING_PROBE="))
    if "jvm" not in record and result.stderr:
        record["diagnostic"] = result.stderr[:2048].decode("utf-8", errors="replace")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", default=str(Path(os.environ["JAVA_HOME"]) / "bin/java")
                        if "JAVA_HOME" in os.environ else "java")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    jar = ROOT / "bootstrap/target/empack-bootstrap.jar"
    classes = ROOT / "bootstrap/target/test-classes"
    if not jar.is_file() or not (classes / "design/inherent/empack/bootstrap/EncodingProbe.class").is_file():
        parser.error("Build the bootstrap and nonshipping test classes first")
    classpath = os.pathsep.join([str(jar), str(classes)])
    java = [args.java, "-cp", classpath, "design.inherent.empack.bootstrap.EncodingProbe"]
    record = {"scope": "diagnostic only; synthetic arguments; no Prism or game launch",
              "platform": platform.system(), "architecture": platform.machine(),
              "jar_sha256": hashlib.sha256(jar.read_bytes()).hexdigest(), "expected_arguments": VALUES}
    if os.name == "nt":
        record["windows_code_pages"] = {
            name: getattr(ctypes.windll.kernel32, name)()
            for name in ["GetACP", "GetOEMCP", "GetConsoleCP", "GetConsoleOutputCP"]}
    with tempfile.TemporaryDirectory(prefix="empack-encoding-") as folder:
        temporary = Path(folder)
        engine = temporary / ("engine.exe" if os.name == "nt" else "engine")
        subprocess.run(["rustc", "--edition=2024", str(ROOT / "bootstrap/tests/fixtures/engine.rs"),
                        "-o", str(engine)], check=True, env=compiler_environment(os.environ))
        invocation = temporary / "arguments.json"
        invocation.write_text(json.dumps(VALUES, ensure_ascii=False), encoding="utf-8")
        cases = {"native_direct": observe([str(engine), "--fixture-argv", *VALUES]),
                 "jvm_literal": observe([*java, "literal", str(engine), *VALUES]),
                 "jvm_utf8_file": observe([*java, "file", str(engine), str(invocation)]),
                 "jvm_literal_file_encoding_utf8": observe([
                     args.java, "-Dfile.encoding=UTF-8", *java[1:], "literal", str(engine), *VALUES])}
        if os.name != "nt":
            c_locale = dict(os.environ, LANG="C", LC_ALL="C")
            for route in ["literal", "file"]:
                values = VALUES if route == "literal" else [str(invocation)]
                cases["jvm_" + route + "_C"] = observe([*java, route, str(engine), *values], env=c_locale)
        else:
            utf8_hint = dict(os.environ, LANG="en_US.UTF-8", LC_ALL="en_US.UTF-8")
            cases["jvm_literal_utf8_locale_variables"] = observe(
                [*java, "literal", str(engine), *VALUES], env=utf8_hint)
        unicode_directory = temporary / "caf\u00e9 \u65e5\u672c\u8a9e"
        unicode_directory.mkdir()
        unicode_engine = unicode_directory / engine.name
        shutil.copy2(engine, unicode_engine)
        cases["unicode_engine_path_literal"] = observe([*java, "literal", str(unicode_engine), *VALUES])
        shutil.copyfile(jar, unicode_directory / "bootstrap.jar")
        for label, path in [("absolute", str(unicode_directory / "bootstrap.jar")),
                            ("relative", "bootstrap.jar")]:
            result = subprocess.run([args.java, "-jar", path, "--version"], cwd=unicode_directory,
                                    capture_output=True, timeout=15)
            cases["unicode_jar_path_" + label] = {
                "status": result.returncode,
                "stdout": result.stdout[:2048].decode("utf-8", errors="replace"),
                "stderr": result.stderr[:2048].decode("utf-8", errors="replace")}
        record["cases"] = cases
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(record, ensure_ascii=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record, ensure_ascii=True, indent=2))


if __name__ == "__main__":
    main()
