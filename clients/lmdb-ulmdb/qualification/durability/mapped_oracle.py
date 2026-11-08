"""Linux physical WRITEMAP observations, separate from buffered qualification.

Requires strace -f -v -yy and fstat/newfstatat/statx in addition to the existing
durability trace set. Address ranges are bound to held-FD device/inode evidence;
MS_ASYNC is scheduling, never a strong fence. These observations do not prove
power-loss recovery or that arbitrary CPU stores reached persistent media.
"""
from pathlib import Path
import ast
import copy
import collections
import hashlib
import json
import os
import re
import types

_path = Path(__file__).with_name("oracle.py")
buffered = types.ModuleType("mapped_buffered_source")
exec(compile(_path.read_bytes(), str(_path), "exec"), buffered.__dict__)
ValidationError = buffered.ValidationError
UnsupportedScope = buffered.UnsupportedScope
ASSETS = buffered.ASSETS
STAGING = "mapped_staging"


def declared_cases():
    path = Path(__file__).with_name("cases.json")
    _require(hashlib.sha256(path.read_bytes()).hexdigest() ==
             "0768ace941caaa3cd05714415b7f7a3564bcfaec85740f46e194e8717b0b1d3f",
             "mapped case declaration changed; requires explicit source review")
    cases = [c for c in json.loads(path.read_text())["cases"] if c["arguments"][1] == "1"]
    # The frozen corpus repeats four modern baseline rows. Preserve those
    # executions too: 314 declared attempts, 310 distinct argument tuples.
    keys = [(c["version"], tuple(c["arguments"])) for c in cases]
    _require(len(keys) == 314 and len(set(keys)) == 310, "mapped declaration must retain every frozen attempt")
    _require({v: sum(c["version"] == v for c in cases) for v in ("0.9.31", "0.9.36", "1.0.2")} ==
             {"0.9.31": 72, "0.9.36": 72, "1.0.2": 170}, "mapped per-profile scope changed")
    return keys


def _require(yes, message):
    if not yes:
        raise ValidationError(message)


def _fields(args):
    result, start, depth, quoted, escaped = [], 0, 0, False, False
    for i, char in enumerate(args):
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in "([{<":
            depth += 1
        elif char in ")]}>" :
            depth -= 1
        elif char == "," and depth == 0:
            result.append(args[start:i].strip())
            start = i + 1
    result.append(args[start:].strip())
    return result


def _integer(value):
    return int(value, 16 if value.startswith("0x") else 10)


def _identity(args):
    ino = re.search(r"\bstx?_ino=(0x[0-9a-f]+|[0-9]+)\b", args)
    if ino is None or not re.search(r"\bstx?_mode=S_IFREG\b", args):
        return None
    dev = re.search(r"\bst_dev=makedev\(([^,]+), ([^)]+)\)", args)
    if dev:
        major, minor = map(_integer, dev.groups())
    else:
        major = re.search(r"\bstx_dev_major=([0-9]+)", args)
        minor = re.search(r"\bstx_dev_minor=([0-9]+)", args)
        if not major or not minor:
            return None
        major, minor = int(major[1]), int(minor[1])
    size = re.search(r"\bstx?_size=([0-9]+)", args)
    return {"device": [major, minor], "inode": _integer(ino[1]),
            "size": int(size[1]) if size else None}


def _key(identity):
    return (*identity["device"], identity["inode"]) if identity else None


