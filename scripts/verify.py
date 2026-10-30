#!/usr/bin/env python3
"""Run fixed verification suites and retain one evidence record per attempt."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import shutil
import sys
import tomllib
import uuid

from verification.classification import compare_rust_inventory
from verification.evidence import aggregate_outcomes, read_results, selected_tests, sha256, write_json
from verification.process import run
from verification.product_build import retain_build
from verification.suites import SUITES, TEST_FEATURES, nextest_command

ROOT = Path(__file__).resolve().parents[1]
NEXTEST_VERSION = "0.9.148"


class Blocked(Exception):
    pass


def checked(command, attempt, name, environment, timeout=120, cwd=ROOT):
    try:
        code, output, timed_out = run(command, cwd=cwd, env=environment,
                                     log=attempt / f"{name}.log", timeout=timeout,
                                     separate_stdout=True)
    except FileNotFoundError as error:
        raise Blocked(f"Required command unavailable: {command[0]}") from error
    if code or timed_out:
        raise RuntimeError(f"{name} failed (exit {code}, timeout={timed_out}); see {attempt / (name + '.log')}")
    return output


def isolated_environment(root, *, compiler=False):
    names = ("PATH", "Path", "SystemRoot", "SYSTEMROOT", "WINDIR", "COMSPEC",
             "PATHEXT", "JAVA_HOME", "SSL_CERT_FILE", "SSL_CERT_DIR")
    result = {key: os.environ[key] for key in names if key in os.environ}
    if compiler and os.name == "nt":
        # Rust's MSVC discovery needs installation roots (vswhere) or the
        # configured developer toolset. Keep these, not Cargo flags/credentials.
        toolset = {"PROGRAMFILES", "PROGRAMFILES(X86)", "VSINSTALLDIR", "VCINSTALLDIR",
                   "VCTOOLSVERSION", "VSCMD_ARG_TGT_ARCH", "VSCMD_ARG_VCVARS_SPECTRE",
                   "WINDOWSSDKDIR", "WINDOWSSDKVERSION", "LIB", "LIBPATH", "INCLUDE"}
        result.update((key, value) for key, value in os.environ.items() if key.upper() in toolset)
    for key in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME",
                "XDG_DATA_HOME", "XDG_CACHE_HOME", "EMPACK_CACHE_DIR", "EMPACK_STATE_DIR",
                "TMP", "TEMP", "TMPDIR"):
        result[key] = str(root)
    result.update(NO_COLOR="1", LANG="C", LC_ALL="C", TZ="UTC")
    return result


def python_environment(home, attempt, environment):
    """Reuse installed compiler tools without inheriting Cargo or application state."""
    probe_environment = dict(environment, RUSTUP_AUTO_INSTALL="0")
    rustup_home = Path(checked(["rustup", "show", "home"], attempt, "rustup-home",
                               probe_environment).strip())
    if not rustup_home.is_absolute() or not rustup_home.is_dir():
        raise Blocked("Python verification requires an existing absolute Rustup home")
    pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    worker = isolated_environment(home, compiler=True)
    worker.update(RUSTUP_HOME=str(rustup_home), RUSTUP_TOOLCHAIN=pin, RUSTUP_AUTO_INSTALL="0",
                  CARGO_HOME=str(home / ".cargo"))
    try:
        rustc = checked(["rustc", "-vV"], attempt, "python-rustc", worker, cwd=home).strip()
        cargo = checked(["cargo", "--version"], attempt, "python-cargo", worker, cwd=home).strip()
    except RuntimeError as error:
        raise Blocked(f"Python verification requires installed Rust {pin}; automatic installation is disabled: {error}") from error
    if f"release: {pin}" not in rustc.splitlines() or not cargo.startswith(f"cargo {pin} "):
        raise Blocked(f"Python verification requires pinned Rust {pin}")
    return worker, dict(rustc=rustc, cargo=cargo, toolchain=pin, automatic_installation=False)


def product(args, attempt, environment, host):
    features = args.product_features
    profile = args.product_profile
    build_audit = {}
    if args.binary:
        if not all((args.sha256, args.version, args.target, features, profile)):
            raise Blocked("--binary requires --sha256, --version, --target, --product-features and --product-profile")
        path = Path(args.binary)
        if not path.is_absolute() or not path.is_file():
            raise Blocked("--binary must identify an existing absolute file")
        path = path.resolve()
        if not re.fullmatch(r"[a-fA-F0-9]{64}", args.sha256):
            raise Blocked("Expected SHA-256 must contain 64 hexadecimal digits")
        digest, version, target = args.sha256.lower(), args.version, args.target
        provenance = "caller-supplied target/features; digest and version checked"
    else:
        if any((args.sha256, args.version, args.target)):
            raise Blocked("Artifact identity overrides require --binary")
        features = features or ("telemetry" if any(SUITES[s].telemetry for s in args.suites) else "default")
        profile = profile or "dev"
        target = host
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        # A distinct target directory keeps product compilation outside the additive
        # workspace test feature graph. Build the ordinary product package only.
        target_dir = (ROOT / "target" / "verification-product" / features).resolve()
        command = ["cargo", "build", "--locked", "-p", "empack", "--target-dir", str(target_dir)]
        if profile == "release":
            command += ["--release"]
        if features == "telemetry":
            command += ["--features", "telemetry"]
        command += ["--message-format=json"]
        messages = checked(command, attempt, "product-build", environment, timeout=1800, cwd=ROOT)
        metadata = checked(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
                           attempt, "product-metadata", environment, cwd=ROOT)
        path = target_dir / ("release" if profile == "release" else "debug") / ("empack.exe" if os.name == "nt" else "empack")
        path = path.resolve()
        digest = sha256(path)
        compiled = retain_build(attempt, messages, json.loads(metadata), features, path, digest)
        build_audit = {"build_audit_sha256": compiled["build_audit_sha256"]}
        provenance = "built by this runner from the ordinary product package"
    if sha256(path) != digest:
        raise Blocked("Selected binary does not match the expected SHA-256")
    probe = attempt / "version-home"
    probe.mkdir()
    output = checked([str(path), "--version"], attempt, "product-version",
                     isolated_environment(probe), timeout=15, cwd=probe)
    if output.strip() != f"empack {version}":
        raise Blocked("Selected binary does not match the expected version")
    return {"path": str(path), "sha256": digest, "version": version,
            "target": target, "features": features, "profile": profile, "provenance": provenance,
            **build_audit}


def java_probe(attempt, environment, label="java", home_variable="JAVA_HOME"):
    executable = "java.exe" if os.name == "nt" else "java"
    home = environment.get(home_variable)
    path = Path(home) / "bin" / executable if home else shutil.which(executable, path=environment.get("PATH"))
    if not path or not Path(path).is_file():
        raise Blocked(f"Java prerequisite unavailable: {home_variable or 'PATH'}")
    path = Path(path).resolve()
    try:
        checked([str(path), "-version"], attempt, f"{label}-version", environment, timeout=15)
    except (OSError, RuntimeError) as error:
        raise Blocked(f"{label} prerequisite unavailable; no runtime tests executed") from error
    version = (attempt / f"{label}-version.log").read_text().strip()
    if label == "java8" and not re.search(r'version "1\.8\.', version):
        raise Blocked("Historical runtime suite requires Java 8 at EMPACK_TEST_JAVA8_HOME")
    return {"path": str(path), "sha256": sha256(path), "version": version}


def run_suite(name, suite, attempt, environment, artifact, record, test_features):
    if reason := suite.inapplicable_reason(artifact["target"]):
        record.update(status="NOT_APPLICABLE", reason=reason)
        return record
    if suite.telemetry and artifact["features"] != "telemetry":
        raise Blocked("Telemetry suite requires a telemetry product artifact")
    missing = [name for name in suite.required_env if not environment.get(name)]
    if missing:
        raise Blocked("Required suite prerequisites unavailable: " + ", ".join(missing))
    if suite.java:
        record["java"] = java_probe(attempt, environment)
        if "EMPACK_TEST_JAVA8_HOME" in suite.required_env:
            record["java8"] = java_probe(attempt, environment, "java8", "EMPACK_TEST_JAVA8_HOME")
    suite_dir = attempt / name
    suite_dir.mkdir()
    junit = suite_dir / "junit.xml"
    config = suite_dir / "nextest.toml"
    config.write_text((ROOT / ".config/nextest.toml").read_text() + f'''
[profile.verification]
default-filter = "all()"
retries = 0
fail-fast = false
junit.path = {json.dumps(str(junit))}
junit.report-skipped = "all"
junit.store-success-output = {str(name == "native-remount").lower()}
junit.store-failure-output = false
''')
    discovery_command = nextest_command("list", suite, config, test_features)
    record["discovery_command"] = discovery_command
    output = checked(discovery_command, suite_dir, "discovery", environment, timeout=1800)
    inventory = json.loads(output)
    write_json(suite_dir / "inventory.json", inventory)
    selected, all_ids = selected_tests(inventory, suite.ignored, suite.binaries, suite.cases)
    bindings = json.loads((ROOT / "tests/verification/test-bindings.json").read_text(encoding="utf-8"))
    requirements = json.loads((ROOT / "tests/verification/requirements.json").read_text(encoding="utf-8"))
    compare_rust_inventory(inventory, bindings, requirements["api_routes"], suite.binaries, test_features)
    record["classification_sha256"] = sha256(ROOT / "tests/verification/test-bindings.json")
    record["selected"] = selected
    record["inventory_sha256"] = sha256(suite_dir / "inventory.json")
    run_env = dict(environment, EMPACK_QUALIFY="1", EMPACK_E2E_STRICT="1",
                   EMPACK_E2E_BIN=artifact["path"], EMPACK_EXPECT_BINARY_SHA256=artifact["sha256"],
                   EMPACK_EXPECT_VERSION=artifact["version"], EMPACK_TEST_PYTHON=sys.executable)
    # Only test processes receive compiler discovery for private crash fixtures.
    # Product children use the harness allowlist and never inherit these controls.
    run_env.update(EMPACK_TEST_INVENTORY=str((suite_dir / "inventory.json").resolve()),
                   EMPACK_TEST_INVENTORY_SHA256=record["inventory_sha256"])
    command = nextest_command("run", suite, config, test_features)
    record["command"] = command
    code, _, timed_out = run(command, cwd=ROOT, env=run_env,
                             log=suite_dir / "run.log", timeout=suite.timeout)
    if junit.is_file():
        record["tests"] = read_results(junit, selected, all_ids)
        record["junit_sha256"] = sha256(junit)
    else:
        record["tests"] = [{"id": identity, "status": "NOT_RUN"} for identity in selected]
    record["status"] = "PASS" if code == 0 and not timed_out and all(
        case["status"] == "PASS" for case in record["tests"]) else "FAIL"
    record.update(exit_code=code, timed_out=timed_out)
    return record


def source_snapshot(attempt, environment):
    attempt = attempt.resolve()
    paths = checked(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                    attempt, "source-files", environment, cwd=ROOT).split("\0")
    files = {}
    for name in sorted(set(paths) - {""}):
        path = ROOT / name
        if path.resolve().is_relative_to(attempt):
            continue
        if path.is_symlink():
            files[name] = {"symlink": os.readlink(path)}
        elif path.is_file():
            files[name] = {"sha256": sha256(path), "executable": bool(path.stat().st_mode & 0o111)}
        elif not path.exists():
            # The snapshot describes present source bytes, not index bookkeeping.
            # Staging an already-deleted file must not change that identity.
            continue
        else:
            raise ValueError(f"Unsupported source entry: {name}")
    return files


def aggregate_status(results):
    return aggregate_outcomes(row["status"] for row in results)


def run_python(attempt, environment):
    from verification.python_tests import BINDINGS, compare_inventory, inputs, platform_outcomes
    record = dict(schema=1, kind="python-suite-run", status="NOT_RUN", qualification=False,
                  created_at=datetime.now(timezone.utc).isoformat(), features="independent",
                  platform={"Darwin": "macos", "Linux": "linux", "Windows": "windows"}[platform.system()],
                  architecture=platform.machine(), python_version=platform.python_version(),
                  interpreter_sha256=sha256(Path(sys.executable).resolve()), tests=[])
    print(f"Python verification attempt: {attempt}", flush=True)
    try:
        record["source_revision"] = checked(["git", "rev-parse", "HEAD"], attempt, "revision", environment).strip()
        record["worktree_dirty"] = bool(checked(["git", "status", "--porcelain"], attempt, "worktree", environment).strip())
        snapshot = source_snapshot(attempt, environment)
        record["inputs"] = inputs(ROOT)
        record["classification_sha256"] = sha256(ROOT / BINDINGS)
        home = attempt / "worker-home"
        home.mkdir()
        worker, record["compiler_tools"] = python_environment(home, attempt, environment)
        code, _, timed_out = run([sys.executable, str(ROOT / "scripts/python-test-driver.py"), "--output", str(attempt)],
            cwd=ROOT, env=worker, log=attempt / "run.log", timeout=900)
        record.update(exit_code=code, timed_out=timed_out)
        inventory = json.loads((attempt / "inventory.json").read_text(encoding="utf-8"))
        classification = json.loads((ROOT / BINDINGS).read_text(encoding="utf-8"))
        compare_inventory(inventory, classification)
        events = json.loads((attempt / "events.json").read_text(encoding="utf-8"))
        record["inventory_sha256"] = sha256(attempt / "inventory.json")
        record["events_sha256"] = sha256(attempt / "events.json")
        record["tests"], record["status"] = platform_outcomes(inventory, events, classification, record["platform"])
        if code or timed_out:
            record["status"] = "FAIL"
        if source_snapshot(attempt, environment) != snapshot:
            raise RuntimeError("Source files changed during verification")
    except Blocked as error:
        record.update(status="BLOCKED", reason=str(error))
    except (OSError, ValueError, RuntimeError, KeyError, KeyboardInterrupt) as error:
        record.update(status="FAIL", reason=str(error) or "Python verification interrupted")
    write_json(attempt / "receipt.json", record)
    print(f"{record['status']}: {attempt / 'receipt.json'} ({len(record['tests'])} tests)", flush=True)
    return 0 if record["status"] == "PASS" else 1


def run_doctests(attempt, environment, features):
    from verification.doctests import BINDINGS, TARGETS, command, evaluate, inputs, inventory
    # Do not permit inherited rustdoc options to select a different test set or
    # replace the pinned rustdoc executable while retaining a nominal Cargo command.
    environment = {key: value for key, value in environment.items() if key not in {
        "RUSTDOC", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS", "RUSTC_BOOTSTRAP"}}
    record = dict(schema=1, kind="doctest-suite-run", qualification=False, tests=[],
                  created_at=datetime.now(timezone.utc).isoformat(), test_features=features,
                  commands={action: command(features, action) for action in ("list", "ignored", "run")})
    print(f"Rustdoc verification attempt: {attempt}", flush=True)
    try:
        record["source_revision"] = checked(["git", "rev-parse", "HEAD"], attempt, "revision", environment).strip()
        record["worktree_dirty"] = bool(checked(["git", "status", "--porcelain"], attempt, "worktree", environment).strip())
        snapshot = source_snapshot(attempt, environment)
        write_json(attempt / "source-snapshot.json", snapshot)
        record["inputs"] = inputs(ROOT)
        record["rustc"] = checked(["rustc", "-vV"], attempt, "rustc-version", environment).strip()
        record["rustdoc"] = checked(["rustdoc", "--version"], attempt, "rustdoc-version", environment).strip()
        pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        if f"release: {pin}" not in record["rustc"].splitlines() or not record["rustdoc"].startswith(f"rustdoc {pin} "):
            raise Blocked("Rustdoc verification requires the pinned compiler and rustdoc")
        record["target"] = next(line.removeprefix("host: ") for line in record["rustc"].splitlines() if line.startswith("host: "))
        record["platform"] = TARGETS[record["target"]]
        listings = {}
        for action in ("list", "ignored"):
            listings[action] = checked(record["commands"][action], attempt, action, environment, timeout=1800)
            (attempt / f"{action}.stdout").write_text(listings[action], encoding="utf-8")
        expected = inventory(listings["list"], listings["ignored"], json.loads((ROOT / BINDINGS).read_text()))
        code, output, timed_out = run(record["commands"]["run"], cwd=ROOT, env=environment,
                                     log=attempt / "run.log", timeout=1800, separate_stdout=True)
        (attempt / "run.stdout").write_text(output, encoding="utf-8")
        record.update(exit_code=code, timed_out=timed_out)
        record["tests"], record["status"], record["parse_error"] = evaluate(output, expected, code, timed_out)
        record["evidence"] = {name: sha256(attempt / name) for name in (
            "list.stdout", "ignored.stdout", "run.stdout", "run.log", "source-snapshot.json")}
        if source_snapshot(attempt, environment) != snapshot:
            raise RuntimeError("Source files changed during verification")
    except (OSError, ValueError, RuntimeError, KeyError, StopIteration, KeyboardInterrupt, Blocked) as error:
        record.update(status="FAIL", reason=str(error) or "Rustdoc verification interrupted", tests=[])
    write_json(attempt / "receipt.json", record)
    print(f"{record['status']}: {attempt / 'receipt.json'} ({len(record['tests'])} observations)", flush=True)
    if record.get("reason") or record.get("parse_error"):
        print(record.get("reason") or record["parse_error"], file=sys.stderr)
    return 0 if record["status"] == "PASS" else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suites", nargs="+", choices=[*SUITES, "python", "doctest"])
    parser.add_argument("--binary", help="Absolute path to an independently built product")
    parser.add_argument("--sha256")
    parser.add_argument("--version")
    parser.add_argument("--target")
    parser.add_argument("--product-features", choices=("default", "telemetry"))
    parser.add_argument("--product-profile", choices=("dev", "release"),
                        help="Product build profile (default dev); required for an external binary")
    parser.add_argument("--test-features", choices=TEST_FEATURES, default="all",
                        help="Workspace test graph; independent of the separately built product")
    parser.add_argument("--output", type=Path, default=ROOT / "target/verification")
    args = parser.parse_args(argv)
    if len(set(args.suites)) != len(args.suites):
        parser.error("A suite may be selected only once per attempt")
    if "python" in args.suites and (args.suites != ["python"] or any((args.binary, args.sha256,
            args.version, args.target, args.product_features, args.product_profile)) or args.test_features != "all"):
        parser.error("Python is an independent tooling run; select it separately without product options")
    if "doctest" in args.suites and (args.suites != ["doctest"] or any((args.binary, args.sha256,
            args.version, args.target, args.product_features, args.product_profile))):
        parser.error("Doctests run separately with a workspace test graph, without product options")
    args.output.mkdir(parents=True, exist_ok=True)
    attempt = args.output.resolve() / str(uuid.uuid4())
    attempt.mkdir(mode=0o700)
    environment = {key: value for key, value in os.environ.items() if not key.startswith("NEXTEST_")}
    environment.update(CARGO_TERM_COLOR="never", NO_COLOR="1")
    if args.suites == ["python"]:
        return run_python(attempt, environment)
    if args.suites == ["doctest"]:
        return run_doctests(attempt, environment, args.test_features)
    receipt = {"schema": 1, "kind": "test-suite-run", "status": "NOT_RUN",
               "created_at": datetime.now(timezone.utc).isoformat(), "suites": [],
               "requested_suites": args.suites, "test_features": args.test_features,
               "requested_product_features": args.product_features or ("telemetry" if any(SUITES[s].telemetry for s in args.suites) else "default"),
               "requested_product_profile": args.product_profile or "dev", "platform": platform.platform(),
               "architecture": platform.machine(), "qualification": False}
    print(f"Verification attempt: {attempt}", flush=True)
    try:
        receipt["source_revision"] = checked(["git", "rev-parse", "HEAD"], attempt, "revision", environment).strip()
        changes = checked(["git", "status", "--porcelain"], attempt, "worktree", environment)
        receipt["worktree_dirty"] = bool(changes.strip())
        snapshot = source_snapshot(attempt, environment)
        write_json(attempt / "source-snapshot.json", snapshot)
        receipt["source_snapshot_sha256"] = sha256(attempt / "source-snapshot.json")
        receipt["inputs"] = {name: sha256(ROOT / name) for name in (
            "Cargo.lock", "rust-toolchain.toml", ".config/nextest.toml",
            "scripts/verification/suites.py", "scripts/verify.py")}
        tool = checked(["cargo", "nextest", "--version"], attempt, "nextest-version", environment)
        if not tool.startswith(f"cargo-nextest {NEXTEST_VERSION} "):
            raise Blocked(f"Install cargo-nextest {NEXTEST_VERSION}; other tool versions are not qualified")
        receipt["nextest"] = tool.strip()
        rustc = checked(["rustc", "-vV"], attempt, "rustc-version", environment)
        receipt["rustc"] = rustc.strip()
        host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
        artifact = product(args, attempt, environment, host)
        receipt["artifact"] = artifact
        for name in args.suites:
            result = {"suite": name, "status": "NOT_RUN", "tests": [], "attempt": 1}
            try:
                run_suite(name, SUITES[name], attempt, environment, artifact, result, args.test_features)
            except Blocked as error:
                result.update(status="BLOCKED", reason=str(error))
            except (OSError, ValueError, RuntimeError, KeyError) as error:
                result.update(status="FAIL", reason=str(error))
            receipt["suites"].append(result)
            print(f"{name}: {result['status']} ({len(result['tests'])} test outcomes)", flush=True)
            if "reason" in result:
                print(result["reason"], file=sys.stderr)
            write_json(attempt / "receipt.json", receipt)
        if sha256(artifact["path"]) != artifact["sha256"]:
            raise RuntimeError("Product artifact changed during verification")
        if source_snapshot(attempt, environment) != snapshot:
            raise RuntimeError("Source files changed during verification")
        receipt["status"] = aggregate_status(receipt["suites"])
    except KeyboardInterrupt:
        receipt.update(status="FAIL", reason="Verification interrupted; no subsequent suites started")
    except Blocked as error:
        receipt.update(status="BLOCKED", reason=str(error))
    except (OSError, ValueError, RuntimeError, KeyError, StopIteration) as error:
        receipt.update(status="FAIL", reason=str(error))
    finished = {row["suite"] for row in receipt["suites"]}
    receipt["suites"].extend({"suite": name, "status": "NOT_RUN", "tests": []}
                             for name in args.suites if name not in finished)
    write_json(attempt / "receipt.json", receipt)
    print(f"{receipt['status']}: {attempt / 'receipt.json'}", flush=True)
    if "reason" in receipt:
        print(receipt["reason"], file=sys.stderr)
    return 0 if receipt["status"] in {"PASS", "NOT_APPLICABLE"} else 2 if receipt["status"] == "BLOCKED" else 1


if __name__ == "__main__":
    sys.exit(main())
