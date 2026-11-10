#!/usr/bin/env python3
"""Run the fixed offline CLI suite against a captured final distribution archive."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import uuid
import zipfile

import verify
from verification.evidence import sha256, write_json
from verification.native_image import capture as capture_image, InspectionUnavailable
from verification.release import TARGETS
from verification.progress import phase

from verification.package_archive import capture_archive, extract_binary


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True, help="Expected archive SHA-256")
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--profile", choices=("dev", "release"), default="release",
                        help="Release builds are required for publication; dev is diagnostic only")
    parser.add_argument("--output", type=Path, default=verify.ROOT / "target/verification-packages")
    args = parser.parse_args(argv)
    archive = args.archive.absolute()
    args.output.mkdir(parents=True, exist_ok=True)
    attempt = args.output.resolve() / str(uuid.uuid4())
    attempt.mkdir()
    record = {"schema": 2, "kind": "extracted-release-archive", "status": "NOT_RUN",
              "qualification": False, "created_at": datetime.now(timezone.utc).isoformat(),
              "archive": {"name": archive.name, "sha256": args.sha256},
              "version": args.version, "target": args.target, "features": "default", "profile": args.profile,
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
            with phase(attempt, "capture-archive"):
                capture_archive(archive, captured, args.sha256)
                executable, digest, length = extract_binary(captured, root, args.target)
            record["executable"] = {"sha256": digest, "length": length}
            with phase(attempt, "inspect-native-image"):
                record["native_image_sha256"] = capture_image(
                    executable, args.target, digest, attempt / "native-image", environment)
            suite_root = attempt / "suite"
            with phase(attempt, "run-cli-suite"):
                code = verify.main(["cli", "--binary", str(executable), "--sha256", digest,
                                    "--version", args.version, "--target", args.target,
                                    "--product-profile", args.profile,
                                    "--product-features", "default", "--output", str(suite_root)])
            receipts = list(suite_root.glob("*/receipt.json"))
            if len(receipts) != 1:
                raise ValueError("Archive execution did not produce exactly one suite attempt")
            receipt = receipts[0]
            result = json.loads(receipt.read_text(encoding="utf-8"))
            record.update(test_source_revision=result["source_revision"],
                          test_source_snapshot_sha256=result["source_snapshot_sha256"],
                          test_worktree_dirty=result["worktree_dirty"],
                          suite_receipt=receipt.relative_to(attempt).as_posix(),
                          suite_receipt_sha256=sha256(receipt))
            if code or result["status"] != "PASS":
                record["status"] = "BLOCKED" if result["status"] == "BLOCKED" else "FAIL"
                raise RuntimeError("Extracted archive did not pass the fixed offline CLI suite")
            if sha256(archive) != args.sha256 or sha256(captured) != args.sha256 or sha256(executable) != digest:
                raise ValueError("Archive or extracted executable changed during verification")
            record["status"] = "PASS"
    except (verify.Blocked, InspectionUnavailable) as error:
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
