"""Source-derived buffered C durability oracle over actual Linux strace events.

The unchanged application supplies phase markers. File-format-specific fences
are checked independently of its successful return. This is neither a physical
WRITEMAP oracle nor a power-loss/performance test.
"""
from pathlib import Path
import ast
import collections
import hashlib
import os
import re


class ValidationError(AssertionError):
    pass


class UnsupportedScope(ValidationError):
    pass


ASSETS = {"segment", "descriptor", "cohort"}
SELECTORS = {"current", "previous", "current_temp", "previous_temp"}
WRITES = {"write", "writev", "pwrite64", "pwritev"}
RENAMES = {"rename", "renameat", "renameat2"}
QUOTED = re.compile(r'"(?:\\.|[^"\\])*"')
CALL = re.compile(r"^(?:(?:\[pid\s+)?(?P<pid>\d+)\]?\s+)?(?P<time>\d+\.\d+)\s+(?P<call>\w+)\((?P<args>.*)\)\s+=\s+(?P<result>.*)$")
FD = re.compile(r"(?P<fd>-?\d+)<(?P<path>[^>]+)>")


def _role(path, root):
    if not path:
        return "unresolved"
    if path == root:
        return "root_directory"
    if path.startswith(root + ".ulmdb-init-"):
        return "bootstrap"
    if not path.startswith(root + "/"):
        return "external"
    relative = path[len(root) + 1:]
    name = relative.rsplit("/", 1)[-1]
    if relative in {"segments", "commits", "cohorts"}:
        return "asset_directory"
    if relative.startswith("segments/") and (name.endswith(".seg") or name.endswith("-segment.tmp")):
        return "segment"
    if relative.startswith("commits/") and name.endswith(".root"):
        return "descriptor"
    if relative.startswith("cohorts/") and name.endswith(".cohort"):
        return "cohort"
    if name.endswith("-CURRENT.tmp"):
        return "current_temp"
    if name.endswith("-PREVIOUS.tmp"):
        return "previous_temp"
    if relative in {"CURRENT", "PREVIOUS"}:
        return relative.lower()
    if relative == "LMDB-MAPSIZE" or name.startswith(".lmdb-mapsize-"):
        return "capacity_hint"
    if relative.startswith((".ulmdb-coordination/", "lmdb-readers/")) or relative.startswith("LMDB-") or name.startswith(".lmdb-readers-"):
        return "coordination"
    if relative == "LOCK":
        return "native_lock"
    return "other_database"


