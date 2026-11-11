"""Build and admit the single bootstrap JAR used by native exports."""
from pathlib import Path
import subprocess
import tomllib

from .evidence import sha256, write_json


def sources(root):
    root = Path(root)
    names = ["bootstrap/pom.xml", "bootstrap/mise.toml"]
    names += [p.relative_to(root).as_posix() for p in (root / "bootstrap/src/main").rglob("*") if p.is_file()]
    return {name: sha256(root / name) for name in sorted(names)}


def admit(root):
    import json
    root = Path(root)
    jar = root / "bootstrap/target/empack-bootstrap.jar"
    path = jar.with_suffix(".json")
    if jar.is_symlink() or not jar.is_file() or path.is_symlink() or not path.is_file():
        raise ValueError("Build the bootstrap first: python3 scripts/build-bootstrap.py")
    record = json.loads(path.read_text(encoding="utf-8"))
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    if (record.get("schema") != 1 or record.get("version") != version
            or record.get("sources") != sources(root) or record.get("jar_sha256") != sha256(jar)
            or not 0 < jar.stat().st_size <= 8 * 1024 * 1024):
        raise ValueError("Bootstrap inputs changed; rebuild with python3 scripts/build-bootstrap.py")
    return record


def build(root):
    root = Path(root)
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    before = sources(root)
    # The Java project pins its own tools. Native builds only consume the admitted
    # bytes; Cargo build scripts never install tools or access the network.
    command = ["mise", "exec", "--", "mvn", "--batch-mode", "--no-transfer-progress",
               "-Drevision=" + version, "package"]
    subprocess.run(command, cwd=root / "bootstrap", check=True)
    if before != sources(root):
        raise ValueError("Bootstrap source changed during compilation")
    java = subprocess.check_output(["mise", "exec", "--", "java", "-version"],
                                   cwd=root / "bootstrap", stderr=subprocess.STDOUT, text=True).strip()
    maven = subprocess.check_output(["mise", "exec", "--", "mvn", "--version"],
                                    cwd=root / "bootstrap", text=True).strip()
    jar = root / "bootstrap/target/empack-bootstrap.jar"
    record = {"schema": 1, "version": version, "sources": before,
              "jar_sha256": sha256(jar), "java": java, "maven": maven}
    write_json(jar.with_suffix(".json"), record)
    admit(root)
    return record
