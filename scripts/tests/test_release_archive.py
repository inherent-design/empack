"""Archive admission tests use inert fixture bytes, not a release executable."""
import importlib.util
import io
from pathlib import Path
import stat
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
spec = importlib.util.spec_from_file_location(
    "release_archive", Path(__file__).resolve().parents[1] / "check-release-archive.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class ReleaseArchiveAdmission(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.destination = self.root / "extracted"
        self.destination.mkdir()

    def tar(self, entries):
        path = self.root / "fixture.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            for name, kind, mode, data in entries:
                entry = tarfile.TarInfo(name)
                entry.type, entry.mode = kind, mode
                if kind == tarfile.REGTYPE:
                    entry.size = len(data)
                if kind == tarfile.SYMTYPE:
                    entry.linkname = "../outside"
                archive.addfile(entry, io.BytesIO(data))
        return path

    def test_expected_tar_layout_preserves_executable_bytes_and_mode(self):
        path = self.tar([(".", tarfile.DIRTYPE, 0o755, b""),
                         ("./empack", tarfile.REGTYPE, 0o755, b"inert executable bytes")])
        binary, digest, length = package.extract_binary(path, self.destination, "aarch64-apple-darwin")
        self.assertEqual(binary.read_bytes(), b"inert executable bytes")
        self.assertEqual(digest, package.sha256(binary))
        self.assertEqual(length, len(binary.read_bytes()))
        if sys.platform != "win32":
            self.assertEqual(stat.S_IMODE(binary.stat().st_mode), 0o755)

    def test_extra_members_traversal_links_and_non_executable_tar_are_rejected(self):
        cases = [[], [("../outside", tarfile.REGTYPE, 0o755, b"x")],
                 [("empack", tarfile.SYMTYPE, 0o755, b"")],
                 [("empack", tarfile.REGTYPE, 0o644, b"x")],
                 [("empack", tarfile.REGTYPE, 0o4755, b"x")],
                 [("empack", tarfile.REGTYPE, 0o755, b"")],
                 [("empack", tarfile.REGTYPE, 0o755, b"x"),
                  ("unexpected", tarfile.REGTYPE, 0o644, b"y")]]
        for entries in cases:
            with self.subTest(entries=entries), tempfile.TemporaryDirectory(dir=self.root) as output:
                with self.assertRaises(ValueError):
                    package.extract_binary(self.tar(entries), Path(output), "x86_64-unknown-linux-gnu")
                self.assertFalse((self.root / "outside").exists())

    def test_windows_zip_is_exact_and_crc_checked(self):
        path = self.root / "fixture.zip"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("empack.exe", b"inert bytes")
        binary, digest, length = package.extract_binary(path, self.destination, "x86_64-pc-windows-msvc")
        self.assertEqual(binary.read_bytes(), b"inert bytes")
        self.assertEqual((digest, length), (package.sha256(binary), 11))
        binary.unlink()
        # Stored bytes have identical length but the central CRC remains unchanged.
        path.write_bytes(path.read_bytes().replace(b"inert bytes", b"wrong bytes"))
        with self.assertRaises(zipfile.BadZipFile):
            package.extract_binary(path, self.destination, "x86_64-pc-windows-msvc")

    def test_zip_rejects_links_duplicates_and_other_member_names(self):
        for name, mode in (("../empack.exe", 0o100644), ("empack.exe", 0o120777),
                           ("folder/empack.exe", 0o100644), ("empack.exe/", 0o40755)):
            with self.subTest(name=name, mode=mode):
                path = self.root / "fixture.zip"
                with zipfile.ZipFile(path, "w") as archive:
                    info = zipfile.ZipInfo(name)
                    info.external_attr = mode << 16
                    archive.writestr(info, b"inert")
                with self.assertRaises(ValueError):
                    package.extract_binary(path, self.destination, "x86_64-pc-windows-msvc")
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("empack.exe", b"inert")
            archive.writestr("unexpected", b"extra")
        with self.assertRaises(ValueError):
            package.extract_binary(path, self.destination, "x86_64-pc-windows-msvc")

    def test_capture_checks_the_bytes_it_copies_and_cannot_overwrite_an_existing_output(self):
        original = self.root / "source"
        original.write_bytes(b"exact archive")
        captured = self.root / "captured"
        package.capture_archive(original, captured, package.sha256(original))
        original.write_bytes(b"changed archive")
        self.assertEqual(captured.read_bytes(), b"exact archive")
        with self.assertRaises(FileExistsError):
            package.capture_archive(original, captured, package.sha256(original))
        with self.assertRaisesRegex(ValueError, "expected SHA-256"):
            package.capture_archive(original, self.root / "wrong-hash", "0" * 64)

    def test_capture_and_uncompressed_binary_have_independent_bounds(self):
        source = self.root / "source"
        source.write_bytes(b"too large")
        with patch.object(package, "MAX_ARCHIVE", 2), self.assertRaisesRegex(ValueError, "byte limit"):
            package.capture_archive(source, self.root / "captured", package.sha256(source))
        archive = self.tar([("empack", tarfile.REGTYPE, 0o755, b"large" * 1000)])
        with patch.object(package, "MAX_BINARY", 100), self.assertRaisesRegex(ValueError, "size"):
            package.extract_binary(archive, self.destination, "x86_64-unknown-linux-gnu")


if __name__ == "__main__":
    unittest.main()
