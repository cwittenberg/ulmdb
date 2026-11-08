"""Three-arm schedule, provenance and untimed orchestration/failure checks."""
import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('ulmdb_three_arm_draft', Path(__file__).with_name('three_arm.py'))
draft = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(draft)


class ThreeArmDraftTests(unittest.TestCase):
    def test_reference_preflight_only_allows_historical_sqlite_hash_context(self):
        capture = {suite: json.loads((draft.BENCHMARK / 'results' / (suite + '.json')).read_text())
                   for suite in ['full', 'millions']}
        references = {arm: copy.deepcopy(capture) for arm in ['initial', 'accepted']}
        for arm, digest in [('initial', '1' * 64), ('accepted', '2' * 64)]:
            for profile in references[arm]['millions']['profiles']:
                if profile['report']['engine'] == 'sqlite':
                    profile['report']['versions']['link_library_sha256'] = digest
        accepted = next(p['report']['versions'] for p in references['accepted']['millions']['profiles']
                        if p['report']['engine'] == 'sqlite')
        context = draft.check_references(references, {'versions': accepted})
        self.assertTrue(context['historical_link_hash_only_change'])
        with self.assertRaisesRegex(ValueError, 'requires verified'):
            draft.check_references(references)
        for key in ['sqlite', 'source_id', 'link_library_path']:
            altered = copy.deepcopy(references)
            for profile in altered['initial']['millions']['profiles']:
                if profile['report']['engine'] == 'sqlite':
                    profile['report']['versions'][key] = 'different'
            with self.subTest(archived=key), self.assertRaisesRegex(ValueError, 'version, source_id or link path'):
                draft.check_references(altered, {'versions': accepted})
        with self.assertRaisesRegex(ValueError, 'runtime identity differs'):
            draft.check_references(references, {'versions': dict(accepted, link_library_sha256='3' * 64)})
        altered = copy.deepcopy(references)
        for profile in altered['initial']['millions']['profiles']:
            if profile['report']['engine'] == 'lmdb':
                profile['report']['versions']['unreviewed'] = 'change'
        with self.assertRaisesRegex(ValueError, 'lmdb versions differ'):
            draft.check_references(altered, {'versions': accepted})
        self.assertEqual(next(p['report']['versions']['link_library_sha256']
                              for p in references['initial']['millions']['profiles']
                              if p['report']['engine'] == 'sqlite'), '1' * 64)

    def test_final_guards_reject_sqlite_runtime_byte_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory).resolve() / 'sqlite.so'
            library.write_bytes(b'pinned runtime')
            versions = {'sqlite': 'version', 'source_id': 'source',
                        'link_library_path': str(library), 'link_library_sha256': draft.runner.sha(library)}
            context = {'runtime_pin': {'versions': versions}, 'runtime': versions}
            with mock.patch.object(draft.runner, 'snapshot', return_value={}), \
                 mock.patch.object(draft.base, 'sqlite_runtime_versions', return_value={'sqlite': 'version', 'source_id': 'source'}):
                self.assertEqual(draft.verify_guards({}, {}, {}, {}, context), {})
                library.write_bytes(b'changed runtime')
                with self.assertRaisesRegex(ValueError, 'bytes changed'):
                    draft.verify_guards({}, {}, {}, {}, context)

    def test_complete_counts_neighboring_groups_and_million_position_balance(self):
        steps = draft.schedule()
        self.assertEqual(len(steps), 39)
        full = [s for s in steps if s['suite'] == 'full']
        self.assertEqual(len(full), 24)
        for start in range(0, len(full), 3):
            group = full[start:start + 3]
            self.assertEqual({s['arm'] for s in group}, set(draft.ARMS))
            self.assertEqual(len({(s['filesystem'], s['round']) for s in group}), 1)
        for filesystem in ['disk', 'ram']:
            first_three = [[s['arm'] for s in full if s['filesystem'] == filesystem and s['round'] == number]
                           for number in range(3)]
            for position in range(3):
                self.assertEqual({order[position] for order in first_three}, set(draft.ARMS))
        million_orders = []
        for number in range(3):
            targets = [s for s in steps if s['suite'] == 'millions'
                       and s['engine'] == 'ulmdb' and s['round'] == number]
            million_orders.append([s['arm'] for s in targets])
            controls = [s for s in steps if s['suite'] == 'millions'
                        and s['engine'] != 'ulmdb' and s['round'] == number]
            self.assertEqual({s['engine'] for s in controls}, {'lmdb', 'sqlite'})
            self.assertTrue(all(s['arm'] == 'shared_control' for s in controls))
        for position in range(3):
            self.assertEqual({order[position] for order in million_orders}, set(draft.ARMS))

    def test_target_hash_does_not_misattribute_shared_control_binary(self):
        document = {'profiles': [
            {'report': {'engine': 'ulmdb', 'build': {'executable_sha256': 'target'}}},
            {'report': {'engine': 'lmdb', 'build': {'executable_sha256': 'shared-controls'}}},
            {'report': {'engine': 'sqlite', 'build': {'executable_sha256': 'shared-controls'}}},
        ]}
        self.assertEqual(draft.target_hash(document, 'millions'), 'target')
        document['profiles'].append({'report': {'engine': 'ulmdb', 'build': {'executable_sha256': 'substituted'}}})
        with self.assertRaises(ValueError):
            draft.target_hash(document, 'millions')

    def test_source_binary_tool_and_trace_drift_each_fail_closed(self):
        before = {'source': 'original'}
        with mock.patch.object(draft.runner, 'snapshot', return_value={'source': 'changed'}):
            with self.assertRaisesRegex(ValueError, 'source changed'):
                draft.verify_guards(before, {}, {}, {})
        for kind in ['executable', 'tool', 'trace']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / kind
                path.write_bytes(b'original')
                original = draft.runner.sha(path)
                executables = {'initial': {'full': {'path': str(path), 'sha256': original}}} \
                    if kind == 'executable' else {}
                tools = {str(path): original} if kind == 'tool' else {}
                traces = {str(path): original} if kind == 'trace' else {}
                with mock.patch.object(draft.runner, 'snapshot', return_value=before):
                    self.assertEqual(draft.verify_guards(before, executables, tools, traces), before)
                    path.write_bytes(b'changed')
                    with self.assertRaises(ValueError):
                        draft.verify_guards(before, executables, tools, traces)


