"""Check the units Cargo actually built, independently of declared CLI features."""
import hashlib
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import tomllib

from .evidence import sha256, write_json
from .process import run
from .compiler import compiler_environment

EVIDENCE_FILES = {"product-build-messages.jsonl", "product-metadata.json", "product-build-audit.json"}
COMPILATION_FILES = EVIDENCE_FILES | {"compile.json", "build.stdout", "metadata.stdout"}


def compiler_stdout(output, builder, channel):
    """Remove only the observed pinned Cross/Rustup prelude, retaining strict JSON.

    Native Cargo has no launcher prelude. Unknown output and anything after the
    first JSON line still reach the strict decoder rather than being discarded.
    """
    if builder != "cross":
        return output
    banner = re.compile(r"  " + re.escape(channel) + r"-x86_64-unknown-linux-gnu unchanged - rustc "
                        + re.escape(channel) + r" \([0-9a-f]{7,40} \d{4}-\d{2}-\d{2}\)")
    lines = output.splitlines(keepends=True)
    for index, line in enumerate(lines):
        if line.startswith("{"):
            return "".join(lines[index:])
        if line.strip() and not banner.fullmatch(line.rstrip("\r\n")):
            raise ValueError("Unexpected Cross launcher output before compiler JSON")
    raise ValueError("Cross did not supply compiler JSON")


def build_command(target, builder="cargo", features="default"):
    if builder not in {"cargo", "cross"} or features not in {"default", "telemetry"}:
        raise ValueError("Unsupported product builder or features")
    return [builder, "build", "--locked", "--release", "-p", "empack", "--target", target,
            *(["--features", "telemetry"] if features == "telemetry" else []), "--message-format=json"]


def metadata_command(target, builder="cargo"):
    return [builder, "metadata", "--locked", "--no-deps", "--format-version", "1",
            *(["--target", target] if builder == "cross" else [])]


def retain_build(directory, messages, metadata, features, executable, digest):
    directory = Path(directory)
    artifact = {"path": str(executable), "features": features, "sha256": digest}
    (directory / "product-build-messages.jsonl").write_text(messages, encoding="utf-8")
    write_json(directory / "product-metadata.json", metadata)
    write_json(directory / "product-build-audit.json",
               inspect_build(messages, metadata, features, executable, digest))
    artifact["build_audit_sha256"] = sha256(directory / "product-build-audit.json")
    return artifact


def compile_product(root, target, builder, directory, environment, features="default"):
    """Compile once, retaining metadata from the same native/container namespace.

    Cross metadata runs inside its selected build container. Its /target mapping
    is not interpreted as a path on the host admitting this evidence later.
    """
    root, directory = Path(root).resolve(), Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    owned_names = COMPILATION_FILES | {"build.log", "metadata.log", "builder-version.log",
                                        "source-changes.log", "source-untracked.log", "source-revision.log"}
    if any((directory / name).exists() or (directory / name).is_symlink() for name in owned_names):
        raise ValueError("Product build output already contains an attempt; select a fresh directory")
    target_dir = Path(environment.get("CARGO_TARGET_DIR", root / "target"))
    target_dir = (root / target_dir).resolve() if not target_dir.is_absolute() else target_dir.resolve()
    environment = compiler_environment(dict(environment, CARGO_TARGET_DIR=str(target_dir), CARGO_TERM_COLOR="never"))

    def checked(command, name, timeout=120):
        code, output, timed_out = run(command, cwd=root, env=environment,
                                     log=directory / (name + ".log"), timeout=timeout, separate_stdout=True)
        if code or timed_out:
            raise RuntimeError(f"{name} failed (exit {code}, timeout={timed_out}); see {directory / (name + '.log')}")
        (directory / (name + ".stdout")).write_text(output, encoding="utf-8")
        return output

    def source():
        if (set(checked(["git", "diff", "HEAD", "--name-only"], "source-changes").splitlines())
                - {"Cargo.toml", "Cargo.lock"}
                or checked(["git", "ls-files", "--others", "--exclude-standard"], "source-untracked").strip()):
            raise ValueError("Product compilation requires reviewed source with only release stamping changes")
        return {"revision": checked(["git", "rev-parse", "HEAD"], "source-revision").strip(),
                "stamped": {name: sha256(root / name) for name in ("Cargo.toml", "Cargo.lock")}}

    before = source()
    version = checked([builder, "--version"], "builder-version").strip()
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    if builder == "cargo" and not version.startswith(f"cargo {channel} "):
        raise ValueError("Product compilation requires the pinned Cargo version")
    if builder == "cross" and version.splitlines()[0] != "cross 0.2.5":
        raise ValueError("Cross build evidence requires pinned cross 0.2.5")
    command = build_command(target, builder, features)
    messages = compiler_stdout(checked(command, "build", timeout=2400), builder, channel)
    metadata_args = metadata_command(target, builder)
    metadata = json.loads(compiler_stdout(checked(metadata_args, "metadata"), builder, channel))
    filename = "empack.exe" if target.endswith("windows-msvc") else "empack"
    binary = target_dir / target / "release" / filename
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("Product compilation did not produce its regular host backing file")
    path_type = PureWindowsPath if target.endswith("windows-msvc") else PurePosixPath
    output_path = path_type(metadata["target_directory"]) / target / "release" / filename
    if not output_path.is_absolute():
        raise ValueError("Cargo metadata did not identify an absolute target directory")
    artifact = retain_build(directory, messages, metadata, features, output_path, sha256(binary))
    if source() != before:
        raise ValueError("Source changed during product compilation")
    record = {"schema": 1, "kind": "compiled-product", "source": before, "target": target,
              "features": features, "command": command, "metadata_command": metadata_args,
              "builder_version": version, "artifact": artifact}
    write_json(directory / "compile.json", record)
    return record


