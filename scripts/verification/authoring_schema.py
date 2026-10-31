"""Fixed editor/CLI conformance inputs and admission; no schema dependency here."""
import copy
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re
import stat
import tomllib

from .evidence import aggregate_outcomes, sha256
from .product_build import inspect_retained_build

ROOT = Path(__file__).resolve().parents[2]
TEST_ID = "schema::authoring::editor-cli-conformance"
BINDING = {"id": TEST_ID, "boundary": "X", "platforms": ["linux", "macos", "windows"]}
INPUTS = ("docs/schemas/empack.schema-5.json", "docs/schemas/empack.bindings.schema-1.json",
          "scripts/check-authoring-schema.py", "scripts/verification/authoring_schema.py",
          "scripts/verify.py", "scripts/verification/product_build.py",
          "tests/verification/requirements.json", "tests/verification/test-bindings.json",
          "Cargo.toml", "Cargo.lock", "rust-toolchain.toml")

BASE = {"schema": 5, "pack": {"id": "test", "name": "Test Pack", "version": "Autumn Update"},
        "runtime": {"minecraft": "1.21.1", "loader": "neoforge", "java": 21}}


def fixtures():
    cases = [(True, {})]
    for dependency in [
        "modrinth:readable-name",
        {"source": "modrinth:readable-name", "pin": "Version1"},
        {"source": "curseforge:123456", "releases": "any"},
        {"path": "vendor/mod.jar"},
        {"path": "vendor/texture.zip", "type": "resource-pack", "side": "client", "optional": True},
        {"source": "modrinth:Example1", "optional": {"choice": "shared-choice", "default": False}},
        {"source": "modrinth:Example1", "optional": {"choice": "shared-choice"}},
        {"url": "https://example.org/mod.jar", "sha256": "a" * 64, "mirrors": ["https://mirror.example.org/mod.jar"]},
        {"source": {"kind": "provider", "identity": {"provider": "modrinth", "project": "Example1"}}},
        {"source": {"kind": "search", "query": "renderer", "providers": ["modrinth"]}, "releases": "stable-only"},
        {"source": {"kind": "local-files", "members": {"primary": "vendor/mod.jar"}}, "placement": {"files": {"primary": [{"destination": "mods/mod.jar", "layer": "common", "environment": {"client": "required", "server": "required"}}]}}},
    ]:
        cases.append((True, {"dependencies": {"example": dependency}}))
    for dependency in [
        {"path": "mod.jar", "pin": "Version1"},
        {"path": "mod.jar", "releases": "any"},
        {"source": "modrinth:Example1", "pin": "Version1", "releases": "any"},
        {"source": "modrinth:Example1", "sha256": "a" * 64},
        {"source": "modrinth:Example1", "path": "mod.jar"},
        {"url": "https://example.org/mod.jar"},
        {"path": "mod.jar", "side": "client", "environment": {"client": "required", "server": "unsupported"}},
        {"source": "modrinth:Example1", "releases": "latest"},
        {"path": "mod.jar", "unknown": True},
        {"source": {"kind": "search", "query": "renderer", "providers": ["modrinth"]}, "sha256": "a" * 64},
        {"source": {"kind": "local-files", "members": {"primary": "vendor/mod.jar"}}, "sha256": "a" * 64},
    ]:
        cases.append((False, {"dependencies": {"example": dependency}}))
    for source in [{"url": "https://example.org/mod.jar"},
                   {"source": {"kind": "url", "downloads": ["https://example.org/mod.jar"]}}]:
        for policy, valid in [({}, False), ({"sha256": "a" * 64}, True),
                              ({"accept-observed": True}, True), ({"accept-observed": False}, False),
                              ({"accept-observed": True, "sha256": "a" * 64}, False)]:
            cases.append((valid, {"dependencies": {"example": dict(source, **policy)}}))
    for source in [{"path": "mod.jar"}, {"source": "modrinth:Example1"}]:
        cases.append((False, {"dependencies": {"example": dict(source, **{"accept-observed": True})}}))
    cases.extend([
        (True, {"build": {"default": ["client", "server"], "profiles": {"client": "prism", "server": {"target": "server", "format": "tar.gz"}}}}),
        (False, {"schema": 4}),
        (False, {"runtime": {"minecraft": "1.21.1", "loader": "neoforge", "java": 7}}),
        (False, {"build": {"profiles": {"client": {"target": "prism", "unknown": True}}}}),
    ])
    binding_cases = [
        (True, {"schema": 1, "bindings": {}}),
        (True, {"schema": 1, "bindings": {"example": {"placement": {"files": {"primary": [{"destination": "mods/menu.jar", "layer": "client", "environment": {"client": "required", "server": "unsupported"}}]}}}}}),
        (False, {"schema": 2, "bindings": {}}),
        (False, {"schema": 1, "bindings": {"example": {}}}),
        (False, {"schema": 1, "bindings": {"example": {"source": "modrinth:Other1"}}}),
    ]
    result = []
    for index, (expected, patch) in enumerate(cases):
        document = copy.deepcopy(BASE)
        document.update(patch)
        result.append(dict(id=f"author-{index:02}", expected=expected, author=document, bindings=None))
    document = copy.deepcopy(BASE)
    document["dependencies"] = {"example": "modrinth:Example1"}
    result.extend(dict(id=f"bindings-{index:02}", expected=expected, author=copy.deepcopy(document), bindings=binding)
                  for index, (expected, binding) in enumerate(binding_cases))
    return result


