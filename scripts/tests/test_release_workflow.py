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


if __name__ == "__main__":
    unittest.main()
