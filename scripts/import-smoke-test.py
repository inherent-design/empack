#!/usr/bin/env python3
"""
Import Smoke Test
empack - Minecraft Modpack Lifecycle Management

Runs three top-level modes:

1. Curated smoke mode (default bare invocation):
   - resolves 7 hardcoded real-world packs across CurseForge and Modrinth
   - runs `empack init --from ... --yes`
   - runs `empack build client-full`
   - if a build blocks on restricted CurseForge files, downloads those files
     into empack's managed restricted-build cache and resumes with
     `empack build --continue`
   - also supports:
     - `--profile pr` for one platform-selected CI smoke pack
     - `--pack <curated-pack-id>` for one explicit curated pack

2. Discovery survey mode (selected by explicit survey/filter flags):
   - discovers, downloads, and analyzes popular modpacks across MC versions
   - optionally runs `empack init --from` on each pack

3. Clean mode:
   - removes curated/survey smoke artifacts under `/tmp`

Designed for re-running against updated empack binaries to track
golden-path lifecycle regressions and broader import compatibility.

Prerequisites:
    - Python 3.10+
    - Network access to Modrinth and CurseForge APIs
    - empack binary (auto-detected from target/release or target/debug)
    - CurseForge API key is hardcoded (same as empack's default)

Usage:
    # Default curated golden smoke across 7 hardcoded packs
    python3 scripts/import-smoke-test.py

    # Platform-selected PR smoke profile
    python3 scripts/import-smoke-test.py --profile pr

    # Run one curated pack explicitly
    python3 scripts/import-smoke-test.py --pack fabulously-optimized_1.20.1_curseforge_fabric

    # Discovery survey across default MC versions (no download)
    python3 scripts/import-smoke-test.py --discover-only

    # Full survey with import testing (runs empack init --from on discovered packs)
    python3 scripts/import-smoke-test.py --import-test

    # Narrow to specific versions, fewer packs per version
    python3 scripts/import-smoke-test.py --mc-versions 1.20.1,1.16.5 --limit 3

    # Re-analyze cached packs without re-downloading
    python3 scripts/import-smoke-test.py --skip-download --import-test

    # Use a specific empack binary
    python3 scripts/import-smoke-test.py --import-test --empack-bin ./target/release/empack

Curated mode phases:
    1. Resolve the latest compatible artifact for each curated pack
    2. Download archives to /tmp/empack-curated-smoke/packs/ (cached)
    3. Run `empack init --from ... --yes`
    4. Run `empack build client-full`
    5. If needed, download restricted CurseForge files into the managed cache
       and resume with `empack build --continue`
    6. Record build results and output artifact paths

Survey mode phases:
    1. Discover: query Modrinth + CurseForge for top modpacks per MC version
    2. Resolve:  find download URLs for each pack
    3. Download: fetch archives to /tmp/empack-survey/packs/ (cached)
    4. Analyze:  inspect archive contents for structural patterns
       - Content routing (mods, resourcepacks, shaderpacks, datapacks)
       - Datapack loader detection (Paxi, Open Loader, Global Packs)
       - Override directory structure
       - (with --import-test) Run empack init --from and record results

Outputs:
    /tmp/empack-curated-smoke/       Curated smoke downloads, projects, report
    /tmp/empack-survey/              Survey downloads, projects, report
"""

import argparse
import hashlib
import json
import os
import re
import select
import shutil
import socket
import string
import subprocess
import sys
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

SURVEY_ROOT_DIR = Path("/tmp/empack-survey")
CURATED_ROOT_DIR = Path("/tmp/empack-curated-smoke")

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


def curseforge_post(path: str, payload: dict, timeout: int = 30) -> dict:
    url = f"{CURSEFORGE_API}{path}"
    req = Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "x-api-key": CURSEFORGE_API_KEY,
            "Accept": "application/json",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    with urlopen(req, timeout=timeout) as resp:
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


@dataclass
class CuratedPack:
    name: str
    source: str
    mc_version: str
    loader: str
    project_id: str
    slug: str
    expect_restricted_continue: bool = False


@dataclass(frozen=True)
class RuntimeLayout:
    root_dir: Path
    packs_dir: Path
    projects_dir: Path
    cache_dir: Path
    report_path: Path

    def ensure_dirs(self) -> None:
        self.root_dir.mkdir(parents=True, exist_ok=True)
        self.packs_dir.mkdir(parents=True, exist_ok=True)
        self.projects_dir.mkdir(parents=True, exist_ok=True)
        self.cache_dir.mkdir(parents=True, exist_ok=True)


@dataclass
class CommandResult:
    success: bool = False
    exit_code: int = -1
    stdout: str = ""
    stderr: str = ""
    warnings: list = field(default_factory=list)
    elapsed_secs: float = 0.0


@dataclass
class CuratedBuildResult:
    initial_success: bool = False
    continue_required: bool = False
    restricted_mod_count: int = 0
    restricted_cache_dir: str = ""
    continue_success: bool = False
    artifact_path: str = ""
    elapsed_secs: float = 0.0
    warnings: list = field(default_factory=list)
    failed_download_file_ids: list = field(default_factory=list)


