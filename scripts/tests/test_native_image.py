"""Inert header/evidence checks; one N case invokes only the native inspector."""
import copy
import hashlib
import json
from pathlib import Path
import platform
import os
import shutil
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from verification import native_image
from verification.evidence import sha256, write_json
import native_image_fixture as fixture


class NativeArchitecture(unittest.TestCase):
    def test_five_literal_abi_headers_report_expected_architecture_without_execution(self):
        for target in native_image.IMAGES:
            with self.subTest(target=target):
                self.assertEqual(native_image.architecture(fixture.header(target), target), fixture.facts(target))

    def test_wrong_architecture_formats_truncation_and_header_bounds_are_refused(self):
        for target in native_image.IMAGES:
            header = fixture.header(target)
            for length in (0, 4, 16, 31):
                with self.subTest(target=target, length=length), self.assertRaises(ValueError):
                    native_image.architecture(header[:length], target)
            other = next(t for t in native_image.IMAGES if t != target
                         and (t.split("-", 1)[0] != target.split("-", 1)[0]
                              or fixture.facts(t)["format"] != fixture.facts(target)["format"]))
            with self.subTest(target=target), self.assertRaises(ValueError):
                native_image.architecture(header, other)
        malformed = []
        for offset, value in ((4, 1), (5, 2), (16, 1), (20, 2), (52, 63)):
            raw = bytearray(fixture.header("x86_64-unknown-linux-gnu"));raw[offset] = value
            malformed.append((raw, "x86_64-unknown-linux-gnu"))
        raw = bytearray(fixture.header("aarch64-apple-darwin"));raw[12] = 6
        malformed.append((raw, "aarch64-apple-darwin"))
        for offset in (0, 63, native_image.HEADER_LIMIT, 0xFFFFFFFF):
            raw = bytearray(fixture.header("x86_64-pc-windows-msvc"));struct.pack_into("<I", raw, 0x3C, offset)
            malformed.append((raw, "x86_64-pc-windows-msvc"))
        for offset, value in ((148, 1), (150, 0x2002), (152, 0x10B)):
            raw = bytearray(fixture.header("x86_64-pc-windows-msvc"));struct.pack_into("<H", raw, offset, value)
            malformed.append((raw, "x86_64-pc-windows-msvc"))
        for raw, target in malformed:
            with self.subTest(target=target, header=bytes(raw[:24]).hex()), self.assertRaises(ValueError):
                native_image.architecture(raw, target)
        with self.assertRaises(ValueError):
            native_image.architecture(b"unknown", "unknown-target")


