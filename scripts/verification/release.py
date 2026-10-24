"""Build-to-package evidence for the five shipped native archives."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tomllib

from .evidence import sha256, write_json
from .receipts import check_suite_receipt
from .release_inputs import release_version, stamped_sources

TARGETS = {
    "x86_64-unknown-linux-gnu": ("tar.gz", "empack"),
    "aarch64-unknown-linux-gnu": ("tar.gz", "empack"),
    "x86_64-apple-darwin": ("tar.gz", "empack"),
    "aarch64-apple-darwin": ("tar.gz", "empack"),
    "x86_64-pc-windows-msvc": ("zip", "empack.exe"),
}


def text_hash(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def archive_name(version, target):
    release_version(version)
    return f"empack-{version}-{target}.{TARGETS[target][0]}"


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def check_inputs(inputs, root, version, revision):
    stamped, packages = stamped_sources(root, version)
    if (inputs.get("schema") != 1 or inputs.get("kind") != "release-build-inputs"
            or inputs.get("source_revision") != revision
            or inputs.get("source_tree") != git(root, "rev-parse", "HEAD^{tree}")
            or inputs.get("version") != version or inputs.get("workspace_packages") != packages
            or inputs.get("build_date") != git(root, "show", "-s", "--format=%cI", revision)
            or inputs.get("stamped") != {name: text_hash(text) for name, text in stamped.items()}
            or inputs.get("toolchain_text_sha256") != text_hash((root / "rust-toolchain.toml").read_text(encoding="utf-8"))):
        raise ValueError("Build inputs differ from the reviewed release source and version")


def record_build(root, inputs_path, archive, target, builder, output):
    inputs = json.loads(inputs_path.read_text(encoding="utf-8"))
    version, revision = inputs["version"], inputs["source_revision"]
    if git(root, "rev-parse", "HEAD") != revision:
        raise ValueError("Build checkout differs from the prepared source")
    check_inputs(inputs, root, version, revision)
    # A build must not alter the prepared manifest, dependency lock or other
    # tracked files. Archive/target outputs are not additional source inputs.
    changed = set(git(root, "diff", "HEAD", "--name-only").splitlines())
    if changed - {"Cargo.toml", "Cargo.lock"} or any(
            sha256(root / name) != digest for name, digest in inputs["stamped"].items()):
        raise ValueError("Release source inputs changed during compilation")
    compiler = subprocess.check_output(["rustc", "-vV"], cwd=root, text=True).strip()
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    if f"release: {channel}" not in compiler.splitlines():
        raise ValueError("Build compiler differs from the repository toolchain")
    if builder == "cargo":
        host = target
    elif builder == "cross" and target == "aarch64-unknown-linux-gnu":
        host = "x86_64-unknown-linux-gnu"
    else:
        raise ValueError("Unreviewed builder and target combination")
    if f"host: {host}" not in compiler.splitlines():
        raise ValueError("Build host differs from the selected builder/target")
    binary = root / "target" / target / "release" / TARGETS[target][1]
    if archive.name != archive_name(version, target) or archive.is_symlink() or not archive.is_file():
        raise ValueError("Build archive does not match its target/version")
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("Build did not produce the expected regular executable")
    record = {"schema": 1, "kind": "release-archive-build", "source_revision": revision,
              "version": version, "target": target, "features": "default", "profile": "release",
              "inputs": inputs, "compiler": compiler,
              "command": [builder, "build", "--locked", "--release", "-p", "empack", "--target", target],
              "archive": {"name": archive.name, "sha256": sha256(archive), "length": archive.stat().st_size},
              "executable": {"sha256": sha256(binary), "length": binary.stat().st_size}}
    output.mkdir(parents=True, exist_ok=False)
    staged = output / archive.name
    shutil.copyfile(archive, staged)
    if sha256(staged) != record["archive"]["sha256"] or sha256(binary) != record["executable"]["sha256"]:
        raise ValueError("Build bytes changed while recording the archive")
    write_json(output / "build.json", record)
    return record


def check_target(root, build_directory, execution_directory, version, target, revision):
    build = json.loads((build_directory / "build.json").read_text(encoding="utf-8"))
    if (build.get("schema") != 1 or build.get("kind") != "release-archive-build"
            or build.get("source_revision") != revision or build.get("version") != version
            or build.get("target") != target or build.get("features") != "default"
            or build.get("profile") != "release"):
        raise ValueError(f"Wrong release build identity: {target}")
    check_inputs(build["inputs"], root, version, revision)
    command = build["command"]
    allowed = [["cargo", "build", "--locked", "--release", "-p", "empack", "--target", target]]
    if target == "aarch64-unknown-linux-gnu":
        allowed.append(["cross", *allowed[0][1:]])
    if command not in allowed:
        raise ValueError(f"Unreviewed production build command: {target}")
    compiler = build["compiler"].splitlines()
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    host = "x86_64-unknown-linux-gnu" if command[0] == "cross" else target
    if f"release: {channel}" not in compiler or f"host: {host}" not in compiler:
        raise ValueError(f"Unreviewed build compiler: {target}")
    expected_name = archive_name(version, target)
    archive = build_directory / expected_name
    if (set(path.name for path in build_directory.iterdir()) != {"build.json", expected_name}
            or build["archive"]["name"] != expected_name or archive.is_symlink()
            or sha256(archive) != build["archive"]["sha256"]
            or archive.stat().st_size != build["archive"]["length"]):
        raise ValueError(f"Release archive bytes changed: {target}")
    packages = list(execution_directory.glob("*/package.json"))
    if len(packages) != 1:
        raise ValueError(f"Expected one archive execution attempt: {target}")
    path = packages[0]
    package = json.loads(path.read_text(encoding="utf-8"))
    if (package.get("schema") != 1 or package.get("kind") != "extracted-release-archive"
            or package.get("status") != "PASS" or package.get("target") != target
            or package.get("version") != version or package.get("features") != "default"
            or package.get("profile") != "release"
            or package.get("test_source_revision") != revision or package.get("test_worktree_dirty") is not False
            or package.get("archive") != {"name": expected_name, "sha256": build["archive"]["sha256"]}
            or package.get("executable") != build["executable"]):
        raise ValueError(f"Missing or unrelated archive execution: {target}")
    suite = check_suite_receipt(path.parent / "suite", ["cli"], target, revision, "default", version, "release")
    actual_receipt = next((path.parent / "suite").glob("*/receipt.json"))
    if (package.get("suite_receipt") != actual_receipt.relative_to(path.parent).as_posix()
            or package.get("suite_receipt_sha256") != suite["receipt_sha256"]
            or package.get("test_source_snapshot_sha256") != suite["source_snapshot_sha256"]
            or build["executable"]["sha256"] != suite["artifact_sha256"]):
        raise ValueError(f"Archive receipt does not bind the executed product: {target}")
    return {"build_sha256": sha256(build_directory / "build.json"),
            "package_sha256": sha256(path), "archive": build["archive"],
            "executable": build["executable"], "suite": suite}


def admit_archives(root, artifacts, version, revision, output):
    if git(root, "rev-parse", "HEAD") != revision:
        raise ValueError("Archive gate checkout differs from the release candidate")
    if git(root, "status", "--porcelain", "--untracked-files=normal"):
        raise ValueError("Archive gate requires a clean test source checkout")
    expected = {prefix + target for target in TARGETS for prefix in ("release-build-", "release-test-")}
    if set(path.name for path in artifacts.iterdir()) != expected:
        raise ValueError("Missing or unexpected target evidence in archive gate")
    records = [check_target(root, artifacts / f"release-build-{target}", artifacts / f"release-test-{target}",
                            version, target, revision) for target in TARGETS]
    # Validate every target before creating any publication payload. Copy the
    # checked archives, never rebuild them after qualification.
    output.mkdir(parents=True, exist_ok=False)
    for target, record in zip(TARGETS, records, strict=True):
        destination = output / record["archive"]["name"]
        shutil.copyfile(artifacts / f"release-build-{target}" / destination.name, destination)
        if sha256(destination) != record["archive"]["sha256"]:
            raise ValueError("Archive changed while staging publication payload")
    (output / "SHA256SUMS").write_text("".join(
        f"{record['archive']['sha256']}  {record['archive']['name']}\n" for record in records), encoding="utf-8", newline="\n")
    result = {"schema": 1, "kind": "release-archive-gate", "status": "PASS", "qualification": False,
              "source_revision": revision, "version": version, "targets": records,
              "scope": "all five final archives and offline CLI suites; full candidate acceptance remains separate"}
    write_json(output / "archive-evidence.json", result)
    return result


def check_payload(directory, version, revision, evidence_sha256):
    path = directory / "archive-evidence.json"
    if sha256(path) != evidence_sha256:
        raise ValueError("Publication evidence differs from the archive gate output")
    record = json.loads(path.read_text(encoding="utf-8"))
    if (record.get("schema") != 1 or record.get("kind") != "release-archive-gate"
            or record.get("status") != "PASS" or record.get("source_revision") != revision
            or record.get("version") != version or len(record.get("targets", [])) != len(TARGETS)):
        raise ValueError("Publication payload belongs to another or incomplete candidate")
    expected = {archive_name(version, target) for target in TARGETS} | {"SHA256SUMS", "archive-evidence.json"}
    if set(path.name for path in directory.iterdir()) != expected:
        raise ValueError("Publication payload has missing or unexpected files")
    checksums = []
    for target, entry in zip(TARGETS, record["targets"], strict=True):
        name = archive_name(version, target)
        archive = directory / name
        if (entry["suite"]["target"] != target or entry["archive"]["name"] != name
                or archive.is_symlink() or sha256(archive) != entry["archive"]["sha256"]
                or archive.stat().st_size != entry["archive"]["length"]):
            raise ValueError(f"Publication archive differs from the tested bytes: {target}")
        checksums.append(f"{entry['archive']['sha256']}  {name}\n")
    if (directory / "SHA256SUMS").read_text(encoding="utf-8") != "".join(checksums):
        raise ValueError("Publication checksum list differs from checked archives")
    return record
