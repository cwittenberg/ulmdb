"""Render the two public benchmark reports from measured JSON, without engine runs."""
from pathlib import Path
import argparse
import json
import math
import statistics
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / '.cache/python'))


def gm(values):
    values = list(values)
    if not values or any(not math.isfinite(x) or x <= 0 for x in values):
        raise ValueError('expected positive, finite measurements')
    return math.exp(statistics.fmean(map(math.log, values)))


def verdict(ulmdb, lmdb, higher_is_better=True):
    gap = 100 * (ulmdb / lmdb - 1)
    if abs(gap) <= 2:
        return 'Roughly on par'
    favorable = gap > 0 if higher_is_better else gap < 0
    return ('Faster' if favorable else 'Slower') if higher_is_better else ('Lower cost' if favorable else 'Higher cost')


def review_note(data):
    review = data.get('performance_review')
    if not review:
        return []
    return [f"**Measurement status: {review['label']}.** {review['note']}", '']


def chart_title(title, data):
    review = data.get('performance_review')
    return title + (f" — {review['chart_label']}" if review else '')


def lifecycle_costs(data):
    groups = {engine: [] for engine in ['ulmdb', 'lmdb', 'sqlite']}
    for profile in data['profiles']:
        report = profile['report']
        if report['engine'] in groups:
            groups[report['engine']].append(report)
    if not all(len(reports) == data['rounds'] and
               all(r['validation']['passed'] and 'close_reopen_ns' in r and
                   'storage_after_close_reopen' in r for r in reports)
               for reports in groups.values()):
        return None
    return {engine: {
        'setup_ms': gm(r['setup_ns'] for r in reports) / 1e6,
        'reopen_ms': gm(r['close_reopen_ns'] for r in reports) / 1e6,
        'apparent_mib': gm(r['storage_after_close_reopen']['apparent_bytes'] for r in reports) / (1 << 20),
        'allocated_mib': gm(r['storage_after_close_reopen']['allocated_bytes'] for r in reports) / (1 << 20),
    } for engine, reports in groups.items()}


def cost_table(costs):
    lines = ['Lower time/space is better; positive excess cost is worse for ULMDB.', '',
             '|Cost|ULMDB|LMDB|SQLite|ULMDB excess vs LMDB|Verdict|',
             '|---|---:|---:|---:|---:|---|']
    for label, key in [('Setup, ms', 'setup_ms'), ('Close/reopen, ms', 'reopen_ms'),
                       ('Apparent files, MiB', 'apparent_mib'), ('Allocated files, MiB', 'allocated_mib')]:
        u, l, q = (costs[e][key] for e in ['ulmdb', 'lmdb', 'sqlite'])
        lines.append(f'|{label}|{u:,.3f}|{l:,.3f}|{q:,.3f}|{100 * (u/l-1):+.2f}%|{verdict(u,l,False)}|')
    return lines


def plot(labels, series, title, destination, xlabel, logarithmic=False, missing_labels=None, percentage_gain=False):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    plt.rcParams.update({'font.size': 10, 'svg.hashsalt': 'ulmdb-benchmark'})
    fig, ax = plt.subplots(figsize=(11, max(3.2, len(labels) * .32 + 1.6)))
    colors = ['#2864a0', '#17916d', '#df9041']
    count = len(series)
    width = .74 / count
    for index, (name, values) in enumerate(series.items()):
        color = (colors[0] if name.upper().startswith('ULMDB') else
                 colors[1] if name.upper().startswith('LMDB') else
                 colors[2] if name.upper().startswith('SQLITE') else colors[index % 3])
        positions = [y + (index - (count - 1) / 2) * width for y in range(len(labels))]
        measured = [float('nan') if value is None else value for value in values]
        bar_colors = ([('#b64d32' if value < -2 else '#777777' if abs(value) <= 2 else colors[0])
                       if value is not None else color for value in values] if percentage_gain else color)
        ax.barh(positions, measured, height=width, label=name, color=bar_colors)
        for item, value in enumerate(values):
            if value is None:
                note = (missing_labels or {}).get((name, item), 'Unavailable')
                ax.text(.01, positions[item], note, transform=ax.get_yaxis_transform(),
                        va='center', fontsize=8, color=color)
            elif percentage_gain:
                ax.annotate(f'{value:+.1f}%', (value, positions[item]),
                            xytext=(4 if value >= 0 else -4, 0), textcoords='offset points',
                            ha='left' if value >= 0 else 'right', va='center', fontsize=8)
    ax.set_yticks(range(len(labels)), labels)
    ax.invert_yaxis()
    ax.set_xlabel(xlabel)
    ax.set_title(title, loc='left', fontweight='bold', pad=16)
    if logarithmic:
        ax.set_xscale('log')
        ax.axvline(1, color='#555555', linestyle='--', linewidth=1)
    if percentage_gain:
        ax.axvline(0, color='#555555', linestyle='--', linewidth=1)
        present = [v for values in series.values() for v in values if v is not None]
        lo, hi = min([0, *present]), max([0, *present])
        padding = max(10, (hi - lo) * .12)
        ax.set_xlim(lo - padding, hi + padding)
    ax.grid(axis='x', alpha=.2)
    ax.set_axisbelow(True)
    ax.spines[['top', 'right']].set_visible(False)
    if percentage_gain:
        from matplotlib.patches import Patch
        ax.legend(handles=[Patch(color=colors[0], label='Faster than LMDB'),
                           Patch(color='#b64d32', label='Slower than LMDB'),
                           Patch(color='#777777', label='Within 2% (descriptive)')], loc='best')
    else:
        ax.legend(loc='best')
    fig.tight_layout()
    destination.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(destination, format='svg', metadata={
        'Date': None, 'Title': title,
        'Description': json.dumps({'labels': labels, 'series': series,
                                   'xlabel': xlabel, 'logarithmic': logarithmic,
                                   'percentage_gain': percentage_gain}, sort_keys=True),
    })
    plt.close(fig)
    destination.write_text('\n'.join(line.rstrip() for line in destination.read_text().splitlines()) + '\n')


