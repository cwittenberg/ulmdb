#!/usr/bin/env python3
"""Compare frozen pre/post-capability engines without publishing public reports.

Each adjacent baseline/candidate pair contains the existing complete 15-case
engine suite and its normal-durability native LMDB reference. Both disk and
tmpfs are measured. A completed campaign is evidence, not a no-regression proof.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import tempfile
import uuid

from provenance import environment, machine, sha, source_files

HERE = Path(__file__).resolve().parent
ARMS = ("baseline", "candidate")
BACKENDS = {"resident": "experimental_resident_compact_borrowed", "default": "default_file_legacy"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    temporary.replace(path)


def snapshot(source):
    """Hash the explicit frozen tree; never substitute the live checkout."""
    return {str(p.relative_to(source)): sha(p) for p in source_files(source)}


def verify_source(source, manifest):
    require(snapshot(source) == manifest, f"frozen source changed: {source}")


def filesystem(path):
    return {"type": subprocess.check_output(["stat", "-f", "-c", "%T", str(path)], text=True).strip(),
            "device": path.stat().st_dev}


def schedule(rounds):
    """Adjacent AB/BA processes rotate arm and filesystem order."""
    jobs = []
    for pair in range(rounds):
        filesystems = ("disk", "ram") if pair % 2 == 0 else ("ram", "disk")
        for index, medium in enumerate(filesystems):
            arms = ARMS if (pair + index) % 2 == 0 else tuple(reversed(ARMS))
            jobs.extend({"pair": pair, "filesystem": medium, "arm": arm} for arm in arms)
    return jobs


def build(source, target, backend, log, env):
    command = ["cargo", "+1.94.0", "build", "--locked", "--release", "--manifest-path",
               str(source / "benchmark/engine/Cargo.toml")]
    if backend == "resident":
        command += ["--features", "resident"]
    with log.open("w") as stream:
        subprocess.run(command, cwd=source, env=env | {"CARGO_TARGET_DIR": str(target)},
                       stdout=stream, stderr=subprocess.STDOUT, check=True)
    executable = target / "release/ulmdb-engine-benchmark"
    require(executable.is_file(), "engine build did not produce its expected executable")
    return executable


def canonical_overlap(path, other):
    return path == other or path in other.parents or other in path.parents


def prepare_canonical(root, output, sources):
    """Create one fresh owned staging root; source inputs are never overwritten."""
    require(not root.exists() and not root.is_symlink(), "canonical build directory must be fresh")
    root = root.resolve()
    require(not root.exists() and not root.is_symlink(), "canonical build directory must be fresh")
    require(not canonical_overlap(root, output), "canonical staging and evidence output must not overlap")
    require(all(not canonical_overlap(root, source) for source in sources.values()),
            "canonical staging must not overlap frozen source inputs")
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    marker = {"schema": 1, "owner": uuid.uuid4().hex, "output": str(output)}
    write_json(root / ".lookup-regression-owner.json", marker)
    return {"directory": str(root), "source": str(root / "source"), "target": str(root / "target"),
            "owner": marker, "arms": {},
            "scope": "same canonical Cargo source and target paths for both arms, with target artifacts cleared between arms; baseline executable preserved before candidate staging; original frozen sources unchanged"}


def verify_canonical_owner(context):
    root = Path(context["directory"])
    require(not root.is_symlink() and root.is_dir(), "canonical staging ownership root changed")
    require(json.loads((root / ".lookup-regression-owner.json").read_text()) == context["owner"],
            "canonical staging owner marker changed")
    source, target = Path(context["source"]), Path(context["target"])
    require(source == root / "source" and target == root / "target", "canonical staging paths changed")
    require(not source.is_symlink() and not target.is_symlink(), "canonical staging paths cannot be symlinks")


def stage_canonical(context, original, manifest):
    verify_canonical_owner(context)
    verify_source(original, manifest)
    staged = Path(context["source"])
    target = Path(context["target"])
    if target.exists():
        require(context["arms"], "unexpected target artifacts before the first canonical build")
        for arm in context["arms"].values():
            require(sha(Path(arm["preserved_binary"])) == arm["preserved_binary_sha256"],
                    "previous canonical executable changed before clearing shared target")
        shutil.rmtree(target)
    if staged.exists():
        shutil.rmtree(staged)
    staged.mkdir()
    for relative, expected in manifest.items():
        require(not Path(relative).is_absolute() and ".." not in Path(relative).parts,
                "unsafe canonical source manifest path")
        contents = (original / relative).read_bytes()
        require(hashlib.sha256(contents).hexdigest() == expected, "original source changed while staging")
        target = staged / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(contents)
    verify_source(staged, manifest)
    verify_source(original, manifest)
    verify_canonical_owner(context)
    return staged


def preserve_canonical_binary(executable, destination):
    """Preserve baseline bytes before the shared target is rebuilt for candidate."""
    expected = sha(executable)
    require(not destination.exists(), "preserved executable must be new")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, destination)
    require(sha(destination) == expected, "canonical executable preservation failed")
    return destination


def canonical_fingerprints(target):
    result = {}
    for path in (target / "release/.fingerprint").glob("*/*.json"):
        if path.name.startswith(("lib-ulmdb", "bin-ulmdb_engine_benchmark", "bin-ulmdb-engine-benchmark")):
            result[str(path.relative_to(target))] = json.loads(path.read_text())
    return result


def validate_build_only_resume(receipt, sources, manifests, args, output):
    require(receipt.get("complete") is False, "completed campaigns cannot resume")
    require(receipt["configuration"].get("build_only") is True, "resume requires a build-only receipt")
    require(receipt.get("runs") == [] and "failure" not in receipt,
            "partial or failed timed campaigns cannot resume")
    require(set(receipt.get("binaries", {})) == set(ARMS), "both build-only executables are required")
    require(not receipt.get("resumed_from_build_only"), "a build-only receipt can execute only once")
    for key in ("backend", "cpu", "rounds", "keep_fixtures"):
        require(receipt["configuration"][key] == getattr(args, key), f"resume configuration differs: {key}")
    require(Path(receipt["configuration"]["ram_dir"]).resolve() == args.ram_dir.resolve(),
            "resume RAM directory differs")
    require(Path(receipt["configuration"]["output"]).resolve() == output, "resume output differs")
    previous_canonical = receipt["configuration"].get("canonical_build_dir")
    current_canonical = getattr(args, "canonical_build_dir", None)
    require((Path(previous_canonical).resolve() if previous_canonical else None)
            == (current_canonical.resolve() if current_canonical else None), "resume canonical build directory differs")
    require(receipt["schedule"] == schedule(args.rounds), "resume schedule differs")
    for arm in ARMS:
        require(Path(receipt["sources"][arm]["directory"]).resolve() == sources[arm],
                f"resume {arm} source directory differs")
        require(receipt["sources"][arm]["sha256"] == manifests[arm], f"resume {arm} frozen source differs")
        expected = output / "target" / arm / "release/ulmdb-engine-benchmark"
        require(Path(receipt["binaries"][arm]["path"]).resolve() == expected,
                f"resume {arm} executable path differs")


def workload_signature(report):
    return {"config": report["config"], "dataset_sha256": report["dataset_sha256"],
            "query_sha256": report["query_sha256"],
            "canonical_ipv4_records": report["canonical_ipv4_records"],
            "canonical_ipv6_records": report["canonical_ipv6_records"],
            "reads": [{key: case[key] for key in ("name", "queries", "hits", "matched_lengths",
                                                    "expected_checksum", "query_indices")}
                      for case in report["reads"]],
            "writes": [{key: case[key] for key in ("name", "records_per_transaction",
                                                     "commits_per_aggregate_phase", "separate_latency_phase_commits")}
                       | {"traces": [trial["trace_sha256"] for trial in case["trials"]]}
                       for case in report["writes"]]}


def validate_report(report, raw, binary_hash, manifest, backend, signature=None):
    require(report["suite"] == "full15" and report["schema"] == 1, "wrong engine benchmark schema")
    require(report["backend"] == BACKENDS[backend], "engine backend substitution")
    require(report["semantic_validation_passed"] is True and report["timing_enabled"] is True,
            "engine validation/timing did not pass")
    require(report["native_flags"] == 0, "native LMDB must use normal durability flags")
    require(report["binary_sha256"] == binary_hash, "executed engine binary hash mismatch")
    require(len(report["reads"]) == 13 and len(report["writes"]) == 2, "full 15-case inventory missing")
    require(len({case["name"] for case in report["reads"]}) == 13, "duplicate read case")
    require({case["records_per_transaction"] for case in report["writes"]} == {1, 64},
            "both durable batch sizes are required")
    require({case["records_per_transaction"] for case in report["combined_writes"]} == {1, 64},
            "combined durable write inventory missing")
    require(all(report["cold"][key] is True for key in (
        "old_reader_survived_environment_drop", "escaped_value_pointer_unchanged",
        "reopened_value_uses_different_live_mapping")), "cold reopen/borrow checks failed")
    require(report["source_hashes"] and all(manifest.get(path) == value
            for path, value in report["source_hashes"].items()), "embedded build source hashes differ from frozen inputs")
    require(hashlib.sha256(raw).hexdigest() == report["raw_latency_sha256"], "raw latency file hash mismatch")
    spans = [report["timer_control"]["raw_latency"]]
    for section in ("reads", "writes"):
        for case in report[section]:
            require(len(case["trials"]) == report["config"]["trials"], "trial count mismatch")
            for trial in case["trials"]:
                for engine in ("ulmdb", "lmdb"):
                    value = trial[engine]
                    elapsed = value["elapsed_ns" if section == "reads" else "aggregate_elapsed_ns"]
                    require(elapsed > 0, "nonpositive measured phase")
                    span = value["raw_latency"]
                    expected = case["queries"] if section == "reads" else case["separate_latency_phase_commits"]
                    require(span["count"] == expected, "raw sample count mismatch")
                    spans.append(span)
    extents = []
    for span in spans:
        begin, end = span["offset_bytes"], span["offset_bytes"] + span["count"] * 8
        require(begin % 8 == 0 and 0 <= begin <= end <= len(raw), "raw sample extent invalid")
        require(hashlib.sha256(raw[begin:end]).hexdigest() == span["sha256"], "raw sample span hash mismatch")
        extents.append((begin, end))
    cursor = 0
    for begin, end in sorted(extents):
        require(begin == cursor, "raw sample gaps or overlaps")
        cursor = end
    require(cursor == len(raw), "unaccounted raw latency bytes")
    current = workload_signature(report)
    require(signature is None or current == signature, "baseline/candidate workloads differ")
    return current


def import_compare(path):
    spec = importlib.util.spec_from_file_location("lookup_regression_compare", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def gm(values):
    values = list(values)
    return math.exp(statistics.fmean(math.log(value) for value in values))


def case_table(comparison):
    indexed = {row["id"]: row for row in comparison["rows"]}
    cases = sorted({(row["filesystem"], row["metric"].removesuffix("/throughput"))
                    for row in comparison["rows"] if row["engine"] == "ulmdb"
                    and row["metric"].endswith("/throughput")})
    rows = []
    for medium, case in cases:
        u = indexed[f"full/{medium}/ulmdb/{case}/throughput"]
        l = indexed[f"full/{medium}/lmdb/{case}/throughput"]
        tail = indexed[f"full/{medium}/ulmdb/{case}/sampled_p99"]
        ltail = indexed[f"full/{medium}/lmdb/{case}/sampled_p99"]
        process_changes = [100 * (u["candidate_observations"][pair] / value - 1)
                           for pair, value in sorted(u["baseline_observations"].items())]
        rows.append({"filesystem": medium, "case": case,
                     "baseline_ulmdb_per_second": u["baseline_center"],
                     "candidate_ulmdb_per_second": u["candidate_center"],
                     "baseline_lmdb_per_second": l["baseline_center"],
                     "candidate_lmdb_per_second": l["candidate_center"],
                     "ulmdb_change_pct": u["change_pct"], "lmdb_control_change_pct": l["change_pct"],
                     "candidate_vs_local_lmdb_pct": 100 * (u["candidate_center"] / l["candidate_center"] - 1),
                     "throughput_ratio_interval": u["interval"], "throughput_status": u["status"],
                     "process_throughput_changes_pct": process_changes,
                     "baseline_ulmdb_sampled_p99_ns": tail["baseline_center"],
                     "candidate_ulmdb_sampled_p99_ns": tail["candidate_center"],
                     "baseline_lmdb_sampled_p99_ns": ltail["baseline_center"],
                     "candidate_lmdb_sampled_p99_ns": ltail["candidate_center"],
                     "sampled_p99_change_pct": tail["change_pct"],
                     "sampled_p99_ratio_interval": tail["interval"], "sampled_p99_status": tail["status"],
                     "adverse_throughput_over_5_percent": u["ratio"] < .95,
                     "adverse_p99_over_5_percent": tail["ratio"] > 1.05})
    require(len(rows) == 30, "complete disk/RAM 30-case summary inventory missing")
    return rows


def render(output, captures, result):
    rows = case_table(result)
    require(len(captures["baseline"]["profiles"]) == len(captures["candidate"]["profiles"]), "arm process counts differ")
    rounds = captures["baseline"]["rounds"]
    adverse = [row for row in result["rows"] if row["role"] == "target" and row.get("ratio") is not None
               and (row["ratio"] < .95 if row["higher_is_better"] else row["ratio"] > 1.05)]
    write_json(output / "table.json", {"schema": 1, "throughput_cases": rows,
               "adverse_target_metrics_over_5_percent": [row["id"] for row in adverse],
               "all_metrics": result["rows"]})
    text = ["# Existing-engine regression campaign", "",
            f"All 15 existing read/write cases on disk and RAM: **30 cases**, {rounds} fresh processes per arm/filesystem, 16 alternating ULMDB/LMDB trials within each process. Baseline/candidate processes run as adjacent AB/BA pairs.", "",
            "This measures ordinary engine behavior with the optional lookup accelerator unused. It does not measure an application keeping a side index alive during writes, rebuilding it, or using it for other operations. The separate direct comparison qualifies accelerated reads.", "",
            "All engines use normal durability flags. RAM uses real tmpfs/ramfs and does not provide reboot durability. Measurements are warm synthetic single-core results on a shared host. Native LMDB controls are retained without normalization or outlier removal.", "",
            f"Centers are process-level geometric means. Sampled p99 columns are geometric means of each trial's timer-inclusive sampled p99; they are not pooled population percentiles. Intervals use paired process log ratios with Bonferroni adjustment across all captured metrics and {rounds} process pairs. Overlapping intervals do not establish equivalence, and no universal no-regression guarantee follows.", "",
            f"ULMDB target metrics with adverse point estimates: {len(result['summary']['adverse_point_estimate'])}. Target metrics worse by more than 5% at their observed centers: **{len(adverse)}**. Full uncertainty, unavailable values and every raw observation are retained in comparison.json.", "",
            "## Complete throughput table", "",
            "| Filesystem | Case | Baseline ULMDB/s | Candidate ULMDB/s | Change | Baseline LMDB/s | Candidate LMDB/s | LMDB drift | Candidate vs local LMDB | Paired ratio interval |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---|"]
    for row in rows:
        interval = row["throughput_ratio_interval"]
        interval_text = f"{interval[0]:.3f}–{interval[1]:.3f}" if interval else "Unavailable"
        text.append(f"| {row['filesystem']} | {row['case']} | {row['baseline_ulmdb_per_second']:,.0f} | {row['candidate_ulmdb_per_second']:,.0f} | {row['ulmdb_change_pct']:+.2f}% | {row['baseline_lmdb_per_second']:,.0f} | {row['candidate_lmdb_per_second']:,.0f} | {row['lmdb_control_change_pct']:+.2f}% | {row['candidate_vs_local_lmdb_pct']:+.2f}% | {interval_text} |")
    text += ["", "## Complete sampled latency table", "",
             "| Filesystem | Case | Baseline ULMDB p99 ns | Candidate ULMDB p99 ns | Change | Baseline LMDB p99 ns | Candidate LMDB p99 ns | Paired ratio interval |",
             "|---|---|---:|---:|---:|---:|---:|---|"]
    for row in rows:
        interval = row["sampled_p99_ratio_interval"]
        interval_text = f"{interval[0]:.3f}–{interval[1]:.3f}" if interval else "Unavailable"
        text.append(f"| {row['filesystem']} | {row['case']} | {row['baseline_ulmdb_sampled_p99_ns']:,.1f} | {row['candidate_ulmdb_sampled_p99_ns']:,.1f} | {row['sampled_p99_change_pct']:+.2f}% | {row['baseline_lmdb_sampled_p99_ns']:,.1f} | {row['candidate_lmdb_sampled_p99_ns']:,.1f} | {interval_text} |")
    text += ["", "## Every other measured metric", "",
             "Includes combined durable commit phases, setup, ULMDB reopen, file allocation and mapping observations. LMDB cold reopen is not measured by the existing suite; unavailable Resident pathname-attributed RSS remains unavailable.", "",
             "| Metric | Role | Baseline | Candidate | Change | Paired ratio interval | Status |",
             "|---|---|---:|---:|---:|---|---|"]
    for row in result["rows"]:
        if row["metric"].endswith(("/throughput", "/sampled_p99")):
            continue
        base, candidate = row.get("baseline_center"), row.get("candidate_center")
        interval = row["interval"]
        values = (f"{base:,.3f}", f"{candidate:,.3f}") if base is not None else ("Unavailable", "Unavailable")
        change = f"{row['change_pct']:+.2f}%" if row["change_pct"] is not None else "N/A"
        interval_text = f"{interval[0]:.4g}–{interval[1]:.4g}" if interval else "Unavailable"
        text.append(f"| {row['id']} ({row['unit']}) | {row['role']} | {values[0]} | {values[1]} | {change} | {interval_text} | {row['status']} |")
    text += ["", "## Target metrics worse by more than 5%", ""]
    text.extend([f"- `{row['id']}`: {row['change_pct']:+.2f}% ({row['status']})." for row in adverse]
                or ["None at the observed centers. This does not prove equivalence."])
    text += ["", "Source manifests, build logs, executable SHA-256 values, exact schedule, process reports, complete latency samples and their digests are retained beside this report. No existing public README, performance table or benchmark/results file is changed.", ""]
    (output / "REGRESSION.md").write_text("\n".join(text))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True, help="frozen pre-capability repository source")
    parser.add_argument("--candidate", type=Path, required=True, help="frozen post-capability repository source")
    parser.add_argument("--output", type=Path, required=True, help="new private evidence directory")
    parser.add_argument("--backend", choices=BACKENDS, default="resident")
    parser.add_argument("--cpu", type=int, default=min(os.sched_getaffinity(0)))
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--ram-dir", type=Path, default=Path("/dev/shm"))
    parser.add_argument("--keep-fixtures", action="store_true")
    parser.add_argument("--build-only", action="store_true", help="build and retain receipts without starting any timed process")
    parser.add_argument("--resume", action="store_true", help="execute a verified build-only receipt with no completed processes; partial campaigns cannot resume")
    parser.add_argument("--canonical-build-dir", type=Path,
                        help="fresh owned staging directory outside both frozen sources and output; compile both arms at the same source and target paths")
    args = parser.parse_args(argv)
    require(args.rounds >= 3, "at least three fresh process pairs required")
    require(not (args.resume and args.build_only), "--resume starts measurements and cannot accompany --build-only")
    require(args.cpu in os.sched_getaffinity(0), "CPU is outside the allowed affinity set")
    sources = {arm: getattr(args, arm).resolve() for arm in ARMS}
    require(sources["baseline"] != sources["candidate"], "distinct frozen source directories required")
    manifests = {arm: snapshot(source) for arm, source in sources.items()}
    support = lambda arm: {path: value for path, value in manifests[arm].items()
                           if path.startswith("benchmark/support/lmdb/")}
    require(support("baseline") == support("candidate"), "native LMDB source/adapter inputs differ")
    require(filesystem(args.ram_dir)["type"] in ("tmpfs", "ramfs"), "RAM path must be a mounted tmpfs or ramfs")
    output = args.output.resolve()
    tooling = {"regression.py": Path(__file__), "provenance.py": HERE / "provenance.py",
               "compare.py": HERE.parent / "compare.py"}
    env = environment() | {"PYTHONDONTWRITEBYTECODE": "1"}
    configuration = {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()}
    binaries = {}
    if args.resume:
        receipt = json.loads((output / "provenance.json").read_text())
        validate_build_only_resume(receipt, sources, manifests, args, output)
        for name, expected in receipt["tooling_sha256"].items():
            require(sha(output / "tooling" / name) == expected, "recorded build tooling changed")
        for arm in ARMS:
            binaries[arm] = Path(receipt["binaries"][arm]["path"])
            require(sha(binaries[arm]) == receipt["binaries"][arm]["sha256"], "built executable changed before resume")
        current_machine = machine(args.cpu)
        require(current_machine["rustc"] == receipt["machine"]["rustc"], "compiler identity changed since build-only phase")
        resume_tooling = output / "tooling-execution"
        resume_tooling.mkdir(exist_ok=False)
        for name, source in tooling.items():
            shutil.copyfile(source, resume_tooling / name)
        receipt["execution_tooling_sha256"] = {name: sha(resume_tooling / name) for name in tooling}
        receipt["build_only_configuration"] = receipt["configuration"]
        receipt["configuration"] = configuration
        receipt["resumed_from_build_only"] = True
        receipt["resumed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        receipt["resume_machine"] = current_machine
        receipt["resume_scope"] = "execute the same recorded binaries over unchanged frozen sources; no rebuild or claim of unrecorded build-environment equivalence"
        write_json(output / "provenance.json", receipt)
    else:
        output.mkdir(parents=True, exist_ok=False)
        (output / "tooling").mkdir()
        (output / "raw").mkdir()
        for name, source in tooling.items():
            shutil.copyfile(source, output / "tooling" / name)
        receipt = {"schema": 1, "complete": False,
               "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
               "configuration": configuration,
               "machine": machine(args.cpu), "backend": args.backend,
               "sources": {arm: {"directory": str(source), "sha256": manifests[arm]} for arm, source in sources.items()},
               "tooling_sha256": {name: sha(output / "tooling" / name) for name in tooling},
               "schedule": schedule(args.rounds), "runs": []}
        write_json(output / "provenance.json", receipt)
        canonical = prepare_canonical(args.canonical_build_dir, output, sources) if args.canonical_build_dir else None
        if canonical is not None:
            receipt["canonical_build"] = canonical
            write_json(output / "provenance.json", receipt)
        for arm in ARMS:
            print(f"Building frozen {arm} {args.backend} engine", flush=True)
            if canonical is not None:
                staged = stage_canonical(canonical, sources[arm], manifests[arm])
                common_target = Path(canonical["target"])
                compiled = build(staged, common_target, args.backend, output / f"build-{arm}.log", env)
                verify_source(staged, manifests[arm])
                verify_canonical_owner(canonical)
                binaries[arm] = preserve_canonical_binary(compiled,
                    output / "target" / arm / "release/ulmdb-engine-benchmark")
                canonical["arms"][arm] = {"original_source": str(sources[arm]),
                    "compiled_source": str(staged), "compiled_target": str(common_target),
                    "source_sha256": manifests[arm], "verified_before_and_after_build": True,
                    "preserved_binary": str(binaries[arm]),
                    "preserved_binary_sha256": sha(binaries[arm]),
                    "fingerprints": canonical_fingerprints(common_target)}
            else:
                binaries[arm] = build(sources[arm], output / "target" / arm, args.backend,
                                      output / f"build-{arm}.log", env)
            verify_source(sources[arm], manifests[arm])
            receipt.setdefault("binaries", {})[arm] = {"path": str(binaries[arm]), "sha256": sha(binaries[arm])}
            write_json(output / "provenance.json", receipt)
    compare = import_compare(output / "tooling/compare.py")
    if args.build_only:
        print(f"Build-only receipts: {output / 'provenance.json'}", flush=True)
        return 0
    disk = output / "fixtures" / "disk"
    disk.mkdir(parents=True)
    ram = Path(tempfile.mkdtemp(prefix="ulmdb-lookup-regression-", dir=args.ram_dir))
    media = {"disk": disk, "ram": ram}
    media_info = {medium: filesystem(path) for medium, path in media.items()}
    captures = {arm: {"schema": 1, "suite": "full", "backend": args.backend, "rounds": args.rounds,
                     "generated_at": receipt["started_at"], "machine": receipt["machine"], "cpu": args.cpu,
                     "filesystems": media_info, "source_sha256": manifests[arm], "profiles": []} for arm in ARMS}
    receipt["fixture_roots"] = {key: str(value) for key, value in media.items()}
    signature, native_identity = None, None
    try:
        for job in receipt["schedule"]:
            arm, medium, pair = job["arm"], job["filesystem"], job["pair"]
            stem = f"{medium}-p{pair}-{arm}"
            directory = media[medium] / stem
            raw_out, stderr = output / f"{stem}.json", output / f"{stem}.stderr.log"
            verify_source(sources[arm], manifests[arm])
            require(sha(binaries[arm]) == receipt["binaries"][arm]["sha256"], "engine executable changed before timing")
            print(f"Measuring {stem}", flush=True)
            with raw_out.open("w") as out, stderr.open("w") as err:
                subprocess.run(["taskset", "-c", str(args.cpu), str(binaries[arm]), "bench", str(directory)],
                               cwd=sources[arm], env=env, stdout=out, stderr=err, check=True)
            report = json.loads(raw_out.read_text())
            require(report["raw_latency_file"] == "latencies.u64le", "unexpected raw latency filename")
            raw_path = output / "raw" / f"{stem}.latencies.u64le"
            shutil.copyfile(directory / "latencies.u64le", raw_path)
            signature = validate_report(report, raw_path.read_bytes(), receipt["binaries"][arm]["sha256"],
                                        manifests[arm], args.backend, signature)
            identity = {"version": report["native_version"], "flags": report["native_flags"]}
            require(native_identity is None or native_identity == identity, "native LMDB reference identity changed")
            native_identity = identity
            expected_medium = media_info[medium]["type"]
            require(report["filesystem"].strip() == expected_medium, "actual benchmark filesystem differs")
            verify_source(sources[arm], manifests[arm])
            require(sha(binaries[arm]) == receipt["binaries"][arm]["sha256"], "engine executable changed during timing")
            captures[arm]["profiles"].append({"filesystem": medium, "round": pair, "pair": pair,
                                              "raw_file": raw_out.name, "raw_sha256": sha(raw_out),
                                              "latency_copy": str(raw_path.relative_to(output)),
                                              "report": report})
            receipt["runs"].append(job | {"raw_file": raw_out.name, "raw_sha256": sha(raw_out),
                                          "latency_file": str(raw_path.relative_to(output)), "latency_sha256": sha(raw_path)})
            write_json(output / f"{arm}.partial.json", captures[arm])
            write_json(output / "provenance.json", receipt)
            if not args.keep_fixtures:
                shutil.rmtree(directory)
        for arm in ARMS:
            require(len(captures[arm]["profiles"]) == 2 * args.rounds, "missing full-suite processes")
            write_json(output / f"{arm}.full.json", captures[arm])
        result = compare.compare({"full": captures["baseline"]}, {"full": captures["candidate"]},
                                 paired=True, min_repeats=3, adjustment="bonferroni")
        write_json(output / "comparison.json", result)
        (output / "COMPARISON.md").write_text(compare.markdown(result))
        render(output, captures, result)
        receipt["complete"] = True
        receipt["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        receipt["screen_clear"] = result["summary"]["screen_clear"]
        receipt["scope"] = "complete observed regression screen, not universal nonregression or accelerated-side-index workload qualification"
        write_json(output / "results.json", receipt)
        write_json(output / "provenance.json", receipt)
        if not args.keep_fixtures:
            ram.rmdir()
        print(f"Complete evidence: {output / 'REGRESSION.md'}", flush=True)
        return 0
    except Exception as error:
        receipt["failure"] = {"type": type(error).__name__, "message": str(error)}
        write_json(output / "failure.json", receipt)
        raise


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Regression campaign failed; retained diagnostics: {error}", file=sys.stderr)
        sys.exit(2)
