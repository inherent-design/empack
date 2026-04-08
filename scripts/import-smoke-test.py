#!/usr/bin/env python3
"""
Import Smoke Test
empack - Minecraft Modpack Lifecycle Management

Discovers, downloads, and analyzes the most popular modpacks from
Modrinth and CurseForge across MC versions. Produces a structured
report of content routing patterns: datapack loaders, override
directories, non-mods file placement, and import success rates.

Designed for re-running against updated empack binaries to track
import compatibility regressions across real-world modpacks.

Prerequisites:
    - Python 3.10+
    - Network access to Modrinth and CurseForge APIs
    - empack binary (auto-detected from target/release or target/debug)
    - CurseForge API key is hardcoded (same as empack's default)

Usage:
    # Discover packs across all 8 default MC versions (no download)
    python3 scripts/import-smoke-test.py --discover-only

    # Full survey: discover, download, analyze structure
    python3 scripts/import-smoke-test.py

    # Full survey with import testing (runs empack init --from on each)
    python3 scripts/import-smoke-test.py --import-test

    # Narrow to specific versions, fewer packs per version
    python3 scripts/import-smoke-test.py --mc-versions 1.20.1,1.16.5 --limit 3

    # Re-analyze cached packs without re-downloading
    python3 scripts/import-smoke-test.py --skip-download --import-test

    # Use a specific empack binary
    python3 scripts/import-smoke-test.py --import-test --empack-bin ./target/release/empack

Phases:
    1. Discover: query Modrinth + CurseForge for top modpacks per MC version
    2. Resolve:  find download URLs for each pack
    3. Download: fetch archives to /tmp/empack-survey/packs/ (cached)
    4. Analyze:  inspect archive contents for structural patterns
       - Content routing (mods, resourcepacks, shaderpacks, datapacks)
       - Datapack loader detection (Paxi, Open Loader, Global Packs)
       - Override directory structure
       - (with --import-test) Run empack init --from and record results

Outputs:
    /tmp/empack-survey/packs/        Downloaded archives (cached between runs)
    /tmp/empack-survey/projects/     empack init --from output (with --import-test)
    /tmp/empack-survey/report.json   Structured findings (one entry per pack)
"""

import argparse
import json
import os
import re
import select
import shutil
import socket
import string
import subprocess
import sys
import tempfile
import time
import zipfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Optional
from urllib.parse import quote, urlparse
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError

try:
    import pty
except ImportError:  # pragma: no cover - Windows fallback
    pty = None

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

CURSEFORGE_API_KEY = os.environ.get(
    "EMPACK_KEY_CURSEFORGE",
    "$2a$10$78GooA4YTCKFQI9vgZ1oEeVM.jNyeNKSIFUhFkwiA0L/Uwv19BFAq",
)

MODRINTH_API = "https://api.modrinth.com/v2"
CURSEFORGE_API = "https://api.curseforge.com/v1"

SURVEY_DIR = Path("/tmp/empack-survey")
PACKS_DIR = SURVEY_DIR / "packs"
PROJECTS_DIR = SURVEY_DIR / "projects"

# Top MC versions by modding activity (ordered by ecosystem size).
# Default set is tuned for reasonable runtime (~30 min with --import-test).
# Use --mc-versions to override.
DEFAULT_MC_VERSIONS = [
    "1.20.1",
    "1.16.5",
    "1.21.1",
]

# Extended set for deep surveys.
ALL_MC_VERSIONS = [
    "1.20.1",
    "1.19.2",
    "1.18.2",
    "1.16.5",
    "1.12.2",
    "1.7.10",
    "1.21.1",
    "1.20.4",
]

# Known datapack loader mods and their install paths.
# Key: mod slug or project name pattern.
# Value: path where datapacks are loaded from.
KNOWN_DATAPACK_LOADERS = {
    "paxi": "config/paxi/datapacks",
    "open-loader": "config/openloader/data",
    "global-packs": "global/datapacks",
    "globaldata": "globaldata",
}

# ---------------------------------------------------------------------------
# API helpers
# ---------------------------------------------------------------------------

