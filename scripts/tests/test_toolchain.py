"""Check toolchain setup and local target declarations without running Cargo."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest


class ToolchainContracts(unittest.TestCase):
    def test_local_nextest_targets_have_classified_members(self):
        root = Path(__file__).parents[2]
        bindings = json.loads((root / "tests/verification/test-bindings.json").read_text())
        targets = {(row["id"].split("::", 1)[0], row["binary_name"])
                   for row in bindings["tests"]}
        tasks = tomllib.loads((root / "mise.toml").read_text())["tasks"]
        selected = set()
        for task, configuration in tasks.items():
            for host in ("run", "run_windows"):
                commands = configuration.get(host, [])
                if isinstance(commands, str):
                    commands = [commands]
                for command in commands:
                    if not command.startswith("cargo nextest "):
                        continue
                    tokens = command.split()
                    package = tokens[tokens.index("-p") + 1] if "-p" in tokens else None
                    for index, token in enumerate(tokens):
                        if token == "--test":
                            name = tokens[index + 1]
                            with self.subTest(task=task, host=host, target=name):
                                self.assertIn((package, name), targets)
                            selected.add((package, name))
        self.assertTrue(selected, "No fixed local Nextest targets were checked")

    def test_windows_msvc_setup_is_pinned_and_precedes_compiler_installation(self):
        action = (Path(__file__).parents[2] / ".github/actions/rust-toolchain/action.yml").read_text()
        setup = "ilammy/msvc-dev-cmd@0b201ec74fa43914dc39ae48a89fd1d8cb592756"
        self.assertIn("if: runner.os == 'Windows'", action)
        self.assertIn("arch: x64", action)
        self.assertLess(action.index(setup), action.index("uses: dtolnay/rust-toolchain@master"))

    def test_repository_pin_and_extra_components_reach_the_installer(self):
        root = Path(__file__).parents[2]
        action = (root / ".github/actions/rust-toolchain/action.yml").read_text()
        script = action.split("        python - <<'PYTHON'\n", 1)[1].split("        PYTHON", 1)[0]
        with tempfile.TemporaryDirectory() as temp:
            work = Path(temp)
            (work / "rust-toolchain.toml").write_text(
                '[toolchain]\nchannel = "1.99.1"\ncomponents = ["clippy", "rustfmt"]\n'
            )
            output = work / "output"
            env = dict(os.environ, GITHUB_OUTPUT=str(output),
                       EXTRA_COMPONENTS="llvm-tools-preview,clippy")
            subprocess.run([sys.executable, "-c", textwrap.dedent(script)],
                           cwd=work, env=env, check=True)
            self.assertEqual(output.read_text().splitlines(), [
                "channel=1.99.1", "components=clippy,rustfmt,llvm-tools-preview",
            ])
