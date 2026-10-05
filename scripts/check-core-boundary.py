#!/usr/bin/env python3
"""Keep the initial semantic core free of runtime dependencies and ambient I/O."""
import json
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=root, text=True
))
core = next(package for package in metadata["packages"] if package["name"] == "empack-core")
assert not core["dependencies"], "Review any new dependency against the pure-core contract"
source_root = root / "crates/empack-core/src"
assert "#![no_std]" in (source_root / "lib.rs").read_text()
for source in source_root.rglob("*.rs"):
    text = source.read_text()
    assert not re.search(r"\bstd\s*::|extern\s+crate\s+std\b", text), source
print("empack-core: no_std, no dependencies, no ambient std imports")
