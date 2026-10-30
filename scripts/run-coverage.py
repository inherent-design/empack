#!/usr/bin/env python3
"""Run instrumented tests with compiler-bound private fixtures; retain failures."""
import json
import os
from pathlib import Path
import re
import shlex
import sys

from verification.evidence import sha256
from verification.process import run

ROOT = Path(__file__).resolve().parents[1]
FEATURES = "test-utils,telemetry"


def instrumented_environment(output, environment):
    """Read cargo-llvm-cov's quoted assignments as data, without evaluating a shell."""
    result = dict(environment)
    for line in output.split("\n"):
        if not line.strip():
            continue
        words = shlex.split(line)
        if len(words) != 2 or words[0] != "export" or "=" not in words[1]:
            raise ValueError("Unexpected cargo-llvm-cov environment assignment")
        name, value = words[1].split("=", 1)
        if not re.fullmatch(r"[A-Z_][A-Z_0-9]*", name):
            raise ValueError("Invalid cargo-llvm-cov environment name")
        result[name] = value
    if result.get("CARGO_LLVM_COV") != "1" or not result.get("RUSTC_WRAPPER"):
        raise ValueError("Missing cargo-llvm-cov instrumentation environment")
    return result


def coverage(root=ROOT, environment=None):
    root = Path(root)
    environment = dict(os.environ if environment is None else environment)
    target = root / "target/llvm-cov-target"
    environment.update(CARGO_TARGET_DIR=str(target), CARGO_LLVM_COV_TARGET_DIR=str(target))
    output = root / "target/verification/coverage"
    output.mkdir(parents=True, exist_ok=True)

    def invoke(arguments, name, env, *, capture=False, checked=True):
        log = output / f"{name}.log"
        code, text, timed_out = run(arguments, cwd=root, env=env, log=log,
                                    timeout=1800, separate_stdout=capture)
        status = 124 if timed_out else 128 - code if code < 0 else code
        if checked and status:
            raise RuntimeError(f"{name} failed (exit {status}, timeout={timed_out}); see {log}")
        return status, text

    invoke(["cargo", "llvm-cov", "clean", "--workspace"], "clean", environment)
    _, settings = invoke(["cargo", "llvm-cov", "show-env", "--sh"], "environment", environment, capture=True)
    instrumented = instrumented_environment(settings, environment)
    invoke(["cargo", "run", "--locked", "-p", "empack", "--features", "telemetry", "--", "version"],
           "product", instrumented)
    _, inventory = invoke(["cargo", "nextest", "list", "--locked", "--workspace", "--features", FEATURES,
                           "--message-format", "json"], "discovery", instrumented, capture=True)
    parsed = json.loads(inventory)
    if not parsed.get("rust-suites") or not parsed.get("test-count"):
        raise ValueError("Instrumented compiler inventory is empty")
    inventory_path = output / "inventory.json"
    inventory_path.write_text(inventory, encoding="utf-8")
    instrumented.update(EMPACK_TEST_INVENTORY=str(inventory_path.resolve()),
                        EMPACK_TEST_INVENTORY_SHA256=sha256(inventory_path))
    # These are instrumented development tests, not release qualification receipts.
    status, _ = invoke(["cargo", "nextest", "run", "--locked", "--workspace", "--features", FEATURES,
                        "--retries", "0", "--no-fail-fast"], "tests", instrumented, checked=False)
    report, _ = invoke(["cargo", "llvm-cov", "report", "--failure-mode", "all", "--lcov",
                        "--output-path", str(root / "lcov.info")], "report", environment, checked=False)
    print(f"Coverage tests: exit {status}; report: exit {report}; logs: {output}")
    return status or report


if __name__ == "__main__":
    try:
        raise SystemExit(coverage())
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Coverage failed: {error}", file=sys.stderr)
        raise SystemExit(1)
