#!/usr/bin/env python3
"""Run the fixed offline CLI suite against a captured final distribution archive."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tarfile
import tempfile
import uuid
import zipfile

import verify
from verification.evidence import sha256, write_json

TARGETS = {
    "x86_64-unknown-linux-gnu": ("tar.gz", "empack"),
    "aarch64-unknown-linux-gnu": ("tar.gz", "empack"),
    "x86_64-apple-darwin": ("tar.gz", "empack"),
    "aarch64-apple-darwin": ("tar.gz", "empack"),
    "x86_64-pc-windows-msvc": ("zip", "empack.exe"),
}
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


def capture_archive(source, destination, expected):
    if not re.fullmatch(r"[a-f0-9]{64}", expected):
        raise ValueError("Expected archive SHA-256 must be 64 lowercase hexadecimal digits")
    if source.is_symlink() or not source.is_file():
        raise ValueError("Release archive must be a regular file")
    with source.open("rb") as stream:
        digest, _ = copy_bounded(stream, destination, MAX_ARCHIVE)
    if digest != expected:
        raise ValueError("Captured archive differs from the expected SHA-256")


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


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True, help="Expected archive SHA-256")
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, default=verify.ROOT / "target/verification-packages")
    args = parser.parse_args(argv)
    archive = args.archive.absolute()
    args.output.mkdir(parents=True, exist_ok=True)
    attempt = args.output.resolve() / str(uuid.uuid4())
    attempt.mkdir()
    record = {"schema": 1, "kind": "extracted-release-archive", "status": "NOT_RUN",
              "qualification": False, "created_at": datetime.now(timezone.utc).isoformat(),
              "archive": {"name": archive.name, "sha256": args.sha256},
              "version": args.version, "target": args.target, "features": "default",
              "scope": "fixed offline CLI suite on extracted bytes; not full release qualification"}
    try:
        extension, _ = TARGETS[args.target]
        if archive.name != f"empack-{args.version}-{args.target}.{extension}":
            raise ValueError("Archive name does not match the selected version and target")
        environment = dict(os.environ)
        compiler = verify.checked(["rustc", "-vV"], attempt, "rustc", environment)
        if f"host: {args.target}" not in compiler.splitlines():
            raise verify.Blocked("Archive must execute on its native target; cross compilation is not execution")
        # The shared suite may compile its test harness in the checkout, but the
        # product binary and every authoring/instance fixture live outside it.
        with tempfile.TemporaryDirectory(prefix="empack archive ") as temporary:
            root = Path(temporary).resolve()
            if root.is_relative_to(verify.ROOT):
                raise ValueError("Release executable must be extracted outside the source checkout")
            captured = root / "captured-archive"
            capture_archive(archive, captured, args.sha256)
            executable, digest, length = extract_binary(captured, root, args.target)
            record["executable"] = {"sha256": digest, "length": length}
            suite_root = attempt / "suite"
            code = verify.main(["cli", "--binary", str(executable), "--sha256", digest,
                                "--version", args.version, "--target", args.target,
                                "--product-features", "default", "--output", str(suite_root)])
            receipts = list(suite_root.glob("*/receipt.json"))
            if len(receipts) != 1:
                raise ValueError("Archive execution did not produce exactly one suite attempt")
            receipt = receipts[0]
            result = json.loads(receipt.read_text(encoding="utf-8"))
            record.update(test_source_revision=result["source_revision"],
                          test_source_snapshot_sha256=result["source_snapshot_sha256"],
                          test_worktree_dirty=result["worktree_dirty"],
                          suite_receipt=str(receipt.relative_to(attempt)),
                          suite_receipt_sha256=sha256(receipt))
            if code or result["status"] != "PASS":
                record["status"] = "BLOCKED" if result["status"] == "BLOCKED" else "FAIL"
                raise RuntimeError("Extracted archive did not pass the fixed offline CLI suite")
            if sha256(archive) != args.sha256 or sha256(captured) != args.sha256 or sha256(executable) != digest:
                raise ValueError("Archive or extracted executable changed during verification")
            record["status"] = "PASS"
    except verify.Blocked as error:
        record.update(status="BLOCKED", reason=str(error))
    except KeyboardInterrupt:
        record.update(status="FAIL", reason="Archive verification interrupted")
    except (OSError, EOFError, ValueError, KeyError, RuntimeError, tarfile.TarError, zipfile.BadZipFile) as error:
        record.update(status="FAIL" if record["status"] != "BLOCKED" else "BLOCKED", reason=str(error))
    write_json(attempt / "package.json", record)
    print(f"{record['status']}: {attempt / 'package.json'}", flush=True)
    if "reason" in record:
        print(record["reason"], file=sys.stderr)
    return 0 if record["status"] == "PASS" else 2 if record["status"] == "BLOCKED" else 1


if __name__ == "__main__":
    sys.exit(main())
