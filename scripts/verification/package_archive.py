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
MAX_LICENSE = 64 * 1024


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
    """Admit one executable and an optional LICENSE, including older binary-only releases."""
    extension, filename = TARGETS[target]
    destination = directory / filename
    seen = set()
    binary_digest = binary_length = None
    if extension == "zip":
        with zipfile.ZipFile(archive) as package:
            entries = package.infolist()
            if not 1 <= len(entries) <= 2:
                raise ValueError("Release ZIP must contain the expected executable and optional LICENSE")
            for entry in entries:
                name = entry.filename
                if name not in (filename, "LICENSE") or name in seen:
                    raise ValueError("Release ZIP contains an unexpected or duplicate member")
                seen.add(name)
                kind = stat.S_IFMT(entry.external_attr >> 16)
                if kind not in (0, stat.S_IFREG) or entry.flag_bits & 1:
                    raise ValueError("Release member must be a regular, unencrypted file")
                limit = MAX_BINARY if name == filename else MAX_LICENSE
                if not 0 < entry.file_size <= limit:
                    raise ValueError("Release member size is outside the admitted bound")
                with package.open(entry) as stream:
                    digest, length = copy_bounded(stream, directory / name, limit)
                if length != entry.file_size:
                    raise ValueError("Extracted member length differs from ZIP metadata")
                if name == filename:
                    binary_digest, binary_length = digest, length
    else:
        with tarfile.open(archive, "r:gz") as package:
            root_seen = False
            # Iterate so an unbounded index cannot be allocated before admission.
            for entry in package:
                if entry.name in (".", "./") and entry.isdir() and not root_seen and not seen:
                    root_seen = True
                    continue
                name = entry.name[2:] if entry.name.startswith("./") else entry.name
                if name not in (filename, "LICENSE") or name in seen or not entry.isfile():
                    raise ValueError("Release TAR contains an unexpected, duplicate or non-regular member")
                seen.add(name)
                if entry.mode & 0o7000 or (name == filename and entry.mode & 0o111 != 0o111):
                    raise ValueError("Release members cannot use special bits; the executable needs executable permissions")
                limit = MAX_BINARY if name == filename else MAX_LICENSE
                if not 0 < entry.size <= limit:
                    raise ValueError("Release member size is outside the admitted bound")
                with package.extractfile(entry) as stream:
                    digest, length = copy_bounded(stream, directory / name, limit)
                if length != entry.size:
                    raise ValueError("Extracted member length differs from TAR metadata")
                if name == filename:
                    binary_digest, binary_length = digest, length
    if filename not in seen:
        raise ValueError("Release archive has no executable")
    if extension != "zip":
        destination.chmod(0o755)
    return destination, binary_digest, binary_length
