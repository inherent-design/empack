"""Execute the release workflow's channel classification against supported tags."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap
import unittest


class ReleaseChannelContracts(unittest.TestCase):
    def test_numbered_and_unnumbered_prereleases_are_not_stable(self):
        bash = "bash"
        if os.name == "nt":
            # PATH's bash.exe can be the WSL launcher, not the runner's Git Bash.
            git = shutil.which("git")
            self.assertIsNotNone(git, "Git for Windows is required for the workflow test")
            bash = str(Path(git).resolve().parents[1] / "bin" / "bash.exe")
            self.assertTrue(Path(bash).is_file(), f"Git Bash is missing: {bash}")
        workflow = (Path(__file__).parents[2] / ".github/workflows/release.yml").read_text()
        step = workflow.split("      - id: channel\n", 1)[1].split("\n      - ", 1)[0]
        script = textwrap.dedent(step.split("        run: |\n", 1)[1])
        cases = [("v0.6.0", "stable", "false")]
        cases += [(f"v0.6.0-{phase}{suffix}", phase, "true")
                  for phase in ("alpha", "beta", "rc") for suffix in ("", ".1", ".10")]
        for tag, channel, prerelease in cases:
            with self.subTest(tag=tag), tempfile.TemporaryDirectory() as root:
                output = Path(root) / "output"
                subprocess.run([bash, "-eu", "-c", script], check=True, cwd=root,
                               env={**os.environ, "GITHUB_REF_NAME": tag, "GITHUB_OUTPUT": "output"})
                self.assertEqual(output.read_text(), f"level={channel}\nprerelease={prerelease}\n")

    def test_parser_producer_is_shared_and_required_before_candidate_publication(self):
        root = Path(__file__).parents[2]
        release = (root / '.github/workflows/release.yml').read_text()
        pr = (root / '.github/workflows/pr-ci.yml').read_text()
        action = (root / '.github/actions/prism-parser/action.yml').read_text()
        self.assertIn('uses: ./.github/actions/prism-parser', release)
        self.assertIn('uses: ./.github/actions/prism-parser', pr)
        self.assertIn('--qmake qmake6', action)
        self.assertIn('7d2d3c1ec6fcf8255ebacd02d9f792688575dfb4', action)
        gate = release.split('  candidate-gate:', 1)[1].split('\n  release:', 1)[0]
        self.assertIn('needs: [archive-gate, consumer, auxiliary]', gate)
        self.assertIn('name: consumer-prism', gate)
        self.assertIn('--consumer .git/candidate-consumer/prism.json', gate)
        auxiliary = release.split('  auxiliary:', 1)[1].split('  candidate-gate:', 1)[0]
        for graph in ('minimal', 'all'):
            self.assertIn(f'scripts/verify.py doctest --test-features {graph} --output .git/candidate-doctest/{graph}', auxiliary)
        self.assertIn('name: candidate-doctest-${{ matrix.os }}', auxiliary)
        self.assertIn('pattern: candidate-doctest-*', gate)
        self.assertIn('test "${#doctest_records[@]}" -eq 6', gate)
        self.assertIn('doctest_args+=(--doctest "$record")', gate)
        self.assertIn('"${doctest_args[@]}"', gate)

    def test_archive_and_cross_builds_use_the_audited_producer_and_pinned_cross(self):
        root = Path(__file__).parents[2]
        release = (root / ".github/workflows/release.yml").read_text()
        pr = (root / ".github/workflows/pr-ci.yml").read_text()
        build = release.split("  build:", 1)[1].split("  test-archives:", 1)[0]
        self.assertEqual(build.count("scripts/release-artifacts.py build "), 1)
        self.assertIn('--compilation "$PRODUCT_COMPILATION"', build)
        self.assertIn('compilation=".git/product-build/${{ github.run_id }}-${{ github.run_attempt }}"', build)
        self.assertLess(build.index("scripts/release-artifacts.py build "), build.index("name: Package archive"))
        self.assertNotIn("run: cargo build", build)
        self.assertNotIn("run: cross build", build)
        for workflow in (release, pr):
            self.assertIn("tool: cross@0.2.5", workflow)
        self.assertIn("--builder cross --output .git/cross-product-build", pr)
        self.assertIn("name: verification-cross-product-build", pr)


if __name__ == "__main__":
    unittest.main()
