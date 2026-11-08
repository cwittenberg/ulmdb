#!/usr/bin/env python3
"""Run and retain the direct native/hash/LMDB comparison without publishing reports.

All timings are sequential fresh processes against one explicitly frozen source.
Use --build-only first, then the same arguments with --resume to measure it.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import csv
import datetime
import fcntl
import hashlib
import io
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time

from provenance import environment, machine, sha, source_files

ARMS = ("legacy_native", "legacy_hash", "resident_native", "resident_hash", "lmdb")
READS = ("hit", "miss", "mixed50", "hot90", "scan_forward100", "scan_reverse100",
         "range_forward64", "range_reverse64")
PINS = ("unpinned_before", "pinned", "unpinned_after")
BATCHES = (1, 64, 1000)
NOTLS = 0x200000
MEDIAN = statistics.median
SCRIPT = Path(__file__).resolve()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    temporary.replace(path)


def snapshot(source):
    return {str(p.relative_to(source)): sha(p) for p in source_files(source)}


def tree_hash(manifest):
    return hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def verify_source(source, manifest):
    require(snapshot(source) == manifest, "frozen source changed; retained timings cannot be mixed")


def arm_config(arm):
    if arm == "lmdb":
        return "legacy", "lmdb"
    return tuple(arm.split("_", 1))


def schedule(sizes, rounds, seed):
    """Keep all five arms adjacent; reverse/rotate order between fresh rounds."""
    pairs = [(size, key) for size in sizes for key in ("fixed16", "hostname")]
    jobs = []
    for round_id in range(rounds):
        selected = pairs[round_id % len(pairs):] + pairs[:round_id % len(pairs)]
        arms = ARMS if round_id % 2 == 0 else tuple(reversed(ARMS))
        offset = (round_id // 2) % len(arms)
        arms = arms[offset:] + arms[:offset]
        for size, key in selected:
            for arm in arms:
                jobs.append({"id": f"r{round_id}-n{size}-{key}-{arm}", "round": round_id,
                             "size": size, "key_kind": key, "arm": arm, "seed": seed + round_id})
    return jobs


def samples_by_case(report):
    result = defaultdict(list)
    for row in report["samples"]:
        key = (row["phase"], row.get("pin_state", ""), row["workload"])
        result[key].append(row)
    return result


def validate_lmdb_flags(details):
    require(details.get("flags") == NOTLS and details.get("normal_sync") is True,
            "native LMDB must use NOTLS only and normal durable commits")


def validate_report(report, job, config):
    require(report.get("schema") == 1 and report.get("benchmark") == "direct_completed_lookup_strategy_comparison",
            "wrong direct-comparison schema")
    expected_backend = "native_lmdb" if job["arm"] == "lmdb" else arm_config(job["arm"])[0]
    expected_arm = arm_config(job["arm"])[1]
    c = report["config"]
    expected = {"keys": job["size"], "queries": config["queries"], "samples": config["samples"],
                "seed": job["seed"], "arm": expected_arm, "backend": expected_backend,
                "key_kind": job["key_kind"], "value_bytes": 32,
                "write_transactions_per_sample": config["write_transactions"]}
    require(all(c.get(k) == v for k, v in expected.items()), "configuration/backend/workload substitution")
    require(c.get("ballast_mib", 0) == config["ballast_mib"], "ballast configuration mismatch")
    validation = report["validation"]
    require(all(validation.get(k) is True for k in (
        "passed", "all_initial_old_new_reopened_records_exact_bytes", "all_timed_queries_checked",
        "forward_reverse_bounded_ranges")), "semantic validation missing or failed")
    require(validation.get("native_arm_never_constructs_hash") is (expected_arm != "hash"),
            "source-only/hash arm admission marker mismatch")
    cases = samples_by_case(report)
    expected_cases = {("read", "", name) for name in READS} | {
        ("write", pin, f"durable_batch{batch}") for pin in PINS for batch in BATCHES}
    require(set(cases) == expected_cases, "missing or unexpected read/write case")
    for (phase, _, workload), rows in cases.items():
        require(len(rows) == config["samples"] and {r["sample"] for r in rows} == set(range(config["samples"])),
                "missing, duplicate or out-of-range sample")
        for row in rows:
            elapsed, per_op = row["elapsed_ns"], row["ns_per_op"]
            require(isinstance(elapsed, int) and elapsed > 0 and math.isfinite(per_op) and per_op > 0,
                    "invalid timing value")
            operations = config["queries"] if phase == "read" and workload in READS[:4] else min(config["queries"], 1024)
            if phase == "write":
                batch = int(workload.removeprefix("durable_batch"))
                operations = batch * config["write_transactions"]
                require(row["transactions"] == config["write_transactions"], "durable transaction count mismatch")
                require(row["durability"] == "normal synchronized durable commit; no relaxed flags",
                        "relaxed or unknown write durability")
                require(math.isclose(row["ns_per_transaction"], elapsed / row["transactions"], rel_tol=1e-12),
                        "transaction denominator mismatch")
            else:
                require(isinstance(row.get("checksum"), int), "missing timed read checksum")
            require(row["operations"] == operations, "query/operation count mismatch")
            require(math.isclose(per_op, elapsed / operations, rel_tol=1e-12), "query denominator mismatch")
    life = report["lifecycle"]
    phases = {row["phase"] for row in life}
    required = {"create_environment", "fixture_load", "maintenance_before_pin", "begin_snapshot", "prepare_lookup",
                "rebuild_after_pinned_writes", "drop_new_lookup", "drop_old_lookup", "release_old_snapshot",
                "prepare_final_validation", "final_validation", "close_environment", "reopen_environment",
                "prepare_after_reopen"}
    expected_lifecycle = required | ({"reopen_database_handle"} if expected_arm == "lmdb" else set())
    require(expected_lifecycle == phases, "missing/unexpected build/drop/reopen lifecycle evidence")
    require(len(phases) == len(life), "duplicate lifecycle phase")
    require(all(isinstance(r["elapsed_ns"], int) and r["elapsed_ns"] >= 0 for r in life),
            "invalid lifecycle timing")
    maintenance = next(r["details"] for r in life if r["phase"] == "maintenance_before_pin")
    if expected_backend == "resident":
        require(maintenance.get("maintenance") == "explicit ordinary Collect before retaining long-lived snapshot",
                "Resident pre-pin collection protocol missing or substituted")
        slots = (job["size"] + 23) // 24 + 8192
        for state in ("before", "after"):
            options = maintenance[state]["options"]
            require(f"kv_pages: {slots}," in options and "max_metadata_bytes: 268435456," in options,
                    "Resident two-generation pool/metadata geometry changed")
    else:
        require(maintenance.get("maintenance") == "none", "unexpected pre-pin maintenance for native file engine")
    initial = next(r["details"] for r in life if r["phase"] == "prepare_lookup")
    if expected_arm == "hash":
        require(initial.get("algorithm") == "Hash" and initial.get("enumerated") is True,
                "production hash strategy was not constructed")
        require(initial["entries"] == job["size"] and initial["total_value_bytes"] == job["size"] * 32,
                "hash is incomplete or references unexpected values")
        require(0 < initial["index_bytes"] <= c["hash_index_budget_bytes"], "actual hash slot budget exceeded")
        require(initial["capacity"] >= 2 * job["size"], "unexpected hash occupancy")
    else:
        require(initial.get("index_bytes") == 0, "source-only arm constructed an index")
        if expected_arm == "native":
            require(initial.get("algorithm") == "original native KvRead" and initial.get("enumerated") is False,
                    "native baseline was replaced by the new wrapper")
    if expected_arm == "lmdb":
        details = next(r["details"] for r in life if r["phase"] == "create_environment")
        validate_lmdb_flags(details)
    require(report.get("lmdb_version") and report.get("lmdb_build_info"), "missing pinned LMDB provenance")
    memory = report["memory"]
    require(len({r["phase"] for r in memory}) == len(memory), "duplicate RSS checkpoint")
    expected_memory = {"process_start_before_fixture", "fixture_ready", "resident_ballast_ready",
                       "loaded_source_only", "before_prepare", "after_prepare", "after_full_validation",
                       "after_pinned_writes", "two_generations", "after_old_lookup_drop",
                       "after_old_snapshot_release", "closed_environment", "after_reopen_full_validation"} | {
                           f"{pin}_batch{batch}" for pin in PINS for batch in BATCHES} | {
                           f"warm_{name}" for name in READS[:4]}
    require(expected_memory == {r["phase"] for r in memory}, "missing/unexpected RSS checkpoint")
    require(all(isinstance(r.get("vmrss_kib"), int) and r["vmrss_kib"] > 0 for r in memory),
            "actual Linux RSS observation missing")
    require(set(report["files"]) == {"apparent_bytes", "allocated_bytes"}, "missing file-space costs")


def semantic_signature(report):
    """Exclude timings/identities while retaining every sample's actual work."""
    return sorted((r["phase"], r.get("pin_state", ""), r["workload"], r["sample"],
                   r["operations"], r.get("transactions"), r.get("checksum")) for r in report["samples"])


