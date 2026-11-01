"""Synthetic transport tests; no GitHub mutation or consumer acceptance is performed."""
import copy
from contextlib import redirect_stdout
import hashlib
import importlib.util
import io
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from test_operator_acceptance import fixture, REVISION, VERSION, PAYLOAD, PRODUCT, TARGET, IDENTITY
from verification.acceptance import inspect_acceptance_evidence
from verification.acceptance_assets import GitHub, collect, stage
from verification.evidence import write_json


class Remote:
    def __init__(self, records):
        self.calls = []
        self.releases = [dict(id=7, tag_name="acceptance-v0.8.4-beta", draft=True)]
        self.assets, self.data = [], {}
        for path in records:
            raw = path.read_bytes()
            digest = hashlib.sha256(raw).hexdigest()
            self.add("acceptance-" + PAYLOAD + "-" + digest + ".json", raw)
            for row in json.loads(raw)["files"]:
                self.add("observation-" + row["sha256"], (path.parent / row["path"]).read_bytes())

    def add(self, name, raw):
        if any(row["name"] == name for row in self.assets):
            return
        identity = len(self.assets) + 1
        self.assets.append(dict(id=identity, name=name, state="uploaded", size=len(raw),
                                digest="sha256:" + hashlib.sha256(raw).hexdigest()))
        self.data[identity] = raw

    def pages(self, endpoint):
        self.calls.append(("GET", endpoint))
        if endpoint == "repos/owner/repo/releases":
            return self.releases
        if endpoint == "repos/owner/repo/releases/7/assets":
            return self.assets
        raise AssertionError(endpoint)

    def download(self, endpoint, path):
        self.calls.append(("GET", endpoint))
        prefix = "repos/owner/repo/releases/assets/"
        assert endpoint.startswith(prefix), endpoint
        path.write_bytes(self.data[int(endpoint.removeprefix(prefix))])