def modrinth_get(path: str, params: dict | None = None) -> dict | list:
    url = f"{MODRINTH_API}{path}"
    if params:
        qs = "&".join(f"{k}={quote(str(v))}" for k, v in params.items())
        url = f"{url}?{qs}"
    req = Request(url, headers={"User-Agent": "empack-survey/1.0"})
    with urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def curseforge_get(path: str, params: dict | None = None) -> dict:
    url = f"{CURSEFORGE_API}{path}"
    if params:
        qs = "&".join(f"{k}={quote(str(v))}" for k, v in params.items())
        url = f"{url}?{qs}"
    req = Request(url, headers={
        "x-api-key": CURSEFORGE_API_KEY,
        "Accept": "application/json",
    })
    with urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def curseforge_download(project_id: int, file_id: int, dest: Path, timeout: int = 120) -> bool:
    try:
        data = curseforge_get(f"/mods/{project_id}/files/{file_id}/download-url")
        dl_url = data.get("data", "")
        if not dl_url:
            return False
        req = Request(dl_url, headers={"User-Agent": "empack-survey/1.0"})
        with urlopen(req, timeout=timeout) as resp:
            dest.write_bytes(resp.read())
        return True
    except (HTTPError, URLError, KeyError, socket.timeout):
        return False


# ---------------------------------------------------------------------------
# Discovery
# ---------------------------------------------------------------------------

def normalize_name(name: str) -> str:
    """Normalize a pack name for deduplication: lowercase, strip punctuation, collapse whitespace."""
    name = name.lower()
    name = name.translate(str.maketrans("", "", string.punctuation))
    name = re.sub(r"\s+", " ", name).strip()
    return name


def filename_from_url(url: str) -> str:
    """Extract the filename component from a URL for dedup."""
    if not url:
        return ""
    path = urlparse(url).path
    return path.rsplit("/", 1)[-1].lower() if "/" in path else path.lower()


@dataclass
class PackCandidate:
    name: str
    mc_version: str
    source: str  # "modrinth" or "curseforge"
    slug: str
    project_id: str
    downloads: int
    loader: str
    # filled after resolve
    file_url: str = ""
    file_id: str = ""
    file_size: Optional[int] = None
    # filled after download
    local_path: str = ""
    format: str = ""  # "mrpack" or "cfzip"


def discover_modrinth(mc_version: str, limit: int) -> list[PackCandidate]:
    facets = json.dumps([
        [f"versions:{mc_version}"],
        ["project_type:modpack"],
    ])
    try:
        results = modrinth_get("/search", {
            "facets": facets,
            "limit": str(limit),
            "index": "downloads",
        })
    except (HTTPError, URLError) as e:
        print(f"  [modrinth] search failed for {mc_version}: {e}", file=sys.stderr)
        return []

    candidates = []
    for hit in results.get("hits", [])[:limit]:
        slug = hit.get("slug", "")
        loader = "unknown"
        for cat in hit.get("categories", []):
            if cat in ("fabric", "forge", "neoforge", "quilt"):
                loader = cat
                break

        candidates.append(PackCandidate(
            name=hit.get("title", slug),
            mc_version=mc_version,
            source="modrinth",
            slug=slug,
            project_id=hit.get("project_id", ""),
            downloads=hit.get("downloads", 0),
            loader=loader,
            format="mrpack",
        ))
    return candidates


def discover_curseforge(mc_version: str, limit: int) -> list[PackCandidate]:
    try:
        results = curseforge_get("/mods/search", {
            "gameId": "432",
            "classId": "4471",  # modpacks
            "gameVersion": mc_version,
            "sortField": "2",   # popularity
            "sortOrder": "desc",
            "pageSize": str(limit),
        })
    except (HTTPError, URLError) as e:
        print(f"  [curseforge] search failed for {mc_version}: {e}", file=sys.stderr)
        return []

    candidates = []
    for mod in results.get("data", [])[:limit]:
        loader = "unknown"
        for lf in mod.get("latestFilesIndexes", []):
            if lf.get("gameVersion") == mc_version:
                ml = lf.get("modLoader")
                if ml == 1:
                    loader = "forge"
                elif ml == 4:
                    loader = "fabric"
                elif ml == 6:
                    loader = "neoforge"
                elif ml == 5:
                    loader = "quilt"
                break

        candidates.append(PackCandidate(
            name=mod.get("name", ""),
            mc_version=mc_version,
            source="curseforge",
            slug=mod.get("slug", ""),
            project_id=str(mod.get("id", "")),
            downloads=mod.get("downloadCount", 0),
            loader=loader,
            format="cfzip",
        ))
    return candidates


def resolve_download_url(c: PackCandidate) -> bool:
    if c.source == "modrinth":
        try:
            versions = modrinth_get(f"/project/{c.project_id}/version", {
                "game_versions": json.dumps([c.mc_version]),
            })
            if not versions:
                return False
            ver = versions[0]
            for f in ver.get("files", []):
                if f.get("primary", False) or len(ver["files"]) == 1:
                    c.file_url = f["url"]
                    c.file_id = ver["id"]
                    if "size" in f:
                        c.file_size = f["size"]
                    return True
            if ver.get("files"):
                chosen = ver["files"][0]
                c.file_url = chosen["url"]
                c.file_id = ver["id"]
                if "size" in chosen:
                    c.file_size = chosen["size"]
                return True
        except (HTTPError, URLError, KeyError, IndexError):
            pass
        return False

    elif c.source == "curseforge":
        try:
            files = curseforge_get(f"/mods/{c.project_id}/files", {
                "gameVersion": c.mc_version,
                "pageSize": "1",
            })
            for f in files.get("data", []):
                c.file_id = str(f["id"])
                if "fileLength" in f:
                    c.file_size = f["fileLength"]
                return True
        except (HTTPError, URLError, KeyError):
            pass
        return False

    return False