def _terminal_error_base(trace_path, database_path, expected_phase):
    """An error-only ledger; never supplies balanced successful-case evidence.

    Public probe's ok(commit) exits inside its marker on data EIO. No invented
    END/write event is appended. Caller must also require its exact exit/error
    transcript and unchanged CURRENT bytes.
    """
    events, phases, renames, phase = [], {}, [], None
    root = os.path.normpath(os.path.abspath(database_path))
    for number, line in enumerate(Path(trace_path).read_text().splitlines(), 1):
        _require("<unfinished ...>" not in line and " resumed>" not in line, "split syscall in terminal error trace")
        match = buffered.CALL.match(line)
        if not match:
            continue
        call, args, result = match["call"], match["args"], match["result"]
        first, returned = buffered.FD.match(args), buffered.FD.match(result)
        marker = re.search(r"@@(BEGIN|END) ([^@]+)@@", args) if call in buffered.WRITES and first and first["fd"] == "2" else None
        if marker:
            action, name = marker.groups()
            if action == "BEGIN":
                _require(phase is None and name not in phases, "nested/duplicate terminal error marker")
                phase, phases[name] = name, []
            else:
                _require(phase == name, "unbalanced terminal error marker")
                phase = None
            continue
        result_number = re.match(r"(-?\d+)", result)
        success = bool(result_number and int(result_number[1]) >= 0)
        paths, path = [], returned["path"] if call == "openat" and returned else first["path"] if first else None
        if call in buffered.RENAMES:
            paths = [ast.literal_eval(q) for q in buffered.QUOTED.findall(args)]
            _require(len(paths) == 2, "unresolved terminal error rename")
            path = paths[-1]
            if success:
                renames.append(tuple(paths))
        event = {"index": len(events), "line": number, "pid": int(match["pid"] or 0),
                 "timestamp": match["time"], "phase": phase, "syscall": call,
                 "path": path.removesuffix(" (deleted)") if path else None, "paths": paths,
                 "role": buffered._role(path, root), "success": success, "result": result,
                 "write": call in buffered.WRITES and success,
                 "fence": call in {"fsync", "fdatasync", "msync"} and success}
        events.append(event)
        if phase is not None:
            phases[phase].append(event)
    _require(phase == expected_phase, "terminal error trace must end inside its exact expected phase")
    _require(sum("(INJECTED)" in e["result"] and e["result"].startswith("-1 EIO ") for e in phases[phase]) == 1,
             "terminal phase lacks exactly one real injected EIO")
    return {"schema": 1, "database": root, "events": events, "phases": phases, "renames": renames,
            "terminal_error_phase": phase, "trace_sha256": hashlib.sha256(Path(trace_path).read_bytes()).hexdigest()}