def inspect_compilation(directory, target, builder, features, revision, stamped, channel):
    directory = Path(directory)
    path = directory / "compile.json"
    if path.is_symlink() or not path.is_file():
        raise ValueError("Missing or redirected product compilation record")
    record = json.loads(path.read_text(encoding="utf-8"))
    if (record.get("schema") != 1 or record.get("kind") != "compiled-product"
            or record.get("target") != target or record.get("features") != features
            or record.get("source") != {"revision": revision, "stamped": stamped}
            or record.get("command") != build_command(target, builder, features)
            or record.get("metadata_command") != metadata_command(target, builder)
            or record.get("artifact", {}).get("features") != features):
        raise ValueError("Product compilation differs from candidate build inputs")
    version = record.get("builder_version", "")
    if (builder == "cargo" and not version.startswith(f"cargo {channel} ")
            or builder == "cross" and version.splitlines()[:1] != ["cross 0.2.5"]):
        raise ValueError("Unreviewed product builder version")
    for name in ("build.stdout", "metadata.stdout"):
        if (directory / name).is_symlink() or not (directory / name).is_file():
            raise ValueError("Missing or redirected compiler stdout")
    emitted = compiler_stdout((directory / "build.stdout").read_text(encoding="utf-8"), builder, channel)
    metadata = json.loads(compiler_stdout((directory / "metadata.stdout").read_text(encoding="utf-8"), builder, channel))
    if (emitted != (directory / "product-build-messages.jsonl").read_text(encoding="utf-8")
            or metadata != json.loads((directory / "product-metadata.json").read_text(encoding="utf-8"))):
        raise ValueError("Compiled product differs from captured compiler stdout")
    inspect_retained_build(directory, record["artifact"])
    return record


def inspect_build(messages, metadata, features, executable, digest):
    """Metadata names opaque IDs; only compiler artifacts establish enabled features.

    Cargo's workspace resolve graph may unify unrelated test features. Never use
    that graph as evidence about this separately compiled product.
    """
    if features not in {"default", "telemetry"}:
        raise ValueError("Unsupported ordinary product features")
    packages = {p["id"]: p["name"] for p in metadata["packages"]}
    required = {"empack", "empack-core", "empack-lib"}
    if not required <= set(packages.values()):
        raise ValueError("Product metadata lacks required workspace packages")
    units, finished, binaries = [], False, []
    for line in messages.splitlines():
        if not line.strip():
            continue
        event = json.loads(line)
        if finished:
            raise ValueError("Cargo output continues after build-finished")
        reason = event["reason"]
        if reason == "build-finished":
            if event.get("success") is not True:
                raise ValueError("Cargo did not finish successfully")
            finished = True
        elif reason == "compiler-artifact":
            name = packages.get(event["package_id"])
            enabled, kind = event["features"], event["target"]["kind"]
            if (not isinstance(enabled, list) or any(not isinstance(f, str) for f in enabled)
                    or len(set(enabled)) != len(enabled)):
                raise ValueError("Invalid compiler feature list")
            if event["profile"].get("test") is not False or set(kind) & {"test", "bench", "example"}:
                raise ValueError("Test/example unit in ordinary product build")
            if name == "empack-tests" or (name == "empack-lib" and "test-utils" in enabled):
                raise ValueError("Test-only package or feature in ordinary product build")
            if name == "empack":
                expected = [] if features == "default" else ["telemetry"]
                if sorted(enabled) != expected:
                    raise ValueError("Compiled empack features differ from requested product")
                if kind == ["bin"] and event["target"]["name"] == "empack":
                    # The producer records a canonical absolute path. Admission
                    # may run on another OS without that host's symlink map.
                    if event.get("executable") != str(executable):
                        raise ValueError("Cargo executable differs from selected product")
                    binaries.append(event["executable"])
            if name == "empack-lib" and ("telemetry" in enabled) != (features == "telemetry"):
                raise ValueError("Compiled library telemetry differs from requested product")
            units.append({"package_id": event["package_id"], "workspace_package": name,
                          "target": event["target"]["name"], "kind": kind,
                          "features": sorted(enabled), "profile": event["profile"]})
        elif reason not in {"compiler-message", "build-script-executed"}:
            raise ValueError(f"Unexpected Cargo build message: {reason}")
    if not finished or len(binaries) != 1 or not required <= {u["workspace_package"] for u in units}:
        raise ValueError("Incomplete Cargo product build evidence")
    return {"schema": 1, "features": features, "executable_sha256": digest, "units": units,
            "messages_sha256": hashlib.sha256(messages.encode("utf-8")).hexdigest(),
            "metadata_sha256": hashlib.sha256(json.dumps(metadata, sort_keys=True).encode("utf-8")).hexdigest()}


def inspect_retained_build(directory, artifact):
    """Recompute the audit from contained build output; do not trust a PASS label."""
    directory = Path(directory)
    paths = [directory / name for name in
             ("product-build-messages.jsonl", "product-metadata.json", "product-build-audit.json")]
    if any(p.is_symlink() or not p.is_file() for p in paths):
        raise ValueError("Missing or redirected ordinary product build evidence")
    messages, metadata, audit = paths
    expected = inspect_build(messages.read_text(encoding="utf-8"),
                             json.loads(metadata.read_text(encoding="utf-8")),
                             artifact["features"], artifact["path"], artifact["sha256"])
    if (json.loads(audit.read_text(encoding="utf-8")) != expected
            or expected["executable_sha256"] != artifact["sha256"]
            or artifact.get("build_audit_sha256") != sha256(audit)):
        raise ValueError("Ordinary product build audit differs from retained inputs")
    return expected