def parse(trace_path, database_path):
    trace_path = Path(trace_path)
    root = os.path.normpath(os.path.abspath(database_path))
    text = trace_path.read_text()
    events, phases, renames = [], {}, []
    phase, directory_cause = None, None
    descriptor_flags = {}
    for line_number, line in enumerate(text.splitlines(), 1):
        if "<unfinished ...>" in line or " resumed>" in line:
            raise ValidationError(f"unfinished/resumed syscall requires an explicit observer extension, line {line_number}")
        match = CALL.match(line)
        if not match:
            continue
        call, args, result = match["call"], match["args"], match["result"]
        pid = int(match["pid"] or 0)
        first = FD.match(args)
        fd = int(first["fd"]) if first else None
        marker = re.search(r"@@(BEGIN|END) ([^@]+)@@", args) if call in WRITES and fd == 2 else None
        if marker:
            action, name = marker.groups()
            if action == "BEGIN":
                if phase is not None or name in phases:
                    raise ValidationError(f"nested/duplicate marker {name}")
                phase, directory_cause = name, None
                phases[name] = []
            elif phase != name:
                raise ValidationError(f"unbalanced marker {name}, active {phase}")
            else:
                phase, directory_cause = None, None
            continue
        number = re.match(r"(-?\d+)", result)
        success = bool(number and int(number[1]) >= 0)
        returned = FD.match(result)
        path = None
        paths = []
        if call in RENAMES:
            paths = [ast.literal_eval(q) for q in QUOTED.findall(args)]
            if len(paths) != 2:
                raise ValidationError(f"unresolved rename paths at line {line_number}")
            path = paths[-1]
            if success:
                renames.append(tuple(paths))
                if os.path.dirname(path) == root:
                    directory_cause = _role(path, root)
        elif call == "openat":
            path = returned["path"] if returned else None
            if path is None:
                strings = QUOTED.findall(args)
                path = ast.literal_eval(strings[0]) if strings else None
            if returned and success:
                descriptor_flags[(pid, int(returned["fd"]))] = args
        elif first:
            path = first["path"]
        elif call in {"mmap", "munmap", "msync"}:
            found = FD.search(args)
            path = found["path"] if found else None
        if path:
            path = path.removesuffix(" (deleted)")
        role = "observer" if fd in {1, 2} and call in WRITES else _role(path, root)
        write = call in WRITES and success
        synchronous_write = bool(write and re.search(r"\bO_(?:D?SYNC)\b", descriptor_flags.get((pid, fd), "")))
        event = {"index": len(events), "line": line_number, "pid": pid, "timestamp": match["time"],
                 "phase": phase, "syscall": call, "path": path, "paths": paths,
                 "role": role, "success": success, "result": result,
                 "write": write, "fence": success and (call in {"fsync", "fdatasync", "msync"} or synchronous_write),
                 "synchronous_write": synchronous_write,
                 "root_directory_cause": directory_cause if role == "root_directory" and call in {"fsync", "fdatasync"} else None}
        events.append(event)
        if phase is not None:
            phases[phase].append(event)
        if call == "close" and fd is not None:
            descriptor_flags.pop((pid, fd), None)
    if phase is not None:
        raise ValidationError(f"unclosed phase {phase}")
    if not phases:
        raise ValidationError("no application phase markers observed")

    def canonical(path):
        if not path:
            return path
        # Immutable assets rename once, and bootstrap directories move once.
        # Keep event role separate from this final-name identity.
        for _ in range(len(renames) + 1):
            changed = False
            for before, after in renames:
                if path == before or path.startswith(before + "/"):
                    path = after + path[len(before):]
                    changed = True
            if not changed:
                return path
        raise ValidationError("cyclic rename identity")

    for event in events:
        event["canonical_path"] = canonical(event["path"])
        event["canonical_role"] = _role(event["canonical_path"], root)
    summaries = {}
    for name, rows in phases.items():
        summaries[name] = {
            "successful_fences_by_role": dict(collections.Counter(e["role"] for e in rows if e["fence"])),
            "all_root_directory_fences": sum(e["fence"] and e["role"] == "root_directory" for e in rows),
            "root_fences_by_cause": dict(collections.Counter(e["root_directory_cause"] or "unattributed" for e in rows if e["fence"] and e["role"] == "root_directory")),
            "asset_writes": sorted({e["canonical_path"] for e in rows if e["write"] and e["role"] in ASSETS}),
            "asset_fences": sorted({e["canonical_path"] for e in rows if e["fence"] and e["role"] in ASSETS}),
            "current_selections": sum(e["syscall"] in RENAMES and e["success"] and e["role"] == "current" for e in rows),
        }
    return {"schema": 1, "trace_sha256": hashlib.sha256(trace_path.read_bytes()).hexdigest(),
            "database": root, "phases": phases, "events": events, "roles": dict(collections.Counter(e["role"] for e in events)),
            "phase_summary": summaries, "renames": renames,
            "scope": "actual syscall observations; bootstrap/coordination/capacity reported separately, no powerloss claim"}


def _parts(value):
    allowed = {"nosync", "nometasync", "mapasync"}
    parts = set() if value == "full" else set(value.split("+"))
    if not parts <= allowed or (value != "full" and not parts):
        raise ValidationError(f"unknown policy {value}")
    return parts


