"""Controlled compiler dispatch checks; no compiler, product or consumer executes."""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import verify
from verification import compiler, product_build
from verification.evidence import sha256
from verification.suites import SUITES

spec = importlib.util.spec_from_file_location("compiler_map", Path(__file__).parents[1] / "check-test-map.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class CompilerSelection(unittest.TestCase):
    def test_configured_toolset_is_case_insensitive_idempotent_and_not_global(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            native = root / "bin/Hostx64/x64"
            native.mkdir(parents=True)
            for name in ("cl.exe", "link.exe"):
                (native / name).write_bytes(b"inert tool backing; never executed")
            original = {"Path": "git-tools", "VCTOOLSINSTALLDIR": str(root),
                        "HOME": "caller-home", "RUSTFLAGS": "caller-flags"}
            before = dict(original)
            with patch.object(compiler.os, "name", "nt"):
                selected = compiler.compiler_environment(original)
                self.assertEqual(selected["PATH"], str(native) + os.pathsep + "git-tools")
                self.assertNotIn("Path", selected)
                self.assertEqual(compiler.compiler_environment(selected), selected)
                self.assertEqual(original, before)
                self.assertEqual(selected["HOME"], original["HOME"])
                self.assertEqual(selected["RUSTFLAGS"], original["RUSTFLAGS"])
                self.assertEqual(compiler.compiler_environment({"VCToolsInstallDir": str(root)})["PATH"],
                                 str(native), "absent PATH must not add an implicit current-directory entry")
                (native / "link.exe").unlink()
                with self.assertRaisesRegex(RuntimeError, "MSVC compiler tools are missing"):
                    compiler.compiler_environment(original)
            with patch.object(compiler.os, "name", "posix"):
                self.assertEqual(compiler.compiler_environment(original), original)
            with patch.object(compiler.os, "name", "nt"):
                self.assertEqual(compiler.compiler_environment({"PATH": "unconfigured"}),
                                 {"PATH": "unconfigured"})


class CompilerDispatch(unittest.TestCase):
    def test_all_owned_compilation_paths_use_the_shared_selection(self):
        original = {"PATH": "git-tools", "HOME": "caller-home", "RUSTDOCFLAGS": "untrusted"}
        def select(environment):
            return dict(environment, PATH="configured-native-tools")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ("product", "suite", "doctest", "release"):
                (root / name).mkdir()
            args = SimpleNamespace(binary=None, sha256=None, version=None, target=None,
                                   product_features="default", product_profile="dev", suites=["cli"])
            with patch.object(verify, "compiler_environment", side_effect=select) as selection, \
                    patch.object(verify, "checked", side_effect=RuntimeError("controlled stop")) as command:
                with self.assertRaisesRegex(RuntimeError, "controlled stop"):
                    verify.product(args, root / "product", original, "x86_64-pc-windows-msvc")
                self.assertEqual(command.call_args.args[0][:2], ["cargo", "build"])
                self.assertEqual(command.call_args.args[3]["PATH"], "configured-native-tools")
                selection.assert_called_once_with(original)
            with patch.object(verify, "compiler_environment", side_effect=select) as selection, \
                    patch.object(verify, "checked", side_effect=RuntimeError("controlled stop")) as command:
                with self.assertRaisesRegex(RuntimeError, "controlled stop"):
                    verify.run_suite("cli", SUITES["cli"], root / "suite", original,
                                     dict(target="x86_64-pc-windows-msvc", features="default"), {}, "all")
                self.assertEqual(command.call_args.args[0][:3], ["cargo", "nextest", "list"])
                self.assertEqual(command.call_args.args[3]["PATH"], "configured-native-tools")
                selection.assert_called_once_with(original)
            with patch.object(verify, "compiler_environment", side_effect=select) as selection, \
                    patch.object(verify, "checked", side_effect=RuntimeError("controlled stop")) as command:
                self.assertEqual(verify.run_doctests(root / "doctest", original, "minimal"), 1)
                self.assertEqual(command.call_args.args[3]["PATH"], "configured-native-tools")
                self.assertNotIn("RUSTDOCFLAGS", selection.call_args.args[0])
                self.assertEqual(original["RUSTDOCFLAGS"], "untrusted")

            (root / "Cargo.toml").write_text("[workspace]\nmembers=[]\n")
            (root / "Cargo.lock").write_text("version=4\n")
            (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel="1.99.1"\n')
            def run(arguments, **kwargs):
                self.assertEqual(kwargs["env"]["PATH"], "configured-native-tools")
                if arguments[:2] == ["git", "rev-parse"]:
                    return 0, "a" * 40, False
                if arguments[0] == "git":
                    return 0, "", False
                if arguments == ["cargo", "--version"]:
                    return 0, "cargo 1.99.1 (controlled fixture)", False
                self.assertEqual(arguments[:2], ["cargo", "build"])
                raise RuntimeError("controlled stop")
            with patch.object(product_build, "compiler_environment", side_effect=select) as selection, \
                    patch.object(product_build, "run", side_effect=run):
                with self.assertRaisesRegex(RuntimeError, "controlled stop"):
                    product_build.compile_product(root, "x86_64-pc-windows-msvc", "cargo", root / "release", original)
                self.assertEqual(selection.call_args.args[0]["PATH"], "git-tools")
            with patch.dict(os.environ, original, clear=True), \
                    patch.object(checker, "compiler_environment", side_effect=select), \
                    patch.object(checker, "run", return_value=(0, "{}", False)) as commands, \
                    patch.object(checker, "rustdoc_tests", return_value={"controlled fixture"}):
                with self.assertRaises(ValueError):  # Deliberately invalid inventories cannot pass.
                    checker.discover_tooling(root / "discovery", set())
                self.assertEqual(len(commands.call_args_list), 4)
                for call in commands.call_args_list:
                    self.assertEqual(call.kwargs["env"]["PATH"], "configured-native-tools")
                self.assertEqual(commands.call_args_list[1].args[0][:2], ["cargo", "run"])
                self.assertEqual(commands.call_args_list[2].args[0][:2], ["cargo", "test"])
            self.assertEqual(original["PATH"], "git-tools")

    def test_external_product_version_probe_does_not_select_compiler_tools(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "empack.exe"
            binary.write_bytes(b"inert product fixture; never executed")
            args = SimpleNamespace(binary=str(binary), sha256=sha256(binary), version="1.2.3",
                                   target="x86_64-pc-windows-msvc", product_features="default", product_profile="release")
            runtime = {"PATH": "runtime-tools"}
            with patch.object(verify, "compiler_environment") as selection, \
                    patch.object(verify, "isolated_environment", return_value=runtime), \
                    patch.object(verify, "checked", return_value="empack 1.2.3\n") as command:
                verify.product(args, root, {}, "x86_64-pc-windows-msvc")
                selection.assert_not_called()
                self.assertEqual(command.call_args.args[3], runtime)


if __name__ == "__main__":
    unittest.main()
