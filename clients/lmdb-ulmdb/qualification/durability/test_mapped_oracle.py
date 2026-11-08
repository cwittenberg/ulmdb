#!/usr/bin/env python3
"""Focused observer controls over a retained REAL Full mapped public trace.

Requires an explicit trace/database/output. No invented syscall log is accepted
as product evidence; mutations below test observer rejection only.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import types

path = Path(__file__).with_name("mapped_oracle.py")
oracle = types.ModuleType("mapped_oracle_checked_source")
oracle.__file__ = str(path)
exec(compile(path.read_bytes(), str(path), "exec"), oracle.__dict__)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--trace", type=Path, required=True)
    ap.add_argument("--database", type=Path, required=True)
    ap.add_argument("--cohort-trace", type=Path, required=True)
    ap.add_argument("--cohort-database", type=Path, required=True)
    ap.add_argument("--output", type=Path, required=True)
    args = ap.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    raw = args.trace.read_text()
    case = ["full", "1", "commit", "full", "full", "commit"]
    receipt = {"complete": False, "passed": False, "controls": [],
               "source_trace_sha256": hashlib.sha256(args.trace.read_bytes()).hexdigest(),
               "scope": "observer/source controls; not deficient-provider or full314 qualification"}
    def check(name, text, reject=True, database=None, reason=None):
        file = args.output / (name + ".trace")
        file.write_text(text)
        try:
            parsed = oracle.parse(file, database or args.database)
            report = oracle.validate(case, parsed, "1.0.2")
        except oracle.ValidationError as error:
            if not reject:
                raise
            if reason is not None and reason not in str(error):
                raise AssertionError(name + " reached unrelated rejection: " + str(error)) from error
            receipt["controls"].append({"name": name, "rejected": True, "reason": str(error)})
        else:
            if reject:
                raise AssertionError("observer silently accepted " + name)
            receipt["controls"].append({"name": name, "passed": report["passed"]})
    try:
        check("actual-positive", raw, reject=False)
        check("shared-to-private", raw.replace("MAP_SHARED", "MAP_PRIVATE"))
        check("no-writable-authority", "\n".join(line.replace("PROT_READ|PROT_WRITE", "PROT_READ")
            if "writemap.tmp>" in line and "mmap(" in line else line for line in raw.splitlines()) + "\n")
        check("missing-inode-observer", re.sub(r"\bstx_ino=[0-9]+", "stx_ino_unobserved=0", raw))
        check("unmapped-flush-address", re.sub(r"msync\(0x[0-9a-f]+", "msync(0x1", raw, count=1))
        check("different-published-inode", "\n".join(re.sub(r"\bstx_ino=[0-9]+", "stx_ino=999999999999", line)
            if "statx(" in line and "/segments/" in line else line for line in raw.splitlines()) + "\n")
        # fsync on the same real mapped inode suffices; MS_SYNC is not mandatory.
        check("same-inode-fsync-suffices", "\n".join(line for line in raw.splitlines() if "msync(" not in line) + "\n", reject=False)
        def preparation_only(text, change):
            output, active = [], False
            for line in text.splitlines():
                if "@@BEGIN seed.commit@@" in line:
                    active = True
                if "@@END seed.commit@@" in line:
                    active = False
                replacement = change(line) if active else line
                if replacement is not None:
                    output.append(replacement)
            return "\n".join(output) + "\n"
        def no_candidate_fsync(line):
            return None if " fsync(" in line and "writemap.tmp>" in line else line
        async_only = preparation_only(raw, lambda line: None if no_candidate_fsync(line) is None else line.replace("MS_SYNC", "MS_ASYNC"))
        check("async-is-not-a-strong-fence", async_only)
        cohort = args.cohort_trace.read_text()
        check("actual-cohort-positive", cohort, reject=False, database=args.cohort_database)
        shared_sync = preparation_only(cohort, no_candidate_fsync)
        check("cohort-shared-full-sync-suffices", shared_sync, reject=False, database=args.cohort_database)
        short_sync = preparation_only(shared_sync, lambda line:
            re.sub(r"(msync\(0x[0-9a-f]+, )[0-9]+", r"\g<1>1", line))
        check("partial-sync-is-not-full-data-completion", short_sync, database=args.cohort_database,
              reason="same-inode fsync/MS_SYNC provides mapped data completion")
        observed = oracle.parse(args.cohort_trace, args.cohort_database)
        final_owner = next(e["mapping"] for e in observed["phases"]["seed.commit"] if e["syscall"] == "msync")
        final_line = next(e["line"] for e in observed["events"] if e["syscall"] == "mmap" and
                          e.get("mapping", {}).get("id") == final_owner["id"])
        assert sum(m["phase"] == "seed.commit" and m["writable"] and "writemap.tmp" in m["path"] for m in observed["mappings"]) >= 2
        line_number = 0
        def private_final_map(line):
            nonlocal line_number
            line_number += 1
            if line_number == final_line:
                return line.replace("MAP_SHARED", "MAP_PRIVATE")
            return line
        mixed = []
        for line in cohort.splitlines():
            replacement = private_final_map(line)
            if replacement is not None:
                mixed.append(replacement)
        check("mixed-shared-initial-private-final", preparation_only("\n".join(mixed), no_candidate_fsync),
              database=args.cohort_database, reason="every newly written immutable asset receives a real fence")
        # Replay an observed descriptor write via positioned I/O after the
        # final shared flush. The extent/content remains unchanged; completion
        # must still follow that real mutation. Later selected sync stays valid.
        lines = cohort.splitlines()
        begin = next(i for i, line in enumerate(lines) if "@@BEGIN seed.commit@@" in line)
        end = next(i for i, line in enumerate(lines) if "@@END seed.commit@@" in line)
        flush = next(i for i in range(begin, end) if " msync(" in lines[i])
        write = next(i for i in range(begin, flush) if " write(" in lines[i] and "writemap.tmp>" in lines[i])
        observed_write = oracle.buffered.CALL.match(lines[write])
        observed_flush = oracle.buffered.CALL.match(lines[flush])
        fields = oracle._fields(observed_write['args'])
        seek = next(oracle.buffered.CALL.match(lines[i]) for i in reversed(range(begin, write))
                    if " lseek(" in lines[i] and "writemap.tmp>" in lines[i])
        offset = oracle._fields(seek['args'])[1]
        late = f"{observed_write['pid']} {observed_flush['time']} pwrite64({', '.join(fields)}, {offset}) = {observed_write['result']}"
        moved = lines[:flush + 1] + [late] + lines[flush + 1:]
        reordered = preparation_only("\n".join(moved), no_candidate_fsync)
        check("completion-before-final-real-writes", reordered, database=args.cohort_database,
              reason="same-inode fsync/MS_SYNC provides mapped data completion")
        original = oracle.parse(args.trace, args.database)
        flush = next(e for e in original['events'] if e['syscall'] == 'msync')
        owner, prefix = flush['mapping'], f"{flush['pid']} {flush['timestamp']} "
        before_flush = flush['line'] - 1
        raw_lines = raw.splitlines()
        for name, event, reason in (
            ('anonymous-fixed-replacement', prefix + f"mmap(0x{owner['address']:x}, {owner['length']}, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED, -1, 0) = 0x{owner['address']:x}", 'replaced live database VMA'),
            ('unobserved-remap-authority', prefix + f"mremap(0x{owner['address']:x}, {owner['length']}, {owner['length']}, MREMAP_MAYMOVE) = 0x{owner['address']:x}", 'mremap touches live database VMA'),
            ('partial-unmap-removes-whole-page', prefix + f"munmap(0x{owner['address']:x}, 1) = 0", 'msync range lacks one live file mapping')):
            check(name, '\n'.join(raw_lines[:before_flush] + [event] + raw_lines[before_flush:]) + '\n', reason=reason)
        keys = oracle.declared_cases()
        assert len(keys) == 314
        receipt["declared_profile_counts"] = {v: sum(key[0] == v for key in keys) for v in ("0.9.31", "0.9.36", "1.0.2")}
        receipt.update(complete=True, passed=True)
    except BaseException as error:
        receipt["error"] = repr(error)
        raise
    finally:
        receipt["artifacts"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in args.output.glob("*.trace")}
        (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