# ---------------------------------------------------------------------------
# Download
# ---------------------------------------------------------------------------

def download_pack(c: PackCandidate, timeout: int = 60) -> bool:
    safe_name = f"{c.slug}_{c.mc_version}_{c.source}"
    ext = ".mrpack" if c.format == "mrpack" else ".zip"
    dest = PACKS_DIR / f"{safe_name}{ext}"

    if dest.exists() and dest.stat().st_size > 0:
        c.local_path = str(dest)
        return True

    if c.source == "modrinth":
        if not c.file_url:
            return False
        try:
            req = Request(c.file_url, headers={"User-Agent": "empack-survey/1.0"})
            with urlopen(req, timeout=timeout) as resp:
                dest.write_bytes(resp.read())
            c.local_path = str(dest)
            return True
        except (HTTPError, URLError, socket.timeout):
            # Clean up partial download
            if dest.exists():
                dest.unlink()
            return False

    elif c.source == "curseforge":
        if not c.file_id:
            return False
        ok = curseforge_download(int(c.project_id), int(c.file_id), dest, timeout=timeout)
        if ok:
            c.local_path = str(dest)
        else:
            # Clean up partial download
            if dest.exists():
                dest.unlink()
        return ok

    return False


# ---------------------------------------------------------------------------
# Analysis
# ---------------------------------------------------------------------------

@dataclass
class PackAnalysis:
    name: str
    mc_version: str
    source: str
    slug: str
    loader: str
    format: str
    # content counts
    total_files: int = 0
    mods_files: int = 0
    resourcepack_files: int = 0
    shaderpack_files: int = 0
    other_files: int = 0
    override_count: int = 0
    # datapack signals
    datapack_files: int = 0
    datapack_loader_mod: str = ""
    datapack_override_path: str = ""
    datapack_override_count: int = 0
    # structural observations
    has_manifest: bool = False
    manifest_type: str = ""  # "modrinth.index.json" or "manifest.json"
    override_dir_name: str = ""
    file_paths_outside_mods: list = field(default_factory=list)
    modrinth_loaders_seen: list = field(default_factory=list)
    errors: list = field(default_factory=list)


def analyze_mrpack(path: Path) -> PackAnalysis:
    a = PackAnalysis(
        name="", mc_version="", source="modrinth", slug="",
        loader="", format="mrpack",
    )
    try:
        with zipfile.ZipFile(path) as z:
            names = z.namelist()

            if "modrinth.index.json" not in names:
                a.errors.append("missing modrinth.index.json")
                return a

            a.has_manifest = True
            a.manifest_type = "modrinth.index.json"

            m = json.loads(z.read("modrinth.index.json"))
            a.name = m.get("name", "")

            deps = m.get("dependencies", {})
            a.mc_version = deps.get("minecraft", "")
            for k in deps:
                if k in ("fabric-loader", "forge", "neoforge", "quilt-loader"):
                    a.loader = k.replace("-loader", "")

            files = m.get("files", [])
            a.total_files = len(files)

            for f in files:
                p = f.get("path", "")
                if p.startswith("mods/"):
                    a.mods_files += 1
                elif p.startswith("resourcepacks/"):
                    a.resourcepack_files += 1
                elif p.startswith("shaderpacks/"):
                    a.shaderpack_files += 1
                else:
                    a.other_files += 1
                    a.file_paths_outside_mods.append(p)

            # Count overrides
            override_dirs = ["overrides/", "client-overrides/", "server-overrides/"]
            a.override_dir_name = m.get("overrides", "overrides")
            for name in names:
                for od in override_dirs:
                    if name.startswith(od):
                        a.override_count += 1
                        break

            # Detect datapack loader patterns
            for name in names:
                for mod_slug, dp_path in KNOWN_DATAPACK_LOADERS.items():
                    if dp_path in name.lower():
                        a.datapack_loader_mod = mod_slug
                        a.datapack_override_path = dp_path
                        break

            # Count datapack override files
            if a.datapack_override_path:
                a.datapack_override_count = sum(
                    1 for n in names
                    if a.datapack_override_path in n and not n.endswith("/")
                )

    except (zipfile.BadZipFile, KeyError, json.JSONDecodeError) as e:
        a.errors.append(str(e))

    return a


