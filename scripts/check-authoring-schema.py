# /// script
# dependencies = ["jsonschema==4.23.0"]
# ///
"""Compare editor schema fixtures with the actual CLI. Run with uv and a built empack."""
import copy
import json
import pathlib
import subprocess
import sys
import tempfile

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = pathlib.Path(__file__).resolve().parents[1]
BASE = {"schema": 5, "pack": {"id": "test", "name": "Test Pack", "version": "Autumn Update"},
        "runtime": {"minecraft": "1.21.1", "loader": "neoforge", "java": 21}}


def main(binary):
    schema = json.loads((ROOT / "docs/schemas/empack.schema-5.json").read_text())
    Draft202012Validator.check_schema(schema)
    bindings_schema = json.loads((ROOT / "docs/schemas/empack.bindings.schema-1.json").read_text())
    Draft202012Validator.check_schema(bindings_schema)
    registry = Registry().with_resource(schema["$id"], Resource.from_contents(schema))
    bindings_validator = Draft202012Validator(bindings_schema, registry=registry)
    validator = Draft202012Validator(schema)
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
    with tempfile.TemporaryDirectory(prefix="empack-schema-") as temporary:
        root = pathlib.Path(temporary)
        project = root / "project"
        project.mkdir()
        for index, (expected, patch) in enumerate(cases):
            document = copy.deepcopy(BASE)
            document.update(patch)
            assert validator.is_valid(document) == expected, f"schema case {index}: {document}"
            path = project / "empack.yml"
            path.write_text(json.dumps(document))
            before = path.read_bytes()
            result = subprocess.run([str(binary), "--workdir", str(project), "--state-dir", str(root / "state"), "--cache-dir", str(root / "cache"), "manifest", "check", "--json"], capture_output=True, text=True)
            assert (result.returncode == 0) == expected, f"CLI case {index}: {result.stdout}\n{result.stderr}"
            assert path.read_bytes() == before
            assert not (root / "state").exists()
            assert not (root / "cache").exists()
        assert list(project.iterdir()) == [path]
        document = copy.deepcopy(BASE)
        document["dependencies"] = {"example": "modrinth:Example1"}
        path.write_text(json.dumps(document))
        binding_cases = [
            (True, {"schema": 1, "bindings": {}}),
            (True, {"schema": 1, "bindings": {"example": {"placement": {"files": {"primary": [{"destination": "mods/menu.jar", "layer": "client", "environment": {"client": "required", "server": "unsupported"}}]}}}}}),
            (False, {"schema": 2, "bindings": {}}),
            (False, {"schema": 1, "bindings": {"example": {}}}),
            (False, {"schema": 1, "bindings": {"example": {"source": "modrinth:Other1"}}}),
        ]
        binding_path = project / "empack.bindings.yml"
        for index, (expected, binding) in enumerate(binding_cases):
            assert bindings_validator.is_valid(binding) == expected, f"bindings schema case {index}"
            binding_path.write_text(json.dumps(binding))
            before = (path.read_bytes(), binding_path.read_bytes())
            result = subprocess.run([str(binary), "--workdir", str(project), "--state-dir", str(root / "state"), "--cache-dir", str(root / "cache"), "manifest", "check", "--json"], capture_output=True, text=True)
            assert (result.returncode == 0) == expected, f"bindings CLI case {index}: {result.stdout}\n{result.stderr}"
            assert (path.read_bytes(), binding_path.read_bytes()) == before
            assert not (root / "state").exists()
            assert not (root / "cache").exists()
        assert set(project.iterdir()) == {path, binding_path}
    print(f"Editor schema and CLI agree on {len(cases)} authoring and {len(binding_cases)} bindings cases; all checks were read-only")


if __name__ == "__main__":
    main(pathlib.Path(sys.argv[1]).resolve())