def validate_cross_arm(runs):
    grouped = defaultdict(list)
    if runs:
        reference_build = (runs[0]["report"]["lmdb_version"], runs[0]["report"]["lmdb_build_info"])
    for run in runs:
        require((run["report"]["lmdb_version"], run["report"]["lmdb_build_info"]) == reference_build,
                "native LMDB reference changed during campaign")
        grouped[(run["round"], run["size"], run["key_kind"])].append(run)
    for key, values in grouped.items():
        require(len({r["arm"] for r in values}) == len(values), "duplicate accepted arm")
        reference = semantic_signature(values[0]["report"])
        for run in values[1:]:
            require(semantic_signature(run["report"]) == reference,
                    f"cross-arm checksum/query/operation inventory mismatch: {key}")
            require((run["report"]["lmdb_version"], run["report"]["lmdb_build_info"]) ==
                    (values[0]["report"]["lmdb_version"], values[0]["report"]["lmdb_build_info"]),
                    "native LMDB reference changed between arms")


def runtime_context():
    memory = Path("/proc/meminfo").read_text().splitlines()
    return {"loadavg": Path("/proc/loadavg").read_text().strip(),
            "mem_available_kib": next(int(s.split()[1]) for s in memory if s.startswith("MemAvailable:"))}


def build(source, output, env):
    command = ["cargo", "+1.94.0", "build", "--release", "--locked", "--offline", "--manifest-path",
               str(source / "benchmark/lookup-strategies/comparison/Cargo.toml")]
    with (output / "build.log").open("w") as stream:
        subprocess.run(command, cwd=source, env=env | {"CARGO_TARGET_DIR": str(output / "target")},
                       stdout=stream, stderr=subprocess.STDOUT, check=True)
    binary = output / "target/release/ulmdb-direct-strategy-comparison"
    require(binary.is_file(), "comparison executable was not produced")
    return binary, command


