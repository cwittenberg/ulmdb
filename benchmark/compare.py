#!/usr/bin/env python3
"""Compare complete benchmark captures without rerunning or publishing them.

Inputs are directories containing full.json and/or millions.json (a results/
subdirectory is also accepted), one such JSON file, or a {"full": ..., "millions":
...} capture. Use --paired only for a deliberately interleaved campaign whose
round/pair identifiers associate neighboring baseline/candidate processes.

No adverse estimate is hidden by a significance test. Overlapping intervals do
not establish equivalence. Exit 1 means adverse/unresolved target estimates,
insufficient repetitions, or unavailable target metrics; 2 means invalid input.
Exit 0 means the reported screen found none of those conditions, not proof of
universal nonregression. JSON retains every process observation and control.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def gm(values):
    return math.exp(statistics.fmean(math.log(x) for x in values))


def _beta_fraction(a, b, x):
    """Continued fraction for the regularized incomplete beta (Lentz method)."""
    tiny = 1e-300
    qab, qap, qam = a + b, a + 1, a - 1
    c = 1.0
    d = 1 - qab * x / qap
    d = 1 / (d if abs(d) > tiny else tiny)
    result = d
    for m in range(1, 401):
        m2 = 2 * m
        aa = m * (b - m) * x / ((qam + m2) * (a + m2))
        d = 1 + aa * d
        c = 1 + aa / c
        d = 1 / (d if abs(d) > tiny else tiny)
        c = c if abs(c) > tiny else tiny
        result *= d * c
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2))
        d = 1 + aa * d
        c = 1 + aa / c
        d = 1 / (d if abs(d) > tiny else tiny)
        c = c if abs(c) > tiny else tiny
        change = d * c
        result *= change
        if abs(change - 1) < 3e-14:
            return result
    raise ValueError('Student interval calculation did not converge')


def _regularized_beta(x, a, b):
    if x <= 0:
        return 0.0
    if x >= 1:
        return 1.0
    front = math.exp(math.lgamma(a + b) - math.lgamma(a) - math.lgamma(b)
                     + a * math.log(x) + b * math.log1p(-x))
    if x < (a + 1) / (a + b + 2):
        return front * _beta_fraction(a, b, x) / a
    return 1 - front * _beta_fraction(b, a, 1 - x) / b


def student_critical(alpha, degrees):
    """Two-sided Student-t critical value, including fractional Welch df."""
    require(0 < alpha < 1 and degrees > 0, 'invalid Student interval parameters')
    # For t >= 0, P(T >= t) = I[df/(df+t*t)](df/2, 1/2) / 2.
    def tail(t):
        return _regularized_beta(degrees / (degrees + t * t), degrees / 2, .5) / 2
    low, high = 0.0, 1.0
    while tail(high) > alpha / 2:
        high *= 2
        require(math.isfinite(high), 'Student interval overflow')
    for _ in range(80):
        middle = (low + high) / 2
        if tail(middle) > alpha / 2:
            low = middle
        else:
            high = middle
    return (low + high) / 2


def interval(baseline, candidate, paired, alpha):
    """Process-level log-ratio interval, or differences when zeros occur."""
    require(not paired or len(baseline) == len(candidate), 'paired observation counts differ')
    logarithmic = all(x > 0 for x in baseline + candidate)
    a = [math.log(x) if logarithmic else x for x in baseline]
    b = [math.log(x) if logarithmic else x for x in candidate]
    center = statistics.fmean(b) - statistics.fmean(a)
    if min(len(a), len(b)) < 2:
        return logarithmic, center, None, None
    if paired:
        changes = [y - x for x, y in zip(a, b)]
        center = statistics.fmean(changes)
        variance = statistics.variance(changes) / len(changes)
        degrees = len(changes) - 1
    else:
        va = statistics.variance(a) / len(a)
        vb = statistics.variance(b) / len(b)
        variance = va + vb
        degrees = (variance * variance /
                   (va * va / (len(a) - 1) + vb * vb / (len(b) - 1))) if variance else 1
    # A zero-width interval from identical small observations is descriptive;
    # the caller labels it separately and never calls it equivalence.
    radius = student_critical(alpha, degrees) * math.sqrt(variance) if variance else 0
    return logarithmic, center, [center - radius, center + radius], degrees


def load_capture(path):
    path = Path(path)
    files = []
    if path.is_dir():
        location = path if any((path / (x + '.json')).exists()
                               for x in ['full', 'millions']) else path / 'results'
        files = [location / (x + '.json') for x in ['full', 'millions']
                 if (location / (x + '.json')).exists()]
        require(files, f'no full.json or millions.json under {path}')
    else:
        files = [path]
    captures, provenance = {}, []
    for file in files:
        raw = file.read_bytes()
        document = json.loads(raw)
        provenance.append({'path': str(file.resolve()), 'sha256': hashlib.sha256(raw).hexdigest()})
        entries = {document['suite']: document} if document.get('suite') in ['full', 'millions'] else document
        for suite in ['full', 'millions']:
            if suite in entries:
                require(suite not in captures, f'duplicate {suite} input')
                require(isinstance(entries[suite], dict), f'{suite} capture is not an object')
                captures[suite] = entries[suite]
    require(captures, 'input contains no full or millions suite')
    return captures, provenance


def flatten(captures):
    metrics, signatures, metadata = {}, {}, {}

    def add(suite, filesystem, engine, name, unit, higher, pair, value, role=None):
        key = '/'.join([suite, filesystem, engine, name])
        metric = metrics.setdefault(key, {
            'suite': suite, 'filesystem': filesystem, 'engine': engine, 'metric': name,
            'unit': unit, 'higher_is_better': higher,
            'role': role or ('target' if engine == 'ulmdb' else 'control'), 'values': {},
        })
        require(pair not in metric['values'], f'duplicate observation {key} pair={pair}')
        require(value is None or (isinstance(value, (int, float)) and not isinstance(value, bool)
                                 and math.isfinite(value) and value >= 0), f'invalid metric {key}')
        metric['values'][pair] = value

    for suite, document in captures.items():
        require(document.get('suite') == suite, f'{suite} suite identity missing')
        require(document.get('rounds', 0) >= 1, f'{suite} rounds missing')
        metadata[suite] = {key: document.get(key) for key in
                           ['generated_at', 'machine', 'cpu', 'filesystems', 'source_sha256',
                            'backend', 'ulmdb_backend', 'config', 'storage', 'rounds']}
        seen, suite_signature = set(), None
        for profile in document['profiles']:
            report = profile['report']
            pair = str(profile.get('pair', profile.get('round')))
            require(pair != 'None', f'{suite} process needs round or pair identifier')
            filesystem = profile['filesystem'] if suite == 'full' else document['storage']
            if suite == 'full':
                require(filesystem in ['disk', 'ram'], 'unknown full-suite filesystem')
                require(report['semantic_validation_passed'] is True and report['timing_enabled'] is True,
                        'full suite validation/timing failed')
                require(report['native_flags'] == 0, 'LMDB full suite used relaxed flags')
                require(len(report['reads']) == 13 and len(report['writes']) == 2,
                        'full suite must contain all 13 reads and both writes')
                require(all(report['cold'][key] is True for key in
                            ['old_reader_survived_environment_drop', 'escaped_value_pointer_unchanged',
                             'reopened_value_uses_different_live_mapping']), 'full reopen/borrow checks failed')
                identity = (filesystem, pair)
                signature = {'config': report['config'], 'dataset': report['dataset_sha256'],
                             'queries': report['query_sha256'],
                             'reads': [{k: c[k] for k in ['name', 'queries', 'hits', 'matched_lengths',
                                                        'expected_checksum', 'query_indices']}
                                       for c in report['reads']],
                             'writes': [{k: c[k] for k in ['name', 'records_per_transaction',
                                                          'commits_per_aggregate_phase',
                                                          'separate_latency_phase_commits']}
                                        | {'traces': [t['trace_sha256'] for t in c['trials']]}
                                        for c in report['writes']]}
                for read, cases in [(True, report['reads']), (False, report['writes'])]:
                    for case in cases:
                        trials = case['trials']
                        require(len(trials) == report['config']['trials'] and len(trials) >= 2,
                                'full trial count differs from config')
                        batch = 1 if read else case['records_per_transaction']
                        name = case['name'] if read else f'durable_batch{batch}'
                        count = case['queries'] if read else case['commits_per_aggregate_phase'] * batch
                        for engine in ['ulmdb', 'lmdb']:
                            times = [t[engine]['elapsed_ns' if read else 'aggregate_elapsed_ns'] for t in trials]
                            require(all(x > 0 for x in times), 'nonpositive full elapsed time')
                            rate = count * len(trials) * 1e9 / sum(times)
                            tail = gm([t[engine]['timer_inclusive_p99_ns' if read else
                                                  'timer_inclusive_p99_transaction_ns'] for t in trials])
                            add(suite, filesystem, engine, name + '/throughput',
                                'queries/s' if read else 'records/s', True, pair, rate)
                            add(suite, filesystem, engine, name + '/sampled_p99', 'ns', False, pair, tail)
                require({c['records_per_transaction'] for c in report['combined_writes']} == {1, 64},
                        'combined write inventory missing')
                for case in report['combined_writes']:
                    for engine in ['ulmdb', 'lmdb']:
                        value = case[engine]
                        require(value['combined_elapsed_ns'] == value['aggregate_elapsed_ns']
                                + value['sampled_phase_elapsed_ns'], 'combined write denominator mismatch')
                        prefix = f"durable_batch{case['records_per_transaction']}/"
                        add(suite, filesystem, engine, prefix + 'combined_throughput', 'records/s', True,
                            pair, value['combined_records'] * 1e9 / value['combined_elapsed_ns'])
                        add(suite, filesystem, engine, prefix + 'pooled_sampled_p99', 'ns', False,
                            pair, value['pooled_sampled_transaction_p99_ns'])
                for key in ['environment_drop_ns', 'open_ns', 'experimental_reenable_ns',
                            'validation_ns', 'complete_cold_check_ns']:
                    add(suite, filesystem, 'ulmdb', 'reopen/' + key, 'ns', False,
                        pair, report['cold'].get(key))
                for engine in ['ulmdb', 'lmdb']:
                    add(suite, filesystem, engine, 'setup/create', 'ns', False, pair,
                        report['creation'][engine + '_create_ns'])
                    for stage in ['before', 'after']:
                        for key in ['files', 'apparent_file_bytes', 'allocated_file_bytes',
                                    'mapped_virtual_kib', 'mapped_rss_kib', 'mapped_pss_kib']:
                            unit = 'files' if key == 'files' else 'KiB' if key.endswith('_kib') else 'bytes'
                            add(suite, filesystem, engine, f'footprint/{stage}/{key}', unit, False,
                                pair, report[stage][engine].get(key),
                                role='diagnostic' if key.startswith('mapped_') else None)
                for key in ['dataset_ns', 'fixture_load_both_ns', 'after_setup_ns']:
                    add(suite, filesystem, 'harness', 'setup/' + key, 'ns', False,
                        pair, report['creation'].get(key), role='diagnostic')
            else:
                engine = report['engine']
                require(engine in ['ulmdb', 'lmdb', 'sqlite'], 'unknown million engine')
                identity = (engine, pair)
                require(report.get('status', 'complete') in ['complete', 'completed', 'success']
                        and report['validation']['passed'] is True, 'million workload failed validation')
                require(all(report['validation'][key] is True for key in
                            ['initial_all_values_and_misses', 'timed_read_hits_and_checksum',
                             'final_all_values_and_misses', 'reopened_all_values_and_misses',
                             'exact_row_count']), 'million workload lacks complete oracle validation')
                require(report['config'] == document['config'], 'million profile/config mismatch')
                signature = {'config': report['config'], 'workload': report['workload'], 'storage': filesystem}
                rows = {row['operation']: row for row in report['rows']}
                require(len(rows) == len(report['rows']) == 3 and set(rows) == {'load', 'read', 'update'},
                        'million operation inventory incomplete')
                for operation, count_key in [('load', 'keys'), ('read', 'reads'), ('update', 'updates')]:
                    row = rows[operation]
                    count = report['config'][count_key]
                    transactions = 1 if operation == 'read' else math.ceil(count / report['config']['batch'])
                    require(row['engine'] == engine and row['operations'] == count
                            and row['transactions'] == transactions and row['elapsed_ns'] > 0,
                            'million denominator/engine mismatch')
                    add(suite, filesystem, engine, operation + '/throughput', 'operations/s', True,
                        pair, count * 1e9 / row['elapsed_ns'])
                for key in ['setup_ns', 'close_reopen_ns']:
                    add(suite, filesystem, engine, key, 'ns', False, pair, report[key])
                for stage in ['storage_before_close', 'storage_after_close_reopen']:
                    for key in ['files', 'apparent_bytes', 'allocated_bytes']:
                        add(suite, filesystem, engine, stage + '/' + key,
                            'files' if key == 'files' else 'bytes', False, pair, report[stage][key])
                if engine == 'lmdb':
                    require(report['settings']['flags'] == 0, 'LMDB million suite used relaxed flags')
                if engine == 'sqlite':
                    settings = report['settings']
                    require(settings['journal_mode'].lower() == 'wal'
                            and str(settings['synchronous_numeric']) == '2', 'SQLite durability changed')
            require(identity not in seen, f'duplicate {suite} process {identity}')
            seen.add(identity)
            require(suite_signature is None or suite_signature == signature,
                    f'{suite} profiles disagree on workload identity')
            suite_signature = signature
        expected_groups = ['disk', 'ram'] if suite == 'full' else ['ulmdb', 'lmdb', 'sqlite']
        for group in expected_groups:
            require(sum(identity[0] == group for identity in seen) == document['rounds'],
                    f'{suite}/{group} missing requested processes')
        signatures[suite] = suite_signature
    return metrics, signatures, metadata


def compare(baseline, candidate, *, paired=False, alpha=.05, min_repeats=4, adjustment='bonferroni'):
    a, a_signatures, a_metadata = flatten(baseline)
    b, b_signatures, b_metadata = flatten(candidate)
    require(a_signatures == b_signatures, 'baseline/candidate suite or workload identities differ')
    require(set(a) == set(b), 'baseline/candidate metric inventories differ')
    warnings = []
    for suite in a_metadata:
        for key in ['cpu', 'filesystems']:
            av, bv = a_metadata[suite][key], b_metadata[suite][key]
            if key == 'filesystems' and av and bv:
                av = {k: v['type'] for k, v in av.items()}
                bv = {k: v['type'] for k, v in bv.items()}
            require(av == bv, f'{suite} {key} differs')
        if a_metadata[suite]['machine'] != b_metadata[suite]['machine']:
            warnings.append(f'{suite}: machine metadata differ; inspect environmental drift')
    metric_alpha = alpha / len(a) if adjustment == 'bonferroni' else alpha
    rows = []
    for key in sorted(a):
        left, right = a[key], b[key]
        require(all(left[k] == right[k] for k in left if k != 'values'), f'metric meaning differs: {key}')
        av, bv = left['values'], right['values']
        if paired:
            require(set(av) == set(bv), f'paired process identifiers differ: {key}')
        row = {k: v for k, v in left.items() if k != 'values'}
        row.update(id=key, baseline_observations=av, candidate_observations=bv,
                   baseline_runs=len(av), candidate_runs=len(bv), flags=[])
        if any(v is None for v in list(av.values()) + list(bv.values())):
            row.update(status='unavailable', flags=['unavailable'], ratio=None, change_pct=None,
                       interval=None, adverse_point_estimate=None)
            rows.append(row)
            continue
        x, y = [av[k] for k in sorted(av)], [bv[k] for k in sorted(bv)]
        logarithmic, center, bounds, degrees = interval(x, y, paired, metric_alpha)
        ratio = math.exp(center) if logarithmic else None
        less_is_worse = row['higher_is_better']
        adverse = center < -1e-14 if less_is_worse else center > 1e-14
        row.update(baseline_center=gm(x) if logarithmic else statistics.fmean(x),
                   candidate_center=gm(y) if logarithmic else statistics.fmean(y),
                   ratio=ratio, change_pct=100 * math.expm1(center) if logarithmic else None,
                   interval=([math.exp(v) for v in bounds] if logarithmic else bounds),
                   interval_scale='candidate/baseline ratio' if logarithmic else 'candidate-baseline difference',
                   degrees_of_freedom=degrees, adverse_point_estimate=adverse)
        if adverse:
            row['flags'].append('adverse_point_estimate')
        if min(len(x), len(y)) < min_repeats:
            row['flags'].append('insufficient_repeats')
        if bounds is None:
            row['status'] = 'insufficient_repeats'
        elif bounds[0] == bounds[1]:
            row['status'] = 'identical_observed_changes'
            if adverse:
                row['flags'].append('consistent_observed_regression')
        elif (bounds[1] < 0 if less_is_worse else bounds[0] > 0):
            row['status'] = 'regression_interval'
            row['flags'].append('regression_interval')
        elif (bounds[0] > 0 if less_is_worse else bounds[1] < 0):
            row['status'] = 'improvement_interval'
        else:
            row['status'] = 'inconclusive_interval'
        rows.append(row)
    targets = [r for r in rows if r['role'] == 'target']
    flags = ['regression_interval', 'consistent_observed_regression', 'adverse_point_estimate',
             'insufficient_repeats', 'unavailable']
    summary = {flag: [r['id'] for r in targets if flag in r['flags']] for flag in flags}
    summary.update(target_metrics=len(targets), all_metrics=len(rows),
                   screen_clear=not any(summary[flag] for flag in flags))
    return {'schema': 1, 'generated_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'method': {'process_comparison': 'paired round/pair log ratios' if paired else 'independent Welch log means',
                       'familywise_alpha': alpha if adjustment == 'bonferroni' else None,
                       'per_metric_alpha': metric_alpha, 'adjustment': adjustment,
                       'minimum_process_repeats': min_repeats,
                       'zero_values': 'use arithmetic differences when any observation is zero',
                       'assumptions': 'independent processes; approximately normal log costs/rates or paired log changes',
                       'pairing': 'round/pair identifiers are associations only; caller must establish actual interleaving',
                       'scope': 'descriptive regression screen; no equivalence, universal no-regression, or causal proof; '
                                'all adverse estimates retained; no outlier exclusion or local-LMDB normalization'},
            'summary': summary, 'warnings': warnings, 'baseline_metadata': a_metadata,
            'candidate_metadata': b_metadata, 'rows': rows}


def markdown(result):
    summary = result['summary']
    lines = ['# Benchmark comparison', '',
             f"{summary['target_metrics']} ULMDB metrics; {summary['all_metrics']} total including controls.", '',
             f"Adverse ULMDB point estimates: {len(summary['adverse_point_estimate'])}; "
             f"adverse intervals: {len(summary['regression_interval'])}; "
             f"consistent observed increases/decreases: {len(summary['consistent_observed_regression'])}; "
             f"insufficient repeats: {len(summary['insufficient_repeats'])}; "
             f"unavailable: {len(summary['unavailable'])}.", '',
             result['method']['scope'] + '.', '',
             'Change is candidate versus baseline. Throughput increases are favorable; cost increases are adverse. '
             'Intervals describe process variation under the stated model, not population latency percentiles.', '',
             '|Metric|Role|Baseline|Candidate|Change|Interval|Status / flags|',
             '|---|---|---:|---:|---:|---|---|']
    for row in result['rows']:
        def value(key):
            return f"{row[key]:,.3f}" if row.get(key) is not None else 'Unavailable'
        change = f"{row['change_pct']:+.2f}%" if row['change_pct'] is not None else 'n/a'
        bounds = row['interval']
        interval_text = (f"[{bounds[0]:.5g}, {bounds[1]:.5g}] "
                         + ('ratio' if row['interval_scale'].endswith('ratio') else 'difference')) if bounds else 'n/a'
        flags = ', '.join(row['flags'])
        lines.append(f"|{row['id']} ({row['unit']})|{row['role']}|{value('baseline_center')}|"
                     f"{value('candidate_center')}|{change}|{interval_text}|{row['status']}"
                     + (f'; {flags}' if flags else '') + '|')
    if result['warnings']:
        lines += ['', 'Warnings:', '', *['- ' + x for x in result['warnings']]]
    return '\n'.join(lines) + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('baseline', type=Path)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('--paired', action='store_true', help='declare interleaved round/pair associations')
    parser.add_argument('--alpha', type=float, default=.05, help='familywise error budget (default .05)')
    parser.add_argument('--adjustment', choices=['bonferroni', 'none'], default='bonferroni')
    parser.add_argument('--min-repeats', type=int, default=4)
    parser.add_argument('--json', type=Path, help='write complete machine-readable comparison')
    parser.add_argument('--markdown', type=Path, help='write complete Markdown table')
    args = parser.parse_args(argv)
    try:
        require(0 < args.alpha < 1 and args.min_repeats >= 2, 'alpha must be in (0,1), minimum repeats >= 2')
        baseline, baseline_files = load_capture(args.baseline)
        candidate, candidate_files = load_capture(args.candidate)
        result = compare(baseline, candidate, paired=args.paired, alpha=args.alpha,
                         min_repeats=args.min_repeats, adjustment=args.adjustment)
        result.update(baseline_inputs=baseline_files, candidate_inputs=candidate_files)
        if args.json:
            args.json.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
        if args.markdown:
            args.markdown.write_text(markdown(result))
        if not args.json and not args.markdown:
            print(markdown(result), end='')
        else:
            print(json.dumps(result['summary'], indent=2))
        return 0 if result['summary']['screen_clear'] else 1
    except (OSError, ValueError, KeyError, TypeError, ZeroDivisionError, OverflowError) as error:
        print(f'invalid benchmark comparison: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