def analyze_cfzip(path: Path) -> PackAnalysis:
    a = PackAnalysis(
        name="", mc_version="", source="curseforge", slug="",
        loader="", format="cfzip",
    )
    try:
        with zipfile.ZipFile(path) as z:
            names = z.namelist()

            if "manifest.json" not in names:
                a.errors.append("missing manifest.json")
                return a

            a.has_manifest = True
            a.manifest_type = "manifest.json"

            m = json.loads(z.read("manifest.json"))
            a.name = m.get("name", "")
            a.mc_version = m.get("minecraft", {}).get("version", "")
            a.override_dir_name = m.get("overrides", "overrides")

            loaders = m.get("minecraft", {}).get("modLoaders", [])
            for loader in loaders:
                lid = loader.get("id", "")
                if lid.startswith("forge"):
                    a.loader = "forge"
                elif lid.startswith("fabric"):
                    a.loader = "fabric"
                elif lid.startswith("neoforge"):
                    a.loader = "neoforge"
                elif lid.startswith("quilt"):
                    a.loader = "quilt"

            a.total_files = len(m.get("files", []))
            a.mods_files = a.total_files  # CF manifest files are all mods

            # Count overrides
            override_prefix = a.override_dir_name + "/"
            for name in names:
                if name.startswith(override_prefix):
                    a.override_count += 1

            # Detect datapack loader patterns
            for name in names:
                for mod_slug, dp_path in KNOWN_DATAPACK_LOADERS.items():
                    if dp_path in name:
                        a.datapack_loader_mod = mod_slug
                        a.datapack_override_path = dp_path
                        break

            if a.datapack_override_path:
                a.datapack_override_count = sum(
                    1 for n in names
                    if a.datapack_override_path in n and not n.endswith("/")
                )

    except (zipfile.BadZipFile, KeyError, json.JSONDecodeError) as e:
        a.errors.append(str(e))

    return a


# ---------------------------------------------------------------------------
# Import test
# ---------------------------------------------------------------------------

@dataclass
class ImportResult:
    success: bool = False
    exit_code: int = -1
    platform_refs_added: int = 0
    overrides_copied: int = 0
    embedded_extracted: int = 0
    stdout: str = ""
    stderr: str = ""
    warnings: list = field(default_factory=list)


def should_echo_live_line(clean: str) -> bool:
    if not clean:
        return False
    if clean.startswith("Error:") or clean.startswith("Caused by:"):
        return True
    if clean.startswith("✗") or clean.startswith("! "):
        return True
    if "failed" in clean.lower():
        return True
    if "warning" in clean.lower():
        return True
    return False


def parse_import_output(stdout: str, stderr: str) -> ImportResult:
    r = ImportResult(
        stdout=stdout,
        stderr=stderr,
    )

    combined = stdout + stderr
    for line in combined.splitlines():
        clean = re.sub(r'\x1b\[[0-9;]*m', '', line.strip())
        if not clean:
            continue
        if "Platform references added:" in clean:
            try:
                r.platform_refs_added = int(clean.split(":")[-1].strip())
            except ValueError:
                pass
        elif "Override files copied:" in clean:
            try:
                r.overrides_copied = int(clean.split(":")[-1].strip())
            except ValueError:
                pass
        elif "Embedded files extracted:" in clean:
            try:
                r.embedded_extracted = int(clean.split(":")[-1].strip().split()[0])
            except (ValueError, IndexError):
                pass
        elif "failed for" in clean or "! " in clean:
            r.warnings.append(clean)
        elif clean.startswith("Error:") or clean.startswith("Caused by:"):
            r.warnings.append(clean)

    return r


