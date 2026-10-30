#!/usr/bin/env python3
"""Build the candidate telemetry variant and test it without altering release archives."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import uuid

import verify
from verification.auxiliary import SUITES, build_command, inspect_auxiliary
from verification.evidence import sha256, write_json
from verification.process import run
from verification.release import TARGETS, git
from verification.release_inputs import prepare
from verification.product_build import compile_product

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "target/verification-auxiliary")
    args = parser.parse_args()
    if git(ROOT, "status", "--porcelain"):
        parser.error("Auxiliary qualification requires a clean source checkout")
    revision = git(ROOT, "rev-parse", "HEAD")
    attempt = args.output.resolve() / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    record = dict(schema=1, kind="candidate-auxiliary-build", source_revision=revision,
                  version=args.version, features="telemetry", profile="release", shipped=False,
                  status="NOT_RUN", created_at=datetime.now(timezone.utc).isoformat())
    print(f"Auxiliary build attempt: {attempt}", flush=True)
    # Compiler tool locations may be retained, but feature/compiler injection and
    # user git/Cargo configuration are not inherited by the disposable build.
    environment = verify.isolated_environment(attempt / "home", compiler=True)
    (attempt / "home").mkdir()
    environment["CARGO_HOME"] = str(attempt / "home/cargo")
    environment["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
    environment.update(CARGO_TERM_COLOR="never", GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
    try:
        compiler = verify.checked(["rustc", "-vV"], attempt, "compiler", environment).strip()
        target = next(line.removeprefix("host: ") for line in compiler.splitlines() if line.startswith("host: "))
        if target not in TARGETS:
            raise ValueError("Unsupported auxiliary execution target")
        record.update(target=target, compiler=compiler, command=build_command(target))
        # Source stamping is confined to this clone. The suite runs from the
        # original clean candidate checkout against the retained executable.
        with tempfile.TemporaryDirectory(prefix="empack-auxiliary-") as temporary:
            source = Path(temporary) / "source"
            verify.checked(["git", "clone", "--shared", "--no-checkout", str(ROOT), str(source)],
                           attempt, "clone", environment)
            verify.checked(["git", "checkout", "--detach", revision], attempt, "checkout", environment, cwd=source)
            inputs = prepare(source, args.version)
            record["inputs"] = inputs
            metadata = dict(GIT_HASH=revision, BUILD_DATE=inputs["build_date"], CARGO_INCREMENTAL="0")
            record["build_environment"] = metadata
            # Separate from workspace tests and the distributed default build.
            target_dir = ROOT / "target/verification-auxiliary-product"
            build_environment = dict(environment, **metadata, CARGO_TARGET_DIR=str(target_dir))
            compile_product(source, target, "cargo", attempt, build_environment, "telemetry")
            record.update(build_exit_code=0, build_timed_out=False,
                          compilation_sha256=sha256(attempt / "compile.json"))
            changed = set(git(source, "diff", "HEAD", "--name-only").splitlines())
            if changed - {"Cargo.toml", "Cargo.lock"} or any(sha256(source / n) != d for n, d in inputs["stamped"].items()):
                raise ValueError("Auxiliary compilation changed reviewed source inputs")
            executable = attempt / TARGETS[target][1]
            shutil.copy2(target_dir / target / "release" / executable.name, executable)
            record["executable"] = dict(sha256=sha256(executable), length=executable.stat().st_size)
        command = [sys.executable, str(ROOT / "scripts/verify.py"), *SUITES,
                   "--binary", str(executable), "--sha256", record["executable"]["sha256"],
                   "--version", args.version, "--target", target, "--product-features", "telemetry",
                   "--product-profile", "release", "--test-features", "all", "--output", str(attempt / "suite")]
        code, _, timed_out = run(command, cwd=ROOT, env=os.environ.copy(), log=attempt / "verification.log", timeout=5400)
        receipts = list((attempt / "suite").glob("*/receipt.json"))
        if len(receipts) != 1 or git(ROOT, "rev-parse", "HEAD") != revision or git(ROOT, "status", "--porcelain"):
            raise ValueError("Auxiliary verification lost its single attempt or clean source identity")
        status = json.loads(receipts[0].read_text())["status"]
        record.update(suite_receipt_sha256=sha256(receipts[0]), status=status)
        if timed_out or bool(code) != (status != "PASS"):
            raise RuntimeError("Auxiliary runner outcome disagrees with its suite; see verification.log")
        write_json(attempt / "build.json", record)
        inspect_auxiliary(attempt / "build.json", ROOT, revision, args.version)
    except (OSError, ValueError, RuntimeError, KeyError, StopIteration, KeyboardInterrupt) as error:
        record.update(status="FAIL", reason=str(error) or "Auxiliary verification interrupted")
    write_json(attempt / "build.json", record)
    print(f"{record['status']}: {attempt / 'build.json'}", flush=True)
    return 0 if record["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