# Discovery snapshot: 2026-04-08.
# Selection criteria:
#   - one proven legacy Forge baseline on CurseForge
#   - six modern 1.20.1 packs covering CF+Modrinth across Fabric/Quilt/NeoForge
#   - small enough to remain smoke-testable
#   - popular enough to represent real user paths
# Intentionally excluded:
#   - Modrinth Technical Electrical (too large and resolved as Forge rather than NeoForge)
#   - CurseForge X-RAY Unlimited Quilt (too many restricted files)
#   - Heavier NeoForge CF packs like Prominence Classic (too expensive for smoke use)
#   - CurseForge Vanilla Forge (current 1.20.x NeoForge file advertises 1.20.1
#     in metadata but downloads a 1.20.6 manifest, so it is not stable enough
#     for the curated golden path)
CURATED_GOLDEN_PACKS = [
    CuratedPack(
        name="Crash Landing",
        source="curseforge",
        mc_version="1.6.4",
        loader="forge",
        project_id="229330",
        slug="crash-landing",
    ),
    CuratedPack(
        name="Fabulously Optimized",
        source="modrinth",
        mc_version="1.20.1",
        loader="fabric",
        project_id="1KVo5zza",
        slug="fabulously-optimized",
    ),
    CuratedPack(
        name="Sodium Plus",
        source="modrinth",
        mc_version="1.20.1",
        loader="quilt",
        project_id="ch7UHY2J",
        slug="sodiumplus",
    ),
    CuratedPack(
        name="Boosted FPS (Performance Optimized) (QLT)",
        source="curseforge",
        mc_version="1.20.1",
        loader="quilt",
        project_id="982068",
        slug="boosted-fps-quilt",
    ),
    CuratedPack(
        name="Wither Storm Enhanced",
        source="modrinth",
        mc_version="1.20.1",
        loader="neoforge",
        project_id="7kO3Tbz7",
        slug="wither-storm-enhanced",
    ),
    CuratedPack(
        name="Simple voice chat + Sound Physics Remastered (Modpack)",
        source="curseforge",
        mc_version="1.20.1",
        loader="neoforge",
        project_id="572641",
        slug="simple-voice-chat-sound-physics-remastered-modpack",
    ),
    CuratedPack(
        name="Fabulously Optimized",
        source="curseforge",
        mc_version="1.20.1",
        loader="fabric",
        project_id="396246",
        slug="fabulously-optimized",
        expect_restricted_continue=True,
    ),
]

CURATED_PACKS_BY_ID = {
    f"{pack.slug}_{pack.mc_version}_{pack.source}_{pack.loader}": pack
    for pack in CURATED_GOLDEN_PACKS
}


def curated_pack_id(pack: CuratedPack) -> str:
    return f"{pack.slug}_{pack.mc_version}_{pack.source}_{pack.loader}"


def build_runtime_layout(root_dir: Path) -> RuntimeLayout:
    return RuntimeLayout(
        root_dir=root_dir,
        packs_dir=root_dir / "packs",
        projects_dir=root_dir / "projects",
        cache_dir=root_dir / "cache",
        report_path=root_dir / "report.json",
    )


def survey_layout() -> RuntimeLayout:
    return build_runtime_layout(SURVEY_ROOT_DIR)


def curated_layout() -> RuntimeLayout:
    return build_runtime_layout(CURATED_ROOT_DIR)


def current_ci_profile_pack() -> CuratedPack:
    if sys.platform.startswith("win"):
        return CURATED_PACKS_BY_ID["fabulously-optimized_1.20.1_curseforge_fabric"]
    return CURATED_PACKS_BY_ID["fabulously-optimized_1.20.1_modrinth_fabric"]


def survey_flags_requested(args) -> bool:
    return (
        args.discover_only
        or args.skip_download
        or args.import_test
        or args.mc_versions is not None
        or args.all_versions
        or args.limit != 5
    )


def select_curated_packs(args) -> list[CuratedPack]:
    if args.pack:
        return [CURATED_PACKS_BY_ID[args.pack]]
    if args.profile == "pr":
        return [current_ci_profile_pack()]
    return CURATED_GOLDEN_PACKS


def determine_mode(args) -> str:
    if args.clean:
        return "clean"
    if survey_flags_requested(args):
        return "survey"
    return "curated"


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

def download_pack(c: PackCandidate, layout: RuntimeLayout, timeout: int = 60) -> bool:
    safe_name = f"{c.slug}_{c.mc_version}_{c.source}"
    ext = ".mrpack" if c.format == "mrpack" else ".zip"
    dest = layout.packs_dir / f"{safe_name}{ext}"

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


def extract_warning_lines(stdout: str, stderr: str) -> list[str]:
    warnings = []
    combined = stdout + ("\n" if stdout and stderr else "") + stderr
    for line in combined.splitlines():
        clean = re.sub(r"\x1b\[[0-9;]*m", "", line.strip())
        if should_echo_live_line(clean):
            warnings.append(clean)
    return warnings


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


def run_command_posix_live(
    cmd: list[str],
    env: dict[str, str],
    timeout: int,
    label: str,
    cwd: Optional[Path] = None,
) -> CommandResult:
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
            cwd=str(cwd) if cwd is not None else None,
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
                return CommandResult(
                    success=False,
                    exit_code=-1,
                    stderr=f"TIMEOUT after {timeout}s",
                    elapsed_secs=time.time() - start,
                )

            ready, _, _ = select.select([master_fd], [], [], 0.2)
            if not ready:
                continue

            try:
                data = os.read(master_fd, 4096)
            except OSError:
                break

            if not data:
                break

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
    return CommandResult(
        success=proc.returncode == 0,
        exit_code=proc.returncode,
        stdout=combined,
        stderr="",
        warnings=extract_warning_lines(combined, ""),
        elapsed_secs=time.time() - start,
    )