def run_import_test_posix_live(
    pack_path: Path,
    project_dir: Path,
    empack_bin: Path,
    env: dict[str, str],
    timeout: int,
    label: str,
) -> ImportResult:
    cmd = [str(empack_bin), "init", "--from", str(pack_path), "--yes", str(project_dir)]
    master_fd, slave_fd = pty.openpty()
    start = time.time()
    echoed_lines: set[str] = set()

    try:
        proc = subprocess.Popen(
            cmd,
            stdin=subprocess.DEVNULL,
            stdout=slave_fd,
            stderr=slave_fd,
            env=env,
            text=False,
        )
    finally:
        os.close(slave_fd)

    chunks: list[str] = []
    line_buffer = ""

    try:
        while True:
            if proc.poll() is not None and not select.select([master_fd], [], [], 0)[0]:
                break

            if time.time() - start > timeout:
                proc.kill()
                proc.wait()
                return ImportResult(stderr=f"TIMEOUT after {timeout}s")

            ready, _, _ = select.select([master_fd], [], [], 0.2)
            if not ready:
                continue

            try:
                data = os.read(master_fd, 4096)
            except OSError:
                break

            if not data:
                continue

            text = data.decode("utf-8", errors="replace")
            chunks.append(text)
            line_buffer += text

            while "\n" in line_buffer:
                raw_line, line_buffer = line_buffer.split("\n", 1)
                clean = re.sub(r'\x1b\[[0-9;]*m', '', raw_line.strip())
                if should_echo_live_line(clean) and clean not in echoed_lines:
                    echoed_lines.add(clean)
                    print(f"      {label}: {clean}")

        proc.wait()
    finally:
        os.close(master_fd)

    if line_buffer:
        chunks.append(line_buffer)
        clean = re.sub(r'\x1b\[[0-9;]*m', '', line_buffer.strip())
        if should_echo_live_line(clean) and clean not in echoed_lines:
            print(f"      {label}: {clean}")

    combined = "".join(chunks)
    result = parse_import_output(combined, "")
    result.success = proc.returncode == 0
    result.exit_code = proc.returncode
    return result


def run_import_test(pack_path: Path, project_name: str, empack_bin: Path) -> ImportResult:
    project_dir = PROJECTS_DIR / project_name
    if project_dir.exists():
        shutil.rmtree(project_dir)

    env = os.environ.copy()
    env["EMPACK_KEY_CURSEFORGE"] = CURSEFORGE_API_KEY
    timeout = 600

    if os.name == "posix" and pty is not None:
        return run_import_test_posix_live(
            pack_path=pack_path,
            project_dir=project_dir,
            empack_bin=empack_bin,
            env=env,
            timeout=timeout,
            label=project_name,
        )

    try:
        proc = subprocess.run(
            [str(empack_bin), "init", "--from", str(pack_path), "--yes", str(project_dir)],
            capture_output=True, text=True, timeout=timeout, env=env,
        )
    except subprocess.TimeoutExpired:
        return ImportResult(stderr=f"TIMEOUT after {timeout}s")

    r = parse_import_output(proc.stdout, proc.stderr)
    r.success = proc.returncode == 0
    r.exit_code = proc.returncode
    return r


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

def print_discovery(candidates: list[PackCandidate]):
    by_version = {}
    for c in candidates:
        by_version.setdefault(c.mc_version, []).append(c)

    for ver in sorted(by_version, key=lambda v: DEFAULT_MC_VERSIONS.index(v) if v in DEFAULT_MC_VERSIONS else 99):
        print(f"\n{'=' * 60}")
        print(f"MC {ver}")
        print(f"{'=' * 60}")
        for c in by_version[ver]:
            dl = f"{c.downloads:,}"
            print(f"  [{c.source:10s}] {c.name:40s} {c.loader:10s} {dl:>12s} dl")


def print_analysis_summary(analyses: list[tuple[PackCandidate, PackAnalysis, ImportResult | None]]):
    print(f"\n{'=' * 80}")
    print("SURVEY RESULTS")
    print(f"{'=' * 80}")

    dp_packs = []
    non_mods_routing = []
    failures = []

    for c, a, ir in analyses:
        tag = f"[{a.mc_version} {a.loader:8s} {a.source:10s}]"
        status = ""
        if ir:
            if ir.success:
                fail_count = len(ir.warnings)
                status = f"OK refs={ir.platform_refs_added} ovr={ir.overrides_copied}"
                if fail_count:
                    status += f" warn={fail_count}"
            else:
                status = f"FAIL exit={ir.exit_code}"
                failures.append((c, a, ir))
        else:
            status = f"files={a.total_files} ovr={a.override_count}"

        print(f"  {tag} {a.name:40s} {status}")

        if a.datapack_loader_mod:
            dp_packs.append((c, a))
        if a.file_paths_outside_mods:
            non_mods_routing.append((c, a))

    if dp_packs:
        print(f"\n--- Datapack Loader Patterns ({len(dp_packs)} packs) ---")
        for c, a in dp_packs:
            print(f"  {a.name}: mod={a.datapack_loader_mod} path={a.datapack_override_path} count={a.datapack_override_count}")

    if non_mods_routing:
        print(f"\n--- Non-mods File Routing ({len(non_mods_routing)} packs) ---")
        for c, a in non_mods_routing:
            by_prefix = {}
            for p in a.file_paths_outside_mods:
                prefix = p.split("/")[0] if "/" in p else p
                by_prefix[prefix] = by_prefix.get(prefix, 0) + 1
            routing = ", ".join(f"{k}={v}" for k, v in sorted(by_prefix.items()))
            print(f"  {a.name}: {routing}")

    # Collect packs with warnings (succeeded but with issues)
    warned = [(c, a, ir) for c, a, ir in analyses
              if ir and ir.success and ir.warnings]

    if warned:
        print(f"\n--- Import Warnings ({len(warned)} packs) ---")
        for c, a, ir in warned:
            print(f"\n  {a.name} [{a.mc_version} {a.source}] ({len(ir.warnings)} warnings)")
            for w in ir.warnings[:5]:
                print(f"    {w[:150]}")
            if len(ir.warnings) > 5:
                print(f"    ... and {len(ir.warnings) - 5} more")

    if failures:
        print(f"\n--- Import Failures ({len(failures)} packs) ---")
        for c, a, ir in failures:
            print(f"\n  {a.name} [{a.mc_version} {a.source}]")
            for w in ir.warnings[:5]:
                print(f"    {w[:150]}")
            if len(ir.warnings) > 5:
                print(f"    ... and {len(ir.warnings) - 5} more")


