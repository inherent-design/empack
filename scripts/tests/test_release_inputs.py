"""Exercise version stamping and Cargo's real locked build in an isolated workspace."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("release_inputs", SCRIPTS / "prepare-release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseInputs(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "Cargo.toml").write_text('''[workspace]
members = ["app", "helper"]
resolver = "2"
[workspace.package]
version = "0.0.0-dev"
edition = "2024"
[workspace.metadata.empack]
release-target = "0.8.4-beta"
''')
        (self.root / "rust-toolchain.toml").write_bytes((SCRIPTS.parent / "rust-toolchain.toml").read_bytes())
        for name in ("app", "helper"):
            path = self.root / name
            (path / "src").mkdir(parents=True)
            manifest = f'[package]\nname = "{name}"\nversion.workspace = true\nedition.workspace = true\n'
            if name == "app":
                manifest += '[dependencies]\nhelper = { path = "../helper" }\n'
                (path / "src/main.rs").write_text('fn main() { println!("{} {}", env!("CARGO_PKG_VERSION"), helper::version()); }\n')
            else:
                (path / "src/lib.rs").write_text('pub fn version() -> &\'static str { env!("CARGO_PKG_VERSION") }\n')
            (path / "Cargo.toml").write_text(manifest)
        (self.root / "Cargo.lock").write_text('''# Fixture, no external dependencies
version = 4

[[package]]
name = "app"
version = "0.0.0-dev"
dependencies = ["helper"]

[[package]]
name = "helper"
version = "0.0.0-dev"
''')
        (self.root / ".gitignore").write_text("/target/\n")
        self.git("init", "-q")
        self.git("config", "user.name", "Release fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True).strip()

    def test_stamped_workspace_builds_locked_without_dependency_resolution(self):
        record = release.prepare(self.root, "0.8.4-beta")
        self.assertEqual(record["source_revision"], self.git("rev-parse", "HEAD"))
        self.assertEqual(record["workspace_packages"], ["app", "helper"])
        before = (self.root / "Cargo.lock").read_bytes()
        output = subprocess.run(["cargo", "run", "--locked", "--offline", "--quiet", "-p", "app"],
                                cwd=self.root, text=True, capture_output=True, timeout=120)
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertEqual(output.stdout.strip(), "0.8.4-beta 0.8.4-beta")
        self.assertEqual((self.root / "Cargo.lock").read_bytes(), before)
        self.assertEqual(set(self.git("diff", "--name-only").splitlines()), {"Cargo.toml", "Cargo.lock"})

    def test_deterministic_stamping_retains_external_lock_records_and_comments(self):
        lock = self.root / "Cargo.lock"
        lock.write_text(lock.read_text() + '''
[[package]]
name = "third-party"
version = "7.8.9"
source = "registry+https://example.invalid/index"
checksum = "unchanged"
''')
        first, names = release.stamped_sources(self.root, "0.8.4-beta")
        second, _ = release.stamped_sources(self.root, "0.8.4-beta")
        self.assertEqual(first, second)
        self.assertEqual(names, ["app", "helper"])
        original = tomllib.loads(lock.read_text())["package"][-1]
        self.assertEqual(tomllib.loads(first["Cargo.lock"])["package"][-1], original)
        self.assertTrue(first["Cargo.lock"].startswith("# Fixture, no external dependencies\n"))

    def test_wrong_target_and_invalid_versions_fail_before_any_write(self):
        before = {name: (self.root / name).read_bytes() for name in ("Cargo.toml", "Cargo.lock")}
        for version in ("v0.8.4-beta", "0.8.5-beta", "0.8.4-preview", "0.8.4-beta\n", "00.8.4"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.prepare(self.root, version)
            self.assertEqual(before, {name: (self.root / name).read_bytes() for name in before})

    def test_dirty_source_is_not_a_release_input(self):
        (self.root / "unreviewed.txt").write_text("untracked\n")
        with self.assertRaisesRegex(ValueError, "clean source"):
            release.prepare(self.root, "0.8.4-beta")
        self.assertIn('version = "0.0.0-dev"', (self.root / "Cargo.toml").read_text())

    def test_missing_stale_or_ambiguous_workspace_lock_records_fail(self):
        path = self.root / "Cargo.lock"
        original = path.read_text()
        for bad in (original.replace('name = "helper"', 'name = "other"'),
                    original.replace('version = "0.0.0-dev"', 'version = "0.1.0"', 1),
                    original + '\n[[package]]\nname = "helper"\nversion = "0.0.0-dev"\n',
                    original.replace('dependencies = ["helper"]', 'dependencies = ["helper 0.0.0-dev"]')):
            path.write_text(bad)
            with self.subTest(lock=bad), self.assertRaises(ValueError):
                release.stamped_sources(self.root, "0.8.4-beta")

    def test_supported_release_suffixes_are_explicit(self):
        for suffix in ("", "-alpha", "-beta", "-rc", "-alpha.0", "-beta.12", "-rc.1"):
            self.assertEqual(release.release_version("0.8.4" + suffix), "0.8.4" + suffix)


if __name__ == "__main__":
    unittest.main()