def validate(case_args, parsed, version):
    if len(case_args) != 6 or version not in {"0.9.31", "0.9.36", "1.0.2"}:
        raise ValidationError("wrong frozen case shape/version")
    policy, wr, scenario, txn_policy, after_policy, finish = case_args
    if wr != "0":
        raise UnsupportedScope("physical WRITEMAP/MAPASYNC remains M11; buffered oracle cannot qualify it")
    initial, local, changed = _parts(policy), _parts(txn_policy), _parts(after_policy)
    if "mapasync" in local or (version != "1.0.2" and local):
        raise UnsupportedScope("transaction-local policy outside the pinned header contract")
    if finish not in {"commit", "abort"}:
        raise ValidationError("unknown final action")
    if scenario == "fault-sync":
        raise UnsupportedScope("EIO calibration uses a separate error-return oracle, not successful-fence validation")
    phases = parsed["phases"]
    checks = []
    def check(yes, why):
        if not yes:
            raise ValidationError(why)
        checks.append(why)
    def rows(name):
        check(name in phases, "phase present: " + name)
        return phases[name]
    def writes_or_fences(items, roles):
        return [e for e in items if e["role"] in roles and (e["write"] or e["fence"] or (e["syscall"] in RENAMES and e["success"]))]
    def selection(name, metadata):
        items = rows(name)
        chosen = [e for e in items if e["syscall"] in RENAMES and e["success"] and e["role"] == "current"]
        check(len(chosen) == 1, name + " exactly one actual CURRENT selection")
        file_fences = [e for e in items if e["fence"] and e["role"] == "current_temp"]
        check(bool(file_fences) == ("nosync" not in metadata), name + " selector inode fence matches current metadata policy (NoMeta strengthening explicit)")
        root_fences = [e for e in items if e["fence"] and e["role"] == "root_directory" and e["root_directory_cause"] == "current"]
        check(bool(root_fences) == (not metadata & {"nosync", "nometasync"}), name + " business CURRENT directory fence matches metadata policy")
        if file_fences:
            check(max(e["index"] for e in file_fences) < chosen[0]["index"], name + " selector file fence precedes rename")
        if root_fences:
            check(min(e["index"] for e in root_fences) > chosen[0]["index"], name + " selector root fence follows rename")
    def preparation(name, effective, publish=False, metadata=None):
        items = rows(name)
        writes = {e["canonical_path"] for e in items if e["write"] and e["role"] in ASSETS}
        fences = {e["canonical_path"] for e in items if e["fence"] and e["role"] in ASSETS}
        check(bool(writes), name + " actual immutable candidate assets written")
        if "nosync" in effective:
            check(not fences, name + " NoSync omits all asset fences, including inherited repair")
        else:
            check(writes <= fences, name + " every newly written immutable asset receives a real fence")
            parents = {str(Path(p).parent) for p in writes}
            fenced_directories = {e["path"] for e in items if e["fence"] and e["role"] == "asset_directory"}
            check(parents <= fenced_directories, name + " each written asset namespace receives its directory fence")
            if name != "seed.commit" and "nosync" in initial:
                first_index = items[0]["index"]
                inherited = {e["canonical_path"] for e in parsed["events"]
                             if e["index"] < first_index and e["write"] and e["canonical_role"] in ASSETS}
                check(inherited <= fences, name + " stronger preparation repairs every previously written inherited weak asset")
        if publish:
            selection(name, metadata)
        else:
            check(not writes_or_fences(items, SELECTORS), name + " preparation does not select or fence CURRENT/PREVIOUS")
    rows("env.open")
    preparation("seed.commit", initial, True, initial)
    current = changed if scenario in {"runtime-before", "runtime-after-prepare", "runtime-after-commit", "rollback-runtime-before", "rollback-runtime-after-prepare"} else initial
    if scenario.startswith("nested-") and scenario != "nested-abort":
        local |= changed
    prepared = scenario in {"prepare", "runtime-after-prepare", "rollback-prepared", "rollback-runtime-after-prepare"}
    consumed = scenario in {"prepare-clean", "prepare-readonly"}
    if scenario not in {"idle", "readonly", "readonly-active"}:
        rows("B.begin")
    if scenario not in {"idle", "readonly", "readonly-active", "sync-clean", "prepare-clean", "prepare-readonly"}:
        check(not writes_or_fences(rows("B.put"), ASSETS | SELECTORS), "buffered small B.put leaves business asset preparation/selection to prepare or commit")
    if scenario.startswith("nested-"):
        rows("nested.finish")
    if scenario in {"runtime-before", "runtime-after-prepare", "runtime-after-commit", "rollback-runtime-before", "rollback-runtime-after-prepare", "readonly", "readonly-active"}:
        rows("flags.change")
    if scenario in {"readonly", "readonly-active"}:
        rows("readonly.open")
    if prepared:
        preparation("B.prepare", initial | local)
    if consumed:
        check(not writes_or_fences(rows("B.prepare.consumed"), ASSETS | SELECTORS), "consumed clean/readonly prepare has no business writes/fences")
    if "B.finish" in phases:
        if finish == "abort":
            check(not writes_or_fences(rows("B.finish"), ASSETS | SELECTORS), "abort does not write/fence/select business assets")
        else:
            env = current if scenario in {"runtime-before", "runtime-after-prepare", "rollback-runtime-before", "rollback-runtime-after-prepare"} else initial
            if prepared:
                check(not writes_or_fences(rows("B.finish"), ASSETS), "prepared finalization never repeats data/descriptor writes or fences")
                selection("B.finish", env | local)
            else:
                preparation("B.finish", env | local, True, env | local)
    elif scenario not in {"idle", "readonly", "readonly-active", "sync-clean", "sync-dirty", "prepare-clean", "prepare-readonly", "fault-sync"}:
        raise ValidationError("missing B.finish phase")
    if scenario.startswith("rollback"):
        items = rows("rollback")
        check(bool(items), "rollback has concrete syscall observations")
        old = {e["canonical_path"] for e in parsed["events"] if e["index"] < items[0]["index"] and e["role"] in ASSETS}
        check(not any(e["fence"] and e["canonical_path"] in old for e in items if e["role"] in ASSETS), "rollback never refences any inherited A/B immutable asset")
        check(not any(e["role"] == "segment" and (e["write"] or e["fence"]) for e in items), "rollback has no user segment write/fence")
        new = {e["canonical_path"] for e in items if e["write"] and e["role"] in {"descriptor", "cohort"}}
        fenced = {e["canonical_path"] for e in items if e["fence"] and e["role"] in {"descriptor", "cohort"}}
        check(bool(new), "rollback writes a real fresh metadata descriptor")
        check((not fenced) if "nosync" in current else new <= fenced, "rollback new-descriptor fence uses ENV policy, not undone local policy")
        selection("rollback", current)
    for name, items in phases.items():
        check(not any(e["syscall"] in {"fsync", "fdatasync", "msync"} and not e["success"]
                      and e["role"] in ASSETS | SELECTORS | {"asset_directory", "root_directory"} for e in items), name + " successful-case fences have no hidden errors")
        check(not any(e["role"] == "other_database" and (e["write"] or e["fence"]) for e in items), name + " no unclassified database write/fence")
        if name == "flags.change" or name == "nested.finish":
            check(not writes_or_fences(items, ASSETS | SELECTORS), name + " changes metadata policy/ownership without business selection")
        if not name.startswith("sync."):
            continue
        readonly = name.startswith("sync.readonly")
        active = not readonly and (name.endswith("force1") or "nosync" not in current)
        check(not any(e["write"] and e["role"] in ASSETS | SELECTORS for e in items), name + " explicit sync writes no business assets/selectors")
        check(not any(e["syscall"] in RENAMES and e["success"] and e["role"] in SELECTORS for e in items), name + " explicit sync performs no selection")
        if not active:
            check(not any(e["fence"] for e in items), name + " readonly/NoSync nonforce executes no fence")
        else:
            check(any(e["fence"] and e["role"] in {"segment", "cohort"} for e in items), name + " active selected sync fences real user data")
            check(any(e["fence"] and e["role"] in {"descriptor", "cohort"} for e in items), name + " active selected sync fences real descriptors")
            check(any(e["fence"] and e["role"] == "current" for e in items), name + " active selected sync fences CURRENT file")
            check(any(e["fence"] and e["role"] == "root_directory" for e in items), name + " active selected sync fences root namespace")
            if prepared and "prepared" in name:
                candidate = {e["canonical_path"] for e in phases["B.prepare"] if e["write"] and e["role"] in ASSETS}
                check(not any(e["fence"] and e["canonical_path"] in candidate for e in items), name + " selected sync excludes every unpublished prepared asset")
    required_sync = "readonly" if scenario.startswith("readonly") else "idle" if scenario == "idle" else "consumed-prepare" if consumed else scenario if scenario in {"sync-clean", "sync-dirty"} else "after-rollback" if scenario.startswith("rollback") else "after"
    if prepared:
        for force in [0, 1]:
            rows(f"sync.prepared.force{force}")
    for force in [0, 1]:
        rows(f"sync.{required_sync}.force{force}")
        if scenario in {"readonly", "readonly-active"}:
            rows(f"sync.readonly.changed.force{force}")
    return {"scope": "buffered Linux native file C durability; physical mapping/powerloss/performance unqualified", "passed": True,
            "arguments": list(case_args), "version": version, "validated_expectations": checks,
            "phase_summary": parsed["phase_summary"],
            "strengthening": "NoMeta selector inode fsync; bootstrap/coordination/map-capacity fences remain visible. Capacity-hint root fsync can incidentally fence CURRENT on first provisioning."}