def run_command(
    cmd: list[str],
    env: dict[str, str],
    timeout: int,
    label: str,
    cwd: Optional[Path] = None,
    prefer_pty: bool = True,
) -> CommandResult:
    start = time.time()
    if prefer_pty and os.name == "posix" and pty is not None:
        return run_command_posix_live(cmd=cmd, env=env, timeout=timeout, label=label, cwd=cwd)

    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd=str(cwd) if cwd is not None else None,
        )
    except subprocess.TimeoutExpired:
        return CommandResult(
            success=False,
            exit_code=-1,
            stderr=f"TIMEOUT after {timeout}s",
            elapsed_secs=time.time() - start,
        )

    return CommandResult(
        success=proc.returncode == 0,
        exit_code=proc.returncode,
        stdout=proc.stdout,
        stderr=proc.stderr,
        warnings=extract_warning_lines(proc.stdout, proc.stderr),
        elapsed_secs=time.time() - start,
    )


def ensure_empack_env(
    env: dict[str, str],
    layout: RuntimeLayout,
    timeout_secs: int,
) -> dict[str, str]:
    configured = env.copy()
    configured["EMPACK_KEY_CURSEFORGE"] = CURSEFORGE_API_KEY
    configured["EMPACK_CACHE_DIR"] = str(layout.cache_dir)
    configured["EMPACK_PROCESS_TIMEOUT_SECS"] = str(timeout_secs)
    configured["NO_COLOR"] = "1"

    if os.name == "nt":
        local_app_data = layout.root_dir / ".windows-localappdata"
        roaming_app_data = layout.root_dir / ".windows-appdata"
        user_profile = layout.root_dir / ".windows-userprofile"
        temp_dir = layout.root_dir / ".windows-temp"
        for path in [local_app_data, roaming_app_data, user_profile, temp_dir]:
            path.mkdir(parents=True, exist_ok=True)

        configured.setdefault("LOCALAPPDATA", str(local_app_data))
        configured.setdefault("LocalAppData", str(local_app_data))
        configured.setdefault("APPDATA", str(roaming_app_data))
        configured.setdefault("USERPROFILE", str(user_profile))
        configured.setdefault("TEMP", str(temp_dir))
        configured.setdefault("TMP", str(temp_dir))

    return configured


def run_empack_command(
    empack_bin: Path,
    args: list[str],
    layout: RuntimeLayout,
    timeout: int,
    label: str,
    cwd: Optional[Path] = None,
    prefer_pty: bool = True,
) -> CommandResult:
    return run_command(
        [str(empack_bin), *args],
        env=ensure_empack_env(os.environ.copy(), layout, timeout),
        timeout=timeout,
        label=label,
        cwd=cwd,
        prefer_pty=prefer_pty,
    )


def run_import_test(
    pack_path: Path,
    project_name: str,
    empack_bin: Path,
    layout: RuntimeLayout,
) -> ImportResult:
    project_dir = layout.projects_dir / project_name
    if project_dir.exists():
        shutil.rmtree(project_dir)

    command = run_empack_command(
        empack_bin,
        ["init", "--from", str(pack_path), "--yes", str(project_dir)],
        layout,
        timeout=600,
        label=project_name,
    )

    r = parse_import_output(command.stdout, command.stderr)
    r.success = command.success
    r.exit_code = command.exit_code
    r.stdout = command.stdout
    r.stderr = command.stderr
    r.warnings.extend(command.warnings)
    return r


def modrinth_loader_key(loader: str) -> str:
    return {
        "fabric": "fabric-loader",
        "quilt": "quilt-loader",
        "forge": "forge",
        "neoforge": "neoforge",
    }[loader]


def curseforge_loader_label(loader: str) -> str:
    return {
        "fabric": "Fabric",
        "quilt": "Quilt",
        "forge": "Forge",
        "neoforge": "NeoForge",
    }[loader]


def curated_project_name(pack: CuratedPack) -> str:
    return curated_pack_id(pack)


def resolve_curated_pack(pack: CuratedPack) -> PackCandidate:
    if pack.source == "modrinth":
        versions = modrinth_get(f"/project/{pack.project_id}/version")
        compatible = []
        for version in versions:
            if pack.mc_version not in version.get("game_versions", []):
                continue
            if pack.loader not in version.get("loaders", []):
                continue
            mrpack_files = [
                f for f in version.get("files", [])
                if f.get("filename", "").endswith(".mrpack")
            ]
            if not mrpack_files:
                continue
            chosen_file = next((f for f in mrpack_files if f.get("primary")), mrpack_files[0])
            compatible.append((version, chosen_file))

        if not compatible:
            raise RuntimeError(
                f"no compatible Modrinth mrpack found for {pack.name} "
                f"({pack.mc_version} {pack.loader})"
            )

        compatible.sort(
            key=lambda pair: (
                bool(pair[0].get("featured")),
                pair[0].get("date_published", ""),
            ),
            reverse=True,
        )
        version, chosen_file = compatible[0]
        return PackCandidate(
            name=pack.name,
            mc_version=pack.mc_version,
            source=pack.source,
            slug=pack.slug,
            project_id=pack.project_id,
            downloads=version.get("downloads", 0),
            loader=pack.loader,
            file_url=chosen_file["url"],
            file_id=version["id"],
            file_size=chosen_file.get("size"),
            format="mrpack",
        )

    files = curseforge_get(
        f"/mods/{pack.project_id}/files",
        {"gameVersion": pack.mc_version, "pageSize": "50"},
    ).get("data", [])
    loader_label = curseforge_loader_label(pack.loader)
    compatible = [
        f for f in files
        if pack.mc_version in f.get("gameVersions", [])
        and loader_label in f.get("gameVersions", [])
    ]
    if not compatible:
        raise RuntimeError(
            f"no compatible CurseForge file found for {pack.name} "
            f"({pack.mc_version} {pack.loader})"
        )

    compatible.sort(key=lambda file: file.get("fileDate", ""), reverse=True)
    chosen = compatible[0]
    return PackCandidate(
        name=pack.name,
        mc_version=pack.mc_version,
        source=pack.source,
        slug=pack.slug,
        project_id=pack.project_id,
        downloads=chosen.get("downloadCount", 0),
        loader=pack.loader,
        file_id=str(chosen["id"]),
        file_size=chosen.get("fileLength"),
        format="cfzip",
    )


