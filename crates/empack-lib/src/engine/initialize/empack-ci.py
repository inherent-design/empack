#!/usr/bin/env python3
"""Build authored profiles and stage exact GitHub release assets."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile

REPOSITORY = "inherent-design/empack"


def run(*args, capture=False):
    return subprocess.run(args, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def install(version):
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("EMPACK_VERSION must name an exact release tag")
    name = f"empack-{version[1:]}-x86_64-unknown-linux-gnu.tar.gz"
    release = json.loads(run("gh", "api", f"repos/{REPOSITORY}/releases/tags/{version}", capture=True))
    assets = [a for a in release["assets"] if a["name"] == name]
    if len(assets) != 1 or not re.fullmatch(r"sha256:[0-9a-f]{64}", assets[0].get("digest") or ""):
        raise ValueError("Release executable has no unique SHA-256 asset evidence")
    with tempfile.TemporaryDirectory(prefix="empack-download-") as temporary:
        root = Path(temporary)
        run("gh", "release", "download", version, "--repo", REPOSITORY,
            "--pattern", name, "--dir", str(root))
        archive = root / name
        if digest(archive) != assets[0]["digest"][7:]:
            raise ValueError("Released executable checksum mismatch")
        destination = Path(os.environ["RUNNER_TEMP"]) / "empack-bin"
        destination.mkdir(exist_ok=False)
        with tarfile.open(archive, "r:gz") as reader:
            members = [m for m in reader.getmembers() if m.name in ("empack", "./empack")]
            if len(members) != 1 or not members[0].isfile() or members[0].size > 512 << 20:
                raise ValueError("Release archive has no valid empack executable")
            with reader.extractfile(members[0]) as source, (destination / "empack").open("xb") as target:
                shutil.copyfileobj(source, target)
        (destination / "empack").chmod(0o755)
        run(str(destination / "empack"), "--version")
        with Path(os.environ["GITHUB_PATH"]).open("a") as output:
            output.write(f"{destination}\n")


def selection():
    args = ["--optional-defaults"]
    for name in os.environ.get("EMPACK_PROFILES", "").splitlines():
        if name.strip():
            args += ["--profile", name.strip()]
    report = json.loads(run("empack", "build", "--explain", "--json", *args, capture=True))
    if report.get("schema") != 1 or report.get("lock") != "current":
        raise ValueError("Build requires a schema-1 explanation with a current lock; run empack sync locally")
    if not report.get("profiles"):
        raise ValueError("No build profiles selected")
    return report


def artifact(root, output):
    relative = Path(output)
    if relative.is_absolute() or not relative.parts or relative.parts[0] != "dist" or ".." in relative.parts:
        raise ValueError(f"Artifact is outside dist: {output}")
    candidate = root
    for part in relative.parts:
        candidate /= part
        if candidate.is_symlink():
            raise ValueError(f"Artifact uses a symlink: {output}")
    if not candidate.is_file():
        raise ValueError(f"Build did not produce a regular artifact: {output}")
    return candidate


def stage(root, report, destination):
    selected = []
    names = {"sha256sums"}
    for profile in report["profiles"]:
        source = artifact(root, profile["output"])
        name = re.sub(r"[^A-Za-z0-9._-]+", "-", source.name).strip(".-")
        if not name or name.lower() in names:
            raise ValueError(f"Published filename is empty or collides: {source.name}")
        names.add(name.lower())
        selected.append((source, name))
    destination.mkdir(exist_ok=False)
    checksums = []
    for source, name in selected:
        target = destination / name
        shutil.copyfile(source, target)
        checksums.append(f"{digest(target)}  {name}\n")
    (destination / "SHA256SUMS").write_text("".join(checksums), encoding="utf-8")


def publish(destination, tag):
    assets = [str(p) for p in sorted(destination.iterdir())]
    options = ["--prerelease"] if "-" in tag else []
    run("gh", "release", "create", tag, *assets, "--verify-tag", "--generate-notes", "--draft", *options)
    run("gh", "release", "edit", tag, "--draft=false", *options)


def build(release=False):
    report = selection()
    tag = os.environ.get("GITHUB_REF_NAME", "")
    if release and tag != "v" + report["version"]:
        raise ValueError(f"Release tag {tag!r} must match authored version v{report['version']}")
    args = ["empack", "--yes", "build", "--optional-defaults"]
    for profile in report["profiles"]:
        args += ["--profile", profile["name"]]
    run(*args)
    destination = Path(os.environ["RUNNER_TEMP"]) / "empack-artifacts"
    stage(Path.cwd(), report, destination)
    if release:
        publish(destination, tag)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["install", "validate", "release"])
    args = parser.parse_args()
    if args.action == "install":
        install(os.environ["EMPACK_VERSION"])
    else:
        build(release=args.action == "release")


if __name__ == "__main__":
    main()
