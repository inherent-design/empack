"""Transport existing operator records; candidate admission remains in acceptance.py."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

from .acceptance import MAX_FILE, MAX_RECORD, bounded_file, declared_files, read_record, retained_files
from .evidence import sha256, write_json
from .release_inputs import release_version


def coordinates(revision, version, archive_digest):
    release_version(version)
    if (not re.fullmatch(r"[a-f0-9]{40}", revision)
            or not re.fullmatch(r"[a-f0-9]{64}", archive_digest)):
        raise ValueError("Acceptance transport requires exact source and archive identities")
    return f"acceptance-v{version}", f"acceptance-{archive_digest}-"


def context(record, revision, version, archive_digest):
    if (type(record.get("schema")) is not int or record["schema"] != 1 or record.get("kind") != "operator-acceptance"
            or record.get("source_revision") != revision or record.get("version") != version
            or record.get("archive_evidence_sha256") != archive_digest):
        raise ValueError("Operator record belongs to a different candidate")


def put(path, data):
    """Only create owned files, or reuse byte-identical content already staged."""
    if path.exists() or path.is_symlink():
        if path.is_symlink() or not path.is_file() or path.read_bytes() != data:
            raise ValueError("Acceptance transport would replace different evidence")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(data)


def stage(paths, output, revision, version, archive_digest):
    tag, prefix = coordinates(revision, version, archive_digest)
    paths = list(paths)
    if not paths:
        raise ValueError("Select at least one operator record to stage")
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    assets = set()
    for path in map(Path, paths):
        data, record = read_record(path)
        context(record, revision, version, archive_digest)
        files = retained_files(path, record)
        name = prefix + hashlib.sha256(data).hexdigest() + ".json"
        put(output / name, data)
        assets.add(name)
        for relative, identity in files.items():
            raw = bounded_file(path.parent / relative, MAX_FILE)
            if hashlib.sha256(raw).hexdigest() != identity["sha256"]:
                raise ValueError("Supporting evidence changed while staging")
            name = "observation-" + identity["sha256"]
            put(output / name, raw)
            assets.add(name)
    result = dict(kind="operator-evidence-staging", release_tag=tag, assets=sorted(assets),
                  source_revision=revision, version=version, archive_evidence_sha256=archive_digest,
                  scope="Existing record and supporting bytes only; no task or candidate qualification")
    write_json(output / "staging.json", result)
    return result


class GitHub:
    """Every request is GET; this helper cannot create, edit or publish a release."""
    def pages(self, endpoint):
        result = subprocess.run(["gh", "api", "--method", "GET", "--paginate", "--slurp", endpoint],
                                check=True, capture_output=True, text=True, timeout=120)
        pages = json.loads(result.stdout)
        if (not isinstance(pages, list) or any(not isinstance(page, list) for page in pages)
                or any(not isinstance(row, dict) for page in pages for row in page)):
            raise ValueError("Expected paginated GitHub release metadata")
        return [row for page in pages for row in page]

    def download(self, endpoint, path):
        with path.open("xb") as stream:
            subprocess.run(["gh", "api", "--method", "GET", "--header", "Accept: application/octet-stream", endpoint],
                           check=True, stdout=stream, timeout=120)


def collect(repository, output, revision, version, archive_digest, github=None):
    tag, prefix = coordinates(revision, version, archive_digest)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Expected an explicit GitHub owner/repository")
    github = github or GitHub()
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    result = dict(kind="operator-evidence-intake", repository=repository, release_tag=tag,
                  source_revision=revision, version=version, archive_evidence_sha256=archive_digest,
                  records=[], assets=[], status="MISSING",
                  scope="Retrieved data only; the candidate gate validates products, procedures and outcomes")
    try:
        releases = [row for row in github.pages(f"repos/{repository}/releases") if row["tag_name"] == tag]
        if len(releases) > 1:
            raise ValueError("Repeated acceptance evidence release")
        if not releases:
            write_json(output / "intake.json", result)
            return result
        release = releases[0]
        if release.get("draft") is not True or type(release["id"]) is not int or release["id"] <= 0:
            raise ValueError("Select the dedicated draft acceptance release")
        result["release_id"] = release["id"]
        rows = github.pages(f"repos/{repository}/releases/{release['id']}/assets")
        assets = {}
        for row in rows:
            if row["name"] in assets:
                raise ValueError("Repeated GitHub evidence asset name")
            assets[row["name"]] = row
        cache = output / "assets"
        cache.mkdir()

        def fetch(name, maximum):
            asset = assets.get(name)
            if (asset is None or asset.get("state") != "uploaded"
                    or type(asset.get("id")) is not int or asset["id"] <= 0
                    or type(asset.get("size")) is not int or not 0 < asset["size"] <= maximum):
                raise ValueError("Missing, incomplete or oversized acceptance asset")
            path = cache / name
            if not path.exists():
                github.download(f"repos/{repository}/releases/assets/{asset['id']}", path)
                data = bounded_file(path, maximum)
                digest = hashlib.sha256(data).hexdigest()
                if (len(data) != asset["size"] or asset.get("digest") not in {None, "sha256:" + digest}):
                    raise ValueError("GitHub acceptance asset differs from its metadata")
                result["assets"].append(dict(id=asset["id"], name=name, sha256=digest, bytes=len(data)))
            return path

        for name in sorted(assets):
            if not name.startswith(prefix):
                continue
            if not re.fullmatch(re.escape(prefix) + r"[a-f0-9]{64}\.json", name):
                raise ValueError("Malformed candidate acceptance asset name")
            path = fetch(name, MAX_RECORD)
            digest = sha256(path)
            if name != prefix + digest + ".json":
                raise ValueError("Acceptance record differs from its content address")
            data, record = read_record(path)
            context(record, revision, version, archive_digest)
            files = declared_files(record, None)
            directory = output / "records" / digest
            directory.mkdir(parents=True)
            # Supporting files retain their original relative names. Choose an
            # unused record name rather than imposing another filename policy.
            record_name = "record.json"
            while any(p.split("/", 1)[0].casefold() == record_name.casefold() for p in files):
                record_name = "_" + record_name
            restored = directory / record_name
            put(restored, data)
            for relative, identity in files.items():
                backing = fetch("observation-" + identity["sha256"], MAX_FILE)
                if backing.stat().st_size != identity["bytes"] or sha256(backing) != identity["sha256"]:
                    raise ValueError("Acceptance observation differs from its record")
                selected = directory / relative
                selected.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(backing, selected)
            retained_files(restored, record)
            result["records"].append(restored.relative_to(output).as_posix())
        if result["records"]:
            result["status"] = "COLLECTED"
        write_json(output / "intake.json", result)
        return result
    except (ValueError, OSError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
        result.update(status="FAIL", records=[])
        write_json(output / "intake.json", result)
        raise