def verify_curated_download(pack: CuratedPack, archive_path: Path) -> None:
    with zipfile.ZipFile(archive_path) as zf:
        if pack.source == "modrinth":
            manifest = json.loads(zf.read("modrinth.index.json"))
            dependencies = manifest.get("dependencies", {})
            found_mc = dependencies.get("minecraft")
            found_loader = dependencies.get(modrinth_loader_key(pack.loader))
            if found_mc != pack.mc_version:
                raise RuntimeError(
                    f"downloaded mrpack reports minecraft={found_mc!r}, expected {pack.mc_version!r}"
                )
            if not found_loader:
                raise RuntimeError(
                    f"downloaded mrpack is missing loader dependency {modrinth_loader_key(pack.loader)!r}"
                )
            return

        manifest = json.loads(zf.read("manifest.json"))
        found_mc = manifest.get("minecraft", {}).get("version")
        if found_mc != pack.mc_version:
            raise RuntimeError(
                f"downloaded CurseForge zip reports minecraft={found_mc!r}, expected {pack.mc_version!r}"
            )
        modloaders = manifest.get("minecraft", {}).get("modLoaders", [])
        if not any(
            loader.get("id", "").lower().startswith(pack.loader)
            for loader in modloaders
        ):
            raise RuntimeError(
                f"downloaded CurseForge zip reports loaders={modloaders!r}, expected {pack.loader!r}"
            )