def save_report(analyses: list[tuple[PackCandidate, PackAnalysis, ImportResult | None]]):
    report = []
    for c, a, ir in analyses:
        entry = {
            "candidate": asdict(c),
            "analysis": asdict(a),
        }
        if ir:
            entry["import_result"] = {
                "success": ir.success,
                "exit_code": ir.exit_code,
                "platform_refs_added": ir.platform_refs_added,
                "overrides_copied": ir.overrides_copied,
                "embedded_extracted": ir.embedded_extracted,
                "warning_count": len(ir.warnings),
                "warnings": ir.warnings[:20],  # cap for readability
            }
        report.append(entry)

    report_path = SURVEY_DIR / "report.json"
    report_path.write_text(json.dumps(report, indent=2))
    print(f"\nReport saved to {report_path}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def find_empack_bin() -> Path:
    script_dir = Path(__file__).resolve().parent
    project_root = script_dir.parent
    release = project_root / "target" / "release" / "empack"
    debug = project_root / "target" / "debug" / "empack"
    if release.exists():
        return release
    if debug.exists():
        return debug
    # fall back to PATH
    which = shutil.which("empack")
    if which:
        return Path(which)
    return release  # will fail at runtime with a clear path


def main():
    parser = argparse.ArgumentParser(description="empack import smoke test")
    parser.add_argument("--discover-only", action="store_true",
                        help="List packs without downloading")
    parser.add_argument("--skip-download", action="store_true",
                        help="Analyze already-downloaded packs")
    parser.add_argument("--import-test", action="store_true",
                        help="Run empack init --from on each pack")
    parser.add_argument("--mc-versions", type=str, default=None,
                        help="Comma-separated MC versions (default: top 3)")
    parser.add_argument("--limit", type=int, default=5,
                        help="Top N modpacks per version per platform (default: 5)")
    parser.add_argument("--all-versions", action="store_true",
                        help="Use all 8 MC versions instead of default 3")
    parser.add_argument("--empack-bin", type=str, default=None,
                        help="Path to empack binary")
    parser.add_argument("--clean", choices=["projects", "all"],
                        help="Remove import output (projects) or everything (all) and exit")
    parser.add_argument("--download-timeout", type=int, default=60,
                        help="Per-pack download timeout in seconds (default: 60)")
    parser.add_argument("--max-file-size", type=int, default=100,
                        help="Max file size in MB; skip packs exceeding this (default: 100)")
    args = parser.parse_args()

    max_file_size_bytes = args.max_file_size * 1024 * 1024

    if args.clean:
        if args.clean == "all":
            if SURVEY_DIR.exists():
                shutil.rmtree(SURVEY_DIR)
                print(f"Removed {SURVEY_DIR}")
        else:
            if PROJECTS_DIR.exists():
                shutil.rmtree(PROJECTS_DIR)
                print(f"Removed {PROJECTS_DIR} (cached packs kept)")
        return

    if args.mc_versions:
        mc_versions = args.mc_versions.split(",")
    elif args.all_versions:
        mc_versions = ALL_MC_VERSIONS
    else:
        mc_versions = DEFAULT_MC_VERSIONS

    PACKS_DIR.mkdir(parents=True, exist_ok=True)
    PROJECTS_DIR.mkdir(parents=True, exist_ok=True)

    # Phase 1: Discover (fetch limit * 2 for backfill buffer)
    fetch_limit = args.limit * 2
    print(f"Phase 1: Discovering modpacks (fetching {fetch_limit} per platform for backfill buffer)...")
    all_candidates = []
    for ver in mc_versions:
        print(f"  MC {ver}...")
        mr = discover_modrinth(ver, fetch_limit)
        cf = discover_curseforge(ver, fetch_limit)
        # Modrinth first so it wins dedup ties (preferred: direct download URLs)
        all_candidates.extend(mr)
        all_candidates.extend(cf)
        time.sleep(0.5)  # rate limit courtesy

    # First-pass dedup: normalized name + mc_version (catches cross-platform dupes)
    # Prefer Modrinth over CurseForge (direct download URLs, no restricted files)
    seen_names: dict[tuple[str, str], PackCandidate] = {}
    for c in all_candidates:
        key = (normalize_name(c.name), c.mc_version)
        if key not in seen_names:
            seen_names[key] = c
        elif c.source == "modrinth" and seen_names[key].source == "curseforge":
            # Replace CF with Modrinth
            seen_names[key] = c

    # Also dedup by slug (original behavior, catches slug-based dupes)
    seen_slugs: set[tuple[str, str]] = set()
    deduped_all: list[PackCandidate] = []
    for c in seen_names.values():
        slug_key = (c.slug.lower().replace("-", "").replace(" ", ""), c.mc_version)
        if slug_key not in seen_slugs:
            seen_slugs.add(slug_key)
            deduped_all.append(c)

    # Sort by downloads descending within each version to prioritize popular packs
    deduped_all.sort(key=lambda c: (-DEFAULT_MC_VERSIONS.index(c.mc_version) if c.mc_version in DEFAULT_MC_VERSIONS else -99, -c.downloads))

    # Split into primary candidates and backfill buffer
    # Group by mc_version, take first `limit` as primary, rest as buffer
    by_version: dict[str, list[PackCandidate]] = {}
    for c in deduped_all:
        by_version.setdefault(c.mc_version, []).append(c)

    primary: list[PackCandidate] = []
    buffer: list[PackCandidate] = []
    for ver in mc_versions:
        ver_candidates = by_version.get(ver, [])
        # Sort by downloads descending within version
        ver_candidates.sort(key=lambda c: -c.downloads)
        primary.extend(ver_candidates[:args.limit])
        buffer.extend(ver_candidates[args.limit:])

    deduped = primary  # active working set
    total_discovered = len(deduped) + len(buffer)
    print(f"  Found {total_discovered} unique packs ({len(deduped)} primary + {len(buffer)} buffer) across {len(mc_versions)} MC versions")
    print_discovery(deduped)

    if args.discover_only:
        return

    # Phase 2: Resolve download URLs
    print("\nPhase 2: Resolving download URLs...")
    resolved = []
    skipped_resolve = 0
    for c in deduped:
        ok = resolve_download_url(c)
        if ok:
            # Check max file size
            if c.file_size is not None and c.file_size > max_file_size_bytes:
                size_mb = c.file_size / (1024 * 1024)
                print(f"  SKIP {c.name} ({c.source}): {size_mb:.0f}MB exceeds --max-file-size {args.max_file_size}MB", file=sys.stderr)
                skipped_resolve += 1
            else:
                resolved.append(c)
        else:
            print(f"  SKIP {c.name} ({c.source}): no download URL", file=sys.stderr)
            skipped_resolve += 1
        time.sleep(0.3)

    # Backfill from buffer for packs skipped during resolve
    if skipped_resolve > 0 and buffer:
        print(f"  Backfilling {min(skipped_resolve, len(buffer))} candidates from buffer...")
        # Second-pass dedup: by downloaded filename (after resolve, before download)
        resolved_filenames: set[str] = set()
        for c in resolved:
            fn = filename_from_url(c.file_url)
            if fn:
                resolved_filenames.add(fn)

        backfilled = 0
        while backfilled < skipped_resolve and buffer:
            bc = buffer.pop(0)
            ok = resolve_download_url(bc)
            if not ok:
                continue
            # Check max file size for backfill candidate
            if bc.file_size is not None and bc.file_size > max_file_size_bytes:
                continue
            # Filename dedup
            fn = filename_from_url(bc.file_url)
            if fn and fn in resolved_filenames:
                continue
            if fn:
                resolved_filenames.add(fn)
            resolved.append(bc)
            backfilled += 1
            time.sleep(0.3)
        if backfilled:
            print(f"  Backfilled {backfilled} packs from buffer")

    # Second-pass dedup on resolved set: by downloaded filename
    seen_filenames: set[str] = set()
    filename_deduped: list[PackCandidate] = []
    for c in resolved:
        fn = filename_from_url(c.file_url)
        if fn and fn in seen_filenames:
            print(f"  DEDUP {c.name} ({c.source}): duplicate filename {fn}", file=sys.stderr)
            continue
        if fn:
            seen_filenames.add(fn)
        filename_deduped.append(c)
    resolved = filename_deduped

    print(f"  Resolved {len(resolved)}/{len(deduped)} packs")

    # Phase 3: Download
    if not args.skip_download:
        print(f"\nPhase 3: Downloading (timeout={args.download_timeout}s per pack)...")
        downloaded = []
        skipped_download = 0
        for i, c in enumerate(resolved):
            safe = f"{c.slug}_{c.mc_version}_{c.source}"
            ext = ".mrpack" if c.format == "mrpack" else ".zip"
            existing = PACKS_DIR / f"{safe}{ext}"
            if existing.exists() and existing.stat().st_size > 0:
                c.local_path = str(existing)
                downloaded.append(c)
                print(f"  [{i+1}/{len(resolved)}] {c.name}: cached")
                continue

            print(f"  [{i+1}/{len(resolved)}] {c.name}...", end=" ", flush=True)
            ok = download_pack(c, timeout=args.download_timeout)
            if ok:
                size_mb = Path(c.local_path).stat().st_size / (1024 * 1024)
                print(f"{size_mb:.1f}MB")
                downloaded.append(c)
            else:
                print("FAILED (timeout or error)")
                skipped_download += 1
            time.sleep(0.3)

        # Backfill from buffer for download failures
        if skipped_download > 0 and buffer:
            print(f"  Backfilling {min(skipped_download, len(buffer))} candidates from buffer for download failures...")
            dl_backfilled = 0
            while dl_backfilled < skipped_download and buffer:
                bc = buffer.pop(0)
                if not bc.file_url:
                    ok = resolve_download_url(bc)
                    if not ok:
                        continue
                    if bc.file_size is not None and bc.file_size > max_file_size_bytes:
                        continue
                print(f"  [backfill] {bc.name}...", end=" ", flush=True)
                ok = download_pack(bc, timeout=args.download_timeout)
                if ok:
                    size_mb = Path(bc.local_path).stat().st_size / (1024 * 1024)
                    print(f"{size_mb:.1f}MB")
                    downloaded.append(bc)
                    dl_backfilled += 1
                else:
                    print("FAILED")
                time.sleep(0.3)
            if dl_backfilled:
                print(f"  Backfilled {dl_backfilled} packs from buffer")

        print(f"  Downloaded {len(downloaded)}/{len(resolved)} packs")
    else:
        downloaded = [c for c in resolved if c.local_path and Path(c.local_path).exists()]
        # Also pick up any packs already on disk
        for f in PACKS_DIR.iterdir():
            if f.suffix in (".mrpack", ".zip") and f.stat().st_size > 0:
                parts = f.stem.split("_")
                if len(parts) >= 3:
                    existing = [c for c in downloaded if c.slug == parts[0] and c.mc_version == parts[1]]
                    if not existing:
                        # Reconstruct minimal candidate
                        source = parts[-1]
                        fmt = "mrpack" if f.suffix == ".mrpack" else "cfzip"
                        downloaded.append(PackCandidate(
                            name=parts[0], mc_version=parts[1], source=source,
                            slug=parts[0], project_id="", downloads=0,
                            loader="unknown", format=fmt, local_path=str(f),
                        ))

    # Phase 4: Analyze
    print("\nPhase 4: Analyzing archives...")
    empack_bin = Path(args.empack_bin) if args.empack_bin else find_empack_bin()

    analyses = []
    for c in downloaded:
        path = Path(c.local_path)
        if c.format == "mrpack" or path.suffix == ".mrpack":
            a = analyze_mrpack(path)
        else:
            a = analyze_cfzip(path)

        # Backfill from candidate
        a.name = a.name or c.name
        a.mc_version = a.mc_version or c.mc_version
        a.source = c.source
        a.slug = c.slug
        a.loader = a.loader or c.loader

        ir = None
        if args.import_test:
            project_name = f"{c.slug}_{c.mc_version}_{c.source}"
            print(f"  import: {a.name}...", end=" ", flush=True)
            ir = run_import_test(path, project_name, empack_bin)
            if ir.success:
                parts = [f"OK refs={ir.platform_refs_added} ovr={ir.overrides_copied}"]
                if ir.warnings:
                    parts.append(f"warn={len(ir.warnings)}")
                print(" ".join(parts))
            else:
                # Show the first error line for quick diagnosis
                first_err = ""
                for w in ir.warnings:
                    if w.startswith("Error:") or w.startswith("Caused by:"):
                        first_err = w
                        break
                if not first_err and ir.warnings:
                    first_err = ir.warnings[0]
                if not first_err:
                    first_err = ir.stderr.strip().splitlines()[0] if ir.stderr.strip() else "unknown error"
                print(f"FAIL exit={ir.exit_code}: {first_err[:120]}")

        analyses.append((c, a, ir))

    print_analysis_summary(analyses)
    save_report(analyses)


if __name__ == "__main__":
    main()
