"""Check the units Cargo actually built, independently of declared CLI features."""
import hashlib
import json
from pathlib import Path

from .evidence import sha256


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