def full_measurements(data):
    groups = {}
    for profile in data['profiles']:
        report = profile['report']
        assert report['semantic_validation_passed'] and report['timing_enabled']
        assert len(report['reads']) == 13 and len(report['writes']) == 2
        groups.setdefault(profile['filesystem'], []).append(report)
    rows, combined, cold = [], [], []
    for filesystem, reports in groups.items():
        assert len(reports) == data['rounds']
        for case_index in range(15):
            read = case_index < 13
            cases = [r['reads'][case_index] if read else r['writes'][case_index - 13] for r in reports]
            batch = 1 if read else cases[0]['records_per_transaction']
            name = cases[0]['name'] if read else f'durable_batch{batch}'
            row = {'filesystem': filesystem, 'case': name}
            for engine in ['ulmdb', 'lmdb']:
                rates, tails = [], []
                for case in cases:
                    number = case['queries'] if read else case['commits_per_aggregate_phase']
                    elapsed = sum(t[engine]['elapsed_ns' if read else 'aggregate_elapsed_ns'] for t in case['trials'])
                    rates.append(number * len(case['trials']) * batch * 1e9 / elapsed)
                    tails.append(gm(t[engine]['timer_inclusive_p99_ns' if read else 'timer_inclusive_p99_transaction_ns'] for t in case['trials']))
                row[engine] = gm(rates)
                row[engine + '_p99_ns'] = gm(tails)
                row[engine + '_range'] = [min(rates), max(rates)]
            row['gap_pct'] = 100 * (row['ulmdb'] / row['lmdb'] - 1)
            rows.append(row)
        for index, batch in enumerate([1, 64]):
            values = [r['combined_writes'][index] for r in reports]
            assert all(x['records_per_transaction'] == batch for x in values)
            result = {'filesystem': filesystem, 'batch': batch}
            for engine in ['ulmdb', 'lmdb']:
                rates = []
                for value in values:
                    x = value[engine]
                    assert x['combined_elapsed_ns'] == x['aggregate_elapsed_ns'] + x['sampled_phase_elapsed_ns']
                    rates.append(x['combined_records'] * 1e9 / x['combined_elapsed_ns'])
                result[engine] = gm(rates)
            result['gap_pct'] = 100 * (result['ulmdb'] / result['lmdb'] - 1)
            combined.append(result)
        cold.append({'filesystem': filesystem, 'open_ms': gm(r['cold']['open_ns'] for r in reports) / 1e6})
    return rows, combined, cold