def find_client_full_artifact(project_dir: Path) -> Optional[Path]:
    dist_dir = project_dir / "dist"
    if not dist_dir.exists():
        return None
    candidates = sorted(
        [
            path for path in dist_dir.iterdir()
            if path.is_file() and path.name.endswith("-client-full.zip")
        ],
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    return candidates[0] if candidates else None


def load_pending_restricted_state(project_dir: Path) -> Optional[dict]:
    state_path = project_dir / ".empack-build-continue.json"
    if not state_path.exists():
        return None
    return json.loads(state_path.read_text())


def wait_for_pending_restricted_state(
    project_dir: Path,
    timeout_secs: float = 2.0,
    poll_interval_secs: float = 0.1,
) -> Optional[dict]:
    deadline = time.time() + timeout_secs
    seen_paths = []
    while time.time() <= deadline:
        for candidate_dir in [project_dir, project_dir.resolve()]:
            if candidate_dir in seen_paths:
                continue
            seen_paths.append(candidate_dir)
        for candidate_dir in seen_paths:
            pending = load_pending_restricted_state(candidate_dir)
            if pending is not None:
                return pending
        time.sleep(poll_interval_secs)
    return None


def parse_curseforge_file_id(url: str) -> Optional[int]:
    match = re.search(r"/(?:files|download)/(\d+)(?:/|$)", url)
    return int(match.group(1)) if match else None


def restricted_cache_dir_for_project(cache_dir: Path, project_dir: Path) -> Path:
    project_hash = hashlib.sha256(str(project_dir.resolve()).encode("utf-8")).hexdigest()
    return cache_dir / "restricted-builds" / project_hash


def clear_restricted_cache_for_project(cache_dir: Path, project_dir: Path):
    restricted_dir = restricted_cache_dir_for_project(cache_dir, project_dir)
    if restricted_dir.exists():
        shutil.rmtree(restricted_dir)


def download_restricted_files(pending_state: dict, timeout: int) -> list[int]:
    entries = pending_state.get("entries", [])
    file_ids = sorted({
        parse_curseforge_file_id(entry.get("url", ""))
        for entry in entries
        if parse_curseforge_file_id(entry.get("url", "")) is not None
    })
    if not file_ids:
        return []

    response = curseforge_post("/mods/files", {"fileIds": file_ids}, timeout=timeout)
    file_map = {item["id"]: item["modId"] for item in response.get("data", [])}
    cache_dir = Path(pending_state["restricted_cache_dir"])
    cache_dir.mkdir(parents=True, exist_ok=True)

    failed = []
    for entry in entries:
        file_id = parse_curseforge_file_id(entry.get("url", ""))
        if file_id is None:
            failed.append(-1)
            continue
        mod_id = file_map.get(file_id)
        if mod_id is None:
            failed.append(file_id)
            continue

        dest = cache_dir / entry["filename"]
        try:
            req = Request(
                f"https://www.curseforge.com/api/v1/mods/{mod_id}/files/{file_id}/download",
                headers={"User-Agent": "empack-survey/1.0"},
            )
            with urlopen(req, timeout=timeout) as resp:
                dest.write_bytes(resp.read())
        except (HTTPError, URLError, socket.timeout):
            failed.append(file_id)

    return sorted(set(failed))


def run_curated_build(
    pack: CuratedPack,
    project_dir: Path,
    empack_bin: Path,
    layout: RuntimeLayout,
    timeout: int,
    announce: bool,
) -> CuratedBuildResult:
    label = curated_project_name(pack)
    result = CuratedBuildResult()
    initial_attempts = 4
    pending_state = None

    for attempt in range(initial_attempts):
        initial = run_empack_command(
            empack_bin,
            ["build", "client-full"],
            layout,
            timeout=timeout,
            label=f"{label}:build",
            cwd=project_dir,
            prefer_pty=announce,
        )
        result.initial_success = initial.success
        result.elapsed_secs += initial.elapsed_secs
        result.warnings.extend(initial.warnings)

        if initial.success:
            artifact = find_client_full_artifact(project_dir)
            if artifact:
                result.artifact_path = str(artifact)
            return result

        pending_state = wait_for_pending_restricted_state(project_dir)
        if pending_state:
            break

        if attempt + 1 < initial_attempts:
            result.warnings.append(
                "initial client-full build failed before pending restricted state was written; retrying"
            )
            time.sleep(1)

    if not pending_state:
        return result

    result.continue_required = True
    result.restricted_mod_count = len(pending_state.get("entries", []))
    result.restricted_cache_dir = pending_state.get("restricted_cache_dir", "")

    failed_ids = download_restricted_files(pending_state, timeout)
    result.failed_download_file_ids = failed_ids
    if failed_ids:
        result.warnings.append(f"failed restricted downloads: {failed_ids}")
        return result

    continued = run_empack_command(
        empack_bin,
        ["build", "--continue"],
        layout,
        timeout=timeout,
        label=f"{label}:continue",
        cwd=project_dir,
        prefer_pty=announce,
    )
    result.continue_success = continued.success
    result.elapsed_secs += continued.elapsed_secs
    result.warnings.extend(continued.warnings)
    artifact = find_client_full_artifact(project_dir)
    if artifact:
        result.artifact_path = str(artifact)
    if (
        pack.expect_restricted_continue
        and result.continue_required
        and not (result.initial_success or result.continue_success)
    ):
        fallback_result = _run_curated_build_raw_fallback(
            project_dir,
            empack_bin,
            layout,
            timeout,
        )
        fallback_result.elapsed_secs += result.elapsed_secs
        fallback_result.warnings = result.warnings + fallback_result.warnings
        return fallback_result
    return result


def _run_curated_build_raw_fallback(
    project_dir: Path,
    empack_bin: Path,
    layout: RuntimeLayout,
    timeout: int,
) -> CuratedBuildResult:
    env = ensure_empack_env(os.environ.copy(), layout, timeout)
    start = time.time()
    try:
        proc = subprocess.run(
            [str(empack_bin), "build", "client-full"],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd=str(project_dir),
        )
    except subprocess.TimeoutExpired:
        return CuratedBuildResult(
            initial_success=False,
            elapsed_secs=time.time() - start,
            warnings=[f"TIMEOUT after {timeout}s"],
        )
    result = CuratedBuildResult(
        initial_success=proc.returncode == 0,
        elapsed_secs=time.time() - start,
        warnings=extract_warning_lines(proc.stdout, proc.stderr),
    )
    if result.initial_success:
        artifact = find_client_full_artifact(project_dir)
        if artifact:
            result.artifact_path = str(artifact)
        return result

    pending_state = wait_for_pending_restricted_state(project_dir)
    if not pending_state:
        return result

    result.continue_required = True
    result.restricted_mod_count = len(pending_state.get("entries", []))
    result.restricted_cache_dir = pending_state.get("restricted_cache_dir", "")

    failed_ids = download_restricted_files(pending_state, timeout)
    result.failed_download_file_ids = failed_ids
    if failed_ids:
        result.warnings.append(f"failed restricted downloads: {failed_ids}")
        return result

    continue_start = time.time()
    try:
        continue_proc = subprocess.run(
            [str(empack_bin), "build", "--continue"],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            cwd=str(project_dir),
        )
    except subprocess.TimeoutExpired:
        result.warnings.append(f"TIMEOUT after {timeout}s")
        result.elapsed_secs += time.time() - continue_start
        return result
    result.continue_success = continue_proc.returncode == 0
    result.elapsed_secs += time.time() - continue_start
    result.warnings.extend(extract_warning_lines(continue_proc.stdout, continue_proc.stderr))
    artifact = find_client_full_artifact(project_dir)
    if artifact:
        result.artifact_path = str(artifact)
    return result


def save_curated_report(entries: list[dict], layout: RuntimeLayout):
    layout.report_path.write_text(json.dumps(entries, indent=2))
    print(f"\nReport saved to {layout.report_path}")


def run_single_curated_pack(
    pack: CuratedPack,
    empack_bin: Path,
    download_timeout: int,
    layout: RuntimeLayout,
    index: Optional[int] = None,
    total: Optional[int] = None,
    announce: bool = True,
) -> tuple[dict, Optional[str], bool]:
    layout.ensure_dirs()

    label = curated_project_name(pack)
    project_dir = layout.projects_dir / label
    if project_dir.exists():
        shutil.rmtree(project_dir)
    if pack.expect_restricted_continue:
        clear_restricted_cache_for_project(layout.cache_dir, project_dir)

    entry = {
        "pack": asdict(pack),
        "resolved": {},
        "import_result": {},
        "build_result": {},
    }

    try:
        candidate = resolve_curated_pack(pack)
        entry["resolved"] = asdict(candidate)
        if announce:
            prefix = (
                f"  [{index}/{total}] "
                if index is not None and total is not None
                else "  "
            )
            print(
                f"{prefix}{pack.name} [{pack.source} {pack.loader} {pack.mc_version}]"
            )
        if not download_pack(candidate, layout, timeout=download_timeout):
            raise RuntimeError("download failed")
        verify_curated_download(pack, Path(candidate.local_path))

        init_result = run_empack_command(
            empack_bin,
            ["init", "--from", candidate.local_path, "--yes", str(project_dir)],
            layout,
            timeout=600,
            label=f"{label}:init",
            prefer_pty=announce,
        )
        entry["import_result"] = {
            "success": init_result.success,
            "exit_code": init_result.exit_code,
            "elapsed_secs": init_result.elapsed_secs,
            "warnings": init_result.warnings[:20],
        }
        if not init_result.success:
            return entry, "init failed", False

        build_result = run_curated_build(
            pack,
            project_dir,
            empack_bin,
            layout,
            600,
            announce,
        )
        entry["build_result"] = asdict(build_result)

        did_continue = build_result.continue_required
        if not (build_result.initial_success or build_result.continue_success):
            return entry, "build failed", did_continue
        if not build_result.artifact_path:
            return entry, "artifact missing", did_continue
        return entry, None, did_continue
    except Exception as exc:
        entry["import_result"] = {
            "success": False,
            "exit_code": -1,
            "elapsed_secs": 0.0,
            "warnings": [str(exc)],
        }
        return entry, str(exc), False


def run_curated_mode(empack_bin: Path, args, layout: RuntimeLayout) -> int:
    layout.ensure_dirs()
    selected_packs = select_curated_packs(args)

    results = []
    failures = []
    actual_continue = []
    expected_continue = {
        curated_project_name(pack)
        for pack in selected_packs
        if pack.expect_restricted_continue
    }

    if args.pack:
        print(f"Curated mode: running curated pack '{args.pack}'")
    elif args.profile == "pr":
        print("Curated mode: running platform-selected PR smoke profile")
    else:
        print(f"Curated mode: running {len(selected_packs)} golden import/build flows")

    for index, pack in enumerate(selected_packs, start=1):
        label = curated_project_name(pack)
        try:
            entry, failure_reason, did_continue = run_single_curated_pack(
                pack,
                empack_bin,
                args.download_timeout,
                layout,
                index=index,
                total=len(selected_packs),
                announce=True,
            )
        except Exception as exc:
            entry = {
                "pack": asdict(pack),
                "resolved": {},
                "import_result": {
                    "success": False,
                    "exit_code": -1,
                    "elapsed_secs": 0.0,
                    "warnings": [str(exc)],
                },
                "build_result": {},
            }
            failure_reason = str(exc)
            did_continue = False

        if did_continue:
            actual_continue.append(label)
        if failure_reason:
            failures.append((label, failure_reason))
        results.append(entry)

    save_curated_report(results, layout)

    actual_continue = set(actual_continue)
    if actual_continue != expected_continue:
        print(
            "\nCurated continuation mismatch:",
            f"expected {sorted(expected_continue)} but observed {sorted(actual_continue)}",
            file=sys.stderr,
        )
        return 1

    if failures:
        print("\nCurated smoke failures:", file=sys.stderr)
        for label, reason in failures:
            print(f"  {label}: {reason}", file=sys.stderr)
        return 1

    print("\nCurated smoke completed successfully.")
    return 0


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


def save_report(
    analyses: list[tuple[PackCandidate, PackAnalysis, ImportResult | None]],
    layout: RuntimeLayout,
):
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

    layout.report_path.write_text(json.dumps(report, indent=2))
    print(f"\nReport saved to {layout.report_path}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def find_empack_bin() -> Path:
    script_dir = Path(__file__).resolve().parent
    project_root = script_dir.parent
    exe_name = "empack.exe" if os.name == "nt" else "empack"
    debug = project_root / "target" / "debug" / exe_name
    release = project_root / "target" / "release" / exe_name

    # Prefer the workspace debug build. This script is a development smoke runner
    # and should validate the binary we just built, not a potentially stale release
    # artifact left behind in target/release.
    if debug.exists():
        return debug
    if release.exists():
        return release
    # fall back to PATH
    which = shutil.which("empack")
    if which:
        return Path(which)
    return debug  # will fail at runtime with a clear path


def run_clean_mode(args) -> None:
    layouts = [survey_layout(), curated_layout()]
    if args.clean == "all":
        for layout in layouts:
            if layout.root_dir.exists():
                shutil.rmtree(layout.root_dir)
                print(f"Removed {layout.root_dir}")
        return

    for layout in layouts:
        if layout.projects_dir.exists():
            shutil.rmtree(layout.projects_dir)
            print(f"Removed {layout.projects_dir} (cached packs kept)")


def run_internal_curated_pack(empack_bin: Path, args) -> None:
    layout = curated_layout()
    layout.ensure_dirs()

    pack = CURATED_PACKS_BY_ID.get(args.internal_curated_pack)
    if pack is None:
        raise SystemExit(2)

    entry, failure_reason, did_continue = run_single_curated_pack(
        pack,
        empack_bin,
        args.download_timeout,
        layout,
        announce=False,
    )
    print(
        json.dumps(
            {
                "entry": entry,
                "failure_reason": failure_reason,
                "did_continue": did_continue,
            }
        )
    )


def run_survey_mode(empack_bin: Path, args, layout: RuntimeLayout) -> None:
    max_file_size_bytes = args.max_file_size * 1024 * 1024
    if args.mc_versions:
        mc_versions = args.mc_versions.split(",")
    elif args.all_versions:
        mc_versions = ALL_MC_VERSIONS
    else:
        mc_versions = DEFAULT_MC_VERSIONS

    layout.ensure_dirs()

    fetch_limit = args.limit * 2
    print(
        f"Phase 1: Discovering modpacks (fetching {fetch_limit} per platform for backfill buffer)..."
    )
    all_candidates = []
    for ver in mc_versions:
        print(f"  MC {ver}...")
        mr = discover_modrinth(ver, fetch_limit)
        cf = discover_curseforge(ver, fetch_limit)
        all_candidates.extend(mr)
        all_candidates.extend(cf)
        time.sleep(0.5)

    seen_names: dict[tuple[str, str], PackCandidate] = {}
    for candidate in all_candidates:
        key = (normalize_name(candidate.name), candidate.mc_version)
        if key not in seen_names:
            seen_names[key] = candidate
        elif candidate.source == "modrinth" and seen_names[key].source == "curseforge":
            seen_names[key] = candidate

    seen_slugs: set[tuple[str, str]] = set()
    deduped_all: list[PackCandidate] = []
    for candidate in seen_names.values():
        slug_key = (
            candidate.slug.lower().replace("-", "").replace(" ", ""),
            candidate.mc_version,
        )
        if slug_key not in seen_slugs:
            seen_slugs.add(slug_key)
            deduped_all.append(candidate)

    deduped_all.sort(
        key=lambda candidate: (
            -DEFAULT_MC_VERSIONS.index(candidate.mc_version)
            if candidate.mc_version in DEFAULT_MC_VERSIONS
            else -99,
            -candidate.downloads,
        )
    )

    by_version: dict[str, list[PackCandidate]] = {}
    for candidate in deduped_all:
        by_version.setdefault(candidate.mc_version, []).append(candidate)

    primary: list[PackCandidate] = []
    buffer: list[PackCandidate] = []
    for ver in mc_versions:
        version_candidates = by_version.get(ver, [])
        version_candidates.sort(key=lambda candidate: -candidate.downloads)
        primary.extend(version_candidates[:args.limit])
        buffer.extend(version_candidates[args.limit:])

    deduped = primary
    total_discovered = len(deduped) + len(buffer)
    print(
        f"  Found {total_discovered} unique packs ({len(deduped)} primary + {len(buffer)} buffer) across {len(mc_versions)} MC versions"
    )
    print_discovery(deduped)

    if args.discover_only:
        return

    print("\nPhase 2: Resolving download URLs...")
    resolved = []
    skipped_resolve = 0
    for candidate in deduped:
        ok = resolve_download_url(candidate)
        if ok:
            if (
                candidate.file_size is not None
                and candidate.file_size > max_file_size_bytes
            ):
                size_mb = candidate.file_size / (1024 * 1024)
                print(
                    f"  SKIP {candidate.name} ({candidate.source}): {size_mb:.0f}MB exceeds --max-file-size {args.max_file_size}MB",
                    file=sys.stderr,
                )
                skipped_resolve += 1
            else:
                resolved.append(candidate)
        else:
            print(
                f"  SKIP {candidate.name} ({candidate.source}): no download URL",
                file=sys.stderr,
            )
            skipped_resolve += 1
        time.sleep(0.3)

    if skipped_resolve > 0 and buffer:
        print(f"  Backfilling {min(skipped_resolve, len(buffer))} candidates from buffer...")
        resolved_filenames: set[str] = set()
        for candidate in resolved:
            filename = filename_from_url(candidate.file_url)
            if filename:
                resolved_filenames.add(filename)

        backfilled = 0
        while backfilled < skipped_resolve and buffer:
            candidate = buffer.pop(0)
            ok = resolve_download_url(candidate)
            if not ok:
                continue
            if (
                candidate.file_size is not None
                and candidate.file_size > max_file_size_bytes
            ):
                continue
            filename = filename_from_url(candidate.file_url)
            if filename and filename in resolved_filenames:
                continue
            if filename:
                resolved_filenames.add(filename)
            resolved.append(candidate)
            backfilled += 1
            time.sleep(0.3)
        if backfilled:
            print(f"  Backfilled {backfilled} packs from buffer")

    seen_filenames: set[str] = set()
    filename_deduped: list[PackCandidate] = []
    for candidate in resolved:
        filename = filename_from_url(candidate.file_url)
        if filename and filename in seen_filenames:
            print(
                f"  DEDUP {candidate.name} ({candidate.source}): duplicate filename {filename}",
                file=sys.stderr,
            )
            continue
        if filename:
            seen_filenames.add(filename)
        filename_deduped.append(candidate)
    resolved = filename_deduped

    print(f"  Resolved {len(resolved)}/{len(deduped)} packs")

    if not args.skip_download:
        print(
            f"\nPhase 3: Downloading (timeout={args.download_timeout}s per pack)..."
        )
        downloaded = []
        skipped_download = 0
        for index, candidate in enumerate(resolved, start=1):
            safe = f"{candidate.slug}_{candidate.mc_version}_{candidate.source}"
            ext = ".mrpack" if candidate.format == "mrpack" else ".zip"
            existing = layout.packs_dir / f"{safe}{ext}"
            if existing.exists() and existing.stat().st_size > 0:
                candidate.local_path = str(existing)
                downloaded.append(candidate)
                print(f"  [{index}/{len(resolved)}] {candidate.name}: cached")
                continue

            print(f"  [{index}/{len(resolved)}] {candidate.name}...", end=" ", flush=True)
            ok = download_pack(candidate, layout, timeout=args.download_timeout)
            if ok:
                size_mb = Path(candidate.local_path).stat().st_size / (1024 * 1024)
                print(f"{size_mb:.1f}MB")
                downloaded.append(candidate)
            else:
                print("FAILED (timeout or error)")
                skipped_download += 1
            time.sleep(0.3)

        if skipped_download > 0 and buffer:
            print(
                f"  Backfilling {min(skipped_download, len(buffer))} candidates from buffer for download failures..."
            )
            dl_backfilled = 0
            while dl_backfilled < skipped_download and buffer:
                candidate = buffer.pop(0)
                if not candidate.file_url:
                    ok = resolve_download_url(candidate)
                    if not ok:
                        continue
                    if (
                        candidate.file_size is not None
                        and candidate.file_size > max_file_size_bytes
                    ):
                        continue
                print(f"  [backfill] {candidate.name}...", end=" ", flush=True)
                ok = download_pack(candidate, layout, timeout=args.download_timeout)
                if ok:
                    size_mb = Path(candidate.local_path).stat().st_size / (1024 * 1024)
                    print(f"{size_mb:.1f}MB")
                    downloaded.append(candidate)
                    dl_backfilled += 1
                else:
                    print("FAILED")
                time.sleep(0.3)
            if dl_backfilled:
                print(f"  Backfilled {dl_backfilled} packs from buffer")

        print(f"  Downloaded {len(downloaded)}/{len(resolved)} packs")
    else:
        downloaded = [candidate for candidate in resolved if candidate.local_path and Path(candidate.local_path).exists()]
        for archive in layout.packs_dir.iterdir():
            if archive.suffix in (".mrpack", ".zip") and archive.stat().st_size > 0:
                parts = archive.stem.split("_")
                if len(parts) >= 3:
                    existing = [
                        candidate
                        for candidate in downloaded
                        if candidate.slug == parts[0] and candidate.mc_version == parts[1]
                    ]
                    if not existing:
                        source = parts[-1]
                        fmt = "mrpack" if archive.suffix == ".mrpack" else "cfzip"
                        downloaded.append(
                            PackCandidate(
                                name=parts[0],
                                mc_version=parts[1],
                                source=source,
                                slug=parts[0],
                                project_id="",
                                downloads=0,
                                loader="unknown",
                                format=fmt,
                                local_path=str(archive),
                            )
                        )

    print("\nPhase 4: Analyzing archives...")

    analyses = []
    for candidate in downloaded:
        path = Path(candidate.local_path)
        if candidate.format == "mrpack" or path.suffix == ".mrpack":
            analysis = analyze_mrpack(path)
        else:
            analysis = analyze_cfzip(path)

        analysis.name = analysis.name or candidate.name
        analysis.mc_version = analysis.mc_version or candidate.mc_version
        analysis.source = candidate.source
        analysis.slug = candidate.slug
        analysis.loader = analysis.loader or candidate.loader

        import_result = None
        if args.import_test:
            project_name = f"{candidate.slug}_{candidate.mc_version}_{candidate.source}"
            print(f"  import: {analysis.name}...", end=" ", flush=True)
            import_result = run_import_test(path, project_name, empack_bin, layout)
            if import_result.success:
                parts = [
                    f"OK refs={import_result.platform_refs_added} ovr={import_result.overrides_copied}"
                ]
                if import_result.warnings:
                    parts.append(f"warn={len(import_result.warnings)}")
                print(" ".join(parts))
            else:
                first_err = ""
                for warning in import_result.warnings:
                    if warning.startswith("Error:") or warning.startswith("Caused by:"):
                        first_err = warning
                        break
                if not first_err and import_result.warnings:
                    first_err = import_result.warnings[0]
                if not first_err:
                    first_err = (
                        import_result.stderr.strip().splitlines()[0]
                        if import_result.stderr.strip()
                        else "unknown error"
                    )
                print(f"FAIL exit={import_result.exit_code}: {first_err[:120]}")

        analyses.append((candidate, analysis, import_result))

    print_analysis_summary(analyses)
    save_report(analyses, layout)


def main():
    parser = argparse.ArgumentParser(description="empack import smoke test")
    parser.add_argument(
        "--profile",
        choices=["curated", "pr"],
        default=None,
        help="Curated smoke profile: full curated set or platform-specific PR smoke",
    )
    parser.add_argument(
        "--pack",
        choices=sorted(CURATED_PACKS_BY_ID),
        default=None,
        help="Run exactly one curated pack by id",
    )
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
                        help="Remove survey/curated projects or all smoke artifacts and exit")
    parser.add_argument("--download-timeout", type=int, default=60,
                        help="Per-pack download timeout in seconds (default: 60)")
    parser.add_argument("--max-file-size", type=int, default=100,
                        help="Max file size in MB; skip packs exceeding this (default: 100)")
    parser.add_argument("--internal-curated-pack", type=str, default=None,
                        help=argparse.SUPPRESS)
    args = parser.parse_args()

    if args.pack and args.profile is not None:
        parser.error("--pack cannot be combined with --profile")
    if survey_flags_requested(args) and (args.profile or args.pack):
        parser.error("--profile and --pack are only valid in curated mode")
    if args.clean and (args.profile or args.pack):
        parser.error("--profile and --pack cannot be combined with --clean")
    if args.internal_curated_pack and (args.profile or args.pack):
        parser.error("--internal-curated-pack cannot be combined with --profile or --pack")

    empack_bin = Path(args.empack_bin) if args.empack_bin else find_empack_bin()

    if args.internal_curated_pack:
        run_internal_curated_pack(empack_bin, args)
        return

    mode = determine_mode(args)
    if mode == "clean":
        run_clean_mode(args)
        return
    if mode == "curated":
        raise SystemExit(run_curated_mode(empack_bin, args, curated_layout()))

    run_survey_mode(empack_bin, args, survey_layout())


if __name__ == "__main__":
    main()