def parse(trace_path, database_path, *, terminal_error_phase=None):
    parsed = buffered.parse(trace_path, database_path) if terminal_error_phase is None else _terminal_error_base(trace_path, database_path, terminal_error_phase)
    root = parsed["database"]
    by_line = {e["line"]: e for e in parsed["events"]}
    descriptors, active, mappings, publications, names = {}, {}, [], [], {}
    page_size = os.sysconf('SC_PAGE_SIZE')
    pids = set()
    def canonical(path):
        for before, after in parsed["renames"]:
            if path == before or path.startswith(before + "/"):
                path = after + path[len(before):]
        return path
    def role(path):
        if path and path.startswith(root + "/transactions/") and path.endswith("-writemap.tmp"):
            return STAGING
        return buffered._role(path, root)
    for number, line in enumerate(Path(trace_path).read_text().splitlines(), 1):
        event = by_line.get(number)
        if event is None:
            continue
        match = buffered.CALL.match(line)
        args, call, pid = match["args"], event["syscall"], event["pid"]
        event["args"] = args
        fields = _fields(args)
        first = buffered.FD.match(args)
        fdkey = (pid, int(first["fd"])) if first else None
        returned = buffered.FD.match(event["result"])
        if call == "openat" and event["success"] and returned:
            key = (pid, int(returned["fd"]))
            descriptors[key] = {"path": returned["path"], "flags": args, "identity": None}
        if call in {"fstat", "newfstatat", "statx"} and event["success"]:
            identity = _identity(args)
            if identity:
                if fdkey in descriptors and (call == "fstat" or '"",' in args):
                    descriptors[fdkey]["identity"] = identity
                    names.setdefault(descriptors[fdkey]["path"], set()).add(_key(identity))
                elif event["path"]:
                    names.setdefault(event["path"], set()).add(_key(identity))
        descriptor = descriptors.get(fdkey)
        if call == "ftruncate" and event["success"] and descriptor and descriptor["identity"]:
            descriptor["identity"] = dict(descriptor["identity"], size=_integer(fields[1]))
        if call == "mmap" and event["success"]:
            _require(len(fields) == 6, "unresolved mmap fields")
            address = _integer(event["result"].split()[0])
            length, offset = _integer(fields[1]), _integer(fields[5])
            span = (length + page_size - 1) // page_size * page_size
            _require(not any(m["path"].startswith(root + "/") and m["address"] < address + span and
                             address < m["address"] + m["vma_length"] for m in active.get(pid, [])),
                     "successful mmap replaced live database VMA; explicit replacement observer required")
            source = buffered.FD.match(fields[4])
            if source:
                owner = descriptors.get((pid, int(source["fd"])))
                path = source["path"].removesuffix(" (deleted)")
                identity = copy.deepcopy(owner["identity"]) if owner else None
                if role(path) == STAGING:
                    _require(identity is not None, "mapped writer lacks full held-FD inode identity; use strace -v + statx/fstat")
                    _require("O_RDWR" in owner["flags"], "mapped writer FD is not writable")
                record = {"id": len(mappings), "pid": pid, "address": address, "length": length, "vma_length": span,
                          "offset": offset, "path": path, "canonical_path": canonical(path),
                          "identity": identity, "shared": bool(re.search(r"\bMAP_SHARED\b", fields[3])),
                          "writable": "PROT_WRITE" in fields[2], "phase": event["phase"], "index": event["index"]}
                mappings.append(record)
                active.setdefault(pid, []).append(record)
                event["mapping"] = record
                if role(path) == STAGING:
                    pids.add(pid)
        if call == "msync":
            address, length = _integer(fields[0]), _integer(fields[1])
            owners = [m for m in active.get(pid, [])
                      if m["address"] <= address and address + length <= m["address"] + m["vma_length"]]
            _require(len(owners) == 1, "msync range lacks one live file mapping")
            owner = owners[0]
            _require(owner["identity"] is not None, "msync lacks authenticated FD identity")
            event.update(mapping=owner, path=owner["path"], canonical_path=owner["canonical_path"],
                         mapped_offset=owner["offset"] + address - owner["address"], mapped_length=length,
                         async_schedule=event["success"] and fields[2] == "MS_ASYNC" and owner["shared"])
            _require(fields[2] in {"MS_ASYNC", "MS_SYNC"}, "unsupported msync flags")
            event["fence"] = event["success"] and fields[2] == "MS_SYNC" and owner["shared"]
        if descriptor and call in {"fsync", "fdatasync", "ftruncate"} | buffered.WRITES:
            event["file_identity"] = copy.deepcopy(descriptor["identity"])
        if call == "munmap" and event["success"]:
            address, length = _integer(fields[0]), _integer(fields[1])
            length = (length + page_size - 1) // page_size * page_size
            remaining = []
            for m in active.get(pid, []):
                lo, hi = max(address, m["address"]), min(address + length, m["address"] + m["vma_length"])
                if lo >= hi:
                    remaining.append(m)
                else:
                    if m["address"] < lo:
                        remaining.append(dict(m, length=min(m["length"], lo - m["address"]), vma_length=lo - m["address"]))
                    if hi < m["address"] + m["vma_length"]:
                        remaining.append(dict(m, address=hi, length=max(0, m["address"] + m["length"] - hi),
                                              vma_length=m["address"] + m["vma_length"] - hi,
                                              offset=m["offset"] + hi - m["address"]))
            active[pid] = remaining
        if call == "mremap" and event["success"]:
            before, old_length, new_length = map(_integer, fields[:3])
            after = _integer(event["result"].split()[0])
            _require(not any(m["path"].startswith(root + "/") and
                             ((m["address"] < before + old_length and before < m["address"] + m["vma_length"]) or
                              (m["address"] < after + new_length and after < m["address"] + m["vma_length"]))
                             for m in active.get(pid, [])), "mremap touches live database VMA; explicit observer required")
        if call in buffered.RENAMES and event["success"]:
            before, after = event["paths"]
            if role(before) == STAGING and buffered._role(after, root) in ASSETS:
                issued = [m for m in mappings if m["pid"] == pid and m["path"] == before]
                _require(bool(issued), "published WRITEMAP inode lacks an observed file mapping")
                identities = {_key(m["identity"]) for m in issued}
                _require(len(identities) == 1 and None not in identities, "mapped publish changed inode authority")
                _require(any(m["shared"] and m["writable"] for m in issued), "candidate never had a shared writable file mapping")
                held = [d for (p, _), d in descriptors.items() if p == pid and d["path"] == before]
                _require(any(_key(d["identity"]) in identities for d in held), "mapped publish lost held inode identity")
                sizes = [d["identity"]["size"] for d in held if d["identity"] and _key(d["identity"]) in identities]
                _require(bool(sizes) and all(n is not None and n > 0 for n in sizes), "published mapped extent is unobserved/empty")
                extent = max(sizes)
                _require(any(m["shared"] and m["writable"] and m["offset"] == 0 and m["length"] >= extent for m in issued),
                         "finalized candidate extent lacks actual shared writable mapping coverage")
                publication = {"path": after, "identity": issued[0]["identity"], "extent": extent,
                               "index": event["index"], "phase": event["phase"], "mappings": [m["id"] for m in issued]}
                publications.append(publication)
                # This is explicitly a mapped-publication observation, not an
                # invented write(2). It lets common preparation/selector checks
                # consume the observed same-inode published candidate.
                event.update(write=True, mapped_publication=publication)
        if call == "close" and event["success"]:
            descriptors.pop(fdkey, None)
        if event["path"]:
            event["canonical_path"] = canonical(event["path"])
            event["canonical_role"] = buffered._role(event["canonical_path"], root)
            event["role"] = event["canonical_role"] if role(event["path"]) == STAGING and event["canonical_role"] in ASSETS else role(event["path"])
    _require(bool(pids), "no actual writer backing file mapping was observed")
    _require(len(pids) == 1, "mapped public probe requires one known address space; fork/clone inheritance needs explicit observer support")
    for publication in publications:
        _require(_key(publication["identity"]) in names.get(publication["path"], set()), "published asset was not independently reopened as the same mapped inode")
    # Buffered parse reports any msync as a fence. Replace that presentation
    # too, not just validation: async scheduling must never appear as strong.
    parsed["phase_summary"] = {name: {
        "successful_fences_by_role": dict(collections.Counter(e["role"] for e in rows if e["fence"])),
        "async_schedules_by_role": dict(collections.Counter(e["role"] for e in rows if e.get("async_schedule"))),
        "mapped_publications": [e["mapped_publication"] for e in rows if "mapped_publication" in e],
        "current_selections": sum(e["syscall"] in buffered.RENAMES and e["success"] and e["role"] == "current" for e in rows),
    } for name, rows in parsed["phases"].items()}
    parsed.update(mapped_schema=1, mappings=mappings, mapped_publications=publications,
                  mapped_scope="real FD identities and address/range observations; no power-loss claim")
    return parsed