def full(data):
    rows, combined, cold = full_measurements(data)
    paired = data.get('paired_baseline')
    if paired:
        previous = {(r['filesystem'], r['case']): r for r in paired['rows']}
        assert len(previous) == len(rows) == 30
        for row in rows:
            baseline = previous[row['filesystem'], row['case']]
            row['previous_ulmdb'] = baseline['ulmdb']
            row['change_pct'] = 100 * (row['ulmdb'] / baseline['ulmdb'] - 1)
            row['previous_ulmdb_p99_ns'] = baseline['ulmdb_p99_ns']
            row['p99_change_pct'] = 100 * (row['ulmdb_p99_ns'] / baseline['ulmdb_p99_ns'] - 1)
    labels = [f"{r['filesystem']} / {r['case'].replace('_', ' ')}" for r in rows]
    plot(labels, {'ULMDB vs LMDB': [r['gap_pct'] for r in rows]},
         chart_title('ULMDB throughput improvement versus LMDB', data), HERE / 'graphs/full.svg',
         'Percent versus local LMDB; positive = faster, negative = slower', percentage_gain=True)
    lines = ['# ULMDB vs LMDB', '', *review_note(data), f"Measured {data['generated_at']}. Backend: **{data['backend']}**. "
             f"{data['rounds']} fresh processes per filesystem; release build, portable CPU baseline. "
             'LMDB runs locally inside every process with normal durability flags (`0`).', '',
             '![Measured throughput percentage improvement versus LMDB](graphs/full.svg)', '',
             '## Complete read/write table', '',
             'Rates are queries/s for reads and updated records/s for writes. Positive gap favors ULMDB. '
             'Values are geometric means of process rates; observed ranges are in the JSON and are not confidence intervals. '
             'Roughly on par means within 2% of the observed centre, not statistical equivalence.', '',
             *(['The previous ULMDB column is the frozen prior executable, rerun locally in the same '
                'interleaved campaign with identical workloads; exact process order is retained in the raw evidence. '
                'Every result is retained, including lower means and slower sampled tails. '
                'Baseline rates, sampled tails and its contemporaneous LMDB measurements are in '
                '`paired_baseline` in the JSON.', '',
                '|Filesystem / operation|Previous ULMDB/s|Current ULMDB/s|Change vs prior ULMDB|Fresh LMDB/s|ULMDB vs LMDB|Verdict|',
                '|---|---:|---:|---:|---:|---:|---|'] if paired else
               ['|Filesystem / operation|LMDB/s|ULMDB/s|ULMDB vs LMDB|Verdict|', '|---|---:|---:|---:|---|'])]
    for r in rows:
        if paired:
            lines.append(f"|{r['filesystem']} / {r['case']}|{r['previous_ulmdb']:,.0f}|{r['ulmdb']:,.0f}|"
                         f"{r['change_pct']:+.2f}%|{r['lmdb']:,.0f}|{r['gap_pct']:+.2f}%|{verdict(r['ulmdb'],r['lmdb'])}|")
        else:
            lines.append(f"|{r['filesystem']} / {r['case']}|{r['lmdb']:,.0f}|{r['ulmdb']:,.0f}|{r['gap_pct']:+.2f}%|{verdict(r['ulmdb'],r['lmdb'])}|")
    lines += ['', '## Sampled p99', '', 'Nanoseconds; lower is better. Write p99 measures a complete transaction. '
              'It is the geometric mean of per-trial sampled p99 values, not an all-operation population percentile.', '',
              *(['|Filesystem / operation|Previous ULMDB ns|Current ULMDB ns|Change vs prior ULMDB|Fresh LMDB ns|ULMDB excess latency vs LMDB|Verdict|',
                 '|---|---:|---:|---:|---:|---:|---|'] if paired else
                ['|Filesystem / operation|LMDB ns|ULMDB ns|ULMDB excess latency vs LMDB|Verdict|', '|---|---:|---:|---:|---|'])]
    for r in rows:
        if paired:
            lines.append(f"|{r['filesystem']} / {r['case']}|{r['previous_ulmdb_p99_ns']:,.2f}|{r['ulmdb_p99_ns']:,.2f}|"
                         f"{r['p99_change_pct']:+.2f}%|{r['lmdb_p99_ns']:,.2f}|{100*(r['ulmdb_p99_ns']/r['lmdb_p99_ns']-1):+.2f}%|{verdict(r['ulmdb_p99_ns'],r['lmdb_p99_ns'],False)}|")
        else:
            lines.append(f"|{r['filesystem']} / {r['case']}|{r['lmdb_p99_ns']:,.2f}|{r['ulmdb_p99_ns']:,.2f}|{100*(r['ulmdb_p99_ns']/r['lmdb_p99_ns']-1):+.2f}%|{verdict(r['ulmdb_p99_ns'],r['lmdb_p99_ns'],False)}|")
    lines += ['', '## All measured write windows', '',
              'Includes aggregate and separately sampled business commits, with timer overhead retained. '
              'Excludes validation and dispatch gaps; it is not total wall time. The original aggregate-only headlines remain above.', '',
              '|Filesystem / batch|LMDB records/s|ULMDB records/s|ULMDB vs LMDB|Verdict|', '|---|---:|---:|---:|---|']
    lines += [f"|{r['filesystem']} / {r['batch']}|{r['lmdb']:,.0f}|{r['ulmdb']:,.0f}|{r['gap_pct']:+.2f}%|{verdict(r['ulmdb'],r['lmdb'])}|" for r in combined]
    backend_option = ' --backend resident' if data['backend'] == 'resident' else ''
    lines += ['', '## Workload and reproduction', '',
              '```sh', './benchmark/run.sh full' + backend_option,
              './benchmark/run.sh full' + backend_option + ' --rounds 4 --cpu 0', '```', '',
              'The standard fixture has 8,192 binary KV records, 1,024 prefix insertion attempts per IP family, '
              '4,000 queries per read case, 16 trials, and eight aggregate plus eight sampled commits per write trial. '
              'Batches contain 1 or 64 records. Trial backend order alternates. Each phase checks the complete KV state '
              'and independent IPv4/IPv6 answers outside measurement. Cold reopening and old snapshot/borrow checks remain enabled.', '',
              'Reads use already bound database views in one snapshot. This measures warm embedded operations, not parsing, '
              'network service latency, concurrent writers or datasets beyond RAM. IP lookup semantics are identical; '
              'LMDB uses the benchmark prefix adapter over its generic ordered index.', '',
              'SQLite is covered by the [million-operation comparison](ULMDB_VS_LMDB_VS_SQLITE.md). '
              'Production code has no LMDB or SQLite dependency. The benchmark-only pinned LMDB source and original notices '
              'live under `support/lmdb/`. Source hashes, compiler/native versions, machine, initialization, memory/file costs '
              'and exact denominators are retained in [results/full.json](results/full.json).', '',
              'Bare ULMDB open, without dropping the operating-system page cache:', '',
              '|Filesystem|Open ms|', '|---|---:|']
    lines += [f"|{r['filesystem']}|{r['open_ms']:.3f}|" for r in cold]
    lines += ['', 'Creation, full validation and optional backend re-enable costs are separate fields in the JSON. '
              'A RAM drive has filesystem-lifetime persistence. These development measurements do not qualify crash recovery, '
              'formal correctness, Internet-scale capacity or universal LMDB parity.', '',
              'Machine: `' + data['machine']['cpu_model'] + '`. Kernel: `' + data['machine']['kernel'] + '`.', '']
    (HERE / 'ULMDB_VS_LMDB.md').write_text('\n'.join(lines).rstrip() + '\n')
    return rows, combined


