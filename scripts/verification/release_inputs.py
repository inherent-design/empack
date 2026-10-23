"""Deterministic workspace stamping for immutable release build inputs."""
import copy
import hashlib
from pathlib import Path
import re
import subprocess
import tomllib

from .evidence import sha256


def release_version(value):
    number = r"(?:0|[1-9][0-9]*)"
    if not re.fullmatch(rf"{number}\.{number}\.{number}(?:-(?:alpha|beta|rc)(?:\.{number})?)?", value):
        raise ValueError("Expected a release version with an optional alpha, beta or rc suffix")
    return value


def stamped_sources(root, version):
    version = release_version(version)
    manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
    lock = (root / "Cargo.lock").read_text(encoding="utf-8")
    original_manifest = tomllib.loads(manifest)
    original_lock = tomllib.loads(lock)
    workspace = original_manifest["workspace"]
    if workspace["metadata"]["empack"]["release-target"] != version:
        raise ValueError("Release version differs from the reviewed workspace release target")
    names = set()
    for member in workspace["members"]:
        path = (root / member).resolve()
        if not path.is_relative_to(root.resolve()):
            raise ValueError("Workspace member is outside the release source")
        package = tomllib.loads((path / "Cargo.toml").read_text(encoding="utf-8"))["package"]
        if package["version"] != {"workspace": True} or package["name"] in names:
            raise ValueError("Release members must inherit one workspace version and have unique names")
        names.add(package["name"])
    if not names:
        raise ValueError("Release workspace has no packages")
    expected_manifest = copy.deepcopy(original_manifest)
    expected_manifest["workspace"]["package"]["version"] = version
    section = re.search(r"(?ms)^\[workspace\.package\]\s*\n.*?(?=^\[|\Z)", manifest)
    if not section:
        raise ValueError("Workspace package section is missing")
    stamped, count = re.subn(r'(?m)^version[ \t]*=[ \t]*"[^"]*"[ \t]*$',
                            f'version = "{version}"', section[0])
    if count != 1:
        raise ValueError("Workspace version needs a single canonical assignment")
    manifest = manifest[:section.start()] + stamped + manifest[section.end():]

    old = workspace["package"]["version"]
    expected_lock = copy.deepcopy(original_lock)
    selected = set()
    for package in expected_lock["package"]:
        if package["name"] in names:
            if package.get("source") or package["name"] in selected or package["version"] != old:
                raise ValueError("Workspace lock entries are ambiguous or stale")
            selected.add(package["name"])
            package["version"] = version
        # Current workspace dependency identities are unqualified names. Refuse a
        # future ambiguous/version-qualified shape instead of resolving it silently.
        if any(dependency.split(" ")[0] in names and " " in dependency
               for dependency in package.get("dependencies", [])):
            raise ValueError("Version-qualified workspace dependencies need an explicit lock migration")
    if selected != names:
        raise ValueError("Lockfile is missing a workspace package")
    blocks = re.split(r"(?m)(?=^\[\[package\]\]\s*$)", lock)
    for index, block in enumerate(blocks):
        values = tomllib.loads(block).get("package", [])
        if values and values[0]["name"] in names:
            blocks[index], count = re.subn(r'(?m)^version = "[^"]*"$', f'version = "{version}"', block)
            if count != 1:
                raise ValueError("Workspace lock version needs a single canonical assignment")
    lock = "".join(blocks)
    if tomllib.loads(manifest) != expected_manifest or tomllib.loads(lock) != expected_lock:
        raise ValueError("Release stamping changed inputs beyond workspace package versions")
    return {"Cargo.toml": manifest, "Cargo.lock": lock}, sorted(names)


def prepare(root, version):
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
    if git("status", "--porcelain", "--untracked-files=normal"):
        raise ValueError("Release preparation requires a clean source checkout")
    revision = git("rev-parse", "HEAD")
    tree = git("rev-parse", "HEAD^{tree}")
    changed, names = stamped_sources(root, version)
    before = {name: sha256(root / name) for name in changed}
    # All admission and semantic comparisons finish before either input is written.
    # This runs in a disposable release checkout; failure never publishes anything.
    for name, text in changed.items():
        (root / name).write_text(text, encoding="utf-8", newline="\n")
    return {"schema": 1, "kind": "release-build-inputs", "source_revision": revision,
            "source_tree": tree, "version": version, "workspace_packages": names,
            "build_date": git("show", "-s", "--format=%cI", revision),
            "before": before, "stamped": {name: sha256(root / name) for name in changed},
            "toolchain_text_sha256": hashlib.sha256(
                (root / "rust-toolchain.toml").read_text(encoding="utf-8").encode("utf-8")).hexdigest(),
            "scope": "workspace version stamping only; locked build and qualification still required"}