class AcceptanceAssets(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.path, self.record = fixture(self.source)

    def collect(self, remote, name="collected"):
        return collect("owner/repo", self.root / name, REVISION, VERSION, PAYLOAD, remote)

    def test_staging_and_collection_preserve_exact_records_and_supporting_bytes(self):
        raw = self.path.read_bytes()
        staged = stage([self.path], self.root / "stage", REVISION, VERSION, PAYLOAD)
        digest = hashlib.sha256(raw).hexdigest()
        self.assertEqual(staged["release_tag"], "acceptance-v0.8.4-beta")
        self.assertEqual(set(staged["assets"]), {
            "acceptance-" + PAYLOAD + "-" + digest + ".json",
            "observation-" + self.record["files"][0]["sha256"]})
        remote = Remote([self.path])
        for row in remote.assets:
            row["url"] = "https://unselected.invalid/untrusted-metadata-url"
        result = self.collect(remote)
        restored = self.root / "collected" / result["records"][0]
        self.assertEqual(restored.read_bytes(), raw)
        self.assertEqual((restored.parent / "observation.txt").read_bytes(),
                         b"Synthetic retained observation, not a consumer execution.\n")
        admitted = inspect_acceptance_evidence(restored, REVISION, VERSION, PAYLOAD, {(TARGET, "default"): PRODUCT})
        self.assertEqual(admitted["receipt_sha256"], digest)
        self.assertEqual(result["status"], "COLLECTED")
        self.assertNotIn("qualification", result)
        self.assertEqual(remote.calls[:2], [("GET", "repos/owner/repo/releases"),
                                            ("GET", "repos/owner/repo/releases/7/assets")])

    def test_missing_evidence_is_distinct_from_failed_access(self):
        remote = Remote([])
        remote.releases = []
        self.assertEqual(self.collect(remote)["status"], "MISSING")
        with patch.object(remote, "pages", side_effect=OSError("controlled access failure")):
            with self.assertRaises(OSError):
                self.collect(remote, "failed")
        failed = json.loads((self.root / "failed/intake.json").read_text())
        self.assertEqual(failed["status"], "FAIL")
        self.assertEqual(failed["records"], [])

    def test_all_attempts_reach_the_gate_and_a_later_pass_does_not_erase_failure(self):
        other = self.root / "failed-source"
        other.mkdir()
        path, record = fixture(other)
        record["status"] = record["observations"][0]["status"] = "FAIL"
        write_json(path, record)
        staged = stage([self.path, path], self.root / "staged-attempts", REVISION, VERSION, PAYLOAD)
        self.assertEqual(len(staged["assets"]), 3, "two records share one observation")
        collected = self.collect(Remote([self.path, path]))
        admitted = [inspect_acceptance_evidence(self.root / "collected" / name,
                    REVISION, VERSION, PAYLOAD, {(TARGET, "default"): PRODUCT}) for name in collected["records"]]
        self.assertCountEqual([row["status"] for row in admitted], ["PASS", "FAIL"])
        from test_candidate_qualification import CandidatePolicy
        from verification.qualification import evaluate_requirements
        candidate = CandidatePolicy()
        candidate.setUp()
        row = next(row for row in candidate.document["requirements"] if row["id"] == "UX-084")
        row.update(evidence_tiers=["C"], cases=[dict(id="UX-084.transport", boundary="C", test_ids=[IDENTITY],
                   required_platforms=["linux"], required_product_features=["default"])])
        result = evaluate_requirements(candidate.document, candidate.classification, candidate.evidence,
                    REVISION, VERSION, candidate.package_proofs, acceptance_evidence=admitted)
        self.assertEqual(result["status"], "FAIL", "synthetic gate mechanics only")

    def test_candidate_and_supporting_identity_failures_cannot_leave_a_partial_success(self):
        for index, mutation in enumerate((
            lambda r: r.data.update({1: b"changed bytes"}),
            lambda r: r.assets.pop(),
            lambda r: r.assets[0].update(size=True),
            lambda r: r.assets[0].update(digest="sha256:" + "0" * 64),
            lambda r: r.assets[0].update(state="starter"),
            lambda r: r.releases[0].update(draft=False),
        )):
            with self.subTest(index=index):
                remote = Remote([self.path])
                mutation(remote)
                with self.assertRaises(ValueError):
                    self.collect(remote, f"bad-{index}")
                result = json.loads((self.root / f"bad-{index}/intake.json").read_text())
                self.assertEqual((result["status"], result["records"]), ("FAIL", []))
        record = copy.deepcopy(self.record)
        record["source_revision"] = "f" * 40
        write_json(self.path, record)
        with self.assertRaisesRegex(ValueError, "different candidate"):
            self.collect(Remote([self.path]), "wrong-source")

    def test_supporting_paths_are_confined_and_do_not_impose_an_extra_record_filename(self):
        proof = self.source / "Record.JSON"
        proof.write_bytes(b"Synthetic supporting bytes with a record-like name.\n")
        old = self.record["files"][0]
        self.record["files"] = [dict(path=proof.name, sha256=hashlib.sha256(proof.read_bytes()).hexdigest(), bytes=proof.stat().st_size)]
        for row in self.record["observations"]:
            row["evidence"] = [proof.name]
        write_json(self.path, self.record)
        result = self.collect(Remote([self.path]))
        restored = self.root / "collected" / result["records"][0]
        self.assertEqual(restored.name, "_record.json")
        self.assertEqual((restored.parent / "Record.JSON").read_bytes(), proof.read_bytes())
        self.record["files"] = [dict(old, path="../outside.txt")]
        write_json(self.path, self.record)
        with self.assertRaisesRegex(ValueError, "Unsafe"):
            stage([self.path], self.root / "unsafe", REVISION, VERSION, PAYLOAD)
        self.assertFalse((self.root / "outside.txt").exists())
        # Build malformed remote bytes independently; the collector must reject
        # the declaration before it can create or request an escaping file.
        remote = Remote([])
        raw = json.dumps(self.record).encode()
        remote.add("acceptance-" + PAYLOAD + "-" + hashlib.sha256(raw).hexdigest() + ".json", raw)
        with self.assertRaisesRegex(ValueError, "Unsafe"):
            self.collect(remote, "remote-unsafe")
        self.assertEqual(len([call for call in remote.calls if "/assets/" in call[1]]), 1)
        self.assertFalse((self.root / "outside.txt").exists())

    def test_existing_output_and_author_records_are_not_replaced(self):
        output = self.root / "owned"
        output.mkdir()
        sentinel = output / "keep.txt"
        sentinel.write_text("keep", encoding="utf-8")
        before = self.path.read_bytes()
        for action in (lambda: stage([self.path], output, REVISION, VERSION, PAYLOAD),
                       lambda: collect("owner/repo", output, REVISION, VERSION, PAYLOAD, Remote([self.path]))):
            with self.assertRaises(FileExistsError):
                action()
        self.assertEqual(sentinel.read_text(), "keep")
        self.assertEqual(self.path.read_bytes(), before)

    def test_github_client_uses_only_get_requests(self):
        github = GitHub()
        def run(command, **kwargs):
            self.assertEqual(command[:4], ["gh", "api", "--method", "GET"])
            self.assertNotIn("input", kwargs)
            if "stdout" in kwargs:
                kwargs["stdout"].write(b"synthetic bytes")
                return SimpleNamespace(returncode=0)
            return SimpleNamespace(stdout="[[]]")
        with patch("verification.acceptance_assets.subprocess.run", side_effect=run) as calls:
            self.assertEqual(github.pages("repos/owner/repo/releases"), [])
            github.download("repos/owner/repo/releases/assets/1", self.root / "download")
        self.assertEqual(calls.call_count, 2)

    def test_command_arguments_wire_both_operations_without_product_execution(self):
        spec = importlib.util.spec_from_file_location("acceptance_command",
            Path(__file__).parents[1] / "acceptance-assets.py")
        command = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(command)
        common = ["--revision", REVISION, "--version", VERSION, "--archive-evidence-sha256", PAYLOAD]
        before = self.path.read_bytes()
        with redirect_stdout(io.StringIO()) as output:
            with patch("sys.argv", ["acceptance-assets.py", "stage", *common,
                                   "--record", str(self.path), "--output", str(self.root / "cli-stage")]):
                self.assertEqual(command.main(), 0)
            with patch("verification.acceptance_assets.GitHub", return_value=Remote([self.path])), \
                 patch("sys.argv", ["acceptance-assets.py", "collect", *common,
                                    "--repository", "owner/repo", "--output", str(self.root / "cli-collect")]):
                self.assertEqual(command.main(), 0)
        self.assertIn("no acceptance claimed", output.getvalue())
        manifest = json.loads((self.root / "cli-collect/intake.json").read_text())
        self.assertEqual((self.root / "cli-collect" / manifest["records"][0]).read_bytes(), before)
        self.assertEqual(self.path.read_bytes(), before)

    def test_release_gate_forwards_collected_records_and_preserves_gate_attempts(self):
        workflow = (Path(__file__).parents[2] / ".github/workflows/release.yml").read_text()
        gate = workflow.split("  candidate-gate:\n", 1)[1].split("\n  release:\n", 1)[0]
        publication = workflow.split("\n  release:\n", 1)[1]
        self.assertIn("scripts/acceptance-assets.py collect", gate)
        self.assertIn('acceptance_args+=(--acceptance "$record")', gate)
        self.assertIn('"${acceptance_args[@]}" --payload', gate)
        self.assertIn(".git/candidate-acceptance/", gate)
        self.assertIn("name: candidate-qualification-${{ github.run_attempt }}", gate)
        self.assertIn("qualification_artifact: ${{ steps.qualify.outputs.qualification_artifact }}", gate)
        self.assertIn("name: ${{ needs.candidate-gate.outputs.qualification_artifact }}", publication)
        self.assertNotIn("candidate-qualification-${{ github.run_attempt }}", publication)
        self.assertIn("github.event_name == 'push'", publication)