def _policy(value):
    parts = buffered._parts(value)
    if "mapasync" in parts:
        parts = parts - {"mapasync"} | {"nosync"}
    return "+".join(sorted(parts)) if parts else "full"


def validate(case_args, parsed, version):
    _require(len(case_args) == 6 and case_args[1] == "1", "mapped oracle requires the frozen WRITEMAP case shape")
    _require(parsed.get("mapped_schema") == 1, "mapped observer evidence required")
    _require("terminal_error_phase" not in parsed, "partial error evidence cannot qualify a successful case")
    policy, _, scenario, local, changed, finish = case_args
    if scenario == "fault-sync":
        raise UnsupportedScope("mapped EIO uses explicit failed-fence/error-return controls")
    _require((version, tuple(case_args)) in declared_cases(), "case is outside the exact 314-case declaration")
    initial = buffered._parts(policy)
    current = buffered._parts(changed) if scenario.startswith("runtime-") or scenario.startswith("rollback-runtime-") else initial
    projected = list(case_args)
    projected[0], projected[1], projected[4] = _policy(policy), "0", _policy(changed)
    common = buffered.validate(projected, parsed, version)
    checks = []
    def check(yes, message):
        _require(yes, message)
        checks.append(message)
    check(bool(parsed["mapped_publications"]), "real mapped seed candidate is published")
    for publication in parsed["mapped_publications"]:
        phase = publication["phase"]
        data_env = initial if phase in {"seed.commit", "B.prepare"} or scenario == "runtime-after-commit" else current
        async_wanted = "mapasync" in data_env and "nosync" not in data_env
        items = parsed["phases"][phase]
        schedules = [e for e in items if e.get("async_schedule") and e["canonical_path"] == publication["path"]]
        check(bool(schedules) == async_wanted, phase + " actual mapped async scheduling matches data-phase ENV policy")
        for event in schedules:
            check(event["mapped_offset"] == 0 and event["mapped_length"] >= publication["extent"],
                  phase + " MS_ASYNC covers the finalized mapped extent")
        weak = "nosync" in data_env or "mapasync" in data_env or "nosync" in buffered._parts(local)
        if not weak:
            change_indices = [e["index"] for e in items if e["success"] and
                       e["syscall"] in buffered.WRITES | {"ftruncate"} and
                       _key(e.get("file_identity")) == _key(publication["identity"])]
            last_change = max(change_indices, default=-1)
            strong = [e for e in items if e["fence"] and e["canonical_path"] == publication["path"]
                      and _key(e.get("file_identity") or e.get("mapping", {}).get("identity")) == _key(publication["identity"])
                      and e["index"] > last_change
                      and (e["syscall"] != "msync" or
                           (e["mapping"]["shared"] and e["mapped_offset"] == 0 and e["mapped_length"] >= publication["extent"]))]
            check(bool(strong), phase + " same-inode fsync/MS_SYNC provides mapped data completion")
            selected = [e["index"] for e in items if e["syscall"] in buffered.RENAMES and e["role"] == "current" and e["success"]]
            if selected:
                check(min(e["index"] for e in strong) < min(selected), phase + " strong data completion precedes CURRENT selection")
    for name, items in parsed["phases"].items():
        check(not any(e["syscall"] == "msync" and not e["success"] for e in items), name + " no hidden mapped flush errors")
        if not name.startswith("sync.") or not name.endswith("force0") or name.startswith("sync.readonly"):
            continue
        if "nosync" in current or "mapasync" not in current:
            continue
        partner = parsed["phases"].get(name[:-1] + "1", [])
        expected = {e["canonical_path"] for e in partner if e["fence"] and e["role"] in ASSETS | {"current", "previous"}}
        actual = {e["canonical_path"] for e in items if e.get("async_schedule")}
        check(bool(expected) and expected <= actual, name + " schedules every genuinely selected asset and selector")
        check(not any(e["fence"] for e in items), name + " MS_ASYNC does not become a strong file or namespace fence")
        check(not any(e.get("async_schedule") and e["role"] == STAGING for e in items), name + " excludes unpublished writer backing")
        check(all(e["mapped_offset"] == 0 and e["mapping"]["identity"]["size"] is not None and
                  e["mapped_length"] >= e["mapping"]["identity"]["size"] for e in items if e.get("async_schedule")),
              name + " schedules complete actual immutable mapped extents")
    return {"passed": True, "scope": "Linux mapped C file policies; no power-loss/performance claim",
            "arguments": list(case_args), "version": version, "mapped_checks": checks,
            "common_phase_checks": common["validated_expectations"],
            "mapped_publications": parsed["mapped_publications"]}