def million_backend(data):
    labels = sorted({p['report']['backend'] for p in data['profiles']
                     if p['report']['engine'] == 'ulmdb'})
    if len(labels) > 1:
        raise ValueError('million results mix ULMDB backends')
    return labels[0] if labels else data.get('backend', 'ULMDB backend not run')


def million_rate(row):
    if row['ops_s'] is not None:
        return f"{row['ops_s']:,.0f}"
    return {'failed': 'Failed', 'not_run': 'Not run', 'incomplete': 'Unavailable'}[row['status']]


def million_gap(ulmdb, lmdb):
    if ulmdb['ops_s'] is None or lmdb['ops_s'] is None:
        return 'Unavailable'
    return f"{100*(ulmdb['ops_s']/lmdb['ops_s']-1):+.2f}%"


def millions(data):
    c, rounds = data['config'], data['rounds']
    groups, failures, backends = {}, [], {}
    for profile in data['profiles']:
        report = profile['report']
        engine = report['engine']
        assert engine in ['ulmdb', 'lmdb', 'sqlite'] and report['config'] == c
        backends.setdefault(engine, []).append(report)
        assert len(backends[engine]) <= rounds
        status = report.get('status', 'completed')
        assert status in ['complete', 'completed', 'success', 'failed']
        failed = status == 'failed'
        assert failed or report['validation']['passed'] is True
        if failed:
            assert report['validation']['passed'] is False
            assert isinstance(report.get('failure'), dict)
            failures.append({'engine': engine, 'round': profile.get('round'),
                             'backend': report['backend'], 'failure': report['failure'],
                             'validation': report['validation'],
                             'selected_state': report.get('selected_state'),
                             'pool': report.get('pool')})
        seen = set()
        for row in report['rows']:
            op = row['operation']
            assert op in ['load', 'read', 'update'] and op not in seen and row['engine'] == engine
            seen.add(op)
            expected = c['keys' if op == 'load' else 'reads' if op == 'read' else 'updates']
            transactions = 1 if op == 'read' else (expected + c['batch'] - 1) // c['batch']
            assert row['operations'] == expected and row['elapsed_ns'] > 0
            assert row['transactions'] == transactions
            # A later failure cannot confer validation on an earlier timer.
            # The adapter explicitly admits only fully measured/oracled phases.
            if failed:
                assert op in report['validation'].get('completed_phases', [])
                assert op != report['failure']['phase']
            groups.setdefault((engine, op), []).append(row)
    rows = []
    for operation in ['load', 'read', 'update']:
        for engine in ['ulmdb', 'lmdb', 'sqlite']:
            group = groups.get((engine, operation), [])
            expected = c['keys' if operation == 'load' else 'reads' if operation == 'read' else 'updates']
            phase_failures = [f for f in failures if f['engine'] == engine and f['failure']['phase'] == operation]
            complete = len(group) == rounds
            status = ('completed' if complete else 'failed' if phase_failures
                      else 'incomplete' if group else 'not_run')
            rates = [x['operations'] * 1e9 / x['elapsed_ns'] for x in group]
            rows.append({'engine': engine, 'operation': operation, 'status': status,
                         'operations_per_run': expected,
                         'transactions_per_run': 1 if operation == 'read' else (expected + c['batch'] - 1) // c['batch'],
                         'completed_runs': len(group), 'required_runs': rounds,
                         'ops_s': gm(rates) if complete else None,
                         'observed_completed_run_rates': rates,
                         'failures': phase_failures})
    lookup = {(x['engine'], x['operation']): x for x in rows}
    operations = ['load', 'read', 'update']
    missing = {(engine.upper(), index): million_rate(lookup[engine, op])
               for engine in ['ulmdb', 'lmdb', 'sqlite'] for index, op in enumerate(operations)
               if lookup[engine, op]['ops_s'] is None}
    backend = million_backend(data)
    plot(['Load', 'Warm reads', 'Updates'],
         {engine.upper(): [None if lookup[engine, op]['ops_s'] is None else lookup[engine, op]['ops_s'] / 1e6
                          for op in operations] for engine in ['ulmdb', 'lmdb', 'sqlite']},
         f"{c['keys']:,} keys: ULMDB vs LMDB vs SQLite", HERE / 'graphs/millions.svg',
         'Million operations per second; higher is better', missing_labels=missing)
    gaps = [100 * (lookup['ulmdb', op]['ops_s'] / lookup['lmdb', op]['ops_s'] - 1)
            if lookup['ulmdb', op]['ops_s'] and lookup['lmdb', op]['ops_s'] else None
            for op in operations]
    plot(['Initial load', 'Warm reads', 'Updates'], {'ULMDB vs LMDB': gaps},
         'ULMDB throughput improvement versus LMDB', HERE / 'graphs/millions-gap.svg',
         'Percent versus local LMDB; positive = faster, negative = slower', percentage_gain=True)
    reads = [lookup[engine, 'read']['ops_s'] for engine in ['ulmdb', 'lmdb', 'sqlite']]
    summary = (f"**Warm reads: {reads[0] / reads[1]:.2f}× LMDB and {reads[0] / reads[2]:.2f}× SQLite** "
               f"({reads[0] / 1e6:.2f} million operations/s)."
               if all(reads) else 'Completed workload results are shown below.')
    lines = ['# ULMDB vs LMDB vs SQLite', '', summary, '',
             f"Recorded results use **{backend}** on {data['storage']} storage. "
             + ('This optional Resident profile differs from the default Legacy and snapshot-log backends.'
                if 'resident' in backend.lower() else ''), '',
             f"{c['keys']:,} keys, {c['reads']:,} reads and {c['updates']:,} updates per engine; "
             f"{c['batch']:,} writes per durable transaction. Rates are geometric means of {rounds} runs, "
             'in operations per second. Higher is better.', '',
             '![Millions of operations per second](graphs/millions.svg)', '',
             '| Operation | ULMDB/s | LMDB/s | SQLite/s | ULMDB vs LMDB |',
             '| --- | ---: | ---: | ---: | ---: |']
    for op in operations:
        u, l, q = [lookup[e, op] for e in ['ulmdb', 'lmdb', 'sqlite']]
        lines.append(f"| {op.capitalize()} | {million_rate(u)} | {million_rate(l)} | {million_rate(q)} | {million_gap(u,l)} |")
    lines += ['', '![Percentage improvement versus LMDB](graphs/millions-gap.svg)']
    if not failures:
        lines += ['', f'All {3 * rounds} runs passed value, miss, ordered-count and reopened-data checks.']
    if failures:
        lines += ['', '## Recorded incomplete runs', '',
                  '|Engine / round|Failed phase|Error|Acknowledged records / requested|', '|---|---|---|---:|']
        for f in failures:
            failure = f['failure']
            error = str(failure['error']).replace('|', '&#124;').replace('\n', '<br>')
            lines.append(f"|{f['engine']} / {f['round']}|{failure['phase']}|{error}|"
                         f"{failure.get('committed_records', 'unknown')} / {failure.get('requested_records', 'unknown')}|")
        lines += ['', 'No failed-phase or later unrun-phase rate is taken from the earlier Legacy engine. '
                  'The failed fixture validation and acknowledged-state observations remain in the JSON.']
        pools = [f for f in failures if f['pool'] and f['selected_state']]
        if pools:
            lines += ['', 'Resident pool observations after the failed transaction was discarded:', '',
                      '|Round|Pool pages|Held pages|Free pages|Committed rows audited|Audit passed|',
                      '|---|---:|---:|---:|---:|---|']
            for f in pools:
                p, state = f['pool'], f['selected_state']
                lines.append(f"|{f['round']}|{p['maps_created']:,}|{p['held_maps']:,}|{p['free_pages']:,}|"
                             f"{state.get('row_count', 'unknown')}|{state['validation_passed']}|")
    complete_backends = {engine: reports for engine, reports in backends.items()
                         if len(reports) == rounds and all(r['validation']['passed'] for r in reports)}
    if len(complete_backends) == 3:
        lines += ['', '## Startup and storage', '',
                  'Startup and storage costs are separate from throughput. '
                  'Geometric means below; lower is better.', '',
                  '| Engine | Setup, ms | Close/reopen, ms | Apparent MiB | Allocated MiB |',
                  '| --- | ---: | ---: | ---: | ---: |']
        costs = lifecycle_costs(data)
        if costs:
            for engine in ['ulmdb', 'lmdb', 'sqlite']:
                cost = costs[engine]
                label = 'SQLite' if engine == 'sqlite' else engine.upper()
                lines.append(f"| {label} | {cost['setup_ms']:,.3f} | {cost['reopen_ms']:,.3f} | "
                             f"{cost['apparent_mib']:,.3f} | {cost['allocated_mib']:,.3f} |")
            lines += ['', 'Close/reopen keeps OS caches. ULMDB replays and audits its WAL; SQLite includes '
                      'close/checkpoint work. Allocated space includes reserved capacity.']
            keys = ['setup_ms', 'reopen_ms', 'apparent_mib', 'allocated_mib']
            plot(['Setup', 'Close/reopen', 'Apparent file bytes', 'Allocated file bytes'],
                 {engine.upper(): [costs[engine][key] / costs['lmdb'][key] for key in keys]
                  for engine in ['ulmdb', 'lmdb', 'sqlite']},
                 'Reopen and file costs relative to LMDB',
                 HERE / 'graphs/costs.svg',
                 'Cost / local LMDB; lower is better (log scale)', True)
            lines += ['', '![Reopen and file-cost ratios](graphs/costs.svg)']
    lines += ['', '## Method', '',
              'Single process/thread, identical eight-byte keys and values. Warm reads use one bound snapshot '
              'with approximately 50% hits. Load is sorted; updates use shuffled keys. '
              'LMDB uses normal durable flags (`0`); SQLite uses prepared statements, WAL and `synchronous=FULL`.', '',
              'Write rates include transaction begin, mutations and commit; read rates include loop/checksum overhead. '
              'Validation runs outside these timers. These are aggregate throughput results, not p99 latency measurements.']
    if not failures and 'resident' in backend.lower():
        settings = backends['ulmdb'][0]['settings']
        assert all(r['settings'] == settings for r in backends['ulmdb'])
        lines += ['', 'ULMDB automatic collection is included in writes. This profile uses '
                  f"{settings['pool_kv_pages']:,} KV pages and a {settings['auto_collect_below']:,}-page "
                  'collection threshold; capacity and engine-private cache settings differ between engines.']
    lines += ['', 'Machine: `' + data['machine']['cpu_model'] + '`; `' + data['machine']['kernel'] + '`.', '',
              '## Reproduce', '',
              '```sh', './benchmark/run.sh millions' + (' --backend resident' if 'resident' in backend.lower() else '')
              + f" --keys {c['keys']} --reads {c['reads']} --updates {c['updates']} --batch {c['batch']}", '```', '',
              'Requires Rust 1.94.0, a C compiler, Python 3.11+ with pip and the SQLite shared library. '
              'Use `--help` for storage, repetition and CPU-affinity options.', '',
              '[Raw results and exact settings](results/millions.json) · '
              '[Read/write latency and CIDR comparison](ULMDB_VS_LMDB.md). '
              'These recorded workload results do not establish universal performance or production durability.', '']
    (HERE / 'ULMDB_VS_LMDB_VS_SQLITE.md').write_text('\n'.join(lines).rstrip() + '\n')
    return rows


