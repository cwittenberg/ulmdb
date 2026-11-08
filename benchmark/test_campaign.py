"""Private campaign scheduling, executable preservation and oracle gates."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('benchmark_campaign', HERE / 'campaign.py')
campaign = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(campaign)


class CampaignTests(unittest.TestCase):
    def test_sqlite_reference_exception_is_hash_only_and_requires_accepted_runtime(self):
        initial = {'sqlite': '3.45.1', 'source_id': 'same-source',
                   'link_library_path': '/canonical/sqlite.so', 'link_library_sha256': '1' * 64}
        accepted = dict(initial, link_library_sha256='2' * 64)
        context = campaign.sqlite_reference_context(initial, accepted, accepted)
        self.assertTrue(context['historical_link_hash_only_change'])
        self.assertEqual(context['archived_initial'], initial)
        self.assertEqual(context['runtime'], accepted)
        for key in ['sqlite', 'source_id', 'link_library_path']:
            with self.subTest(key=key), self.assertRaises(ValueError):
                campaign.sqlite_reference_context(dict(initial, **{key: 'different'}), accepted, accepted)
        for key in campaign.SQLITE_VERSION_KEYS:
            with self.subTest(runtime=key), self.assertRaises(ValueError):
                campaign.sqlite_reference_context(initial, accepted, dict(accepted, **{key: 'wrong'}))
        with self.assertRaises(ValueError):
            campaign.sqlite_reference_context(dict(initial, unknown='unreviewed'), accepted, accepted)

    def test_sqlite_loader_path_and_runtime_mutation_are_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            library, other, executable = root / 'sqlite.so', root / 'other.so', root / 'engine'
            library.write_bytes(b'first library image')
            other.write_bytes(library.read_bytes())
            executable.write_bytes(b'ELF resolution fixture, never executed')
            versions = {'sqlite': '3.45.1', 'source_id': 'same-source',
                        'link_library_path': str(library), 'link_library_sha256': campaign.runner.sha(library)}
            metadata = {key: versions[key] for key in ['sqlite', 'source_id']}
            def resolution(path):
                return SimpleNamespace(returncode=0, stdout=f' libsqlite3.so.0 => {path} (0x1234)\n', stderr='')
            with mock.patch.object(campaign, 'sqlite_runtime_versions', return_value=metadata), \
                 mock.patch.object(campaign.subprocess, 'run', return_value=resolution(library)):
                pin = campaign.inspect_sqlite_runtime(executable, versions, {})
                self.assertEqual(pin['canonical_path'], str(library))
                campaign.verify_sqlite_runtime_pin(pin)
                library.write_bytes(b'changed while campaign was active')
                with self.assertRaisesRegex(ValueError, 'bytes changed'):
                    campaign.verify_sqlite_runtime_pin(pin)
            library.write_bytes(b'first library image')
            with mock.patch.object(campaign, 'sqlite_runtime_versions', return_value=metadata), \
                 mock.patch.object(campaign.subprocess, 'run', return_value=resolution(other)):
                with self.assertRaisesRegex(ValueError, 'loader path'):
                    campaign.inspect_sqlite_runtime(executable, versions, {})
            with mock.patch.object(campaign, 'sqlite_runtime_versions', return_value=dict(metadata, source_id='wrong')):
                with self.assertRaisesRegex(ValueError, 'version/source_id'):
                    campaign.verify_sqlite_runtime_pin({'versions': versions})
            with self.assertRaisesRegex(ValueError, 'instrumentation'):
                campaign.inspect_sqlite_runtime(executable, versions, {'LD_PRELOAD': 'unexpected.so'})

    def test_sqlite_returned_metadata_is_strict_independently_of_durability(self):
        document = json.loads((HERE / 'results/millions.json').read_text())
        reference = next(p['report'] for p in document['profiles'] if p['report']['engine'] == 'sqlite')
        digest = reference['build']['executable_sha256']
        campaign.validate_millions(reference, reference, document['config'], digest)
        for key in campaign.SQLITE_VERSION_KEYS:
            modified = copy.deepcopy(reference)
            modified['versions'][key] = 'substituted'
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'SQLite runtime'):
                campaign.validate_millions(modified, reference, document['config'], digest)
        for alteration in ['missing', 'extra', 'invalid_hash']:
            modified = copy.deepcopy(reference)
            if alteration == 'missing':
                del modified['versions']['source_id']
            elif alteration == 'extra':
                modified['versions']['unreviewed'] = 'metadata'
            else:
                modified['versions']['link_library_sha256'] = 'not a sha256 digest'
            with self.subTest(alteration=alteration), self.assertRaisesRegex(ValueError, 'SQLite runtime'):
                campaign.validate_millions(modified, modified, document['config'], digest)

    def test_schedule_keeps_pairs_adjacent_alternates_arms_and_rotates_controls(self):
        steps = campaign.schedule(4, 3)
        self.assertEqual(len(steps), 28)
        full = [s for s in steps if s['suite'] == 'full']
        self.assertEqual(len(full), 16)
        self.assertEqual([s['number'] for s in steps], list(range(28)))
        for index in range(0, len(full), 2):
            a, b = full[index:index + 2]
            self.assertEqual((a['filesystem'], a['round']), (b['filesystem'], b['round']))
            self.assertEqual({a['arm'], b['arm']}, {'baseline', 'candidate'})
        self.assertEqual([full[i]['filesystem'] for i in range(0, 16, 4)],
                         ['disk', 'ram', 'disk', 'ram'])
        for number, expected_order in enumerate([['ulmdb', 'lmdb', 'sqlite'],
                                                  ['lmdb', 'sqlite', 'ulmdb'],
                                                  ['sqlite', 'ulmdb', 'lmdb']]):
            million = [s for s in steps if s['suite'] == 'millions' and s['round'] == number]
            compressed = [s['engine'] for i, s in enumerate(million)
                          if i == 0 or s['engine'] != million[i - 1]['engine']]
            self.assertEqual(compressed, expected_order)
            targets = [s for s in million if s['engine'] == 'ulmdb']
            self.assertEqual([s['arm'] for s in targets],
                             ['baseline', 'candidate'] if number % 2 == 0 else ['candidate', 'baseline'])
            self.assertTrue(all(s['arm'] == 'shared_control' for s in million if s['engine'] != 'ulmdb'))

    def test_preserved_executable_is_independent_and_rejects_wrong_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'original'
            source.write_bytes(b'preserved executable fixture')
            source.chmod(0o755)
            info = campaign.preserve_executable(source, root / 'copy')
            source.write_bytes(b'caller rebuilt original')
            self.assertEqual(campaign.runner.sha(Path(info['path'])), info['sha256'])
            self.assertNotEqual(source.stat().st_ino, Path(info['path']).stat().st_ino)
            with self.assertRaises(ValueError):
                campaign.preserve_executable(source, root / 'rejected', info['sha256'])

    def test_real_million_capture_requires_all_oracles_exact_counts_and_normal_durability(self):
        document = json.loads((HERE / 'results/millions.json').read_text())
        for profile in document['profiles'][:3]:
            report = profile['report']
            digest = report['build']['executable_sha256']
            campaign.validate_millions(report, report, document['config'], digest)
            for alteration in ['denominator', 'reopen', 'digest', 'backend']:
                modified = copy.deepcopy(report)
                if alteration == 'denominator':
                    modified['rows'][0]['transactions'] -= 1
                elif alteration == 'reopen':
                    modified['validation']['reopened_all_values_and_misses'] = False
                elif alteration == 'digest':
                    modified['build']['executable_sha256'] = 'wrong'
                else:
                    modified['backend'] = 'substituted backend'
                with self.subTest(engine=report['engine'], alteration=alteration), self.assertRaises(ValueError):
                    campaign.validate_millions(modified, report, document['config'], digest)
            if report['engine'] == 'lmdb':
                modified = copy.deepcopy(report)
                modified['settings']['flags'] = 0x10000
                with self.assertRaises(ValueError):
                    campaign.validate_millions(modified, report, document['config'], digest)


if __name__ == '__main__':
    unittest.main()
