import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'crates/empack-lib/src/engine/initialize'
spec = importlib.util.spec_from_file_location('automation', SOURCE / 'empack-ci.py')
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class ProjectAutomation(unittest.TestCase):
    def report(self):
        return {'schema': 1, 'pack': 'Pack With Spaces', 'version': '1.2.3-beta',
                'lock': 'current', 'profiles': [
                    {'name': 'launcher profile', 'output': 'dist/Pack With Spaces-prism.zip'},
                    {'name': 'modrinth', 'output': 'dist/Pack With Spaces.mrpack'}]}

    def test_build_selects_exact_profiles_and_stages_only_expected_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'dist').mkdir()
            report = self.report()
            for row in report['profiles']:
                (root / row['output']).write_bytes(row['name'].encode())
            (root / 'dist/stale.zip').write_bytes(b'old')
            with patch.dict(os.environ, {'RUNNER_TEMP': tmp, 'EMPACK_PROFILES': 'launcher profile\nmodrinth'}), \
                 patch.object(ci.Path, 'cwd', return_value=root), \
                 patch.object(ci, 'run', return_value=json.dumps(report)) as run:
                ci.build()
            self.assertEqual(run.call_args_list[1].args,
                             ('empack', '--yes', 'build', '--optional-defaults',
                              '--profile', 'launcher profile', '--profile', 'modrinth'))
            destination = root / 'empack-artifacts'
            self.assertEqual(len(list(destination.iterdir())), 3)
            for line in (destination / 'SHA256SUMS').read_text().splitlines():
                checksum, name = line.split('  ')
                self.assertNotIn(' ', name)
                self.assertEqual(ci.digest(destination / name), checksum)

    def test_mismatched_tag_fails_before_build_or_publication(self):
        with patch.dict(os.environ, {'GITHUB_REF_NAME': 'v9'}), \
             patch.object(ci, 'selection', return_value=self.report()), patch.object(ci, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'must match'):
                ci.build(release=True)
            run.assert_not_called()

    def test_build_failure_never_stages_or_publishes(self):
        with patch.object(ci, 'selection', return_value=self.report()), \
             patch.object(ci, 'run', side_effect=subprocess.CalledProcessError(1, 'empack')), \
             patch.object(ci, 'stage') as stage:
            with self.assertRaises(subprocess.CalledProcessError):
                ci.build()
            stage.assert_not_called()

    def test_stale_lock_rejected(self):
        report = self.report()
        report['lock'] = 'stale'
        with patch.object(ci, 'run', return_value=json.dumps(report)):
            with self.assertRaisesRegex(ValueError, 'current lock'):
                ci.selection()

    def test_unsafe_paths_and_name_collisions_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for output in ['/outside', 'dist/../outside', 'other/pack.zip', 'dist/missing.zip']:
                with self.assertRaises(ValueError):
                    ci.artifact(root, output)
            (root / 'dist').mkdir()
            for name in ['a b.zip', 'a-b.zip']:
                (root / 'dist' / name).write_bytes(b'bytes')
            report = {'profiles': [{'output': 'dist/a b.zip'}, {'output': 'dist/a-b.zip'}]}
            with self.assertRaisesRegex(ValueError, 'collides'):
                ci.stage(root, report, root / 'staged')
            self.assertFalse((root / 'staged').exists())

    @unittest.skipIf(os.name == 'nt', 'native symlink creation requires host privilege')
    def test_symlinked_ancestor_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'real').mkdir()
            (root / 'real/pack.zip').write_bytes(b'bytes')
            (root / 'dist').symlink_to(root / 'real', target_is_directory=True)
            with self.assertRaisesRegex(ValueError, 'symlink'):
                ci.artifact(root, 'dist/pack.zip')

    def test_failed_upload_keeps_release_draft(self):
        with tempfile.TemporaryDirectory() as tmp, \
             patch.object(ci, 'run', side_effect=subprocess.CalledProcessError(1, 'gh')) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                ci.publish(Path(tmp), 'v1.2.3-beta')
            self.assertEqual(run.call_count, 1)
            self.assertIn('--draft', run.call_args.args)
            self.assertIn('--prerelease', run.call_args.args)

    def test_publication_reveals_release_only_after_upload(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(ci, 'run') as run:
            ci.publish(Path(tmp), 'v1.2.3')
            self.assertEqual(run.call_args_list[1].args,
                             ('gh', 'release', 'edit', 'v1.2.3', '--draft=false'))

    def test_missing_digest_prevents_download(self):
        asset = {'name': 'empack-0.7.1-beta-x86_64-unknown-linux-gnu.tar.gz'}
        with patch.object(ci, 'run', return_value=json.dumps({'assets': [asset]})) as run:
            with self.assertRaisesRegex(ValueError, 'SHA-256'):
                ci.install('v0.7.1-beta')
            self.assertEqual(run.call_count, 1)

    def test_digest_mismatch_prevents_execution(self):
        asset = {'name': 'empack-0.7.1-beta-x86_64-unknown-linux-gnu.tar.gz',
                 'digest': 'sha256:' + '0' * 64}
        def fake(*args, **kwargs):
            if args[1] == 'api':
                return json.dumps({'assets': [asset]})
            Path(args[-1], asset['name']).write_bytes(b'wrong bytes')
        with patch.object(ci, 'run', side_effect=fake) as run:
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                ci.install('v0.7.1-beta')
            self.assertEqual(run.call_count, 2)

    def test_verified_archive_installs_only_binary_and_exports_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive = root / 'source.tar.gz'
            with tarfile.open(archive, 'w:gz') as writer:
                member = tarfile.TarInfo('./empack')
                member.size = 4
                writer.addfile(member, io.BytesIO(b'test'))
            asset = {'name': 'empack-0.7.1-beta-x86_64-unknown-linux-gnu.tar.gz',
                     'digest': 'sha256:' + ci.digest(archive)}
            def fake(*args, **kwargs):
                if args[1] == 'api':
                    return json.dumps({'assets': [asset]})
                if args[1] == 'release':
                    Path(args[-1], asset['name']).write_bytes(archive.read_bytes())
            with patch.object(ci, 'run', side_effect=fake), patch.dict(os.environ, {
                    'RUNNER_TEMP': tmp, 'GITHUB_PATH': str(root / 'path')}):
                ci.install('v0.7.1-beta')
            self.assertEqual((root / 'empack-bin/empack').read_bytes(), b'test')
            self.assertEqual((root / 'path').read_text().strip(), str(root / 'empack-bin'))

    def test_workflows_share_helper_and_pin_actions(self):
        import re
        for name in ['validate', 'release']:
            text = (SOURCE / f'{name}.yml').read_text()
            self.assertIn(f'python3 .github/scripts/empack-ci.py {name}', text)
            self.assertIn('python3 .github/scripts/empack-ci.py install', text)
            self.assertNotIn('cargo install', text)
            self.assertIn('persist-credentials: false', text)
            for action in re.findall(r'uses: (\S+)', text):
                self.assertRegex(action, r'@[0-9a-f]{40}$')
        self.assertIn('cancel-in-progress: false', (SOURCE / 'release.yml').read_text())
        self.assertIn('contents: read', (SOURCE / 'validate.yml').read_text())