def input_hashes(root=ROOT):
    return {name: sha256(Path(root) / name) for name in INPUTS}


def workspace_snapshot(root):
    """Observe membership, bytes and attributes without following a created link."""
    result = {}
    for path in sorted(Path(root).rglob("*")):
        value = path.lstat()
        row = {"mode": stat.S_IMODE(value.st_mode)}
        if stat.S_ISLNK(value.st_mode):
            row.update(kind="symlink", target=str(path.readlink()))
        elif stat.S_ISDIR(value.st_mode):
            row.update(kind="directory")
        elif stat.S_ISREG(value.st_mode):
            row.update(kind="file", sha256=sha256(path))
        else:
            row.update(kind="special")
        result[path.relative_to(root).as_posix()] = row
    return result


def observation_results(observations):
    """Recompute results from exact fixtures, responses and whole-workspace observations."""
    expected = fixtures()
    if [row["id"] for row in observations] != [case["id"] for case in expected]:
        raise ValueError("Schema observations omit, repeat or reorder source fixtures")
    results = []
    for case, row in zip(expected, observations, strict=True):
        before, after = row["before"], row["after"]
        documents = {"project/empack.yml": case["author"]}
        if case["bindings"] is not None:
            documents["project/empack.bindings.yml"] = case["bindings"]
        if set(before) != {"home", "project", *documents}:
            raise ValueError("Schema observation did not start from the required isolated workspace")
        for name, entry in before.items():
            if type(entry["mode"]) is not int or not 0 <= entry["mode"] <= 0o7777:
                raise ValueError("Invalid workspace attributes")
            required = {"mode": entry["mode"], "kind": "directory"}
            if name in documents:
                data = json.dumps(documents[name]).encode("utf-8")
                required.update(kind="file", sha256=hashlib.sha256(data).hexdigest())
            if entry != required:
                raise ValueError("Schema observation has unrelated author inputs")
        if (type(row["schema_valid"]) is not bool or type(row["exit_code"]) is not int
                or type(row["timed_out"]) is not bool
                or any(not isinstance(row[key], str) for key in ("stdout", "stderr"))):
            raise ValueError("Malformed schema/CLI observation")
        valid = (row["schema_valid"] == case["expected"]
                 and (row["exit_code"] == 0) == case["expected"]
                 and not row["timed_out"] and before == after)
        if case["expected"] and row["exit_code"] == 0:
            try:
                response = json.loads(row["stdout"])
                valid &= isinstance(response, dict) and response.get("author_schema") == 5
            except json.JSONDecodeError:
                valid = False
        elif not case["expected"]:
            # A crash, signal or interruption is not an observed validation refusal.
            valid &= 0 < row["exit_code"] < 128 and bool(row["stderr"].strip())
        results.append({"id": case["id"], "status": "PASS" if valid else "FAIL"})
    return results


