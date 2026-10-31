# /// script
# dependencies = ["jsonschema==4.23.0"]
# ///
"""Observe editor-schema/CLI conformance. Use verify.py schema for admitted evidence."""
import argparse
from importlib.metadata import version
import json
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

from verification.authoring_schema import (ROOT, fixtures, input_hashes, observation_results,
                                           workspace_snapshot)
from verification.evidence import sha256, write_json
from verify import isolated_environment


def observe(binary, validators):
    observations = []
    for case in fixtures():
        with tempfile.TemporaryDirectory(prefix="empack-schema-") as temporary:
            root = Path(temporary)
            project, home = root / "project", root / "home"
            project.mkdir()
            home.mkdir()
            (project / "empack.yml").write_bytes(json.dumps(case["author"]).encode("utf-8"))
            if case["bindings"] is not None:
                (project / "empack.bindings.yml").write_bytes(json.dumps(case["bindings"]).encode("utf-8"))
            before = workspace_snapshot(root)
            command = [str(binary), "--workdir", str(project), "--state-dir", str(root / "state"),
                       "--cache-dir", str(root / "cache"), "manifest", "check", "--json"]
            schema_valid = validators[0].is_valid(case["author"])
            if case["bindings"] is not None:
                schema_valid &= validators[1].is_valid(case["bindings"])
            try:
                result = subprocess.run(command, cwd=home, env=isolated_environment(home),
                                        capture_output=True, text=True, timeout=30)
                row = dict(exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr, timed_out=False)
            except subprocess.TimeoutExpired as error:
                def text(value):
                    return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value or ""
                row = dict(exit_code=-1, stdout=text(error.stdout), stderr=text(error.stderr), timed_out=True)
            observations.append(dict(id=case["id"], schema_valid=schema_valid,
                                     before=before, after=workspace_snapshot(root), **row))
    return observations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if args.output.exists() or args.output.is_symlink():
        parser.error("Output already exists; retain each attempt separately")
    inputs, digest = input_hashes(), sha256(binary)
    schema = json.loads((ROOT / "docs/schemas/empack.schema-5.json").read_text())
    bindings = json.loads((ROOT / "docs/schemas/empack.bindings.schema-1.json").read_text())
    Draft202012Validator.check_schema(schema)
    Draft202012Validator.check_schema(bindings)
    registry = Registry().with_resource(schema["$id"], Resource.from_contents(schema))
    validators = (Draft202012Validator(schema), Draft202012Validator(bindings, registry=registry))
    observations = observe(binary, validators)
    if sha256(binary) != digest or input_hashes() != inputs:
        raise RuntimeError("Product or source changed during schema conformance")
    write_json(args.output, dict(schema=1, kind="authoring-schema-observations", inputs=inputs,
               artifact_sha256=digest, dependencies={"jsonschema": version("jsonschema")},
               python_version=platform.python_version(), interpreter_sha256=sha256(Path(sys.executable).resolve()),
               fixtures=observations))
    failures = [row["id"] for row in observation_results(observations) if row["status"] != "PASS"]
    print(f"Editor schema/CLI: {len(observations) - len(failures)}/{len(observations)} read-only fixtures passed")
    if failures:
        print("Failed fixtures: " + ", ".join(failures), file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