def validate_eio(parsed, phase, syscall):
    """Pair this observer result with the unchanged app's exact EIO transcript.

    It does not turn a failed syscall into a successful-case durability report.
    Driver must separately check return5, no target mutation and writer progress.
    """
    _require(parsed.get("mapped_schema") == 1, "mapped observer evidence required")
    items = parsed["phases"].get(phase, [])
    failures = [e for e in items if e["syscall"] == syscall and not e["success"]]
    _require(len(failures) == 1, "expected exactly one failed mapped I/O operation")
    failed = failures[0]
    _require(failed["result"].startswith("-1 EIO ") and "(INJECTED)" in failed["result"], "EIO injection did not hit requested operation")
    if syscall == "msync":
        mapping = failed.get("mapping", {})
        _require(mapping.get("shared") and mapping.get("writable") and mapping.get("identity"), "EIO did not hit the actual shared writable mapped inode")
        _require(not any(e["success"] and e["syscall"] in buffered.RENAMES and e["role"] == "current" for e in items), "failed data preparation selected CURRENT")
    else:
        _require(syscall in {"fsync", "fdatasync"} and failed["role"] in ASSETS | {"current", "previous"}, "force-sync EIO did not hit a selected business file")
        _require(failed.get("file_identity") is not None, "failed force-sync lacks held inode identity")
    return {"passed": True, "scope": "injected real Linux I/O error observation; no power-loss claim", "failed_event": failed}