def inspect_schema_evidence(path, revision, root=ROOT):
    """Admit a fixed producer attempt, not its self-reported success alone."""
    path = Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 4 << 20:
        raise ValueError("Missing, redirected or oversized schema receipt")
    try:
        record = json.loads(path.read_text(encoding="utf-8"))
        raw = path.parent / "observations.json"
        artifact = record["artifact"]
        if (record["schema"] != 1 or record["kind"] != "authoring-schema-run"
                or record["source_revision"] != revision or record["worktree_dirty"] is not False
                or record["qualification"] is not False or record.get("reason")
                or record["inputs"] != input_hashes(root)
                or datetime.fromisoformat(record["created_at"]).tzinfo is None
                or artifact["features"] != "default" or artifact["profile"] != "dev"
                or artifact["provenance"] != "built by this runner from the ordinary product package"):
            raise ValueError("Unrelated or unstable schema attempt")
        targets = json.loads((Path(root) / "tests/verification/test-bindings.json").read_text())["targets"]
        package = tomllib.loads((Path(root) / "Cargo.toml").read_text())["workspace"]["package"]
        pin = tomllib.loads((Path(root) / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        if (targets.get(artifact["target"]) != record["platform"]
                or f"host: {artifact['target']}" not in record["rustc"].splitlines()
                or f"release: {pin}" not in record["rustc"].splitlines()
                or artifact["version"] != package["version"]
                or not re.fullmatch(r"[0-9a-f]{64}", artifact["sha256"])):
            raise ValueError("Schema product and execution host disagree")
        inspect_retained_build(path.parent, artifact)
        if raw.is_symlink() or not raw.is_file() or raw.stat().st_size > 4 << 20 or sha256(raw) != record["observations_sha256"]:
            raise ValueError("Missing or changed schema observations")
        probe = json.loads(raw.read_text(encoding="utf-8"))
        if (probe["schema"] != 1 or probe["kind"] != "authoring-schema-observations"
                or probe["artifact_sha256"] != artifact["sha256"]
                or probe["inputs"] != record["inputs"]
                or probe["dependencies"] != {"jsonschema": "4.23.0"}
                or not re.fullmatch(r"[0-9a-f]{64}", probe["interpreter_sha256"])
                or not probe["python_version"]):
            raise ValueError("Schema probe lacks product, source or dependency identity")
        results = observation_results(probe["fixtures"])
        status = aggregate_outcomes(row["status"] for row in results)
        if (type(record["exit_code"]) is not int or type(record["timed_out"]) is not bool
                or record["timed_out"] or record["exit_code"] != (0 if status == "PASS" else 1)
                or record["status"] != status):
            raise ValueError("Schema attempt disagrees with observed conformance")
        return dict(kind="authoring-schema", source_revision=revision, status=status,
                    platform=record["platform"], target=artifact["target"], features="default",
                    test_id=TEST_ID, artifact_sha256=artifact["sha256"], fixtures=results,
                    created_at=record["created_at"], receipt_sha256=sha256(path),
                    observations_sha256=sha256(raw), dependencies=probe["dependencies"],
                    interpreter_sha256=probe["interpreter_sha256"], python_version=probe["python_version"])
    except (KeyError, TypeError, AttributeError) as error:
        raise ValueError("Malformed schema conformance evidence") from error