def retain_failure(document, output, job, attempt, message):
    document.setdefault("failures", []).append({"job": job, "attempt": attempt, "reason": str(message),
                                               "at": datetime.datetime.now(datetime.timezone.utc).isoformat()})
    document["complete"] = False
    document["status"] = "failed; raw output retained, no completed table published"
    write_json(output / "results.json", document)


def execute(args):
    source, output = args.source.resolve(), args.output.resolve()
    require(source.is_dir(), "--source must be the explicit frozen candidate tree")
    require(source != output and source not in output.parents, "output must be outside the frozen source")
    require(args.cpu in os.sched_getaffinity(0), "CPU is outside the allowed affinity mask")
    sizes = [int(n) for n in args.sizes.split(",")]
    require(len(set(sizes)) == len(sizes) and all(2 <= n <= 10_000_000 for n in sizes), "invalid/duplicate sizes")
    require(min(args.rounds, args.queries, args.samples, args.write_transactions, args.seed) > 0,
            "rounds/counts/seed must be positive")
    require(0 <= args.ballast_mib <= 1024, "ballast must be in [0,1024] MiB")
    config = {"sizes": sizes, "rounds": args.rounds, "queries": args.queries, "samples": args.samples,
              "write_transactions": args.write_transactions, "cpu": args.cpu, "seed": args.seed,
              "ballast_mib": args.ballast_mib}
    if args.resume:
        require(output.is_dir() and (output / "results.json").is_file(), "resume requires a retained campaign")
    else:
        output.mkdir(parents=True, exist_ok=False)
    lock = (output / ".runner.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    env = environment()
    if args.resume:
        document = json.loads((output / "results.json").read_text())
        require(document["configuration"] == config and document["source"] == str(source),
                "resume must use exactly the retained source and configuration")
        require(document["runner_sha256"] == sha(SCRIPT), "runner changed; retain a separately versioned campaign")
        verify_source(source, document["source_hashes"])
        binary = Path(document["binary"])
        require(sha(binary) == document["binary_sha256"], "comparison executable changed")
        for run in document["runs"]:
            require(sha(output / run["raw_file"]) == run["raw_sha256"], "retained raw result changed")
            raw = json.loads((output / run["raw_file"]).read_text())
            require(raw == run["report"], "retained parsed report differs from raw output")
            validate_report(raw, run, config)
        validate_cross_arm(document["runs"])
    else:
        manifest = snapshot(source)
        document = {"schema": 1, "benchmark": "production_lookup_direct_campaign", "source": str(source),
                    "source_hashes": manifest, "source_manifest_sha256": tree_hash(manifest),
                    "runner_sha256": sha(SCRIPT), "configuration": config, "machine": machine(args.cpu),
                    "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    "scope": {"fixture": "synthetic bytewise fixed16/hostname KV;32-byte borrowed values",
                              "execution": "five adjacent fresh-process arms; sequential timing; shared host",
                              "native": "original KvRead; no hash built; identical old-snapshot pin state",
                              "hash": "actual opt-in production fixed-slot table; borrowed keys/values;512MiB slot cap",
                              "resident_preparation": "charged explicit ordinary Collect before old pin for both arms;ceil(keys/24)+8192 KV slots;256MiB metadata cap",
                              "lmdb": "pinned C reference;NOTLS only;normal locking and synchronized commits",
                              "limits": "warm single-thread reads;not custom comparators,large-value benchmarks,cold-cache,controlled memory pressure,concurrent throughput,crash/power-loss qualification"},
                    "jobs": schedule(sizes, args.rounds, args.seed), "runs": [], "failures": [],
                    "complete": False, "status": "building"}
        write_json(output / "results.json", document)
        (output / "direct_campaign.py").write_bytes(SCRIPT.read_bytes())
        print("Building frozen production comparison", flush=True)
        binary, command = build(source, output, env)
        verify_source(source, manifest)
        document.update(binary=str(binary), binary_sha256=sha(binary), build_command=command,
                        build_log_sha256=sha(output / "build.log"), status="built; timings not yet run")
        write_json(output / "results.json", document)
    if args.build_only:
        print(f"Build retained: {output / 'results.json'}; no timings run", flush=True)
        return
    accepted = {run["id"] for run in document["runs"]}
    (output / "raw").mkdir(exist_ok=True)
    (output / "databases").mkdir(exist_ok=True)
    for job in document["jobs"]:
        if job["id"] in accepted:
            continue
        verify_source(source, document["source_hashes"])
        require(sha(binary) == document["binary_sha256"] and sha(SCRIPT) == document["runner_sha256"],
                "runner/executable changed before timing")
        attempt = 0
        while (output / "raw" / f"{job['id']}-a{attempt}.json").exists():
            attempt += 1
        stem = f"{job['id']}-a{attempt}"
        raw, err = output / "raw" / f"{stem}.json", output / "raw" / f"{stem}.stderr.log"
        backend, arm = arm_config(job["arm"])
        command = ["taskset", "-c", str(args.cpu), str(binary), "--keys", str(job["size"]),
                   "--queries", str(args.queries), "--samples", str(args.samples), "--seed", str(job["seed"]),
                   "--arm", arm, "--backend", backend, "--key-kind", job["key_kind"],
                   "--write-transactions", str(args.write_transactions), "--ballast-mib", str(args.ballast_mib),
                   "--dir", str(output / "databases" / stem)]
        print(f"Measuring {job['id']}", flush=True)
        before, start = runtime_context(), time.monotonic()
        try:
            with raw.open("w") as out, err.open("w") as errors:
                subprocess.run(command, cwd=source, env=env, stdout=out, stderr=errors, check=True)
            report = json.loads(raw.read_text())
            validate_report(report, job, config)
            run = job | {"report": report, "raw_file": str(raw.relative_to(output)), "raw_sha256": sha(raw),
                         "stderr_file": str(err.relative_to(output)), "command": command,
                         "process_wall_seconds": time.monotonic() - start, "host_before": before,
                         "host_after": runtime_context()}
            validate_cross_arm(document["runs"] + [run])
            verify_source(source, document["source_hashes"])
            document["runs"].append(run)
            document["status"] = f"measuring;{len(document['runs'])}/{len(document['jobs'])} accepted processes"
            write_json(output / "results.json", document)
        except Exception as error:
            retain_failure(document, output, job, attempt, error)
            raise
    require(len(document["runs"]) == len(document["jobs"]), "not all planned processes completed")
    validate_cross_arm(document["runs"])
    document["complete"] = True
    document["status"] = "complete; every raw process retained"
    document["completed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    first = document["runs"][0]["report"]
    document["lmdb_version"] = first["lmdb_version"]
    document["lmdb_build_info"] = first["lmdb_build_info"]
    write_json(output / "results.json", document)
    render(output, document)
    print(f"Complete table: {output / 'FULL_TABLE.md'}", flush=True)


def aggregate(document):
    require(document.get("complete") is True, "partial evidence cannot produce a complete table")
    config = document["configuration"]
    expected_jobs = schedule(config["sizes"], config["rounds"], config["seed"])
    require(len(document["runs"]) == len(expected_jobs) and
            {r["id"] for r in document["runs"]} == {j["id"] for j in expected_jobs},
            "complete campaign is missing planned processes")
    groups = defaultdict(lambda: defaultdict(dict))
    units = {}
    for run in document["runs"]:
        report, size, kind, arm, round_id = run["report"], run["size"], run["key_kind"], run["arm"], run["round"]
        for (phase, pin, case), values in samples_by_case(report).items():
            key = (phase, size, kind, pin, case)
            metric = "ns_per_transaction" if phase == "write" else "ns_per_op"
            groups[key][arm][round_id] = MEDIAN(row[metric] for row in values)
            units[key] = metric
        for row in report["lifecycle"]:
            key = ("lifecycle", size, kind, "", row["phase"])
            groups[key][arm][round_id] = row["elapsed_ns"]
            units[key] = "ns_per_event"
        preparation = {r["phase"]: r["elapsed_ns"] for r in report["lifecycle"]}
        key = ("lifecycle", size, kind, "preparation", "total_snapshot_prepare")
        components = ("maintenance_before_pin", "begin_snapshot", "prepare_lookup")
        require(set(components) <= set(preparation), "preparation total lacks charged maintenance/snapshot/index cost")
        groups[key][arm][round_id] = sum(preparation[name] for name in components)
        units[key] = "ns_per_event"
        for row in report["memory"]:
            for metric in ("vmrss_kib", "vmhwm_kib", "rssanon_kib", "rssfile_kib", "rssshmem_kib", "vmsize_kib", "vmpte_kib"):
                if row.get(metric) is None:
                    continue
                key = ("memory", size, kind, row["phase"], metric)
                groups[key][arm][round_id] = row[metric]
                units[key] = "KiB"
        for metric, amount in report["files"].items():
            key = ("storage", size, kind, "after_reopen_validation", metric)
            groups[key][arm][round_id] = amount
            units[key] = "bytes"
        stats = next(r["details"] for r in report["lifecycle"] if r["phase"] == "prepare_lookup")
        key = ("index", size, kind, "initial_preparation", "owned_slot_bytes")
        groups[key][arm][round_id] = stats["index_bytes"]
        units[key] = "bytes"
    output = []
    for key, arms in sorted(groups.items()):
        row = dict(zip(("family", "keys", "key_kind", "phase", "operation"), key))
        row["unit"] = units[key]
        if row["operation"] == "total_snapshot_prepare":
            row["derived_components"] = ["maintenance_before_pin", "begin_snapshot", "prepare_lookup"]
        row["arms"] = {arm: {"median": MEDIAN(values.values()), "minimum": min(values.values()),
                              "maximum": max(values.values()), "process_values": values}
                       for arm, values in arms.items()}
        row["comparisons"] = {}
        for backend in ("legacy", "resident"):
            candidate = backend + "_hash"
            for baseline, label in ((backend + "_native", backend + "_hash_vs_native"), ("lmdb", backend + "_hash_vs_lmdb")):
                if candidate not in arms or baseline not in arms:
                    continue
                require(set(arms[candidate]) == set(arms[baseline]), "unpaired aggregate process observations")
                ratios, deltas, adverse = [], [], []
                for process in sorted(arms[candidate]):
                    b, c = arms[baseline][process], arms[candidate][process]
                    ratios.append(b / c if b > 0 and c > 0 else None)
                    deltas.append((c / b - 1) * 100 if b > 0 else None)
                    adverse.append(c > b * 1.05)
                defined = [r for r in ratios if r is not None]
                row["comparisons"][label] = {"baseline": baseline, "candidate": candidate,
                    "cost_speedup_median": MEDIAN(defined) if defined else None,
                    "cost_speedup_min": min(defined) if defined else None,
                    "cost_speedup_max": max(defined) if defined else None,
                    "process_speedups": ratios, "process_cost_change_pct": deltas,
                    "adverse_over_5_percent_any_process": any(adverse),
                    "adverse_over_5_percent_all_processes": all(adverse),
                    "material_reduction_all_processes": len(defined) == len(ratios) and min(defined) >= 1.10}
        output.append(row)
    return output


def number(row, arm):
    value = row["arms"].get(arm, {}).get("median")
    if value is None:
        return "N/A"
    scale = 1e6 if row["family"] == "lifecycle" else 1e3 if row["family"] == "write" else 1024 if row["family"] == "memory" else (1 << 20) if row["family"] in ("storage", "index") else 1
    return f"{value / scale:,.3f}"


def ratio(row, label):
    c = row["comparisons"].get(label)
    if not c or c["cost_speedup_median"] is None:
        return "N/A"
    return f"{c['cost_speedup_median']:.3g}x [{c['cost_speedup_min']:.3g},{c['cost_speedup_max']:.3g}]"


def adverse(row):
    found = []
    for backend in ("legacy", "resident"):
        c = row["comparisons"].get(backend + "_hash_vs_native")
        if c and c["adverse_over_5_percent_any_process"]:
            found.append(backend + (" (all)" if c["adverse_over_5_percent_all_processes"] else " (some)"))
    return ", ".join(found) or "None observed"


def markdown_table(rows, with_ratios=True):
    text = ["| Keys | Key type | Phase / operation | Legacy native | Legacy hash | Resident native | Resident hash | LMDB |" +
            (" Legacy hash/native | Resident hash/native | Legacy hash/LMDB | Resident hash/LMDB | >5% hash cost increase vs native |" if with_ratios else ""),
            "|---:|---|---|---:|---:|---:|---:|---:|" + ("---|---|---|---|---|" if with_ratios else "")]
    for row in rows:
        label = " / ".join(x for x in (row["phase"], row["operation"]) if x)
        parts = [str(row["keys"]), row["key_kind"], label] + [number(row, arm) for arm in ARMS]
        if with_ratios:
            parts += [ratio(row, label) for label in ("legacy_hash_vs_native", "resident_hash_vs_native", "legacy_hash_vs_lmdb", "resident_hash_vs_lmdb")]
            parts.append(adverse(row))
        text.append("| " + " | ".join(parts) + " |")
    return text


def render(output, document):
    rows = aggregate(document)
    summary = {"schema": 1, "complete": True, "source_manifest_sha256": document["source_manifest_sha256"],
               "binary_sha256": document["binary_sha256"], "configuration": document["configuration"],
               "method": "median of each process's samples; paired ratios across fresh rounds; min/max are observations, not confidence intervals",
               "ratio_direction": "baseline cost/candidate cost; above1 is lower candidate cost",
               "rows": rows,
               "adverse_hash_vs_native_rows": [{k: r[k] for k in ("family", "keys", "key_kind", "phase", "operation", "unit", "comparisons")}
                    for r in rows if adverse(r) != "None observed"]}
    write_json(output / "summary.json", summary)
    flat = []
    for row in rows:
        record = {k: row[k] for k in ("family", "keys", "key_kind", "phase", "operation", "unit")}
        for arm in ARMS:
            for name in ("median", "minimum", "maximum"):
                record[arm + "_" + name] = row["arms"].get(arm, {}).get(name)
        for label in ("legacy_hash_vs_native", "resident_hash_vs_native", "legacy_hash_vs_lmdb", "resident_hash_vs_lmdb"):
            for name in ("cost_speedup_median", "cost_speedup_min", "cost_speedup_max", "adverse_over_5_percent_any_process", "adverse_over_5_percent_all_processes"):
                record[label + "_" + name] = row["comparisons"].get(label, {}).get(name)
        flat.append(record)
    with (output / "FULL_TABLE.csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(flat[0]))
        writer.writeheader()
        writer.writerows(flat)
    config, host = document["configuration"], document["machine"]
    text = ["# Production lookup strategy: complete direct comparison", "",
            f"{len(document['runs'])} fresh processes; {config['rounds']} rounds × {len(config['sizes'])} sizes × two key types × five arms. {config['samples']} samples per read/write case; {config['queries']:,} point queries and up to1,024 scan queries per workload. Seeds {config['seed']} through {config['seed'] + config['rounds'] - 1}.", "",
            f"Machine: {host['cpu_model']}; {host['architecture']}; Linux {host['kernel']}; CPU {config['cpu']}. Shared host; no exclusive CPU reservation. Source manifest SHA-256 `{document['source_manifest_sha256']}`; executable SHA-256 `{document['binary_sha256']}`. Pinned native reference: {document['lmdb_version']}.", "",
            "All five arms execute sequentially in fresh processes with reversing/rotating group order. Native uses the original prepared KvRead and constructs no hash. Hash uses the completed production API with borrowed keys/values and exact owned slot capacity accounting. LMDB uses only NOTLS (0x200000), normal locking and synchronized durable commits. A held old reader exists in native, hash and LMDB pinned-write phases; hash additionally retains its side index. All initial, old, new and reopened values and forward/reverse ranges are checked against the same deterministic byte oracle. Every corresponding timed checksum and operation count is checked across all five arms.", "",
            "Values are medians of within-process samples, then medians across fresh processes. Each ratio is baseline cost / hash cost; above1 is a lower hash cost. Brackets show the minimum and maximum paired process ratio, not confidence intervals. A >5% increase in any process is retained as an adverse observation, including construction, memory, retention and lifecycle costs; it is not automatically a statistically established regression. 'all' means every process exceeded5%. Missing or undefined zero-cost ratios are N/A.", "",
            "## Read throughput costs", "",
            "Nanoseconds per point query or one cursor seek plus up to100/64 complete key/value results. These are batch throughput averages, not individual request p99 latency.", ""]
    text += markdown_table([r for r in rows if r["family"] == "read"])
    text += ["", "## Durable writes in every pin state", "",
             "Microseconds per synchronized transaction. Batch sizes are encoded in operation names; raw files also retain ns/update and exact operation counts. Fixtures are precomputed outside timing. No hash is automatically rebuilt on commit: this table measures writes with the same old snapshot held, plus the optional existing index; the explicit full rebuild is charged below.", ""]
    text += markdown_table([r for r in rows if r["family"] == "write"])
    text += ["", "## Build, drop, snapshot and reopen lifecycle", "",
             "Milliseconds per event. Each event is observed once per fresh process, rather than seven samples. Resident performs an explicitly charged ordinary Collect before retaining the old snapshot in both native and hash arms; Legacy and LMDB use a no-op. The total_snapshot_prepare row sums maintenance_before_pin + begin_snapshot + prepare_lookup for each process before aggregation. Maintenance is never treated as free. The capability does not collect automatically. Resident's common benchmark pool is ceil(keys/24)+8192 KV slots with256MiB metadata admission, providing room for both pinned generations; this is a disclosed benchmark setting, not a change to engine defaults. Rebuild includes acquiring the new snapshot and complete preparation. Final_validation measures the complete final byte verification. Initial loaders use different APIs and are labeled in raw reports; their costs do not establish equal-loader superiority. Reopen uses a warm OS cache. N/A identifies engine-specific events.", ""]
    text += markdown_table([r for r in rows if r["family"] == "lifecycle"])
    text += ["", "## Actual per-process RSS at every checkpoint", "",
             "MiB of Linux VmRSS. All input/query/model storage, mappings and allocator retention remain included. The after_full_validation checkpoint follows full source reads for every arm; after_prepare alone also reflects hash's eager source-page touching. Two-generations checkpoints retain both old and new lookup pairs. Dropping a lookup releases its table; the caller's old transaction continues retaining source pages until separately dropped. Allocators may retain freed storage. All other status counters, including RssAnon/RssFile/VmHWM/VmPTE, are retained in FULL_TABLE.csv and summary.json. VmHWM is cumulative, not isolated index peak usage.", ""]
    text += markdown_table([r for r in rows if r["family"] == "memory" and r["operation"] == "vmrss_kib"])
    text += ["", "## Exact index slots and file space", "",
             "MiB. Index values are actual owned slot-vector capacity bytes, rather than the previous prototype's HashMap estimate. Keys/values remain borrowed; source mappings, allocator bookkeeping and all retained snapshot pages are excluded from this slot count. File sizes include the complete retained engine directory after reopening validation.", ""]
    text += markdown_table([r for r in rows if r["family"] in ("index", "storage")])
    adverse_rows = [r for r in rows if adverse(r) != "None observed"]
    text += ["", "## Adverse observations are retained", "",
             f"{len(adverse_rows)} rows have at least one >5% hash cost increase against the matching native arm, across all timing/memory/storage counters. Each appears in the complete CSV and typed summary, including cases that improve at other sizes. Read/write/lifecycle rows and main RSS rows are shown above; secondary memory counters are in the CSV. No universal no-regression claim follows from this campaign.", "",
             "## Scope and remaining qualification", "",
             "This is a synthetic bytewise fixed16/hostname KV benchmark with32-byte values and one read thread. It does not benchmark custom comparators, duplicate/prefix lookups, large values, controlled memory pressure, larger-than-RAM data, cold caches, concurrent request throughput, power-loss recovery or every production IPDon journey. Ballast, if configured, is a touched process allocation; it does not establish OS memory pressure. The production API deliberately rejects unsupported hash sources and preserves native ordered fallback. A separate frozen baseline/candidate full engine suite is required to assess unchanged/default paths.", "",
             "## Evidence and reproduction", "",
             "results.json retains every full raw report, exact command, source hashes, executable hash, LMDB build information and host observations. The raw directory retains successful and failed attempts; failures are never silently replaced. summary.json has typed rows and paired process values. FULL_TABLE.csv includes every metric and adverse flag. Existing public performance reports are untouched.", "", "```sh",
             f"python3 benchmark/lookup-strategies/direct_campaign.py --source FROZEN_CANDIDATE --output NEW_OUTPUT --cpu {config['cpu']} \\",
             f"  --rounds {config['rounds']} --sizes {','.join(map(str, config['sizes']))} --queries {config['queries']} --samples {config['samples']} \\",
             f"  --write-transactions {config['write_transactions']} --seed {config['seed']} --ballast-mib {config['ballast_mib']}",
             "```", "", "Use --build-only to stop before measurements; --resume continues the exact retained configuration and hashes.", ""]
    (output / "FULL_TABLE.md").write_text("\n".join(text))


def main():
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cpu", type=int, default=12)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--sizes", default="8192,100000,1000000")
    parser.add_argument("--queries", type=int, default=65536)
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--write-transactions", type=int, default=4)
    parser.add_argument("--seed", type=int, default=73)
    parser.add_argument("--ballast-mib", type=int, default=0)
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--render-only", action="store_true")
    args = parser.parse_args()
    if args.render_only:
        document = json.loads((args.output / "results.json").read_text())
        require(document.get("complete") is True, "cannot render partial evidence")
        for run in document["runs"]:
            require(sha(args.output / run["raw_file"]) == run["raw_sha256"], "raw evidence changed")
            validate_report(run["report"], run, document["configuration"])
        validate_cross_arm(document["runs"])
        render(args.output, document)
    else:
        execute(args)


def self_test():
    """Small negative controls; these never execute a measured process."""
    import copy
    import tempfile
    import unittest

    class EvidenceTests(unittest.TestCase):
        def test_relaxed_lmdb_flags_cannot_be_accepted(self):
            validate_lmdb_flags({"flags": NOTLS, "normal_sync": True})
            for flags in (0, NOTLS | 0x10000, NOTLS | 0x40000):
                with self.assertRaisesRegex(ValueError, "NOTLS only"):
                    validate_lmdb_flags({"flags": flags, "normal_sync": True})
            with self.assertRaisesRegex(ValueError, "normal durable"):
                validate_lmdb_flags({"flags": NOTLS, "normal_sync": False})

        def test_schedule_keeps_all_five_adjacent_and_rotates_order(self):
            jobs = schedule([8192, 100000, 1000000], 3, 73)
            self.assertEqual(len(jobs), 90)
            self.assertEqual(len({j["id"] for j in jobs}), 90)
            for i in range(0, len(jobs), 5):
                group = jobs[i:i + 5]
                self.assertEqual({j["arm"] for j in group}, set(ARMS))
                self.assertEqual(len({(j["size"], j["key_kind"], j["round"], j["seed"]) for j in group}), 1)
            self.assertNotEqual([j["arm"] for j in jobs[:5]], [j["arm"] for j in jobs[30:35]])

        def test_cross_arm_checksum_and_query_count_changes_fail(self):
            row = {"phase": "read", "workload": "hit", "sample": 0, "operations": 4, "checksum": 71}
            first = {"round": 0, "size": 4, "key_kind": "fixed16", "arm": "legacy_native",
                     "report": {"samples": [row], "lmdb_version": "pinned", "lmdb_build_info": "same"}}
            second = copy.deepcopy(first)
            second["arm"] = "legacy_hash"
            validate_cross_arm([first, second])
            for field in ("checksum", "operations", "sample"):
                bad = copy.deepcopy(second)
                bad["report"]["samples"][0][field] += 1
                with self.assertRaisesRegex(ValueError, "checksum/query/operation"):
                    validate_cross_arm([first, bad])

        def test_adverse_rows_and_zero_cost_allocations_survive_rendering(self):
            config = {"sizes": [2], "rounds": 3, "seed": 73, "queries": 4, "samples": 1,
                      "cpu": 12, "write_transactions": 1, "ballast_mib": 0}
            runs = []
            for job in schedule(config["sizes"], config["rounds"], config["seed"]):
                is_hash = job["arm"].endswith("_hash")
                cost = 200 if is_hash else 100
                report = {"samples": [{"phase": "read", "workload": "hit", "sample": 0,
                                       "operations": 4, "ns_per_op": cost}],
                          "lifecycle": [{"phase": "maintenance_before_pin", "elapsed_ns": 20, "details": {}},
                                        {"phase": "begin_snapshot", "elapsed_ns": 10, "details": {}},
                                        {"phase": "prepare_lookup", "elapsed_ns": cost,
                                         "details": {"index_bytes": 128 if is_hash else 0}}],
                          "memory": [{"phase": "after_full_validation", "vmrss_kib": cost}],
                          "files": {"apparent_bytes": 100, "allocated_bytes": 100}}
                runs.append(job | {"report": report})
            document = {"complete": True, "runs": runs, "configuration": config,
                        "source_manifest_sha256": "a" * 64, "binary_sha256": "b" * 64,
                        "lmdb_version": "synthetic unit fixture", "machine": {
                            "cpu_model": "fixture", "architecture": "fixture", "kernel": "fixture"}}
            rows = aggregate(document)
            read = next(r for r in rows if r["family"] == "read")
            comparison = read["comparisons"]["legacy_hash_vs_native"]
            self.assertEqual(comparison["cost_speedup_median"], 0.5)
            self.assertTrue(comparison["adverse_over_5_percent_all_processes"])
            preparation = next(r for r in rows if r["operation"] == "total_snapshot_prepare")
            self.assertEqual(preparation["arms"]["legacy_native"]["median"], 130)
            self.assertEqual(preparation["arms"]["legacy_hash"]["median"], 230)
            self.assertEqual(preparation["derived_components"], ["maintenance_before_pin", "begin_snapshot", "prepare_lookup"])
            allocation = next(r for r in rows if r["family"] == "index")
            self.assertIsNone(allocation["comparisons"]["legacy_hash_vs_native"]["cost_speedup_median"])
            self.assertTrue(allocation["comparisons"]["legacy_hash_vs_native"]["adverse_over_5_percent_any_process"])
            with tempfile.TemporaryDirectory() as directory:
                output = Path(directory)
                render(output, document)
                self.assertIn("0.5x", (output / "FULL_TABLE.md").read_text())
                self.assertIn("200", (output / "FULL_TABLE.csv").read_text())
                self.assertTrue(json.loads((output / "summary.json").read_text())["adverse_hash_vs_native_rows"])
            missing = copy.deepcopy(document)
            missing["runs"].pop()
            with self.assertRaisesRegex(ValueError, "missing planned processes"):
                aggregate(missing)

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(EvidenceTests)
    require(unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful(), "negative controls failed")


if __name__ == "__main__":
    main()
