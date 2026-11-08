"""Generic artifact plumbing for the independently reviewed retained-EIO V2 contract.

Derived from accepted driver a40f6944 and protocol25d56191. The unchanged C
fixture supplies all lifetime/value/commit/reopen assertions. Artifact paths
come from the enclosing runner, never from an ignored historical workdir.
"""
from pathlib import Path
import re

PHASE = 'retained.sync.force1'
PASS = b'retained-sync EIO: original writer committed once; old borrow stable; reopen B/8192 PASS\n'
ASSETS = {'segment', 'descriptor', 'cohort'}
NEGATIVES = [('legacy', 'clean', 'full'), ('cohort', 'dirty', 'nometasync'), ('indexed', 'prepared', 'nosync')]


def check(yes, message):
    if not yes:
        raise RuntimeError('retained-EIO observer: ' + message)


def prefix(oracle, row, stdout, parsed):
    check(f"retained.state={row['state']} policy={row['policy']}\n".encode() in stdout, 'state creation did not complete')
    check(PHASE in parsed['phases'] and 'seed.commit' in parsed['phases'], 'required phase absent')
    check(sum(e['syscall'] in oracle.RENAMES and e['success'] and e['role'] == 'current'
              for e in parsed['phases']['seed.commit']) == 1, 'seed A was not selected exactly once')
    force = parsed['phases'][PHASE]
    check(not any((e['write'] and e['role'] in ASSETS | oracle.SELECTORS)
                  or (e['syscall'] in oracle.RENAMES and e['role'] in oracle.SELECTORS) for e in force),
          'forced sync wrote/selected business data or metadata')
    if row['state'] == 'prepared':
        check('retained.prepare' in parsed['phases'], 'prepared phase absent')
        check(not any(e['syscall'] in oracle.RENAMES and e['role'] == 'current'
                      for e in parsed['phases']['retained.prepare']), 'preparation selected B')
    seed = {e['canonical_path'] for e in parsed['phases']['seed.commit'] if e['write'] and e['role'] in ASSETS}
    prepared = {e['canonical_path'] for e in parsed['phases'].get('retained.prepare', []) if e['write'] and e['role'] in ASSETS}
    if row['state'] == 'prepared':
        check(bool(prepared), 'prepared B assets not observed')
    # Initializer generation0 is the actual authenticated predecessor of seedA,
    # not a random descriptor chosen by filename order. The force operation
    # legitimately fences both selected A and its retained predecessor0.
    committed = seed | {item['path'] for item in row['initializer_committed_assets']}
    check(not prepared & committed, 'B preparation reused committed assets')
    fences = {e['canonical_path'] for e in force if e['fence'] and e['canonical_role'] in ASSETS}
    check(not fences & prepared, 'forced sync fenced unpublished B')
    check(fences <= committed, 'forced sync fenced outside initializer0 and selected A')
    return committed, prepared


def calibration(oracle, row, stdout, stderr, parsed, require_fsync):
    committed, prepared = prefix(oracle, row, stdout, parsed)
    check(row['returncode'] == 1 and b'retained.sync.force1=0\n' in stdout and PASS not in stdout,
          'calibration did not stop at expected-EIO assertion')
    check(re.findall(rb'^retained-sync oracle: (.*)$', stderr, re.M) == [b'rc == EIO'], 'unrelated calibration rejection')
    check('(INJECTED)' not in Path(row['path'], 'syscalls.trace').read_text(), 'calibration already injected')
    check(not any(name in parsed['phases'] for name in ['retained.commit', 'retained.after.sync']), 'calibration continued past assertion')
    targets = [e for e in parsed['phases'][PHASE] if e['syscall'] == 'fsync' and e['fence']
               and e['canonical_role'] in ASSETS and e['canonical_path'] in committed]
    if require_fsync:
        check(bool(targets), 'no selected committed asset force-fsync')
        target = targets[0]
        ordinal = sum(e['syscall'] == 'fsync' and e['pid'] == target['pid'] and e['index'] <= target['index']
                      for e in parsed['events'])
        return {'ordinal': ordinal, 'target': target, 'selected_committed_assets': sorted(committed),
                'unselected_prepared_assets': sorted(prepared)}
    check(not any(e['syscall'] in {'fsync', 'fdatasync', 'msync'} for e in parsed['phases'][PHASE]), 'no-op control performed a fence')
    pids = {e['pid'] for e in parsed['events']}
    check(len(pids) == 1, 'no-op calibration needs explicit multi-thread observer extension')
    pid = next(iter(pids))
    ordinal = sum(e['syscall'] == 'fsync' and e['pid'] == pid for e in parsed['events']) + 1
    return {'ordinal': ordinal, 'reason': 'next per-process fsync after successful prefix; force phase has none and the exact assertion stops all later business work'}


def injected(oracle, row, stdout, stderr, parsed, target):
    committed, prepared = prefix(oracle, row, stdout, parsed)
    check(row['returncode'] == 0 and stdout.endswith(PASS) and b'retained.sync.force1=5\n' in stdout,
          'original writer/value/reopen assertions did not complete')
    check(not re.findall(rb'^retained-sync oracle:', stderr, re.M), 'C fixture reported an oracle failure')
    failures = [e for e in parsed['events'] if '(INJECTED)' in e['result']]
    check(len(failures) == 1, 'wrong number of injected events')
    failure = failures[0]
    check(failure['syscall'] == 'fsync' and failure['phase'] == PHASE
          and failure['canonical_path'] in committed and failure['canonical_path'] not in prepared
          and failure['canonical_role'] == target['target']['canonical_role']
          and not failure['success'] and failure['result'].startswith('-1 EIO '), 'injection hit an unrelated object/phase')
    ordinal = sum(e['syscall'] == 'fsync' and e['pid'] == failure['pid'] and e['index'] <= failure['index'] for e in parsed['events'])
    check(ordinal == target['ordinal'], 'injected per-process ordinal drifted')
    check(sum(e['syscall'] in oracle.RENAMES and e['success'] and e['role'] == 'current'
              for e in parsed['phases']['retained.commit']) == 1, 'original retained writer did not select B exactly once')
    check(sum(e['syscall'] in oracle.RENAMES and e['success'] and e['role'] == 'current'
              for e in parsed['events']) == 2, 'unexpected extra publication beyond A and B')
    check(any(e['fence'] and e['canonical_role'] in ASSETS for e in parsed['phases']['retained.after.sync']), 'post-commit selected sync retry did not fence B')
    return {'passed': True, 'exact_eio_event': failure, 'selected_committed_assets': sorted(committed),
            'unselected_prepared_assets': sorted(prepared), 'C_postconditions': 'original writer usable/committed once; old borrow+cursor; B/8192 and reopen verified'}