class OrchestrationTests(unittest.TestCase):
    """Real schemas/statistics; fake tiny executables/children, no timed engines."""
    @classmethod
    def setUpClass(cls):
        root = draft.BENCHMARK / 'work/optimization-20261105'
        # Historical private captures are optional on fresh clones. The pure
        # tests above and existing shared validator tests still run there.
        cls.reference_paths = {'initial': root / 'baseline', 'accepted': root / 'iteration-1/candidate'}
        if not all((p / 'full.json').exists() and (p / 'millions.json').exists()
                   for p in cls.reference_paths.values()):
            raise unittest.SkipTest('local historical captures unavailable')
        cls.references = {arm: draft.comparison.load_capture(path)[0]
                          for arm, path in cls.reference_paths.items()}

    def run_mocked_campaign(self, root, fail_at=None, runtime_pin_failure_at=None):
        output, ram = root / 'campaign', root / 'ram'
        ram.mkdir()
        sources = {'candidate': {'test-source': 'stable'}} | {
            arm: reference['full']['source_sha256'] for arm, reference in self.references.items()}
        steps = draft.schedule()
        calls, source_bindings, runtime_checks = [], [], []

        def runtime_pin(pin):
            runtime_checks.append(pin)
            if len(runtime_checks) == runtime_pin_failure_at:
                raise ValueError('pinned SQLite runtime bytes changed')

        def preserve(original, target, expected=None):
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(str(target).encode())
            target.chmod(0o555)
            return {'original': str(original), 'path': str(target), 'sha256': draft.runner.sha(target)}

        def filesystem(path):
            return {'type': 'tmpfs' if Path(path).resolve().is_relative_to(ram) else 'test-disk', 'device': 1}

        def child(argv, fixture, env, cpu):
            step = steps[len(calls)]
            calls.append(step)
            fixture.mkdir()
            (fixture / 'owner').write_text('fixture retained until validated')
            fixture.with_name(fixture.name + '.stdout.json').write_text('{"captured":true}\n')
            fixture.with_name(fixture.name + '.stderr.log').write_text('mock child stderr\n')
            if step['number'] == fail_at:
                (fixture / 'resident-failure.json').write_text('{"status":"failed"}\n')
                raise RuntimeError('injected child failure')
            arm = 'candidate' if step['arm'] == 'shared_control' else step['arm']
            reference = self.references['initial' if arm == 'initial' else 'accepted']
            if step['suite'] == 'full':
                profile = next(p for p in reference['full']['profiles']
                               if p['filesystem'] == step['filesystem'] and p['round'] == step['round'])
                report = copy.deepcopy(profile['report'])
                report['binary_sha256'] = draft.runner.sha(Path(argv[0]))
                raw = fixture / report['raw_latency_file']
                raw.write_bytes(b'untimed orchestration trace\n')
                report['raw_latency_sha256'] = draft.runner.sha(raw)
            else:
                profile = next(p for p in reference['millions']['profiles']
                               if p['report']['engine'] == step['engine'] and p['round'] == step['round'])
                report = copy.deepcopy(profile['report'])
                report['build']['executable_sha256'] = draft.runner.sha(Path(argv[0]))
            return report

        def full_validator(report, fixture, expected, digest, source):
            arm = fixture.name.rsplit('-', 1)[1]
            self.assertEqual(source, sources[arm])
            self.assertEqual(report['binary_sha256'], digest)
            self.assertEqual(draft.base.full_signature(report), draft.base.full_signature(expected))
            source_bindings.append(arm)
            return fixture / report['raw_latency_file']

        argv = ['--initial', str(self.reference_paths['initial']), '--accepted', str(self.reference_paths['accepted']),
                '--candidate-full', str(self.reference_paths['accepted'] / 'binaries/ulmdb-engine-benchmark'),
                '--candidate-millions', str(self.reference_paths['accepted'] / 'binaries/ulmdb-millions'),
                '--output', str(output), '--ram-dir', str(ram), '--cpu', str(min(os.sched_getaffinity(0)))]
        sqlite_versions = next(p['report']['versions'] for p in self.references['accepted']['millions']['profiles']
                               if p['report']['engine'] == 'sqlite')
        runtime = {'versions': sqlite_versions, 'loader_path': sqlite_versions['link_library_path'],
                   'canonical_path': sqlite_versions['link_library_path']}
        with mock.patch.object(draft.base, 'preserve_executable', side_effect=preserve), \
             mock.patch.object(draft.base, 'validate_full', side_effect=full_validator), \
             mock.patch.object(draft.base, 'inspect_sqlite_runtime', return_value=runtime), \
             mock.patch.object(draft.base, 'verify_sqlite_runtime_pin', side_effect=runtime_pin), \
             mock.patch.object(draft.runner, 'snapshot', return_value=sources['candidate']), \
             mock.patch.object(draft.runner, 'filesystem_info', side_effect=filesystem), \
             mock.patch.object(draft.runner, 'child_json', side_effect=child), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            code = draft.main(argv)
        return output, code, calls, source_bindings

    def test_all_39_untimed_children_emit_three_complete_captures_and_both_comparisons(self):
        with tempfile.TemporaryDirectory() as directory:
            output, code, calls, bindings = self.run_mocked_campaign(Path(directory))
            self.assertIn(code, [0, 1])  # Real historical adverse estimates may make screen non-clear.
            self.assertEqual(len(calls), 39)
            self.assertEqual({arm: bindings.count(arm) for arm in draft.ARMS},
                             {arm: 8 for arm in draft.ARMS})
            manifest = json.loads((output / 'campaign.json').read_text())
            self.assertEqual(manifest['status'], 'complete')
            self.assertTrue(manifest['source_unchanged'])
            self.assertEqual(len(manifest['raw_traces_sha256']), 24)
            captures = {}
            for arm in draft.ARMS:
                captures[arm] = draft.comparison.load_capture(output / arm)[0]
                draft.comparison.flatten(captures[arm])
                self.assertEqual(len(captures[arm]['full']['profiles']), 8)
                self.assertEqual(len(captures[arm]['millions']['profiles']), 9)
                self.assertFalse((output / arm / 'full.partial.json').exists())
            for engine in ['lmdb', 'sqlite']:
                observations = [[p for p in captures[arm]['millions']['profiles']
                                 if p['report']['engine'] == engine] for arm in draft.ARMS]
                self.assertEqual(observations[0], observations[1])
                self.assertEqual(observations[1], observations[2])
                self.assertTrue(all(p['source_arm'] == 'candidate' and p['shared_control']
                                    for p in observations[0]))
            for arm in ['initial', 'accepted']:
                comparison = json.loads((output / (arm + '-vs-candidate.json')).read_text())
                self.assertEqual(comparison['reference_arm'], arm)
                self.assertEqual(comparison['candidate_arm'], 'candidate')
                self.assertEqual(comparison['summary']['target_metrics'], 103)
            self.assertEqual(len(list((output / 'logs').glob('*.stdout.json'))), 39)
            self.assertEqual(len(list((output / 'logs').glob('*.stderr.log'))), 39)

    def test_mid_group_failure_keeps_fixture_logs_receipt_and_prior_partial_capture(self):
        for failed_step in [1, 4]:  # Disk and RAM failures use the same durable log path.
            with self.subTest(failed_step=failed_step), tempfile.TemporaryDirectory() as directory:
                output, code, calls, _ = self.run_mocked_campaign(Path(directory), fail_at=failed_step)
                self.assertEqual(code, 2)
                self.assertEqual(len(calls), failed_step + 1)
                manifest = json.loads((output / 'campaign.json').read_text())
                self.assertEqual(manifest['status'], 'failed')
                self.assertEqual(manifest['failed_step']['number'], failed_step)
                self.assertTrue(Path(manifest['retained_failed_fixture']).is_dir())
                self.assertTrue((output / 'initial/full.partial.json').exists())
                self.assertEqual(len(list((output / 'logs').glob('*.stdout.json'))), failed_step + 1)
                self.assertEqual(len(list((output / 'logs').glob('*.stderr.log'))), failed_step + 1)
                self.assertEqual(len(list((output / 'logs').glob('*.resident-failure.json'))), 1)
                self.assertFalse((output / 'initial/full.json').exists())
                self.assertFalse((output / 'accepted-vs-candidate.json').exists())

    def test_runtime_mutation_after_child_rejects_result_and_stops_campaign(self):
        with tempfile.TemporaryDirectory() as directory:
            output, code, calls, _ = self.run_mocked_campaign(Path(directory), runtime_pin_failure_at=2)
            self.assertEqual(code, 2)
            self.assertEqual(len(calls), 1)
            manifest = json.loads((output / 'campaign.json').read_text())
            self.assertEqual(manifest['status'], 'failed')
            self.assertEqual(manifest['completed'], [])
            self.assertIn('runtime bytes changed', manifest['error'])
            self.assertTrue(Path(manifest['retained_failed_fixture']).is_dir())
            self.assertEqual(len(list((output / 'logs').glob('*.stdout.json'))), 1)
            self.assertFalse((output / 'initial' / 'full.json').exists())


if __name__ == '__main__':
    unittest.main()
