"""Synthetic compiler records for admission tests; no product compilation claimed."""
from pathlib import PurePosixPath, PureWindowsPath
import json

from verification.product_build import build_command, metadata_command, retain_build
from verification.evidence import sha256, write_json


def compilation(directory, target, digest, revision, stamped, features="default", builder="cargo"):
    path_type = PureWindowsPath if target.endswith("windows-msvc") else PurePosixPath
    target_dir = path_type("C:/fixture/target" if target.endswith("windows-msvc") else "/fixture/target")
    executable = target_dir / target / "release" / ("empack.exe" if target.endswith("windows-msvc") else "empack")
    packages = [{"id": "synthetic::" + name, "name": name}
                for name in ("empack-core", "empack-lib", "empack")]
    events = []
    for package in packages:
        name = package["name"]
        events.append({"reason": "compiler-artifact", "package_id": package["id"],
                       "features": (["telemetry"] if features == "telemetry" else [])
                       if name == "empack" else ["default", "telemetry"]
                       if name == "empack-lib" and features == "telemetry" else [],
                       "target": {"name": name.replace("-", "_"), "kind": ["bin" if name == "empack" else "lib"]},
                       "profile": {"test": False, "debug_assertions": False, "opt_level": "3"},
                       "executable": str(executable) if name == "empack" else None})
    events.append({"reason": "build-finished", "success": True})
    messages = "\n".join(map(json.dumps, events))
    metadata = {"packages": packages, "target_directory": str(target_dir)}
    artifact = retain_build(directory, messages, metadata,
                            features, executable, digest)
    (directory / "build.stdout").write_text(messages, encoding="utf-8")
    (directory / "metadata.stdout").write_text(json.dumps(metadata), encoding="utf-8")
    record = {"schema": 1, "kind": "compiled-product", "source": {"revision": revision, "stamped": stamped},
              "target": target, "features": features, "command": build_command(target, builder, features),
              "metadata_command": metadata_command(target, builder),
              "builder_version": "cross 0.2.5\ncargo 1.99.1 (synthetic)" if builder == "cross" else "cargo 1.99.1 (synthetic)",
              "artifact": artifact}
    write_json(directory / "compile.json", record)
    return sha256(directory / "compile.json")