def update_readme():
    path = HERE.parent / 'README.md'
    text = path.read_text()
    start, end = '<!-- BENCHMARK_SUMMARY_BEGIN -->', '<!-- BENCHMARK_SUMMARY_END -->'
    # A README without the generated block owns its presentation separately.
    if start not in text or end not in text:
        return
    full_path, million_path = HERE / 'results/full-summary.json', HERE / 'results/millions-summary.json'
    if not full_path.exists() or not million_path.exists():
        return
    a, b = json.loads(full_path.read_text()), json.loads(million_path.read_text())
    full_data = json.loads((HERE / 'results/full.json').read_text())
    million_data = json.loads((HERE / 'results/millions.json').read_text())
    label = ('experimental Resident, iteration 34 profile with scalable page directory' if full_data['backend'] == 'resident'
             else 'default file engine')
    if full_data.get('performance_review'):
        label = 'experimental Resident, ' + full_data['performance_review']['label']
    selected = [('ordered_kv_mixed', 'KV mixed reads'), ('ipv4_lpm_mixed', 'IPv4 mixed reads'),
                ('ipv6_lpm_mixed', 'IPv6 mixed reads'), ('durable_batch1', 'Writes, batch 1'),
                ('durable_batch64', 'Writes, batch 64')]
    lookup = {(r['filesystem'], r['case']): r for r in a['rows']}
    headline = [(fs, name, lookup[fs, case]) for fs in ['disk', 'ram'] for case, name in selected]
    plot([fs.upper() + ' / ' + name for fs, name, _ in headline],
         {'ULMDB vs LMDB': [r['gap_pct'] for _, _, r in headline]},
         chart_title('ULMDB throughput improvement versus LMDB', full_data),
         HERE / 'graphs/readme.svg', 'Percent versus local LMDB; positive = faster, negative = slower', percentage_gain=True)
    ahead = sum(r['gap_pct'] > 0 for r in a['rows'])
    lines = [*review_note(full_data),
             f"**Latest full-suite results — {label}** ({full_data['generated_at']}).", '',
             f"8,192 KV keys; 16 trials and {full_data['rounds']} fresh processes per filesystem. "
             'LMDB runs locally with normal durability flags. Positive throughput percentages mean ULMDB is faster. '
             'Roughly on par means within 2% of the observed centre, not statistical equivalence.', '',
             '|Filesystem / operation|ULMDB/s|LMDB/s|ULMDB vs LMDB|Verdict|', '|---|---:|---:|---:|---|']
    for fs, name, r in headline:
        lines.append(f"|{fs.upper()} / {name}|{r['ulmdb']:,.0f}|{r['lmdb']:,.0f}|{r['gap_pct']:+.2f}%|{verdict(r['ulmdb'],r['lmdb'])}|")
    lines += ['', '![Latest mixed-read and durable-write results](benchmark/graphs/readme.svg)', '',
              f'ULMDB exceeds local LMDB in **{ahead}/{len(a["rows"])}** measured throughput cases. '
              'The [full report](benchmark/ULMDB_VS_LMDB.md) contains all cases, sampled latency, '
              'cold-open costs and slower results.']
    if full_data.get('paired_baseline'):
        reads = [r for r in a['rows'] if not r['case'].startswith('durable_')]
        lower_means = sum(r['change_pct'] < 0 for r in reads)
        longer_tails = sum(r['p99_change_pct'] > 0 for r in reads)
        lines += ['', f'Against the saved pre-fix ULMDB executable rerun in this campaign, '
                  f'**{lower_means}/{len(reads)} read throughput means are lower** and '
                  f'**{longer_tails}/{len(reads)} sampled read p99 values are higher**. '
                  'The full report preserves every before/after value; retention status, '
                  'marginal tradeoffs and unresolved tails are documented separately.']
    m = {(r['engine'], r['operation']): r for r in b}
    backend = million_backend(million_data)
    config = million_data['config']
    lines += ['', f"**Fresh million-key comparison — {backend}** ({million_data['generated_at']}).", '',
              f"{config['keys']:,} keys, {config['reads']:,} reads and {config['updates']:,} updates requested; "
              f"batch {config['batch']:,}, {million_data['rounds']} runs per engine. "
              'This is a fresh benchmark of the current Resident engine.', '',
              '|Operation|ULMDB/s|LMDB/s|SQLite/s|ULMDB vs LMDB|Verdict|', '|---|---:|---:|---:|---:|---|']
    for op in ['load', 'read', 'update']:
        u, l = m['ulmdb', op], m['lmdb', op]
        status = verdict(u['ops_s'], l['ops_s']) if u['ops_s'] and l['ops_s'] else 'Unavailable'
        lines.append('|' + op + '|' + '|'.join(million_rate(m[e,op]) for e in ['ulmdb', 'lmdb', 'sqlite']) + f'|{million_gap(u,l)}|{status}|')
    unavailable = [r for r in b if r['ops_s'] is None]
    if unavailable:
        lines += ['', 'A rate is unavailable unless every requested run completes that phase and its validation. '
                  'Failed and later unrun phases remain visible in the report; partial acknowledgments are not throughput.']
    lines += ['', '![Load, read and update percentage improvement versus LMDB](benchmark/graphs/millions-gap.svg)', '',
              'The [million-key report](benchmark/ULMDB_VS_LMDB_VS_SQLITE.md) retains its own workload, '
              'backend, failures and measurement date. These separate comparisons do not establish universal performance parity.']
    costs = lifecycle_costs(million_data)
    if costs:
        ram_single = lookup['ram', 'durable_batch1']
        lines += ['', '**Remaining LMDB gaps:** initial loading, RAM single-record writes '
                  f"({ram_single['ulmdb']:,.0f}/s versus {ram_single['lmdb']:,.0f}/s), "
                  'reopening, file allocation and some sampled write latencies. Throughput '
                  'wins do not establish parity on these costs.', '', *cost_table(costs), '',
                  '![Reopen and file allocation relative to LMDB; lower is better](benchmark/graphs/costs.svg)']
        load = m['ulmdb', 'load']['ops_s']
        native_load = m['lmdb', 'load']['ops_s']
        lines += ['', '**Distance remaining to LMDB parity at the observed means:**', '',
                  '|Target|Current ULMDB versus LMDB|Improvement still needed|', '|---|---:|---:|']
        for label, u, l in [('Initial-load throughput', load, native_load),
                             ('RAM single-write throughput', ram_single['ulmdb'], ram_single['lmdb'])]:
            if u and l:
                lines.append(f'|{label}|{100*(u/l-1):+.2f}%|{max(0,100*(l/u-1)):.2f}% more ULMDB throughput|')
        for label, key in [('Setup time', 'setup_ms'), ('Close/reopen time', 'reopen_ms'),
                           ('Allocated file space', 'allocated_mib')]:
            u, l = costs['ulmdb'][key], costs['lmdb'][key]
            lines.append(f'|{label}|{u/l:,.2f}× LMDB cost|{max(0,100*(1-l/u)):.3f}% less ULMDB cost|')
    review = full_data.get('performance_review', {})
    if review.get('accepted_summary'):
        lines += ['', '**Last accepted progress:** ' + review['accepted_summary']]
    lines += ['', 'The [complete performance results](PERFORMANCE_RESULTS.md) retain accepted '
              'and held candidates, all sampled tails and uncertainty. The '
              '[load optimization plan](PERFORMANCE_PLAN.md#focused-initial-load-plan-updated-from-current-measurements) '
              'targets measured page zeroing/copying, with isolated ownership/corruption controls '
              'and a complete disk/RAM/read/write benchmark gate before retention.']
    before, rest = text.split(start, 1)
    _, after = rest.split(end, 1)
    path.write_text(before + start + '\n' + '\n'.join(lines) + '\n' + end + after)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('suite', choices=['full', 'millions', 'all'])
    args = parser.parse_args()
    if args.suite in ['full', 'all']:
        data = json.loads((HERE / 'results/full.json').read_text())
        rows, combined = full(data)
        (HERE / 'results/full-summary.json').write_text(json.dumps({'rows': rows, 'combined_writes': combined}, indent=2) + '\n')
    if args.suite in ['millions', 'all']:
        data = json.loads((HERE / 'results/millions.json').read_text())
        (HERE / 'results/millions-summary.json').write_text(json.dumps(millions(data), indent=2) + '\n')
    update_readme()


if __name__ == '__main__':
    main()