class NativeImageEvidence(unittest.TestCase):
    def test_native_metadata_is_rechecked_against_checked_image_and_report_bytes(self):
        for target in native_image.IMAGES:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary) / "image"
                payload = fixture.header(target)
                binary = dict(sha256=hashlib.sha256(payload).hexdigest(), length=len(payload))
                record = fixture.observation(directory, target, **dict(digest=binary["sha256"], length=len(payload)))
                path = directory / "image.json"
                expected = fixture.facts(target)
                result = native_image.inspect(path, target, binary, expected)
                self.assertEqual(result["image"], expected)
                for field in ("target", "image", "commands", "tool", "host", "executable"):
                    changed = copy.deepcopy(record)
                    if field == "target": changed[field] = "unrelated"
                    elif field == "image": changed[field]["machine"] = 0
                    elif field == "commands": changed[field]["dependencies"][-1] = "another-file"
                    elif field == "tool": changed[field]["version"] = "unrelated"
                    elif field == "host": changed[field]["system"] = "unrelated"
                    else: changed[field]["sha256"] = "0" * 64
                    write_json(path, changed)
                    with self.subTest(field=field), self.assertRaises(ValueError):
                        native_image.inspect(path, target, binary, expected)
                write_json(path, record)
                report = directory / "dependencies.log"
                report.write_bytes(b"changed observation\n")
                with self.assertRaisesRegex(ValueError, "report changed"):
                    native_image.inspect(path, target, binary, expected)
                report.unlink()
                with self.assertRaises(ValueError):
                    native_image.inspect(path, target, binary, expected)

    def test_capture_uses_only_native_inspector_and_never_invokes_captured_product(self):
        target = self.target()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary, tool = self.files(root, target)
            content = fixture.reports(target)
            def run(arguments, **kwargs):
                self.assertEqual(arguments[0], str(tool))
                self.assertNotEqual(arguments[0], str(binary))
                kwargs["log"].write_bytes(content[kwargs["log"].name].encode("utf-8"))
                return 0, "", False
            with patch.object(native_image.shutil, "which", return_value=str(tool)), patch.object(native_image, "run", side_effect=run):
                digest = native_image.capture(binary, target, sha256(binary), root / "image", {})
            self.assertEqual(digest, sha256(root / "image/image.json"))
            self.assertEqual(binary.read_bytes(), fixture.header(target))
            self.assertEqual(tool.read_bytes(), b"inert inspector; never executed")

    def test_unavailable_failed_empty_oversized_or_changed_inputs_do_not_finish_capture(self):
        target = self.target()
        for fault in ("unavailable", "failed", "timeout", "empty", "oversized", "binary", "tool"):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                binary, tool = self.files(root, target)
                content = fixture.reports(target)
                def run(arguments, **kwargs):
                    text = "" if fault == "empty" else "x" * 100 if fault == "oversized" else content[kwargs["log"].name]
                    kwargs["log"].write_bytes(text.encode("utf-8"))
                    if fault in ("binary", "tool"):
                        selected = binary if fault == "binary" else tool
                        selected.write_bytes(selected.read_bytes() + b"changed")
                    return (1 if fault == "failed" else 0), "", fault == "timeout"
                with patch.object(native_image.shutil, "which", return_value=None if fault == "unavailable" else str(tool)), \
                        patch.object(native_image, "run", side_effect=run), \
                        patch.object(native_image, "REPORT_LIMIT", 64 if fault == "oversized" else 4 << 20):
                    with self.assertRaises((ValueError, RuntimeError)):
                        native_image.capture(binary, target, sha256(binary), root / "image", {})
                self.assertFalse((root / "image/image.json").exists())

    @staticmethod
    def target():
        system = platform.system()
        arm = "arm" in platform.machine().lower() or "aarch64" in platform.machine().lower()
        return ("x86_64-pc-windows-msvc" if system == "Windows" else
                ("aarch64" if arm else "x86_64") + ("-apple-darwin" if system == "Darwin" else "-unknown-linux-gnu"))

    @staticmethod
    def files(root, target):
        format = fixture.facts(target)["format"]
        binary = root / ("empack.exe" if format == "pe" else "empack")
        tool = root / {"elf": "readelf", "mach-o": "otool", "pe": "dumpbin.exe"}[format]
        binary.write_bytes(fixture.header(target))
        tool.write_bytes(b"inert inspector; never executed")
        return binary, tool


class NativeImageInspector(unittest.TestCase):
    def test_installed_native_inspector_reads_a_copy_without_executing_it(self):
        # Use an installed native compiler image, not Python's independently
        # packaged launcher. Rustup is already a prerequisite of this test host.
        # Never invoke the copy or compile a fixture; this is not P evidence.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            code, output, expired = native_image.run(
                ["rustup", "which", "rustc"], cwd=Path(__file__).resolve().parents[2],
                env=dict(os.environ), log=root / "rustup-which.log", timeout=30, separate_stdout=True)
            self.assertFalse(expired)
            self.assertEqual(code, 0, (root / "rustup-which.log").read_text()[-4096:])
            source = Path(output.strip()).resolve(strict=True)
            self.assertTrue(source.is_absolute() and source.is_file())
            system = platform.system()
            targets = [target for target, profile in native_image.IMAGES.items() if profile[3] == system]
            matches = []
            for target in targets:
                try:
                    matches.append((target, native_image.inspect_architecture(source, target)))
                except ValueError:
                    pass
            with source.open("rb") as stream:
                prefix = stream.read(32).hex()
            self.assertEqual(len(matches), 1,
                f"Installed compiler must have one thin native image profile: {source}; header={prefix}")
            target, expected = matches[0]
            binary = root / ("empack.exe" if system == "Windows" else "empack")
            shutil.copyfile(source, binary)
            digest = sha256(binary)
            result = native_image.capture(binary, target, digest, root / "native-image", dict(os.environ))
            self.assertEqual(result, sha256(root / "native-image/image.json"))
            record = native_image.inspect(root / "native-image/image.json", target,
                dict(sha256=digest, length=binary.stat().st_size), expected)
            self.assertTrue(record["reports"])
            self.assertEqual(record["image"], expected)
            self.assertEqual(sha256(binary), digest)


if __name__ == "__main__":
    unittest.main()
