"""Bounded reader for empack's one-executable native distribution archives."""
import hashlib
import os
import re
import stat
import tarfile
import zipfile

from .release import TARGETS

MAX_ARCHIVE = 512 * 1024 * 1024
MAX_BINARY = 256 * 1024 * 1024


def copy_bounded(source, destination, limit):
    digest, length = hashlib.sha256(), 0
    with destination.open("xb") as output:
        while chunk := source.read(65536):
            length += len(chunk)
            if length > limit:
                raise ValueError("Release content exceeds the admitted byte limit")
            digest.update(chunk)
            output.write(chunk)
    return digest.hexdigest(), length


def _capture(source, destination, expected, limit):
    if not re.fullmatch(r"[a-f0-9]{64}", expected):
        raise ValueError("Expected archive SHA-256 must be 64 lowercase hexadecimal digits")
    if source.is_symlink() or not source.is_file():
        raise ValueError("Release input must be a regular file")
    with source.open("rb") as stream:
        digest, _ = copy_bounded(stream, destination, limit)
    if digest != expected:
        raise ValueError("Captured input differs from the expected SHA-256")


def capture_archive(source, destination, expected):
    _capture(source, destination, expected, MAX_ARCHIVE)


def capture_binary(source, destination, expected):
    """Restore invocation attributes on an owned copy, never on downloaded proof."""
    _capture(source, destination, expected, MAX_BINARY)
    if os.name != "nt":
        destination.chmod(0o755)


def extract_binary(archive, directory, target):
    """Admit the shipped one-file layout, not arbitrary archive extraction paths."""
    extension, filename = TARGETS[target]
    destination = directory / filename
    if extension == "zip":
        with zipfile.ZipFile(archive) as package:
            entries = package.infolist()
            if len(entries) != 1 or entries[0].filename != filename:
                raise ValueError("Release ZIP must contain exactly the expected executable")
            entry = entries[0]
            kind = stat.S_IFMT(entry.external_attr >> 16)
            if kind not in (0, stat.S_IFREG) or entry.flag_bits & 1:
                raise ValueError("Release executable must be a regular, unencrypted file")
            if not 0 < entry.file_size <= MAX_BINARY:
                raise ValueError("Release executable size is outside the admitted bound")
            with package.open(entry) as stream:
                digest, length = copy_bounded(stream, destination, MAX_BINARY)
            if length != entry.file_size:
                raise ValueError("Extracted executable length differs from ZIP metadata")
    else:
        with tarfile.open(archive, "r:gz") as package:
            found = False
            root_seen = False
            # Iterate so an unbounded index cannot be allocated before admission.
            for entry in package:
                if entry.name in (".", "./") and entry.isdir() and not root_seen and not found:
                    root_seen = True
                    continue
                if found or entry.name not in (filename, "./" + filename) or not entry.isfile():
                    raise ValueError("Release TAR must contain exactly the expected regular executable")
                if entry.mode & 0o7000 or entry.mode & 0o111 != 0o111:
                    raise ValueError("Release executable needs executable permissions without special bits")
                if not 0 < entry.size <= MAX_BINARY:
                    raise ValueError("Release executable size is outside the admitted bound")
                with package.extractfile(entry) as stream:
                    digest, length = copy_bounded(stream, destination, MAX_BINARY)
                if length != entry.size:
                    raise ValueError("Extracted executable length differs from TAR metadata")
                found = True
            if not found:
                raise ValueError("Release TAR has no executable")
        destination.chmod(0o755)
    return destination, digest, length
